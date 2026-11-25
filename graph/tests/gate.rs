//! Verification tests for the [`Gate`] (noise gate) block.
//!
//! The gate attenuates audio below a configurable threshold and passes audio
//! above it. These tests feed known sine waves and check the output level
//! — the same pattern used for the other effects in this workspace.
//!
//! The gate owns an envelope follower: on a sudden level change the output
//! briefly ramps (attack on level rises, release on level falls). The
//! steady-state tests therefore warm the gate up before comparing, and the
//! transient tests open or close their analysis window on the first samples
//! of the step, before the envelope settles.
//!
//!     cargo test -p dsp-graph --test gate
//!
//! See [`Gate`] for the block implementation.

use dsp_graph::blocks::{builtin_registry, Gate, IoInput, IoOutput};
use dsp_graph::{Executor, Graph};
use dsp_graph::types::{Category, PortKind, Unit};

/// 2π.
const TAU: f32 = core::f32::consts::TAU;

/// Sample rate and block size shared by the gate tests.
const SR: f32 = 48_000.0;
const BLOCK: usize = 4096;

/// The gate spec: id, name, category, and parameter declarations.
fn gate_spec() -> dsp_graph::types::BlockSpec {
    builtin_registry()
        .get("dynamics.noise_gate")
        .expect("dynamics.noise_gate should be registered")
        .clone()
}

/// Build a minimal rig: in -> gate -> out, with the given real values.
fn gate_graph(threshold: f32, attack: f32, release: f32, mix: f32) -> Graph {
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("gate", Box::new(Gate::default())).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "gate", "in").unwrap();
    g.connect("gate", "out", "out", "in").unwrap();
    g.set_param_real("gate", "threshold", threshold).unwrap();
    g.set_param_real("gate", "attack", attack).unwrap();
    g.set_param_real("gate", "release", release).unwrap();
    g.set_param_real("gate", "mix", mix).unwrap();
    g
}

/// Run the gate rig over the whole `input` in `BLOCK`-sized chunks and
/// return the concatenated output. The gate's envelope starts at zero, so
/// the first block(s) contain the attack/release transient.
fn run_gate(threshold: f32, attack: f32, release: f32, mix: f32, input: &[f32]) -> Vec<f32> {
    let mut g = gate_graph(threshold, attack, release, mix);
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let chunk = input[i..].len().min(BLOCK);
        let chunk_in = &input[i..(i + chunk)];
        let chunk_out = ex.run(chunk_in, chunk).unwrap();
        out.extend_from_slice(chunk_out);
        i += chunk;
    }
    out
}

/// Run the gate rig and discard the first `warmup` blocks, returning only the
/// output after the envelope has reached steady state.
fn run_gate_warmup(
    threshold: f32,
    attack: f32,
    release: f32,
    mix: f32,
    input: &[f32],
    warmup: usize,
) -> Vec<f32> {
    let mut g = gate_graph(threshold, attack, release, mix);
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();
    // Run the warmup blocks and drop their output.
    let mut i = 0;
    while i < (warmup * BLOCK).min(input.len()) {
        let chunk = input[i..].len().min(BLOCK);
        let chunk_in = &input[i..(i + chunk)];
        let _chunk_out = ex.run(chunk_in, chunk).unwrap();
        i += chunk;
    }
    // Continue to the end, collecting output.
    let mut out = Vec::with_capacity(input.len() - warmup * BLOCK);
    while i < input.len() {
        let chunk = input[i..].len().min(BLOCK);
        let chunk_in = &input[i..(i + chunk)];
        let chunk_out = ex.run(chunk_in, chunk).unwrap();
        out.extend_from_slice(chunk_out);
        i += chunk;
    }
    out
}

/// A steady sine at the given dBFS amplitude (peak = `10.0^(amp_db / 20)`).
fn sine_amplitude(amplitude_db: f32, n: usize) -> Vec<f32> {
    let amp = 10.0_f32.powf(amplitude_db / 20.0);
    (0..n)
        .map(|i| {
            (TAU * 440.0 * i as f32 / SR).sin() * amp
        })
        .collect()
}

/// A steady input tone must come out unchanged when it sits above the
/// threshold — the gate opens fully (gain = 1) and the mix knob is at 100%.
#[test]
fn gate_passes_signal_above_threshold() {
    // Input at -20 dB: the envelope converges near the DC component of the
    // full-wave rectified sine, ~0.064 (-23.9 dB). With the threshold at the
    // minimum (-60 dB) the excess is strongly positive, so gain = 1 and
    // out = input * 1 = input. The first blocks contain the envelope's
    // attack transient, so the comparison is made on the steady-state
    // tail only.
    let amplitude_db = -20.0;
    let threshold_db = -60.0;
    let input = sine_amplitude(amplitude_db, 64 * BLOCK);
    let output = run_gate_warmup(threshold_db, 1.0, 5.0, 100.0, &input, 4);

    // Compare only the steady-state tail (skip the warmup).
    let expected = &input[4 * BLOCK..];
    assert_eq!(expected, output.as_slice(), "gate should pass signals above threshold unchanged");
}

/// A signal far below the threshold is attenuated by the full 60 dB.
#[test]
fn gate_attentuates_signal_below_threshold() {
    // Input at -60 dB (amplitude 1e-3). The envelope converges near the DC
    // component of the abs sine (~6.4e-4, -63.9 dB), which is below the 0 dB
    // threshold by more than 60 dB. The gate closes by the full 60 dB:
    // gain = 1e-3, so the 1e-3 input comes out at ~1e-6.
    let amplitude_db = -60.0;
    let threshold_db = 0.0;
    let input = sine_amplitude(amplitude_db, 64 * BLOCK);
    let output = run_gate_warmup(threshold_db, 1.0, 5.0, 100.0, &input, 4);

    // With a settled envelope, the gate applies 60 dB of gain reduction.
    let peak_out = output.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
    // attenuation = 60 dB => gain = 1e-3; peak_out ≈ peak_in * 1e-3 = 1e-6.
    assert!(peak_out < 1e-5, "expected < 1e-5 (over-attenuated), got {peak_out}");
    assert!(peak_out > 1e-7, "expected > 1e-7 (under-attenuated), got {peak_out}");
}

/// Raising the threshold increases the attenuation of a fixed-level input:
/// a signal can sit below threshold by a larger margin and therefore gets a
/// stronger gain reduction.
#[test]
fn gate_threshold_controls_attenuation() {
    let amplitude_db = -20.0; // peak = 0.1
    let n = 64 * BLOCK;
    let input = sine_amplitude(amplitude_db, n);

    // Threshold at -20 dB: envelope (-23.9 dB) sits 3.9 dB below threshold.
    let out_low = run_gate_warmup(-20.0, 1.0, 5.0, 100.0, &input, 4);
    // Threshold at -10 dB: envelope sits 13.9 dB below threshold -> stronger
    // reduction.
    let out_high = run_gate_warmup(-10.0, 1.0, 5.0, 100.0, &input, 4);

    let peak_low = out_low.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
    let peak_high = out_high.iter().map(|x| x.abs()).fold(0.0f32, f32::max);

    // Higher threshold -> lower output peak (more attenuation).
    assert!(peak_high < peak_low, "higher threshold should attenuate more");
    // At -20 dB threshold the gain is ~0.637 (3.9 dB reduction), so peak ~0.064.
    // Allow the envelope's attack transient to raise the peak slightly.
    assert!(peak_low < 0.10, "low threshold peak {peak_low} too high");
    assert!(peak_low > 0.05, "low threshold peak {peak_low} too low");
}

/// A fast attack lets the gate open quickly on a sudden onset; a slow attack
/// keeps attenuating during the envelope's rise.
#[test]
fn gate_attack_controls_opening_speed() {
    // Input: silence for the first half, then a -20 dB sine for the second.
    let n = 64 * BLOCK;
    let mut input = vec![0.0f32; n];
    for i in (32 * BLOCK)..n {
        input[i] = (TAU * 440.0 * i as f32 / SR).sin() * 0.1;
    }

    let fast = run_gate(-30.0, 1.0, 5.0, 100.0, &input);
    let slow = run_gate(-30.0, 10.0, 5.0, 100.0, &input);

    // The first 200 samples after the onset capture the attack transient.
    let onset = 32 * BLOCK;
    let window_fast = &fast[onset..onset + 200];
    let window_slow = &slow[onset..onset + 200];
    let peak_fast = window_fast
        .iter()
        .map(|x| x.abs())
        .fold(0.0f32, f32::max);
    let peak_slow = window_slow
        .iter()
        .map(|x| x.abs())
        .fold(0.0f32, f32::max);

    // The fast attack opens sooner, so a higher peak survives the transient.
    assert!(peak_fast > peak_slow, "fast attack should open faster (got {peak_fast} vs {peak_slow})");
}

/// A fast release lets the gate close quickly when the signal drops; a slow
/// release keeps attenuating for longer.
#[test]
fn gate_release_controls_closing_speed() {
    // Input: a -20 dB sine for the first half, then a -60 dB sine for the
    // second. The -60 dB tone sits below the -30 dB threshold, so the gate
    // closes. The release time constant controls how quickly the envelope
    // falls, and therefore how long the gate stays open.
    let n = 64 * BLOCK;
    let mut input = vec![0.0f32; n];
    for i in (32 * BLOCK)..(48 * BLOCK) {
        input[i] = (TAU * 440.0 * i as f32 / SR).sin() * 0.1;
    }
    for i in (48 * BLOCK)..n {
        input[i] = (TAU * 440.0 * i as f32 / SR).sin() * 0.001;
    }

    let fast = run_gate(-30.0, 1.0, 5.0, 100.0, &input);
    let slow = run_gate(-30.0, 1.0, 50.0, 100.0, &input);

    // The first 1000 samples of the second half capture the release transient.
    // The fast release (5 ms) closes the gate within ~200 samples; the slow
    // release (50 ms) keeps the gate open for ~1500 samples.
    let second_half = 48 * BLOCK;
    let window_fast = &fast[second_half..second_half + 1000];
    let window_slow = &slow[second_half..second_half + 1000];
    let peak_fast = window_fast
        .iter()
        .map(|x| x.abs())
        .fold(0.0f32, f32::max);
    let peak_slow = window_slow
        .iter()
        .map(|x| x.abs())
        .fold(0.0f32, f32::max);

    // The fast release closes sooner, so the output drops faster (lower peak).
    assert!(peak_fast < peak_slow, "fast release should close faster (got {peak_fast} vs {peak_slow})");
}

/// The mix knob sets the dry/wet balance: mix = 0 returns the input
/// unchanged (dry), mix = 100 returns only the processed (wet) signal.
#[test]
fn gate_mix_controls_dry_wet_balance() {
    let amplitude_db = -60.0;
    let threshold_db = 0.0;
    let input = sine_amplitude(amplitude_db, 64 * BLOCK);

    // mix = 0: dry path only, output equals the input (steady-state portion).
    let out_mix0 = run_gate_warmup(threshold_db, 1.0, 5.0, 0.0, &input, 4);
    let expected = &input[4 * BLOCK..];
    assert_eq!(expected, out_mix0.as_slice(), "mix = 0 must pass the input unchanged");

    // mix = 100: wet path only, output is the attenuated signal (~1e-6).
    let out_mix100 = run_gate_warmup(threshold_db, 1.0, 5.0, 100.0, &input, 4);
    let peak = out_mix100
        .iter()
        .map(|x| x.abs())
        .fold(0.0f32, f32::max);
    assert!(peak < 1e-5, "mix = 100 should be the attenuated wet signal, got {peak}");
}

/// The gate is present in the registry with the expected name, category and
/// parameter declarations.
#[test]
fn gate_is_in_the_registry() {
    let spec = gate_spec();

    assert_eq!(spec.id, "dynamics.noise_gate", "block id");
    assert_eq!(spec.name, "Noise gate", "block name");
    assert_eq!(
        spec.category, Category::Dynamics,
        "block category"
    );
    assert_eq!(spec.inputs.len(), 1, "one audio input");
    assert_eq!(spec.inputs[0].name, "in", "audio input name");
    assert_eq!(spec.inputs[0].kind, PortKind::Audio, "audio input kind");
    assert_eq!(spec.outputs.len(), 1, "one audio output");
    assert_eq!(spec.outputs[0].name, "out", "audio output name");
    assert_eq!(spec.outputs[0].kind, PortKind::Audio, "audio output kind");

    // threshold: Decibels, -60..0, default -40, AmpGain curve.
    assert_param(&spec, "threshold", "Threshold", Unit::Decibels, -60.0, 0.0, -40.0, dsp_graph::types::Curve::AmpGain);
    // attack: Milliseconds, 0.1..10, default 1, Log curve.
    assert_param(&spec, "attack", "Attack", Unit::Milliseconds, 0.1, 10.0, 1.0, dsp_graph::types::Curve::Log);
    // release: Milliseconds, 1..100, default 5, Log curve.
    assert_param(&spec, "release", "Release", Unit::Milliseconds, 1.0, 100.0, 5.0, dsp_graph::types::Curve::Log);
    // mix: Percent, 0..100, default 100, Linear curve.
    assert_param(&spec, "mix", "Mix", Unit::Percent, 0.0, 100.0, 100.0, dsp_graph::types::Curve::Linear);
}

/// A parameter declaration.
fn assert_param(spec: &dsp_graph::types::BlockSpec, id: &str, name: &str, unit: Unit,
                min: f32, max: f32, default: f32, curve: dsp_graph::types::Curve) {
    let p = spec
        .params
        .iter()
        .find(|p| p.id == id)
        .expect("parameter id present");
    assert_eq!(p.name, name, "parameter {id}");
    assert_eq!(p.unit, unit, "parameter {id}");
    assert!((p.min - min).abs() < 1e-6, "parameter {id} min");
    assert!((p.max - max).abs() < 1e-6, "parameter {id} max");
    assert!((p.default - default).abs() < 1e-6, "parameter {id} default");
    assert_eq!(p.curve, curve, "parameter {id}");
}
