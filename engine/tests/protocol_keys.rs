//! Guard the wire contract between engine and UI.
//!
//! Two bugs in this repo came from key-name drift: the engine emitted
//! `input_gain` where `docs/IPC-PROTOCOL.md` and the UI's `State` interface say
//! `inputGain`, and camelCase SVG props were ignored by the browser. Neither
//! fails to compile -- both silently do nothing. This test pins the exact key
//! set so a rename on either side fails here instead of in a user's UI.
//!
//! Boots the real binary on an ephemeral port, like `tuner.rs`.

use std::net::{SocketAddr, TcpListener};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

const BIN: &str = env!("CARGO_BIN_EXE_pedal-engine");

fn free_port() -> u16 {
    let probe = TcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = probe.local_addr().expect("probe addr").port();
    drop(probe);
    port
}

async fn state_event(port: u16) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let (mut ws, _) = loop {
        if let Ok(conn) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}")).await {
            break conn;
        }
        assert!(Instant::now() < deadline, "engine never came up on {port}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => {
                let v: Value = serde_json::from_str(&t).expect("json");
                if v["type"] == "event" && v["event"] == "state" {
                    return v["payload"].clone();
                }
            }
            Some(Ok(_)) => continue,
            other => panic!("socket closed or errored: {other:?}"),
        }
    }
}

/// Exact top-level keys of a state snapshot, per docs/IPC-PROTOCOL.md.
const STATE_KEYS: &[&str] = &[
    "rev",
    "preset",
    "presets",
    "source",
    "registry",
    "channels",
    "inputGain",
    "masterOutVolume",
    "routings",
    "bufferFrames",
    "sampleRate",
    "oneWayLatencyMs",
    "roundTripLatencyMs",
];

/// Exact keys of one block entry.
const BLOCK_KEYS: &[&str] = &["id", "type", "name", "bypass", "params"];

/// Exact keys of one channel entry.
const CHANNEL_KEYS: &[&str] = &["id", "name", "volume", "sources", "blocks", "muted"];

fn assert_exactly(actual: &Value, expected: &[&str], what: &str) {
    let obj = actual.as_object().unwrap_or_else(|| panic!("{what} is not an object"));
    let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
    keys.sort_unstable();
    let mut want: Vec<&str> = expected.to_vec();
    want.sort_unstable();
    assert_eq!(keys, want, "{what}: key set drifted from the protocol");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_snapshot_keys_match_the_documented_protocol() {
    let port = free_port();
    let mut child = Command::new(BIN)
        .args([
            "--headless",
            "--source",
            "silence",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pedal-engine");

    let payload = state_event(port).await;
    assert_exactly(&payload, STATE_KEYS, "state snapshot");

    // The preset has to actually carry blocks/channels for these to mean anything.
    let channels = payload["channels"].as_array().expect("channels array");
    assert!(!channels.is_empty(), "no channels in the snapshot");
    for (i, ch) in channels.iter().enumerate() {
        assert_exactly(ch, CHANNEL_KEYS, &format!("channels[{i}]"));
        for (j, b) in ch["blocks"].as_array().expect("blocks array").iter().enumerate() {
            assert_exactly(b, BLOCK_KEYS, &format!("channels[{i}].blocks[{j}]"));
        }
    }

    // The two fields that drifted once already -- assert them by name so the
    // failure message names the culprit.
    assert!(payload["inputGain"].is_number(), "inputGain missing");
    assert!(payload["masterOutVolume"].is_number(), "masterOutVolume missing");

    child.kill().expect("kill engine");
}

/// `rev` must advance on a mutation that does not rebuild the graph -- that is
/// the bug that froze the knobs. `setInputGain` takes this path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rev_advances_on_param_only_mutations() {
    let port = free_port();
    let mut child = Command::new(BIN)
        .args([
            "--headless",
            "--source",
            "silence",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pedal-engine");

    let first = state_event(port).await;
    let rev0 = first["rev"].as_u64().expect("rev");

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("ws");
    ws.send(Message::Text(
        serde_json::json!({"type":"req","id":1,"cmd":"setInputGain","args":{"value":3.0}}).to_string().into(),
    ))
    .await
    .expect("send");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "no state after setInputGain");
        match ws.next().await {
            Some(Ok(Message::Text(t))) => {
                let v: Value = serde_json::from_str(&t).expect("json");
                if v["type"] == "event" && v["event"] == "state" {
                    let rev1 = v["payload"]["rev"].as_u64().expect("rev");
                    // This second connection is greeted with the current state
                    // first -- skip that frame and wait for the broadcast.
                    if rev1 > rev0 {
                        break;
                    }
                }
            }
            Some(Ok(_)) => continue,
            other => panic!("socket closed: {other:?}"),
        }
    }

    child.kill().expect("kill engine");
}
