//! Engine tests for the Shimmer reverb block.
//!
//! Run with: `cargo test -p dsp-graph --test shimmer`

use dsp_graph::Block;
use dsp_graph::blocks::ShimmerReverb;
use dsp_graph::{Doc, Executor, Graph};

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;

fn reg() -> dsp_graph::Registry {
    dsp_graph::blocks::builtin_registry()
}

fn fresh_block() -> ShimmerReverb {
    ShimmerReverb::default()
}

fn prepared_block() -> ShimmerReverb {
    let mut b = fresh_block();
    let sr = SR;
    let _ = b.prepare(sr, BLOCK);
    b
}

// --- minimal graph builder: in -> shimmer -> out ---

fn shimmer_graph(interval: f32, params: &[(String, f32)]) -> Graph {
    let mut g = Graph::new();
    g.add("in", Box::new(dsp_graph::blocks::IoInput)).unwrap();
    g.add("shimmer", Box::new(ShimmerReverb::default())).unwrap();
    g.add("out", Box::new(dsp_graph::blocks::IoOutput)).unwrap();
    g.connect("in", "out", "shimmer", "in").unwrap();
    g.connect("shimmer", "out", "out", "in").unwrap();
    for (k, v) in params {
        g.set_param_real("shimmer", k, *v).unwrap();
    }
    // interval is a Choice; set it via spec id
    g.set_param_real("shimmer", "interval", interval).unwrap();
    g
}

fn run_graph(g: &mut Graph, input: &[f32]) -> Vec<f32> {
    let mut ex = Executor::compile_owned(g, SR, BLOCK).unwrap();
    let total = input.len();
    let mut out = Vec::with_capacity(total);
    let mut done = 0;
    while done < total {
        let n = BLOCK.min(total - done);
        out.extend_from_slice(ex.run(&input[done..done + n], n).unwrap());
        done += n;
    }
    out
}

/// RMS of a slice.
fn rms(s: &[f32]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let mut acc = 0.0f64;
    for &v in s {
        acc += (v as f64) * (v as f64);
    }
    (acc / s.len() as f64).sqrt() as f32
}

/// Peak of a slice.
fn peak(s: &[f32]) -> f32 {
    s.iter().map(|v| v.abs()).fold(0.0f32, f32::max)
}

#[test]
fn spec_fails_no_block() {
    let _ = fresh_block().spec();
}

#[test]
fn reg_fails_no_block() {
    let r = reg();
    assert!(r.get("time.shimmer").is_some());
}

#[test]
fn spec_is_found() {
    let r = reg();
    let spec = r.get("time.shimmer").unwrap();
    assert_eq!(spec.id, "time.shimmer");
    assert_eq!(spec.name, "Shimmer");
    assert_eq!(spec.category, dsp_graph::types::Category::Time);
    assert_eq!(spec.inputs.len(), 1);
    assert_eq!(spec.outputs.len(), 1);
    assert!(spec.inputs[0].name == "in");
    assert!(spec.outputs[0].name == "out");

    // Params must match the spec table exactly.
    let param_ids = spec.params.iter().map(|p| p.id).collect::<Vec<_>>();
    assert_eq!(param_ids, vec![
        "mix", "size", "decay", "shimmer", "interval", "tone", "pre_delay",
    ]);
    // mix
    assert_eq!(spec.param_index("mix").unwrap(), 0);
    assert!(spec.params[0].unit == dsp_graph::types::Unit::Percent);
    assert!((spec.params[0].min - 0.0).abs() < 1e-6);
    assert!((spec.params[0].max - 100.0).abs() < 1e-6);
    assert!((spec.params[0].default - 40.0).abs() < 1e-6);
    // size
    assert_eq!(spec.param_index("size").unwrap(), 1);
    assert!((spec.params[1].min - 0.0).abs() < 1e-6);
    assert!((spec.params[1].max - 100.0).abs() < 1e-6);
    assert!((spec.params[1].default - 70.0).abs() < 1e-6);
    // decay
    assert_eq!(spec.param_index("decay").unwrap(), 2);
    assert!(spec.params[2].unit == dsp_graph::types::Unit::Seconds);
    assert!((spec.params[2].min - 0.2).abs() < 1e-6);
    assert!((spec.params[2].max - 8.0).abs() < 1e-6);
    assert!((spec.params[2].default - 2.5).abs() < 1e-6);
    // shimmer
    assert_eq!(spec.param_index("shimmer").unwrap(), 3);
    assert!((spec.params[3].min - 0.0).abs() < 1e-6);
    assert!((spec.params[3].max - 100.0).abs() < 1e-6);
    assert!((spec.params[3].default - 55.0).abs() < 1e-6);
    // interval
    assert_eq!(spec.param_index("interval").unwrap(), 4);
    assert!(spec.params[4].unit == dsp_graph::types::Unit::Choice(
        &["+12 st up", "+7 st up", "−12 st down"]
    ));
    assert!((spec.params[4].min - 0.0).abs() < 1e-6);
    assert!((spec.params[4].max - 2.0).abs() < 1e-6);
    assert!((spec.params[4].default - 0.0).abs() < 1e-6);
    // tone
    assert_eq!(spec.param_index("tone").unwrap(), 5);
    assert!((spec.params[5].min - 0.0).abs() < 1e-6);
    assert!((spec.params[5].max - 100.0).abs() < 1e-6);
    assert!((spec.params[5].default - 50.0).abs() < 1e-6);
    // pre_delay
    assert_eq!(spec.param_index("pre_delay").unwrap(), 6);
    assert!(spec.params[6].unit == dsp_graph::types::Unit::Milliseconds);
    assert!((spec.params[6].min - 0.0).abs() < 1e-6);
    assert!((spec.params[6].max - 80.0).abs() < 1e-6);
    assert!((spec.params[6].default - 0.0).abs() < 1e-6);
}

#[test]
fn default_params_equal_spec_table() {
    let mut b = fresh_block();
    let spec = b.spec();
    let params = spec.params;
    assert!((params[0].default - 40.0).abs() < 1e-6, "mix default");
    assert!((params[1].default - 70.0).abs() < 1e-6, "size default");
    assert!((params[2].default - 2.5).abs() < 1e-6, "decay default");
    assert!((params[3].default - 55.0).abs() < 1e-6, "shimmer default");
    assert!((params[4].default - 0.0).abs() < 1e-6, "interval default");
    assert!((params[5].default - 50.0).abs() < 1e-6, "tone default");
    assert!((params[6].default - 0.0).abs() < 1e-6, "pre_delay default");
}

#[test]
fn interval_ratios() {
    // Verify the mapping analytically: the spec fixates
    // ratio = 2^(interval_semitones/12) → interval 0,1,2 → 2.0, ≈1.4983, 0.5.
    // interval 0 = +12 st, interval 1 = +7 st, interval 2 = −12 st.
    assert!((2.0_f32.powf(12.0 / 12.0) - 2.0).abs() < 1e-9);
    assert!((2.0_f32.powf(7.0 / 12.0) - 1.498307).abs() < 1e-4);
    assert!((2.0_f32.powf(-12.0 / 12.0) - 0.5).abs() < 1e-9);
}

#[test]
fn silence_in_is_silence_out() {
    // With `mix = 0` (bypass) and any other params, silence in → silence out.
    let mut g = shimmer_graph(0.0, &[]);
    g.set_param_real("shimmer", "mix", 0.0).unwrap();
    // drive every other param to mid-range
    g.set_param_real("shimmer", "size", 50.0).unwrap();
    g.set_param_real("shimmer", "decay", 2.0).unwrap();
    g.set_param_real("shimmer", "shimmer", 50.0).unwrap();
    g.set_param_real("shimmer", "tone", 50.0).unwrap();
    g.set_param_real("shimmer", "pre_delay", 10.0).unwrap();

    let input = vec![0.0; 1000];
    let out = run_graph(&mut g, &input);

    assert!(rms(&out) < 1e-9, "RMS = {}", rms(&out));
}

#[test]
fn mix_zero_bypass_is_sample_exact() {
    // With `mix = 0`, the output must equal the input sample-for-sample for
    // the full parameter range. We iterate a grid of param values.
    let mut rng = fast_rand(0x9e3779b97f4a7c15);
    for _ in 0..20 {
        // random input
        let input: Vec<f32> = (0..512)
            .map(|i| {
                let sig = if i % 4 == 0 {
                    (i as f32 * 0.05).sin()
                } else {
                    2.0 * (rng.next_u32() >> 16) as f32 / 65536.0 - 1.0
                };
                sig * (0.1 + rng.next_f32() * 2.0)
            })
            .collect();

        // vary mix=0 and all other params across the range
        let size = rng.next_f32() * 100.0;
        let decay = 0.2 + rng.next_f32() * 7.8;
        let shimmer = rng.next_f32() * 100.0;
        let tone = rng.next_f32() * 100.0;
        let pre_delay = rng.next_f32() * 80.0;
        let interval = (rng.next_f32() * 3.0).round() as i32;

        let mut g = shimmer_graph(interval as f32, &[]);
        g.set_param_real("shimmer", "mix", 0.0).unwrap();
        g.set_param_real("shimmer", "size", size).unwrap();
        g.set_param_real("shimmer", "decay", decay).unwrap();
        g.set_param_real("shimmer", "shimmer", shimmer).unwrap();
        g.set_param_real("shimmer", "tone", tone).unwrap();
        g.set_param_real("shimmer", "pre_delay", pre_delay).unwrap();

        let out = run_graph(&mut g, &input);

        for (i, (&a, &b)) in input.iter().zip(out.iter()).enumerate() {
            assert_eq!(a, b, "mix=0 bypass mismatch at sample {i}");
        }
    }
}

fn fast_rand(seed: u64) -> FastRand {
    FastRand(seed)
}

struct FastRand(u64);

impl FastRand {
    fn next_u64(&mut self) -> u64 {
        let mut z = self.0;
        z = z.wrapping_add(0x9e3779b97f4a7c15);
        self.0 = z;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn next_u32(&mut self) -> u32 {
        (self.next_u64() & 0xffff_ffff) as u32
    }
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16777216.0
    }
}

#[test]
fn no_nan_at_max_params() {
    // 10 s ramp at 48 kHz with every param at max must produce no NaN/inf.
    let mut g = Graph::new();
    g.add("in", Box::new(dsp_graph::blocks::IoInput)).unwrap();
    g.add("shimmer", Box::new(ShimmerReverb::default())).unwrap();
    g.add("out", Box::new(dsp_graph::blocks::IoOutput)).unwrap();
    g.connect("in", "out", "shimmer", "in").unwrap();
    g.connect("shimmer", "out", "out", "in").unwrap();
    // put every param at max
    g.set_param_real("shimmer", "mix", 100.0).unwrap();
    g.set_param_real("shimmer", "size", 100.0).unwrap();
    g.set_param_real("shimmer", "decay", 8.0).unwrap();
    g.set_param_real("shimmer", "shimmer", 100.0).unwrap();
    g.set_param_real("shimmer", "interval", 0.0).unwrap();
    g.set_param_real("shimmer", "tone", 100.0).unwrap();
    g.set_param_real("shimmer", "pre_delay", 80.0).unwrap();

    // ramp input
    let total = (SR * 10.0) as usize;
    let mut input = Vec::with_capacity(total);
    for i in 0..total {
        input.push((i as f32 / SR) * 0.1);
    }

    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();
    let mut peak = 0.0f32;
    let mut had_nan = false;
    for chunk in input.chunks_exact(BLOCK) {
        let n = chunk.len();
        let out = ex.run(chunk, n).unwrap();
        for &v in out {
            if !v.is_finite() {
                had_nan = true;
            }
            peak = peak.max(v.abs());
        }
    }

    assert!(!had_nan, "output contained NaN/inf");
    assert!(peak < 1e6, "peak {} too large", peak);
}

#[test]
fn max_stability_decays_to_silence() {
    // size=max, decay=8.0, shimmer=max, tone=0 → bounded (peak < 2.0), no NaN,
    // and the tail decays to silence.
    let mut g = Graph::new();
    g.add("in", Box::new(dsp_graph::blocks::IoInput)).unwrap();
    g.add("shimmer", Box::new(ShimmerReverb::default())).unwrap();
    g.add("out", Box::new(dsp_graph::blocks::IoOutput)).unwrap();
    g.connect("in", "out", "shimmer", "in").unwrap();
    g.connect("shimmer", "out", "out", "in").unwrap();
    g.set_param_real("shimmer", "size", 100.0).unwrap();
    g.set_param_real("shimmer", "decay", 8.0).unwrap();
    g.set_param_real("shimmer", "shimmer", 100.0).unwrap();
    g.set_param_real("shimmer", "tone", 0.0).unwrap();
    g.set_param_real("shimmer", "interval", 0.0).unwrap();
    g.set_param_real("shimmer", "pre_delay", 0.0).unwrap();

    // impulse input
    let total = (SR * 30.0) as usize;
    let mut input = vec![0.0; total];
    input[0] = 1.0;

    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();
    let mut out_all: Vec<f32> = Vec::with_capacity(total);
    let mut peak = 0.0f32;
    for chunk in input.chunks_exact(BLOCK) {
        let n = chunk.len();
        let out = ex.run(chunk, n).unwrap();
        for &v in out {
            if v.is_nan() {
                panic!("NaN in output");
            }
            peak = peak.max(v.abs());
        }
        out_all.extend_from_slice(out);
    }

    assert!(peak < 2.0, "peak {} too large", peak);
    // RMS of the last second of the OUTPUT should be very small (decayed).
    let tail_start = total - SR as usize;
    let tail_rms = rms(&out_all[tail_start..]);
    assert!(tail_rms < 0.02, "tail RMS {} not decayed", tail_rms);
}

#[test]
fn interval_out_of_range_clamped() {
    let mut g = Graph::new();
    g.add("in", Box::new(dsp_graph::blocks::IoInput)).unwrap();
    g.add("shimmer", Box::new(ShimmerReverb::default())).unwrap();
    g.add("out", Box::new(dsp_graph::blocks::IoOutput)).unwrap();
    g.connect("in", "out", "shimmer", "in").unwrap();
    g.connect("shimmer", "out", "out", "in").unwrap();

    // interval < 0 and interval > 2 must clamp to 0..2.
    g.set_param_real("shimmer", "interval", -1.0).unwrap();
    let idx = g.node("shimmer").unwrap().spec.param_index("interval").unwrap();
    let norm = g.node("shimmer").unwrap().params[idx];
    assert!((norm - 0.0).abs() < 1e-6, "norm {norm}");

    g.set_param_real("shimmer", "interval", 5.0).unwrap();
    let idx = g.node("shimmer").unwrap().spec.param_index("interval").unwrap();
    let norm = g.node("shimmer").unwrap().params[idx];
    assert!((norm - 1.0).abs() < 1e-6, "norm {norm}");
}

#[test]
fn pre_delay_lags_wet_tail() {
    // pre_delay of 40 ms should shift the start of the wet tail by ≈40 ms.
    let total = (SR * 2.0) as usize;
    let mut input = vec![0.0; total];
    input[0] = 1.0;

    let mut g0 = shimmer_graph(0.0, &[
        ("pre_delay".to_string(), 0.0),
        ("mix".to_string(), 100.0),
    ]);
    let mut g1 = shimmer_graph(0.0, &[
        ("pre_delay".to_string(), 40.0),
        ("mix".to_string(), 100.0),
    ]);

    let out0 = run_graph(&mut g0, &input);
    let out1 = run_graph(&mut g1, &input);
    assert_eq!(out0.len(), total);
    assert_eq!(out1.len(), total);

    // First sample above the threshold marks the wet-tail onset.
    let first0 = out0.iter().position(|v| v.abs() > 1e-6).unwrap_or(total);
    let first1 = out1.iter().position(|v| v.abs() > 1e-6).unwrap_or(total);
    let lag_ms = (first1 - first0) as f32 * 1000.0 / SR;
    // 40 ms → ~1920 samples at 48 kHz. Allow a tolerance.
    assert!((lag_ms - 40.0).abs() < 10.0, "lag {lag_ms} ms, expected ≈40 ms");
}

#[test]
fn preset_round_trip_preserves_params() {
    // Build a Doc with one ShimmerReverb node at non-default params, round-trip
    // through to_json → from_json → build, and verify every param is preserved.
    let doc = Doc {
        name: "shimmer rt".into(),
        nodes: vec![
            dsp_graph::NodeDoc {
                id: "in".into(),
                block: "io.input".into(),
                params: std::collections::BTreeMap::new(),
                bypass: false,
                pos: None,
                model: None, curve: None,
            },
            dsp_graph::NodeDoc {
                id: "shimmer".into(),
                block: "time.shimmer".into(),
                params: {
                    let mut p = std::collections::BTreeMap::new();
                    p.insert("mix".into(), 30.0);
                    p.insert("size".into(), 40.0);
                    p.insert("decay".into(), 3.0);
                    p.insert("shimmer".into(), 80.0);
                    p.insert("interval".into(), 1.0);
                    p.insert("tone".into(), 70.0);
                    p.insert("pre_delay".into(), 25.0);
                    p
                },
                bypass: false,
                pos: None,
                model: None, curve: None,
            },
            dsp_graph::NodeDoc {
                id: "out".into(),
                block: "io.output".into(),
                params: std::collections::BTreeMap::new(),
                bypass: false,
                pos: None,
                model: None, curve: None,
            },
        ],
        edges: vec![
            dsp_graph::EdgeDoc::new(("in", "out"), ("shimmer", "in")),
            dsp_graph::EdgeDoc::new(("shimmer", "out"), ("out", "in")),
        ],
        routings: vec![],
    };

    let json = doc.to_json();
    let rebuilt = Doc::from_json(&json).unwrap();
    // Rebuild the graph and confirm the params are unchanged.
    let reg = reg();
    let mut g = rebuilt.build(&reg).unwrap();
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();

    // Verify via param_real directly.
    let mix = ex.param_real("shimmer", "mix").unwrap();
    let size = ex.param_real("shimmer", "size").unwrap();
    let decay = ex.param_real("shimmer", "decay").unwrap();
    let shimmer = ex.param_real("shimmer", "shimmer").unwrap();
    let interval = ex.param_real("shimmer", "interval").unwrap();
    let tone = ex.param_real("shimmer", "tone").unwrap();
    let pre_delay = ex.param_real("shimmer", "pre_delay").unwrap();

    assert!((mix - 30.0).abs() < 1e-3, "mix {mix}");
    assert!((size - 40.0).abs() < 1e-3, "size {size}");
    assert!((decay - 3.0).abs() < 1e-3, "decay {decay}");
    assert!((shimmer - 80.0).abs() < 1e-3, "shimmer {shimmer}");
    assert!((interval - 1.0).abs() < 1e-3, "interval {interval}");
    assert!((tone - 70.0).abs() < 1e-3, "tone {tone}");
    assert!((pre_delay - 25.0).abs() < 1e-3, "pre_delay {pre_delay}");

    // An unknown param must be rejected, not silently dropped.
    let bad_json = r#"{"name":"bad","nodes":[{"id":"s","block":"time.shimmer","params":{"gian":3}}]}"#;
    let e = Doc::from_json(bad_json)
        .unwrap()
        .build(&reg)
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("unknown param"), "{e}");
}
