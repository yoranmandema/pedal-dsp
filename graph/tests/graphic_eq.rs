//! Tests for the `tone.graphic_eq` graphic EQ block.
//!
//! Run with `cargo test -p dsp-graph graphic_eq`.

use dsp_graph::{blocks::builtin_registry, Doc, Executor, Graph, Registry};
use dsp_graph::blocks::{IoInput, IoOutput};
use dsp_graph::doc::NodeDoc;

const SR: f32 = 48_000.0;
const BLOCK: usize = 4096;

/// A stable parameter id: `gain0` … `gain9`. Written by hand so no temporary
/// `String` is ever created (ParamSpec ids are `&'static str`).
fn gain_id(i: usize) -> &'static str {
    match i {
        0 => "gain0",
        1 => "gain1",
        2 => "gain2",
        3 => "gain3",
        4 => "gain4",
        5 => "gain5",
        6 => "gain6",
        7 => "gain7",
        8 => "gain8",
        9 => "gain9",
        _ => unreachable!("gain index out of range"),
    }
}

fn reg() -> Registry {
    builtin_registry()
}

/// Build a minimal graph: `io.input` -> `block` -> `io.output`.
fn graph_for(block: Box<dyn dsp_graph::Block>) -> Graph {
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("eq", block).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "eq", "in").unwrap();
    g.connect("eq", "out", "out", "in").unwrap();
    g
}

/// Build a minimal graph whose `eq` node comes from the registry.
fn graph_from_registry(block_id: &str) -> Graph {
    let block = reg()
        .build(block_id)
        .expect("block registered");
    graph_for(block)
}

/// Render a sine through the block and return its steady-state dB gain.
///
/// Only the bands named in `gains` are set; the rest stay at their defaults
/// (0 dB). The RMS is measured over the second half of the buffer so the
/// filter state has settled.
fn gain_db(block_id: &str, freq: f32, gains: &[(usize, f32)]) -> f32 {
    let mut g = graph_from_registry(block_id);
    for (i, v) in gains {
        g.set_param_real("eq", gain_id(*i), *v).unwrap();
    }
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();
    let mut input = Vec::with_capacity(BLOCK);
    let mut output = Vec::with_capacity(BLOCK);
    for i in 0..BLOCK {
        input.push((2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin());
    }
    output.extend_from_slice(ex.run(&input, BLOCK).unwrap());
    let start = BLOCK / 4;
    let rms_in = rms(&input[start..]);
    let rms_out = rms(&output[start..]);
    20.0 * (rms_out / rms_in).log10()
}

fn rms(s: &[f32]) -> f32 {
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for &v in &s[s.len() / 2..] {
        sum += (v * v) as f64;
        n += 1;
    }
    ((sum / n as f64).sqrt()) as f32
}

#[test]
fn registry_block_has_correct_identity_and_ports() {
    let r = reg();
    let spec = r
        .get("tone.graphic_eq")
        .expect("graphic eq registered in the registry");

    assert_eq!(spec.id, "tone.graphic_eq");
    assert_eq!(spec.name, "Graphic EQ");
    assert_eq!(spec.category, dsp_graph::types::Category::Tone);

    assert_eq!(spec.inputs.len(), 1);
    assert_eq!(spec.outputs.len(), 1);
    assert!(matches!(spec.inputs[0].kind, dsp_graph::types::PortKind::Audio));
    assert!(matches!(spec.outputs[0].kind, dsp_graph::types::PortKind::Audio));

    // One param per band, gain0..gain9, dB, -12..+12, default 0, AmpGain.
    let expected_ids = [
        "gain0", "gain1", "gain2", "gain3", "gain4", "gain5", "gain6", "gain7", "gain8", "gain9",
    ];
    assert_eq!(spec.params.len(), expected_ids.len());
    for (i, p) in spec.params.iter().enumerate() {
        assert_eq!(p.id, expected_ids[i]);
        assert_eq!(p.unit, dsp_graph::types::Unit::Decibels);
        assert!((p.min - -12.0).abs() < 1e-6, "{}: min {}", p.id, p.min);
        assert!((p.max - 12.0).abs() < 1e-6, "{}: max {}", p.id, p.max);
        assert!((p.default - 0.0).abs() < 1e-6, "{}: default {}", p.id, p.default);
        assert_eq!(p.curve, dsp_graph::types::Curve::AmpGain);
    }
}

#[test]
fn zero_db_is_sample_exact_identity() {
    let mut g = graph_from_registry("tone.graphic_eq");
    for i in 0..10 {
        g.set_param_real("eq", gain_id(i), 0.0).unwrap();
    }
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();

    let mut input = Vec::with_capacity(BLOCK);
    let mut output = Vec::with_capacity(BLOCK);
    for i in 0..BLOCK {
        input.push((2.0 * std::f32::consts::PI * 800.0 * i as f32 / SR).sin());
    }
    output.extend_from_slice(ex.run(&input, BLOCK).unwrap());

    // Max absolute error over the whole buffer must be below 1e-6.
    let err = input
        .iter()
        .zip(output.iter())
        .map(|(&a, &b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(
        err < 1e-6,
        "0 dB must be sample-exact identity, max abs error {err}"
    );
}

#[test]
fn band_shape_and_no_interaction() {
    // Band 3 is centred at 250 Hz.
    let centre = 250.0f32;

    // Boost / cut band 3 alone.
    let boost3 = [(3, 12.0)];
    let cut3 = [(3, -12.0)];
    // Boost the two far bands (31.5 Hz and 16 kHz).
    let far_up = [(0, 12.0), (9, 12.0)];
    // Far bands up + band 3 up.
    let far_boosted = [(0, 12.0), (9, 12.0), (3, 12.0)];
    // Far bands up + band 3 down.
    let far_cut_down = [(0, 12.0), (9, 12.0), (3, -12.0)];

    // ±12 dB at a band's own centre gives ≈∓12 dB of response.
    assert!((gain_db("tone.graphic_eq", centre, &boost3) - 12.0).abs() < 0.75,
        "band3 +12 = {}", gain_db("tone.graphic_eq", centre, &boost3));
    assert!((gain_db("tone.graphic_eq", centre, &cut3) - -12.0).abs() < 0.75,
        "band3 -12 = {}", gain_db("tone.graphic_eq", centre, &cut3));

    // Bands do not interact: far bands (31.5 Hz, 16 kHz) leave the 250 Hz
    // response essentially unchanged (the small skirt leakage is < 3 dB).
    assert!((gain_db("tone.graphic_eq", centre, &far_up) - 0.0).abs() < 3.0,
        "far bands moved the 250 Hz reading");
    // Band 3 at +12 with far bands up stays ≈ +12 dB (no interaction).
    assert!((gain_db("tone.graphic_eq", centre, &far_boosted) - 12.0).abs() < 1.0,
        "band3 +12 + far = {}", gain_db("tone.graphic_eq", centre, &far_boosted));
    // Band 3 at -12 with far bands up stays ≈ -12 dB (no interaction).
    assert!((gain_db("tone.graphic_eq", centre, &far_cut_down) - -12.0).abs() < 1.0,
        "band3 -12 + far = {}", gain_db("tone.graphic_eq", centre, &far_cut_down));
}

#[test]
fn param_schema_and_denormalise_mapping() {
    let r = reg();
    let spec = r
        .get("tone.graphic_eq")
        .expect("graphic eq registered in the registry");

    assert_eq!(spec.params.len(), 10, "exactly 10 params");

    let expected = [
        "gain0", "gain1", "gain2", "gain3", "gain4", "gain5", "gain6", "gain7", "gain8", "gain9",
    ];
    for (i, p) in spec.params.iter().enumerate() {
        assert_eq!(p.id, expected[i]);
        assert_eq!(p.min, -12.0);
        assert_eq!(p.max, 12.0);
        assert_eq!(p.default, 0.0);
        assert_eq!(p.curve, dsp_graph::types::Curve::AmpGain);
    }

    // Denormalise / normalise round-trip for the -12..+12 dB range.
    // t=0 -> -12 dB, t=0.5 -> 0 dB, t=1 -> +12 dB.
    let p = &spec.params[0];
    assert!((p.denormalise(0.0) - -12.0).abs() < 1e-6);
    assert!((p.denormalise(1.0) - 12.0).abs() < 1e-6);
    assert!((p.denormalise(0.5) - 0.0).abs() < 1e-6);
    assert!((p.normalise(-12.0) - 0.0).abs() < 1e-6);
    assert!((p.normalise(12.0) - 1.0).abs() < 1e-6);
    assert!((p.normalise(0.0) - 0.5).abs() < 1e-6);
}

#[test]
fn node_doc_params_round_trip_and_recompile_finite() {
    // Non-default gains for every band.
    let values = (0..10).map(|i| (i as f32 - 4.5) * 2.0).collect::<Vec<_>>();

    let node = NodeDoc {
        id: "eq".into(),
        block: "tone.graphic_eq".into(),
        params: values
            .iter()
            .enumerate()
            .map(|(i, &v)| (gain_id(i).to_string(), v))
            .collect(),
        bypass: false,
        pos: None,
        model: None, curve: None,
    };
    let doc = Doc {
        name: "roundtrip".into(),
        nodes: vec![node],
        edges: vec![],
        routings: vec![],
    };

    // Serialise / deserialise and confirm the params round-trip.
    let json = doc.to_json();
    let doc2 = Doc::from_json(&json).expect("deserialise NodeDoc");
    let node2 = &doc2.nodes[0];
    assert_eq!(node2.id, "eq");
    assert_eq!(node2.block, "tone.graphic_eq");
    assert_eq!(node2.params.len(), 10);
    for (i, &v) in values.iter().enumerate() {
        let got = node2
            .params
            .get(gain_id(i))
            .copied()
            .expect("param present");
        assert!((got - v).abs() < 1e-6, "param {} mismatch", gain_id(i));
    }

    // Re-compile a graph containing this node (params applied at real values)
    // and confirm the output is finite for a full-scale impulse.
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add(
        "eq",
        reg()
            .build("tone.graphic_eq")
            .expect("graphic eq registered"),
    )
    .unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "eq", "in").unwrap();
    g.connect("eq", "out", "out", "in").unwrap();
    for i in 0..10 {
        g.set_param_real(
            "eq",
            gain_id(i),
            node2
                .params
                .get(gain_id(i))
                .copied()
                .expect("param present"),
        )
        .unwrap();
    }
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();
    let mut input = vec![0.0f32; BLOCK];
    input[0] = 0.5;
    let out = ex.run(&input, BLOCK).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "non-finite output from the round-tripped node"
    );
}
