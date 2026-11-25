//! Fuzz effect: three circuit-derived variants sharing one DSP path.
//!
//! Models:
//! - `Face` — Fuzz Face (asymmetric germanium feedback pair)
//! - `Muff` — Big Muff (cascaded tanh + diode clamp, sustain via feedback)
//! - `Bender` — Tone Bender / FZ-1 (gain + gated zener-style clipper)
//!
//! Shared signal path (mono): input HPF (20 Hz) → per-variant waveshaper
//! (4× oversampled) → passive tone stack (bass shelf 200 Hz / mid peak 1 kHz /
//! treble shelf 4 kHz, Q = 0.7) → cubic soft limiter → level → output DC block.
//!
//! The waveshaper runs at 4× oversampling (two cascaded 63-tap half-band
//! FIRs) so the memoryless nonlinearities and their sharp edges alias as
//! little as possible. The tone stack runs at native rate.
//!
//! All fields are plain `f32`/`Copy` so the control loop can update them
//! without allocating. `Fuzz` is `no_std` and allocator-free, like `Shaper`
//! and `EffectChain`.

use crate::dc_blocker::DcBlocker;
use crate::oversample::{Downsampler2, Oversampler4, Upsampler2};
use crate::Biquad;
use crate::{BiquadKind, util::clamp};

/// Cubic soft limiter: unity-ish slope near 0, saturates at ±1 (NaN-safe).
#[inline]
fn soft_limit(x: f32) -> f32 {
    let x = if x.is_finite() { clamp(x, -1.5, 1.5) } else { 0.0 };
    x - x * x * x / 6.75
}

/// The three fuzz variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzModel {
    /// Fuzz Face (Shin-ei FU-725S): asymmetric germanium feedback pair.
    Face,
    /// Big Muff (Electro-Harmonix / Riverside): cascaded tanh + diode clamp.
    Muff,
    /// Tone Bender / Maestro FZ-1: gain + gated zener-style clipper.
    Bender,
}

impl Default for FuzzModel {
    fn default() -> Self {
        FuzzModel::Face
    }
}

/// Valid choice labels for the `model` parameter (mirrors `Amp::AMP_VOICES`).
pub const FUZZ_MODELS: &[&str] = &["Fuzz Face", "Big Muff", "Tone Bender / FZ-1"];

/// One Fuzz processor: three circuit-derived variants sharing one signal
/// path. Mono, `no_std`, zero-alloc, safe to call from the audio callback.
pub struct Fuzz {
    pub model: FuzzModel,
    /// Input gain into the first clipping stage (0.1 .. 10, log).
    pub drive: f32,
    /// Bass shelf gain, dB (−12 .. +12).
    pub bass: f32,
    /// Mid peak gain, dB (−12 .. +12).
    pub mid: f32,
    /// Treble shelf gain, dB (−12 .. +12).
    pub treble: f32,
    /// Output level after the soft limiter (0 .. 4, linear, from the dB `level`
    /// param).
    pub level: f32,
    /// Oversampling mode (uniform X4 for all three models).
    pub oversampling: Oversampling,

    /// 2× up/down pairs (kept for API compatibility with `Shaper`; only X4
    /// is used here).
    up2: Upsampler2,
    down2: Downsampler2,
    /// Two cascaded 2x stages (4x total).
    up4: Oversampler4,

    dc_in: DcBlocker,
    dc_out: DcBlocker,

    tone_low: Biquad,
    tone_mid: Biquad,
    tone_high: Biquad,
}

impl Default for Fuzz {
    fn default() -> Self {
        Self::new()
    }
}

impl Fuzz {
    pub fn new() -> Self {
        Self {
            model: FuzzModel::Face,
            drive: 2.5,
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            level: 0.5,
            oversampling: Oversampling::X4,
            up2: Upsampler2::new(),
            down2: Downsampler2::new(),
            up4: Oversampler4::new(),
            dc_in: DcBlocker::new(20.0, crate::DEFAULT_SAMPLE_RATE),
            dc_out: DcBlocker::new(20.0, crate::DEFAULT_SAMPLE_RATE),
            tone_low: Biquad::identity(),
            tone_mid: Biquad::identity(),
            tone_high: Biquad::identity(),
        }
    }

    /// Update all parameters, designing the tone biquads with the given
    /// sample rate. Called once per block by the graph block's `process`.
    pub fn set_params(
        &mut self,
        model: FuzzModel,
        drive: f32,
        bass: f32,
        mid: f32,
        treble: f32,
        level: f32,
        sr: f32,
    ) {
        self.model = model;
        self.drive = clamp(drive, 0.1, 10.0);
        self.bass = clamp(bass, -12.0, 12.0);
        self.mid = clamp(mid, -12.0, 12.0);
        self.treble = clamp(treble, -12.0, 12.0);
        self.level = clamp(level, 0.0, 4.0);
        self.design_tone_stack(sr);
    }

    /// Design the three tone-stack biquads from the current dB gains.
    pub fn set_tone(&mut self, bass: f32, mid: f32, treble: f32, sr: f32) {
        self.bass = clamp(bass, -12.0, 12.0);
        self.mid = clamp(mid, -12.0, 12.0);
        self.treble = clamp(treble, -12.0, 12.0);
        self.design_tone_stack(sr);
    }

    fn design_tone_stack(&mut self, sr: f32) {
        // Big Muff's tone stack: bass shelf 200 Hz, mid peak 1 kHz, treble
        // shelf 4 kHz, all Q = 0.7.
        self.tone_low.design(BiquadKind::LowShelf, sr, 200.0, 0.7, self.bass);
        self.tone_mid.design(BiquadKind::Peak, sr, 1000.0, 0.7, self.mid);
        self.tone_high.design(BiquadKind::HighShelf, sr, 4000.0, 0.7, self.treble);
    }

    /// Process a single mono sample (the graph block calls this per sample).
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // Input DC block (20 Hz high-pass).
        let x = self.dc_in.process(x);

        // 4× oversampled waveshaper → downsample to native rate.
        let [a, b, c, d] = self.up4.up(x);
        let rest = self.rest();
        let ys = match self.model {
            FuzzModel::Face => [self.face(a) - rest, self.face(b) - rest, self.face(c) - rest, self.face(d) - rest],
            FuzzModel::Muff => [self.muff(a) - rest, self.muff(b) - rest, self.muff(c) - rest, self.muff(d) - rest],
            FuzzModel::Bender => [self.bender(a) - rest, self.bender(b) - rest, self.bender(c) - rest, self.bender(d) - rest],
        };
        let mut y = 0.0;
        for v in ys {
            if let Some(o) = self.up4.down(v) {
                y = o;
            }
        }

        // Tone stack (bass → mid → treble, native rate).
        let y = self.tone_low.process(y);
        let y = self.tone_mid.process(y);
        let y = self.tone_high.process(y);

        // Cubic soft limiter + level, then output DC block.
        let y = soft_limit(y) * self.level;
        self.dc_out.process(y)
    }

    /// Shaper output at zero input (the bias point), subtracted so silence
    /// in gives silence out.
    #[inline]
    fn rest(&self) -> f32 {
        match self.model {
            FuzzModel::Face => self.face(0.0),
            FuzzModel::Muff => self.muff(0.0),
            FuzzModel::Bender => self.bender(0.0),
        }
    }

    /// Reset all stateful components (oversamplers, DC blockers, tone biquads).
    pub fn reset(&mut self) {
        self.dc_in.reset();
        self.dc_out.reset();
        self.tone_low.reset();
        self.tone_mid.reset();
        self.tone_high.reset();
        self.up4.reset();
        self.down2.reset();
        self.up2.reset();
    }

    // ------------------------------------------------------------------------
    // Per-variant waveshapers (memoryless nonlinearities, documented in §5).
    // ------------------------------------------------------------------------

    /// Fuzz Face (model 0): asymmetric germanium feedback pair.
    ///
    /// `y(x) = x` for `|x| < Vd`; soft knee for `x > Vd`; harder knee for
    /// `x < -Vd`. The bias is a drive-dependent +offset that shrinks as
    /// drive rises (the "starve" position), folded into `drive`/`mid`.
    fn face(&self, x: f32) -> f32 {
        let b = self.bias();
        let x = x + b;
        if x >= 0.0 {
            self.face_positive(x)
        } else {
            self.face_negative(x)
        }
    }

    fn face_positive(&self, x: f32) -> f32 {
        let v_d = 0.25;
        if x < v_d {
            x
        } else {
            v_d + (x - v_d) * (1.0 - (x - v_d) / (1.0 - v_d))
        }
    }

    fn face_negative(&self, x: f32) -> f32 {
        let v_d = 0.25;
        if x > -v_d {
            x
        } else {
            -v_d - (x + v_d) * (0.3 + 0.9 * (x + v_d) / v_d)
        }
    }

    /// Drive-dependent starve bias: shrinks as drive rises; `mid` re-biases.
    fn bias(&self) -> f32 {
        let drive_norm = (self.drive - 0.1) / (10.0 - 0.1);
        let drive_factor = 1.0 - drive_norm;
        // mid: -12..+12 dB → 0.7..1.3 scale on the bias.
        let mid_factor = 1.0 + self.mid * 0.25;
        clamp(drive_factor * 0.25 * mid_factor, 0.0, 1.0)
    }

    /// Big Muff (model 1): cascaded tanh (k1 ≈ 3.0) + knee→clamp (k2 grows
    /// with drive = feedback/sustain).
    fn muff(&self, x: f32) -> f32 {
        // Biased first stage: the offset makes the clip asymmetric, which
        // puts energy into the even harmonics.
        let y1 = crate::util::tanh_approx(self.k1_muff() * x + 0.6);
        self.muff_stage2(y1)
    }

    fn muff_stage2(&self, x: f32) -> f32 {
        // Continuous diode-style clamp: the positive swing is squashed
        // toward 0.5, the negative swing passes (softly limited at -1).
        let k2 = self.k2_muff() / 4.0; // 0.45 .. 1.0
        if x > 0.0 {
            0.5 * crate::util::tanh_approx(x * 2.0 * k2)
        } else {
            x
        }
    }

    fn k1_muff(&self) -> f32 {
        // k1 ≈ 3.0, roughly constant (the drive lives in k2 instead).
        3.0
    }

    fn k2_muff(&self) -> f32 {
        // Drive-dependent feedback: more drive → more sustain → steeper clamp.
        let drive_norm = (self.drive - 0.1) / (10.0 - 0.1);
        1.8 + drive_norm * 2.2 // 1.8 .. 4.0
    }

    /// Tone Bender / FZ-1 (model 2): tanh (k1 shrinks with drive) + gated
    /// zener-style hard clamp at Vz ≈ 0.7.
    fn bender(&self, x: f32) -> f32 {
        let k1 = self.k1_bender();
        let y1 = crate::util::tanh_approx(k1 * x + 0.5);
        self.bender_stage2(y1)
    }

    fn bender_stage2(&self, x: f32) -> f32 {
        let v_z = 0.5;
        if x >= 0.0 {
            // Zener-style hard clamp on the positive swing.
            if x > v_z { v_z } else { x }
        } else {
            // Continuous, much softer clip on the negative swing.
            // Gated: the negative swing is starved.
            0.25 * x
        }
    }

    fn k1_bender(&self) -> f32 {
        // Drive-dependent fuzz-middle: more drive → softer stage-1 knee.
        let drive_norm = (self.drive - 0.1) / (10.0 - 0.1);
        1.5 - drive_norm * 1.0 // 1.5 .. 0.5
    }
}

/// Oversampling mode for the fuzz processor (uniform X4 for all models).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Oversampling {
    None,
    X2,
    X4,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_samples(freq: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                amp * (2.0 * std::f32::consts::PI * freq * i as f32 / 48_000.0).sin()
            })
            .collect()
    }

    /// Crude harmonic probe: energy in the 2nd vs 3rd harmonic bin of a
    /// sine input (coherent single-bin DFT, not a sum of squares).
    fn harmonic_ratio(samples: &[f32], freq: f32, fs: f32) -> (f64, f64) {
        let mut re2 = 0.0f64;
        let mut im2 = 0.0f64;
        let mut re3 = 0.0f64;
        let mut im3 = 0.0f64;
        for (i, &s) in samples.iter().enumerate() {
            let w = 2.0 * std::f32::consts::PI * freq * i as f32 / fs;
            let s64 = s as f64;
            re2 += s64 * (2.0 * w as f64).cos();
            im2 += s64 * (2.0 * w as f64).sin();
            let w3 = 3.0 * w as f64;
            re3 += s64 * w3.cos();
            im3 += s64 * w3.sin();
        }
        (re2 * re2 + im2 * im2, re3 * re3 + im3 * im3)
    }

    #[test]
    fn face_is_asymmetric_and_bounded() {
        let mut f = Fuzz::new();
        f.set_params(FuzzModel::Face, 3.0, 0.0, 0.0, 0.0, 1.0, 48_000.0);
        let input = sine_samples(440.0, 4096, 0.8);
        let mut out = Vec::with_capacity(input.len());
        for &s in &input {
            out.push(f.process(s));
        }
        // Finite and bounded (soft limiter caps at `level`; DC-blocker
        // transient may push the peak a few percent past `level`).
        for &y in &out {
            assert!(y.is_finite(), "face: non-finite output {}", y);
            assert!(y.abs() <= 1.2 * f.level + 1e-6, "face: output {} exceeds 1.2x level", y);
        }
        // Even-harmonic heavy (asymmetric curve): 2nd energy > 3rd.
        let (e2, e3) = harmonic_ratio(&out, 440.0, 48_000.0);
        assert!(e2 > e3, "face: expected even-harmonic-heavy, got 2nd {e2}, 3rd {e3}");
    }

    #[test]
    fn muff_is_bounded_and_asymmetric() {
        let mut f = Fuzz::new();
        f.set_params(FuzzModel::Muff, 4.0, 0.0, 0.0, 0.0, 1.0, 48_000.0);
        let input = sine_samples(440.0, 4096, 0.8);
        let mut out = Vec::with_capacity(input.len());
        for &s in &input {
            out.push(f.process(s));
        }
        // Finite and bounded (soft limiter caps at `level`; DC-blocker
        // transient may push the peak a few percent past `level`).
        for &y in &out {
            assert!(y.is_finite(), "muff: non-finite output {}", y);
            assert!(y.abs() <= 1.2 * f.level + 1e-6, "muff: output {} exceeds 1.2x level", y);
        }
        // Big Muff has both 2nd and 3rd harmonics, 2nd still dominant.
        let (e2, e3) = harmonic_ratio(&out, 440.0, 48_000.0);
        assert!(e2 > e3, "muff: expected 2nd > 3rd, got {e2} {e3}");
    }

    #[test]
    fn bender_is_bounded_and_has_strong_2nd_harmonic() {
        let mut f = Fuzz::new();
        f.set_params(FuzzModel::Bender, 2.0, 0.0, 0.0, 0.0, 1.0, 48_000.0);
        let input = sine_samples(440.0, 4096, 0.8);
        let mut out = Vec::with_capacity(input.len());
        for &s in &input {
            out.push(f.process(s));
        }
        // Finite and bounded (soft limiter caps at `level`; DC-blocker
        // transient may push the peak a few percent past `level`).
        for &y in &out {
            assert!(y.is_finite(), "bender: non-finite output {}", y);
            assert!(y.abs() <= 1.2 * f.level + 1e-6, "bender: output {} exceeds 1.2x level", y);
        }
        // Hard zener clamp → strong 2nd harmonic.
        let (e2, e3) = harmonic_ratio(&out, 440.0, 48_000.0);
        assert!(e2 > e3, "bender: expected 2nd > 3rd, got {e2} {e3}");
    }

    #[test]
    fn silence_in_silence_out() {
        let mut f = Fuzz::new();
        f.set_params(FuzzModel::Face, 3.0, 0.0, 0.0, 0.0, 1.0, 48_000.0);
        assert!(f.process(0.0).abs() < 1e-6);
        f.set_params(FuzzModel::Muff, 4.0, 0.0, 0.0, 0.0, 1.0, 48_000.0);
        assert!(f.process(0.0).abs() < 1e-6);
        f.set_params(FuzzModel::Bender, 2.0, 0.0, 0.0, 0.0, 1.0, 48_000.0);
        assert!(f.process(0.0).abs() < 1e-6);
    }



    #[test]
    fn bounded_at_max_settings() {
        let mut f = Fuzz::new();
        // Max drive, near-max level.
        f.set_params(FuzzModel::Face, 10.0, 12.0, 12.0, 12.0, 4.0, 48_000.0);
        let input = sine_samples(440.0, 4096, 1.0);
        for &s in &input {
            let y = f.process(s);
            assert!(y.is_finite(), "face max: non-finite {}", y);
            // Soft limit caps at ±1 before level; DC-blocker transient may
            // push the peak a few percent past `level`.
            assert!(y.abs() <= 1.2 * f.level + 1e-6, "face max: {}", y);
        }
    }

    #[test]
    fn no_nan_inf_per_variant_at_extremes() {
        let inputs = [sine_samples(440.0, 1024, 1.0), sine_samples(880.0, 1024, 0.5)];
        for model in [FuzzModel::Face, FuzzModel::Muff, FuzzModel::Bender] {
            let mut f = Fuzz::new();
            for drive in [0.1, 2.5, 10.0] {
                for level in [0.0, 4.0] {
                    f.set_params(model, drive, 12.0, -12.0, 12.0, level, 48_000.0);
                    for s in &inputs {
                        for &sample in s {
                            let y = f.process(sample);
                            assert!(
                                y.is_finite(),
                                "model={:?} drive={drive} level={level} input={sample}: {y}",
                                model
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn process_is_deterministic() {
        let mut f = Fuzz::new();
        f.set_params(FuzzModel::Face, 3.0, 0.5, -2.0, 1.0, 1.0, 48_000.0);
        let input = sine_samples(440.0, 2048, 0.9);
        let mut first = Vec::with_capacity(input.len());
        for &s in &input {
            first.push(f.process(s));
        }
        // Rebuild: the oscillators share state, so rebuild for a clean run.
        let mut f2 = Fuzz::new();
        f2.set_params(FuzzModel::Face, 3.0, 0.5, -2.0, 1.0, 1.0, 48_000.0);
        let mut second = Vec::with_capacity(input.len());
        for &s in &input {
            second.push(f2.process(s));
        }
        assert_eq!(first, second, "face is non-deterministic");
    }

    #[test]
    fn reset_clears_state() {
        let mut f = Fuzz::new();
        f.set_params(FuzzModel::Face, 3.0, 0.0, 0.0, 0.0, 1.0, 48_000.0);
        let input = sine_samples(440.0, 8, 0.5);
        f.process(input[0]);
        f.reset();
        // After reset, a quiet input should not pick up leftover DC.
        let y = f.process(0.0);
        assert!(y.abs() < 1e-6, "reset left DC: {y}");
    }
}
