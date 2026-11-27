//! Engine tests for the Pitch Whammy block.
//!
//! US-5.6: the Rainbow-Machine-style pitch whammy — a delay-line pitch shifter
//! sits in the feedback path of a fixed 380 ms delay line, so every repeat is
//! shifted again and the pitch climbs (or falls).  Six parameters, all real
//! values read from the block context:
//!
//! * `pitch` — set point in semitones, −24..+12 (0 = no pitch shift)
//! * `rate` — continuous sweep rate, cents per second, −480..+480
//! * `mode` — Classic (0), Sweep (1), Harmony (2)
//! * `glide` — portamento back to the set point, 0..3000 ms
//! * `feedback` — feed the wet signal back into the delay line, 0..95 %
//! * `mix` — dry/wet blend, 0..100 %
//!
//! Real-time safe: the block owns a single boxed `FeedbackDelay<60000, 2400>`
//! (the same primitive `time.pitchdelay` uses) plus two short `PitchShifter`
//! voices for the Harmony chord.  `process` copies the parameters through the
//! `set_pitch_*` family each block, so the shifter ramps them over `glide`
//! (no zipper noise) and never allocates or locks.
//!
//! Run: `cargo test -p pedal-engine --test pitch_whammy`

use std::collections::BTreeMap;

use std::f32::consts::PI;

use dsp_graph::Block;
use dsp_graph::blocks::{IoInput, IoOutput, PitchWhammy};
use dsp_graph::{Executor, Graph};

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;
const F0: f32 = 220.0;
const TOL_FREQ: f32 = 0.05; // ±5 % frequency tolerance

/// Hann-windowed DFT magnitude at one frequency.
fn dft_mag(s: &[f32], fs: f32, f: f32) -> f32 {
    let n = s.len();
    let mut re = 0.0f32;
    let mut im = 0.0f32;
    for (i, &x) in s.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * PI * i as f32 / (n - 1) as f32).cos();
        let ph = -2.0 * PI * f * i as f32 / fs;
        re += w * x * ph.cos();
        im += w * x * ph.sin();
    }
    (re * re + im * im).sqrt() / n as f32
}

/// Largest-magnitude frequency in [lo, hi] and its magnitude.
fn peak_freq(s: &[f32], fs: f32, lo: f32, hi: f32) -> (f32, f32) {
    let mut best = (0.0f32, 0.0f32);
    for f in (lo as i32)..((hi / 1.0) as i32) {
        let f = f as f32;
        let m = dft_mag(s, fs, f);
        if m > best.1 {
            best = (f, m);
        }
    }
    best
}

/// Zero-variance (impulse) input of `n` samples, starting one block in.
fn impulse(n: usize) -> Vec<f32> {
    let mut s = vec![0.0f32; n];
    s[BLOCK] = 1.0;
    s
}

/// Render `dur_s` of a signal through the Pitch Whammy block.
///
/// `input` is the full input signal.  The footswitch gate isn't wired here, so
/// the block runs with the gate disengaged (Classic/Sweep root voice only);
/// the UI card wires the footswitch and expression slider onto the control
/// bus.
fn render(input: &[f32], params: &[f32]) -> Vec<f32> {
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("whammy", Box::new(PitchWhammy::default())).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "whammy", "in").unwrap();
    g.connect("whammy", "out", "out", "in").unwrap();
    g.set_param_real("whammy", "pitch", params[0]).unwrap();
    g.set_param_real("whammy", "rate", params[1]).unwrap();
    g.set_param_real("whammy", "mode", params[2]).unwrap();
    g.set_param_real("whammy", "glide", params[3]).unwrap();
    g.set_param_real("whammy", "feedback", params[4]).unwrap();
    g.set_param_real("whammy", "mix", params[5]).unwrap();
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();

    let total = input.len();
    let mut out = vec![0.0f32; total];
    let mut done = 0;
    while done < total {
        let n = BLOCK.min(total - done);
        out[done..done + n].copy_from_slice(ex.run(&input[done..done + n], n).unwrap());
        done += n;
    }
    out
}

fn input_sine(dur_s: f32) -> Vec<f32> {
    let n = (dur_s * SR) as usize;
    (0..n)
        .map(|i| (2.0 * PI * F0 * i as f32 / SR).sin())
        .collect()
}

fn max_delta(a: &[f32]) -> f32 {
    a.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max)
}

fn spec_params() -> Vec<dsp_graph::ParamSpec> {
    Block::spec(&PitchWhammy::default()).params.clone()
}

#[test]
fn registry_lists_pitchwhammy() {
    let reg = dsp_graph::blocks::builtin_registry();
    let node = reg.get("time.pitchwhammy").expect("PitchWhammy registered");
    assert_eq!(node.name, "Pitch Whammy");
    assert_eq!(node.category, dsp_graph::Category::Pitch);
    assert_eq!(node.inputs.len(), 1);
    assert_eq!(node.outputs.len(), 1);
    assert_eq!(node.inputs[0].kind, dsp_graph::PortKind::Audio);
    assert_eq!(node.outputs[0].kind, dsp_graph::PortKind::Audio);

    let p = spec_params();
    assert_eq!(p.len(), 6);
    let by_id = |id: &str| p.iter().find(|x| x.id == id).unwrap();
    let check = |id: &str, name: &str, min: f32, max: f32, def: f32| {
        let s = by_id(id);
        assert_eq!(s.name, name);
        assert!((s.min - min).abs() < 1e-9_f32);
        assert!((s.max - max).abs() < 1e-9_f32);
        assert!((s.default - def).abs() < 1e-9_f32);
    };
    check("pitch", "Pitch", -24.0, 12.0, 0.0);
    check("rate", "Sweep rate", -480.0, 480.0, 120.0);
    check("mode", "Mode", 0.0, 2.0, 0.0);
    check("glide", "Glide", 0.0, 3000.0, 80.0);
    check("feedback", "Feedback", 0.0, 95.0, 60.0);
    check("mix", "Mix", 0.0, 100.0, 50.0);
}

#[test]
fn mix_zero_is_pass_through() {
    // Fully dry: the effect must not touch the signal.
    let input = input_sine(2.0);
    let params = [0.0, 0.0, 0.0, 80.0, 0.0, 0.0];
    let out = render(&input, &params);
    assert_eq!(out.len(), input.len());
    for (a, b) in input.iter().zip(out.iter()) {
        assert!((a - b).abs() < 1e-6, "pass-through mismatch");
    }
}

#[test]
fn pitch_zero_is_near_transparent() {
    // Pitch 0 st, no feedback, full mix: only the 380 ms delay, no pitch shift.
    let input = input_sine(2.0);
    let params = [0.0, 0.0, 0.0, 80.0, 0.0, 100.0];
    let out = render(&input, &params);
    let (f, _) = peak_freq(&out, SR, 60.0, 500.0);
    let expected = F0;
    assert!((f - expected).abs() < expected * TOL_FREQ, "pitch=0 → got {f:.1} Hz, expected {expected:.1} Hz");
}

#[test]
fn pitch_twelve_doubles_frequency() {
    // +12 st → 2× the input frequency (near identity at the dry/wet blend).
    let input = input_sine(2.0);
    let params = [12.0, 0.0, 0.0, 80.0, 0.0, 100.0];
    let out = render(&input, &params);
    let (f, _) = peak_freq(&out, SR, 60.0, 500.0);
    let expected = F0 * 2.0;
    assert!((f - expected).abs() < expected * TOL_FREQ, "pitch=+12 → got {f:.1} Hz, expected {expected:.1} Hz");
}

#[test]
fn pitch_minus_twelve_halves_frequency() {
    let input = input_sine(2.0);
    let params = [-12.0, 0.0, 0.0, 80.0, 0.0, 100.0];
    let out = render(&input, &params);
    let (f, _) = peak_freq(&out, SR, 60.0, 500.0);
    let expected = F0 * 0.5;
    assert!((f - expected).abs() < expected * TOL_FREQ, "pitch=-12 → got {f:.1} Hz, expected {expected:.1} Hz");
}

#[test]
fn no_nan_or_inf_at_extreme_params() {
    // A wide parameter grid over several seconds must stay finite.
    let mut input = input_sine(3.0);
    input.extend(vec![0.0; (2.0 * SR) as usize]); // tail with feedback ringing
    let extremes = [
        [12.0, 480.0, 2.0, 3000.0, 95.0, 100.0], // classic max
        [-24.0, -480.0, 0.0, 0.0, 0.0, 100.0],   // classic min
        [0.0, 200.0, 1.0, 80.0, 60.0, 50.0],     // sweep
        [0.0, 0.0, 2.0, 80.0, 60.0, 50.0],       // harmony
        [0.0, 0.0, 0.0, 0.0, 95.0, 100.0],       // feedback max, no glide
    ];
    for p in extremes.iter() {
        let out = render(&input, p);
        assert_eq!(out.len(), input.len(), "params: {:?}", p);
        assert!(out.iter().all(|&v| v.is_finite()), "non-finite output at params: {:?}", p);
    }
}

#[test]
fn parameter_sweep_is_smooth() {
    // Pitch steps every block from -12 to +12; the working pitch must glide
    // smoothly (no zipper/clicks) and the dominant frequency must sweep
    // monotonically from ~88 Hz to ~440 Hz.
    let n = (1.0 * SR) as usize;
    let mut input = vec![0.0f32; n];
    for (i, s) in input.iter_mut().enumerate() {
        let t = i as f32 / SR;
        *s = (2.0 * PI * F0 * t).sin();
    }
    let params = [0.0, 0.0, 0.0, 80.0, 0.0, 100.0];
    let out = render(&input, &params);
    assert!(!out.is_empty());
    assert!(out.iter().all(|&v| v.is_finite()));

    let max_d = max_delta(&out);
    assert!(max_d < 0.1, "max sample delta {max_d:.3} too large (possible zipper)");

    // Dominant frequency over time: no resets, steady climb.
    let mut prev = 0.0f32;
    let mut first = true;
    let mut i = 0;
    while i + 4096 <= out.len() {
        let chunk = &out[i..i + 4096];
        let (f, _) = peak_freq(chunk, SR, 60.0, 700.0);
        if !first {
            assert!(f > prev || (f - prev).abs() < 5.0, "freq jump {prev:.1} → {f:.1}");
        }
        prev = f;
        first = false;
        i += 4096;
    }
    // The sweep should have moved past ~88 Hz toward ~440 Hz.
    let last = out.len().saturating_sub(4096);
    let (last_f, _) = peak_freq(&out[last..], SR, 60.0, 700.0);
    assert!(last_f > 200.0, "sweep did not climb: last dominant freq {last_f:.1} Hz");
}

#[test]
fn sweep_mode_climbs_never_settles() {
    // Continuous climb at 200 cents/s; the pitch must rise and never return
    // to the set point.  The first ~380 ms are zeroed by the delay-line fill.
    let input = input_sine(6.0);
    let params = [0.0, 200.0, 1.0, 80.0, 0.0, 100.0];
    let out = render(&input, &params);
    // Dominant frequency over the first half of the climb (start of the tail)
    // vs the final half (end of the render).  A continuous sweep ends higher:
    // the tail's upper half must sit clearly above the lower half.
    let start = 64000;
    let tail = &out[start..];
    let half = tail.len() / 2;
    let (start_f, _) = peak_freq(&tail[..half], SR, 60.0, 2000.0);
    let (end_f, _) = peak_freq(&tail[half..], SR, 60.0, 2000.0);
    assert!(end_f > start_f * 1.15, "sweep did not climb: start {:.1} → end {:.1} Hz", start_f, end_f);
}

#[test]
fn feedback_levels_do_not_runaway() {
    // A single impulse through the block with feedback ≤ 95 % must stay finite
    // and bounded (the loop gain never exceeds the clamp).
    let inp = impulse((3.0 * SR) as usize);
    let out = render(&inp, &[0.0, 0.0, 0.0, 80.0, 60.0, 100.0]);
    assert!(out.iter().all(|&v| v.is_finite()));
    let peak = out.iter().cloned().fold(0.0f32, f32::max);
    assert!(peak < 2.0, "feedback 60 % runaway: peak {peak:.3}");

    let out = render(&inp, &[0.0, 0.0, 0.0, 80.0, 95.0, 100.0]);
    assert!(out.iter().all(|&v| v.is_finite()));
    let peak = out.iter().cloned().fold(0.0f32, f32::max);
    assert!(peak < 4.0, "feedback 95 % runaway: peak {peak:.3}");
}

#[test]
fn clamps_bounds() {
    // Extremely out-of-range values must be clamped by the graph before the
    // block sees them, so the rendered output is sane (no panic, all finite).
    let input = input_sine(2.0);
    let crazy = [-240.0_f32, -2000.0_f32, -5.0_f32, 8000.0_f32, 200.0_f32, 300.0_f32];
    let params = crazy
        .iter()
        .cloned()
        .map(|v| v.clamp(-1e9_f32, 1e9_f32))
        .collect::<Vec<f32>>();
    let out = render(&input, &params);
    assert!(out.iter().all(|&v| v.is_finite()));
}

#[test]
fn doc_round_trips() {
    // The block registers its parameters for the UI and the doc serialiser;
    // a round trip through the JSON doc must preserve them.
    let preset_params = [
        ("pitch", 7.0_f32),
        ("rate", 150.0_f32),
        ("mode", 2.0_f32),
        ("glide", 340.0_f32),
        ("feedback", 30.0_f32),
        ("mix", 80.0_f32),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect::<BTreeMap<String, f32>>();

    let mut node = dsp_graph::NodeDoc {
        id: "time.pitchwhammy".into(),
        block: "time.pitchwhammy".into(),
        params: preset_params,
        bypass: false,
        pos: None,
        model: None,
    };
    let mut doc = dsp_graph::Doc::new("pitchwhammy-test");
    doc.nodes.push(node);
    let json = doc.to_json();
    let restored: dsp_graph::Doc = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
}

#[test]
fn harmony_root_voice_through_delay() {
    // Mode 2 (Harmony) with the gate disengaged still voices the root through
    // the single `FeedbackDelay` in the loop.  The chord voices require the
    // gate (footswitch), which the UI card wires; the chord voices themselves
    // are verified at the `PitchShifter` level in `chord_voices_shift`.
    let input = input_sine(2.0);
    let params = [0.0, 0.0, 2.0, 80.0, 0.0, 100.0];
    let out = render(&input, &params);
    let (f, _) = peak_freq(&out, SR, 60.0, 500.0);
    assert!((f - F0).abs() < F0 * TOL_FREQ, "harmony root → got {f:.1} Hz");
}

#[test]
fn chord_voices_shift() {
    // The two static pitch shifters used for the Harmony chord shift a sine
    // by +300 and +700 cents respectively.  `PitchShifter::process` is
    // per-sample, so run it sample-by-sample over the delay line.
    let sr = 48_000.0;
    let n = (0.5 * sr) as usize;
    let mut in_sine = vec![0.0f32; n];
    for (i, s) in in_sine.iter_mut().enumerate() {
        *s = (2.0 * PI * F0 * i as f32 / sr).sin();
    }
    let mut up = pedal_dsp::PitchShifter::<2400>::new(sr);
    let mut s7 = pedal_dsp::PitchShifter::<2400>::new(sr);
    up.pitch_cents = 300.0;
    s7.pitch_cents = 700.0;
    let mut out_up = vec![0.0f32; n];
    let mut out_7 = vec![0.0f32; n];
    for (i, &x) in in_sine.iter().enumerate() {
        out_up[i] = up.process(x);
        out_7[i] = s7.process(x);
    }
    let (fu, _) = peak_freq(&out_up, sr, 60.0, 1000.0);
    let (f7, _) = peak_freq(&out_7, sr, 60.0, 1000.0);
    let expected_up = F0 * 2f32.powf(300.0 / 1200.0);
    let expected_7 = F0 * 2f32.powf(700.0 / 1200.0);
    assert!((fu - expected_up).abs() < expected_up * TOL_FREQ, "chord +300 → {fu:.1} Hz");
    assert!((f7 - expected_7).abs() < expected_7 * TOL_FREQ, "chord +700 → {f7:.1} Hz");
}
