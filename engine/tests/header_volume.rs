//! Engine-level regression tests for the header volume controls:
//! global input trim and master output volume.
//!
//! No audio device, no sound: the graph test drives the executor directly and
//! the IPC test is a headless WebSocket client.

use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use dsp_graph::blocks::{IoInput, IoOutput};
use dsp_graph::{Executor, Graph};
use futures_util::{SinkExt, StreamExt};
use pedal_engine::audio::{new_engine, Setup, ChannelMode};
use pedal_engine::channels::Rig;
use pedal_engine::server::{serve_listener, Shared};
use pedal_engine::store::Store;
use pedal_dsp::util::db_to_lin;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

const SR: f32 = 48_000.0;

/// A rig with both header values set, to exercise the `Doc` round trip.
fn header_rig() -> Rig {
    let mut rig = Rig::new("header-volumes");
    rig.input_gain = -6.0;
    rig.master_volume = -3.0;
    rig
}

/// `header_volumes_survive_a_doc_round_trip` — set `input_gain = -6.0`,
/// `master_volume = -3.0`; `Rig::from_doc(&rig.to_doc())` returns both
/// exactly.
#[test]
fn header_volumes_survive_a_doc_round_trip() {
    let rig = header_rig();
    let back = Rig::from_doc(&rig.to_doc());
    assert_eq!(back.input_gain, -6.0);
    assert_eq!(back.master_volume, -3.0);
}

/// `the_input_trim_scales_the_rendered_output` — `Graph` with `IoInput` →
/// `IoOutput`, `Executor::compile_owned`, `set_param_at(in_node, 0, -60.0)`,
/// run one block of a known non-silent signal, assert the output peak is
/// `< 1e-3` of the unity-gain peak. No audio device, no sound.
#[test]
fn the_input_trim_scales_the_rendered_output() {
    let source = Arc::new(AtomicU8::new(0));
    let (_handle, _cmd_rx, _trash_tx, _running) = new_engine(
        Setup {
            sample_rate: SR,
            out_channels: 2,
            block: 64,
            channel_mode: ChannelMode::First(2),
            latency_frames: 0,
        },
        source.clone(),
    );

    // in -> out. The executor writes the raw input into the `io.input` node's
    // output buffer before running the steps, and `IoInput::process` scales it
    // in place. A square wave is non-silent at every sample, so the peak is
    // exact and repeatable.
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).expect("add io.input");
    g.add("out", Box::new(IoOutput)).expect("add io.output");
    g.connect("in", "out", "out", "in").expect("connect");

    let mut ex = Executor::compile_owned(&mut g, SR, 64).expect("compile rig");
    let in_node = ex
        .node_ids()
        .iter()
        .position(|n| n == "in")
        .expect("in node");

    let signal = [0.5f32; 128];
    let unity = ex.run(&signal, 64).expect("run unity");
    let unity_peak = unity.iter().fold(0.0f32, |p, &v| p.max(v.abs()));

    // Apply the trim as the IPC `set_input_gain` handler does, in real-value
    // space, at the `gain` param (position 0) of the `in` node.
    ex.set_param_at(in_node, 0, -60.0);

    let attenuated = ex.run(&signal, 64).expect("run attenuated");
    let attenuated_peak = attenuated.iter().fold(0.0f32, |p, &v| p.max(v.abs()));

    // -60 dB is 1e-3 linear. `db_to_lin` is a libm `powf`, so allow one ulp of
    // slack on the threshold while the output must still be a deep null.
    let expected = db_to_lin(-60.0); // ≈ 1e-3
    assert!(
        attenuated_peak <= unity_peak * expected * 1.000001,
        "input trim did not scale the rendered output (unity {unity_peak}, attenuated {attenuated_peak}, expected <= {expected})"
    );
}

/// The IPC commands report and clamp.
///
/// `serve_listener` on an ephemeral port as `channel_slots.rs` does; send
/// `setInputGain {value: 999}` and `setMasterOutVolume {value: -999}`; assert
/// the replies carry `inputGain == 24` and `masterOutVolume == -60`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ipc_commands_report_and_clamp() {
    let source = Arc::new(AtomicU8::new(0));
    let (mut handle, _cmd_rx, _trash_tx, _running) = new_engine(
        Setup {
            sample_rate: SR,
            out_channels: 2,
            block: 64,
            channel_mode: ChannelMode::First(2),
            latency_frames: 0,
        },
        source.clone(),
    );
    let registry = handle.registry.clone();
    let store = Store::discover();
    handle.preset_dir = store.dir.clone();

    let sh = Arc::new(Shared::new(handle, registry, source, store,
        Arc::new(AtomicBool::new(true)), Arc::new(pedal_engine::models::Models::new())));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().unwrap().port();
    let sh_task = sh.clone();
    tokio::spawn(async move { serve_listener(sh_task, &listener).await });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    let _greeting = next_event(&mut ws, "state").await;

    // setInputGain {value: 999} — must clamp to +24 and report 24.
    ws.send(Message::Text(
        json!({
            "type": "req", "id": 1, "cmd": "setInputGain", "args": { "value": 999.0 }
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send");
    let res = next_res(&mut ws, 1).await;
    assert_eq!(res["ok"], true, "setInputGain should be accepted: {res}");
    assert_eq!(
        res["payload"]["inputGain"],
        24.0,
        "inputGain should be clamped to +24: {res}"
    );

    // setMasterOutVolume {value: -999} — must clamp to -60 and report -60.
    ws.send(Message::Text(
        json!({
            "type": "req", "id": 2, "cmd": "setMasterOutVolume", "args": { "value": -999.0 }
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send");
    let res = next_res(&mut ws, 2).await;
    assert_eq!(res["ok"], true, "setMasterOutVolume should be accepted: {res}");
    assert_eq!(
        res["payload"]["masterOutVolume"],
        -60.0,
        "masterOutVolume should be clamped to -60: {res}"
    );
}

/// The next text frame, skipping ping/pong/close.
async fn next_text<S>(ws: &mut S) -> String
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => return t.to_string(),
            Some(Ok(_)) => continue,
            other => panic!("websocket closed or errored: {other:?}"),
        }
    }
}

/// The next `event` frame of the given kind. Meters arrive on their own timer
/// and can overtake a reply.
async fn next_event<S>(ws: &mut S, event: &str) -> Value
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    loop {
        let v: Value = serde_json::from_str(&next_text(ws).await).expect("json");
        if v["type"] == "event" && v["event"] == event {
            return v;
        }
    }
}

/// The reply to the request with this id.
async fn next_res<S>(ws: &mut S, id: i64) -> Value
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    loop {
        let v: Value = serde_json::from_str(&next_text(ws).await).expect("json");
        if v["type"] == "res" && v["id"] == json!(id) {
            return v;
        }
    }
}
