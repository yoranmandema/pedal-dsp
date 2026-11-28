//! The IPC server: a loopback WebSocket speaking `docs/IPC-PROTOCOL.md`.
//!
//! One [`Shared`] is owned by the process. WebSocket tasks mutate it behind
//! mutexes and push [`Cmd`]s to the audio thread, exactly as the egui app's UI
//! thread did. The audio thread never touches the WebSocket.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::Message;

use dsp_graph::{Doc, EdgeDoc, Executor, Graph, NodeDoc, Registry, RoutingEndpoint};

use crate::audio::{read_f32, Cmd, Handle};
use crate::channels::{master_id, Rig, OUT_MASTER_ID, IN};
use crate::host::{DEFAULT_IO_BUFFER_FRAMES, IOBUFFER_FALLBACK, one_way_latency_ms, round_trip_latency_ms};
use crate::models::Models;
use crate::store::{slugify, Store};

/// Loopback only. Nothing outside this machine can reach the engine.
pub const PORT: u16 = 8737;
/// Meters are pushed at this interval.
const METERS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
/// Tuner readings are pushed at this interval (20 Hz).
const TUNER_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

/// Everything the protocol can touch.
pub struct Shared {
    pub rig: Mutex<Rig>,
    pub store: Mutex<Store>,
    pub handle: Handle,
    pub registry: Arc<Registry>,
    /// 0 = riff, 1 = live input, 2 = silence.
    pub source: Arc<AtomicU8>,
    /// Current preset name, `""` when the rig has never been saved.
    pub preset: Mutex<String>,
    /// The I/O buffer size in frames (the low-latency default is 128; the
    /// fallback ladder is [128, 256, 512]).
    pub buffer_frames: Mutex<u32>,
    /// Sample rate in Hz, `None` keeps the device default.
    pub sample_rate: Mutex<Option<u32>>,
    pub clients: Mutex<Vec<mpsc::UnboundedSender<String>>>,
    /// The audio thread's running flag. `false` starts the shutdown sequence.
    pub running: Arc<AtomicBool>,
    /// Woken when `engine.wait` is received (via `apply`), so the main loop
    /// drops the streams and listener and returns. Set in `Engine::new`.
    pub shutdown: Notify,
    /// Cross-channel routings.
    /// The bypass graph for a rig with no blocks in any slot. Built once and
    /// recompiled (cheaply) whenever an empty pedalboard needs an executor, so
    /// an empty rig never builds a `Doc` or walks the block registry. See
    /// [`rebuild`].
    bypass: Mutex<Option<Graph>>,
    /// Bumped on every successful rebuild. The UI uses it to ignore a stale
    /// snapshot, so an out-of-order delivery can never roll the UI backwards.
    pub rev: AtomicU64,
    /// The model-browsing service: the cached index and the `listModels`/
    /// `getModel`/`refreshIndex`/`downloadModel`/`cancelDownload` commands.
    pub models: Arc<Models>,
}

impl Shared {
    pub fn new(handle: Handle, registry: Arc<Registry>, source: Arc<AtomicU8>, store: Store, running: Arc<AtomicBool>, models: Arc<Models>) -> Self {
        Self {
            rig: Mutex::new(Rig::default()),
            store: Mutex::new(store),
            handle,
            registry,
            source,
            preset: Mutex::new(String::new()),
            buffer_frames: Mutex::new(DEFAULT_IO_BUFFER_FRAMES),
            sample_rate: Mutex::new(None),
            clients: Mutex::new(Vec::new()),
            running,
            shutdown: Notify::new(),
            bypass: Mutex::new(None),
            rev: AtomicU64::new(0),
            models,
        }
    }

    /// Preset names, sorted, for the state snapshot and `listPresets`.
    fn presets(&self) -> Vec<String> {
        self.store.lock().unwrap().presets.iter().map(|(n, _)| n.clone()).collect()
    }

    fn source_name(&self) -> &'static str {
        match self.source.load(Ordering::Relaxed) {
            1 => "live",
            2 => "silence",
            _ => "riff",
        }
    }
}

// ---------------------------------------------------------------------------
// State snapshot
// ---------------------------------------------------------------------------

fn registry_json(reg: &Registry) -> Value {
    let mut out = BTreeMap::new();
    for spec in reg.specs() {
        let params: Vec<Value> = spec
            .params
            .iter()
            .map(|p| {
                let mut o = json!({
                    "id": p.id,
                    "name": p.name,
                    "min": p.min,
                    "max": p.max,
                    "default": p.default,
                    "unit": p.unit.to_string(),
                });
                if let Some(choices) = p.choices() {
                    o["kind"] = json!("choice");
                    o["choices"] = json!(choices);
                } else {
                    o["kind"] = json!("number");
                }
                o
            })
            .collect();
        out.insert(
            spec.id.to_string(),
            json!({
                "name": spec.name,
                "category": spec.category.to_string(),
                "params": params,
            }),
        );
    }
    Value::Object(out.into_iter().collect())
}

fn snapshot(sh: &Shared) -> Value {
    let rig = sh.rig.lock().unwrap();
    let channels: Vec<Value> = rig
        .channels
        .iter()
        .map(|c| {
            let blocks: Vec<Value> = c
                .blocks
                .iter()
                .map(|b| {
                    let mut params = serde_json::Map::new();
                    // Fill in defaults for anything the preset did not set, so
                    // the UI never has to merge.
                    if let Some(spec) = sh.registry.get(&b.block) {
                        for p in &spec.params {
                            let v = b.params.get(p.id).copied().unwrap_or(p.default);
                            params.insert(p.id.to_string(), json!(v));
                        }
                    }
                    if b.block == "dist.waveshaper" {
                        params.insert(
                            "curve".to_string(),
                            b.curve.clone().unwrap_or_else(|| dsp_graph::waveshaper::TransferCurve::identity().to_json()),
                        );
                    }
                    json!({
                        "id": b.id,
                        "type": b.block,
                        "name": sh.registry.get(&b.block).map(|s| s.name).unwrap_or(&b.block),
                        "bypass": b.bypass,
                        "params": Value::Object(params),
                    })
                })
                .collect();
            json!({
                "id": c.id,
                "name": c.name,
                "volume": c.master_volume,
                "muted": c.muted,
                "sources": c.sources,
                "blocks": blocks,
            })
        })
        .collect();
    let routings = serde_json::to_value(&rig.routings).unwrap_or_else(|_| json!([]));
    let preset = sh.preset.lock().unwrap().clone();
    let rev = sh.rev.load(Ordering::Acquire);
    let sample_rate = sh.sample_rate.lock().unwrap().unwrap_or(48_000);
    let one_way_ms = one_way_latency_ms(*sh.buffer_frames.lock().unwrap(), sample_rate);
    let round_trip_ms = round_trip_latency_ms(*sh.buffer_frames.lock().unwrap(), sample_rate);
    json!({
        "rev": rev,
        "preset": preset,
        "presets": sh.presets(),
        "source": sh.source_name(),
        "registry": registry_json(&sh.registry),
        "channels": channels,
        "inputGain": rig.input_gain,
        "masterOutVolume": rig.master_volume,
        "routings": routings,
        "bufferFrames": *sh.buffer_frames.lock().unwrap(),
        "sampleRate": sample_rate,
        "oneWayLatencyMs": (one_way_ms * 100.0).round() / 100.0,
        "roundTripLatencyMs": (round_trip_ms * 100.0).round() / 100.0,
    })
}

fn state_event(sh: &Shared) -> String {
    json!({ "type": "event", "event": "state", "payload": snapshot(sh) }).to_string()
}

/// Push a state snapshot to every client except `skip` (the one that asked).
fn send_state(sh: &Shared, skip: Option<&mpsc::UnboundedSender<String>>) {
    broadcast(sh, state_event(sh), skip);
}

fn broadcast(sh: &Shared, text: String, skip: Option<&mpsc::UnboundedSender<String>>) {
    let mut clients = sh.clients.lock().unwrap();
    clients.retain(|c| {
        if skip.map(|s| s.same_channel(c)).unwrap_or(false) {
            return true;
        }
        c.send(text.clone()).is_ok()
    });
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

/// The pedal's global output limiter, as a node. Shared by every command
/// handler that has no rig left to compile.
fn out_master() -> NodeDoc {
    NodeDoc {
        id: OUT_MASTER_ID.into(),
        block: "util.master".into(),
        params: [("volume".to_string(), 0.0), ("ceiling".to_string(), -1.0)].into_iter().collect(),
        bypass: false,
        pos: Some([1320.0, 400.0]),
        model: None, curve: None,
    }
}

/// The graph a rig with no blocks at all still has to run.
///
/// A pedalboard with nothing in it is a *bypass*, not a dead pedal: the input
/// feeds the output limiter directly, so the pedal passes audio until the user
/// drops a block in. [`Rig::to_doc`] builds the same shape for a loaded preset.
fn bypass_doc() -> Doc {
    let mut doc = Doc::new(String::new());
    doc.nodes.push(NodeDoc {
        id: "in".into(),
        block: "io.input".into(),
        params: BTreeMap::new(),
        bypass: false,
        pos: Some([60.0, 400.0]),
        model: None, curve: None,
    });
    doc.nodes.push(NodeDoc {
        id: "out".into(),
        block: "io.output".into(),
        params: BTreeMap::new(),
        bypass: false,
        pos: Some([1400.0, 400.0]),
        model: None, curve: None,
    });
    doc.nodes.push(out_master());
    doc.edges.push(EdgeDoc {
        from: ["in".to_string(), "out".into()],
        to: ["out__master".to_string(), "in".into()],
    });
    doc.edges.push(EdgeDoc {
        from: ["out__master".to_string(), "out".into()],
        to: ["out".to_string(), "in".into()],
    });
    doc
}

/// Compile the bypass graph once, at startup, and hold it.
///
/// The empty-rig fast path. Building a `Doc` and compiling it costs real time
/// on the UI thread (every block is instantiated and prepared), so a rig of
/// only empty channel slots must not pay it — and must not pay it again on
/// every volume slider move. [`Shared::bypass`] holds the built graph;
/// `rebuild` recompiles that tiny graph (three blocks) and hands the result to
/// the audio thread. The moment content appears, `rebuild` compiles the rig's
/// own graph instead, so an empty channel that gets a block resumes normal
/// processing without a reload.
fn bypass_graph(sh: &Shared) -> Result<Graph, String> {
    let doc = bypass_doc();
    doc.build_in(&sh.registry, &sh.handle.preset_dir)
        .map_err(|e| e.to_string())
}

/// Rebuild the executor from the rig and hand it to the audio thread.
///
/// An empty rig recompiles the cached bypass graph (cheap: three blocks, no
/// instantiating a palette); any other rig compiles its own. That fast path is
/// the point — while the pedalboard is empty, a rebuild does not walk the block
/// registry or build a `Doc`, so the volume sliders stay responsive.
fn rebuild(sh: &Shared) -> Result<(), String> {
    let ex = if sh.rig.lock().unwrap().has_content() {
        compile(sh)?
    } else {
        let mut guard = sh.bypass.lock().unwrap();
        if guard.is_none() {
            *guard = Some(bypass_graph(sh)?);
        }
        let g = guard.as_mut().expect("just built");
        Executor::compile_borrowed(g, &sh.registry, sh.handle.sample_rate, sh.handle.block())
            .map_err(|e| e.to_string())?
    };
    let ids = ex.node_ids().to_vec();
    let bypass: Vec<(String, bool)> = sh
        .rig
        .lock()
        .unwrap()
        .channels
        .iter()
        .flat_map(|c| c.blocks.iter().filter(|b| b.bypass).map(|b| (b.id.clone(), true)))
        .collect();
    sh.handle.push(Cmd::Swap(ex, bypass)).map_err(|e| e.to_string())?;
    *sh.handle.node_ids.lock().unwrap() = ids;
    *sh.handle.rate_mismatches.lock().unwrap() = Vec::new();
    sh.rev.fetch_add(1, Ordering::Release);
    Ok(())
}

/// Compile the rig's graph into a fresh executor.
fn compile(sh: &Shared) -> Result<Executor, String> {
    let doc = sh.rig.lock().unwrap().to_doc();
    // Index ids resolve through the cached model index; anything else is a
    // legacy path, exactly as before.
    let resolver = crate::model_index::resolver();
    let mut g = doc
        .build_in_with(&sh.registry, &sh.handle.preset_dir, &resolver)
        .map_err(|e| e.to_string())?;
    // Routings are realised here, at compile time; `to_doc` only carries them.
    sh.rig.lock().unwrap().apply_routings(&mut g, &sh.registry)?;
    Executor::compile_owned(&mut g, sh.handle.sample_rate, sh.handle.block())
        .map_err(|e| e.to_string())
}

fn set_param(sh: &Shared, channel: &str, block: &str, param: &str, value: f32) -> Result<(), String> {
    let (nidx, pidx) = {
        let rig = sh.rig.lock().unwrap();
        let ch = rig.channel(channel).ok_or_else(|| format!("no such channel: {channel}"))?;
        let node = ch.blocks.iter().find(|b| b.id == block).ok_or_else(|| format!("no such block: {block}"))?;
        let spec = sh.registry.get(&node.block).ok_or_else(|| format!("unknown block type: {}", node.block))?;
        let pidx = spec.params.iter().position(|p| p.id == param).ok_or_else(|| format!("no such param: {param}"))?;
        (sh.handle.node_index(block), pidx)
    };
    let nidx = nidx.ok_or_else(|| format!("block not in the compiled graph: {block}"))?;
    {
        let mut rig = sh.rig.lock().unwrap();
        let ch = rig.channel_mut(channel).unwrap();
        let node = ch.blocks.iter_mut().find(|b| b.id == block).unwrap();
        node.params.insert(param.to_string(), value);
    }
    sh.handle.push(Cmd::SetParam { node: nidx, param: pidx, value }).map_err(|e| e.to_string())?;
    Ok(())
}

/// `setParam` with the composite param `"curve"` (waveshaper transfer curve).
/// Validated strictly *before* any state changes; the compiled graph is then
/// rebuilt so the new table is baked off the audio thread and swapped in.
fn set_curve(sh: &Shared, channel: &str, block: &str, value: &Value) -> Result<(), String> {
    {
        let rig = sh.rig.lock().unwrap();
        let ch = rig.channel(channel).ok_or_else(|| format!("no such channel: {channel}"))?;
        let node = ch.blocks.iter().find(|b| b.id == block).ok_or_else(|| format!("no such block: {block}"))?;
        if node.block != "dist.waveshaper" {
            return Err(format!("block {block} has no curve param"));
        }
    }
    dsp_graph::waveshaper::TransferCurve::from_json(value)?;
    let prev = {
        let mut rig = sh.rig.lock().unwrap();
        let ch = rig.channel_mut(channel).unwrap();
        let node = ch.blocks.iter_mut().find(|b| b.id == block).unwrap();
        node.curve.replace(value.clone())
    };
    if let Err(e) = rebuild(sh) {
        let mut rig = sh.rig.lock().unwrap();
        let ch = rig.channel_mut(channel).unwrap();
        ch.blocks.iter_mut().find(|b| b.id == block).unwrap().curve = prev;
        return Err(e);
    }
    Ok(())
}

fn set_master_volume(sh: &Shared, channel: &str, value: f32) -> Result<(), String> {
    let nidx = {
        let mut rig = sh.rig.lock().unwrap();
        let ch = rig.channel_mut(channel).ok_or_else(|| format!("no such channel: {channel}"))?;
        ch.master_volume = value;
        sh.handle.node_index(&master_id(channel))
    };
    if let Some(nidx) = nidx {
        sh.handle.push(Cmd::SetParam { node: nidx, param: 0, value }).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn set_input_gain(sh: &Shared, value: f32) -> Result<(), String> {
    let value = value.clamp(-60.0, 24.0);
    let nidx = {
        let mut rig = sh.rig.lock().unwrap();
        rig.input_gain = value;
        sh.handle.node_index("in")
    };
    if let Some(nidx) = nidx {
        sh.handle.push(Cmd::SetParam { node: nidx, param: 0, value }).map_err(|e| e.to_string())?;
    }
    // The cached bypass graph was built with the old trim; drop it so the next
    // rebuild picks up the new value.
    *sh.bypass.lock().unwrap() = None;
    Ok(())
}

fn set_master_out_volume(sh: &Shared, value: f32) -> Result<(), String> {
    let value = value.clamp(-60.0, 12.0);
    let nidx = {
        let mut rig = sh.rig.lock().unwrap();
        rig.master_volume = value;
        sh.handle.node_index(OUT_MASTER_ID)
    };
    if let Some(nidx) = nidx {
        sh.handle.push(Cmd::SetParam { node: nidx, param: 0, value }).map_err(|e| e.to_string())?;
    }
    // The cached bypass graph was built with the old volume; drop it so the
    // next rebuild picks up the new value.
    *sh.bypass.lock().unwrap() = None;
    Ok(())
}

fn set_bypass(sh: &Shared, channel: &str, block: &str, on: bool) -> Result<(), String> {
    let nidx = {
        let mut rig = sh.rig.lock().unwrap();
        let ch = rig.channel_mut(channel).ok_or_else(|| format!("no such channel: {channel}"))?;
        let node = ch.blocks.iter_mut().find(|b| b.id == block).ok_or_else(|| format!("no such block: {block}"))?;
        node.bypass = on;
        sh.handle.node_index(block)
    };
    if let Some(nidx) = nidx {
        sh.handle.push(Cmd::SetBypass { node: nidx, on }).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn add_block(sh: &Shared, channel: &str, btype: &str, name: &str) -> Result<(), String> {
    if sh.registry.get(btype).is_none() {
        return Err(format!("unknown block type: {btype}"));
    }
    {
        let mut rig = sh.rig.lock().unwrap();
        if rig.channel(channel).is_none() {
            return Err(format!("no such channel: {channel}"));
        }
        // Placeholder position; the UI does not use it.
        rig.add_block_as(channel, btype, name, [0.0, 0.0]);
    }
    rebuild(sh)
}

fn remove_block(sh: &Shared, channel: &str, block: &str) -> Result<(), String> {
    {
        let mut rig = sh.rig.lock().unwrap();
        rig.remove_block(channel, block);
    }
    rebuild(sh)
}

fn move_block(sh: &Shared, channel: &str, block: &str, to: usize) -> Result<(), String> {
    {
        let mut rig = sh.rig.lock().unwrap();
        rig.move_block(channel, block, to);
    }
    rebuild(sh)
}

fn create_routing(sh: &Shared, source: RoutingEndpoint, dest: RoutingEndpoint) -> Result<(), String> {
    {
        let mut rig = sh.rig.lock().unwrap();
        rig.create_routing(&source, &dest)?;
    }
    rebuild(sh)
}

fn remove_routing(sh: &Shared, id: &str) -> Result<(), String> {
    {
        let mut rig = sh.rig.lock().unwrap();
        if !rig.remove_routing(id) {
            return Err(format!("no such routing: {id}"));
        }
    }
    rebuild(sh)
}

fn set_routing_enabled(sh: &Shared, id: &str, enabled: bool) -> Result<(), String> {
    {
        let mut rig = sh.rig.lock().unwrap();
        rig.set_enabled(id, enabled)?;
    }
    rebuild(sh)
}

fn set_routing_gain(sh: &Shared, id: &str, gain: f32) -> Result<(), String> {
    {
        let mut rig = sh.rig.lock().unwrap();
        rig.set_gain(id, gain)?;
    }
    rebuild(sh)
}

fn list_routings(sh: &Shared) -> Value {
    let rig = sh.rig.lock().unwrap();
    json!({ "routings": serde_json::to_value(&rig.routings).unwrap_or_else(|_| json!([])) })
}

/// Parse `{ "channel": "...", "block": "..." }` from a command's args.
fn endpoint(args: &Value, key: &str) -> Result<RoutingEndpoint, String> {
    let o = args.get(key).ok_or_else(|| key.to_string())?;
    let channel = o.get("channel").and_then(|v| v.as_str()).ok_or("channel")?;
    let block = o.get("block").and_then(|v| v.as_str()).ok_or("block")?;
    Ok(RoutingEndpoint::new(channel, block))
}

fn set_sources(sh: &Shared, channel: &str, sources: Vec<String>) -> Result<(), String> {
    {
        let mut rig = sh.rig.lock().unwrap();
        let ch = rig.channel_mut(channel).ok_or_else(|| format!("no such channel: {channel}"))?;
        ch.sources = if sources.is_empty() { vec![IN.to_string()] } else { sources };
    }
    rebuild(sh)
}

fn set_muted(sh: &Shared, channel: &str, muted: bool) -> Result<(), String> {
    let mut rig = sh.rig.lock().unwrap();
    rig.channel_mut(channel)
        .ok_or_else(|| format!("no such channel: {channel}"))?;
    rig.set_muted(channel, muted)
        .map_err(|e| format!("set_mute: {e}"))?;
    Ok(())
}

fn load_preset(sh: &Shared, name: &str) -> Result<(), String> {
    let doc = {
        let store = sh.store.lock().unwrap();
        store.presets.iter().find(|(n, _)| n == name).map(|(_, d)| d.clone()).ok_or_else(|| format!("no such preset: {name}"))?
    };
    let rig = Rig::from_doc(&doc);
    {
        let mut r = sh.rig.lock().unwrap();
        *r = rig;
    }
    *sh.preset.lock().unwrap() = name.to_string();
    rebuild(sh)
}

fn save_preset(sh: &Shared, name: &str) -> Result<(), String> {
    let doc = sh.rig.lock().unwrap().to_doc();
    {
        let mut store = sh.store.lock().unwrap();
        store.save(name, &doc).map_err(|e| e.to_string())?;
        store.reload();
    }
    *sh.preset.lock().unwrap() = slugify(name);
    Ok(())
}

fn delete_preset(sh: &Shared, name: &str) -> Result<(), String> {
    let path = {
        let store = sh.store.lock().unwrap();
        store.dir.join(format!("{name}.json"))
    };
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    {
        let mut store = sh.store.lock().unwrap();
        store.reload();
    }
    {
        let mut preset = sh.preset.lock().unwrap();
        if preset.as_str() == name {
            preset.clear();
        }
    }
    Ok(())
}

fn rename_preset(sh: &Shared, from: &str, to: &str) -> Result<(), String> {
    let doc = {
        let store = sh.store.lock().unwrap();
        store
            .presets
            .iter()
            .find(|(n, _)| n == from)
            .map(|(_, d)| d.clone())
            .ok_or_else(|| format!("no such preset: {from}"))?
    };
    let slug = slugify(to);
    if slug == from {
        return Ok(());
    }
    {
        let mut store = sh.store.lock().unwrap();
        store.write_exact(&slug, &doc)?;
        let old = store.dir.join(format!("{from}.json"));
        let _ = std::fs::remove_file(old);
        store.reload();
    }
    {
        let mut preset = sh.preset.lock().unwrap();
        if preset.as_str() == from {
            *preset = slug;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

fn apply(sh: &Shared, cmd: &str, args: &Value) -> Result<Value, String> {
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(|v| v.to_string());
    let f = |k: &str| args.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let b = |k: &str| args.get(k).and_then(|v| v.as_bool()).unwrap_or(false);

    match cmd {
        "getState" | "setParam" | "setMasterVolume" | "setBypass" | "addBlock"
        | "removeBlock" | "moveBlock" | "setSources" | "createRouting"
        | "removeRouting" | "setEnabled" | "setGain" | "setInputGain" | "setMasterOutVolume"
        | "loadPreset" | "nextPreset" | "prevPreset" | "setSource" | "savePreset" | "deletePreset" | "renamePreset" | "setMute" => {
            match cmd {
                "getState" => {},
                "setParam" => {
                    if s("param").as_deref() == Some("curve") {
                        set_curve(sh, &s("channel").ok_or("channel")?, &s("block").ok_or("block")?, args.get("value").ok_or("value")?)?;
                    } else {
                        set_param(sh, &s("channel").ok_or("channel")?, &s("block").ok_or("block")?, &s("param").ok_or("param")?, f("value"))?;
                    }
                }
                "setMasterVolume" => {
                    set_master_volume(sh, &s("channel").ok_or("channel")?, f("value"))?;
                }
                "setInputGain" => {
                    set_input_gain(sh, f("value"))?;
                }
                "setMasterOutVolume" => {
                    set_master_out_volume(sh, f("value"))?;
                }
                "setBypass" => {
                    set_bypass(sh, &s("channel").ok_or("channel")?, &s("block").ok_or("block")?, b("bypass"))?;
                }
                "addBlock" => {
                    add_block(sh, &s("channel").ok_or("channel")?, &s("type").ok_or("type")?, &s("name").unwrap_or_default())?;
                }
                "removeBlock" => {
                    remove_block(sh, &s("channel").ok_or("channel")?.as_str(), &s("block").ok_or("block")?.as_str())?;
                }
                "moveBlock" => {
                    let to = args.get("to").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    move_block(sh, &s("channel").ok_or("channel")?, &s("block").ok_or("block")?, to)?;
                }
                "setSources" => {
                    let sources: Vec<String> = args
                        .get("sources")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default();
                    set_sources(sh, &s("channel").ok_or("channel")?, sources)?;
                }
                "createRouting" => {
                    let source = endpoint(args, "source")?;
                    let dest = endpoint(args, "dest")?;
                    create_routing(sh, source, dest)?;
                }
                "removeRouting" => {
                    remove_routing(sh, &s("id").ok_or("id")?)?;
                }
                "setEnabled" => {
                    set_routing_enabled(sh, &s("id").ok_or("id")?, b("enabled"))?;
                }
                "setGain" => {
                    set_routing_gain(sh, &s("id").ok_or("id")?, f("gain"))?;
                }
                "loadPreset" => {
                    load_preset(sh, &s("name").ok_or("name")?)?;
                }
                "nextPreset" | "prevPreset" => {
                    // Footswitch stepping: neighbour in the sorted preset list;
                    // a no-op at either end. Loads via the same block-boundary swap.
                    let names = sh.presets();
                    let cur = sh.preset.lock().unwrap().clone();
                    if let Some(i) = names.iter().position(|n| *n == cur) {
                        let j = if cmd == "nextPreset" { i + 1 } else { i.wrapping_sub(1) };
                        if let Some(n) = names.get(j) {
                            load_preset(sh, n)?;
                        }
                    }
                }
                "setSource" => {
                    let v = match s("source").as_deref() {
                        Some("live") => 1u8,
                        Some("silence") => 2u8,
                        _ => 0u8,
                    };
                    sh.source.store(v, Ordering::Relaxed);
                }
                "renamePreset" => {
                    rename_preset(sh, &s("from").ok_or("from")?, &s("to").ok_or("to")?)?;
                }
                "setMute" => {
                    set_muted(sh, &s("channel").ok_or("channel")?, b("muted"))?;
                }
                _ => unreachable!(),
            }
            // Bump for EVERY mutation, not just the structural ones: `setParam`
            // and friends hand the change straight to the audio thread and never
            // call `rebuild`, so without this the snapshot's `rev` is unchanged
            // and the UI's rev guard discards the update -- knobs would move the
            // sound but not the indicator.
            if cmd != "getState" {
                sh.rev.fetch_add(1, Ordering::Release);
            }
            Ok(snapshot(sh))
        }
        "setConfig" => {
            // The I/O buffer size is the only config the UI can change at runtime.
            // The sample rate is set once at startup; changing it would require
            // rebuilding the audio streams, so it is exposed read-only.
            *sh.buffer_frames.lock().unwrap() = f("buffer_frames").clamp(32.0, 1024.0) as u32;
            Ok(Value::Null)
        }
        "getConfig" => {
            let buffer_frames = *sh.buffer_frames.lock().unwrap();
            let sample_rate = sh.sample_rate.lock().unwrap().unwrap_or(48_000);
            let one_way_ms = one_way_latency_ms(buffer_frames, sample_rate);
            let round_trip_ms = round_trip_latency_ms(buffer_frames, sample_rate);
            Ok(json!({
                "bufferFrames": buffer_frames,
                "sampleRate": sample_rate,
                "supported": IOBUFFER_FALLBACK,
                "oneWayLatencyMs": (one_way_ms * 100.0).round() / 100.0,
                "roundTripLatencyMs": (round_trip_ms * 100.0).round() / 100.0,
            }))
        }
        "listPresets" => Ok(json!({ "presets": sh.presets() })),
        "listRoutings" => Ok(list_routings(sh)),
        "engine.wait" => {
            // Idempotent graceful stop: signal the main loop to tear down.
            sh.running.store(false, Ordering::Relaxed);
            sh.shutdown.notify_one();
            Ok(snapshot(sh))
        }
        "engine.status" => {
            let status = if sh.running.load(Ordering::Relaxed) {
                "running"
            } else {
                "stopped"
            };
            Ok(json!({ "status": status }))
        }
        "listModels" => sh.models.list_models(&args),
        "getModel" => sh.models.get_model(&s("id").ok_or_else(|| "id".to_string())?),
        "refreshIndex" => sh.models.refresh_index(),
        "downloadModel" => sh.models.download_model(&s("id").ok_or_else(|| "id".to_string())?),
        "cancelDownload" => sh.models.cancel_download(&s("id").ok_or_else(|| "id".to_string())?),
        other => Err(format!("unknown command: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

async fn handle_client(ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, sh: Arc<Shared>) {
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    sh.clients.lock().unwrap().push(tx.clone());
    eprintln!("pedal-engine: client connected ({} total)", sh.clients.lock().unwrap().len());

    // Greet with a full snapshot so a reconnecting UI never has to ask.
    if sink.send(Message::Text(state_event(&sh).into())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                let Message::Text(text) = msg else { continue };
                let reply = match serde_json::from_str::<Value>(&text) {
                    Ok(req) => {
                        let id = req.get("id").cloned().unwrap_or(Value::Null);
                        let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let args = req.get("args").cloned().unwrap_or(json!({}));
                        eprintln!("pedal-engine: <- {cmd}");
                        // A download fetches model bytes over blocking HTTP. Run
                        // it on tokio's blocking pool instead of this task:
                        // `reqwest::blocking` starts its own runtime and panics
                        // if invoked inside an async one, and either way a slow
                        // transfer must not stall `listModels` for other clients
                        // beyond the index lock it holds while writing.
                        let result = if cmd == "downloadModel" {
                            let models = sh.models.clone();
                            let id = args
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            match tokio::task::spawn_blocking(move || models.download_model(&id)).await {
                                Ok(inner) => inner,
                                Err(e) => Err(format!("download task failed: {e}")),
                            }
                        } else {
                            apply(&sh, &cmd, &args)
                        };
                        match result {
                            Ok(payload) => {
                                if cmd != "getState" {
                                    // Tell everyone, the requester included. The
                                    // reply carries the state too and the UI drops
                                    // duplicates by `rev`, so if the reply is lost
                                    // the broadcast still lands — that redundancy
                                    // is what keeps the UI from freezing.
                                    send_state(&sh, None);
                                }
                                json!({"type":"res","id":id,"ok":true,"payload":payload})
                            }
                            Err(e) => json!({"type":"res","id":id,"ok":false,"error":e}),
                        }
                    }
                    Err(e) => json!({"type":"res","id":Value::Null,"ok":false,"error":format!("bad JSON: {e}")}),
                };
                if sink.send(Message::Text(reply.to_string().into())).await.is_err() {
                    break;
                }
            }
            out = rx.recv() => {
                let Some(text) = out else { break };
                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    sh.clients.lock().unwrap().retain(|c| !c.is_closed());
}

/// Broadcast meters to every connected client.
async fn meters_loop(sh: Arc<Shared>) {
    let mut tick = tokio::time::interval(METERS_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        if sh.clients.lock().unwrap().is_empty() {
            continue;
        }
        let m = &sh.handle.meters;
        let out = read_f32(&m.out_peak);
        let cpu = read_f32(&m.callback_s) / (crate::audio::MAX_BLOCK as f32 / sh.handle.sample_rate) * 100.0;
        let xruns = m.xruns.load(Ordering::Relaxed);
        // Per-band gain reduction / output level of the multiband compressor.
        let bands: Vec<_> = m
            .bands()
            .iter()
            .map(|b| json!({"band": b.band, "gr_db": b.gr_db, "out_db": b.out_db}))
            .collect();
        // Smart noise gate peak reduction (dB, >= 0).
        let smart_gate_reduction = read_f32(&m.reduction_db);
        let text = json!({
            "type":"event","event":"meters",
            "payload":{"in":0.0,"out":out,"cpu":cpu,"xruns":xruns,"bands":bands,"smart_gate_reduction":smart_gate_reduction}
        })
        .to_string();
        broadcast(&sh, text, None);
    }
}

/// Broadcast tuner readings to every connected client at 20 Hz, while any
/// client is connected. The reading is not part of `state`: it changes far
/// faster than the rig and must be visible immediately on reconnect.
async fn tuner_loop(sh: Arc<Shared>) {
    let mut tick = tokio::time::interval(TUNER_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        eprintln!("DEBUG tuner_loop: clients={}, now", sh.clients.lock().unwrap().len());
        if sh.clients.lock().unwrap().is_empty() {
            continue;
        }
        let r = sh.handle.meters.tuner.lock().unwrap().clone();
        eprintln!("DEBUG tuner_loop: broadcasting active={} freq={:?}", r.active, r.freq);
        let text = json!({
            "type": "event",
            "event": "tuner",
            "payload": {
                "active": r.active,
                "freq": r.freq,
                "note": r.note,
                "octave": r.octave,
                "cents": r.cents,
                "clarity": r.clarity,
                "reference": crate::tuner::REFERENCE_HZ,
            }
        }).to_string();
        broadcast(&sh, text, None);
    }
}

/// Bind loopback and serve until the process exits.
pub async fn serve(sh: Arc<Shared>) -> Result<(), String> {
    serve_port(sh, PORT).await
}

/// Bind loopback on an explicit port and serve until the process exits.
///
/// Split out from [`serve`] so a test (or a Pi whose [`PORT`] is taken) can
/// bind somewhere else.
pub async fn serve_port(sh: Arc<Shared>, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| e.to_string())?;
    eprintln!("pedal-engine: ipc listening on ws://127.0.0.1:{port}");
    serve_listener(sh, &listener).await
}

/// Serve on an already-bound listener.
///
/// Split out from [`serve`] so a test can bind an ephemeral port instead of
/// the production [`PORT`] (which a stale engine from an earlier session may
/// still hold).
pub async fn serve_listener(sh: Arc<Shared>, listener: &TcpListener) -> Result<(), String> {
    eprintln!(
        "pedal-engine: ipc listening on {}",
        listener.local_addr().map_err(|e| e.to_string())?
    );
    tokio::spawn(tuner_loop(sh.clone()));
    tokio::spawn(meters_loop(sh.clone()));
    loop {
        let (stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let sh = sh.clone();
        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(ws) => handle_client(ws, sh).await,
                Err(_) => {}
            }
        });
    }
}
