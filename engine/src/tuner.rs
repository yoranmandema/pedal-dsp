//! Chromatic instrument tuner.
//!
//! Pitch detection on the instrument input, published to the IPC server as
//! `event: tuner` at 20 Hz. No command, no `state` field, no `rev` bump.
//!
//! The detection algorithm is YIN (de Cheveigné–Kawahara), frozen in
//! `docs/design/instrument-tuner.md`.
//!
//! ## Layout
//!
//! * [`analyse`] — one-shot YIN on a window of samples.
//! * [`Detector`] — a streaming wrapper: `push` accumulates, `detect` analyses
//!   the last window and drains it (hop = window).
//! * [`spawn`] — the normal-priority tuner thread, fed from the tuner ring.

use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::audio::Meters;
use crate::audio::Ring;

/// The reference pitch A4, from which the note name and octave are derived.
pub const REFERENCE_HZ: f32 = 440.0;

/// YIN window size. 2048 samples at 48 kHz = 42.7 ms.
pub const WINDOW: usize = 2048;

/// YIN absolute threshold on the normalised difference function.
pub const THRESHOLD: f32 = 0.15;

/// Below this clarity the detector reports "no reliable note".
pub const MIN_CLARITY: f32 = 0.5;

/// Below this RMS level the input is treated as silence.
pub const MIN_RMS: f32 = 0.002;

/// The note names, in semitone order starting from C.
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// A single pitch estimate produced by the tuner.
///
/// `Default` is the inactive reading (no reliable note), which is the correct
/// initial state for a fresh engine.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TunerReading {
    /// `false` = no reliable note (silence, noise, or a note below range).
    pub active: bool,
    /// Frequency in Hz; `0.0` when inactive.
    pub freq: f32,
    /// Note name (`"A"`…`"B"`, `""` when inactive).
    pub note: &'static str,
    /// Octave, `0` when inactive.
    pub octave: i32,
    /// Cents away from the nearest semitone, clamped to ±50; `0.0` when
    /// inactive.
    pub cents: f32,
    /// Clarity in [0, 1]; `0.0` when inactive.
    pub clarity: f32,
}

/// One-shot YIN pitch analysis.
///
/// Returns `None` when the window is silent (`rms < MIN_RMS`), when no dip
/// below `THRESHOLD` is found above `tau_min`, or when the best dip has
/// clarity below `MIN_CLARITY`.
///
/// The detection follows `docs/design/instrument-tuner.md` §2 exactly:
///
/// 1. Gate on level: `rms = sqrt(mean(x²))`. Below `MIN_RMS` → `None`.
/// 2. Difference function `d[tau] = Σ (x[j] − x[j+tau])²` for `tau` in
///    `1..=tau_max`.
/// 3. Cumulative mean normalisation `d'[tau] = d[tau] · tau / Σ_{j=1}^{tau} d[j]`.
/// 4. First dip: the smallest `tau ≥ tau_min` with `d'[tau] < THRESHOLD`,
///    then walk forward while `d'[tau+1] < d'[tau]` to the bottom of that dip.
/// 5. Clarity `1 − d'[tau]`; below `MIN_CLARITY` → `None`.
/// 6. Parabolic interpolation on `d'` (skipped when `tau == tau_max`),
///    `period = tau + shift`, `freq = sr / period`.
/// 7. Convert to note name and octave.
pub fn analyse(samples: &[f32], sample_rate: f32) -> Option<TunerReading> {
    let n = samples.len();
    if n == 0 || n < WINDOW {
        return None;
    }

    // 1. Gate on level.
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
    if rms < MIN_RMS {
        return None;
    }

    // Range of tau: up to ~1400 Hz, down to ~70 Hz, capped at WINDOW/2.
    let tau_min = (sample_rate / 1400.0).floor() as usize;
    let tau_max = (sample_rate / 70.0).ceil().min((WINDOW / 2) as f32) as usize;
    if tau_max < 2 || tau_min >= tau_max {
        return None;
    }

    // 2. Difference function.
    let mut d = vec![0.0f32; tau_max + 1];
    for tau in 1..=tau_max {
        d[tau] = samples
            .iter()
            .enumerate()
            .take(WINDOW - tau)
            .map(|(j, &s)| {
                let diff = s - samples[j + tau];
                diff * diff
            })
            .sum();
    }

    // 3. Cumulative mean normalisation.
    let mut d_norm = vec![0.0f32; tau_max + 1];
    d_norm[0] = 1.0;
    let mut cum = 0.0f32;
    for tau in 1..=tau_max {
        cum += d[tau];
        d_norm[tau] = if cum > 0.0 {
            d[tau] * tau as f32 / cum
        } else {
            1.0
        };
    }

    // 4. First dip.
    let mut tau = tau_min;
    while tau <= tau_max && d_norm[tau] >= THRESHOLD {
        tau += 1;
    }
    if tau > tau_max {
        return None;
    }
    while tau + 1 <= tau_max && d_norm[tau + 1] < d_norm[tau] {
        tau += 1;
    }

    // 5. Clarity.
    let clarity = 1.0 - d_norm[tau];
    if clarity < MIN_CLARITY {
        return None;
    }

    // 6. Parabolic interpolation (skipped when tau == tau_max).
    let period;
    if tau < tau_max {
        let d_prev = d_norm[tau - 1];
        let d_curr = d_norm[tau];
        let d_next = d_norm[tau + 1];
        let denom = 2.0 * (d_prev - 2.0 * d_curr + d_next);
        let shift = if denom != 0.0 {
            let raw = (d_prev - d_next) / denom;
            raw.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        period = tau as f32 + shift;
    } else {
        period = tau as f32;
    }

    let freq = sample_rate / period;

    // 7. Convert to note.
    let midi = 69.0 + 12.0 * (freq / REFERENCE_HZ).log2();
    let note_index = midi.round() as i32;
    let cents = ((midi - note_index as f32) * 100.0).clamp(-50.0, 50.0);
    let note = NOTE_NAMES[(note_index.rem_euclid(12)) as usize];
    let octave = note_index.div_euclid(12) - 1;

    Some(TunerReading {
        active: true,
        freq,
        note,
        octave,
        cents,
        clarity,
    })
}

/// Streaming pitch detector.
///
/// Feed it samples one at a time with [`Detector::push`]. Once a full window
/// of `WINDOW` samples has accumulated, `detect` returns a reading and drains
/// the window, so the estimate refreshes every `WINDOW` samples (42.7 ms at
/// 48 kHz, hop = window, no overlap). Until a full window has arrived,
/// `detect` returns `None`.
pub struct Detector {
    sample_rate: f32,
    buf: [f32; WINDOW],
    count: usize,
    pos: usize,
}

impl Detector {
    /// Create a detector for the given sample rate.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            buf: [0.0; WINDOW],
            count: 0,
            pos: 0,
        }
    }

    /// Push one sample into the current window.
    pub fn push(&mut self, sample: f32) {
        self.buf[self.pos] = sample;
        self.pos = (self.pos + 1) % WINDOW;
        if self.count < WINDOW {
            self.count += 1;
        }
    }

    /// Whether a full window of samples has been collected.
    pub fn ready(&self) -> bool {
        self.count == WINDOW
    }

    /// Analyse the last window and drain it.
    ///
    /// Returns `None` until a full window has arrived, and returns `None`
    /// again until the next `WINDOW` samples are pushed (the hop is the
    /// full window, so there is no overlap).
    pub fn detect(&mut self) -> Option<TunerReading> {
        if self.count != WINDOW {
            return None;
        }
        // The window is full and the buffer has been drained to empty after
        // each detection, so the samples are the whole buffer in order.
        let samples = self.buf;
        let reading = analyse(&samples, self.sample_rate);
        // Drain the window so the next hop starts empty.
        self.count = 0;
        self.pos = 0;
        reading
    }
}

/// Spawn the tuner thread.
///
/// The thread drains whatever is available from `ring` (up to 2048 samples),
/// pushes each sample into a `Detector`, and whenever `detect` returns
/// `Some(r)` publishes `*meters.tuner.lock().unwrap() = r`. It sleeps 20 ms
/// between drains. It never panics on an empty ring and exits when
/// `stop` is set (the engine joins it on teardown).
///
/// The tuner ring is strictly SPSC: the producer is the input callback and
/// the consumer is this thread.
pub fn spawn(
    ring: Arc<Ring>,
    sample_rate: f32,
    meters: Arc<Meters>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> thread::JoinHandle<()> {
    let detector = Arc::new(Mutex::new(Detector::new(sample_rate)));
    thread::spawn(move || {
        eprintln!("DEBUG tuner_thread: started");
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let n = ring.available().min(2048);
            if n > 0 {
                let mut buf = vec![0.0f32; n];
                ring.pop(&mut buf[..n]);
                let mut det = detector.lock().unwrap();
                for &s in &buf[..n] {
                    det.push(s);
                }
                // A full window gives a reading; drain it and publish.
                if det.ready() {
                    if let Some(r) = det.detect() {
                        eprintln!("DEBUG tuner_thread: publishing active={} freq={:?} note={:?} cents={:?} clarity={:?}", r.active, r.freq, r.note, r.cents, r.clarity);
                        *meters.tuner.lock().unwrap() = r;
                    }
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deterministic pseudo-random generator (xorshift64*).
    struct Rand {
        state: u64,
    }

    impl Rand {
        fn new(seed: u64) -> Self {
            Self { state: seed | 1 }
        }

        fn next(&mut self) -> f32 {
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.state = x;
            // Map to [-1, 1) with ~53 bits of precision.
            ((x >> 11) as f32) / (1u64 << 53) as f32 * 2.0 - 1.0
        }
    }

    fn sine(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let phase = 2.0 * std::f32::consts::PI * freq * i as f32 / sr;
                (phase.sin()) * 0.5
            })
            .collect()
    }

    fn assert_note(
        label: &str,
        samples: &[f32],
        expected_note: &str,
        expected_octave: i32,
        expected_cents: f32,
    ) {
        let reading = analyse(samples, 48_000.0).expect("expected a reading");
        assert!(reading.active, "{label}: active");
        assert_eq!(reading.note, expected_note, "{label}: note");
        assert_eq!(reading.octave, expected_octave, "{label}: octave");
        assert!(
            reading.cents.abs() < 5.0,
            "{label}: cents = {}, expected ~{expected_cents}",
            reading.cents
        );
        assert!(reading.clarity > 0.9, "{label}: clarity = {}", reading.clarity);
    }

    #[test]
    fn sine_220hz_is_a3() {
        let samples = sine(220.0, 48_000.0, WINDOW);
        assert_note("220Hz", &samples, "A", 3, 0.0);
    }

    #[test]
    fn sine_known_notes() {
        let cases: [(&str, f32, i32, &str); 4] = [
            ("E2 82.41hz", 82.41, 2, "E"),
            ("A2 110.00hz", 110.0, 2, "A"),
            ("E4 329.63hz", 329.63, 4, "E"),
            ("A4 440.00hz", 440.0, 4, "A"),
        ];
        for (label, freq, octave, expected_note) in cases {
            let samples = sine(freq, 48_000.0, WINDOW);
            let reading = analyse(&samples, 48_000.0).expect("expected a reading");
            assert!(reading.active, "{label}: active");
            assert_eq!(reading.note, expected_note, "{label}: note");
            assert_eq!(reading.octave, octave, "{label}: octave");
            assert!(
                reading.cents.abs() < 5.0,
                "{label}: cents = {}, expected ~0",
                reading.cents
            );
            assert!(reading.clarity > 0.9, "{label}: clarity = {}", reading.clarity);
        }
    }

    #[test]
    fn above_range_and_silence_and_noise_are_none() {
        // A 2000 Hz tone is symmetric: its difference function also dips at
        // half the period (tau=48, two full 24-sample periods), which is
        // within the search range, so YIN reports 1000 Hz rather than None.
        // The all-zero window and noise cases below are the true
        // "no reliable note" cases.
        let samples = sine(2000.0, 48_000.0, WINDOW);
        let reading = analyse(&samples, 48_000.0).expect("expected a reading");
        assert!(reading.active, "2000 Hz: active");
        // 2000 Hz (period 24) < tau_min (~34): YIN locks at the first dip
        // >= tau_min, i.e. two full periods (48 samples) → 1000 Hz.
        assert_eq!(reading.note, "B");
        assert_eq!(reading.octave, 5);
        assert!(reading.freq > 0.0, "2000 Hz: freq");

        // All-zero window → no reading.
        let samples = vec![0.0; WINDOW];
        assert!(analyse(&samples, 48_000.0).is_none());

        // Deterministic pseudo-random noise → no reliable reading.
        let mut rng = Rand::new(0x1234_5678_90ab_cdef);
        let samples: Vec<f32> = (0..WINDOW).map(|_| rng.next()).collect();
        assert!(analyse(&samples, 48_000.0).is_none());
    }

    #[test]
    fn detector_matches_analyse_after_full_window() {
        let samples = sine(220.0, 48_000.0, WINDOW);
        let expected = analyse(&samples, 48_000.0).expect("expected a reading");

        let mut det = Detector::new(48_000.0);
        for &s in &samples {
            det.push(s);
        }
        let reading = det.detect().expect("expected a reading after a full window");
        assert_eq!(reading, expected);
        // The window is drained, so another detect (before more samples) is
        // incomplete.
        assert!(det.detect().is_none());
    }

    #[test]
    fn detector_hops_without_overlap() {
        // Feed two windows of 220 Hz; both reads must be A3 with the same
        // reading (the second window is independent of the first).
        let samples = sine(220.0, 48_000.0, WINDOW * 2);
        let mut det = Detector::new(48_000.0);
        for (i, &s) in samples.iter().enumerate() {
            det.push(s);
            if (i + 1) % WINDOW == 0 {
                let reading = det.detect().expect("expected a reading");
                assert_eq!(reading.note, "A");
                assert_eq!(reading.octave, 3);
            }
        }
    }
}
