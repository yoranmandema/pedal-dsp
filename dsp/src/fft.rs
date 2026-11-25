//! In-place iterative radix-2 FFT.
//!
//! Kept separate from the pitch shifter so it can be tested on its own and
//! reused.  `N` must be a power of two.  Only the tables are stored; the caller
//! supplies the scratch buffers, so nothing here allocates.
//!
//! The transform is a plain decimation-in-time Cooley-Tukey with a bit-reversal
//! permutation.  It is not the fastest possible FFT, but at the sizes a pedal
//! can afford (N = 512..2048, a few hundred transforms a second) it is a few
//! percent of a modern desktop CPU and far simpler to audit than a split-radix one.
//!
//! Note on sizing: this crate targets a toolchain without
//! `generic_const_exprs`, so array lengths must use the bare const parameter.
//! The twiddle table is therefore `[f32; N]` even though only the first `N/2`
//! entries are used; the half-sized phase arrays in [`crate::pitch_vocoder`]
//! do the same.

/// Bit-reversed iterative radix-2 FFT of size `N` (power of two).
pub struct Fft<const N: usize> {
    rev: [u16; N],
    /// `tw_re[k]`/`tw_im[k]` = exp(-2*pi*i*k/N).  Only `0..N/2` is meaningful.
    tw_re: [f32; N],
    tw_im: [f32; N],
}

impl<const N: usize> Default for Fft<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Fft<N> {
    /// Build the bit-reversal permutation and the twiddle table.
    ///
    /// # Panics
    /// Panics if `N` is not a power of two.
    pub fn new() -> Self {
        assert!(N.is_power_of_two(), "Fft<N> requires N to be a power of two");
        assert!(N >= 2, "Fft<N> requires N >= 2");
        let bits = N.trailing_zeros();
        let mut rev = [0u16; N];
        for i in 0..N {
            let mut r = 0u32;
            for b in 0..bits {
                if i & (1 << b) != 0 {
                    r |= 1 << (bits - 1 - b);
                }
            }
            rev[i] = r as u16;
        }
        let mut tw_re = [0.0f32; N];
        let mut tw_im = [0.0f32; N];
        for k in 0..N {
            let theta = -2.0 * core::f32::consts::PI * k as f32 / N as f32;
            tw_re[k] = libm::cosf(theta);
            tw_im[k] = libm::sinf(theta);
        }
        Self {
            rev,
            tw_re,
            tw_im,
        }
    }

    /// Forward complex FFT, in place.
    pub fn transform(&self, re: &mut [f32; N], im: &mut [f32; N]) {
        // Bit-reversal permutation.
        for i in 0..N {
            let j = self.rev[i] as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        // Butterflies.
        let mut len = 2usize;
        while len <= N {
            let half = len / 2;
            let step = N / len;
            let mut i = 0;
            while i < N {
                let mut k = 0;
                while k < half {
                    let tw = k * step;
                    let wr = self.tw_re[tw];
                    let wi = self.tw_im[tw];
                    let a = i + k;
                    let b = a + half;
                    let xr = re[b] * wr - im[b] * wi;
                    let xi = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - xr;
                    im[b] = im[a] - xi;
                    re[a] += xr;
                    im[a] += xi;
                    k += 1;
                }
                i += len;
            }
            len <<= 1;
        }
    }

    /// Inverse complex FFT, in place (scaled by 1/N).
    pub fn inverse(&self, re: &mut [f32; N], im: &mut [f32; N]) {
        for x in im.iter_mut() {
            *x = -*x;
        }
        self.transform(re, im);
        let scale = 1.0 / N as f32;
        for x in re.iter_mut() {
            *x *= scale;
        }
        for x in im.iter_mut() {
            *x = -*x * scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_gives_flat_magnitude() {
        let fft = Fft::<16>::new();
        let mut re = [0.0f32; 16];
        let mut im = [0.0f32; 16];
        re[0] = 1.0;
        fft.transform(&mut re, &mut im);
        for k in 0..16 {
            let m = (re[k] * re[k] + im[k] * im[k]).sqrt();
            assert!((m - 1.0).abs() < 1e-4, "bin {k} magnitude {m}");
        }
    }

    #[test]
    fn sine_peaks_at_the_right_bins() {
        let fft = Fft::<64>::new();
        let mut re = [0.0f32; 64];
        let mut im = [0.0f32; 64];
        // 4 cycles over 64 samples -> bins 4 and 60.
        for n in 0..64 {
            re[n] = libm::sinf(2.0 * core::f32::consts::PI * 4.0 * n as f32 / 64.0);
        }
        fft.transform(&mut re, &mut im);
        let m4 = (re[4] * re[4] + im[4] * im[4]).sqrt();
        let m0 = (re[0] * re[0] + im[0] * im[0]).sqrt();
        assert!(m4 > 10.0, "bin 4 magnitude {m4}");
        assert!(m0 < 1e-3, "DC magnitude {m0}");
    }

    #[test]
    fn round_trip_recovers_the_input() {
        let fft = Fft::<32>::new();
        let mut re = [0.0f32; 32];
        let mut im = [0.0f32; 32];
        for n in 0..32 {
            re[n] = libm::sinf(n as f32 * 0.3) + 0.5 * libm::cosf(n as f32 * 0.7);
        }
        let orig = re;
        fft.transform(&mut re, &mut im);
        fft.inverse(&mut re, &mut im);
        for n in 0..32 {
            assert!(
                (re[n] - orig[n]).abs() < 1e-4,
                "sample {n}: {} vs {}",
                re[n],
                orig[n]
            );
        }
    }

    #[test]
    fn dc_gives_only_bin_zero() {
        let fft = Fft::<8>::new();
        let mut re = [1.0f32; 8];
        let mut im = [0.0f32; 8];
        fft.transform(&mut re, &mut im);
        assert!((re[0] - 8.0).abs() < 1e-4);
        for k in 1..8 {
            assert!(re[k].abs() < 1e-4 && im[k].abs() < 1e-4, "bin {k}");
        }
    }
}
