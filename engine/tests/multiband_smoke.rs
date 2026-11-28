//! Multiband compressor: preset round trip and a 10 s offline audio-path smoke.
//! No audio device is opened; the rig is rendered straight through the executor.

use std::sync::atomic::Ordering;

use dsp_graph::blocks::{builtin_registry, multiband_meters, MULTIBAND_ACTIVE};
use dsp_graph::{Doc, Executor};
use pedal_engine::channels::Rig;

const SR: f32 = 48_000.0;
const BLOCK: usize = 64;
const ID: &str = "dynamics.multiband_compressor";

fn rig(bands: f32) -> (Doc, String) {
    let mut rig = Rig::new("mb");
    let id = rig.add_block_as("ch1", ID, "Multiband", [0.0, 0.0], None).unwrap();
    let mut doc = rig.to_doc();
    let n = doc.node_mut(&id).expect("multiband node");
    n.params.insert("bands".into(), bands);
    n.params.insert("crossover_1".into(), 250.0);
    n.params.insert("band_1_threshold".into(), -30.0);
    n.params.insert("band_1_ratio".into(), 4.0);
    n.params.insert("output_gain".into(), 1.5);
    (doc, id)
}

#[test]
fn multiband_params_survive_json_and_rig_round_trip() {
    for bands in [3.0f32, 4.0] {
        let (doc, id) = rig(bands);
        let back = Doc::from_json(&doc.to_json()).expect("parse");
        assert_eq!(back, doc, "JSON round trip changed the doc");
        let p = &back.node(&id).unwrap().params;
        assert_eq!(p["bands"], bands);
        assert_eq!(p["crossover_1"], 250.0);
        assert_eq!(p["band_1_threshold"], -30.0);
        assert_eq!(p["band_1_ratio"], 4.0);
        assert_eq!(p["output_gain"], 1.5);

        // Through the channel model and back.
        let again = Rig::from_doc(&back).to_doc();
        assert_eq!(again.node(&id).map(|n| &n.params), Some(p), "rig round trip lost params");
    }
}

/// A loud low tone plus a quiet high tone, with a transient burst each second.
fn render(bands: f32) -> (f32, f32, usize) {
    let (doc, _) = rig(bands);
    let reg = builtin_registry();
    let mut g = doc.build(&reg).expect("build");
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).expect("compile");
    let total = SR as usize * 10 / BLOCK * BLOCK;
    let mut buf = [0.0f32; BLOCK];
    let (mut peak, mut energy, mut bad) = (0.0f32, 0.0f64, 0usize);
    for start in (0..total).step_by(BLOCK) {
        for (i, b) in buf.iter_mut().enumerate() {
            let n = start + i;
            let t = n as f32 / SR;
            let burst = if n % (SR as usize) < 2400 { 0.8 } else { 0.1 };
            *b = burst * (std::f32::consts::TAU * 110.0 * t).sin()
                + 0.05 * (std::f32::consts::TAU * 5000.0 * t).sin();
        }
        let out = ex.run(&buf, BLOCK).expect("run");
        for &s in &out[..BLOCK] {
            if !s.is_finite() {
                bad += 1;
            } else {
                peak = peak.max(s.abs());
                energy += (s as f64).powi(2);
            }
        }
    }
    (peak, (energy / total as f64).sqrt() as f32, bad)
}

#[test]
fn multiband_rig_renders_ten_seconds_sanely() {
    for bands in [3.0f32, 4.0] {
        let (peak, rms, bad) = render(bands);
        assert_eq!(bad, 0, "{bands} bands: non-finite samples");
        assert!(rms > 0.01, "{bands} bands: output silent (rms {rms})");
        assert!(peak < 4.0, "{bands} bands: runaway output (peak {peak})");
        let m = multiband_meters();
        assert_eq!(
            MULTIBAND_ACTIVE.load(Ordering::Relaxed),
            bands as u32,
            "active band count"
        );
        assert!(m[0].0 > 0.5, "band 1 (110 Hz burst) should be compressed, gr {}", m[0].0);
        println!("{bands} bands: peak {peak:.3} rms {rms:.3}");
    }
}
