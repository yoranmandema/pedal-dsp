//! The effect chain: the musical heart of the pedal.
//!
//! Signal flow (mono):
//!
//!   in -> DC block -> distortion -> tone stack -> delay -> reverb -> DC block -> out
//!
//! The pedal is a **mono** device, so the whole chain runs on a single
//! channel.  That is not just simpler: it halves the delay-line and reverb
//! memory, which is what makes a ~1.25 s delay affordable on a small target such as the Pi 5.
//!
//! Parameters are plain `f32` fields so the control loop can update them
//! from pots/footswitch without allocating.
//!
//! This module lives in `pedal-dsp` (not in the firmware crate) so the host
//! reference model in `examples/wav_process.rs` runs the *exact same* code
//! as the MCU.  No `std`, no allocator.

use crate::reverb::Reverb48kMono;
use crate::{
    Biquad, BiquadKind, DcBlocker, EnvelopeFollower, FeedbackDelay, PitchMode, Shaper, ShaperKind,
};

/// Maximum delay line length in samples.
///
/// 60000 samples = 1.25 s at 48 kHz (1.36 s at 44.1 kHz).  The line is
/// `f32`, so this costs 240 KB.
pub const MAX_DELAY_SAMPLES: usize = 60000;

/// Pitch-shifter buffer length in samples.
///
/// 2400 samples = 50 ms at 48 kHz.  The two crossfaded read heads sit between
/// N/2 and N samples behind the write head, so the latency is ~25-50 ms
/// depending on where in the crossfade the signal lands (the measured impulse
/// peak is at ~N = 50 ms).  A shorter buffer lowers the latency but wraps the
/// read head faster, which granulates the shift; a longer one smears it,
/// because the two heads are only N/2 apart and the crossfade completes in
/// N/|rate-1| samples.
///
/// The commercial reference this was measured against (Neural DSP Parallax X
/// transpose, black-boxed over its AudioUnit) sits at ~12-20 ms, so this
/// design is roughly 2-3x slower but musically the same idea.
pub const PITCH_BUFFER_SAMPLES: usize = 2400;

pub struct EffectChain {
    sample_rate: f32,

    dc_in: DcBlocker,
    dc_out: DcBlocker,

    // Distortion
    shaper: Shaper,

    // Tone stack
    tone_low: Biquad,
    tone_high: Biquad,

    // Time-based
    delay: FeedbackDelay<MAX_DELAY_SAMPLES, PITCH_BUFFER_SAMPLES>,
    reverb: Reverb48kMono,

    // Dynamics (envelope follower drives an optional auto-gain)
    follower: EnvelopeFollower,

    // ---- parameters (written by the control loop) ----
    /// Distortion drive, 0.1 .. 20
    pub drive: f32,
    /// Distortion output level, 0 .. 4
    pub distortion_level: f32,
    /// Dry/wet mix of the distortion, 0..1
    pub distortion_mix: f32,
    /// Tone, -1 .. 1 (maps to -12 .. +12 dB on both shelves)
    pub tone: f32,
    /// Master output level, 0 .. 2
    pub level: f32,
    /// Delay time in ms
    pub delay_ms: f32,
    /// Delay mix, 0..1
    pub delay_mix: f32,
    /// Delay feedback, 0..0.95
    pub delay_feedback: f32,
    /// Delay pitch swing in cents, -2400..+2400 (0 = off)
    pub pitch_cents: f32,
    /// Glide time toward a new pitch, in ms (0 = instant)
    pub pitch_glide_ms: f32,
    /// Continuous pitch sweep rate, cents per second
    pub pitch_rate: f32,
    /// Target-and-hold or continuous sweep.
    pub pitch_mode: PitchMode,
    /// Trigger gate for the sweep (hold to sweep, release to return).
    pub sweep_gate: bool,
    /// Delay pitch jitter amount, 0..1
    pub jitter: f32,
    /// Reverb mix, 0..1
    pub reverb_mix: f32,
    /// Effect bypass
    pub enabled: bool,
}

impl EffectChain {
    pub fn new(sample_rate: f32) -> Self {
        let mut chain = Self {
            sample_rate,
            dc_in: DcBlocker::new(20.0, sample_rate),
            dc_out: DcBlocker::new(20.0, sample_rate),
            shaper: Shaper::new(ShaperKind::Tanh),
            tone_low: Biquad::make(BiquadKind::LowShelf, sample_rate, 200.0, 0.707, 0.0),
            tone_high: Biquad::make(BiquadKind::HighShelf, sample_rate, 4_000.0, 0.707, 0.0),
            delay: FeedbackDelay::new(sample_rate, 250.0),
            reverb: Reverb48kMono::new_48k(),
            follower: EnvelopeFollower::new(2.0, 50.0, sample_rate),
            drive: 3.0,
            distortion_level: 0.8,
            distortion_mix: 1.0,
            tone: 0.0,
            level: 0.9,
            delay_ms: 250.0,
            delay_mix: 0.0,
            delay_feedback: 0.35,
            pitch_cents: 0.0,
            pitch_glide_ms: 0.0,
            pitch_rate: 0.0,
            pitch_mode: PitchMode::Target,
            sweep_gate: true,
            jitter: 0.0,
            reverb_mix: 0.0,
            enabled: true,
        };
        chain.apply_static_params();
        chain
    }

    fn apply_static_params(&mut self) {
        self.shaper.set_drive(self.drive);
        self.shaper.set_level(self.distortion_level);
        self.shaper.set_mix(self.distortion_mix);

        let tone_db = self.tone * 12.0;
        self.tone_low
            .design(BiquadKind::LowShelf, self.sample_rate, 200.0, 0.707, tone_db);
        self.tone_high
            .design(BiquadKind::HighShelf, self.sample_rate, 4_000.0, 0.707, tone_db);

        self.delay.set_delay_ms(self.delay_ms);
        self.delay.set_feedback(self.delay_feedback);
        self.delay.set_pitch_cents(self.pitch_cents);
        self.delay.set_pitch_glide_ms(self.pitch_glide_ms);
        self.delay.set_pitch_rate(self.pitch_rate);
        self.delay.set_pitch_mode(self.pitch_mode);
        self.delay.set_sweep_gate(self.sweep_gate);
        self.delay.set_jitter(self.jitter);
        self.delay.mix = self.delay_mix;

        self.reverb.set_wet(self.reverb_mix);
        self.reverb.set_dry(1.0 - self.reverb_mix);
    }

    /// Call after changing any parameter.
    pub fn update_params(&mut self) {
        self.apply_static_params();
    }

    /// Swap the distortion type.  The shaper is recreated (its internal
    /// oversampling state resets), which is acceptable -- the user is
    /// changing character.  Do NOT call this per sample: it would silently
    /// disable oversampling and the distortion would alias.
    pub fn set_shaper(&mut self, kind: ShaperKind) {
        self.shaper = Shaper::new(kind);
        self.apply_static_params();
    }

    /// Process one mono sample.
    #[inline]
    pub fn process_mono(&mut self, x_in: f32) -> f32 {
        // Input DC removal.
        let x = self.dc_in.process(x_in);

        if !self.enabled {
            // Still limited: the bypass path would otherwise let a hot input
            // times a master > 1 clip the DAC.
            return soft_limit(x * self.level);
        }

        // Distortion
        let x = self.shaper.process(x);

        // Tone stack
        let x = self.tone_low.process(x);
        let x = self.tone_high.process(x);

        // Delay
        let x = self.delay.process(x);

        // Reverb
        let x = self.reverb.process(x);

        // Output DC removal + master level
        let x = self.dc_out.process(x) * self.level;

        // Soft-limit the output so the codec never sees a clipped word.
        soft_limit(x)
    }

    /// Stereo convenience: the pedal is mono, so both channels are identical.
    #[inline]
    pub fn process_stereo(&mut self, l_in: f32, r_in: f32) -> (f32, f32) {
        let _ = r_in;
        let y = self.process_mono(l_in);
        (y, y)
    }

    /// Envelope follower, useful for tuner/dynamics ideas.
    #[inline]
    pub fn envelope(&mut self, x: f32) -> f32 {
        self.follower.process(x)
    }

    pub fn reset(&mut self) {
        self.dc_in.reset();
        self.dc_out.reset();
        self.shaper.reset();
        self.tone_low.reset();
        self.tone_high.reset();
        self.delay.clear();
        self.reverb.clear();
        self.follower.reset();
    }
}

/// Gentle output limiter.
///
/// This must be a SOFT clipper: a hard `if x > 1 { 1 }` corner is a
/// nonlinearity too, and it folds harmonics back into the audio band,
/// which is exactly the aliasing we oversample to avoid.
#[inline]
fn soft_limit(x: f32) -> f32 {
    // Cubic saturation: unity slope at 0, reaches +/-1 with zero slope,
    // so the knee is smooth and adds no aliasing of its own.
    let x = if x > 1.0 {
        1.0
    } else if x < -1.0 {
        -1.0
    } else {
        x
    };
    1.5 * x - 0.5 * x * x * x
}
