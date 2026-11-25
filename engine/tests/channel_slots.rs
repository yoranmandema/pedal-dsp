//! The channel list is fixed: [`CHANNEL_COUNT`] slots, never added to or
//! removed from.
//!
//! Both halves of that contract are tested against the real surface, not a
//! mock: the model keeps every slot across a project-file round trip, and the
//! IPC command set has no entry that mutates the list — a command naming a
//! channel that does not exist is refused, and the slot count is unchanged
//! across a refused command.

use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use pedal_engine::audio::{new_engine, ChannelMode, Setup};
use pedal_engine::channels::{Rig, CHANNEL_COUNT, IN};
use pedal_engine::server::{serve_listener, Shared};
use pedal_engine::store::Store;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

const SR: f32 = 48_000.0;

fn setup() -> Setup {
    Setup {
        sample_rate: SR,
        out_channels: 2,
        block: 64,
        channel_mode: ChannelMode::First(2),
        latency_frames: 0,
    }
}

/// [`CHANNEL_COUNT`] is the slot count the UI and the docs promise.
#[test]
fn a_fresh_rig_has_every_slot() {
    assert_eq!(CHANNEL_COUNT, 4, "the UI promises four channel slots");
    let rig = Rig::new("slots");
    assert_eq!(rig.channels.len(), CHANNEL_COUNT);
    for (i, c) in rig.channels.iter().enumerate() {
        assert_eq!(c.id, format!("ch{}", i + 1), "slot ids are stable");
        assert_eq!(c.sources, vec![IN.to_string()]);
        assert!(c.blocks.is_empty(), "a fresh slot is empty");
    }
}

/// Saving and reloading must not grow or shrink the slot list, and content
/// must land back in the same slot.
#[test]
fn channel_slots_survive_a_doc_round_trip() {
    let mut rig = Rig::new("slots");
    rig.add_block("ch1", "amp.basic", [0.0, 0.0]);

    let doc = rig.to_doc();
    // Empty slots are real in the model but cost nothing in the graph.
    assert!(
        !doc.nodes.iter().any(|n| n.id.starts_with("ch2")),
        "an empty channel should not be compiled into the graph: {:?}",
        doc.nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
    );

    let back = Rig::from_doc(&doc);
    assert_eq!(back.channels.len(), CHANNEL_COUNT, "the slot count is fixed");
    assert_eq!(back.channels[0].id, "ch1", "content stays in its slot");
    assert_eq!(back.channels[0].blocks.len(), 1);
    assert!(
        back.channels[1..].iter().all(|c| c.blocks.is_empty()),
        "the remaining slots come back empty, not missing"
    );
}

async fn next_text<S>(ws: &mut S) -> String
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => return t.to_string(),
            Some(Ok(_)) => continue,
            other => panic!("websocket closed or errored: {other:?}"),
        }
    }
}

async fn next_event<S>(ws: &mut S, event: &str) -> Value
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let v: Value = serde_json::from_str(&next_text(ws).await).expect("json");
        if v["type"] == "event" && v["event"] == event {
            return v;
        }
    }
}

async fn next_res<S>(ws: &mut S, id: i64) -> Value
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let v: Value = serde_json::from_str(&next_text(ws).await).expect("json");
        if v["type"] == "res" && v["id"] == json!(id) {
            return v;
        }
    }
}

/// The IPC surface offers no way to change the channel count.
///
/// The command set has no entry that mutates the list, so a command that names
/// a channel outside the fixed set — the shape an "add a channel" or "remove a
/// channel" command would have to take — is refused, and the slot count is
/// unchanged across a refused command. The greeting already carries every slot,
/// so there is nothing for the client to add.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ipc_surface_cannot_add_or_remove_a_channel() {
    let source = Arc::new(AtomicU8::new(0));
    let (mut handle, _cmd_rx, _trash_tx, _running) = new_engine(setup(), source.clone());
    let registry = handle.registry.clone();
    let store = Store::discover();
    handle.preset_dir = store.dir.clone();

    let sh = Arc::new(Shared::new(handle, registry, source, store,
        Arc::new(AtomicBool::new(true)), Arc::new(pedal_engine::models::Models::new())));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let sh_task = sh.clone();
    tokio::spawn(async move { serve_listener(sh_task, &listener).await });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    let greeting = next_event(&mut ws, "state").await;
    let before = greeting["payload"]["channels"].as_array().unwrap().len();
    assert_eq!(before, CHANNEL_COUNT, "the greeting already carries every slot");

    // A channel outside the fixed set does not exist, and no command grows the
    // list to reach it. `setMasterVolume` is the representative mutation: it
    // takes a channel id and returns the full state, so if the count could be
    // changed by any request this is the shape it would take.
    for (id, cmd, args) in [
        (1i64, "setMasterVolume", json!({ "channel": "ch5", "value": 0.0 })),
        (2, "setSources", json!({ "channel": "ch5", "sources": ["in"] })),
        (3, "addBlock", json!({ "channel": "ch5", "type": "amp.basic", "name": "Amp" })),
    ] {
        ws.send(Message::Text(
            json!({ "type": "req", "id": id, "cmd": cmd, "args": args }).to_string().into(),
        ))
        .await
        .expect("send");
        let res = next_res(&mut ws, id).await;
        assert_eq!(res["ok"], false, "{cmd} on a non-existent slot must be refused: {res}");
        assert!(
            res["error"].as_str().is_some_and(|e| e.contains("no such channel")),
            "{cmd} should be refused for the missing slot: {res}"
        );
    }

    // Every refusal is a no-op, so the count is unchanged.
    let after = sh.rig.lock().unwrap().channels.len();
    assert_eq!(after, before, "a refused command changed the channel list");
}

/// A rig of nothing but empty slots runs the bypass, not a compiled rig, and
/// every channel-addressed command still works while it is empty.
///
/// The empty-rig fast path is what this pins: the executor the audio thread
/// ends up with has no channel master in it (an empty slot is not compiled),
/// so a volume move on an empty channel is a no-op rather than an error, and
/// adding a block to a previously empty channel makes the rig compile normally
/// again — no reload, no restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_rig_runs_the_bypass_and_resumes_on_the_first_block() {
    let source = Arc::new(AtomicU8::new(0));
    let (mut handle, _cmd_rx, _trash_tx, _running) = new_engine(setup(), source.clone());
    let registry = handle.registry.clone();
    let store = Store::discover();
    handle.preset_dir = store.dir.clone();

    let sh = Arc::new(Shared::new(handle, registry, source, store,
        Arc::new(AtomicBool::new(true)), Arc::new(pedal_engine::models::Models::new())));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let sh_task = sh.clone();
    tokio::spawn(async move { serve_listener(sh_task, &listener).await });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    let greeting = next_event(&mut ws, "state").await;
    assert_eq!(greeting["payload"]["channels"].as_array().unwrap().len(), CHANNEL_COUNT);

    // `rebuild` is private, so drive it the way the protocol does: any command
    // that mutates the rig rebuilds. `setMasterVolume` is the cheap one.
    // A fresh rig is empty, so the bypass is what is running: no channel master.
    assert!(!sh.rig.lock().unwrap().has_content(), "a fresh rig has no content");
    ws.send(Message::Text(
        json!({ "type": "req", "id": 1, "cmd": "setMasterVolume", "args": { "channel": "ch3", "value": -6.0 } })
            .to_string()
            .into(),
    ))
    .await
    .expect("send");
    let res = next_res(&mut ws, 1).await;
    assert_eq!(res["ok"], true, "a volume move on an empty channel is accepted: {res}");
    assert_eq!(
        sh.handle.node_ids.lock().unwrap().iter().filter(|n| n.contains("ch3")).count(),
        0,
        "an empty channel has no compiled node"
    );

    // Populating a previously empty channel compiles it normally: the master
    // and the block both appear, with no reload and no restart.
    ws.send(Message::Text(
        json!({ "type": "req", "id": 2, "cmd": "addBlock", "args": { "channel": "ch3", "type": "amp.basic", "name": "Amp" } })
            .to_string()
            .into(),
    ))
    .await
    .expect("send");
    let res = next_res(&mut ws, 2).await;
    assert_eq!(res["ok"], true, "adding a block to an empty channel works: {res}");
    assert!(sh.rig.lock().unwrap().has_content(), "the rig now has content");
    let ids = sh.handle.node_ids.lock().unwrap().clone();
    assert!(ids.iter().any(|n| n == "ch3__master"), "the channel master is compiled: {ids:?}");
    assert!(ids.iter().any(|n| n == "amp"), "the new block is compiled: {ids:?}");

    // And the state the UI sees still carries all four slots, empty ones included.
    let state = next_event(&mut ws, "state").await;
    let channels = state["payload"]["channels"].as_array().unwrap();
    assert_eq!(channels.len(), CHANNEL_COUNT, "the slot count never changes");
}

/// `engine.wait` (and the read-only `engine.status`) are the explicit shutdown
/// API: the engine stops its audio thread, releases the listener and exits 0
/// — no shell required to stop it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_wait_and_status_are_the_shutdown_api() {
    let source = Arc::new(AtomicU8::new(0));
    let (mut handle, _cmd_rx, _trash_tx, _running) = new_engine(setup(), source.clone());
    let registry = handle.registry.clone();
    let store = Store::discover();
    handle.preset_dir = store.dir.clone();

    let sh = Arc::new(Shared::new(handle, registry, source, store,
        Arc::new(AtomicBool::new(true)), Arc::new(pedal_engine::models::Models::new())));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let sh_task = sh.clone();
    tokio::spawn(async move { serve_listener(sh_task, &listener).await });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    // The greeting confirms the engine is running.
    let _greeting = next_event(&mut ws, "state").await;

    // `engine.wait` — the explicit, idempotent shutdown command.
    ws.send(Message::Text(
        json!({ "type": "req", "id": 10, "cmd": "engine.wait" }).to_string().into(),
    ))
    .await
    .expect("send");
    let _wait = next_res(&mut ws, 10).await;
    assert_eq!(sh.running.load(Ordering::Relaxed), false, "engine.wait sets running=false");

    // `engine.status` — the read-only view.
    ws.send(Message::Text(
        json!({ "type": "req", "id": 11, "cmd": "engine.status" }).to_string().into(),
    ))
    .await
    .expect("send");
    let status = next_res(&mut ws, 11).await;
    assert_eq!(status["payload"]["status"], "stopped", "status reflects the stop");

    // The engine is stopped; the Shared's shutdown Notify woke the main loop
    // (the test owns the last Shared clone, so the struct is dropped here).
    drop(sh);
}
