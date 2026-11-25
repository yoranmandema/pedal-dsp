//! Shared overdrive building blocks (design §1).
//!
//! These are the reusable pieces every overdrive variant is built from:
//!
//! - **B1** input high-pass (20 Hz, fixed),
//! - **B2/B5** tone bins (pre-bass / pre-mid / pre-treble / post-tone /
//!   presence / Big Muff tone) and the tone stage that cascades them,
//! - **B3** waveshapers (soft clip, tanh, Tube, Fuzz, DiodeClip — see
//!   `pedal_dsp::waveshape`),
//! - **B4** anti-aliasing (4× oversampling — see
//!   `pedal_dsp::oversample`),
//! - **B6** level/gain staging (the variant blocks apply `db_to_lin` to the
//!   final `level` knob).
//!
//! The variant blocks in `graph/src/blocks.rs` compose these helpers.

use pedal_dsp::biquad::Biquad;
use pedal_dsp::biquad::BiquadKind;
use pedal_dsp::util::clamp;

/// B1 — input high-pass, fixed at 20 Hz (design §1).
///
/// Unity-gain, Q 0.707, no parameter. Designed once per graph (`Block::prepare`)
/// and applied per sample.
pub struct InputHpf {
    filt: Biquad,
}

impl InputHpf {
    /// Build the fixed 20 Hz, Q 0.707, unity-gain high-pass at `sr`.
    pub fn new(sr: f32) -> Self {
        Self {
            filt: Biquad::make(BiquadKind::HighPass, sr, 20.0, 0.707, 0.0),
        }
    }

    /// Process one sample.
    pub fn process(&mut self, x: f32) -> f32 {
        self.filt.process(x)
    }

    /// Clear internal state.
    pub fn reset(&mut self) {
        self.filt.reset();
    }
}

/// B2/B5 — the shared tone stage: a cascade of Biquad bins.
///
/// All bins default to pass-through (`Biquad::identity()`). Each variant sets
/// the bins it needs. `process` cascades the bins in a fixed order:
/// pre_bass → pre_mid → pre_treble → post_tone → presence → big_muff_tone.
///
/// Bin frequencies are fixed by the design doc (B2) except `big_muff_tone`,
/// whose centre frequency is swept from a Hertz knob. Coefficients are
/// designed at the current sample rate per block (never per sample).
pub struct ToneStage {
    pre_bass: Biquad,
    pre_mid: Biquad,
    pre_treble: Biquad,
    post_tone: Biquad,
    presence: Biquad,
    big_muff_tone: Biquad,

    // Target parameters (updated from UI/host, applied at block boundary)
    target_pre_bass_db: f32,
    target_pre_mid_db: f32,
    target_pre_treble_db: f32,
    target_post_tone_db: f32,
    target_presence_db: f32,
    target_bigmuff_f0_hz: f32,
    target_bigmuff_db: f32,

    // Current (smoothed) parameters for per-sample interpolation
    current_pre_bass_db: f32,
    current_pre_mid_db: f32,
    current_pre_treble_db: f32,
    current_post_tone_db: f32,
    current_presence_db: f32,
    current_bigmuff_f0_hz: f32,
    current_bigmuff_db: f32,

    // Coefficients for per-sample EMA smoothing (τ = 20 ms default)
    smooth_coef: f32,

    // Dirty flags — true when target != current, meaning we need to rebuild
    dirty: bool,
}

impl ToneStage {
    /// Build a `ToneStage` with every bin set to pass-through.
    pub fn new(_sr: f32) -> Self {
        Self {
            pre_bass: Biquad::identity(),
            pre_mid: Biquad::identity(),
            pre_treble: Biquad::identity(),
            post_tone: Biquad::identity(),
            presence: Biquad::identity(),
            big_muff_tone: Biquad::identity(),

            target_pre_bass_db: 0.0,
            target_pre_mid_db: 0.0,
            target_pre_treble_db: 0.0,
            target_post_tone_db: 0.0,
            target_presence_db: 0.0,
            target_bigmuff_f0_hz: 1000.0,
            target_bigmuff_db: 0.0,

            current_pre_bass_db: 0.0,
            current_pre_mid_db: 0.0,
            current_pre_treble_db: 0.0,
            current_post_tone_db: 0.0,
            current_presence_db: 0.0,
            current_bigmuff_f0_hz: 1000.0,
            current_bigmuff_db: 0.0,

            smooth_coef: 0.0,
            dirty: true,
        }
    }

    /// Prepare at the host sample rate. Call from `Block::prepare`.
    pub fn prepare(&mut self, sr: f32) {
        // Default τ = 20 ms for tone parameter smoothing
        self.smooth_coef = smoothing_coef_ms(20.0, sr);
        // Rebuild all bins at their target values
        self.rebuild_bins(sr);
    }

    /// Update smoothing time constant (call from prepare if you need a different τ).
    pub fn set_smoothing_ms(&mut self, ms: f32, sr: f32) {
        self.smooth_coef = smoothing_coef_ms(ms, sr);
    }

    /// Set pre-bass target (low shelf, 200 Hz, Q 0.707). Call from `process` or param update.
    pub fn set_pre_bass(&mut self, db: f32) {
        let db = clamp(db, -24.0, 24.0);
        if (self.target_pre_bass_db - db).abs() > 1e-4 {
            self.target_pre_bass_db = db;
            self.dirty = true;
        }
    }

    /// Set pre-mid target (peaking, 720 Hz, Q 1.2).
    pub fn set_pre_mid(&mut self, db: f32) {
        let db = clamp(db, -24.0, 24.0);
        if (self.target_pre_mid_db - db).abs() > 1e-4 {
            self.target_pre_mid_db = db;
            self.dirty = true;
        }
    }

    /// Set pre-treble target (high shelf, 4000 Hz, Q 0.707).
    pub fn set_pre_treble(&mut self, db: f32) {
        let db = clamp(db, -24.0, 24.0);
        if (self.target_pre_treble_db - db).abs() > 1e-4 {
            self.target_pre_treble_db = db;
            self.dirty = true;
        }
    }

    /// Set post-tone target (low shelf, 3200 Hz, Q 0.707).
    pub fn set_post_tone(&mut self, db: f32) {
        let db = clamp(db, -24.0, 24.0);
        if (self.target_post_tone_db - db).abs() > 1e-4 {
            self.target_post_tone_db = db;
            self.dirty = true;
        }
    }

    /// Set presence target (high shelf, 5000 Hz, Q 0.8).
    pub fn set_presence(&mut self, db: f32) {
        let db = clamp(db, -24.0, 24.0);
        if (self.target_presence_db - db).abs() > 1e-4 {
            self.target_presence_db = db;
            self.dirty = true;
        }
    }

    /// Set Big Muff tone target (peaking at `f0`, Q 0.7, gain dB).
    pub fn set_bigmuff_tone(&mut self, f0: f32, db: f32) {
        let f0 = clamp(f0, 20.0, 20000.0);
        let db = clamp(db, -24.0, 24.0);
        if (self.target_bigmuff_f0_hz - f0).abs() > 1e-3 || (self.target_bigmuff_db - db).abs() > 1e-4 {
            self.target_bigmuff_f0_hz = f0;
            self.target_bigmuff_db = db;
            self.dirty = true;
        }
    }

    /// Rebuild all bins from current (smoothed) parameters. Call once per block before processing.
    fn rebuild_bins(&mut self, sr: f32) {
        self.pre_bass = Biquad::make(BiquadKind::LowShelf, sr, 200.0, 0.707, self.current_pre_bass_db);
        self.pre_mid = Biquad::make(BiquadKind::Peak, sr, 720.0, 1.2, self.current_pre_mid_db);
        self.pre_treble = Biquad::make(BiquadKind::HighShelf, sr, 4000.0, 0.707, self.current_pre_treble_db);
        self.post_tone = Biquad::make(BiquadKind::LowShelf, sr, 3200.0, 0.707, self.current_post_tone_db);
        self.presence = Biquad::make(BiquadKind::HighShelf, sr, 5000.0, 0.8, self.current_presence_db);
        self.big_muff_tone = Biquad::make(BiquadKind::Peak, sr, self.current_bigmuff_f0_hz, 0.7, self.current_bigmuff_db);
        self.dirty = false;
    }

        /// True when any smoothed parameter has not yet reached its target.
    fn converging(&self) -> bool {
        (self.current_pre_bass_db - self.target_pre_bass_db).abs() > 1e-5
            || (self.current_pre_mid_db - self.target_pre_mid_db).abs() > 1e-5
            || (self.current_pre_treble_db - self.target_pre_treble_db).abs() > 1e-5
            || (self.current_post_tone_db - self.target_post_tone_db).abs() > 1e-5
            || (self.current_presence_db - self.target_presence_db).abs() > 1e-5
            || (self.current_bigmuff_f0_hz - self.target_bigmuff_f0_hz).abs() > 1e-3
            || (self.current_bigmuff_db - self.target_bigmuff_db).abs() > 1e-5
    }

    /// Update smoothed parameters toward targets. Call once per block before processing.
    fn update_smoothing(&mut self) {
        if self.current_pre_bass_db != self.target_pre_bass_db {
            self.current_pre_bass_db += self.smooth_coef * (self.target_pre_bass_db - self.current_pre_bass_db);
            if (self.current_pre_bass_db - self.target_pre_bass_db).abs() < 1e-5 {
                self.current_pre_bass_db = self.target_pre_bass_db;
            }
        }
        if self.current_pre_mid_db != self.target_pre_mid_db {
            self.current_pre_mid_db += self.smooth_coef * (self.target_pre_mid_db - self.current_pre_mid_db);
            if (self.current_pre_mid_db - self.target_pre_mid_db).abs() < 1e-5 {
                self.current_pre_mid_db = self.target_pre_mid_db;
            }
        }
        if self.current_pre_treble_db != self.target_pre_treble_db {
            self.current_pre_treble_db += self.smooth_coef * (self.target_pre_treble_db - self.current_pre_treble_db);
            if (self.current_pre_treble_db - self.target_pre_treble_db).abs() < 1e-5 {
                self.current_pre_treble_db = self.target_pre_treble_db;
            }
        }
        if self.current_post_tone_db != self.target_post_tone_db {
            self.current_post_tone_db += self.smooth_coef * (self.target_post_tone_db - self.current_post_tone_db);
            if (self.current_post_tone_db - self.target_post_tone_db).abs() < 1e-5 {
                self.current_post_tone_db = self.target_post_tone_db;
            }
        }
        if self.current_presence_db != self.target_presence_db {
            self.current_presence_db += self.smooth_coef * (self.target_presence_db - self.current_presence_db);
            if (self.current_presence_db - self.target_presence_db).abs() < 1e-5 {
                self.current_presence_db = self.target_presence_db;
            }
        }
        if self.current_bigmuff_f0_hz != self.target_bigmuff_f0_hz {
            self.current_bigmuff_f0_hz += self.smooth_coef * (self.target_bigmuff_f0_hz - self.current_bigmuff_f0_hz);
            if (self.current_bigmuff_f0_hz - self.target_bigmuff_f0_hz).abs() < 1e-3 {
                self.current_bigmuff_f0_hz = self.target_bigmuff_f0_hz;
            }
        }
        if self.current_bigmuff_db != self.target_bigmuff_db {
            self.current_bigmuff_db += self.smooth_coef * (self.target_bigmuff_db - self.current_bigmuff_db);
            if (self.current_bigmuff_db - self.target_bigmuff_db).abs() < 1e-5 {
                self.current_bigmuff_db = self.target_bigmuff_db;
            }
        }
    }

    /// Begin a new block: update smoothing, rebuild bins while still converging.
    /// Call once per block from `Block::process` before the sample loop.
    pub fn begin_block(&mut self, sr: f32) {
        self.update_smoothing();
        if self.converging() || self.dirty {
            self.rebuild_bins(sr);
        }
    }

    /// Process one sample through the active bins.
    pub fn process(&mut self, x: f32) -> f32 {
        let mut s = x;
        s = self.pre_bass.process(s);
        s = self.pre_mid.process(s);
        s = self.pre_treble.process(s);
        s = self.post_tone.process(s);
        s = self.presence.process(s);
        s = self.big_muff_tone.process(s);
        s
    }

    /// Clear internal state on all bins.
    pub fn reset(&mut self) {
        let _ = self.pre_bass.reset();
        let _ = self.pre_mid.reset();
        let _ = self.pre_treble.reset();
        let _ = self.post_tone.reset();
        let _ = self.presence.reset();
        let _ = self.big_muff_tone.reset();
        // Snap current to target to avoid jumps after reset
        self.current_pre_bass_db = self.target_pre_bass_db;
        self.current_pre_mid_db = self.target_pre_mid_db;
        self.current_pre_treble_db = self.target_pre_treble_db;
        self.current_post_tone_db = self.target_post_tone_db;
        self.current_presence_db = self.target_presence_db;
        self.current_bigmuff_f0_hz = self.target_bigmuff_f0_hz;
        self.current_bigmuff_db = self.target_bigmuff_db;
    }
}

/// Per-sample EMA smoothing coefficient for a given time constant (ms) at sample rate `sr`.
fn smoothing_coef_ms(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms * 0.001 * sr)).exp()
}