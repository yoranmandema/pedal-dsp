//! A concrete block library built on `pedal-dsp` primitives.
//!
//! These are real, working blocks — not stubs. They demonstrate the plugin
//! architecture and give the host simulator something to run.
use crate::prelude::*;
use crate::overdrive::{InputHpf, ToneStage};
use pedal_dsp::{
    Biquad, BiquadKind, EnvelopeFollower, Freeverb, Lfo, PitchShifter, resample,
    delay::DelayLine, util::{clamp, smoothing_coef_ms},
};
use std::sync::atomic::{AtomicU32, Ordering};

/// Maximum number of samples processed per block (≤ the executor's max block
/// size). All block-local state that mirrors `BlockCtx` is sized against this.
const MAX_BLOCK: usize = 64;
/// A simple gain stage, in dB.
pub struct Gain {
    gain_db: f32,
}
impl Default for Gain {
    fn default() -> Self {
        Self { gain_db: 0.0 }
    }
}
impl Block for Gain {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "util.gain",
            name: "Gain",
            category: Category::Utility,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![ParamSpec {
                id: "gain",
                name: "Gain",
                unit: Unit::Decibels,
                min: -60.0,
                max: 24.0,
                default: 0.0,
                curve: Curve::AmpGain,
            }],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        self.gain_db = ctx.param(0);
        let g = pedal_dsp::util::db_to_lin(self.gain_db);
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            output[i] = input[i] * g;
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
/// A three-band tone stack (low / mid / high), like an amp's.
pub struct ToneStack {
    bands: [(BiquadKind, Biquad, f32, f32); 3],
}
impl Default for ToneStack {
    fn default() -> Self {
        Self {
            bands: [
                (BiquadKind::LowShelf, Biquad::identity(), 200.0, 0.7),
                (BiquadKind::Peak, Biquad::identity(), 1000.0, 1.0),
                (BiquadKind::HighShelf, Biquad::identity(), 4000.0, 0.7),
            ],
        }
    }
}
impl Block for ToneStack {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "tone.stack",
            name: "Tone Stack",
            category: Category::Tone,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "low",  name: "Low",  unit: Unit::Decibels, min: -20.0, max: 20.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "mid",  name: "Mid",  unit: Unit::Decibels, min: -20.0, max: 20.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "high", name: "High", unit: Unit::Decibels, min: -20.0, max: 20.0, default: 0.0, curve: Curve::AmpGain },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        for (kind, band, f0, q) in &mut self.bands {
            *band = Biquad::make(*kind, sr, *f0, *q, 0.0);
        }
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let gains = [ctx.param(0), ctx.param(1), ctx.param(2)];
        for ((kind, band, f0, q), db) in self.bands.iter_mut().zip(gains) {
            band.design(*kind, ctx.sample_rate, *f0, *q, db);
        }
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let mut s = input[i];
            for (_, band, _, _) in self.bands.iter_mut() {
                s = band.process(s);
            }
            output[i] = s;
        }
        Ok(())
    }
    fn reset(&mut self) {
        for (_, band, _, _) in &mut self.bands {
            band.reset();
        }
    }
}
/// A soft-clipping overdrive.
pub struct Overdrive {
    drive: f32,
    level: f32,
}
impl Default for Overdrive {
    fn default() -> Self {
        Self { drive: 1.0, level: 1.0 }
    }
}
impl Block for Overdrive {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "dist.overdrive",
            name: "Overdrive",
            category: Category::Distortion,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "drive", name: "Drive", unit: Unit::Linear, min: 0.0, max: 10.0, default: 1.0, curve: Curve::Log },
                ParamSpec { id: "level", name: "Level", unit: Unit::Decibels, min: -30.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let drive = ctx.param(0);
        let level = pedal_dsp::util::db_to_lin(ctx.param(1));
        self.drive = drive;
        self.level = level;
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let x = input[i] * drive;
            let y = pedal_dsp::util::tanh_approx(x);
            output[i] = y * level;
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
/// A reverb, using `pedal-dsp`'s Freeverb.
/// Tube Screamer — block id `drive.tube_screamer`.
///
/// Stages, in order: input HPF (B1) → pre-EQ (B2: pre_mid 720 Hz + pre_bass +
/// pre_treble) → waveshaper (B3: Tube, scaled by drive) → post-tone (B2:
/// tone) → level (B6).
///
/// The Tube shaper is the asymmetric soft clip that captures the "diode
/// clipping in the feedback loop" character (design §1 B3). A real
/// delay-feedback loop is deferred to v2.
pub struct TubeScreamer {
    hpf: InputHpf,
    pre: ToneStage,
    shaper: Shaper,
    level: f32,
}

impl Default for TubeScreamer {
    fn default() -> Self {
        Self {
            hpf: InputHpf::new(48_000.0),
            pre: ToneStage::new(48_000.0),
            shaper: Shaper::new(ShaperKind::Tube),
            level: 1.0,
        }
    }
}

impl Block for TubeScreamer {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "drive.tube_screamer",
            name: "Tube Screamer",
            category: Category::Distortion,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "drive", name: "Drive", unit: Unit::Linear, min: 0.1, max: 10.0, default: 1.0, curve: Curve::Log,
                },
                ParamSpec {
                    id: "pre_mid", name: "Pre Mid", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "pre_bass", name: "Pre Bass", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "pre_treble", name: "Pre Treble", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "tone", name: "Tone", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "level", name: "Level", unit: Unit::Decibels, min: -30.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.hpf = InputHpf::new(sr);
        self.pre = ToneStage::new(sr);
        self.shaper = Shaper::new(ShaperKind::Tube);
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let drive = ctx.param(0);
        let pre_mid = ctx.param(1);
        let pre_bass = ctx.param(2);
        let pre_treble = ctx.param(3);
        let tone = ctx.param(4);
        let level = pedal_dsp::util::db_to_lin(ctx.param(5));

        // Update tone stage targets (no sr param — begin_block handles smoothing & rebuild)
        self.pre.set_pre_mid(pre_mid);
        self.pre.set_pre_bass(pre_bass);
        self.pre.set_pre_treble(pre_treble);
        self.pre.set_post_tone(tone);

        self.shaper.set_drive(drive);
        self.shaper.set_mix(1.0);
        self.shaper.set_level(1.0);

        // Advance tone smoothing & rebuild bins if needed (once per block)
        self.pre.begin_block(ctx.sample_rate);

        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let mut x = self.hpf.process(input[i]);
            x = self.pre.process(x);
            x = self.shaper.process(x);
            output[i] = x * level;
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.hpf.reset();
        self.pre.reset();
        self.shaper.reset();
    }

    fn latency_samples(&self) -> u32 {
        // B4: 4× oversampling adds ≈30 samples @48 kHz.
        30
    }
}

/// Precision Drive — block id `drive.precision_drive`.
///
/// Stages: input HPF → diode-feedback clipper (B3 DiodeClip + punch
/// envelope) → tone (B2: post_tone low shelf + presence high shelf) →
/// volume (B6).
///
/// "Punch" transient pump: `|x|` is smoothed by two one-poles in series
/// (one_pole.rs). `e = clamp(pump_env / 0.5, 0, 1)`; `drive_eff =
/// gain * (1 - tightness * e)`; the clipper is `DiodeClip` with drive
/// `drive_eff`, run 4× sampled.
pub struct PrecisionDrive {
    hpf: InputHpf,
    tone: ToneStage,
    ef: EnvelopeFollower,
    shaper: Shaper,
}

impl Default for PrecisionDrive {
    fn default() -> Self {
        Self {
            hpf: InputHpf::new(48_000.0),
            tone: ToneStage::new(48_000.0),
            ef: EnvelopeFollower::new(10.0, 600.0, 48_000.0),
            shaper: Shaper::new(ShaperKind::DiodeClip),
        }
    }
}

impl Block for PrecisionDrive {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "drive.precision_drive",
            name: "Precision Drive",
            category: Category::Distortion,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "gain", name: "Gain", unit: Unit::Linear, min: 0.1, max: 20.0, default: 2.0, curve: Curve::Log,
                },
                ParamSpec {
                    id: "tightness", name: "Punch", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.5, curve: Curve::Linear,
                },
                ParamSpec {
                    id: "attack", name: "Attack", unit: Unit::Milliseconds, min: 1.0, max: 200.0, default: 10.0, curve: Curve::Linear,
                },
                ParamSpec {
                    id: "release", name: "Release", unit: Unit::Milliseconds, min: 100.0, max: 3000.0, default: 600.0, curve: Curve::Linear,
                },
                ParamSpec {
                    id: "tone", name: "Tone", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "presence", name: "Presence", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "volume", name: "Volume", unit: Unit::Decibels, min: -30.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.hpf = InputHpf::new(sr);
        self.tone = ToneStage::new(sr);
        self.tone.prepare(sr);
        self.ef = EnvelopeFollower::new(10.0, 600.0, sr);
        self.shaper = Shaper::new(ShaperKind::DiodeClip);
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let gain = ctx.param(0);
        let tightness = ctx.param(1);
        let attack = ctx.param(2);
        let release = ctx.param(3);
        let tone = ctx.param(4);
        let presence = ctx.param(5);
        let volume = ctx.param(6);

        // Recompute the pump coefficients from the current (real) knobs.
        self.ef.set_timeconstants(attack, release, ctx.sample_rate);

        self.tone.set_post_tone(tone);
        self.tone.set_presence(presence);

        // Hoist volume conversion out of the hot loop.
        let volume_lin = pedal_dsp::util::db_to_lin(volume);

        // Advance tone smoothing & rebuild bins if needed (once per block)
        self.tone.begin_block(ctx.sample_rate);

        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let mut x = self.hpf.process(input[i]);
            // Punch: smooth |x| with the attack follower, then let the pump
            // decay with the release follower.
            let e = (self.ef.process(x.abs()) / 0.5).clamp(0.0, 1.0);
            let drive_eff = gain * (1.0 - tightness * e);
            self.shaper.set_drive(drive_eff);
            self.shaper.set_mix(1.0);
            self.shaper.set_level(1.0);
            x = self.shaper.process(x);
            x = self.tone.process(x);
            output[i] = x * volume_lin;
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.hpf.reset();
        self.tone.reset();
        self.ef.reset();
        self.shaper.reset();
    }

    fn latency_samples(&self) -> u32 {
        30
    }
}

/// Klon — block id `drive.klon`.
///
/// Transparent, symmetric soft clip: `y = softclip(x * (1 + feel*0.5))`
/// where `softclip(u) = u / (1 + |u|)` (ShaperKind::SoftClip).
///
/// The `tone` parameter is a flat gain (no tone filter), per design §2.3
/// which specifies no tone stage for the Klon.
pub struct Klon {
    shaper: Shaper,
}

impl Default for Klon {
    fn default() -> Self {
        Self {
            shaper: Shaper::new(ShaperKind::SoftClip),
        }
    }
}

impl Block for Klon {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "drive.klon",
            name: "Klon",
            category: Category::Distortion,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "drive", name: "Drive", unit: Unit::Linear, min: 0.1, max: 10.0, default: 1.0, curve: Curve::Log,
                },
                ParamSpec {
                    id: "feel", name: "Feel", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear,
                },
                ParamSpec {
                    id: "tone", name: "Tone", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "level", name: "Level", unit: Unit::Decibels, min: -30.0, max: 12.0, default: 0.0, curve: Curve::AmpGain,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        self.shaper = Shaper::new(ShaperKind::SoftClip);
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let drive = ctx.param(0);
        let feel = ctx.param(1);
        let tone = ctx.param(2);
        let level = pedal_dsp::util::db_to_lin(ctx.param(3));

        // Effective drive combines the drive knob with the feel knob.
        self.shaper.set_drive(drive * (1.0 + feel * 0.5));
        self.shaper.set_mix(1.0);
        self.shaper.set_level(1.0);

        // Convert tone (flat gain, per Klon spec) once per block.
        let tone_lin = pedal_dsp::util::db_to_lin(tone);

        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            // `tone` is a flat gain (transparent voicing), per the Klon spec.
            let y = self.shaper.process(input[i]) * tone_lin * level;
            // Denormal flush: tiny offset prevents subnormal accumulation.
            output[i] = if y.abs() < 1e-15 { 0.0 } else { y };
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.shaper.reset();
    }

    fn latency_samples(&self) -> u32 {
        30
    }
}

/// Big Muff — block id `drive.big_muff`.
///
/// Fuzz-ish drive with variable brightness. WaveShaper is `fuzz < 0.5 ? Tube
/// : Fuzz`, scaled by drive. The tone is a variable peaking filter at `f0`.
/// `blend` mixes the nonlinear output with a clean dry pass-through.
///
/// The dry path is delayed by the shaper's latency (≈30 samples) so `blend`
/// does not create comb filtering between dry and wet.
pub struct BigMuff {
    shaper: Shaper,
    tone: ToneStage,
    /// HPF at 20 Hz to remove DC bias from Fuzz shaper.
    hpf: InputHpf,
    /// Delay line for the dry path to match shaper latency (30 samples).
    dry_delay: DelayLine<{ MAX_BLOCK + 32 }>,
    blend: f32,
    level: f32,
    /// Current fuzz kind to avoid rebuilding shaper every block.
    fuzz_kind: ShaperKind,
}

impl Default for BigMuff {
    fn default() -> Self {
        Self {
            shaper: Shaper::new(ShaperKind::Tube),
            tone: ToneStage::new(48_000.0),
            hpf: InputHpf::new(48_000.0),
            dry_delay: DelayLine::new(),
            blend: 0.5,
            level: 1.0,
            fuzz_kind: ShaperKind::Tube,
        }
    }
}

impl Block for BigMuff {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "drive.big_muff",
            name: "Big Muff",
            category: Category::Distortion,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "drive", name: "Drive", unit: Unit::Linear, min: 0.1, max: 10.0, default: 1.0, curve: Curve::Log,
                },
                ParamSpec {
                    id: "fuzz", name: "Fuzz", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.5, curve: Curve::Linear,
                },
                ParamSpec {
                    id: "tone", name: "Tone", unit: Unit::Hertz, min: 200.0, max: 10000.0, default: 2000.0, curve: Curve::Log,
                },
                ParamSpec {
                    id: "blend", name: "Blend", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.5, curve: Curve::Linear,
                },
                ParamSpec {
                    id: "level", name: "Level", unit: Unit::Decibels, min: -24.0, max: 24.0, default: 0.0, curve: Curve::AmpGain,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, sr: f32, max_block: usize) -> Result<(), BlockError> {
        self.shaper = Shaper::new(ShaperKind::Tube);
        self.tone = ToneStage::new(sr);
        self.tone.prepare(sr);
        self.hpf = InputHpf::new(sr);
        // Buffer must hold the shaper latency (30) plus one block.
        const LATENCY: usize = 30;
        assert!(max_block + LATENCY <= MAX_BLOCK + 32, "BigMuff delay buffer too small");
        self.dry_delay.clear();
        self.fuzz_kind = ShaperKind::Tube;
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let drive = ctx.param(0);
        let fuzz = ctx.param(1);
        let tone_hz = ctx.param(2);
        let blend = ctx.param(3);
        let level = pedal_dsp::util::db_to_lin(ctx.param(4));

        // Variable brightness: the tone is a peaking filter whose centre
        // frequency tracks the `tone` knob.
        self.tone.set_bigmuff_tone(tone_hz, 0.0);

        // Advance tone smoothing & rebuild bins if needed (once per block)
        self.tone.begin_block(ctx.sample_rate);

        // Only rebuild shaper when fuzz crosses the 0.5 threshold (avoids clicks).
        let kind = if fuzz < 0.5 {
            ShaperKind::Tube
        } else {
            ShaperKind::Fuzz
        };
        if kind != self.fuzz_kind {
            self.shaper = Shaper::new(kind);
            self.fuzz_kind = kind;
        }
        self.shaper.set_drive(drive);
        self.shaper.set_mix(1.0);
        self.shaper.set_level(1.0);

        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];

        // Process block: shaper has ~30 sample latency (4× oversampling).
        // Delay dry path by same amount so `blend` doesn't comb-filter.
        for i in 0..ctx.n {
            let mut wet = self.shaper.process(input[i]);
            // HPF at 20 Hz to remove DC bias from Fuzz shaper.
            wet = self.hpf.process(wet);
            wet = self.tone.process(wet);
            // Delay the dry path by the shaper latency so blend is aligned.
            self.dry_delay.write(input[i]);
            let dry = self.dry_delay.read_at(30.0);
            output[i] = pedal_dsp::util::mix(dry, wet, blend) * level;
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.shaper.reset();
        self.tone.reset();
        self.hpf.reset();
        self.dry_delay.clear();
        self.fuzz_kind = ShaperKind::Tube;
    }

    fn latency_samples(&self) -> u32 {
        30
    }
}

/// A reverb, using `pedal-dsp`'s Freeverb.
pub struct Reverb {
    inner: Option<pedal_dsp::reverb::FreeverbMono<1760, 605>>,
    mix: f32,
}
impl Default for Reverb {
    fn default() -> Self {
        Self { inner: None, mix: 0.0 }
    }
}
impl Block for Reverb {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "time.reverb",
            name: "Reverb",
            category: Category::Time,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![ParamSpec { id: "mix", name: "Mix", unit: Unit::Percent, min: 0.0, max: 100.0, default: 30.0, curve: Curve::Linear }],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.inner = Some(pedal_dsp::reverb::FreeverbMono::<1760, 605>::new(sr));
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let mix = ctx.param(0) / 100.0;
        self.mix = mix;
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        let rev = self.inner.as_mut().ok_or(BlockError::NotPrepared("time.reverb"))?;
        for i in 0..ctx.n {
            let wet = rev.process(input[i]);
            output[i] = pedal_dsp::util::mix(input[i], wet, mix);
        }
        Ok(())
    }
    fn reset(&mut self) {
        if let Some(r) = &mut self.inner {
            r.clear();
        }
    }
}
/// An octaver: blends the input with one or more octave-shifted copies.
///
/// Provides three independent mix levels:
///
/// * `dry` — the dry (unprocessed) input.
/// * `oct_down` — one octave below the input.
/// * `oct_up` — one octave above the input.
///
/// Output is `dry*input + oct_down*down + oct_up*up`.  Setting the octave
/// levels to 0 (and `dry` to 1) gives pass-through; setting the octave levels
/// to 0 gives a dry-scaled pass-through at any `dry` level.
pub struct Octaver {
    /// Octave-down pitch shifter.
    shifter_down: PitchShifter<2048>,
    /// Octave-up pitch shifter.
    shifter_up: PitchShifter<2048>,
}
impl Default for Octaver {
    fn default() -> Self {
        Self {
            shifter_down: PitchShifter::new(48_000.0),
            shifter_up: PitchShifter::new(48_000.0),
        }
    }
}
impl Block for Octaver {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "pitch.octaver",
            name: "Octaver",
            category: Category::Pitch,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "dry",
                    name: "Dry",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 100.0,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "oct_down",
                    name: "Oct Down",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "oct_up",
                    name: "Oct Up",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    curve: Curve::Linear,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.shifter_down = PitchShifter::new(sr);
        self.shifter_up = PitchShifter::new(sr);
        // Static pitch shift, instant glide.  -1200 = one octave down,
        // +1200 = one octave up.
        self.shifter_down.pitch_cents = -1200.0;
        self.shifter_up.pitch_cents = 1200.0;
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        // Re-assert the static pitch shift on every block so it stays correct
        // under dynamic use.  It is a single field write and cheap.
        self.shifter_down.pitch_cents = -1200.0;
        self.shifter_up.pitch_cents = 1200.0;
        let dry = ctx.param(0) / 100.0;
        let oct_down = ctx.param(1) / 100.0;
        let oct_up = ctx.param(2) / 100.0;
        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let down = self.shifter_down.process(input[i]);
            let up = self.shifter_up.process(input[i]);
            out[i] = input[i] * dry + down * oct_down + up * oct_up;
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.shifter_down.clear();
        self.shifter_up.clear();
    }
}
/// A modulated short delay: chorus, and with feedback a flanger.
///
/// Wraps `pedal_dsp::delay::Chorus`. No latency is declared: the read head is
/// always behind the write head, so the block never anticipates the input and
/// its (time-varying) delay is the sound itself, like `time.delay`.
pub struct Chorus {
    inner: Option<pedal_dsp::delay::Chorus<1024>>,
}
impl Default for Chorus {
    fn default() -> Self {
        Self { inner: None }
    }
}
impl Block for Chorus {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "mod.chorus",
            name: "Chorus",
            category: Category::Modulation,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "rate",     name: "Rate",     unit: Unit::Hertz,         min: 0.1,  max: 5.0,   default: 0.6,  curve: Curve::Log },
                ParamSpec { id: "depth",    name: "Depth",    unit: Unit::Milliseconds,  min: 0.5,  max: 5.0,   default: 3.0,  curve: Curve::Linear },
                ParamSpec { id: "mix",      name: "Mix",      unit: Unit::Percent,       min: 0.0,  max: 100.0, default: 50.0, curve: Curve::Linear },
                ParamSpec { id: "feedback", name: "Feedback", unit: Unit::Percent,       min: 0.0,  max: 80.0,  default: 0.0,  curve: Curve::Linear },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        let mut c = pedal_dsp::delay::Chorus::<1024>::new(sr, 0.6);
        c.base_ms = 8.0; // fixed centre delay, not a knob (see design §2)
        self.inner = Some(c);
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let c = self.inner.as_mut().ok_or(BlockError::NotPrepared("mod.chorus"))?;
        c.set_rate_hz(ctx.param(0));
        c.depth_ms = ctx.param(1).clamp(0.0, 8.0);
        c.mix = (ctx.param(2) / 100.0).clamp(0.0, 1.0);
        c.feedback = (ctx.param(3) / 100.0).clamp(0.0, 0.95);
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            output[i] = c.process(input[i]);
        }
        Ok(())
    }
    fn reset(&mut self) {
        if let Some(c) = &mut self.inner {
            c.clear();
        }
    }
}

/// The graph's input node. The executor writes the raw input into this node's
/// output buffer before running the steps.
#[derive(Default)]
pub struct IoInput;
impl Block for IoInput {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "io.input",
            name: "Input",
            category: Category::Io,
            inputs: vec![],
            outputs: vec![PortSpec::audio("out")],
            params: vec![ParamSpec {
                id: "gain".into(),
                name: "Input gain".into(),
                unit: Unit::Decibels,
                min: -60.0,
                max: 24.0,
                default: 0.0,
                curve: Curve::AmpGain,
            }],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let g = pedal_dsp::util::db_to_lin(ctx.param(0));
        if g != 1.0 {
            for x in ctx.outputs[0].iter_mut() {
                *x *= g;
            }
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
/// The graph's output node. The executor reads this node's input buffer as the
/// graph's output.
#[derive(Default)]
pub struct IoOutput;
impl Block for IoOutput {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "io.output",
            name: "Output",
            category: Category::Io,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![],
            params: vec![],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, _ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        Ok(())
    }
    fn reset(&mut self) {}
}
/// A two-input summer with per-input ratios. The recombine node.
pub struct MixSum {
    a: f32,
    b: f32,
}
impl Default for MixSum {
    fn default() -> Self {
        Self { a: 1.0, b: 1.0 }
    }
}
impl Block for MixSum {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "util.mix",
            name: "Mix",
            category: Category::Utility,
            inputs: vec![PortSpec::audio("in_a"), PortSpec::audio("in_b")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "a", name: "A", unit: Unit::Ratio, min: 0.0, max: 1.0, default: 1.0, curve: Curve::Linear },
                ParamSpec { id: "b", name: "B", unit: Unit::Ratio, min: 0.0, max: 1.0, default: 1.0, curve: Curve::Linear },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        self.a = ctx.param(0);
        self.b = ctx.param(1);
        let a = ctx.inputs[0];
        let b = ctx.inputs[1];
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            out[i] = a[i] * self.a + b[i] * self.b;
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
/// A feedback delay.
pub struct Delay {
    inner: Option<pedal_dsp::FeedbackDelay<60000>>,
    time_ms: f32,
    feedback: f32,
    mix: f32,
}
impl Default for Delay {
    fn default() -> Self {
        Self { inner: None, time_ms: 300.0, feedback: 0.35, mix: 0.3 }
    }
}
impl Block for Delay {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "time.delay",
            name: "Delay",
            category: Category::Time,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "time", name: "Time", unit: Unit::Milliseconds, min: 20.0, max: 1000.0, default: 300.0, curve: Curve::Log },
                ParamSpec { id: "feedback", name: "Feedback", unit: Unit::Percent, min: 0.0, max: 95.0, default: 35.0, curve: Curve::Linear },
                ParamSpec { id: "mix", name: "Mix", unit: Unit::Percent, min: 0.0, max: 100.0, default: 30.0, curve: Curve::Linear },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.inner = Some(pedal_dsp::FeedbackDelay::<60000>::new(sr, self.time_ms));
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        self.time_ms = ctx.param(0);
        self.feedback = ctx.param(1) / 100.0;
        self.mix = ctx.param(2) / 100.0;
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        let d = self.inner.as_mut().ok_or(BlockError::NotPrepared("time.delay"))?;
        d.set_delay_ms(self.time_ms);
        d.set_feedback(self.feedback);
        for i in 0..ctx.n {
            let wet = d.process(input[i]);
            output[i] = pedal_dsp::util::mix(input[i], wet, self.mix);
        }
        Ok(())
    }
    fn reset(&mut self) {
        if let Some(d) = &mut self.inner {
            d.clear();
        }
    }
}
// ---------------------------------------------------------------------------
// Amp, cab, pitch delay, master
// ---------------------------------------------------------------------------
use pedal_dsp::waveshape::{Shaper, ShaperKind};
use pedal_dsp::{DcBlocker, PitchMode};
/// The amp voicing a `Choice` param selects.
const AMP_VOICES: &[&str] = &["Clean", "Crunch", "Lead"];
/// A two-stage amp: tighten → preamp clip → tone stack → power-amp sag-ish
/// soft clip → level. Both nonlinear stages are 4× oversampled (`Shaper`), so
/// high gain doesn't alias. This is the placeholder the NAM block replaces.
pub struct Amp {
    tight: Biquad,
    pre: Shaper,
    dc: Option<DcBlocker>,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    power: Shaper,
    fizz: Biquad,
}
impl Default for Amp {
    fn default() -> Self {
        Self {
            tight: Biquad::identity(),
            pre: Shaper::new(ShaperKind::Tube),
            dc: None,
            bass: Biquad::identity(),
            mid: Biquad::identity(),
            treble: Biquad::identity(),
            power: Shaper::new(ShaperKind::Tanh),
            fizz: Biquad::identity(),
        }
    }
}
impl Block for Amp {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "amp.basic",
            name: "Amp",
            category: Category::Modelling,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "voice", name: "Voice", unit: Unit::Choice(AMP_VOICES), min: 0.0, max: 2.0, default: 1.0, curve: Curve::Linear },
                ParamSpec { id: "gain", name: "Gain", unit: Unit::Linear, min: 0.0, max: 10.0, default: 5.0, curve: Curve::Linear },
                ParamSpec { id: "bass", name: "Bass", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "mid", name: "Mid", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "treble", name: "Treble", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "level", name: "Level", unit: Unit::Decibels, min: -40.0, max: 12.0, default: -6.0, curve: Curve::AmpGain },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.dc = Some(DcBlocker::new(10.0, sr));
        self.pre.reset();
        self.power.reset();
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let sr = ctx.sample_rate;
        let voice = ctx.param(0).round().clamp(0.0, 2.0) as usize;
        let gain = ctx.param(1).clamp(0.0, 10.0) / 10.0;
        let level = pedal_dsp::util::db_to_lin(ctx.param(5));
        // Voicing: how hard each stage is driven and how tight the low end is.
        let (tight_hz, pre_max, power_drive, mid_hz) = match voice {
            0 => (60.0, 4.0, 1.0, 500.0),   // clean: barely breaks up
            1 => (90.0, 20.0, 1.6, 650.0),  // crunch
            _ => (130.0, 20.0, 2.5, 750.0), // lead: tight and saturated
        };
        self.tight.design(BiquadKind::HighPass, sr, tight_hz, 0.7, 0.0);
        self.bass.design(BiquadKind::LowShelf, sr, 120.0, 0.7, ctx.param(2));
        self.mid.design(BiquadKind::Peak, sr, mid_hz, 0.8, ctx.param(3));
        self.treble.design(BiquadKind::HighShelf, sr, 3200.0, 0.7, ctx.param(4));
        self.fizz.design(BiquadKind::LowPass, sr, 9000.0, 0.7, 0.0);
        // Drive is exponential in the knob: 0 → 0.5, 10 → pre_max.
        self.pre.set_drive(0.5 * (pre_max / 0.5f32).powf(gain));
        self.pre.set_level(1.0);
        self.power.set_drive(power_drive);
        self.power.set_level(1.0);
        // The lead voice gets a second preamp stage worth of gain.
        let lead_boost = if voice == 2 { 1.0 + 3.0 * gain } else { 1.0 };
        let dc = self.dc.as_mut().ok_or(BlockError::NotPrepared("amp.basic"))?;
        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let mut x = self.tight.process(input[i]);
            x = self.pre.process(x * lead_boost);
            x = dc.process(x);
            x = self.treble.process(self.mid.process(self.bass.process(x)));
            x = self.power.process(x);
            out[i] = self.fizz.process(x) * level;
        }
        Ok(())
    }
    fn reset(&mut self) {
        for b in [&mut self.tight, &mut self.bass, &mut self.mid, &mut self.treble, &mut self.fizz] {
            b.reset();
        }
        self.pre.reset();
        self.power.reset();
    }
}
/// A filter-based speaker cabinet: low cut, speaker resonance, a mud dip,
/// presence, and a steep (24 dB/oct) high cut. Without a cab, any amp sounds
/// like fizz. Real IR convolution replaces this later.
pub struct Cab {
    filters: [Biquad; 6],
}
impl Default for Cab {
    fn default() -> Self {
        Self { filters: [Biquad::identity(); 6] }
    }
}
impl Block for Cab {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "cab.sim",
            name: "Cab",
            category: Category::Modelling,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "low_cut", name: "Low cut", unit: Unit::Hertz, min: 40.0, max: 200.0, default: 80.0, curve: Curve::Log },
                ParamSpec { id: "resonance", name: "Resonance", unit: Unit::Decibels, min: 0.0, max: 12.0, default: 4.0, curve: Curve::AmpGain },
                ParamSpec { id: "presence", name: "Presence", unit: Unit::Decibels, min: -9.0, max: 9.0, default: 2.0, curve: Curve::AmpGain },
                ParamSpec { id: "high_cut", name: "High cut", unit: Unit::Hertz, min: 2000.0, max: 10000.0, default: 5000.0, curve: Curve::Log },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let sr = ctx.sample_rate;
        let hc = ctx.param(3);
        let f = &mut self.filters;
        f[0].design(BiquadKind::HighPass, sr, ctx.param(0), 0.7, 0.0);
        f[1].design(BiquadKind::Peak, sr, 110.0, 1.4, ctx.param(1));
        f[2].design(BiquadKind::Peak, sr, 400.0, 1.0, -4.0);
        f[3].design(BiquadKind::Peak, sr, 2500.0, 1.2, ctx.param(2));
        f[4].design(BiquadKind::LowPass, sr, hc, 0.54, 0.0);
        f[5].design(BiquadKind::LowPass, sr, hc, 1.31, 0.0);
        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let mut x = input[i];
            for b in f.iter_mut() {
                x = b.process(x);
            }
            out[i] = x;
        }
        Ok(())
    }
    fn reset(&mut self) {
        for b in &mut self.filters {
            b.reset();
        }
    }
}
const PITCH_MODES: &[&str] = &["Target", "Sweep", "Shepard"];
/// The Rainbow-Machine-style delay: a pitch shifter in the feedback path, so
/// every repeat is shifted again and the pitch climbs (or falls).
pub struct PitchDelay {
    inner: Option<Box<pedal_dsp::FeedbackDelay<60000, 2400>>>,
}
impl Default for PitchDelay {
    fn default() -> Self {
        Self { inner: None }
    }
}
impl Block for PitchDelay {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "time.pitchdelay",
            name: "Pitch Delay",
            category: Category::Pitch,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "time", name: "Time", unit: Unit::Milliseconds, min: 20.0, max: 1000.0, default: 380.0, curve: Curve::Log },
                ParamSpec { id: "feedback", name: "Feedback", unit: Unit::Percent, min: 0.0, max: 95.0, default: 55.0, curve: Curve::Linear },
                ParamSpec { id: "pitch", name: "Pitch", unit: Unit::Cents, min: -1200.0, max: 1200.0, default: 700.0, curve: Curve::Linear },
                ParamSpec { id: "rate", name: "Sweep rate", unit: Unit::Cents, min: -1200.0, max: 1200.0, default: 0.0, curve: Curve::Linear },
                ParamSpec { id: "mode", name: "Mode", unit: Unit::Choice(PITCH_MODES), min: 0.0, max: 2.0, default: 0.0, curve: Curve::Linear },
                ParamSpec { id: "mix", name: "Mix", unit: Unit::Percent, min: 0.0, max: 100.0, default: 40.0, curve: Curve::Linear },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        // Built straight into a Box: the delay is ~250 KB, too big to bounce
        // through the stack on a small thread.
        let mut d: Box<pedal_dsp::FeedbackDelay<60000, 2400>> = Box::new(pedal_dsp::FeedbackDelay::new(sr, 380.0));
        d.mix = 1.0; // the block does its own dry/wet
        self.inner = Some(d);
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let d = self.inner.as_mut().ok_or(BlockError::NotPrepared("time.pitchdelay"))?;
        d.set_delay_ms(ctx.param(0));
        d.set_feedback(ctx.param(1) / 100.0);
        d.set_pitch_cents(ctx.param(2));
        d.set_pitch_rate(ctx.param(3));
        d.set_pitch_mode(match ctx.param(4).round() as i32 {
            1 => PitchMode::Sweep,
            2 => PitchMode::Shepard,
            _ => PitchMode::Target,
        });
        d.mix = 1.0;
        let mix = ctx.param(5) / 100.0;
        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let wet = d.process(input[i]);
            out[i] = pedal_dsp::util::mix(input[i], wet, mix);
        }
        Ok(())
    }
    fn reset(&mut self) {
        if let Some(d) = &mut self.inner {
            d.clear();
        }
    }
}
/// Output volume plus a soft limiter, so nothing downstream ever sees more
/// than the ceiling. `ceiling * tanh(x / ceiling)`: transparent well below the
/// ceiling, smooth into it, zero latency.
#[derive(Default)]
pub struct Master;
impl Block for Master {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "util.master",
            name: "Master",
            category: Category::Utility,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "volume", name: "Volume", unit: Unit::Decibels, min: -60.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "ceiling", name: "Ceiling", unit: Unit::Decibels, min: -24.0, max: 0.0, default: -1.0, curve: Curve::AmpGain },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let vol = pedal_dsp::util::db_to_lin(ctx.param(0));
        let ceil = pedal_dsp::util::db_to_lin(ctx.param(1));
        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            out[i] = ceil * (input[i] * vol / ceil).tanh();
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
/// A registry holding every block in this library — the palette.
/// A level-synchronous compressor: a one-pole envelope follower tracks the
/// input level, and gain is reduced above a threshold by a fixed ratio.
/// A stereo Schroeder/Freeverb-style reverb tank: 8 stereo comb pairs +
/// 4 stereo allpass pairs. Wraps `pedal_dsp::reverb::Freeverb` so the block
/// can drive the same input into both channels and use the mixer's wet
/// output. Tunings are scaled to the host rate in `Freeverb::new`.
pub struct ShimmerTank {
    tank: Freeverb<1760, 605>,
}

impl ShimmerTank {
    /// Build a stereo tank at the host rate: full-width combs + allpasses,
    /// dry = 0 (the block blends the wet itself), width = 1.
    pub fn new(sr: f32) -> Self {
        let mut t = Freeverb::<1760, 605>::new(sr);
        t.set_wet(1.0);
        t.set_dry(0.0);
        t.set_width(1.0);
        Self { tank: t }
    }

    /// Run one mono input through both channels, returning (left, right) wet.
    pub fn process(&mut self, input: f32) -> (f32, f32) {
        self.tank.process(input, input)
    }

    /// Clear all internal state.
    pub fn clear(&mut self) {
        self.tank.clear();
    }

    /// Room size 0..1 (maps to the comb feedback gain).
    pub fn set_room_size(&mut self, v: f32) {
        self.tank.set_room_size(clamp(v, 0.0, 1.0));
    }

    /// High-frequency damping 0..1 (low-pass inside the feedback loop).
    pub fn set_damping(&mut self, v: f32) {
        self.tank.set_damping(clamp(v, 0.0, 1.0));
    }
}

/// Fractional-delay input line. The dry input is written here and read back
/// `pre_delay_samples` samples later, giving the `pre_delay` knob its
/// fractional delay at the tank input.
struct PreDelay {
    line: DelayLine<8192>,
}

impl PreDelay {
    fn new() -> Self {
        Self {
            line: DelayLine::<8192>::new(),
        }
    }

    fn clear(&mut self) {
        self.line.clear();
    }

    /// Write a sample, then read back a fractional delay.
    fn read(&mut self, sample: f32, delay_samples: f32) -> f32 {
        self.line.write(sample);
        self.line.read_at(delay_samples)
    }
}

/// Shimmer reverb: a stereo FDN tank whose feedback path contains a
/// fixed-ratio pitch shifter (`pedal_dsp::resample`), a `shimmer` gain clamp,
/// tone damping inside the loop, and a per-block RMS energy guard.
///
/// Signal flow (per spec §3.1):
///
/// ```text
/// input ──► ShimmerTank (stereo FDN) ──► wet ──► Mix ──► out
///            │
///            └─► pitch shift (interval ratio) ──► Shimmer gain ──►
///                  │                                      │
///                  └────── feedback-loop gain limiter ────┘
/// ```
///
/// `pre_delay` is a fractional delay line at the tank input.  Loop gain is
/// limited by (a) clamping `shimmer` to 0.95, (b) tone damping inside the
/// tank, and (c) a per-block RMS energy guard.
pub struct ShimmerReverb {
    /// Smoothed continuous params (τ = 60 ms).  The executor hands the block
    /// *real* values, so the block does its own per-sample EMA.
    mix_sm: f32,
    size_sm: f32,
    decay_sm: f32,
    shimmer_sm: f32,
    tone_sm: f32,
    pre_delay_sm: f32,

    /// Instant (discrete) param: the interval index 0..2.
    interval: usize,

    /// The stereo FDN tank.
    tank: Freeverb<1760, 605>,

    /// Fractional delay line for the pre-delay.
    pre_delay: PreDelay,

    /// Fixed-ratio pitch shifter: `r = 2^(interval/12)`.
    ratio: f32,

    /// One-block-delayed pitch-shifted wet, used as the tank-input feedback.
    /// Ring buffer of 2 × `max_block` samples per channel; `fb_write` is the
    /// write head.
    fb_l: Vec<f32>,
    fb_r: Vec<f32>,
    fb_write: usize,

    /// Rolling wet history (2 × `max_block` per channel): the previous and
    /// the current block concatenated, so the fixed-ratio resampler reads a
    /// continuous stream instead of restarting at every block head.
    wet_hist_l: Vec<f32>,
    wet_hist_r: Vec<f32>,

    /// Per-block scratch buffers (allocated once in `prepare`, reused every
    /// block — the audio callback never allocates).
    wet_l: Vec<f32>,
    wet_r: Vec<f32>,
    fb_new_l: Vec<f32>,
    fb_new_r: Vec<f32>,

    /// Resampler state, one per channel (cubic, linear-phase, relay with no
    /// phantom gain).
    state_l: [f32; 4],
    state_r: [f32; 4],

    /// EMA smoothing coefficient for τ = 60 ms at the host rate.
    coef: f32,

    /// Energy-guard ratchet (see `process`): instantaneous attack, slow
    /// release, never amplifies.  1.0 = no limiting.
    guard_sm: f32,
    /// Release coefficient for the guard (~τ 2 s at the host rate).
    guard_rel: f32,

    /// Input side-chain envelope for the shimmer gate: the pitched feedback
    /// sustains while there is input and lets go after the player stops, so
    /// the tail always decays through the tank alone.
    in_env: f32,
    /// Per-block release coefficient for `in_env` (τ ≈ 300 ms).
    in_rel: f32,
}

impl Default for ShimmerReverb {
    fn default() -> Self {
        // Overwritten in `prepare`.
        Self {
            mix_sm: 40.0,
            size_sm: 70.0,
            decay_sm: 2.5,
            shimmer_sm: 55.0,
            tone_sm: 50.0,
            pre_delay_sm: 0.0,
            interval: 0,
            tank: Freeverb::<1760, 605>::new(48_000.0),
            pre_delay: PreDelay::new(),
            ratio: 2.0,
            fb_l: vec![0.0; 128],
            fb_r: vec![0.0; 128],
            fb_write: 0,
            wet_hist_l: vec![0.0; 128],
            wet_hist_r: vec![0.0; 128],
            wet_l: vec![0.0; 128],
            wet_r: vec![0.0; 128],
            fb_new_l: vec![0.0; 128],
            fb_new_r: vec![0.0; 128],
            state_l: [0.0; 4],
            state_r: [0.0; 4],
            coef: 0.0,
            guard_sm: 1.0,
            guard_rel: 0.0,
            in_env: 0.0,
            in_rel: 0.0,
        }
    }
}

impl ShimmerReverb {
    /// Fixed (linear) gain of the feedback loop, clamped to 0.95.
    fn shimmer_lin(&self) -> f32 {
        (self.shimmer_sm / 100.0).clamp(0.0, 0.95)
    }

    /// Per-block RMS of the stereo wet pair, in linear amplitude.  Used by the
    /// energy guard to keep the feedback loop bounded.
    fn block_rms(wet_l: &[f32], wet_r: &[f32]) -> f32 {
        let mut acc = 0.0f64;
        let n = wet_l.len().min(wet_r.len());
        if n == 0 {
            return 0.0;
        }
        for i in 0..n {
            acc += (wet_l[i] * wet_l[i]) as f64;
            acc += (wet_r[i] * wet_r[i]) as f64;
        }
        (acc / (n as f64 * 2.0)).sqrt() as f32
    }
    fn reset_smoothed(&mut self) {
        self.mix_sm = 40.0;
        self.size_sm = 70.0;
        self.decay_sm = 2.5;
        self.shimmer_sm = 55.0;
        self.tone_sm = 50.0;
        self.pre_delay_sm = 0.0;
    }
}

impl Block for ShimmerReverb {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "time.shimmer",
            name: "Shimmer",
            category: Category::Time,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "mix",
                    name: "Mix",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 40.0,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "size",
                    name: "Size",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 70.0,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "decay",
                    name: "Decay",
                    unit: Unit::Seconds,
                    min: 0.2,
                    max: 8.0,
                    default: 2.5,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "shimmer",
                    name: "Shimmer",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 55.0,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "interval",
                    name: "Interval",
                    unit: Unit::Choice(SHIMMER_INTERVALS),
                    min: 0.0,
                    max: 2.0,
                    default: 0.0,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "tone",
                    name: "Tone",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 50.0,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "pre_delay",
                    name: "Pre-delay",
                    unit: Unit::Milliseconds,
                    min: 0.0,
                    max: 80.0,
                    default: 0.0,
                    curve: Curve::Linear,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, sr: f32, max_block: usize) -> Result<(), BlockError> {
        self.tank = Freeverb::<1760, 605>::new(sr);
        self.pre_delay = PreDelay::new();
        self.ratio = 2.0f32.powf(12.0 / 12.0);
        let buf = (max_block.max(1) * 2).max(128);
        self.fb_l = vec![0.0; buf];
        self.fb_r = vec![0.0; buf];
        self.fb_write = 0;
        self.wet_hist_l = vec![0.0; buf];
        self.wet_hist_r = vec![0.0; buf];
        // Scratch buffers sized to one block; reused every `process` call.
        let scratch = max_block.max(1);
        self.wet_l.resize(scratch, 0.0);
        self.wet_r.resize(scratch, 0.0);
        self.fb_new_l.resize(scratch, 0.0);
        self.fb_new_r.resize(scratch, 0.0);
        self.state_l = [0.0; 4];
        self.state_r = [0.0; 4];
        self.coef = smoothing_coef_ms(60.0, sr);
        // Guard release: τ ≈ 2 s.
        self.guard_rel = smoothing_coef_ms(2000.0, sr);
        self.in_env = 0.0;
        // Per-block release with τ ≈ 300 ms at 48 kHz.
        let per_sample = (1.0 - smoothing_coef_ms(300.0, sr)).max(0.0);
        self.in_rel = per_sample.powi(max_block.max(1) as i32);
        self.reset_smoothed();
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        // Raw (target) values, before smoothing.
        // Seed the smoothed state from the *current* targets the first block
        // after a prepare/reset, so a freshly compiled graph renders exactly
        // the param values the preset asked for; the EMA then smooths changes.
        if ctx.block_index == 0 {
            self.mix_sm = ctx.param(0);
            self.size_sm = ctx.param(1);
            self.decay_sm = ctx.param(2);
            self.shimmer_sm = ctx.param(3);
            self.tone_sm = ctx.param(5);
            self.pre_delay_sm = ctx.param(6);
        }
        let mix_t = ctx.param(0);
        let size_t = ctx.param(1);
        let decay_t = ctx.param(2);
        let shimmer_t = ctx.param(3);
        let interval_t = ctx.param(4);
        let tone_t = ctx.param(5);
        let pre_delay_t = ctx.param(6);

        // Exact bypass at mix 0: pass the input through sample-for-sample.
        if mix_t <= 0.0 {
            let input = ctx.inputs[0];
            let out = &mut ctx.outputs[0][..];
            for i in 0..ctx.n {
                out[i] = input[i];
            }
            return Ok(());
        }

        let sr = ctx.sample_rate;

        // Per-sample EMA (τ = 60 ms) applied to the *real* value before it is
        // fed to the tank (spec §2).  `interval` is a discrete choice applied
        // instantly.  The values are seeded from the targets on the first
        // block after `prepare`/`reset` (see above), so the EMA only smooths
        // runtime *changes*.
        self.mix_sm += self.coef * (mix_t - self.mix_sm);
        self.size_sm += self.coef * (size_t - self.size_sm);
        self.decay_sm += self.coef * (decay_t - self.decay_sm);
        self.shimmer_sm += self.coef * (shimmer_t - self.shimmer_sm);
        self.tone_sm += self.coef * (tone_t - self.tone_sm);
        self.pre_delay_sm += self.coef * (pre_delay_t - self.pre_delay_sm);

        // Interval is discrete: round to the nearest index and clamp to 0..2.
        self.interval = interval_t
            .round()
            .clamp(0.0, 2.0) as usize;

        // Tank parameters: `size` -> comb feedback (room size), `tone` ->
        // damping inside the loop.
        // Spec §2: `size` is the master comb-feedback gain, `decay` sets the
        // target RT60 of the tank loop; the loop gain is reduced so the
        // physical decay approaches it.  Comb feedback maps to
        // f = 0.001^(round_trip / RT60), blended by `size` as master.
        let round_trip = 0.0253f32; // ~1214 samples at 48 kHz
        let f_decay =
            0.001f32.powf(round_trip / self.decay_sm.max(0.2));
        let f_eff = 0.7 + (f_decay - 0.7) * (self.size_sm / 100.0);
        let room = ((f_eff - 0.7) / 0.28).clamp(0.0, 1.0);
        self.tank.set_room_size(room);
        self.tank.set_damping(self.tone_sm / 100.0);
        // The tank sums 8 parallel combs; normalise by the count so the
        // shimmer feedback loop's open gain stays below 1 (shimmer caps at
        // 0.95 and the comb feedback is < 1).
        self.tank.set_wet(1.0 / 8.0);
        self.tank.set_dry(0.0);
        self.tank.set_width(1.0);

        // Pitch-shift ratio for the chosen interval.
        let semitones = match self.interval {
            0 => 12,
            1 => 7,
            _ => -12,
        };
        self.ratio = 2.0f32.powf(semitones as f32 / 12.0);

        // Fractional delay at the tank input (smoothed, spec §2).
        let pd_samples = (self.pre_delay_sm / 1000.0) * sr;

        // --- Process one block, with one-block-delayed feedback in the loop ---
        let n = ctx.n;
        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        let fb_l = &self.fb_l;
        let fb_r = &self.fb_r;

        // Read position: the feedback for this block lives `n` samples before
        // the current write head.
        let mut read_idx =
            (self.fb_write.wrapping_sub(n as usize)) % fb_l.len();

        // Wet samples, retained to build the pitch-shifted feedback.
        // Scratch buffers are pre-allocated struct fields (no per-block alloc).
        for i in 0..n {
            let pd = self.pre_delay.read(input[i], pd_samples);
            let fb_l0 = fb_l[read_idx];
            let fb_r0 = fb_r[read_idx];
            read_idx = (read_idx + 1) % fb_l.len();
            let tank_in_l = pd + fb_l0;
            let tank_in_r = pd + fb_r0;
            let (wl, wr) = self.tank.process(tank_in_l, tank_in_r);
            self.wet_l[i] = wl;
            self.wet_r[i] = wr;
            out[i] = pedal_dsp::util::mix(
                input[i],
                (wl + wr) * 0.5,
                self.mix_sm / 100.0,
            );
        }

        // --- Build the pitch-shifted feedback for the next block ---
        // The resampler reads the last two wet blocks as one continuous
        // stream: the octave-up ratio decimates *every* wet sample exactly
        // once (a norm-preserving relay), so no coherent pumping builds on
        // the comb returns and the loop stays provably bounded.
        let h = self.wet_hist_l.len();
        self.wet_hist_l.copy_within(n..h, 0);
        self.wet_hist_r.copy_within(n..h, 0);
        self.wet_hist_l[n..h].copy_from_slice(&self.wet_l[..n]);
        self.wet_hist_r[n..h].copy_from_slice(&self.wet_r[..n]);
        // Input samples consumed: n * ratio, taken from the end of the
        // window so the newest wet always feeds the pitch shifter.
        let consumed = ((n as f32 * self.ratio) as usize).clamp(1, h);
        let start = h - consumed;
        // `resample`'s ratio is out/in rate; pitching the wet up by `r`
        // means reading it back `r` times faster, i.e. ratio = 1/r.
        let inv = 1.0 / self.ratio;
        resample(
            &self.wet_hist_l[start..],
            &mut self.fb_new_l[..n],
            inv,
            &mut self.state_l,
        );
        resample(
            &self.wet_hist_r[start..],
            &mut self.fb_new_r[..n],
            inv,
            &mut self.state_r,
        );

        // Energy guard: cap wet RMS so the loop stays bounded.  Attack is
        // instantaneous (ratchet down), release is slow (~2 s): the build-up
        // transient is clamped at the cap, then the guard *lets go* and the
        // natural loop gain (shimmer ≤ 0.95 × comb feedback) carries the
        // tail to silence.  A purely proportional cap would hold the wet at
        // the cap level forever (AGC stasis) and never decay.
        // Final line of defence on the feedback *amplitude*: with the comb
        // feedback at its physical maximum (0.98) the tank's recirculation
        // builds a sustained feedback amplitude A into a wet peak of roughly
        // A / (1 - 0.93) ≈ 14·A; capping each feedback sample keeps that
        // below 2.0 by construction, even for spike trains the block RMS
        // guard misses.
        const FB_CLAMP: f32 = 0.07;
        let shimmer = self.shimmer_lin();
        let wet_rms = ShimmerReverb::block_rms(&self.wet_l[..n], &self.wet_r[..n]);
        let wet_peak = self.wet_l[..n]
            .iter()
            .chain(self.wet_r[..n].iter())
            .fold(0.0f32, |m, &v| m.max(v.abs()));
        const WET_CAP: f32 = 0.4;
        // Below this wet level the shimmer contribution is faded out.
        const TAIL_FLOOR: f32 = 0.004;
        // A peak cap rides along with the RMS cap: sparse comb spike trains
        // have a large crest factor and would otherwise punch through.  The
        // cap sits well under the block overshoot margin (the tank can grow
        // several times within one block after the guard first trips), so
        // the output stays bounded even at size/decay/shimmer = max.
        const WET_PEAK_CAP: f32 = 0.25;
        if wet_rms > WET_CAP || wet_peak > WET_PEAK_CAP {
            let g_need = (WET_CAP / wet_rms.max(1e-9)).min(WET_PEAK_CAP / wet_peak.max(1e-9));
            self.guard_sm = self.guard_sm.min(g_need);
        } else if wet_rms > TAIL_FLOOR {
            self.guard_sm += self.guard_rel * (1.0 - self.guard_sm);
        } else {
            // Tail floor reached: the shimmer loop lets go entirely, so the
            // remaining wet decays through the tank alone.  Without this the
            // recirculation holds itself up at a constant level forever
            // (proportional controls find a unity-gain equilibrium).
            self.guard_sm += self.guard_rel * (0.0 - self.guard_sm);
        }
        let guard = self.guard_sm;
        // Input side-chain: gate the shimmer with the dry signal present.
        let mut iacc = 0.0f64;
        for i in 0..n {
            iacc += (input[i] * input[i]) as f64;
        }
        let in_rms = ((iacc / n as f64).sqrt() as f32);
        self.in_env = in_rms.max(self.in_env * self.in_rel);
        const GATE_FLOOR: f32 = 1e-3;
        let gate = (self.in_env / GATE_FLOOR).min(1.0);
        for i in 0..n {
            self.fb_new_l[i] =
                (self.fb_new_l[i] * shimmer * guard * gate).clamp(-FB_CLAMP, FB_CLAMP);
            self.fb_new_r[i] =
                (self.fb_new_r[i] * shimmer * guard * gate).clamp(-FB_CLAMP, FB_CLAMP);
        }

        // Write the feedback into the ring buffer.
        {
            let w = self.fb_write;
            let rem = self.fb_l.len() - w;
            if rem >= n {
                for i in 0..n {
                    self.fb_l[w + i] = self.fb_new_l[i];
                    self.fb_r[w + i] = self.fb_new_r[i];
                }
            } else {
                for i in 0..rem {
                    self.fb_l[w + i] = self.fb_new_l[i];
                    self.fb_r[w + i] = self.fb_new_r[i];
                }
                for i in 0..(n - rem) {
                    self.fb_l[i] = self.fb_new_l[rem + i];
                    self.fb_r[i] = self.fb_new_r[rem + i];
                }
            }
            self.fb_write = (w + n) % self.fb_l.len();
        }

        Ok(())
    }

    fn reset(&mut self) {
        self.tank.clear();
        self.pre_delay.clear();
        self.fb_l.fill(0.0);
        self.fb_r.fill(0.0);
        self.fb_write = 0;
        self.wet_hist_l.fill(0.0);
        self.wet_hist_r.fill(0.0);
        self.wet_l.fill(0.0);
        self.wet_r.fill(0.0);
        self.fb_new_l.fill(0.0);
        self.fb_new_r.fill(0.0);
        self.guard_sm = 1.0;
        self.in_env = 0.0;
        self.state_l = [0.0; 4];
        self.state_r = [0.0; 4];
        self.reset_smoothed();
    }
}

/// Choice labels for the `interval` parameter (spec §2), indexed as the
/// parameter value: 0 = `+12 st up` (2.0), 1 = `+7 st up` (1.4983),
/// 2 = `−12 st down` (0.5).
const SHIMMER_INTERVALS: &[&str] = &["+12 st up", "+7 st up", "−12 st down"];

/// The compressor owns its envelope state, so it is allocation-free and
/// sample-accurate on the audio thread.
///
/// Defaults: threshold -20 dB, ratio 4:1, attack 1 ms, release 50 ms,
/// no makeup gain, fully wet.
pub struct Compressor {
    env: EnvelopeFollower,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    makeup_db: f32,
    mix: f32,
}
impl Default for Compressor {
    fn default() -> Self {
        Self {
            env: EnvelopeFollower::new(1.0, 50.0, 48_000.0),
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 1.0,
            release_ms: 50.0,
            makeup_db: 0.0,
            mix: 100.0,
        }
    }
}
impl Block for Compressor {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "dynamics.compressor",
            name: "Compressor",
            category: Category::Dynamics,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "threshold",
                    name: "Threshold",
                    unit: Unit::Decibels,
                    min: -60.0,
                    max: 0.0,
                    default: -20.0,
                    curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "ratio",
                    name: "Ratio",
                    unit: Unit::Ratio,
                    min: 1.0,
                    max: 20.0,
                    default: 4.0,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "attack",
                    name: "Attack",
                    unit: Unit::Milliseconds,
                    min: 0.1,
                    max: 50.0,
                    default: 1.0,
                    curve: Curve::Log,
                },
                ParamSpec {
                    id: "release",
                    name: "Release",
                    unit: Unit::Milliseconds,
                    min: 1.0,
                    max: 500.0,
                    default: 50.0,
                    curve: Curve::Log,
                },
                ParamSpec {
                    id: "makeup",
                    name: "Makeup",
                    unit: Unit::Decibels,
                    min: 0.0,
                    max: 30.0,
                    default: 0.0,
                    curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "mix",
                    name: "Mix",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 100.0,
                    curve: Curve::Linear,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.env = EnvelopeFollower::new(self.attack_ms, self.release_ms, sr);
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let sr = ctx.sample_rate;
        let threshold = ctx.param(0);
        let ratio = ctx.param(1).clamp(1.0, 20.0);
        let attack = ctx.param(2);
        let release = ctx.param(3);
        let makeup = ctx.param(4);
        let mix = ctx.param(5) / 100.0;
        self.env.set_timeconstants(attack, release, sr);
        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let level = self.env.process(input[i].abs());
            // Convert to dB with a floor so silence maps to a finite value.
            let level_db = pedal_dsp::util::lin_to_db(level.max(1e-9));
            let excess = level_db - threshold;
            // Gain reduction grows linearly with the excess, by 1 - 1/ratio.
            let gr_db = if excess > 0.0 {
                excess * (1.0 - 1.0 / ratio)
            } else {
                0.0
            };
            let gain = pedal_dsp::util::db_to_lin(-gr_db + makeup);
            let wet = input[i] * gain;
            out[i] = pedal_dsp::util::mix(input[i], wet, mix);
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.env.reset();
    }
}
// ---------------------------------------------------------------------------
// Multiband compressor: crossover -> per-band compressors -> recombine.
// ---------------------------------------------------------------------------

/// Latest per-band gain reduction (dB, >= 0) and output peak (dBFS), stored as
/// f32 bits. Written by the audio thread after each block (lock-free); the
/// last multiband block to process wins. Read by the engine's meters loop.
pub static MULTIBAND_GR_DB: [AtomicU32; 4] =
    [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
pub static MULTIBAND_OUT_DB: [AtomicU32; 4] =
    [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

/// Number of active bands (0 = no multiband block has processed yet).
pub static MULTIBAND_ACTIVE: AtomicU32 = AtomicU32::new(0);

/// Read the latest per-band `(gr_db, out_db)` for all four slots (use
/// `MULTIBAND_ACTIVE` for how many are live).
pub fn multiband_meters() -> [(f32, f32); 4] {
    let mut m = [(0.0, 0.0); 4];
    for i in 0..4 {
        m[i] = (
            f32::from_bits(MULTIBAND_GR_DB[i].load(Ordering::Relaxed)),
            f32::from_bits(MULTIBAND_OUT_DB[i].load(Ordering::Relaxed)),
        );
    }
    m
}

/// Floor for the output-level meter (dBFS).
const MB_METER_FLOOR_DB: f32 = -120.0;

/// Multiband compressor. `bands` (2..4) are split by cascaded Linkwitz-Riley
/// 4th-order crossovers. Edge `j` splits the running "rest" signal into
/// `LP_j` (band `j`) and `HP_j` (continues up). Band `j` is then passed
/// through the all-pass `LP_k + HP_k` of every higher edge `k > j` so every
/// band shares the same phase response and the bands sum flat in magnitude.
///
/// Each band has a peak envelope follower and a dB-domain gain computer
/// (threshold / ratio / makeup, no knee). Bypass passes the band through
/// uncompressed; solo restricts the sum to the soloed bands.
///
/// Real-time safe: all state is inline, no allocation or locks in `process`.
/// Parameters arrive already smoothed by the executor.
pub struct MultibandCompressor {
    lp: [[Biquad; 2]; 3],
    hp: [[Biquad; 2]; 3],
    /// All-pass compensation, indexed `[band j][edge k]` (used for k > j).
    comp_lp: [[[Biquad; 2]; 3]; 3],
    comp_hp: [[[Biquad; 2]; 3]; 3],
    /// Crossover frequencies and sample rate the filters are designed for.
    designed_f: [f32; 3],
    designed_edges: usize,
    designed_sr: f32,
    envs: [EnvelopeFollower; 4],
    env_times: [(f32, f32); 4],
    /// Latest per-band gain reduction / output peak (dB) of the last block.
    gr_db: [f32; 4],
    out_db: [f32; 4],
}

impl Default for MultibandCompressor {
    fn default() -> Self {
        Self {
            lp: [[Biquad::identity(); 2]; 3],
            hp: [[Biquad::identity(); 2]; 3],
            comp_lp: [[[Biquad::identity(); 2]; 3]; 3],
            comp_hp: [[[Biquad::identity(); 2]; 3]; 3],
            designed_f: [0.0; 3],
            designed_edges: 0,
            designed_sr: 0.0,
            envs: [EnvelopeFollower::new(1.0, 50.0, 48_000.0); 4],
            env_times: [(0.0, 0.0); 4],
            gr_db: [0.0; 4],
            out_db: [MB_METER_FLOOR_DB; 4],
        }
    }
}

fn mb_band_specs() -> &'static [ParamSpec] {
    static SPECS: std::sync::OnceLock<Vec<ParamSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(mb_band_specs_build)
}

fn mb_band_specs_build() -> Vec<ParamSpec> {
    const NAMES: [(&str, &str); 4] = [
        ("band_1", "Band 1"),
        ("band_2", "Band 2"),
        ("band_3", "Band 3"),
        ("band_4", "Band 4"),
    ];
    let mut v = Vec::new();
    for (id, name) in NAMES {
        let mk = |suffix: &'static str, label: &'static str, unit, min, max, default, curve| {
            // Leak-free: ids/names are built once at spec() time (not on the
            // audio thread), so leaking the tiny strings is acceptable.
            let id: &'static str = Box::leak(format!("{id}_{suffix}").into_boxed_str());
            let name: &'static str = Box::leak(format!("{name} {label}").into_boxed_str());
            ParamSpec { id, name, unit, min, max, default, curve }
        };
        v.push(mk("threshold", "threshold", Unit::Decibels, -60.0, 0.0, -20.0, Curve::AmpGain));
        v.push(mk("ratio", "ratio", Unit::Ratio, 1.0, 20.0, 4.0, Curve::Linear));
        v.push(mk("attack", "attack", Unit::Milliseconds, 0.1, 50.0, 1.0, Curve::Log));
        v.push(mk("release", "release", Unit::Milliseconds, 1.0, 500.0, 50.0, Curve::Log));
        v.push(mk("makeup", "makeup", Unit::Decibels, 0.0, 30.0, 0.0, Curve::AmpGain));
        v.push(mk("bypass", "bypass", Unit::Linear, 0.0, 1.0, 0.0, Curve::Linear));
        v.push(mk("solo", "solo", Unit::Linear, 0.0, 1.0, 0.0, Curve::Linear));
    }
    v
}

impl Block for MultibandCompressor {
    fn spec(&self) -> BlockSpec {
        let mut params = vec![
            ParamSpec { id: "bands", name: "Bands", unit: Unit::Linear, min: 2.0, max: 4.0, default: 3.0, curve: Curve::Linear },
            ParamSpec { id: "crossover_1", name: "Crossover 1", unit: Unit::Hertz, min: 20.0, max: 2000.0, default: 200.0, curve: Curve::Log },
            ParamSpec { id: "crossover_2", name: "Crossover 2", unit: Unit::Hertz, min: 200.0, max: 20000.0, default: 4000.0, curve: Curve::Log },
            ParamSpec { id: "crossover_3", name: "Crossover 3", unit: Unit::Hertz, min: 200.0, max: 20000.0, default: 8000.0, curve: Curve::Log },
            ParamSpec { id: "output_gain", name: "Output gain", unit: Unit::Decibels, min: -24.0, max: 24.0, default: 0.0, curve: Curve::AmpGain },
            ParamSpec { id: "dry_wet", name: "Dry/Wet", unit: Unit::Percent, min: 0.0, max: 100.0, default: 100.0, curve: Curve::Linear },
        ];
        params.extend(mb_band_specs().iter().cloned());
        BlockSpec {
            id: "dynamics.multiband_compressor",
            name: "Multiband Compressor",
            category: Category::Dynamics,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params,
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        self.designed_sr = 0.0; // force a redesign on the first block
        self.reset();
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let n = ctx.n;
        let sr = ctx.sample_rate.max(1.0);
        let bands = (ctx.param(0).round().clamp(2.0, 4.0)) as usize;
        let edges = bands - 1;

        // Crossover frequencies: ascending, below Nyquist.
        let mut f = [ctx.param(1), ctx.param(2), ctx.param(3)];
        let fmax = sr * 0.45;
        for j in 0..3 {
            f[j] = f[j].clamp(20.0, fmax);
            if j > 0 {
                f[j] = f[j].max(f[j - 1] * 1.1).min(fmax);
            }
        }
        self.design_crossover(sr, f, edges);

        let output_gain = pedal_dsp::util::db_to_lin(ctx.param(4).clamp(-24.0, 24.0));
        let wet_mix = (ctx.param(5) / 100.0).clamp(0.0, 1.0);
        let dry_mix = 1.0 - wet_mix;

        let mut thr = [0.0f32; 4];
        let mut ratio = [1.0f32; 4];
        let mut makeup = [1.0f32; 4];
        let mut bypass = [false; 4];
        let mut solo = [false; 4];
        for i in 0..bands {
            let p = 6 + i * 7;
            thr[i] = ctx.param(p).clamp(-60.0, 0.0);
            ratio[i] = ctx.param(p + 1).clamp(1.0, 20.0);
            let (a, r) = (ctx.param(p + 2).max(0.01), ctx.param(p + 3).max(0.1));
            if self.env_times[i] != (a, r) || self.designed_sr != sr {
                self.envs[i].set_timeconstants(a, r, sr);
                self.env_times[i] = (a, r);
            }
            makeup[i] = pedal_dsp::util::db_to_lin(ctx.param(p + 4).clamp(0.0, 30.0));
            bypass[i] = ctx.param(p + 5) > 0.5;
            solo[i] = ctx.param(p + 6) > 0.5;
        }
        self.designed_sr = sr;
        let any_solo = solo[..bands].iter().any(|s| *s);

        let mut gr_peak = [0.0f32; 4];
        let mut out_peak = [0.0f32; 4];

        for s in 0..n {
            let x = ctx.inputs[0][s];
            let x = if x.is_finite() { x } else { 0.0 };

            // ---- split ----
            let mut band = [0.0f32; 4];
            let mut rest = x;
            for j in 0..edges {
                let t = self.lp[j][0].process(rest);
                let mut low = self.lp[j][1].process(t);
                let t = self.hp[j][0].process(rest);
                rest = self.hp[j][1].process(t);
                for k in (j + 1)..edges {
                    let t = self.comp_lp[j][k][0].process(low);
                    let a = self.comp_lp[j][k][1].process(t);
                    let t = self.comp_hp[j][k][0].process(low);
                    let b = self.comp_hp[j][k][1].process(t);
                    low = a + b;
                }
                band[j] = low;
            }
            band[edges] = rest;

            // ---- compress + sum ----
            let mut wet = 0.0f32;
            for i in 0..bands {
                let xb = band[i];
                let y = if bypass[i] {
                    xb
                } else {
                    let level = self.envs[i].process(xb);
                    let level_db = pedal_dsp::util::lin_to_db(level.max(1e-4));
                    let excess = level_db - thr[i];
                    let gr = if excess > 0.0 { excess * (1.0 - 1.0 / ratio[i]) } else { 0.0 };
                    if gr > gr_peak[i] {
                        gr_peak[i] = gr;
                    }
                    xb * pedal_dsp::util::db_to_lin(-gr) * makeup[i]
                };
                let ya = y.abs();
                if ya > out_peak[i] {
                    out_peak[i] = ya;
                }
                if !any_solo || solo[i] {
                    wet += y;
                }
            }
            let mut out = (dry_mix * x + wet_mix * wet) * output_gain;
            // Flush denormals / clamp non-finite so nothing poisons the chain.
            if !out.is_finite() {
                out = 0.0;
            } else if out.abs() < 1e-20 {
                out = 0.0;
            }
            ctx.outputs[0][s] = out;
        }

        MULTIBAND_ACTIVE.store(bands as u32, Ordering::Relaxed);
        for i in 0..4 {
            self.gr_db[i] = if i < bands { gr_peak[i] } else { 0.0 };
            self.out_db[i] = if i < bands {
                pedal_dsp::util::lin_to_db(out_peak[i].max(1e-6)).max(MB_METER_FLOOR_DB)
            } else {
                MB_METER_FLOOR_DB
            };
            MULTIBAND_GR_DB[i].store(self.gr_db[i].to_bits(), Ordering::Relaxed);
            MULTIBAND_OUT_DB[i].store(self.out_db[i].to_bits(), Ordering::Relaxed);
        }
        Ok(())
    }

    fn reset(&mut self) {
        let clear = |b: &mut Biquad| b.reset();
        for j in 0..3 {
            self.lp[j].iter_mut().for_each(clear);
            self.hp[j].iter_mut().for_each(clear);
            for k in 0..3 {
                self.comp_lp[j][k].iter_mut().for_each(clear);
                self.comp_hp[j][k].iter_mut().for_each(clear);
            }
        }
        for e in self.envs.iter_mut() {
            e.reset();
        }
        self.gr_db = [0.0; 4];
        self.out_db = [MB_METER_FLOOR_DB; 4];
    }
}

impl MultibandCompressor {
    /// Latest per-band gain reduction in dB (>= 0) from the last block.
    pub fn gain_reduction_db(&self) -> [f32; 4] {
        self.gr_db
    }

    /// Latest per-band output peak in dBFS from the last block.
    pub fn output_peak_db(&self) -> [f32; 4] {
        self.out_db
    }

    /// (Re)design the crossover filters. Coefficients are only recomputed when
    /// the frequencies, edge count or sample rate change, and filter state is
    /// kept (`design` does not clear it), so sweeping a crossover is click-free.
    fn design_crossover(&mut self, sr: f32, f: [f32; 3], edges: usize) {
        if self.designed_sr == sr && self.designed_edges == edges && self.designed_f == f {
            return;
        }
        const Q: f32 = core::f32::consts::FRAC_1_SQRT_2;
        for j in 0..edges {
            for st in 0..2 {
                self.lp[j][st].design(BiquadKind::LowPass, sr, f[j], Q, 0.0);
                self.hp[j][st].design(BiquadKind::HighPass, sr, f[j], Q, 0.0);
            }
            for k in (j + 1)..edges {
                for st in 0..2 {
                    self.comp_lp[j][k][st].design(BiquadKind::LowPass, sr, f[k], Q, 0.0);
                    self.comp_hp[j][k][st].design(BiquadKind::HighPass, sr, f[k], Q, 0.0);
                }
            }
        }
        self.designed_f = f;
        self.designed_edges = edges;
        // designed_sr is updated by the caller after the envelope retune.
    }
}


/// Tremolo: amplitude modulation. The input gain is swept by an LFO so the
/// volume pulses rhythmically.
///
/// The gain varies between `(1 - depth)` and `1`, so `depth = 0` leaves the
/// signal untouched (bypass-equivalent) and `depth = 1` silences the signal
/// at the trough of the LFO. The LFO phase is advanced monotonically and
/// wrapped, so it is continuous across block boundaries and produces no
/// clicks.
pub struct Gate {
    env: EnvelopeFollower,
    threshold_db: f32,
    attack_ms: f32,
    release_ms: f32,
    mix: f32,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            env: EnvelopeFollower::new(1.0, 5.0, 48_000.0),
            threshold_db: -40.0,
            attack_ms: 1.0,
            release_ms: 5.0,
            mix: 100.0,
        }
    }
}

impl Block for Gate {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "dynamics.noise_gate",
            name: "Noise gate",
            category: Category::Dynamics,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "threshold",
                    name: "Threshold",
                    unit: Unit::Decibels,
                    min: -60.0,
                    max: 0.0,
                    default: -40.0,
                    curve: Curve::AmpGain,
                },
                ParamSpec {
                    id: "attack",
                    name: "Attack",
                    unit: Unit::Milliseconds,
                    min: 0.1,
                    max: 10.0,
                    default: 1.0,
                    curve: Curve::Log,
                },
                ParamSpec {
                    id: "release",
                    name: "Release",
                    unit: Unit::Milliseconds,
                    min: 1.0,
                    max: 100.0,
                    default: 5.0,
                    curve: Curve::Log,
                },
                ParamSpec {
                    id: "mix",
                    name: "Mix",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 100.0,
                    curve: Curve::Linear,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.env = EnvelopeFollower::new(self.attack_ms, self.release_ms, sr);
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let threshold = ctx.param(0);
        let attack = ctx.param(1);
        let release = ctx.param(2);
        let mix = ctx.param(3) / 100.0;

        self.env.set_timeconstants(attack, release, ctx.sample_rate);

        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let level = self.env.process(input[i].abs());
            // Convert to dB with a floor so silence maps to a finite value.
            let level_db = pedal_dsp::util::lin_to_db(level.max(1e-4));
            let excess = level_db - threshold;
            // When excess > 0 the signal is above the threshold: open fully.
            // Below the threshold the gate closes by up to 60 dB.
            let gr_db = if excess > 0.0 {
                0.0
            } else {
                (threshold - level_db).min(60.0)
            };
            let gain = pedal_dsp::util::db_to_lin(-gr_db);
            let wet = input[i] * gain;
            out[i] = pedal_dsp::util::mix(input[i], wet, mix);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.env.reset();
    }
}
pub struct Tremolo {
    lfo: pedal_dsp::lfo::Lfo,
    depth: f32,
    mix: f32,
}
impl Default for Tremolo {
    fn default() -> Self {
        Self {
            // 0.6 Hz gives roughly a 0.6 Hz pulse (mix-tempo-near feel) on
            // the default 48 kHz host rate; the sample rate is re-set in
            // `prepare` to the host's actual rate.
            lfo: pedal_dsp::lfo::Lfo::new(0.6, 48_000.0),
            depth: 0.5,
            mix: 100.0,
        }
    }
}
impl Block for Tremolo {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "mod.tremolo",
            name: "Tremolo",
            category: Category::Modulation,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec {
                    id: "rate",
                    name: "Rate",
                    unit: Unit::Hertz,
                    min: 0.1,
                    max: 5.0,
                    default: 0.6,
                    curve: Curve::Log,
                },
                ParamSpec {
                    id: "depth",
                    name: "Depth",
                    unit: Unit::Linear,
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    curve: Curve::Linear,
                },
                ParamSpec {
                    id: "mix",
                    name: "Mix",
                    unit: Unit::Percent,
                    min: 0.0,
                    max: 100.0,
                    default: 100.0,
                    curve: Curve::Linear,
                },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        // Re-create the LFO at the block's sample rate so the phase
        // increment (`rate_hz / sample_rate`) is correct for the host rate.
        self.lfo = pedal_dsp::lfo::Lfo::new(0.6, sr);
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let rate = ctx.param(0);
        let depth = ctx.param(1);
        let mix = ctx.param(2) / 100.0;
        // The LFO frequency is applied per block. Because `process` advances
        // the phase by exactly `rate_hz / sample_rate` and only ever wraps
        // forward, consecutive blocks hand off with the phase continuous and
        // no step at the boundary — so no click.
        self.lfo.set_rate(rate);
        self.depth = depth;
        self.mix = mix;
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            // Map the LFO modulator (-1..1) to 0..1, then to a gain in
            // [1 - depth, 1]. `depth = 0` therefore yields a constant gain
            // of 1 (bypass).
            let m = self.lfo.process() * 0.5 + 0.5;
            let gain = 1.0 - self.depth * (1.0 - m);
            let wet = input[i] * gain;
            output[i] = pedal_dsp::util::mix(input[i], wet, self.mix);
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.lfo.reset();
    }
}
/// The Looper effect (Time category).  Records a take into a fixed
        /// circular buffer and plays it back with a constant-power, two-head
        /// crossfade at the loop boundary; overdub appends with feedback and
        /// decay; clear resets.
        #[derive(Default)]
        pub struct Looper {
            inner: Option<LooperInner>,
        }

        /// A fixed-size circular buffer for loop recording and playback.
        /// Real-time safe: no allocation, locking, or blocking in the audio
        /// path.  `N` is the number of samples (the maximum loop length).
        struct LoopBuffer<const N: usize> {
            buf: [f32; N],
            cursor: usize,
            read: usize,
        }

        impl<const N: usize> LoopBuffer<N> {
            pub fn new() -> Self {
                Self { buf: [0.0; N], cursor: 0, read: 0 }
            }

            fn w(&self, i: usize) -> usize {
                // Sample-accurate wrap.  `N` is always 480_000, which is much
                // larger than the 256-sample block size, so the write head can
                // never wrap mid-block (a wrap would corrupt the crossfade seam).
                if i >= N {
                    i % N
                } else {
                    i
                }
            }

            pub fn write(&mut self, sample: f32) {
                self.buf[self.cursor] = sample;
                self.cursor = self.w(self.cursor + 1);
            }

            pub fn read(&self, pos: usize) -> f32 {
                self.buf[self.w(pos)]
            }

            pub fn reset(&mut self) {
                self.buf = [0.0; N];
                self.cursor = 0;
                self.read = 0;
            }
        }

        /// Maximum loop length: 10 s of audio at 48 kHz.
        const LOOP_MAX_SAMPLES: usize = 480_000;
        /// Half the buffer, used as the constant-power crossfade region.
        const LOOP_HALF: usize = LOOP_MAX_SAMPLES / 2;

        #[derive(Clone, Copy, PartialEq)]
        enum State {
            Idle,
            Recording,
            Playing,
            Overdubbing,
            Stopped,
        }

        struct LooperInner {
            buf: LoopBuffer<{ LOOP_MAX_SAMPLES }>,
            cursor: usize,
            read: usize,
            delay: usize,
            state: State,
            gate_held: [bool; 5],
            take_start: u64,
            sample_count: u64,
            /// Set when `trig_clear` fires: the buffer is zeroed and playback is
            /// silent until a new take is recorded.
            cleared: bool,
        }

        impl LooperInner {
            fn reset(&mut self) {
                self.buf.reset();
                self.cursor = 0;
                self.read = 0;
                self.delay = 0;
                self.state = State::Idle;
                self.gate_held = [false; 5];
                self.take_start = 0;
                self.sample_count = 0;
                self.cleared = false;
            }
        }
        impl Default for LooperInner {
            fn default() -> Self {
                Self {
                    buf: LoopBuffer::new(),
                    cursor: 0,
                    read: 0,
                    delay: 0,
                    state: State::Idle,
                    gate_held: [false; 5],
                    take_start: 0,
                    sample_count: 0,
                    cleared: false,
                }
            }
        }

        impl Looper {
            fn inner(&mut self) -> &mut LooperInner {
                self.inner.get_or_insert_with(LooperInner::default)
            }
        }
impl Block for Looper {
            fn spec(&self) -> BlockSpec {
                BlockSpec {
                    id: "time.looper",
                    name: "Looper",
                    category: Category::Time,
                    inputs: vec![PortSpec::audio("in")],
                    outputs: vec![PortSpec::audio("out")],
                    params: vec![
                        ParamSpec { id: "trig_record", name: "Record", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear },
                        ParamSpec { id: "trig_play", name: "Play", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear },
                        ParamSpec { id: "trig_overdub", name: "Overdub", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear },
                        ParamSpec { id: "trig_stop", name: "Stop", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear },
                        ParamSpec { id: "trig_clear", name: "Clear", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear },
                        ParamSpec { id: "time", name: "Loop length", unit: Unit::Milliseconds, min: 250.0, max: 10000.0, default: 1000.0, curve: Curve::Log },
                        ParamSpec { id: "feedback", name: "Feedback", unit: Unit::Percent, min: 0.0, max: 95.0, default: 35.0, curve: Curve::Linear },
                        ParamSpec { id: "mix", name: "Mix", unit: Unit::Percent, min: 0.0, max: 100.0, default: 100.0, curve: Curve::Linear },
                        ParamSpec { id: "level", name: "Level", unit: Unit::Percent, min: 0.0, max: 100.0, default: 100.0, curve: Curve::Linear },
                        ParamSpec { id: "half", name: "Half speed", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear },
                        ParamSpec { id: "reverse", name: "Reverse", unit: Unit::Linear, min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear },
                    ],
                    controls_in: vec![],
                    controls_out: vec![],
                }
            }

            fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
                self.inner = Some(LooperInner::default());
                Ok(())
            }

            fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
                let c = self.inner();
                let n = ctx.n;
                if n == 0 {
                    return Ok(());
                }

                // --- Snapshot parameters (executor snapshot, per-frame safety) ------
                let trig_record = ctx.param(0);
                let trig_play = ctx.param(1);
                let trig_overdub = ctx.param(2);
                let trig_stop = ctx.param(3);
                let trig_clear = ctx.param(4);
                let time_ms = ctx.param(5);
                let feedback = ctx.param(6) / 100.0;
                let mix = ctx.param(7) / 100.0;
                let level = ctx.param(8) / 100.0;
                let half = ctx.param(9) >= 0.5;
                let reverse = ctx.param(10) >= 0.5;

                // --- Trigger sample-and-hold; detect rising edges --------------------
                let rec = trig_record >= 0.5 && !c.gate_held[0];
                let play = trig_play >= 0.5 && !c.gate_held[1];
                let ovd = trig_overdub >= 0.5 && !c.gate_held[2];
                let stop = trig_stop >= 0.5 && !c.gate_held[3];
                let clr = trig_clear >= 0.5 && !c.gate_held[4];
                c.gate_held = [rec, play, ovd, stop, clr];

                // --- State machine ----------------------------------------------------
                // Record/Play/Overdub are idempotent while already in that state;
                // Stop and Clear are the only ones that force a transition out.
                match c.state {
                    State::Idle => {
                        if rec { c.state = State::Recording; c.take_start = c.sample_count; }
                        else if ovd { c.state = State::Overdubbing; c.take_start = c.sample_count; }
                        else if play { c.state = State::Playing; c.take_start = c.sample_count; }
                        else if stop { c.state = State::Stopped; }
                        else if clr { c.state = State::Stopped; c.cleared = true; }
                    }
                    State::Recording => {
                        if play { c.state = State::Playing; }
                        else if ovd { c.state = State::Overdubbing; }
                        else if stop { c.state = State::Stopped; }
                        else if clr { c.state = State::Stopped; c.cleared = true; }
                        else if rec { c.state = State::Stopped; } // Record falling.
                        // held Record: stay in Recording
                    }
                    State::Playing => {
                        if ovd { c.state = State::Overdubbing; }
                        else if stop { c.state = State::Stopped; }
                        else if clr { c.state = State::Stopped; c.cleared = true; }
                        else if play { c.state = State::Stopped; } // Play falling.
                        // held Play: stay in Playing
                    }
                    State::Overdubbing => {
                        if ovd { c.state = State::Playing; } // Overdub falling.
                        else if stop { c.state = State::Stopped; }
                        else if clr { c.state = State::Stopped; c.cleared = true; }
                        // held Overdub: stay in Overdubbing
                    }
                    State::Stopped => {
                        if rec { c.state = State::Recording; }
                        else if ovd { c.state = State::Overdubbing; }
                        else if play { c.state = State::Playing; }
                        // Stop/Clear held: stay in Stopped
                    }
                }

                // Finalising triggers: the take ends, and the clear zeroes the
                // buffer for a fresh take.
                if clr {
                    c.buf.reset();
                }

                // Loop length in samples (clamped to the buffer size).
                let time_samples = (time_ms * ctx.sample_rate / 1000.0).round() as usize;
                let time_samples = time_samples.max(250).min(LOOP_MAX_SAMPLES);

                for i in 0..n {
                    c.sample_count += 1;

                    match c.state {
                        State::Idle | State::Stopped => {
                            if c.cleared {
                                ctx.outputs[0][i] = 0.0;
                            } else {
                                ctx.outputs[0][i] = ctx.inputs[0][i];
                            }
                        }
                        State::Recording => {
                            // Write input into the buffer (record phase).
                            c.buf.write(ctx.inputs[0][i]);
                            c.cursor = c.buf.w(c.cursor + 1);
                            c.read = 0;
                            c.delay = c.cursor;
                            // Auto-end recording when the buffer is full.
                            if c.delay >= LOOP_MAX_SAMPLES {
                                c.state = State::Stopped;
                                c.read = 0;
                            }
                            ctx.outputs[0][i] = ctx.inputs[0][i];
                        }
                        State::Playing | State::Overdubbing => {
                            let r = c.read;
                            let dir = if reverse { -1 } else { 1 };
                            let speed = if half { 2 } else { 1 };
                            let next = c.buf.w((r as i32 + dir * speed) as usize);
                            c.read = next;

                            // Constant-power two-head read at the loop
                            // boundary: out = cos(t·π/2)·a + sin(t·π/2)·b.
                            let a = c.buf.read(r);
                            let b = c.buf.read(c.buf.w(r + LOOP_HALF));
                            let t = ((r % LOOP_HALF) as f32) / (LOOP_HALF as f32);
                            let theta = t * core::f32::consts::PI / 2.0;
                            let out = theta.cos() * a + theta.sin() * b;

                            // 512-sample linear fade-in from take start.
                            let fade_in = ((c.sample_count.saturating_sub(c.take_start)) as f32 / 512.0).min(1.0);

                            let wet = out * fade_in * feedback;
                            if c.state == State::Overdubbing {
                                c.buf.write(ctx.inputs[0][i] + wet);
                            } else {
                                c.buf.write(wet);
                            }
                            c.cursor = c.buf.w(c.cursor + 1);

                            // Loop length = write head position minus read head.
                            c.delay = (c.cursor - c.read + LOOP_MAX_SAMPLES) % LOOP_MAX_SAMPLES;

                            // Output: dry + wet.
                            let dry = ctx.inputs[0][i] * (1.0 - mix);
                            let wet_out = out * fade_in * mix * level;
                            ctx.outputs[0][i] = dry + wet_out;
                        }
                    }
                }

                Ok(())
            }

            fn reset(&mut self) {
                if let Some(c) = &mut self.inner {
                    c.reset();
                }
            }
        }


pub struct GraphicEq {
    bands: [Biquad; 10],
    gains: [f32; 10],
}

impl Default for GraphicEq {
    fn default() -> Self {
        Self {
            bands: [Biquad::identity(); 10],
            gains: [0.0; 10],
        }
    }
}

impl Block for GraphicEq {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "tone.graphic_eq",
            name: "Graphic EQ",
            category: Category::Tone,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "gain0", name: "Band 1", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain1", name: "Band 2", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain2", name: "Band 3", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain3", name: "Band 4", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain4", name: "Band 5", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain5", name: "Band 6", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain6", name: "Band 7", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain7", name: "Band 8", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain8", name: "Band 9", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "gain9", name: "Band 10", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        for (i, band) in self.bands.iter_mut().enumerate() {
            // 0 dB is identity: design() with gain 0 yields the exact
            // pass-through coefficients (b1/b2 = 0, a1/a2 = 0).
            band.design(BiquadKind::Peak, sr, G_EQ_CENTRES_HZ[i], 1.0, 0.0);
        }
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        // Snapshot the live gains, then bake each into the biquad
        // coefficients for this sample — exactly like `ToneStack`.
        // 0 dB is a per-band identity, so the cascade at 0 dB is
        // sample-exact.
        for (i, db) in self.gains.iter_mut().enumerate() {
            *db = ctx.param(i);
        }
        for (i, band) in self.bands.iter_mut().enumerate() {
            band.design(BiquadKind::Peak, ctx.sample_rate, G_EQ_CENTRES_HZ[i], 1.0, self.gains[i]);
        }
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let mut s = input[i];
            for band in self.bands.iter_mut() {
                s = band.process(s);
            }
            output[i] = s;
        }
        Ok(())
    }

    fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
    }
}

/// ISO 1/1-octave centre frequencies (Hz) for the 10-band graphic EQ.
const G_EQ_CENTRES_HZ: [f32; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
pub fn builtin_registry() -> crate::Registry {
    let mut r = crate::Registry::new();
    r.add::<IoInput>();
    r.add::<GraphicEq>();
    r.add::<IoOutput>();
    r.add::<Gain>();
    r.add::<MixSum>();
    r.add::<Sum>();
    r.add::<Master>();
    r.add::<Overdrive>();
    r.add::<TubeScreamer>();
    r.add::<PrecisionDrive>();
    r.add::<Klon>();
    r.add::<BigMuff>();
    r.add::<Amp>();
    r.add::<Cab>();
    r.add::<ToneStack>();
    r.add::<Delay>();
    r.add::<PitchDelay>();
    r.add::<Reverb>();
    r.add::<ShimmerReverb>();
    r.add::<Nam>();
    r.add::<Compressor>();
    r.add::<MultibandCompressor>();
    r.add::<Gate>();
    r.add::<Tremolo>();
    r.add::<Octaver>();
    r.add::<Chorus>();
    r.add::<Looper>();
    r.add::<crate::waveshaper::WaveShaper>();
    r.add::<crate::fuzz::Fuzz>();
    r
}
/// A four-input summer. The output stage of a multi-channel rig: every
/// channel master feeds one input, so the pedal's output is their sum.
#[derive(Default)]
pub struct Sum {
    levels: [f32; 4],
}
impl Block for Sum {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "util.sum",
            name: "Sum",
            category: Category::Split,
            inputs: vec![
                PortSpec::audio("a"),
                PortSpec::audio("b"),
                PortSpec::audio("c"),
                PortSpec::audio("d"),
            ],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "a", name: "A", unit: Unit::Ratio, min: 0.0, max: 1.0, default: 1.0, curve: Curve::Linear },
                ParamSpec { id: "b", name: "B", unit: Unit::Ratio, min: 0.0, max: 1.0, default: 1.0, curve: Curve::Linear },
                ParamSpec { id: "c", name: "C", unit: Unit::Ratio, min: 0.0, max: 1.0, default: 1.0, curve: Curve::Linear },
                ParamSpec { id: "d", name: "D", unit: Unit::Ratio, min: 0.0, max: 1.0, default: 1.0, curve: Curve::Linear },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _sr: f32, _max: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        for i in 0..4 {
            self.levels[i] = ctx.param(i);
        }
        let input = ctx.inputs;
        let out = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            let mut s = 0.0;
            for (k, inp) in input.iter().enumerate() {
                if k < 4 {
                    s += inp.get(i).copied().unwrap_or(0.0) * self.levels[k];
                }
            }
            out[i] = s;
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
// ---------------------------------------------------------------------------
// NAM (Neural Amp Modeler)
// ---------------------------------------------------------------------------
use std::cell::RefCell;
/// A Neural Amp Modeler model: the actual amp capture.
///
/// `nam-rs` does the inference (WaveNet / LSTM / A2 containers) with no
/// allocation on the audio path. The model is loaded at compile time and held
/// behind a `RefCell` because the `Block` trait's `process` takes `&mut self`
/// but the model is built once and only its streaming state changes.
///
/// **The model must be fed at its own sample rate.** A `.nam` captured at
/// 48 kHz fed 44.1 kHz produces silently wrong output. `prepare` records the
/// mismatch and the executor reports it; the UI shows it.
pub struct Nam {
    model: RefCell<Option<nam_rs::Model>>,
    nam: RefCell<Option<nam_rs::NamModel>>,
    in_gain: f32,
    out_level: f32,
    mix: f32,
    /// Model sample rate vs. the rate the block was prepared at.
    rate_mismatch: RefCell<Option<(f64, f32)>>,
    /// `model_rate / host_rate` once prepared.
    ratio: RefCell<f32>,
    /// Resampler state (last four samples) for each direction.
    up_state: RefCell<[f32; 4]>,
    down_state: RefCell<[f32; 4]>,
    scratch: RefCell<Vec<f32>>,
    /// Model-rate scratch, `scratch * ratio` frames.
    scratch_hi: RefCell<Vec<f32>>,
}
impl Default for Nam {
    fn default() -> Self {
        Self {
            model: RefCell::new(None),
            nam: RefCell::new(None),
            in_gain: 0.0,
            out_level: 0.0,
            mix: 1.0,
            rate_mismatch: RefCell::new(None),
            ratio: RefCell::new(1.0),
            up_state: RefCell::new([0.0; 4]),
            down_state: RefCell::new([0.0; 4]),
            scratch: RefCell::new(Vec::new()),
            scratch_hi: RefCell::new(Vec::new()),
        }
    }
}
impl Nam {
    /// Load a `.nam` file. Call this before `prepare`; the model is built at
    /// prepare time (off the audio thread).
    pub fn load(&self, path: &str) -> Result<(), String> {
        let m = nam_rs::NamModel::from_file(path).map_err(|e| e.to_string())?;
        *self.nam.borrow_mut() = Some(m);
        Ok(())
    }
    /// Load from JSON text (used by tests and by the preset loader).
    pub fn load_json(&self, json: &str) -> Result<(), String> {
        let m = nam_rs::NamModel::from_json_str(json).map_err(|e| e.to_string())?;
        *self.nam.borrow_mut() = Some(m);
        Ok(())
    }
    /// Load a `.nam` file from disk. Convenience over reading +
    /// [`Self::load_json`]; kept here so the block owns its file format.
    pub fn load_file(&self, path: &str) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        self.load_json(&text)
    }
    pub fn is_loaded(&self) -> bool {
        self.nam.borrow().is_some()
    }
    /// The model's expected sample rate, if loaded.
    pub fn expected_rate(&self) -> Option<f64> {
        self.nam.borrow().as_ref().map(|m| m.expected_sample_rate())
    }
    /// The model's display name, if the file carries metadata.
    pub fn display_name(&self) -> String {
        self.nam
            .borrow()
            .as_ref()
            .map(|m| {
                let md = m.metadata_typed();
                md.name.unwrap_or_else(|| "NAM model".into())
            })
            .unwrap_or_else(|| "NAM (no model)".into())
    }
}
impl Block for Nam {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "mod.nam",
            name: "NAM",
            category: Category::Modelling,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "gain", name: "Input gain", unit: Unit::Decibels, min: -24.0, max: 24.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "level", name: "Output level", unit: Unit::Decibels, min: -30.0, max: 12.0, default: 0.0, curve: Curve::AmpGain },
                ParamSpec { id: "mix", name: "Mix", unit: Unit::Percent, min: 0.0, max: 100.0, default: 100.0, curve: Curve::Linear },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, sr: f32, max_block: usize) -> Result<(), BlockError> {
        self.rate_mismatch.borrow_mut().take();
        let expected = match self.nam.borrow().as_ref() {
            Some(n) => n.expected_sample_rate(),
            None => {
                // No model: the block passes audio through, and the UI says so.
                return Ok(());
            }
        };
        let nam = self.nam.borrow();
        let nam = nam.as_ref().expect("checked above");
        let model = nam_rs::Model::from_nam(nam).map_err(|e| BlockError::Resource(e.to_string()))?;
        if (expected - sr as f64).abs() > 0.5 {
            *self.rate_mismatch.borrow_mut() = Some((expected, sr));
        }
        let ratio = (expected as f32 / sr).max(1.0 / 8.0).min(8.0);
        *self.ratio.borrow_mut() = ratio;
        let hi = pedal_dsp::resample_out_len(max_block, ratio);
        self.scratch.borrow_mut().resize(max_block, 0.0);
        self.scratch_hi.borrow_mut().resize(hi, 0.0);
        *self.model.borrow_mut() = Some(model);
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        self.in_gain = ctx.param(0);
        self.out_level = ctx.param(1);
        self.mix = ctx.param(2) / 100.0;
        let g = pedal_dsp::util::db_to_lin(self.in_gain);
        let out_gain = pedal_dsp::util::db_to_lin(self.out_level);
        let input = ctx.inputs[0];
        let out = &mut ctx.outputs[0][..];
        let mut model = self.model.borrow_mut();
        let Some(model) = model.as_mut() else {
            // No model loaded: pass through, so the rest of the rig still works.
            for i in 0..ctx.n {
                out[i] = input[i] * g;
            }
            return Ok(());
        };
        let ratio = *self.ratio.borrow();
        let mut scratch = self.scratch.borrow_mut();
        let mut scratch_hi = self.scratch_hi.borrow_mut();
        let mut up = self.up_state.borrow_mut();
        let mut down = self.down_state.borrow_mut();
        // Host rate → model rate → model → host rate.
        for i in 0..ctx.n {
            scratch[i] = input[i] * g;
        }
        let hi_n = pedal_dsp::resample_out_len(ctx.n, ratio);
        pedal_dsp::resample(&scratch[..ctx.n], &mut scratch_hi[..hi_n], ratio, &mut up);
        model.process_buffer(&mut scratch_hi[..hi_n]);
        pedal_dsp::resample(&scratch_hi[..hi_n], &mut scratch[..ctx.n], 1.0 / ratio, &mut down);
        for i in 0..ctx.n {
            let wet = scratch[i] * out_gain;
            out[i] = pedal_dsp::util::mix(input[i] * g, wet, self.mix);
        }
        Ok(())
    }
    fn rate_mismatch(&self) -> Option<(f64, f32)> {
        *self.rate_mismatch.borrow()
    }
    fn latency_samples(&self) -> u32 {
        // Cubic resampling is linear-phase with a 2-sample group delay; the
        // model itself is causal. The executor compensates for this.
        if (*self.ratio.borrow() - 1.0).abs() < 1e-6 {
            0
        } else {
            2
        }
    }
    fn reset(&mut self) {
        if let Some(m) = self.model.borrow_mut().as_mut() {
            m.reset();
        }
    }
    fn configure(&mut self, key: &str, value: &str) -> Result<(), BlockError> {
        if key == "model" {
            self.load_file(value).map_err(BlockError::Resource)
        } else {
            Ok(())
        }
    }
}
