//! Stomp test: footswitch preset up/down on the headless engine.
//! Drives `nextPreset`/`prevPreset` over a real WebSocket and asserts the
//! preset walks the sorted list with no xruns. No audio is ever played.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

const BIN: &str = env!("CARGO_BIN_EXE_pedal-engine");
const PRESETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../presets");
const TIMEOUT: Duration = Duration::from_secs(30);

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_json(ws: &mut Ws) -> Value {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => return serde_json::from_str(t.as_ref()).expect("json"),
            Some(Ok(_)) => continue,
            other => panic!("websocket closed or errored: {other:?}"),
        }
    }
}

/// Send a command; return its reply. Meters seen on the way must show 0 xruns.
async fn call(ws: &mut Ws, id: i64, cmd: &str, xruns_seen: &mut u32) -> Value {
    ws.send(Message::Text(json!({"type":"req","id":id,"cmd":cmd}).to_string().into()))
        .await
        .expect("send");
    loop {
        let v = next_json(ws).await;
        if v["type"] == "event" && v["event"] == "meters" {
            assert_eq!(v["payload"]["xruns"], 0, "xrun while stomping: {v}");
            *xruns_seen += 1;
        } else if v["type"] == "res" && v["id"] == json!(id) {
            return v;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stomping_walks_presets_without_xruns() {
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let child = Command::new(BIN)
        .args(["--headless", "--port", &port.to_string(), "--bench", "3"])
        .env("PEDAL_PRESETS", PRESETS)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");

    let deadline = Instant::now() + TIMEOUT;
    while tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err() {
        assert!(Instant::now() < deadline, "engine never listened");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    let state = loop {
        let v = next_json(&mut ws).await;
        if v["type"] == "event" && v["event"] == "state" {
            break v;
        }
    };
    assert_eq!(state["payload"]["preset"], "04-rainbow");

    let mut xs = 0u32;
    let mut id = 0i64;
    let mut last_rev = state["payload"]["rev"].as_u64().unwrap_or(0);
    let walk = |expect: &str, res: &Value, last_rev: &mut u64| {
        assert_eq!(res["ok"], true, "refused: {res}");
        assert_eq!(res["payload"]["preset"], expect, "wrong preset: {res}");
        let rev = res["payload"]["rev"].as_u64().unwrap_or(0);
        assert!(rev >= *last_rev, "rev went backwards");
        *last_rev = rev;
    };

    // Up from 04: 05, 06, 07, then clamped at 07 (no-op).
    for exp in ["05-endless-rise", "06-nam-amp", "07-nam-od808", "07-nam-od808", "07-nam-od808"] {
        id += 1;
        let r = call(&mut ws, id, "nextPreset", &mut xs).await;
        walk(exp, &r, &mut last_rev);
    }
    // All the way down: 06 ... 00, then clamped at 00.
    let down = ["06-nam-amp", "05-endless-rise", "04-rainbow", "03-two-amps", "02-crunch", "01-clean", "00-blank", "00-blank"];
    for exp in down {
        id += 1;
        let r = call(&mut ws, id, "prevPreset", &mut xs).await;
        walk(exp, &r, &mut last_rev);
    }
    // 10 up and 10 down in a row from 00.
    for _ in 0..10 {
        id += 1;
        let r = call(&mut ws, id, "nextPreset", &mut xs).await;
        assert_eq!(r["ok"], true);
    }
    for _ in 0..10 {
        id += 1;
        let r = call(&mut ws, id, "prevPreset", &mut xs).await;
        assert_eq!(r["ok"], true);
    }
    id += 1;
    let r = call(&mut ws, id, "getState", &mut xs).await;
    assert_eq!(r["payload"]["preset"], "00-blank");

    let out = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    let mut log = String::from_utf8_lossy(&out.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "engine exited {:?}\n{log}", out.status);
    let done = log.lines().find(|l| l.contains("bench done")).expect("bench done");
    let xr = done.split_whitespace().find(|w| w.starts_with("xruns=")).unwrap();
    assert_eq!(xr, "xruns=0", "{done}");
    println!("stomp presets: {done} (meters events checked: {xs})");
}
