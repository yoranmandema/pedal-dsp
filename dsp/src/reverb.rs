//! Schroeder/Freeverb reverb: parallel comb filters followed by series
//! allpass diffusers.
//!
//! Stereo in, stereo out. Tunings are the classic 44.1 kHz Freeverb values
//! scaled to the requested sample rate, so the const-generic buffer sizes
//! must be large enough for the chosen rate:
//!
//! ```ignore
//! // 48 kHz: longest comb is 1760 samples, longest allpass 605
//! type Reverb48k = Freeverb<1760, 605>;
//! ```
//!
//! Memory cost is `2 * (8*COMB + 4*ALLPASS) * 4` bytes — 132 KB at 48 kHz.
//! That is fine for a desktop-class target, but would need a smaller
//! topology on a constrained embedded device.

use crate::util::clamp;

pub const NUM_COMBS: usize = 8;
pub const NUM_ALLPASSES: usize = 4;

/// Classic Freeverb comb tunings at 44.1 kHz.
const COMB_TUNINGS_44K: [usize; NUM_COMBS] =
    [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
/// Classic Freeverb allpass tunings at 44.1 kHz.
const ALLPASS_TUNINGS_44K: [usize; NUM_ALLPASSES] = [556, 441, 341, 225];

struct Comb<const N: usize> {
    buffer: [f32; N],
    pos: usize,
    size: usize,
    filter_state: f32,
    feedback: f32,
    damp: f32,
}

impl<const N: usize> Comb<N> {
    fn new(size: usize, feedback: f32, damp: f32) -> Self {
        Self {
            buffer: [0.0f32; N],
            pos: 0,
            size: size.min(N),
            filter_state: 0.0,
            feedback,
            damp,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.pos];
        // Damped feedback (low-pass in the loop).
        self.filter_state = output * (1.0 - self.damp) + self.filter_state * self.damp;
        self.buffer[self.pos] = input + self.filter_state * self.feedback;
        self.pos += 1;
        if self.pos >= self.size {
            self.pos = 0;
        }
        output
    }

    fn clear(&mut self) {
        self.buffer = [0.0f32; N];
        self.pos = 0;
        self.filter_state = 0.0;
    }

    fn set_params(&mut self, feedback: f32, damp: f32) {
        self.feedback = feedback;
        self.damp = damp;
    }
}

struct Allpass<const N: usize> {
    buffer: [f32; N],
    pos: usize,
    size: usize,
}

impl<const N: usize> Allpass<N> {
    fn new(size: usize) -> Self {
        Self {
            buffer: [0.0f32; N],
            pos: 0,
            size: size.min(N),
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buffer[self.pos];
        let output = -input + buf_out;
        self.buffer[self.pos] = input + buf_out * 0.5;
        self.pos += 1;
        if self.pos >= self.size {
            self.pos = 0;
        }
        output
    }

    fn clear(&mut self) {
        self.buffer = [0.0f32; N];
        self.pos = 0;
    }
}

pub struct Freeverb<const COMB: usize, const ALLPASS: usize> {
    combs_l: [Comb<COMB>; NUM_COMBS],
    combs_r: [Comb<COMB>; NUM_COMBS],
    aps_l: [Allpass<ALLPASS>; NUM_ALLPASSES],
    aps_r: [Allpass<ALLPASS>; NUM_ALLPASSES],
    room_size: f32,
    damping: f32,
    wet: f32,
    dry: f32,
    width: f32,
}

impl Freeverb<1760, 605> {
    /// 48 kHz stereo reverb (132 KB of buffers).
    pub fn new_48k() -> Self {
        Self::new(48_000.0)
    }
}

impl<const COMB: usize, const ALLPASS: usize> Freeverb<COMB, ALLPASS> {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let comb_sizes: [usize; NUM_COMBS] = core::array::from_fn(|i| {
            (COMB_TUNINGS_44K[i] as f32 * scale) as usize
        });
        let ap_sizes: [usize; NUM_ALLPASSES] = core::array::from_fn(|i| {
            (ALLPASS_TUNINGS_44K[i] as f32 * scale) as usize
        });

        let mut rv = Self {
            combs_l: core::array::from_fn(|i| Comb::new(comb_sizes[i], 0.84, 0.2)),
            combs_r: core::array::from_fn(|i| Comb::new(comb_sizes[i], 0.84, 0.2)),
            aps_l: core::array::from_fn(|i| Allpass::new(ap_sizes[i])),
            aps_r: core::array::from_fn(|i| Allpass::new(ap_sizes[i])),
            room_size: 0.5,
            damping: 0.5,
            wet: 0.3,
            dry: 0.7,
            width: 1.0,
        };
        rv.recompute();
        rv
    }

    fn recompute(&mut self) {
        let feedback = self.room_size * 0.28 + 0.7;
        let damp = self.damping * 0.4;
        for c in self.combs_l.iter_mut() {
            c.set_params(feedback, damp);
        }
        for c in self.combs_r.iter_mut() {
            c.set_params(feedback, damp);
        }
    }

    /// Room size 0..1 (higher = longer decay).
    pub fn set_room_size(&mut self, v: f32) {
        self.room_size = clamp(v, 0.0, 1.0);
        self.recompute();
    }

    /// High-frequency damping 0..1.
    pub fn set_damping(&mut self, v: f32) {
        self.damping = clamp(v, 0.0, 1.0);
        self.recompute();
    }

    pub fn set_wet(&mut self, v: f32) {
        self.wet = clamp(v, 0.0, 1.0);
    }

    pub fn set_dry(&mut self, v: f32) {
        self.dry = clamp(v, 0.0, 1.0);
    }

    pub fn set_width(&mut self, v: f32) {
        self.width = clamp(v, 0.0, 1.0);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        for c in self.combs_l.iter_mut() {
            out_l += c.process(input_l);
        }
        for c in self.combs_r.iter_mut() {
            out_r += c.process(input_r);
        }
        for a in self.aps_l.iter_mut() {
            out_l = a.process(out_l);
        }
        for a in self.aps_r.iter_mut() {
            out_r = a.process(out_r);
        }

        let l = out_l * self.wet + input_l * self.dry;
        let r = out_r * self.wet + input_r * self.dry;

        // Mid/side width control.
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5 * self.width;
        (mid + side, mid - side)
    }

    pub fn clear(&mut self) {
        for c in self.combs_l.iter_mut() {
            c.clear();
        }
        for c in self.combs_r.iter_mut() {
            c.clear();
        }
        for a in self.aps_l.iter_mut() {
            a.clear();
        }
        for a in self.aps_r.iter_mut() {
            a.clear();
        }
    }
}

/// Mono Freeverb: identical topology and tunings, one channel, half the
/// memory (66 KB at 48 kHz instead of 132 KB).
pub struct FreeverbMono<const COMB: usize, const ALLPASS: usize> {
    combs: [Comb<COMB>; NUM_COMBS],
    aps: [Allpass<ALLPASS>; NUM_ALLPASSES],
    room_size: f32,
    damping: f32,
    wet: f32,
    dry: f32,
}

impl FreeverbMono<1760, 605> {
    pub fn new_48k() -> Self {
        Self::new(48_000.0)
    }
}

impl<const COMB: usize, const ALLPASS: usize> FreeverbMono<COMB, ALLPASS> {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let comb_sizes: [usize; NUM_COMBS] =
            core::array::from_fn(|i| (COMB_TUNINGS_44K[i] as f32 * scale) as usize);
        let ap_sizes: [usize; NUM_ALLPASSES] =
            core::array::from_fn(|i| (ALLPASS_TUNINGS_44K[i] as f32 * scale) as usize);

        let mut rv = Self {
            combs: core::array::from_fn(|i| Comb::new(comb_sizes[i], 0.84, 0.2)),
            aps: core::array::from_fn(|i| Allpass::new(ap_sizes[i])),
            room_size: 0.5,
            damping: 0.5,
            wet: 0.3,
            dry: 0.7,
        };
        rv.recompute();
        rv
    }

    fn recompute(&mut self) {
        let feedback = self.room_size * 0.28 + 0.7;
        let damp = self.damping * 0.4;
        for c in self.combs.iter_mut() {
            c.set_params(feedback, damp);
        }
    }

    pub fn set_room_size(&mut self, v: f32) {
        self.room_size = clamp(v, 0.0, 1.0);
        self.recompute();
    }

    pub fn set_damping(&mut self, v: f32) {
        self.damping = clamp(v, 0.0, 1.0);
        self.recompute();
    }

    pub fn set_wet(&mut self, v: f32) {
        self.wet = clamp(v, 0.0, 1.0);
    }

    pub fn set_dry(&mut self, v: f32) {
        self.dry = clamp(v, 0.0, 1.0);
    }

    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let mut out = 0.0f32;
        for c in self.combs.iter_mut() {
            out += c.process(input);
        }
        for a in self.aps.iter_mut() {
            out = a.process(out);
        }
        out * self.wet + input * self.dry
    }

    pub fn clear(&mut self) {
        for c in self.combs.iter_mut() {
            c.clear();
        }
        for a in self.aps.iter_mut() {
            a.clear();
        }
    }
}

/// Convenience alias for a 48 kHz stereo reverb.
pub type Reverb48k = Freeverb<1760, 605>;

/// Convenience alias for a 48 kHz mono reverb.
pub type Reverb48kMono = FreeverbMono<1760, 605>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverb_is_quiet_for_silence() {
        let mut r = Reverb48k::new_48k();
        r.set_wet(1.0);
        r.set_dry(0.0);
        for _ in 0..1000 {
            assert!(r.process(0.0, 0.0).0.abs() < 1e-9);
        }
    }

    #[test]
    fn impulse_decays() {
        let mut r = Reverb48k::new_48k();
        r.set_room_size(0.5);
        r.set_wet(1.0);
        r.set_dry(0.0);
        let mut peak = 0.0f32;
        let mut last = 0.0f32;
        for i in 0..48_000 {
            let (l, _) = r.process(if i == 0 { 1.0 } else { 0.0 }, 0.0);
            peak = peak.max(l.abs());
            last = l.abs();
        }
        assert!(peak > 0.01, "no output: peak {peak}");
        assert!(last < peak * 0.2, "did not decay: last {last}, peak {peak}");
    }

    #[test]
    fn larger_room_decays_slower() {
        fn decay_time(room: f32) -> usize {
            let mut r = Reverb48k::new_48k();
            r.set_room_size(room);
            r.set_wet(1.0);
            r.set_dry(0.0);
            // Measure the decay of the block RMS, so the individual comb
            // echoes (which are separated by silence) do not confuse it.
            let block = 4800usize;
            let mut max_rms = 0.0f32;
            let mut acc = 0.0f32;
            let mut blocks = 0usize;
            let mut n = 0usize;
            loop {
                let input = if n == 0 { 1.0 } else { 0.0 };
                let (l, _) = r.process(input, 0.0);
                acc += l * l;
                n += 1;
                if n % block == 0 {
                    let rms = (acc / block as f32).sqrt();
                    acc = 0.0;
                    if blocks == 0 {
                        max_rms = rms;
                    }
                    blocks += 1;
                    if blocks > 1 && rms < max_rms * 1e-3 {
                        break;
                    }
                }
                if n > 400_000 {
                    break;
                }
            }
            n
        }
        let small = decay_time(0.2);
        let large = decay_time(0.95);
        assert!(large > small, "large room {large} vs small {small}");
    }
}
