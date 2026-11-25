//! `fuzz.fuzz` — three-model fuzz block (Fuzz Face / Big Muff / Tone Bender).
//! Design: `docs/design/fuzz.md`. DSP lives in `pedal_dsp::fuzz`.

use crate::prelude::*;
use pedal_dsp::fuzz::{Fuzz as FuzzDsp, FuzzModel, FUZZ_MODELS};

pub struct Fuzz {
    dsp: FuzzDsp,
    /// Smoothed drive / level (linear), one-pole per sample: no zipper noise.
    drive_s: f32,
    level_s: f32,
}

impl Default for Fuzz {
    fn default() -> Self {
        let dsp = FuzzDsp::new();
        Self { drive_s: dsp.drive, level_s: 0.5, dsp }
    }
}

fn model_from(v: f32) -> FuzzModel {
    match v.round() as i32 {
        1 => FuzzModel::Muff,
        2 => FuzzModel::Bender,
        _ => FuzzModel::Face,
    }
}

impl Block for Fuzz {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "fuzz.fuzz",
            name: "Fuzz",
            category: Category::Distortion,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![
                ParamSpec { id: "model", name: "Model", unit: Unit::Choice(FUZZ_MODELS), min: 0.0, max: 2.0, default: 0.0, curve: Curve::Linear },
                ParamSpec { id: "drive", name: "Drive", unit: Unit::Linear, min: 0.1, max: 10.0, default: 2.5, curve: Curve::Log },
                ParamSpec { id: "bass", name: "Bass", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::Linear },
                ParamSpec { id: "mid", name: "Mid", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::Linear },
                ParamSpec { id: "treble", name: "Treble", unit: Unit::Decibels, min: -12.0, max: 12.0, default: 0.0, curve: Curve::Linear },
                ParamSpec { id: "level", name: "Level", unit: Unit::Decibels, min: -40.0, max: 12.0, default: -6.0, curve: Curve::Linear },
            ],
            controls_in: vec![],
            controls_out: vec![],
        }
    }

    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> {
        self.dsp = FuzzDsp::new();
        self.dsp.set_tone(0.0, 0.0, 0.0, sr);
        self.drive_s = self.dsp.drive;
        self.level_s = 0.5;
        Ok(())
    }

    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        let model = model_from(ctx.param(0));
        let drive = ctx.param(1).clamp(0.1, 10.0);
        let (bass, mid, treble) = (ctx.param(2), ctx.param(3), ctx.param(4));
        let level = ctx.param_db(5).clamp(0.0, 4.0);
        self.dsp.set_params(model, drive, bass, mid, treble, level, ctx.sample_rate);
        // Smooth drive and level toward their targets (~5 ms).
        let k = 1.0 - (-1.0 / (0.005 * ctx.sample_rate)).exp();
        let input = ctx.inputs[0];
        let output = &mut ctx.outputs[0][..];
        for i in 0..ctx.n {
            self.drive_s += (drive - self.drive_s) * k;
            self.level_s += (level - self.level_s) * k;
            self.dsp.drive = self.drive_s;
            self.dsp.level = self.level_s;
            let y = self.dsp.process(input[i]);
            output[i] = if y.is_finite() { y } else { 0.0 };
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.dsp.reset();
    }
}
