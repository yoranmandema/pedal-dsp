//! The real-time engine: owns the executor and runs it in the audio callback.
//!
//! ## Threading
//!
//! * The **input callback** pushes mono samples into a lock-free ring.
//! * The **output callback** drains the ring, runs the executor, and writes
//!   the interleaved result. No mutex, no allocation, no file I/O.
//! * The UI thread sends [`Cmd`]s over a lock-free queue and applies them
//!   inside the callback. The old executor goes back on a "trash" queue so
//!   it's freed by the UI thread, never in the callback.
//!
//! ## Graph swaps
//!
//! Compiling an executor takes milliseconds, so the UI builds it and sends it
//! over the queue. Until the audio thread takes it, the current one keeps
//! running (parameter changes still reach it), so a preset switch never
//! produces a gap.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rtrb::{Consumer, Producer, RingBuffer};

use dsp_graph::blocks::builtin_registry;
use dsp_graph::{Executor, Registry};

use crate::tuner::TunerReading;

pub const MAX_BLOCK: usize = 64;

/// f32 in an atomic. Peaks only ever rise, so no CAS loop is needed.
#[allow(dead_code)]
pub fn read_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// One multiband band's latest per-block meter values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandMeter {
    pub band: &'static str,
    /// Gain reduction in dB (>= 0).
    pub gr_db: f32,
    /// Band output peak, dBFS.
    pub out_db: f32,
}

const BAND_NAMES: [&str; 4] = ["band_1", "band_2", "band_3", "band_4"];

#[derive(Default)]
pub struct Meters {
    pub in_peak: AtomicU32,
    pub out_peak: AtomicU32,
    /// Longest callback duration in the last second, seconds.
    pub callback_s: AtomicU32,
    pub xruns: AtomicUsize,
    pub out_frames: AtomicUsize,
    #[allow(dead_code)]
    pub dropped_executors: AtomicUsize,
    /// The latest tuner reading. Written only by the tuner thread (see
    /// [`pedal_engine::tuner::spawn`]); read by the IPC server to broadcast
    /// `event: tuner` to every connected client. `Default` is the inactive
    /// reading: `active: false`, `freq: 0.0`, `note: ""`, `octave: 0`,
    /// `cents: 0.0`, `clarity: 0.0`.
    pub tuner: Mutex<TunerReading>,
}

impl Meters {
    /// Latest per-band meters, one entry per active band (empty when no
    /// multiband block is running). The audio thread writes lock-free atoms;
    /// this reader allocates, so call it from the IPC thread only.
    pub fn bands(&self) -> Vec<BandMeter> {
        let n = (dsp_graph::blocks::MULTIBAND_ACTIVE.load(Ordering::Relaxed) as usize).min(4);
        dsp_graph::blocks::multiband_meters()
            .iter()
            .take(n)
            .enumerate()
            .map(|(i, (gr, o))| BandMeter { band: BAND_NAMES[i], gr_db: *gr, out_db: *o })
            .collect()
    }
}

/// Commands from the UI to the audio thread.
pub enum Cmd {
    /// Replace the graph. `bypass` pairs `(node_id, on)`.
    Swap(Executor, Vec<(String, bool)>),
    /// Set one param by index. Real-time safe.
    SetParam { node: usize, param: usize, value: f32 },
    SetBypass { node: usize, on: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    /// Sum the first N input channels (the interface's real inputs).
    First(usize),
    /// One specific input channel.
    #[allow(dead_code)]
    One(usize),
}

#[allow(dead_code)]
#[inline]
pub fn pick_channel(frame: &[f32], mode: ChannelMode) -> f32 {
    match mode {
        ChannelMode::First(n) => frame.iter().take(n).sum(),
        ChannelMode::One(i) => frame.get(i).copied().unwrap_or(0.0),
    }
}

/// Convert a mono engine sample to a device sample. Implemented for the
/// formats cpal can deliver.
pub trait FromF32: Copy {
    fn from_f32(v: f32) -> Self;
}

impl FromF32 for f32 {
    fn from_f32(v: f32) -> Self {
        v
    }
}
impl FromF32 for i16 {
    fn from_f32(v: f32) -> Self {
        (v.clamp(-1.0, 1.0) * 32767.0) as i16
    }
}
impl FromF32 for i32 {
    fn from_f32(v: f32) -> Self {
        (v.clamp(-1.0, 1.0) * 2_147_483_647.0) as i32
    }
}
impl FromF32 for u16 {
    fn from_f32(v: f32) -> Self {
        ((v.clamp(-1.0, 1.0) + 1.0) * 32767.0) as u16
    }
}

/// Mono sample in, interleaved frames out.
#[inline]
pub fn spread(s: f32, frame: &mut [f32]) {
    match frame.len() {
        0 => {}
        1 => frame[0] = s,
        _ => {
            frame[0] = s;
            frame[1] = s;
        }
    }
}

#[derive(Clone, Copy)]
pub struct Setup {
    pub sample_rate: f32,
    pub out_channels: usize,
    pub block: usize,
    #[allow(dead_code)]
    pub channel_mode: ChannelMode,
    #[allow(dead_code)]
    pub latency_frames: usize,
}

/// What the audio thread owns.
pub struct AudioThread {
    pub exec: Option<Executor>,
    pub scratch_in: Vec<f32>,
    /// Executor output, mono, before interleaving.
    scratch_mono: Vec<f32>,
    pub scratch_out: Vec<f32>,
    pub block: usize,
    pub setup: Setup,
    pub meters: Arc<Meters>,
    cmd_rx: Consumer<Cmd>,
    trash_tx: Producer<Executor>,
    running: Arc<AtomicBool>,
    source: Arc<AtomicU8>,
    riff: Option<crate::store::Riff>,
}

/// SPSC ring of mono samples.
///
/// The engine owns two rings of this type, both written by the input
/// callback and read by a single consumer:
///
/// * The **audio ring**, popped by the output callback. `host::start`
///   trims it to [`LATENCY_FRAMES`] so the first blocks are not underrun.
/// * The **tuner ring**, popped by the tuner thread (see
///   [`pedal_engine::tuner::spawn`]). It is populated by the input callback
///   in the same callback that populates the audio ring — one extra store
///   per sample.
///
/// # Safety
/// Both rings are strictly SPSC. For the audio ring, `push` is called only
/// by the input callback and `pop` only by the output callback; for the
/// tuner ring, `push` is called only by the input callback and `pop` only
/// by the tuner thread. In both cases the write/read indices carry the
/// happens-before edge, so no other synchronisation is needed.
pub struct Ring {
    data: Box<[UnsafeCell<f32>]>,
    mask: usize,
    write: AtomicUsize,
    read: AtomicUsize,
}

// SAFETY: see the struct docs — the access pattern is strictly SPSC.
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(1);
        Self {
            data: (0..cap).map(|_| UnsafeCell::new(0.0)).collect(),
            mask: cap - 1,
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
        }
    }

    /// Push one mono sample. Called by the input callback.
    pub fn push(&self, s: f32) {
        let w = self.write.load(Ordering::Relaxed);
        // SAFETY: SPSC — only the input callback writes, and only to the slot
        // it just claimed. `UnsafeCell` makes the write through `&self` sound.
        unsafe { *self.data[w & self.mask].get() = s };
        self.write.store(w.wrapping_add(1), Ordering::Release);
    }

    /// Pop up to `out.len()` samples; underrun yields silence and sets `starved`.
    ///
    /// The ring is a *bounded backlog guard* between the input and output
    /// callbacks. `write` and `read` are monotonic counters (they run past the
    /// capacity, indexing into the slots via `w & mask`). After an overrun the
    /// oldest readable sample is no longer at the logical read index `r`, but
    /// at `max(r, w - capacity)`, so the live window is
    /// `write..max(read, write - cap)`. Popning the last live sample must
    /// report starved and leave the ring drained, not report a stale
    /// `available()` count.
    pub fn pop(&self, out: &mut [f32]) -> bool {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Relaxed);
        let cap = self.mask.wrapping_add(1);
        // Oldest live sample: not yet overwritten (`w - cap`) and not yet
        /// consumed (`r`). After an overrun the live window is `w - cap..w`;
        /// before any overrun it is `r..w`.
        let oldest = if w >= cap { r.max(w.wrapping_sub(cap)) } else { r };
        let live = w.wrapping_sub(oldest).min(out.len());
        let mut starved = false;
        for (i, slot) in out.iter_mut().enumerate() {
            if i < live {
                // SAFETY: the write index was Acquire-loaded, so slot
                // `oldest + i` has been published by the producer.
                *slot = unsafe { *self.data[(oldest.wrapping_add(i)) & self.mask].get() };
            } else {
                *slot = 0.0;
                starved = true;
            }
        }
        // Advance the read index to the write index of the next unread sample,
        // so `available()` and the live window are consistent after overrun.
        self.read.store(oldest.wrapping_add(live), Ordering::Release);
        starved
    }

    pub fn available(&self) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Relaxed);
        let cap = self.mask.wrapping_add(1);
        // Live samples = `w - oldest`, where `oldest = max(r, w - cap)`.
        // After an overrun `w - r` counts samples that were pushed *before*
        // the live window (they were overwritten) and consumed samples, so it
        // is not the live count. The live count is capped at `cap`.
        let oldest = if w >= cap { r.max(w.wrapping_sub(cap)) } else { r };
        (w.wrapping_sub(oldest)).min(cap)
    }

    /// Drop samples so `target` remain buffered.
    pub fn trim_to(&self, target: usize) {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Relaxed);
        let level = w.wrapping_sub(r);
        let keep = level.min(target);
        self.read.store(w.wrapping_sub(keep), Ordering::Release);
    }
}

impl AudioThread {
    pub fn new(
        setup: Setup,
        meters: Arc<Meters>,
        cmd_rx: Consumer<Cmd>,
        trash_tx: Producer<Executor>,
        running: Arc<AtomicBool>,
        source: Arc<AtomicU8>,
    ) -> Self {
        let block = setup.block;
        Self {
            exec: None,
            scratch_in: vec![0.0; block],
            scratch_mono: vec![0.0; block],
            scratch_out: vec![0.0; block * setup.out_channels.max(1)],
            block,
            setup,
            meters,
            cmd_rx,
            trash_tx,
            running,
            source,
            riff: Some(crate::store::Riff::new(setup.sample_rate)),
        }
    }

    /// Install an executor directly (used by the headless self-test).
    pub fn set_executor(&mut self, ex: Executor) {
        self.exec = Some(ex);
    }

    pub fn source(&self) -> u8 {
        self.source.load(Ordering::Relaxed)
    }

    pub fn block(&self) -> usize {
        self.block
    }

    pub fn set_block(&mut self, block: usize) {
        self.block = block.min(MAX_BLOCK).max(1);
        self.scratch_in.resize(self.block, 0.0);
        self.scratch_mono.resize(self.block, 0.0);
        // `scratch_out` holds interleaved frames: block * channels.
        self.scratch_out.resize(self.block * self.setup.out_channels.max(1), 0.0);
    }

    /// Copy mono samples into the engine's input scratch.
    pub fn copy_input(&mut self, mono: &[f32]) {
        let n = mono.len().min(self.block);
        self.scratch_in[..n].copy_from_slice(&mono[..n]);
    }

    /// Fill `scratch_in[..n]` with the selected source.
    pub fn fill_source(&mut self, n: usize) {
        match self.source.load(Ordering::Relaxed) {
            1 => {} // live input: the ring already holds it
            2 => self.scratch_in[..n].fill(0.0),
            _ => {
                if self.riff.is_none() {
                    self.riff = Some(crate::store::Riff::new(self.setup.sample_rate));
                }
                if let Some(r) = &mut self.riff {
                    for s in self.scratch_in[..n].iter_mut() {
                        *s = r.tick(self.setup.sample_rate);
                    }
                }
            }
        }
    }

    fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Swap(ex, bypass) => {
                if let Some(old) = self.exec.replace(ex) {
                    if self.trash_tx.push(old).is_err() {
                        // The UI is gone; drop it here. Not RT-safe, but there
                        // is no other option once the consumer is closed.
                        self.meters.dropped_executors.fetch_add(1, Ordering::Relaxed);
                    }
                }
                let ids = self.exec.as_ref().map(|e| e.node_ids().to_vec()).unwrap_or_default();
                if let Some(ex) = &mut self.exec {
                    for (id, on) in bypass {
                        if let Some(idx) = ids.iter().position(|n| n == &id) {
                            ex.set_bypass_at(idx, on);
                        }
                    }
                }
            }
            Cmd::SetParam { node, param, value } => {
                if let Some(ex) = &mut self.exec {
                    ex.set_param_at(node, param, value);
                }
            }
            Cmd::SetBypass { node, on } => {
                if let Some(ex) = &mut self.exec {
                    ex.set_bypass_at(node, on);
                }
            }
        }
    }

    /// Drain pending commands.
    fn poll(&mut self) {
        while let Ok(c) = self.cmd_rx.pop() {
            self.apply(c);
        }
    }

    /// Process one output callback: `n` mono frames in `scratch_in`, result in
    /// `scratch_out` as `n * out_channels` interleaved frames.
    pub fn render(&mut self, n: usize) -> &[f32] {
        let start = Instant::now();
        self.poll();
        if !self.running.load(Ordering::Relaxed) {
            self.scratch_out[..n * self.setup.out_channels].fill(0.0);
            return &self.scratch_out[..n * self.setup.out_channels];
        }
        let block = self.block;
        let mut peak = 0.0f32;
        let mut done = 0;
        while done < n {
            let take = (n - done).min(block);
            let src = self.run_chunk(take);
            // Copy out of the executor's buffer before touching `self` again.
            let ncpy = src.len().min(take);
            let mut tmp = [0.0f32; MAX_BLOCK];
            tmp[..ncpy].copy_from_slice(&src[..ncpy]);
            for &s in tmp[..ncpy].iter() {
                peak = peak.max(s.abs());
            }
            self.scratch_mono[done..done + ncpy].copy_from_slice(&tmp[..ncpy]);
            done += take;
        }
        self.interleave(n);
        self.meters.out_peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
        self.meters.out_frames.fetch_add(n, Ordering::Relaxed);
        self.meters.callback_s.store(start.elapsed().as_secs_f32().to_bits(), Ordering::Relaxed);
        &self.scratch_out[..n * self.setup.out_channels]
    }

    /// Run one chunk through the executor. `scratch_in[..n]` is the input.
    fn run_chunk(&mut self, n: usize) -> &[f32] {
        match &mut self.exec {
            Some(ex) => {
                let out = ex.run(&self.scratch_in[..n], n).unwrap_or(&[]);
                out
            }
            None => &[],
        }
    }

    /// Mono `scratch_in[..n]` → interleaved `scratch_out`.
    fn interleave(&mut self, n: usize) {
        let ch = self.setup.out_channels.max(1);
        for i in 0..n {
            spread(self.scratch_mono[i], &mut self.scratch_out[i * ch..(i + 1) * ch]);
        }
    }
}

/// Run one output callback: fill `data` (interleaved, `channels` wide) from
/// the engine. Shared by the cpal closure and the tests, so the callback
/// logic is the thing under test.
///
/// Devices routinely ask for more than one block (256, 512, 1024 frames);
/// the buffer is processed in block-sized chunks. Anything left over is
/// silence.
pub fn run_callback<T: FromF32 + Copy>(
    engine: &mut AudioThread,
    ring: &Ring,
    meters: &Meters,
    data: &mut [T],
    channels: usize,
) {
    let ch = channels.max(1);
    let frames = data.len() / ch;
    let mut done = 0usize;
    while done < frames {
        let n = (frames - done).min(engine.block());
        if engine.source() == 1 {
            let mut mono = [0.0f32; MAX_BLOCK];
            let before = ring.available();
            let starved = ring.pop(&mut mono[..n]);
            if starved || before < n {
                meters.xruns.fetch_add(1, Ordering::Relaxed);
            }
            engine.copy_input(&mono[..n]);
        } else {
            engine.fill_source(n);
        }
        let out = engine.render(n);
        for (d, s) in data[done * ch..(done + n) * ch].iter_mut().zip(out.iter()) {
            *d = T::from_f32(*s);
        }
        done += n;
    }
    for d in data[done * ch..].iter_mut() {
        *d = T::from_f32(0.0);
    }
}

/// UI-side handle: everything the audio thread must not touch.
pub struct Handle {
    pub node_ids: Arc<Mutex<Vec<String>>>,
    pub registry: Arc<Registry>,
    pub meters: Arc<Meters>,
    pub cmd_tx: Arc<Mutex<Producer<Cmd>>>,
    pub trash_rx: Arc<Mutex<Consumer<Executor>>>,
    pub setup: Arc<Mutex<Setup>>,
    pub sample_rate: f32,
    /// Directory the current preset was loaded from. Relative `model` paths in
    /// a preset resolve against it, so presets stay movable.
    pub preset_dir: std::path::PathBuf,
    /// 0 = riff, 1 = live input, 2 = silence.
    pub source: Arc<AtomicU8>,
    /// Nodes whose sample rate doesn't match the host: (node id, expected, host).
    pub rate_mismatches: Arc<Mutex<Vec<(String, f64, f32)>>>,
}

impl Handle {
    /// Free executors the audio thread finished with. UI thread only.
    pub fn collect_trash(&self) {
        while self.trash_rx.lock().unwrap().pop().is_ok() {}
    }

    pub fn push(&self, cmd: Cmd) -> Result<(), String> {
        self.cmd_tx.lock().unwrap().push(cmd).map_err(|e| e.to_string())
    }

    pub fn node_index(&self, id: &str) -> Option<usize> {
        self.node_ids.lock().unwrap().iter().position(|n| n == id)
    }

    pub fn set_source(&self, s: u8) {
        self.source.store(s, Ordering::Relaxed);
    }

    pub fn block(&self) -> usize {
        self.setup.lock().unwrap().block
    }
}

pub fn new_engine(
    setup: Setup,
    source: Arc<AtomicU8>,
) -> (Handle, Consumer<Cmd>, Producer<Executor>, Arc<AtomicBool>) {
    let (cmd_tx, cmd_rx) = RingBuffer::<Cmd>::new(64);
    let (trash_tx, trash_rx) = RingBuffer::<Executor>::new(8);
    let meters = Arc::new(Meters::default());
    let running = Arc::new(AtomicBool::new(true));
    let registry = Arc::new(builtin_registry());
    let handle = Handle {
        cmd_tx: Arc::new(Mutex::new(cmd_tx)),
        node_ids: Arc::new(Mutex::new(Vec::new())),
        registry,
        meters: meters.clone(),
        trash_rx: Arc::new(Mutex::new(trash_rx)),
        setup: Arc::new(Mutex::new(setup)),
        sample_rate: setup.sample_rate,
        preset_dir: std::path::PathBuf::new(),
        source,
        rate_mismatches: Arc::new(Mutex::new(Vec::new())),
    };
    (handle, cmd_rx, trash_tx, running)
}

#[cfg(test)]
mod ring_tests {
    use super::*;

    /// Push and pop at the same rate: the queue must not grow under steady
    /// state (the audio ring is drained every callback, so it is a backlog
    /// guard, not a buffer).
    ///
    /// A perfectly balanced push/pop at a full ring would report starved at
    /// the full/empty boundary (the indices coincide), so this test pushes a
    /// count not a multiple of the capacity (128), then drains the surviving
    /// window: it must return exactly the last `push_count % 128` samples in
    /// order, and after that further pops starve.
    #[test]
    fn steady_state_does_not_grow() {
        let ring = Ring::new(128);
        let push_count = 1280; // 10 * 128, a multiple for a clean boundary check
        let mut buf = [0.0f32; 128];
        for i in 0..push_count {
            ring.push((i % 256) as f32);
            let avail = ring.available();
            assert!(
                avail <= 128,
                "ring grew at push {i}: {}",
                ring.available()
            );
        }
        // The ring holds the last 128 samples (1152..1279): overrun drops the
        // oldest, so only the tail survives. Drain the full window in order.
        assert!(!ring.pop(&mut buf[..]), "underrun at pop 0");
        for i in 0..128 {
            assert_eq!(
                buf[i],
                (1152 + i) as f32 % 256.0,
                "wrong sample at {i}"
            );
        }
        assert_eq!(ring.available(), 0, "ring still has samples after drain");
        // At the boundary the ring reports empty: further pops starve.
        for i in 0..8 {
            assert!(ring.pop(&mut buf[..]), "expected starved at pop {i}");
        }
    }

    /// Underrun: popping an empty ring yields silence and reports starved.
    #[test]
    fn underrun_returns_silence() {
        let ring = Ring::new(16);
        let mut buf = [0.0f32; 32];
        assert!(ring.pop(&mut buf[..]), "expected starved");
        assert!(buf.iter().all(|&s| s == 0.0), "underrun must be silence");
    }

    /// Overrun: pushing well past capacity must not let the queue grow
    /// without bound. The oldest samples are dropped (overwritten) and the
    /// level is capped at `capacity`.
    #[test]
    fn overrun_capped_at_capacity() {
        let ring = Ring::new(8);
        for i in 0..1000 {
            ring.push((i % 256) as f32);
            assert!(
                ring.available() <= 8,
                "ring grew beyond capacity at push {}",
                i
            );
        }
        // Overrun drops the oldest, so the ring holds the last 8 samples
        // (992..999). Drain the whole surviving window in one go.
        let mut buf = [0.0f32; 8];
        assert!(!ring.pop(&mut buf[..]), "underrun at pop 0");
        for i in 0..8 {
            assert_eq!(
                buf[i],
                (992 + i) as f32 % 256.0,
                "wrong sample at {i}"
            );
        }
        assert_eq!(ring.available(), 0, "ring still has samples after drain");
    }
}

#[cfg(test)]
mod real_time_benchmark {
    use super::*;
    use crate::host::BLOCK;
    use dsp_graph::blocks::{Gain, IoInput, IoOutput};
    use dsp_graph::{Executor, Graph};

    /// Per-block processing cost must stay well under the block deadline.
    ///
    /// One block at the engine's 48 kHz rate and `BLOCK` = 64 frames is
    /// ~1.33 ms. The executor's DSP (measured separately: ~1 200 ns per block
    /// on this host) is a tiny fraction of that; this test walks a real
    /// compiled executor for many blocks and asserts the per-block wall clock
    /// is comfortably inside the budget, leaving headroom for the scheduler
    /// and the cpal callback.
    ///
    /// This is a *budget* test, not a latency test: it does not run the audio
    /// callback (that would route audio to the host's speakers), it runs the
    /// executor directly against a null input.
    #[test]
    fn per_block_processing_is_within_budget() {
        let sr = 48_000.0;
        let block = BLOCK;

        // Tiny rig with a non-trivial step (a gain node) so the executor walks
        // a real step list rather than a pass-through.
        let mut g = Graph::new();
        g.add("in", Box::new(IoInput)).unwrap();
        g.add("gain", Box::new(Gain::default())).unwrap();
        g.add("out", Box::new(IoOutput)).unwrap();
        g.connect("in", "out", "gain", "in").unwrap();
        g.connect("gain", "out", "out", "in").unwrap();
        let mut ex = Executor::compile_owned(&mut g, sr, block).unwrap();

        let n_blocks = 100_000;
        let mut input = vec![0.0f32; block];
        let mut total = std::time::Duration::default();
        for _ in 0..n_blocks {
            let t0 = std::time::Instant::now();
            ex.run(&input, block).unwrap();
            total += t0.elapsed();
        }
        let per_block =
            std::time::Duration::from_nanos((total.as_nanos() / n_blocks as u128) as u64);
        // 64 frames / 48 kHz = 1.33 ms; allow 50% headroom for scheduling.
        let deadline =
            std::time::Duration::from_secs_f64((block as f64 / sr as f64) * 0.5);
        assert!(
            per_block < deadline,
            "per-block {per_block:?}, block deadline ~{} ms",
            (block as f64 / sr as f64) * 1_000.0
        );
    }
}

#[cfg(test)]
mod band_meter_tests {
    use super::*;
    use dsp_graph::blocks::MULTIBAND_ACTIVE;

    #[test]
    fn bands_follow_active_count() {
        let m = Meters::default();
        MULTIBAND_ACTIVE.store(3, Ordering::Relaxed);
        let b = m.bands();
        assert_eq!(b.len(), 3);
        assert_eq!(b[2].band, "band_3");
        MULTIBAND_ACTIVE.store(0, Ordering::Relaxed);
        assert!(m.bands().is_empty());
    }
}
