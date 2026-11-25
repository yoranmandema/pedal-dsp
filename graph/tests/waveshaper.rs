//! Tests for the `dist.waveshaper` block (design: docs/design/waveshaper.md).
//!
//!     cargo test -p dsp-graph --test waveshaper

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use dsp_graph::blocks::{IoInput, IoOutput};
use dsp_graph::waveshaper::{lookup, Interpolation, TransferCurve, WaveShaper, TABLE_SIZE};
use dsp_graph::{Block, Category, Executor, Graph};
use serde_json::json;

thread_local! { static ALLOCS: Cell<usize> = const { Cell::new(0) }; }
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static A: Counting = Counting;
fn allocs() -> usize {
    ALLOCS.with(|c| c.get())
}

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;

fn rig(curve: Option<&str>, params: &[(&str, f32)]) -> Executor {
    let mut ws = WaveShaper::default();
    if let Some(c) = curve {
        ws.configure("curve", c).unwrap();
    }
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("ws", Box::new(ws)).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "ws", "in").unwrap();
    g.connect("ws", "out", "out", "in").unwrap();
    for (k, v) in params {
        g.set_param_real("ws", k, *v).unwrap();
    }
    Executor::compile_owned(&mut g, SR, BLOCK).unwrap()
}

fn run(ex: &mut Executor, input: &[f32]) -> Vec<f32> {
    let mut out = Vec::new();
    for chunk in input.chunks(BLOCK) {
        out.extend_from_slice(ex.run(chunk, chunk.len()).unwrap());
    }
    out
}

fn ramp(n: usize, lo: f32, hi: f32) -> Vec<f32> {
    (0..n).map(|i| lo + (hi - lo) * i as f32 / (n - 1) as f32).collect()
}

const HARD_CLIP: &str = r#"{"points":[[-1,-1],[-0.1,-1],[0.1,1],[1,1]]}"#;

#[test]
fn spec_is_registered_and_complete() {
    let spec = WaveShaper::default().spec();
    assert_eq!(spec.id, "dist.waveshaper");
    assert_eq!(spec.name, "Waveshaper");
    assert_eq!(spec.category, Category::Distortion);
    assert_eq!(spec.inputs.len(), 1);
    assert_eq!(spec.inputs[0].name, "in");
    assert_eq!(spec.outputs.len(), 1);
    assert_eq!(spec.outputs[0].name, "out");
    let ids: Vec<_> = spec.params.iter().map(|p| p.id).collect();
    assert_eq!(ids, ["input_gain", "output_gain", "mix", "clamp", "interpolation"]);
    let reg = dsp_graph::blocks::builtin_registry();
    assert!(reg.get("dist.waveshaper").is_some());
    assert!(reg.build("dist.waveshaper").is_some());
}

#[test]
fn identity_curve_passes_signal_unchanged_at_any_mix() {
    let input = ramp(512, -1.0, 1.0);
    for mix in [0.0, 50.0, 100.0] {
        let mut ex = rig(None, &[("mix", mix)]);
        let out = run(&mut ex, &input);
        for (i, (a, b)) in input.iter().zip(&out).enumerate() {
            assert!((a - b).abs() < 1e-3, "mix {mix} sample {i}: {a} vs {b}");
        }
    }
}

#[test]
fn hard_clip_curve_clips() {
    let mut ex = rig(Some(HARD_CLIP), &[]);
    let out = run(&mut ex, &[0.0, 0.5, -0.5, 1.0, -1.0, 0.05]);
    assert!((out[1] - 1.0).abs() < 1e-3, "{}", out[1]);
    assert!((out[2] + 1.0).abs() < 1e-3);
    assert!(out[0].abs() < 1e-3);
    assert!((out[5] - 0.5).abs() < 0.02, "inside the ramp: {}", out[5]);
    assert!(out.iter().all(|y| y.abs() <= 1.0 + 1e-6));
}

#[test]
fn gains_scale_input_and_output() {
    // Identity curve: input_gain 2 with clamp -> clamps at 1; output_gain 0.5 halves.
    let mut ex = rig(None, &[("input_gain", 2.0)]);
    let out = run(&mut ex, &[0.25, 0.9]);
    assert!((out[0] - 0.5).abs() < 1e-3 && (out[1] - 1.0).abs() < 1e-3, "{out:?}");
    let mut ex = rig(None, &[("output_gain", 0.5)]);
    let out = run(&mut ex, &[0.5]);
    assert!((out[0] - 0.25).abs() < 1e-3);
    // Clamp off lets output gain exceed 1.
    let mut ex = rig(None, &[("output_gain", 4.0), ("clamp", 0.0)]);
    let out = run(&mut ex, &[0.5]);
    assert!((out[0] - 2.0).abs() < 1e-2, "{out:?}");
}

#[test]
fn out_of_range_and_non_finite_input_is_safe() {
    let mut ex = rig(Some(HARD_CLIP), &[]);
    let out = run(&mut ex, &[5.0, -5.0, 1e30, f32::INFINITY, f32::NAN, f32::NEG_INFINITY]);
    assert!((out[0] - 1.0).abs() < 1e-3 && (out[1] + 1.0).abs() < 1e-3);
    assert!(out.iter().all(|y| y.is_finite() && y.abs() <= 1.0 + 1e-6), "{out:?}");
    // Direct lookup never panics at the edges.
    let t = [0.0f32; TABLE_SIZE];
    for u in [-1e9, -1.0, 0.0, 1.0, 1e9, f32::NAN, f32::INFINITY] {
        let _ = lookup(&t, u);
    }
}

#[test]
fn strict_validation_rejects_bad_curves() {
    assert!(TransferCurve::new(vec![[0.0, 0.0]]).is_err(), "too few points");
    assert!(TransferCurve::new(vec![[0.5, 0.0], [-0.5, 0.0]]).is_err(), "unsorted");
    assert!(TransferCurve::new(vec![[0.0, 0.0], [0.0, 1.0]]).is_err(), "duplicate x");
    assert!(TransferCurve::new(vec![[-1.0, -2.0], [1.0, 1.0]]).is_err(), "y out of range");
    assert!(TransferCurve::new(vec![[-1.0, f32::NAN], [1.0, 1.0]]).is_err(), "NaN");
    assert!(TransferCurve::new(vec![[-1.0, -1.0], [1.0, 1.0]]).is_ok());

    let mut ws = WaveShaper::default();
    for bad in [
        "not json",
        r#"{"points":[]}"#,
        r#"{"points":[[0,0]]}"#,
        r#"{"points":[[0,0],[-1,1]]}"#,
        r#"{"points":[[0,0],[2,1]]}"#,
        r#"{"points":[[0,0,1],[1,1]]}"#,
        r#"{"nope":1}"#,
    ] {
        assert!(ws.configure("curve", bad).is_err(), "accepted {bad}");
    }
    // A rejected curve leaves the previous one intact.
    assert_eq!(ws.curve(), &TransferCurve::identity());
}

#[test]
fn sanitized_repairs_unsorted_and_out_of_range_points() {
    let c = TransferCurve::sanitized(&[[0.5, 3.0], [-2.0, -1.0], [f32::NAN, 0.0], [0.5, 0.2], [1.0, 1.0]]);
    assert_eq!(c.points(), &[[-1.0, -1.0], [0.5, 0.2], [1.0, 1.0]]);
    assert_eq!(TransferCurve::sanitized(&[[0.1, 0.1]]), TransferCurve::identity());
    assert_eq!(TransferCurve::sanitized(&[]), TransferCurve::identity());
}

#[test]
fn json_round_trip() {
    let c = TransferCurve::new(vec![[-1.0, -1.0], [-0.5, 0.25], [1.0, 1.0]]).unwrap();
    let back = TransferCurve::from_json(&c.to_json()).unwrap();
    assert_eq!(c, back);
    assert_eq!(c.to_json(), json!({"points": [[-1.0, -1.0], [-0.5, 0.25], [1.0, 1.0]]}));
}

#[test]
fn monotone_cubic_does_not_overshoot() {
    // A staircase-ish monotone curve: plain cubics would overshoot the plateaus.
    let c = TransferCurve::new(vec![[-1.0, -1.0], [-0.2, -0.1], [0.2, 0.1], [0.3, 0.9], [1.0, 1.0]]).unwrap();
    let mut prev = f32::MIN;
    for i in 0..=2000 {
        let x = -1.0 + 2.0 * i as f32 / 2000.0;
        let y = c.eval(x, Interpolation::MonotoneCubic);
        assert!(y >= prev - 1e-5, "non-monotone at x={x}: {y} < {prev}");
        assert!((-1.0 - 1e-5..=1.0 + 1e-5).contains(&y), "overshoot at x={x}: {y}");
        prev = y;
    }
    // Passes through control points.
    for p in c.points() {
        assert!((c.eval(p[0], Interpolation::MonotoneCubic) - p[1]).abs() < 1e-4);
    }
}

#[test]
fn interpolation_param_switches_the_table() {
    let curve = r#"{"points":[[-1,-1],[0,0.5],[1,1]]}"#;
    let mut lin = rig(Some(curve), &[("interpolation", 0.0)]);
    let mut cub = rig(Some(curve), &[("interpolation", 1.0)]);
    let a = run(&mut lin, &[-0.5])[0];
    let b = run(&mut cub, &[-0.5])[0];
    assert!((a - -0.25).abs() < 1e-3, "{a}");
    assert!((a - b).abs() > 1e-3, "cubic should differ from linear: {a} vs {b}");
}

#[test]
fn process_path_does_not_allocate() {
    let mut ex = rig(Some(HARD_CLIP), &[("interpolation", 1.0)]);
    let input: Vec<f32> = (0..BLOCK).map(|i| (i as f32 * 0.07).sin() * 3.0).collect();
    ex.run(&input, BLOCK).unwrap();
    let before = allocs();
    for _ in 0..1000 {
        std::hint::black_box(ex.run(&input, BLOCK).unwrap());
    }
    assert_eq!(allocs() - before, 0, "run() allocated");
}

#[test]
fn doc_with_curve_builds_and_rejects_bad_curve() {
    use dsp_graph::Doc;
    let mk = |curve: serde_json::Value| -> Doc {
        serde_json::from_value(json!({
            "name": "t",
            "nodes": [{"id":"ws","block":"dist.waveshaper","curve":curve}],
        })).unwrap()
    };
    let reg = dsp_graph::blocks::builtin_registry();
    let ok = mk(json!({"points":[[-1,-1],[0.5,0.2],[1,1]]}));
    assert!(ok.build(&reg).is_ok());
    // Round-trips through serde.
    let s = serde_json::to_string(&ok).unwrap();
    assert_eq!(serde_json::from_str::<Doc>(&s).unwrap(), ok);
    let bad = mk(json!({"points":[[1,1],[-1,-1]]}));
    assert!(bad.build(&reg).is_err());
    // No curve key → identity, no error.
    let none: Doc = serde_json::from_value(json!({"name":"t","nodes":[{"id":"ws","block":"dist.waveshaper"}]})).unwrap();
    assert!(none.build(&reg).is_ok());
}
