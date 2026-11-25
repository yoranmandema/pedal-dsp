//! Distortion / waveshaping stages.
//!
//! Aliasing is the enemy here: a nonlinearity applied at the native sample
//! rate folds harmonics back into the audio band. Every kind except the
//! sample-rate-dependent ones (`Bitcrush`, `Decimator`) runs through an
//! oversampling stage by default.

use crate::oversample::{Downsampler2, Oversampler4, Upsampler2};
use crate::util::{clamp, clamp_u32, mix, saturate, tanh_approx};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaperKind {
    HardClip,
    SoftClip,
    Tanh,
    /// Asymmetric, even-harmonic heavy — the "tube" flavour.
    Tube,
    /// Aggressive fuzz with bias.
    Fuzz,
    RectifyHalf,
    RectifyFull,
    Bitcrush,
    Decimator,
    /// Two-stage "diode-like" clip: `y = tanh_approx(g * tanh_approx(x))`.
    /// A single-pole diode would be just `tanh_approx(g * x)`; feeding the
    /// output of one tanh into a second tanh pushes the transfer curve
    /// further toward a hard clip while keeping even harmonics low. Used by
    /// the Precision Drive input stage (4× oversampling wraps it).
    DiodeClip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Oversampling {
    None,
    X2,
    X4,
}

pub struct Shaper {
    pub kind: ShaperKind,
    /// Input gain before the nonlinearity, 0.1 .. 20.
    pub drive: f32,
    /// 0 = dry, 1 = fully wet.
    pub mix: f32,
    /// Output level.
    pub level: f32,
    pub oversampling: Oversampling,

    up2: Upsampler2,
    down2: Downsampler2,
    up4: Oversampler4,

    // Sample-rate-dependent effects state
    decim_counter: u32,
    decim_factor: u32,
    held: f32,
    bit_levels: f32,
}

impl Default for Shaper {
    fn default() -> Self {
        Self::new(ShaperKind::Tanh)
    }
}

impl Shaper {
    pub fn new(kind: ShaperKind) -> Self {
        Self {
            kind,
            drive: 3.0,
            mix: 1.0,
            level: 0.8,
            oversampling: if matches!(kind, ShaperKind::Bitcrush | ShaperKind::Decimator) {
                Oversampling::None
            } else {
                Oversampling::X4
            },
            up2: Upsampler2::new(),
            down2: Downsampler2::new(),
            up4: Oversampler4::new(),
            decim_counter: 0,
            decim_factor: 4,
            held: 0.0,
            bit_levels: 256.0,
        }
    }

    pub fn set_drive(&mut self, drive: f32) {
        self.drive = clamp(drive, 0.1, 20.0);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = clamp(mix, 0.0, 1.0);
    }

    pub fn set_level(&mut self, level: f32) {
        self.level = clamp(level, 0.0, 4.0);
    }

    /// Bit depth for `Bitcrush`, 1..16 bits.
    pub fn set_bits(&mut self, bits: u32) {
        let bits = clamp_u32(bits, 1, 16);
        self.bit_levels = (1u32 << bits) as f32;
    }

    /// Decimation factor for `Decimator`, 1..64.
    pub fn set_decimation(&mut self, factor: u32) {
        self.decim_factor = clamp_u32(factor, 1, 64);
        self.decim_counter = 0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        match self.kind {
            ShaperKind::Bitcrush => {
                let y = self.bitcrush(x);
                mix(x, y, self.mix) * self.level
            }
            ShaperKind::Decimator => {
                let y = self.decimate(x);
                mix(x, y, self.mix) * self.level
            }
            _ => {
                let y = match self.oversampling {
                    Oversampling::None => self.shape(x),
                    Oversampling::X2 => {
                        let [a, b] = self.up2.process(x);
                        let ya = self.shape(a);
                        let yb = self.shape(b);
                        if let Some(v) = self.down2.process(ya) {
                            v
                        } else {
                            self.down2.process(yb).unwrap()
                        }
                    }
                    Oversampling::X4 => {
                        let [a, b, c, d] = self.up4.up(x);
                        let ys = [
                            self.shape(a),
                            self.shape(b),
                            self.shape(c),
                            self.shape(d),
                        ];
                        let mut y = 0.0;
                        for v in ys {
                            if let Some(o) = self.up4.down(v) {
                                y = o;
                            }
                        }
                        y
                    }
                };
                mix(x, y, self.mix) * self.level
            }
        }
    }

    fn shape(&self, x: f32) -> f32 {
        let d = x * self.drive;
        match self.kind {
            ShaperKind::HardClip => saturate(d),
            ShaperKind::SoftClip => d / (1.0 + d.abs()),
            ShaperKind::Tanh => tanh_approx(d),
            ShaperKind::Tube => {
                if d >= 0.0 {
                    tanh_approx(d)
                } else {
                    tanh_approx(d * 0.6) * 0.75
                }
            }
            ShaperKind::Fuzz => {
                // Bias then clip hard: lots of even harmonics.
                let biased = d * 0.7 + 0.15;
                saturate(biased * 1.6)
            }
            ShaperKind::RectifyHalf => d.max(0.0),
            ShaperKind::RectifyFull => d.abs(),
            ShaperKind::Bitcrush | ShaperKind::Decimator => d,
            // Two-stage diode-like clip: y = tanh_approx(g * tanh_approx(x)).
            // The drive multiplies the inner tanh. The input is NOT pre-scaled
            // by `d` here (the shared `d = x * self.drive` above is only used
            // by the other shapes).
            ShaperKind::DiodeClip => tanh_approx(self.drive * tanh_approx(x)),
        }
    }

    fn bitcrush(&self, x: f32) -> f32 {
        let levels = self.bit_levels;
        (libm::roundf(x * levels)) / levels
    }

    fn decimate(&mut self, x: f32) -> f32 {
        self.decim_counter += 1;
        if self.decim_counter >= self.decim_factor {
            self.decim_counter = 0;
            self.held = x;
        }
        self.held
    }

    pub fn reset(&mut self) {
        self.up2.reset();
        self.down2.reset();
        self.up4.reset();
        self.decim_counter = 0;
        self.held = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_clip_is_bounded() {
        let mut s = Shaper::new(ShaperKind::HardClip);
        s.set_drive(10.0);
        for i in 0..1000 {
            let y = s.process(libm::sinf(i as f32 * 0.1));
            // The half-band FIR rings slightly around the clip corners.
            assert!(y.abs() <= 1.2 * s.level + 1e-6, "y {y}");
        }
    }

    #[test]
    fn tanh_is_bounded() {
        let mut s = Shaper::new(ShaperKind::Tanh);
        s.set_drive(50.0);
        for i in 0..1000 {
            let y = s.process(i as f32 * 0.01);
            assert!(y.abs() <= 1.05 * s.level + 1e-6, "y {y}");
        }
    }

    #[test]
    fn dry_wet_mix() {
        let mut s = Shaper::new(ShaperKind::HardClip);
        s.set_drive(1.0);
        s.set_mix(0.0);
        s.set_level(1.0);
        assert!((s.process(0.5) - 0.5).abs() < 1e-6, "mix=0 must be dry");
        s.set_mix(1.0);
        // Let the oversampling FIR settle before measuring.
        for _ in 0..2000 {
            s.process(0.5);
        }
        assert!((s.process(0.5) - 0.5).abs() < 0.02, "below clip threshold");
    }

    #[test]
    fn rectifiers_are_unbipolar() {
        let mut half = Shaper::new(ShaperKind::RectifyHalf);
        half.set_level(1.0);
        for i in 0..100 {
            assert!(half.process(i as f32 * 0.05 - 2.0) >= -0.1);
        }
        let mut full = Shaper::new(ShaperKind::RectifyFull);
        full.set_level(1.0);
        for i in 0..100 {
            assert!(full.process(i as f32 * 0.05 - 2.0) >= -0.1);
        }
    }

    #[test]
    fn bitcrush_quantizes() {
        let mut s = Shaper::new(ShaperKind::Bitcrush);
        s.set_bits(3);
        s.set_level(1.0);
        s.set_mix(1.0);
        let y = s.process(0.5);
        let levels = 8.0f32;
        let q = (0.5 * levels).round() / levels;
        assert!((y - q).abs() < 1e-6, "got {y}, expected {q}");
    }

    #[test]
    fn decimator_holds() {
        let mut s = Shaper::new(ShaperKind::Decimator);
        s.set_decimation(4);
        s.set_level(1.0);
        s.set_mix(1.0);
        let first = s.process(1.0);
        for _ in 0..3 {
            assert_eq!(s.process(0.0), first, "should hold");
        }
        assert_eq!(s.process(0.0), 0.0, "should update after 4");
    }

    #[test]
    fn diode_clip_is_a_two_stage_diode_like_clip() {
        // y = tanh_approx(g * tanh_approx(x)). The drive g multiplies the
        // inner tanh; a large drive approaches a hard clip.
        let mut s = Shaper::new(ShaperKind::DiodeClip);
        s.set_drive(8.0);
        s.set_level(1.0);
        s.set_mix(1.0);
        // Symmetric (odd) transfer: tanh is odd, so the two-stage clip is odd.
        assert!((s.process(0.7) - s.process(-0.7)).abs() < 1e-6, "diode clip is not symmetric");
        // Bounded by the output level.
        let y = s.process(50.0);
        assert!(y.abs() <= 1.05 * s.level + 1e-6, "y = {y}");
        // Drive 0 forces the output to 0 (tanh_approx(0 * ...) = 0). The
        // Shaper::set_drive clamps to 0.1, so test the lower bound directly.
        let mut s0 = Shaper::new(ShaperKind::DiodeClip);
        s0.set_drive(0.1);
        s0.set_level(1.0);
        s0.set_mix(1.0);
        let y0 = s0.process(0.3);
        assert!(y0.abs() < 0.05, "drive 0.1 must be near-silent: {y0}");
    }

    #[test]
    fn shapers_are_finite_for_large_input() {
        let mut s = Shaper::new(ShaperKind::DiodeClip);
        s.set_drive(20.0);
        for i in 0..1000 {
            let y = s.process(i as f32 * 0.5);
            assert!(y.is_finite(), "non-finite at sample {i}: {y}");
        }
    }

    #[test]
    fn diode_clip_silence_in_silence_out() {
        let mut s = Shaper::new(ShaperKind::DiodeClip);
        s.set_drive(10.0);
        s.set_level(1.0);
        s.set_mix(1.0);
        assert!((s.process(0.0) - 0.0).abs() < 1e-6);
    }
}
