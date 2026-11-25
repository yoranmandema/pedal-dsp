//! Envelopes: ADSR (footswitch / gate driven) and an envelope follower
//! (extracts the amplitude of the input signal, for tremolo/auto-wah).

use crate::util::clamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdsrState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub struct Adsr {
    pub attack_s: f32,
    pub decay_s: f32,
    /// Sustain level, 0..1.
    pub sustain: f32,
    pub release_s: f32,
    sample_rate: f32,
    state: AdsrState,
    value: f32,
    release_start: f32,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            attack_s: 0.005,
            decay_s: 0.15,
            sustain: 0.7,
            release_s: 0.25,
            sample_rate,
            state: AdsrState::Idle,
            value: 0.0,
            release_start: 0.0,
        }
    }

    /// Start (or restart) the attack.
    pub fn trigger(&mut self) {
        self.state = AdsrState::Attack;
    }

    /// Begin the release phase.
    pub fn release(&mut self) {
        if self.value > 0.0 {
            self.release_start = self.value;
            self.state = AdsrState::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.state != AdsrState::Idle
    }

    pub fn state(&self) -> AdsrState {
        self.state
    }

    #[inline]
    pub fn process(&mut self) -> f32 {
        let fs = self.sample_rate;
        match self.state {
            AdsrState::Idle => {
                self.value = 0.0;
            }
            AdsrState::Attack => {
                let inc = 1.0 / (self.attack_s * fs).max(1.0);
                self.value += inc;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.state = AdsrState::Decay;
                }
            }
            AdsrState::Decay => {
                let dec = (1.0 - self.sustain) / (self.decay_s * fs).max(1.0);
                self.value -= dec;
                if self.value <= self.sustain {
                    self.value = self.sustain;
                    self.state = AdsrState::Sustain;
                }
            }
            AdsrState::Sustain => {
                self.value = self.sustain;
            }
            AdsrState::Release => {
                let rel = self.release_start / (self.release_s * fs).max(1.0);
                self.value -= rel;
                if self.value <= 0.0 {
                    self.value = 0.0;
                    self.state = AdsrState::Idle;
                }
            }
        }
        self.value
    }

    pub fn reset(&mut self) {
        self.state = AdsrState::Idle;
        self.value = 0.0;
    }

    pub fn set_attack(&mut self, s: f32) {
        self.attack_s = s.max(0.001);
    }

    pub fn set_decay(&mut self, s: f32) {
        self.decay_s = s.max(0.001);
    }

    pub fn set_sustain(&mut self, level: f32) {
        self.sustain = clamp(level, 0.0, 1.0);
    }

    pub fn set_release(&mut self, s: f32) {
        self.release_s = s.max(0.001);
    }
}

/// Tracks the absolute amplitude of the input with separate attack/release
/// time constants. Output is 0..~1 for a full-scale signal.
#[derive(Clone, Copy)]
pub struct EnvelopeFollower {
    attack_coef: f32,
    release_coef: f32,
    value: f32,
}

impl EnvelopeFollower {
    pub fn new(attack_ms: f32, release_ms: f32, sample_rate: f32) -> Self {
        Self {
            attack_coef: crate::util::smoothing_coef_ms(attack_ms, sample_rate),
            release_coef: crate::util::smoothing_coef_ms(release_ms, sample_rate),
            value: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let abs = x.abs();
        let coef = if abs > self.value {
            self.attack_coef
        } else {
            self.release_coef
        };
        self.value += (abs - self.value) * coef;
        self.value
    }

    pub fn set_timeconstants(&mut self, attack_ms: f32, release_ms: f32, sample_rate: f32) {
        self.attack_coef = crate::util::smoothing_coef_ms(attack_ms, sample_rate);
        self.release_coef = crate::util::smoothing_coef_ms(release_ms, sample_rate);
    }

    pub fn reset(&mut self) {
        self.value = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adsr_reaches_sustain() {
        let mut a = Adsr::new(48_000.0);
        a.set_attack(0.01);
        a.set_decay(0.1);
        a.set_sustain(0.5);
        a.trigger();
        for _ in 0..48_000 {
            a.process();
        }
        assert_eq!(a.state(), AdsrState::Sustain);
        assert!((a.process() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn adsr_releases_to_zero() {
        let mut a = Adsr::new(48_000.0);
        a.set_attack(0.001);
        a.set_release(0.05);
        a.trigger();
        for _ in 0..480 {
            a.process();
        }
        a.release();
        for _ in 0..48_000 {
            a.process();
        }
        assert_eq!(a.state(), AdsrState::Idle);
        assert_eq!(a.process(), 0.0);
    }

    #[test]
    fn follower_tracks_amplitude() {
        let mut f = EnvelopeFollower::new(1.0, 10.0, 48_000.0);
        let mut peak: f32 = 0.0;
        for i in 0..4800 {
            let x = libm::sinf(i as f32 * 0.05);
            peak = peak.max(f.process(x));
        }
        assert!(peak > 0.85, "follower peak {peak}");
        assert!(peak < 1.1, "follower overshoot {peak}");
    }

    #[test]
    fn follower_rejects_dc_free_signal() {
        let mut f = EnvelopeFollower::new(1.0, 10.0, 48_000.0);
        for i in 0..4800 {
            f.process(if i % 2 == 0 { 1.0 } else { -1.0 });
        }
        assert!(f.process(0.0) > 0.9);
    }
}
