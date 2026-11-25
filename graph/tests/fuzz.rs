//! Tests for the `fuzz.fuzz` block.
use dsp_graph::blocks::{builtin_registry, IoInput, IoOutput};
use dsp_graph::{Executor, Graph};

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;

fn rig(params: &[(&str, f32)]) -> Executor {
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("fz", builtin_registry().build("fuzz.fuzz").expect("fuzz.fuzz registered")).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "fz", "in").unwrap();
    g.connect("fz", "out", "out", "in").unwrap();
    for (k, v) in params {
        g.set_param_real("fz", k, *v).unwrap();
    }
    Executor::compile_owned(&mut g, SR, BLOCK).unwrap()
}

fn run(ex: &mut Executor, input: &[f32]) -> Vec<f32> {
    let mut out = Vec::new();
    for c in input.chunks(BLOCK) {
        out.extend_from_slice(ex.run(c, c.len()).unwrap());
    }
    out
}

fn sine(n: usize, amp: f32) -> Vec<f32> {
    (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / SR).sin()).collect()
}

#[test]
fn registered_and_silent() {
    for m in 0..3 {
        let mut ex = rig(&[("model", m as f32)]);
        let out = run(&mut ex, &vec![0.0; 4096]);
        assert!(out.iter().all(|v| v.abs() < 1e-6));
    }
}

#[test]
fn bounded_finite_and_low_dc_at_extremes() {
    for m in 0..3 {
        let mut ex = rig(&[("model", m as f32), ("drive", 10.0), ("bass", 12.0), ("mid", 12.0), ("treble", 12.0), ("level", 12.0)]);
        let out = run(&mut ex, &sine(48_000, 1.0));
        assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 5.0), "model {m}");
        let tail = &out[24_000..];
        let dc = tail.iter().sum::<f32>() / tail.len() as f32;
        assert!(dc.abs() < 0.1, "model {m} dc {dc}");
    }
}

#[test]
fn models_differ_and_params_roundtrip() {
    let input = sine(4096, 0.5);
    let a = run(&mut rig(&[("model", 0.0)]), &input);
    let b = run(&mut rig(&[("model", 1.0)]), &input);
    assert_ne!(a, b);
    let spec = builtin_registry().build("fuzz.fuzz").unwrap().spec();
    let ids: Vec<_> = spec.params.iter().map(|p| p.id).collect();
    assert_eq!(ids, ["model", "drive", "bass", "mid", "treble", "level"]);
}
