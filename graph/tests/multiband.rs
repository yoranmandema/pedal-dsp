//! Tests for `dynamics.multiband_compressor`: crossover flat-sum, per-band
//! compression, band isolation, 1:1 transparency, bypass/solo, safety.
use dsp_graph::blocks::{multiband_meters, IoInput, IoOutput, MultibandCompressor};
use dsp_graph::{Executor, Graph};
use std::sync::Mutex;

/// The block publishes meters through process-wide atomics, so serialise tests.
static LOCK: Mutex<()> = Mutex::new(());
const BLOCK: usize = 64;

fn build(sr: f32, params: &[(&str, f32)]) -> Executor {
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("mbc", Box::new(MultibandCompressor::default())).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "mbc", "in").unwrap();
    g.connect("mbc", "out", "out", "in").unwrap();
    for (k, v) in params {
        g.set_param_real("mbc", k, *v).unwrap();
    }
    Executor::compile_owned(&mut g, sr, BLOCK).unwrap()
}

/// Neutral params: every band 1:1, so only the crossover acts.
fn neutral() -> Vec<(&'static str, f32)> {
    vec![
        ("band_1_ratio", 1.0),
        ("band_2_ratio", 1.0),
        ("band_3_ratio", 1.0),
        ("band_4_ratio", 1.0),
    ]
}

/// Feed `secs` of a sine; return (input_rms, output_rms) over the second half.
fn sine_rms(ex: &mut Executor, sr: f32, freq: f32, amp: f32, secs: f32) -> (f32, f32) {
    let total = (sr * secs) as usize / BLOCK * BLOCK;
    let (mut si, mut so, mut cnt) = (0.0f64, 0.0f64, 0usize);
    let mut buf = [0.0f32; BLOCK];
    for start in (0..total).step_by(BLOCK) {
        for (i, b) in buf.iter_mut().enumerate() {
            let t = (start + i) as f32 / sr;
            *b = amp * (2.0 * std::f32::consts::PI * freq * t).sin();
        }
        let out = ex.run(&buf, BLOCK).unwrap();
        if start >= total / 2 {
            for i in 0..BLOCK {
                si += (buf[i] as f64).powi(2);
                so += (out[i] as f64).powi(2);
            }
            cnt += BLOCK;
        }
    }
    (((si / cnt as f64).sqrt()) as f32, ((so / cnt as f64).sqrt()) as f32)
}

fn gain_db(ex: &mut Executor, sr: f32, freq: f32, amp: f32) -> f32 {
    let (i, o) = sine_rms(ex, sr, freq, amp, 0.5);
    20.0 * (o / i).log10()
}

#[test]
fn crossover_bands_sum_flat() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for &sr in &[44_100.0f32, 48_000.0] {
        for bands in [2.0f32, 3.0, 4.0] {
            let mut p = neutral();
            p.push(("bands", bands));
            let mut ex = build(sr, &p);
            for &f in &[30.0f32, 100.0, 200.0, 400.0, 1000.0, 3000.0, 4000.0, 6000.0, 8000.0, 12000.0] {
                let mut ex2 = build(sr, &p);
                let d = gain_db(&mut ex2, sr, f, 0.05);
                assert!(d.abs() < 0.3, "sr {sr} bands {bands} f {f}: {d:.3} dB");
            }
            let _ = &mut ex;
        }
    }
}

#[test]
fn ratio_one_is_transparent() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ex = build(48_000.0, &[("band_1_ratio", 1.0), ("band_2_ratio", 1.0), ("band_3_ratio", 1.0),
        ("band_1_threshold", -60.0), ("band_2_threshold", -60.0), ("band_3_threshold", -60.0)]);
    let d = gain_db(&mut ex, 48_000.0, 1000.0, 0.8);
    assert!(d.abs() < 0.3, "ratio 1:1 changed level by {d:.3} dB");
}

#[test]
fn above_threshold_band_reduced_others_unaffected() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Band 2 (200 Hz..4 kHz): threshold -20 dB, ratio 4. Others 1:1.
    let params = [
        ("band_1_ratio", 1.0), ("band_3_ratio", 1.0),
        ("band_2_threshold", -20.0), ("band_2_ratio", 4.0),
    ];
    // 1 kHz peak 0.5 (-6 dB). Peak follower sits on ~|x| smoothed; with a
    // release of 50 ms the level hovers between 0.32 and 0.5 -> compare against
    // the formula using the measured mean-ish level, with generous tolerance.
    let mut ex = build(48_000.0, &params);
    let d = gain_db(&mut ex, 48_000.0, 1000.0, 0.5);
    let level_db = 20.0 * (0.5f32 * 0.7).log10(); // ~avg-ish detector level
    let expect = -(level_db + 20.0) * 0.75;
    assert!(d < -3.0, "band 2 should be compressed, got {d:.2} dB");
    assert!((d - expect).abs() < 3.0, "gr {d:.2} vs expected ~{expect:.2}");
    // Meters: band 2 reduces, bands 1 and 3 do not.
    let m = multiband_meters();
    assert!(m[1].0 > 3.0, "band 2 GR meter {:?}", m);
    assert!(m[0].0 < 0.01 && m[2].0 < 0.01, "other bands must not reduce: {:?}", m);
    // Other bands unaffected: a loud 50 Hz tone (band 1) passes at ~0 dB.
    let mut ex = build(48_000.0, &params);
    let d1 = gain_db(&mut ex, 48_000.0, 50.0, 0.8);
    assert!(d1.abs() < 0.5, "band 1 changed by {d1:.2} dB");
    let mut ex = build(48_000.0, &params);
    let d3 = gain_db(&mut ex, 48_000.0, 10_000.0, 0.8);
    assert!(d3.abs() < 0.5, "band 3 changed by {d3:.2} dB");
}

#[test]
fn makeup_and_output_gain_apply() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = neutral();
    p.push(("output_gain", 6.0));
    let mut ex = build(48_000.0, &p);
    let d = gain_db(&mut ex, 48_000.0, 1000.0, 0.1);
    assert!((d - 6.0).abs() < 0.4, "output gain {d:.2}");
    let mut p = neutral();
    p.push(("band_2_makeup", 6.0));
    let mut ex = build(48_000.0, &p);
    let d = gain_db(&mut ex, 48_000.0, 1000.0, 0.1);
    assert!((d - 6.0).abs() < 0.4, "makeup {d:.2}");
}

#[test]
fn dry_wet_zero_is_dry() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ex = build(48_000.0, &[("dry_wet", 0.0), ("band_2_threshold", -60.0), ("band_2_ratio", 20.0)]);
    let d = gain_db(&mut ex, 48_000.0, 1000.0, 0.8);
    assert!(d.abs() < 0.01, "dry path altered level by {d:.3} dB");
}

#[test]
fn bypass_skips_compression_and_solo_isolates() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let hard = [("band_2_threshold", -40.0), ("band_2_ratio", 20.0)];
    let mut p = hard.to_vec();
    p.push(("band_2_bypass", 1.0));
    let mut ex = build(48_000.0, &p);
    let d = gain_db(&mut ex, 48_000.0, 1000.0, 0.8);
    assert!(d.abs() < 0.3, "bypassed band changed level {d:.2}");
    assert!(multiband_meters()[1].0 < 0.01);

    // Solo band 2: 1 kHz passes, 50 Hz and 10 kHz are removed.
    let mut p = neutral();
    p.push(("band_2_solo", 1.0));
    let mut ex = build(48_000.0, &p);
    assert!(gain_db(&mut ex, 48_000.0, 1000.0, 0.5).abs() < 0.3);
    let mut ex = build(48_000.0, &p);
    assert!(gain_db(&mut ex, 48_000.0, 50.0, 0.5) < -20.0);
    let mut ex = build(48_000.0, &p);
    assert!(gain_db(&mut ex, 48_000.0, 10_000.0, 0.5) < -20.0);
}

#[test]
fn silence_in_silence_out() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ex = build(48_000.0, &[]);
    let z = [0.0f32; BLOCK];
    for _ in 0..2000 {
        let out = ex.run(&z, BLOCK).unwrap();
        assert!(out.iter().all(|v| *v == 0.0));
    }
    assert!(multiband_meters().iter().all(|m| m.0 == 0.0));
}

#[test]
fn nan_inf_and_extreme_params_stay_finite() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ex = build(
        44_100.0,
        &[("bands", 4.0), ("crossover_1", 2000.0), ("crossover_2", 200.0), ("crossover_3", 20000.0),
          ("band_1_ratio", 20.0), ("band_1_threshold", -60.0), ("band_1_attack", 0.1)],
    );
    let mut buf = [0.0f32; BLOCK];
    for blk in 0..400 {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = match (blk * BLOCK + i) % 97 {
                0 => f32::NAN,
                1 => f32::INFINITY,
                2 => f32::NEG_INFINITY,
                k => ((k as f32) * 0.37).sin() * 4.0,
            };
        }
        let out = ex.run(&buf, BLOCK).unwrap();
        assert!(out.iter().all(|v| v.is_finite()), "block {blk}");
    }
    // Decay to silence stays finite and denormal-free-ish.
    let z = [0.0f32; BLOCK];
    for _ in 0..5000 {
        let out = ex.run(&z, BLOCK).unwrap();
        assert!(out.iter().all(|v| v.is_finite() && (*v == 0.0 || v.abs() >= f32::MIN_POSITIVE)));
    }
}

#[test]
fn registered_in_registry() {
    let r = dsp_graph::blocks::builtin_registry();
    assert!(r.get("dynamics.multiband_compressor").is_some());
}

#[test]
fn per_band_gr_matches_formula_across_ratio_and_threshold() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Fast attack + max release hold the peak detector at the tone's peak, so
    // GR = (peak_dB - threshold) * (1 - 1/ratio) can be checked tightly.
    let amp = 0.5f32; // -6.02 dBFS peak
    let peak_db = 20.0 * amp.log10();
    for (ratio, thr) in [(2.0f32, -40.0f32), (4.0, -20.0), (10.0, -10.0)] {
        let params = [
            ("band_1_ratio", 1.0), ("band_3_ratio", 1.0),
            ("band_2_threshold", thr), ("band_2_ratio", ratio),
            ("band_2_attack", 0.1), ("band_2_release", 500.0),
        ];
        let mut ex = build(48_000.0, &params);
        // 1 kHz sits mid band 2 (200 Hz..4 kHz).
        let _ = sine_rms(&mut ex, 48_000.0, 1000.0, amp, 1.0);
        let expect = (peak_db - thr).max(0.0) * (1.0 - 1.0 / ratio);
        let gr = multiband_meters()[1].0;
        assert!(
            (gr - expect).abs() < 0.1,
            "ratio {ratio} thr {thr}: meter GR {gr:.3} dB vs expected {expect:.3} dB"
        );
    }
}
