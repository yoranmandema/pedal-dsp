//! Per-sample one-pole (leaky integrator) filters.
//!
//! A one-pole holds a smoothed value `y` and advances it with
//!
//! ```text
//! y = coef * x + (1 - coef) * y
//! ```
//!
//! A low `coef` is a long time constant. The coefficient is normally derived
//! from a time constant in seconds and the sample rate:
//!
//! ```text
//! coef = 1 - exp(-1 / (tau * fs))
//! ```
//!
//! The envelope follower used by the Precision Drive "Punch" transient pump
//! is built on two of these, in series: an attack follower (one-pole on
//! `|x|`) and a pump follower (one-pole on the attack follower) with fast
//! attack and slow release.

/// Per-sample one-pole filter.
#[derive(Clone, Copy)]
pub struct OnePole {
    /// `1 - exp(-1 / (tau * fs))`, normally in `(0, 1)`.
    coef: f32,
    /// Smoothed value.
    value: f32,
}

impl OnePole {
    /// Build with coefficient `coef` (normally in `(0, 1)`).
    pub fn new(coef: f32) -> Self {
        Self { coef, value: 0.0 }
    }

    /// Replace the coefficient, keeping the smoothed value.
    pub fn set_coef(&mut self, coef: f32) {
        self.coef = coef;
    }

    /// Advance one sample: `y = coef * x + (1 - coef) * y`.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.value = self.coef * x + (1.0 - self.coef) * self.value;
        self.value
    }

    /// Current smoothed value without advancing.
    #[inline]
    pub fn smoothed(&self) -> f32 {
        self.value
    }

    /// Clear internal state.
    pub fn reset(&mut self) {
        self.value = 0.0;
    }
}

/// One-pole envelope follower for the Precision Drive punch pump.
///
/// Follows `|x|` through two one-poles in series:
///
/// 1. `attack` — one-pole on `|x|` with time constant `attack_ms` (fast).
/// 2. `pump` — one-pole on the attack follower with time constant `release_ms`;
///    when the attack follower exceeds the pump it tracks up with the fast
///    attack coefficient, otherwise it decays toward 0 with the slow release
///    coefficient.
///
/// This produces a classic peak envelope: the pump rises quickly on a
/// transient and decays slowly, which is exactly the "Punch" behaviour the
/// Precision Drive needs (design §2.2).
#[derive(Clone, Copy)]
pub struct EnvelopeFollower {
    attack: OnePole,
    pump: OnePole,
    /// Time constant of the attack follower (ms).
    attack_ms: f32,
    /// Time constant of the pump follower (ms).
    release_ms: f32,
    sr: f32,
    attack_coef: f32,
    release_coef: f32,
}

impl EnvelopeFollower {
    /// Build with `attack_ms` and `release_ms` time constants at sample rate
    /// `sr`.
    pub fn new(attack_ms: f32, release_ms: f32, sr: f32) -> Self {
        let (attack_coef, release_coef) = Self::coefs(attack_ms, release_ms, sr);
        Self {
            attack: OnePole::new(attack_coef),
            pump: OnePole::new(release_coef),
            attack_ms,
            release_ms,
            sr,
            attack_coef,
            release_coef,
        }
    }

    /// Recompute the coefficients from the current knob values. Called per
    /// block by the Precision Drive block as its `attack`/`release` knobs
    /// change; the envelope state persists across blocks so the response is
    /// continuous.
    pub fn set_timeconstants(&mut self, attack_ms: f32, release_ms: f32, sr: f32) {
        let (attack_coef, release_coef) = Self::coefs(attack_ms, release_ms, sr);
        self.attack.set_coef(attack_coef);
        self.pump.set_coef(release_coef);
        self.attack_ms = attack_ms;
        self.release_ms = release_ms;
        self.sr = sr;
        self.attack_coef = attack_coef;
        self.release_coef = release_coef;
    }

    /// Coefficients from time constants: `1 - exp(-1 / (tau * fs))`.
    fn coefs(tau_ms: f32, tau_ms2: f32, fs: f32) -> (f32, f32) {
        let tau1 = (tau_ms / 1000.0) * fs;
        let tau2 = (tau_ms2 / 1000.0) * fs;
        (
            1.0 - libm::expf(-1.0 / tau1.max(1.0)),
            1.0 - libm::expf(-1.0 / tau2.max(1.0)),
        )
    }

    /// Process one sample, returning the pump envelope (a peak follower that
    /// rises fast and decays slowly).
    pub fn process(&mut self, x: f32) -> f32 {
        let ax = x.abs();
        // Fast attack follower: one-pole on |x|.
        let ae = self.attack.process(ax);
        // Pump follower: tracks up fast when the attack follower wins,
        // decays toward 0 otherwise.
        let mut pe = self.pump.smoothed();
        if ae > pe {
            pe = self.attack_coef * ae + (1.0 - self.attack_coef) * pe;
        } else {
            pe = self.release_coef * ae + (1.0 - self.release_coef) * pe;
        }
        self.pump.value = pe;
        pe
    }

    /// Clear internal state.
    pub fn reset(&mut self) {
        self.attack.reset();
        self.pump.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pole_passes_through_when_coef_is_one() {
        let mut p = OnePole::new(1.0);
        p.process(0.5);
        assert!((p.smoothed() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn one_pole_holds_when_coef_is_zero() {
        let mut p = OnePole::new(0.0);
        p.process(0.5);
        assert!((p.smoothed() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn one_pole_moves_toward_input_at_each_sample() {
        let mut p = OnePole::new(0.5);
        p.process(1.0);
        assert!((p.smoothed() - 0.5).abs() < 1e-6);
        p.process(0.0);
        assert!((p.smoothed() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn envelope_follower_tracks_a_step_and_releases() {
        // Fast attack (1 ms), slow release (500 ms) at 48 kHz.
        let mut ef = EnvelopeFollower::new(1.0, 500.0, 48_000.0);
        let mut v = 0.0;
        // Feed a constant 1.0 and walk the attack follower up to steady
        // state (~5 * tau_attack samples).
        for _ in 0..2000 {
            v = ef.process(1.0);
        }
        assert!(v > 0.9, "attack follow failed: {v}");
        // Hold the input at 0 and walk the release toward 0 (~5 * tau_release
        // samples; tau_release = 24000 at 48 kHz for a 500 ms time constant).
        for _ in 0..60000 {
            v = ef.process(0.0);
        }
        assert!(v < 0.1, "release follow failed: {v}");
    }

    #[test]
    fn envelope_follower_respects_input_amplitude() {
        // 1.0 step: the pump settles near 1.0.
        {
            let mut ef = EnvelopeFollower::new(10.0, 600.0, 48_000.0);
            let mut v = 0.0;
            for _ in 0..5000 {
                v = ef.process(1.0);
            }
            assert!(v > 0.9);
        }
        // 0.2 step: fresh state, so it must settle at 0.2, not inherit the
        // 1.0 state from the previous test.
        {
            let mut ef = EnvelopeFollower::new(10.0, 600.0, 48_000.0);
            let mut v2 = 0.0;
            for _ in 0..5000 {
                v2 = ef.process(0.2);
            }
            assert!(v2 > 0.18 && v2 < 0.22);
        }
    }

    #[test]
    fn envelope_follower_is_finite_for_the_lifetime_of_a_signal() {
        let mut ef = EnvelopeFollower::new(10.0, 600.0, 48_000.0);
        for i in 0..1000 {
            let v = ef.process(libm::sinf(i as f32 * 0.01));
            assert!(v.is_finite(), "non-finite at {i}: {v}");
        }
    }
}
