//! `pedal-dsp` — no_std audio DSP primitives for embedded guitar pedals.
//!
//! Everything here is written against `core` only (plus `libm` for the
//! transcendental functions used at *initialisation* time). The per-sample
//! hot path uses plain `f32` arithmetic for portability and low cost.
//!
//! Modules:
//! - [`biquad`]  — the workhorse filter (RBJ cookbook + transposed DF2)
//! - [`svf`]     — state-variable filter (LP/BP/HP/notch in one)
//! - [`fir`]     — windowed-sinc FIR, used for oversampling
//! - [`oversample`] — 2x/4x oversampling to kill aliasing from nonlinearities
//! - [`waveshape`] — distortion / rectification / bit-crush
//! - [`delay`]   — fractional delay lines, feedback delay, chorus
//! - [`fft`]     — radix-2 FFT used by the phase vocoders
//! - [`pitch_vocoder`] — timing-preserving pitch shifter (time-stretch + resample)
//! - [`pitch_remap`] — simpler bin-remap pitch shifter (aliases at some frequencies)
//! - [`envelope`] — ADSR and envelope follower
//! - [`lfo`]     — modulation sources
//! - [`reverb`]  — Freeverb (combs + allpasses)
//! - [`dc_blocker`] — remove DC offset before/after processing
//!
//! Every processor follows the same shape: `new`/`default`, `process`, `reset`.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

pub mod biquad;
pub mod chain;
pub mod dc_blocker;
pub mod delay;
pub mod envelope;
pub mod fft;
pub mod fir;
pub mod lfo;
pub mod oversample;
pub mod pitch_remap;
pub mod pitch_vocoder;
pub mod reverb;
pub mod resample;
pub mod svf;
pub mod util;
pub mod waveshape;
pub mod fuzz;
pub mod one_pole;

pub use biquad::{Biquad, BiquadKind, StereoBiquad};
pub use chain::EffectChain;
pub use dc_blocker::DcBlocker;
pub use delay::{DelayLine, FeedbackDelay, PitchMode, PitchShifter};
pub use envelope::{Adsr, EnvelopeFollower};
pub use fft::Fft;
pub use lfo::{Lfo, LfoShape};
pub use pitch_remap::PitchRemap;
pub use pitch_vocoder::{PitchVocoder, PITCH_VOCODER_SIZE};
pub use oversample::{Downsampler2, Oversampler4, Upsampler2};
pub use reverb::Freeverb;
pub use resample::{out_len as resample_out_len, resample};
pub use svf::{Svf, SvfOutput};
pub use waveshape::{Shaper, ShaperKind};

/// Default sample rate the examples and tests assume.
pub const DEFAULT_SAMPLE_RATE: f32 = 48_000.0;
