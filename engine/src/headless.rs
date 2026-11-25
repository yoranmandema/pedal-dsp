//! A simulated audio device, so the engine runs with no interface attached.
//!
//! The pedal may boot before an audio interface is present — first power-up on
//! the bench, a headless CI run, a Pi driving only a display.
//! `pedal-engine --headless` replaces [`crate::host`]'s device pickup and
//! streams with this module: the same real [`run_callback`] runs on a paced
//! simulated device, the input is a generated test signal, and the output is
//! **discarded** — nothing is ever sent to a speaker. The IPC server is
//! unchanged, so the UI behaves exactly as it does against a real interface.
//!
//! It is also the measurement vehicle for US-5.4: the callback time is real, so
//! [`Stats`] over a wall-clock window gives the engine's true CPU cost on the
//! target, and `--bench <seconds>` prints it.
//!
//! The pace is a sleep-plus-spin: `std::thread::sleep` overshoots by an order of
//! magnitude on the development host (5 ms → ~21 ms), and a pure spin burns a
//! whole core on the Pi. Sleeping all but the last 200 µs and spinning the
//! remainder keeps the deadline without hogging the CPU. If the sleep still
//! overshoots, the deadline is dropped and resynchronised rather than running
//! flat out — the same thing a late device interrupt does.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dsp_graph::Executor;
use rtrb::{Consumer, Producer};

use crate::audio::{
    read_f32, run_callback, AudioThread, Cmd, Meters, Ring, Setup,
};
use crate::host::{BLOCK, LATENCY_FRAMES, RING_CAPACITY, TUNER_RING_CAPACITY};

/// Simulated device callback, frames. Wider than one block, like a real device.
pub const CB: usize = 256;
/// The test signal the simulated input callback generates.
const TONE_HZ: f32 = 220.0;
/// How much of the period to spin rather than sleep.
const SPIN_TAIL: Duration = Duration::from_micros(200);

/// Real-time pace for the simulated device.
struct Pacer {
    next: Instant,
    period: Duration,
}

impl Pacer {
    fn new(period: Duration) -> Self {
        Self { next: Instant::now() + period, period }
    }

    fn wait(&mut self) {
        self.next += self.period;
        let now = Instant::now();
        if self.next <= now {
            // The callback overran the period: resync rather than run flat out.
            self.next = now + self.period;
            return;
        }
        let budget = self.next - now;
        if budget > SPIN_TAIL {
            std::thread::sleep(budget - SPIN_TAIL);
        }
        while Instant::now() < self.next {
            std::hint::spin_loop();
        }
    }
}

/// Windowed CPU counters, read by the `--bench` report.
#[derive(Default)]
pub struct Stats {
    /// Callbacks run since the device started.
    pub callbacks: AtomicU64,
    /// Total seconds spent inside those callbacks.
    pub callback_s: AtomicU64,
}

impl Stats {
    fn add(&self, seconds: f32) {
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        let prev = f64::from_bits(self.callback_s.load(Ordering::Relaxed));
        self.callback_s.store((prev + seconds as f64).to_bits(), Ordering::Relaxed);
    }

    /// `(callbacks, total callback seconds)` since the last call.
    pub fn take(&self) -> (u64, f64) {
        let callbacks = self.callbacks.swap(0, Ordering::Relaxed);
        let seconds = f64::from_bits(self.callback_s.swap(0, Ordering::Relaxed));
        (callbacks, seconds)
    }
}

/// A running headless device. Drop it to stop the simulated callbacks.
pub struct Headless {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
    /// Windowed callback counters for the `--bench` report.
    pub stats: Arc<Stats>,
    /// The tuner ring is populated by the input callback (in the same
    /// `source == 1` branch that feeds the audio ring) and read by the
    /// detached tuner thread.
    pub tuner_ring: Arc<Ring>,
}

impl Headless {
    /// Stop the device and wait for its thread.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Run the real audio callback on a simulated device until dropped.
///
/// `source` follows the engine's usual convention — 0 = riff, 1 = live input
/// (the generated tone), 2 = silence — and is read every callback, so the UI can
/// still switch it.
pub fn start(
    setup: Setup,
    cmd_rx: Consumer<Cmd>,
    trash_tx: Producer<Executor>,
    running: Arc<AtomicBool>,
    meters: Arc<Meters>,
    source: Arc<AtomicU8>,
) -> Headless {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let stats = Arc::new(Stats::default());
    let stats_thread = stats.clone();
    let channels = setup.out_channels.max(1);
    let sample_rate = setup.sample_rate;
    let tuner_ring = Arc::new(Ring::new(TUNER_RING_CAPACITY));
    let tuner_ring_for_tuner = tuner_ring.clone();

    let join = std::thread::spawn(move || {
        let mut engine = AudioThread::new(setup, meters.clone(), cmd_rx, trash_tx, running, source.clone());
        engine.set_block(BLOCK);
        let ring = Ring::new(RING_CAPACITY);
        for _ in 0..LATENCY_FRAMES {
            ring.push(0.0);
            tuner_ring_for_tuner.push(0.0);
        }
        let mut pacer = Pacer::new(Duration::from_secs_f32(CB as f32 / sample_rate));
        let mut buf = vec![0.0f32; CB * channels];
        let mut phase = 0.0f32;
        let step = 2.0 * std::f32::consts::PI * TONE_HZ / sample_rate;

        while !stop_thread.load(Ordering::Relaxed) {
            // Input callback: the generated tone, when the source is "live".
            if source.load(Ordering::Relaxed) == 1 {
                for _ in 0..CB {
                    let s = (phase * step).sin() * 0.5;
                    ring.push(s);
                    tuner_ring_for_tuner.push(s);
                    phase += 1.0;
                }
            }
            // Output callback: the real one, into a buffer nobody plays.
            run_callback(&mut engine, &ring, &meters, &mut buf, channels);
            stats_thread.add(read_f32(&meters.callback_s));
            pacer.wait();
        }
    });

    Headless { stop, join: Some(join), stats, tuner_ring }
}
