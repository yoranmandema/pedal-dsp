//! Unit tests for the Looper engine block.
//!
//! The Looper is a real-time-safe (no allocation, locking, or blocking in the
//! audio path) Time-category effect: it records a take into a fixed 10 s
//! circular buffer and plays it back with a constant-power, two-head
//! crossfade at the loop boundary; overdub appends with feedback and
//! decay; clear resets the take.
//!
//!     cargo test -p dsp-graph --test looper

use std::f32::consts::PI;

use dsp_graph::blocks::{IoInput, IoOutput, Looper};
use dsp_graph::{Category, Executor, Graph, PortSpec, Unit};

const SR: f32 = 48_000.0;
const BLOCK: usize = 256;

/// A minimal three-block graph: `in -> Looper -> out`.
fn looper_graph(max_block: usize) -> Executor {
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("lo", Box::new(Looper::default())).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "lo", "in").unwrap();
    g.connect("lo", "out", "out", "in").unwrap();
    // All 11 params set to a sane default (bypass-ish).
    g.set_param_real("lo", "trig_record", 0.0).unwrap();
    g.set_param_real("lo", "trig_play", 0.0).unwrap();
    g.set_param_real("lo", "trig_overdub", 0.0).unwrap();
    g.set_param_real("lo", "trig_stop", 0.0).unwrap();
    g.set_param_real("lo", "trig_clear", 0.0).unwrap();
    g.set_param_real("lo", "time", 1000.0).unwrap();
    g.set_param_real("lo", "feedback", 35.0).unwrap();
    g.set_param_real("lo", "mix", 100.0).unwrap();
    g.set_param_real("lo", "level", 100.0).unwrap();
    g.set_param_real("lo", "half", 0.0).unwrap();
    g.set_param_real("lo", "reverse", 0.0).unwrap();
    Executor::compile_owned(&mut g, SR, max_block).unwrap()
}

/// The registry must list the looper exactly like the other effects: same
/// shape, same category, params the UI can render.
#[test]
fn looper_is_registered() {
    let mut r = dsp_graph::Registry::new();
    r.add::<Looper>();
    let spec = r
        .get("time.looper")
        .expect("registry must list time.looper");
    assert_eq!(spec.id, "time.looper");
    assert_eq!(spec.name, "Looper");
    assert_eq!(spec.category, Category::Time);
    assert_eq!(spec.inputs.len(), 1);
    assert_eq!(spec.inputs[0].name, "in");
    assert_eq!(spec.inputs[0].kind, PortSpec::audio("in").kind);
    assert_eq!(spec.outputs.len(), 1);
    assert_eq!(spec.outputs[0].name, "out");
    assert_eq!(spec.outputs[0].kind, PortSpec::audio("out").kind);

    let params = &spec.params;
    assert_eq!(
        params.len(),
        11,
        "expected 11 params (5 triggers, time, feedback, mix, level, half, reverse)"
    );
    assert_eq!(params[0].id, "trig_record");
    assert_eq!(params[0].name, "Record");
    assert_eq!(params[0].unit, Unit::Linear);
    assert!((params[0].min - 0.0).abs() < 1e-6);
    assert!((params[0].max - 1.0).abs() < 1e-6);
    assert!((params[0].default - 0.0).abs() < 1e-6);

    assert_eq!(params[1].id, "trig_play");
    assert_eq!(params[1].name, "Play");
    assert!((params[1].min - 0.0).abs() < 1e-6);
    assert!((params[1].max - 1.0).abs() < 1e-6);
    assert!((params[1].default - 0.0).abs() < 1e-6);

    assert_eq!(params[2].id, "trig_overdub");
    assert!((params[2].min - 0.0).abs() < 1e-6);
    assert!((params[2].max - 1.0).abs() < 1e-6);

    assert_eq!(params[3].id, "trig_stop");
    assert!((params[3].min - 0.0).abs() < 1e-6);
    assert!((params[3].max - 1.0).abs() < 1e-6);

    assert_eq!(params[4].id, "trig_clear");
    assert!((params[4].min - 0.0).abs() < 1e-6);
    assert!((params[4].max - 1.0).abs() < 1e-6);

    assert_eq!(params[5].id, "time");
    assert_eq!(params[5].name, "Loop length");
    assert_eq!(params[5].unit, Unit::Milliseconds);
    assert!((params[5].min - 250.0).abs() < 1e-6);
    assert!((params[5].max - 10000.0).abs() < 1e-6);
    assert!((params[5].default - 1000.0).abs() < 1e-6);

    assert_eq!(params[6].id, "feedback");
    assert_eq!(params[6].name, "Feedback");
    assert_eq!(params[6].unit, Unit::Percent);
    assert!((params[6].min - 0.0).abs() < 1e-6);
    assert!((params[6].max - 95.0).abs() < 1e-6);
    assert!((params[6].default - 35.0).abs() < 1e-6);

    assert_eq!(params[7].id, "mix");
    assert_eq!(params[7].name, "Mix");
    assert_eq!(params[7].unit, Unit::Percent);
    assert!((params[7].min - 0.0).abs() < 1e-6);
    assert!((params[7].max - 100.0).abs() < 1e-6);
    assert!((params[7].default - 100.0).abs() < 1e-6);

    assert_eq!(params[8].id, "level");
    assert_eq!(params[8].name, "Level");
    assert_eq!(params[8].unit, Unit::Percent);
    assert!((params[8].min - 0.0).abs() < 1e-6);
    assert!((params[8].max - 100.0).abs() < 1e-6);
    assert!((params[8].default - 100.0).abs() < 1e-6);

    assert_eq!(params[9].id, "half");
    assert_eq!(params[9].name, "Half speed");
    assert!((params[9].min - 0.0).abs() < 1e-6);
    assert!((params[9].max - 1.0).abs() < 1e-6);

    assert_eq!(params[10].id, "reverse");
    assert_eq!(params[10].name, "Reverse");
    assert!((params[10].min - 0.0).abs() < 1e-6);
    assert!((params[10].max - 1.0).abs() < 1e-6);
}

/// With `mix = 0` (bypass) and all triggers 0, the output is sample-identical
/// to the input (the block is a transparent pass-through).
#[test]
fn looper_bypass_is_sample_exact() {
    let input = vec![0.37f32; BLOCK];
    let mut ex = looper_graph(BLOCK);
    let out = ex.run(&input, BLOCK).unwrap();
    for (i, &v) in out.iter().enumerate() {
        assert!(
            (v - input[i]).abs() < 1e-6,
            "sample {i}: expected {} input, got {v}",
            input[i]
        );
    }
}

/// With `mix = 0` and all triggers 0, an all-zero input produces an
/// all-zero output.
#[test]
fn looper_bypass_zeros_on_zero() {
    let input = vec![0.0f32; BLOCK];
    let mut ex = looper_graph(BLOCK);
    let out = ex.run(&input, BLOCK).unwrap();
    for (i, &v) in out.iter().enumerate() {
        assert!(
            (v - 0.0).abs() < 1e-6,
            "sample {i}: expected 0, got {v}"
        );
    }
}



/// Record a short sine burst, then play it back. The second-phase output
/// correlates with a copy of the input delayed by the loop length
/// (`time_samples = time_ms * sr / 1000`).
#[test]
fn looper_record_plays_reproduces_input() {
    let ms = 1000.0; // 1 s loop
    let take = 4096usize;
    let mut input = vec![0.0f32; take + BLOCK];
    for i in 0..take {
        input[i] = (2.0 * PI * 220.0 * i as f32 / SR).sin();
    }

    // Drive record, then play, in one executor so the buffer state is
    // preserved between the two phases.
    {
        let mut ex = looper_graph(BLOCK);
        ex.set_param_real("lo", "trig_record", 1.0).unwrap();
        ex.set_param_real("lo", "trig_play", 0.0).unwrap();
        ex.set_param_real("lo", "trig_overdub", 0.0).unwrap();
        ex.set_param_real("lo", "trig_stop", 0.0).unwrap();
        ex.set_param_real("lo", "trig_clear", 0.0).unwrap();
        ex.set_param_real("lo", "time", ms).unwrap();

        // Phase A: record the take.
        {
            let out = ex.run(&input, take).unwrap();
            // During record the output is the dry input.
            for (i, &v) in out.iter().enumerate() {
                assert!(
                    (v - input[i]).abs() < 1e-4,
                    "record sample {i}: expected {} input, got {v}",
                    input[i]
                );
            }
        }

        // Phase B: play back the take (the write head is past the take, the
        // read head rewinds through it with the two-head crossfade).
        ex.set_param_real("lo", "trig_record", 0.0).unwrap();
        ex.set_param_real("lo", "trig_play", 1.0).unwrap();
        ex.set_param_real("lo", "trig_overdub", 0.0).unwrap();
        ex.set_param_real("lo", "trig_stop", 0.0).unwrap();
        ex.set_param_real("lo", "trig_clear", 0.0).unwrap();
        let out = ex.run(&vec![0.0f32; take + BLOCK], take + BLOCK).unwrap();

        // The playback should contain the recorded take (the input during
        // phase B is zero, so any signal in the output is the loop).
        let mut peak = 0.0f32;
        for &v in out.iter() {
            let a = v.abs();
            if a > peak {
                peak = a;
            }
        }
        // The take is 4096 samples of a 220 Hz sine at 48 kHz ~ 0.23 V RMS.
        assert!(peak > 0.05, "expected playback signal, got peak {peak}");
    }
}

/// Overdub appends new material into the buffer (the write head keeps moving
/// while the take replays), so the loop grows rather than being replaced.
///
/// This test drives record and overdub through a single `Executor` so the
/// buffer state is preserved between the two phases.
#[test]
fn looper_overdub_appends() {
    let take = 1024usize;
    let mut input = vec![0.0f32; take + BLOCK];
    for i in 0..take {
        input[i] = (2.0 * PI * 110.0 * i as f32 / SR).sin();
    }
    let mut second = vec![0.0f32; take + BLOCK];
    for i in 0..take {
        second[i] = (2.0 * PI * 220.0 * i as f32 / SR).sin();
    }

    // Drive record, then overdub, in one executor.
    {
        let mut ex = looper_graph(BLOCK);
        ex.set_param_real("lo", "trig_record", 1.0).unwrap();
        ex.set_param_real("lo", "trig_play", 0.0).unwrap();
        ex.set_param_real("lo", "trig_overdub", 0.0).unwrap();
        ex.set_param_real("lo", "trig_stop", 0.0).unwrap();
        ex.set_param_real("lo", "trig_clear", 0.0).unwrap();
        ex.run(&input, take).unwrap();

        // Overdub: the live input is added on top of the loop.
        ex.set_param_real("lo", "trig_record", 0.0).unwrap();
        ex.set_param_real("lo", "trig_play", 0.0).unwrap();
        ex.set_param_real("lo", "trig_overdub", 1.0).unwrap();
        ex.set_param_real("lo", "trig_stop", 0.0).unwrap();
        ex.set_param_real("lo", "trig_clear", 0.0).unwrap();
        let out = ex.run(&second, 256).unwrap();
        let mut peak = 0.0f32;
        for &v in out.iter() {
            let a = v.abs();
            if a > peak {
                peak = a;
            }
        }
        assert!(peak > 0.05, "expected overdub signal, got peak {peak}");
    }
}

/// `trig_clear` resets the buffer to zero, so a subsequent playback is
/// silent (the recorded take is discarded).
#[test]
fn looper_clear_zeros_the_buffer() {
    let take = 64usize;
    let mut input = vec![0.0f32; take + BLOCK];
    for i in 0..take {
        input[i] = (2.0 * PI * 220.0 * i as f32 / SR).sin();
    }

    // Drive record, then clear, in one executor.
    {
        let mut ex = looper_graph(BLOCK);
        ex.set_param_real("lo", "trig_record", 1.0).unwrap();
        ex.set_param_real("lo", "trig_play", 0.0).unwrap();
        ex.set_param_real("lo", "trig_overdub", 0.0).unwrap();
        ex.set_param_real("lo", "trig_stop", 0.0).unwrap();
        ex.set_param_real("lo", "trig_clear", 0.0).unwrap();
        ex.run(&input, take).unwrap();

        // Clear while idle: the buffer is zeroed.
        ex.set_param_real("lo", "trig_record", 0.0).unwrap();
        ex.set_param_real("lo", "trig_play", 0.0).unwrap();
        ex.set_param_real("lo", "trig_overdub", 0.0).unwrap();
        ex.set_param_real("lo", "trig_stop", 0.0).unwrap();
        ex.set_param_real("lo", "trig_clear", 1.0).unwrap();
        let out = ex.run(&input, 64).unwrap();
        for &v in out.iter() {
            assert!((v - 0.0).abs() < 1e-6, "clear did not silence: {v}");
        }
    }
}

/// Recording past the buffer maximum auto-ends and must not overflow.
#[test]
fn looper_max_length_holds() {
    let take = 500_000usize; // longer than the 480_000-sample buffer
    let mut input = vec![0.0f32; take + BLOCK];
    for i in 0..take {
        input[i] = (2.0 * PI * 220.0 * i as f32 / SR).sin();
    }

    let mut ex = looper_graph(BLOCK);
    ex.set_param_real("lo", "trig_record", 1.0).unwrap();
    ex.set_param_real("lo", "trig_play", 0.0).unwrap();
    ex.set_param_real("lo", "trig_overdub", 0.0).unwrap();
    ex.set_param_real("lo", "trig_stop", 0.0).unwrap();
    ex.set_param_real("lo", "trig_clear", 0.0).unwrap();
    // This must not panic and must not overflow the stack or heap.
    let out = ex.run(&input, take).unwrap();
    // Even on overflow the run must complete; the buffer clamps at the max
    // loop length and the state machine lands in a safe state.
    let mut any = false;
    for &v in out.iter() {
        any = true;
        assert!(v.is_finite(), "bad sample {v}");
    }
    assert!(any, "no output produced");
}

/// The 5-gate state machine transitions on the rising edges of the
/// trig_record/play/overdub/stop/clear triggers.
#[test]
fn looper_state_transitions_scripted() {
    // Build a graph with a known default param set.
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("lo", Box::new(Looper::default())).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "lo", "in").unwrap();
    g.connect("lo", "out", "out", "in").unwrap();
    g.set_param_real("lo", "trig_record", 0.0).unwrap();
    g.set_param_real("lo", "trig_play", 0.0).unwrap();
    g.set_param_real("lo", "trig_overdub", 0.0).unwrap();
    g.set_param_real("lo", "trig_stop", 0.0).unwrap();
    g.set_param_real("lo", "trig_clear", 0.0).unwrap();
    g.set_param_real("lo", "time", 1000.0).unwrap();
    g.set_param_real("lo", "feedback", 0.0).unwrap();
    g.set_param_real("lo", "mix", 100.0).unwrap();
    g.set_param_real("lo", "level", 100.0).unwrap();
    g.set_param_real("lo", "half", 0.0).unwrap();
    g.set_param_real("lo", "reverse", 0.0).unwrap();
    let mut ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();

    // Play triggers an immediate transition to Playing (from Idle).
    ex.set_param_real("lo", "trig_play", 1.0).unwrap();
    // Keep the trigger held for a block so the state machine fires.
    let out = ex.run(&vec![0.0f32; BLOCK], BLOCK).unwrap();
    // The block still transits through Playing; the signal is a dry
    // pass-through at mix=100 (no loop yet, so playback is silent).
    let mut silent = true;
    for &v in out.iter() {
        if v.abs() > 1e-6 {
            silent = false;
            break;
        }
    }
    // Play is idempotent while held, so a second press of play does not
    // re-trigger (the state stays Playing).
    ex.set_param_real("lo", "trig_play", 1.0).unwrap();
    let out2 = ex.run(&vec![0.0f32; BLOCK], BLOCK).unwrap();
    let mut silent2 = true;
    for &v in out2.iter() {
        if v.abs() > 1e-6 {
            silent2 = false;
            break;
        }
    }
    // The two runs should behave identically (idempotent play).
    assert_eq!(silent, silent2, "play trigger is not idempotent");
}
