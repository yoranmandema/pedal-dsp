//! US-2.10 — the UI never touches the audio path.
//!
//! The UI is a WebSocket client; the engine owns the audio callback. The only
//! thing that crosses that boundary is a [`Cmd`] on an SPSC queue. These tests
//! hold the real engine code to the contract:
//!
//! 1. [`killing_the_ui_does_not_disturb_audio`] runs the real callback on a
//!    simulated device, the real IPC server, and a real WebSocket client. The
//!    client is frozen and then killed, and audio must keep flowing: frames
//!    advance, xruns stay at zero, the output stays non-silent, and a second
//!    client can still connect afterwards.
//! 2. [`a_command_lands_within_one_block`] measures the worst-case
//!    UI-to-sound latency. A gate block pins the exact instant a command is
//!    issued — the middle of a block — and the command must be applied by the
//!    start of the next block.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use dsp_graph::blocks::{Gain, IoInput, IoOutput};
use dsp_graph::prelude::*;
use dsp_graph::{Executor, Graph};
use futures_util::{SinkExt, StreamExt};
use pedal_engine::audio::{
    new_engine, read_f32, run_callback, AudioThread, ChannelMode, Cmd, Handle, Meters, Ring, Setup,
};
use pedal_engine::channels::Rig;
use pedal_engine::host::{BLOCK, LATENCY_FRAMES, RING_CAPACITY};
use pedal_engine::server::{serve_listener, Shared};
use pedal_engine::store::Store;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// The engine runs at 48 kHz; one block is 64 frames = 1.33 ms.
const SR: f32 = 48_000.0;
/// Simulated device callback, frames. Wider than one block, like a real device.
const CB: usize = 64;
const CH: usize = 2;
/// Live input buffered before the output callback starts.
const PREFILL: usize = 64;
/// One device period: 256 frames at 48 kHz.
fn period() -> Duration {
    Duration::from_secs_f32(CB as f32 / SR)
}

/// Real-time pace for the simulated device.
///
/// `std::thread::sleep` is not usable for this: on the development host it
/// overshoots by an order of magnitude (a 5 ms sleep takes ~21 ms). The wait
/// is therefore a spin on the monotonic clock, which is all a device driver
/// does anyway — it wakes on a hardware timer, not on a scheduler tick.
struct Pacer {
    next: Instant,
    period: Duration,
}

impl Pacer {
    fn new(period: Duration) -> Self {
        Self { next: Instant::now() + period, period }
    }

    fn wait(&mut self) {
        self.next += self.period;
        let now = Instant::now();
        if self.next < now {
            // The callback overran the period: resync rather than run flat out.
            self.next = now + self.period;
            return;
        }
        while Instant::now() < self.next {
            std::hint::spin_loop();
        }
    }
}

fn setup() -> Setup {
    Setup {
        sample_rate: SR,
        out_channels: CH,
        block: BLOCK,
        channel_mode: ChannelMode::First(2),
        latency_frames: LATENCY_FRAMES,
    }
}

/// Build the rig, compile it, and install it exactly as `server::rebuild` does.
fn compile_rig(handle: &Handle, rig: &Rig) {
    let mut g = rig
        .to_doc()
        .build_in(&handle.registry, &handle.preset_dir)
        .expect("rig builds");
    let ex = Executor::compile_owned(&mut g, handle.sample_rate, BLOCK).expect("rig compiles");
    *handle.node_ids.lock().unwrap() = ex.node_ids().to_vec();
    handle.push(Cmd::Swap(ex, Vec::new())).expect("swap");
}

/// A pedalboard with a recognisable knob on it.
fn test_rig() -> Rig {
    let mut rig = Rig::new("US-2.10");
    rig.add_block_as("ch1", "amp.basic", "Amp", [0.0, 0.0]);
    rig.add_block_as("ch1", "cab.sim", "Cab", [0.0, 0.0]);
    rig.add_block_as("ch1", "time.delay", "Delay", [0.0, 0.0]);
    rig
}

/// The input callback: one buffer of guitar per device period.
fn spawn_input(ring: Arc<Ring>, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut pacer = Pacer::new(period());
        let mut phase = 0.0f32;
        let step = 2.0 * std::f32::consts::PI * 220.0 / SR;
        while !stop.load(Ordering::Relaxed) {
            if ring.available() > RING_CAPACITY - CB {
                // The consumer has fallen behind; wait for room. Never
                // overwrite unread samples.
                std::thread::yield_now();
                continue;
            }
            for _ in 0..CB {
                ring.push((phase * step).sin() * 0.5);
                phase += 1.0;
            }
            pacer.wait();
        }
    });
}

/// The output callback: the real [`run_callback`], paced like a device.
/// `last_peak` carries the most recent callback's peak out to the test.
fn spawn_output(
    meters: Arc<Meters>,
    cmd_rx: rtrb::Consumer<Cmd>,
    trash_tx: rtrb::Producer<Executor>,
    running: Arc<AtomicBool>,
    source: Arc<AtomicU8>,
    ring: Arc<Ring>,
    last_peak: Arc<AtomicU32>,
) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    std::thread::spawn(move || {
        let mut engine =
            AudioThread::new(setup(), meters.clone(), cmd_rx, trash_tx, running, source);
        engine.set_block(BLOCK);
        let mut pacer = Pacer::new(period());
        let mut buf = vec![0.0f32; CB * CH];
        while !stop_t.load(Ordering::Relaxed) {
            run_callback(&mut engine, &ring, &meters, &mut buf, CH);
            let peak = buf.iter().fold(0.0f32, |p, &v| p.max(v.abs()));
            last_peak.store(peak.to_bits(), Ordering::Relaxed);
            pacer.wait();
        }
    });
    stop
}

/// The next text frame, skipping ping/pong/close.
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

/// The next `event` frame of the given kind. Meters arrive on their own timer
/// and can overtake a reply.
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

/// The reply to the request with this id.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killing_the_ui_does_not_disturb_audio() {
    // Live input, so an underrun is possible and `xruns` means something.
    let source = Arc::new(AtomicU8::new(1));
    let (mut handle, cmd_rx, trash_tx, running) = new_engine(setup(), source.clone());
    let meters = handle.meters.clone();
    let registry = handle.registry.clone();
    let store = Store::discover();
    handle.preset_dir = store.dir.clone();
    let rig = test_rig();
    compile_rig(&handle, &rig);

    // Simulated device: an input callback that feeds the ring, an output
    // callback that drains it through the real engine.
    let ring = Arc::new(Ring::new(RING_CAPACITY));
    for i in 0..PREFILL {
        ring.push(((i as f32) * 0.01).sin() * 0.3);
    }
    handle.set_source(1);
    let stop_input = Arc::new(AtomicBool::new(false));
    spawn_input(ring.clone(), stop_input.clone());
    let last_peak = Arc::new(AtomicU32::new(0));
    let stop_output = spawn_output(
        meters.clone(),
        cmd_rx,
        trash_tx,
        running,
        source.clone(),
        ring,
        last_peak.clone(),
    );

    // The real server, on an ephemeral port (8737 may be held by a stale
    // engine from an earlier session).
    let sh = Arc::new(Shared::new(handle, registry, source, store,
        Arc::new(AtomicBool::new(true)), Arc::new(pedal_engine::models::Models::new())));
    *sh.rig.lock().unwrap() = rig.clone();
    *sh.preset.lock().unwrap() = "us-2.10".into();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let sh_task = sh.clone();
    tokio::spawn(async move { serve_listener(sh_task, &listener).await });

    // The UI: a real WebSocket client.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    let greeting = next_event(&mut ws, "state").await;
    assert_eq!(greeting["payload"]["preset"], "us-2.10");

    // The UI drives a knob before it dies.
    ws.send(Message::Text(
        json!({
            "type": "req", "id": 1, "cmd": "setParam",
            "args": {"channel": "ch1", "block": "amp", "param": "gain", "value": 7.0}
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("send");
    let res = next_res(&mut ws, 1).await;
    assert_eq!(res["ok"], true, "setParam was refused: {res}");

    // Let the rig settle with the UI attached.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // --- Frozen UI: attached, but silent -----------------------------------
    let frames_before = meters.out_frames.load(Ordering::Relaxed);
    let xruns_before = meters.xruns.load(Ordering::Relaxed);
    let t = Instant::now();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let elapsed = t.elapsed();
    let advanced = meters.out_frames.load(Ordering::Relaxed) - frames_before;
    let expected = (SR * elapsed.as_secs_f32()) as usize;
    assert!(
        advanced > expected / 2,
        "a frozen UI stalled audio: {advanced} frames in {elapsed:?} (expected ~{expected})"
    );
    assert_eq!(
        meters.xruns.load(Ordering::Relaxed),
        xruns_before,
        "a frozen UI produced xruns"
    );
    assert!(read_f32(&last_peak) > 0.0, "a frozen UI silenced audio");

    // --- Dead UI: the socket is gone --------------------------------------
    drop(ws);
    let frames_at_kill = meters.out_frames.load(Ordering::Relaxed);
    let xruns_at_kill = meters.xruns.load(Ordering::Relaxed);
    let t = Instant::now();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let elapsed = t.elapsed();
    let frames_after_kill = meters.out_frames.load(Ordering::Relaxed);
    let advanced = frames_after_kill - frames_at_kill;
    let expected = (SR * elapsed.as_secs_f32()) as usize;
    assert!(
        advanced > expected / 2,
        "audio ran slow after the UI was killed: {advanced} frames in {elapsed:?} (expected ~{expected})"
    );
    assert_eq!(
        meters.xruns.load(Ordering::Relaxed),
        xruns_at_kill,
        "killing the UI produced xruns"
    );
    assert!(read_f32(&last_peak) > 0.0, "audio went silent after the UI was killed");

    // --- The engine outlived the UI ---------------------------------------
    let (mut ws2, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("reconnect");
    let greeting2 = next_event(&mut ws2, "state").await;
    // The knob the dead UI turned is still where it left it.
    assert_eq!(greeting2["payload"]["channels"][0]["blocks"][0]["params"]["gain"], 7.0);

    stop_input.store(true, Ordering::Relaxed);
    stop_output.store(true, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Latency
// ---------------------------------------------------------------------------

/// A block that stops the audio thread inside a block, so the test can issue a
/// command at a known point in the middle of the block. Test-only.
struct Gate {
    armed: bool,
}

impl Default for Gate {
    fn default() -> Self {
        Self { armed: true }
    }
}

static GATE_ENTERED: AtomicBool = AtomicBool::new(false);
static GATE_RELEASE: AtomicBool = AtomicBool::new(false);

impl Block for Gate {
    fn spec(&self) -> BlockSpec {
        BlockSpec {
            id: "test.gate",
            name: "Gate",
            category: Category::Utility,
            inputs: vec![PortSpec::audio("in")],
            outputs: vec![PortSpec::audio("out")],
            params: vec![],
            controls_in: vec![],
            controls_out: vec![],
        }
    }
    fn prepare(&mut self, _: f32, _: usize) -> Result<(), BlockError> {
        Ok(())
    }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        if self.armed {
            self.armed = false;
            GATE_ENTERED.store(true, Ordering::SeqCst);
            while !GATE_RELEASE.load(Ordering::SeqCst) {
                std::hint::spin_loop();
            }
        }
        for i in 0..ctx.n {
            ctx.outputs[0][i] = ctx.inputs[0][i];
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.armed = true;
    }
}

#[test]
fn a_command_lands_within_one_block() {
    let source = Arc::new(AtomicU8::new(0)); // the riff: no device needed
    let (handle, cmd_rx, trash_tx, running) = new_engine(setup(), source.clone());
    let meters = handle.meters.clone();

    // in -> gain -> gate -> out. The gate sits *after* the gain, so the block
    // it stops on has already been rendered with the old parameter value.
    let mut g = Graph::new();
    g.add("in", Box::new(IoInput)).unwrap();
    g.add("gain", Box::new(Gain::default())).unwrap();
    g.add("gate", Box::new(Gate::default())).unwrap();
    g.add("out", Box::new(IoOutput)).unwrap();
    g.connect("in", "out", "gain", "in").unwrap();
    g.connect("gain", "out", "gate", "in").unwrap();
    g.connect("gate", "out", "out", "in").unwrap();
    let ex = Executor::compile_owned(&mut g, SR, BLOCK).unwrap();
    let gain_node = ex.node_ids().iter().position(|n| n == "gain").expect("gain node");

    GATE_ENTERED.store(false, Ordering::SeqCst);
    GATE_RELEASE.store(false, Ordering::SeqCst);

    // The audio thread runs exactly one device callback of two blocks and
    // hands the rendered buffer back.
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let audio = std::thread::spawn(move || {
        let mut engine =
            AudioThread::new(setup(), meters.clone(), cmd_rx, trash_tx, running, source);
        engine.set_block(BLOCK);
        engine.set_executor(ex);
        let ring = Ring::new(64);
        let mut buf = vec![0.0f32; 2 * BLOCK * CH];
        run_callback(&mut engine, &ring, &meters, &mut buf, CH);
        let _ = tx.send(buf);
    });

    // Wait until the audio thread is inside block 1, then issue the command.
    while !GATE_ENTERED.load(Ordering::SeqCst) {
        std::hint::spin_loop();
    }
    handle
        .push(Cmd::SetParam { node: gain_node, param: 0, value: -60.0 })
        .expect("push");
    GATE_RELEASE.store(true, Ordering::SeqCst);

    let buf = rx.recv().expect("audio thread produced no buffer");
    audio.join().expect("audio thread panicked");

    // The command was issued during block 1. Block 1 must still carry the old
    // gain (0 dB); block 2 must already carry the new one (-60 dB).
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |p, &v| p.max(v.abs()));
    let old = peak(&buf[..BLOCK * CH]);
    let new = peak(&buf[BLOCK * CH..]);
    assert!(old > 1.0e-4, "block 1 is silent ({old}); the riff is not flowing");
    assert!(
        new < old * 0.01,
        "the command was not applied by block 2 (old peak {old}, new peak {new})"
    );

    // Measure the latency from the buffer: the last frame whose window still
    // carries the old level. The command was issued inside block 1, so the
    // answer must be no more than one block later.
    let window = 16;
    let threshold = old * 0.1;
    let mut last_old = 0usize;
    for i in 0..(2 * BLOCK - window) {
        if peak(&buf[i * CH..(i + window) * CH]) > threshold {
            last_old = i + window;
        }
    }
    assert!(
        last_old <= BLOCK + window,
        "audio kept the old value until frame {last_old}, more than one block ({BLOCK}) after the command"
    );
    // One block is 64 frames = 1.33 ms at 48 kHz. That is the worst case: the
    // audio thread drains the command queue once per `render`, and `render`
    // never handles more than one block, so at most one block of audio can be
    // rendered with the old value after a command is issued.
    assert_eq!(BLOCK, 64, "one block is 64 frames");
}
