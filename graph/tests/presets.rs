//! Every shipped preset must load, compile, and produce sane audio.

use dsp_graph::blocks::builtin_registry;
use dsp_graph::{Doc, Executor};

/// The shipped presets are channel-based; the app converts them through
/// `Rig` before compiling. The graph crate can't depend on the app, so this
/// mirrors that conversion: the presets are validated as graphs via a small
/// local rig-to-doc step.
fn rig_doc(json: &str) -> Doc {
    // Parse the preset as a Doc and re-emit it with a summing node, the same
    // shape the app's `Rig::to_doc` produces.
    let doc = Doc::from_json(json).unwrap();
    let mut out = Doc::new(doc.name.clone());
    out.nodes.extend(doc.nodes.iter().cloned());
    // Add a sum node and reroute every edge into `out` through it.
    let mut sum = dsp_graph::NodeDoc {
        id: "sum".into(),
        block: "util.sum".into(),
        params: Default::default(),
        bypass: false,
        pos: None, model: None, curve: None, col: None, };
    sum.params.insert("a".into(), 1.0);
    sum.params.insert("b".into(), 1.0);
    sum.params.insert("c".into(), 1.0);
    sum.params.insert("d".into(), 1.0);
    out.nodes.push(sum);
    let mut edges = Vec::new();
    let mut sum_port = 0;
    for e in &doc.edges {
        if e.to[0] == "out" {
            let port = ["a", "b", "c", "d"][sum_port.min(3)];
            sum_port += 1;
            edges.push(dsp_graph::EdgeDoc { from: e.from.clone(), to: ["sum".to_string(), port.into()] });
        } else {
            edges.push(e.clone());
        }
    }
    edges.push(dsp_graph::EdgeDoc { from: ["sum".to_string(), "out".into()], to: ["out".to_string(), "in".into()] });
    // Unused sum inputs are tied to silence, as the app does.
    for (k, port) in ["a", "b", "c", "d"].iter().enumerate().skip(sum_port) {
        let id = format!("sum_z{k}");
        out.nodes.push(dsp_graph::NodeDoc {
            id: id.clone(),
            block: "util.gain".into(),
            params: [("gain".to_string(), -60.0)].into_iter().collect(),
            bypass: false,
            pos: None, model: None, curve: None, col: None, });
        edges.push(dsp_graph::EdgeDoc { from: ["in".to_string(), "out".into()], to: [id.clone(), "in".into()] });
        edges.push(dsp_graph::EdgeDoc { from: [id, "out".into()], to: ["sum".to_string(), port.to_string()] });
    }
    out.edges = edges;
    out
}

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;

fn presets() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../presets");
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .expect("presets/ dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).unwrap()))
        .collect();
    v.sort();
    v
}

/// A plucked-ish test signal: decaying saw bursts, roughly guitar level.
fn pluck_signal(seconds: f32) -> Vec<f32> {
    let n = (SR * seconds) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let note_t = t % 0.5;
            let f = [82.4, 110.0, 146.8, 196.0][(t / 0.5) as usize % 4];
            let saw = 2.0 * (note_t * f).fract() - 1.0;
            0.3 * saw * (-note_t * 6.0).exp()
        })
        .collect()
}

#[test]
fn all_presets_load_compile_and_sound() {
    let reg = builtin_registry();
    let list = presets();
    assert!(list.len() >= 5, "expected the shipped presets, found {}", list.len());
    let input = pluck_signal(2.0);
    for (file, json) in list {
        let doc = rig_doc(&json);
        let mut g = doc.build(&reg).unwrap_or_else(|e| panic!("{file}: {e}"));
        let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap_or_else(|e| panic!("{file}: {e}"));
        let mut peak = 0.0f32;
        let mut energy = 0.0f64;
        for chunk in input.chunks_exact(BLOCK) {
            for &v in ex.run(chunk, BLOCK).unwrap() {
                assert!(v.is_finite(), "{file}: non-finite output");
                peak = peak.max(v.abs());
                energy += (v * v) as f64;
            }
        }
        let rms = (energy / input.len() as f64).sqrt();
        println!("{file:24} peak {peak:.3}  rms {rms:.4}");
        assert!(peak <= 1.0, "{file}: peak {peak} exceeds full scale (master limiter)");
        assert!(rms > 0.01, "{file}: nearly silent (rms {rms})");
    }
}

#[test]
fn doc_json_round_trips() {
    for (file, json) in presets() {
        let doc = Doc::from_json(&json).unwrap();
        let again = Doc::from_json(&doc.to_json()).unwrap();
        assert_eq!(doc, again, "{file}");
    }
}

#[test]
fn doc_rejects_unknown_param_and_block() {
    let reg = builtin_registry();
    let bad = r#"{"name":"x","nodes":[{"id":"a","block":"amp.basic","params":{"gian":3}}]}"#;
    let e = Doc::from_json(bad).unwrap().build(&reg).err().unwrap().to_string();
    assert!(e.contains("unknown param 'gian'"), "{e}");
    let bad = r#"{"name":"x","nodes":[{"id":"a","block":"amp.nope"}]}"#;
    let e = Doc::from_json(bad).unwrap().build(&reg).err().unwrap().to_string();
    assert!(e.contains("unknown block type"), "{e}");
}

/// A small explicit doc, so this test does not depend on which preset sorts
/// first (a new preset used to silently change the ids generated here).
const EDIT_DOC: &str = r#"{
  "name": "edit test",
  "nodes": [
    {"id": "in", "block": "io.input"},
    {"id": "amp", "block": "amp.basic"},
    {"id": "cab", "block": "cab.sim"},
    {"id": "master", "block": "util.master"},
    {"id": "out", "block": "io.output"}
  ],
  "edges": [
    {"from": ["in", "out"], "to": ["amp", "in"]},
    {"from": ["amp", "out"], "to": ["cab", "in"]},
    {"from": ["cab", "out"], "to": ["master", "in"]},
    {"from": ["master", "out"], "to": ["out", "in"]}
  ]
}"#;

#[test]
fn doc_editing_helpers() {
    let mut d = Doc::from_json(EDIT_DOC).unwrap();
    let id = d.add_node("amp.basic", [0.0, 0.0]);
    assert_eq!(id, "amp2");
    // in -> amp2 is fine; amp2 -> in would loop only if in were downstream.
    assert!(!d.would_loop("in", "amp2"));
    d.connect(("in", "out"), ("amp2", "in"));
    d.connect(("amp2", "out"), ("cab", "in")); // replaces amp -> cab
    assert_eq!(d.edges.iter().filter(|e| e.to == ["cab".to_string(), "in".to_string()]).count(), 1);
    // master -> amp2 would close a loop: amp2 -> cab -> ... -> master.
    assert!(d.would_loop("master", "amp2"));
    d.remove_node("amp2");
    assert!(d.edges.iter().all(|e| e.from[0] != "amp2" && e.to[0] != "amp2"));
}
