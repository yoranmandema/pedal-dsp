//! Cross-channel routings: the data model, the project-file round trip, and
//! the IPC surface. See `docs/IPC-PROTOCOL.md`.
//!
//! The contract under test: a routing is its own entity, distinct from a
//! block's normal output routing, and creating one leaves the source block's
//! downstream connection alone.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::atomic::AtomicU8;

use dsp_graph::blocks::builtin_registry;
use dsp_graph::{Doc, Routing, RoutingEndpoint};
use futures_util::{SinkExt, StreamExt};
use pedal_engine::channels::{master_id, Rig, ROUTING_GAIN_PREFIX, ROUTING_MIX_PREFIX, IN};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

/// A two-channel rig: `ch1` = Amp → Delay, `ch2` = Reverb.
fn rig_with_two_channels() -> (Rig, String, String, String) {
    let mut rig = Rig::new("routed");
    let amp = rig.add_block_as("ch1", "amp.basic", "Amp", [0.0, 0.0]);
    let delay = rig.add_block_as("ch1", "time.delay", "Delay", [0.0, 0.0]);
    let reverb = rig.add_block_as("ch2", "time.reverb", "Reverb", [0.0, 0.0]);
    (rig, amp, delay, reverb)
}

fn route(from: (&str, &str), to: (&str, &str)) -> (RoutingEndpoint, RoutingEndpoint) {
    (RoutingEndpoint::new(from.0, from.1), RoutingEndpoint::new(to.0, to.1))
}

#[test]
fn create_routing_records_both_endpoints_enabled_with_unity_gain() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    let id = rig.create_routing(&source, &dest).expect("routing should be accepted");

    assert_eq!(id, "route1");
    let list = rig.list_routings();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].source, source);
    assert_eq!(list[0].dest, dest);
    assert!(list[0].enabled, "a new routing is on");
    assert_eq!(list[0].gain, 1.0, "a new routing is unity gain");
}

/// The acceptance criterion: a routing is not an edge, and the source block's
/// original downstream connection survives.
#[test]
fn a_routing_is_not_an_edge_and_leaves_the_source_chain_alone() {
    let (mut rig, amp, delay, reverb) = rig_with_two_channels();
    let before = rig.to_doc();

    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    rig.create_routing(&source, &dest).unwrap();
    let after = rig.to_doc();

    assert_eq!(before.edges, after.edges, "creating a routing rewired the graph");
    assert_eq!(before.nodes, after.nodes, "creating a routing changed the nodes");
    assert_eq!(after.routings.len(), 1, "the routing is stored as its own entity");
    assert_eq!(after.routings[0].source.block, amp);

    // Nothing in `edges` stands in for the routing.
    assert!(!after.edges.iter().any(|e| e.from[0] == amp && e.to[0] == reverb));
    // The source's normal chain is intact.
    assert!(after.edges.iter().any(|e| e.from[0] == amp && e.to[0] == delay));
    assert!(after.edges.iter().any(|e| e.from[0] == delay && e.to[0] == master_id("ch1")));

    // And the channel source rows are untouched.
    assert_eq!(rig.channel("ch1").unwrap().sources, vec![IN.to_string()]);
    assert_eq!(rig.channel("ch2").unwrap().sources, vec![IN.to_string()]);
}

#[test]
fn routings_survive_a_project_save_and_reload() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    let id = rig.create_routing(&source, &dest).unwrap();
    rig.set_enabled(&id, false).unwrap();
    rig.set_gain(&id, 0.25).unwrap();

    let json = rig.to_doc().to_json();
    assert!(json.contains("\"routings\""), "the save file dropped the routings: {json}");
    let doc = Doc::from_json(&json).expect("save file is valid JSON");
    let back = Rig::from_doc(&doc);

    assert_eq!(back.routings, rig.routings, "the routing did not round trip");
    assert_eq!(back.list_routings()[0].id, id);
    assert_eq!(back.list_routings()[0].source, source);
    assert_eq!(back.list_routings()[0].dest, dest);
    assert!(!back.list_routings()[0].enabled);
    assert_eq!(back.list_routings()[0].gain, 0.25);
}

/// A project written before routings existed (or by hand) must still load:
/// `enabled` and `gain` are optional.
#[test]
fn a_routing_without_enabled_or_gain_takes_the_defaults() {
    let json = r#"{
        "name": "hand-written",
        "nodes": [],
        "edges": [],
        "routings": [
            { "id": "r1",
              "source": { "channel": "ch1", "block": "amp" },
              "dest":   { "channel": "ch2", "block": "reverb" } }
        ]
    }"#;
    let doc = Doc::from_json(json).expect("json");
    assert!(doc.routings[0].enabled);
    assert_eq!(doc.routings[0].gain, 1.0);
}

#[test]
fn remove_and_toggle_a_routing() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    let id = rig.create_routing(&source, &dest).unwrap();

    rig.set_enabled(&id, false).unwrap();
    assert!(!rig.routing(&id).expect("routing").enabled);
    rig.set_enabled(&id, true).unwrap();
    assert!(rig.routing(&id).expect("routing").enabled);
    assert!(rig.set_enabled("route404", true).is_err(), "unknown id must be refused");
    assert!(rig.set_gain("route404", 0.5).is_err());

    assert!(rig.remove_routing(&id));
    assert!(!rig.remove_routing(&id), "removing twice is not a success");
    assert!(rig.list_routings().is_empty());
}

#[test]
fn invalid_routings_are_refused() {
    let (mut rig, amp, delay, reverb) = rig_with_two_channels();

    // Unknown block.
    let (bad, dest) = route(("ch1", "ghost"), ("ch2", &reverb));
    assert!(rig.create_routing(&bad, &dest).is_err());

    // A block routing to itself.
    let (self_src, self_dst) = route(("ch1", &amp), ("ch1", &amp));
    assert!(rig.create_routing(&self_src, &self_dst).is_err());

    // Within one channel.
    let (same_src, same_dst) = route(("ch1", &amp), ("ch1", &delay));
    assert!(rig.create_routing(&same_src, &same_dst).is_err());

    // Valid, then an exact duplicate.
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    rig.create_routing(&source, &dest).unwrap();
    assert!(rig.create_routing(&source, &dest).is_err(), "duplicate must be refused");

    // A feedback loop: reverb → amp closes the circle.
    let (back_src, back_dst) = route(("ch2", &reverb), ("ch1", &amp));
    assert!(rig.create_routing(&back_src, &back_dst).is_err(), "cycle must be refused");

    assert_eq!(rig.list_routings().len(), 1, "refused routings must not enter the list");
}

/// Deleting a block takes its routings with it: a routing whose endpoint is
/// gone cannot be honoured, so it must not linger in the list or in the save.
#[test]
fn deleting_a_block_prunes_its_routings() {
    let (mut rig, amp, delay, reverb) = rig_with_two_channels();
    let (a_to_r, a_to_r_dest) = route(("ch1", &amp), ("ch2", &reverb));
    let _ = rig.create_routing(&a_to_r, &a_to_r_dest).unwrap();
    // A second routing that does not touch the deleted block survives.
    let delay_to_reverb = (
        RoutingEndpoint::new("ch1", delay.clone()),
        RoutingEndpoint::new("ch2", reverb.clone()),
    );
    let keep = rig.create_routing(&delay_to_reverb.0, &delay_to_reverb.1).unwrap();

    rig.remove_block("ch1", &amp);
    assert!(rig.routing("route1").is_none(), "the routing into the deleted block is gone");
    assert_eq!(rig.list_routings().len(), 1);
    assert_eq!(rig.list_routings()[0].id, keep);

    // The survivor round trips, and the pruned one stays gone.
    let back = Rig::from_doc(&rig.to_doc());
    assert_eq!(back.list_routings().len(), 1);
    assert_eq!(back.list_routings()[0].id, keep);
}

/// Reordering a strip must not disturb routings: they name block ids, not slots.
#[test]
fn reordering_a_channel_keeps_its_routings() {
    let (mut rig, amp, delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    let id = rig.create_routing(&source, &dest).unwrap();

    rig.move_block("ch1", &delay, 0);
    let r = rig.routing(&id).expect("routing survives a reorder");
    assert_eq!(r.source, source);
    assert_eq!(r.dest, dest);
}

#[test]
fn load_drops_invalid_routings() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    let good = rig.create_routing(&source, &dest).unwrap();

    let mut doc = rig.to_doc();
    doc.routings.push(Routing {
        id: "bad".into(),
        source: RoutingEndpoint::new("ch1", amp.clone()),
        dest: RoutingEndpoint::new("ch1", amp.clone()),
        enabled: true,
        gain: 1.0,
    });

    let back = Rig::from_doc(&doc);
    assert_eq!(back.list_routings().len(), 1, "the invalid routing should have been dropped");
    assert_eq!(back.list_routings()[0].id, good);
}

// ---------------------------------------------------------------------------
// Audio graph
// ---------------------------------------------------------------------------

/// Build the rig's graph the way the engine does: lay it out as a doc, then
/// apply the routings. No audio is rendered — these tests assert on structure.
fn graph_of(rig: &Rig) -> dsp_graph::Graph {
    let reg = builtin_registry();
    let mut g = rig
        .to_doc()
        .build_in(&reg, std::path::Path::new("."))
        .expect("rig builds");
    rig.apply_routings(&mut g, &reg).expect("routings apply");
    g
}

/// Is `from`'s output wired into `to`'s `port`?
fn feeds(g: &dsp_graph::Graph, from: &str, to: &str, port: &str) -> bool {
    g.edges()
        .iter()
        .any(|e| e.from.0 == from && e.from.1 == "out" && e.to.0 == to && e.to.1 == port)
}

/// How many wires drive `node`'s `port`. An input port takes exactly one.
fn drivers(g: &dsp_graph::Graph, node: &str, port: &str) -> usize {
    g.edges()
        .iter()
        .filter(|e| e.to.0 == node && e.to.1 == port)
        .count()
}

/// The acceptance criterion, on the graph: a routing taps the source and sums
/// into the destination, and the source's own wire is untouched.
#[test]
fn a_routing_taps_the_source_and_sums_into_the_destination() {
    let (mut rig, amp, delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    let id = rig.create_routing(&source, &dest).unwrap();
    let g = graph_of(&rig);

    let mix = format!("{ROUTING_MIX_PREFIX}{id}");
    assert_eq!(g.node(&mix).expect("mix node").spec.id, "util.mix");
    assert!(
        g.node(&format!("{ROUTING_GAIN_PREFIX}{id}")).is_none(),
        "a unity routing inserts no gain node"
    );

    // The source keeps its own downstream wire, and the tap reaches the mix.
    assert!(
        feeds(&g, &amp, &delay, "in"),
        "the source's own chain is untouched"
    );
    assert!(feeds(&g, &amp, &mix, "in_b"), "the tap reaches the mix");
    // The destination's original driver is preserved as the mix's other input.
    assert!(
        feeds(&g, "in", &mix, "in_a"),
        "the original driver feeds the mix"
    );
    assert!(
        feeds(&g, &mix, &reverb, "in"),
        "the mix drives the destination"
    );
    assert_eq!(drivers(&g, &reverb, "in"), 1);
}

/// Fan-out: one source can feed several routings, and each gets its own mix.
#[test]
fn two_routings_out_of_one_source_both_reach_their_destinations() {
    let (mut rig, amp, delay, reverb) = rig_with_two_channels();
    let ch3 = rig.add_block_as("ch3", "time.reverb", "Reverb", [0.0, 0.0]);
    let (s1, d1) = route(("ch1", &amp), ("ch2", &reverb));
    let (s2, d2) = route(("ch1", &amp), ("ch3", &ch3));
    let first = rig.create_routing(&s1, &d1).unwrap();
    let second = rig.create_routing(&s2, &d2).unwrap();
    let g = graph_of(&rig);

    for id in [first, second] {
        let mix = format!("{ROUTING_MIX_PREFIX}{id}");
        assert_eq!(g.node(&mix).expect("mix node").spec.id, "util.mix");
        assert!(feeds(&g, &amp, &mix, "in_b"), "the tap reaches its mix");
    }
    // The source's own chain is still there, exactly once.
    assert_eq!(drivers(&g, &delay, "in"), 1);
}

/// Two routings into one block nest: the second finds the first mix as the
/// destination's current driver, so the block sums both taps.
#[test]
fn two_routings_into_one_destination_chain_two_mixes() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let ch3 = rig.add_block_as("ch3", "time.reverb", "Reverb", [0.0, 0.0]);
    let (s1, d1) = route(("ch1", &amp), ("ch2", &reverb));
    let (s2, d2) = route(("ch3", &ch3), ("ch2", &reverb));
    let first = rig.create_routing(&s1, &d1).unwrap();
    let second = rig.create_routing(&s2, &d2).unwrap();
    let g = graph_of(&rig);

    let mix1 = format!("{ROUTING_MIX_PREFIX}{first}");
    let mix2 = format!("{ROUTING_MIX_PREFIX}{second}");
    assert!(
        feeds(&g, &mix1, &mix2, "in_a"),
        "the second routing found the first mix"
    );
    assert!(
        feeds(&g, &ch3, &mix2, "in_b"),
        "the second tap reaches its mix"
    );
    assert!(
        feeds(&g, &mix2, &reverb, "in"),
        "the outer mix drives the destination"
    );
    assert_eq!(drivers(&g, &reverb, "in"), 1);
    // The destination's original driver now feeds the first mix.
    assert!(feeds(&g, "in", &mix1, "in_a"));
}

/// A non-unity routing gets a `util.gain` expressed in dB, in front of the mix.
#[test]
fn a_non_unity_routing_gets_a_gain_stage_in_db() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    let id = rig.create_routing(&source, &dest).unwrap();
    rig.set_gain(&id, 0.25).unwrap();
    let g = graph_of(&rig);

    let gain_id = format!("{ROUTING_GAIN_PREFIX}{id}");
    let node = g.node(&gain_id).expect("gain node");
    assert_eq!(node.spec.id, "util.gain");
    let spec = node.spec.params[0].clone();
    let db = 20.0 * 0.25f32.log10();
    assert!(
        (node.params[0] - spec.normalise(db)).abs() < 1e-6,
        "gain should be {db} dB"
    );

    let mix = format!("{ROUTING_MIX_PREFIX}{id}");
    assert!(
        feeds(&g, &amp, &gain_id, "in"),
        "the tap runs through the gain"
    );
    assert!(feeds(&g, &gain_id, &mix, "in_b"), "the gain feeds the mix");
    assert!(feeds(&g, "in", &mix, "in_a"));
    assert!(feeds(&g, &mix, &reverb, "in"));
}

/// Disabling a routing removes its nodes and edges: the graph is identical to
/// one that never had it.
#[test]
fn a_disabled_routing_leaves_the_graph_untouched() {
    let (mut rig, amp, delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    let id = rig.create_routing(&source, &dest).unwrap();
    let enabled = graph_of(&rig);
    let plain = graph_of(&rig_with_two_channels().0);

    rig.set_enabled(&id, false).unwrap();
    let disabled = graph_of(&rig);

    let mix = format!("{ROUTING_MIX_PREFIX}{id}");
    assert!(enabled.node(&mix).is_some());
    assert!(
        disabled.node(&mix).is_none(),
        "a disabled routing inserts no node"
    );
    let mut got = disabled.node_ids();
    let mut want = plain.node_ids();
    got.sort();
    want.sort();
    assert_eq!(got, want, "a disabled routing changed the node set");
    assert_eq!(
        disabled.edges(),
        plain.edges(),
        "a disabled routing changed the wires"
    );
    assert!(
        feeds(&disabled, &amp, &delay, "in"),
        "the source's own chain is back"
    );
}

/// A routed graph still validates, and the routing nodes are in the compiled
/// executor — the fan-out is real signal path, not just a drawn line.
#[test]
fn a_routed_graph_validates_and_compiles() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let ch3 = rig.add_block_as("ch3", "time.reverb", "Reverb", [0.0, 0.0]);
    let (s1, d1) = route(("ch1", &amp), ("ch2", &reverb));
    let (s2, d2) = route(("ch1", &amp), ("ch3", &ch3));
    let first = rig.create_routing(&s1, &d1).unwrap();
    let second = rig.create_routing(&s2, &d2).unwrap();

    let mut g = graph_of(&rig);
    g.validate().expect("a routed graph validates");
    let ex =
        dsp_graph::Executor::compile_owned(&mut g, 48_000.0, 256).expect("a routed graph compiles");
    let ids = ex.node_ids();
    assert!(ids.contains(&format!("{ROUTING_MIX_PREFIX}{first}")));
    assert!(ids.contains(&format!("{ROUTING_MIX_PREFIX}{second}")));
}

/// A destination with no driver cannot be summed into; the routing is refused
/// rather than silently dropped or panicking.
#[test]
fn a_routing_needs_a_destination_that_is_already_driven() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    rig.create_routing(&source, &dest).unwrap();

    let reg = builtin_registry();
    let mut g = rig
        .to_doc()
        .build_in(&reg, std::path::Path::new("."))
        .unwrap();
    g.disconnect_input(&reverb, "in");
    let err = rig
        .apply_routings(&mut g, &reg)
        .expect_err("an undriven destination must be refused");
    assert!(
        err.contains("has no input to mix with"),
        "unexpected error: {err}"
    );
    assert!(g.node(&format!("{ROUTING_MIX_PREFIX}route1")).is_none());
}

/// A routing whose generated node id is already taken is refused, not merged.
#[test]
fn a_routing_refuses_a_node_id_that_is_already_taken() {
    let (mut rig, amp, _delay, reverb) = rig_with_two_channels();
    let (source, dest) = route(("ch1", &amp), ("ch2", &reverb));
    rig.create_routing(&source, &dest).unwrap();

    let reg = builtin_registry();
    let mut g = rig
        .to_doc()
        .build_in(&reg, std::path::Path::new("."))
        .unwrap();
    let taken = format!("{ROUTING_MIX_PREFIX}route1");
    g.add(taken.as_str(), reg.build("util.mix").unwrap())
        .unwrap();
    let err = rig
        .apply_routings(&mut g, &reg)
        .expect_err("a taken node id must be refused");
    assert!(err.contains("already taken"), "unexpected error: {err}");
}
// ---------------------------------------------------------------------------
// IPC
// ---------------------------------------------------------------------------

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

/// Send one `req` frame.
async fn req<S>(ws: &mut S, id: i64, cmd: &str, args: Value)
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    ws.send(Message::Text(
        json!({ "type": "req", "id": id, "cmd": cmd, "args": args }).to_string().into(),
    ))
    .await
    .expect("send");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn routing_commands_over_ipc() {
    use pedal_engine::audio::{new_engine, ChannelMode, Setup};
    use pedal_engine::host::{BLOCK, LATENCY_FRAMES};
    use pedal_engine::server::{serve_listener, Shared};
    use pedal_engine::store::Store;
    use tokio::net::TcpListener;

    let setup = Setup {
        sample_rate: 48_000.0,
        out_channels: 2,
        block: BLOCK,
        channel_mode: ChannelMode::First(2),
        latency_frames: LATENCY_FRAMES,
    };
    let source = Arc::new(AtomicU8::new(0));
    let (mut handle, _cmd_rx, _trash_tx, _running) = new_engine(setup, source.clone());
    let registry = handle.registry.clone();
    let store = Store::discover();
    handle.preset_dir = store.dir.clone();

    let (rig, amp, delay, reverb) = rig_with_two_channels();
    let sh = Arc::new(Shared::new(handle, registry, source, store,
        Arc::new(AtomicBool::new(true)), Arc::new(pedal_engine::models::Models::new())));
    *sh.rig.lock().unwrap() = rig;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let sh_task = sh.clone();
    tokio::spawn(async move { serve_listener(sh_task, &listener).await });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");
    let state = next_event(&mut ws, "state").await;
    assert_eq!(state["payload"]["routings"], json!([]), "a fresh rig has no routings");

    // Create.
    req(
        &mut ws,
        1,
        "createRouting",
        json!({"source": {"channel": "ch1", "block": amp}, "dest": {"channel": "ch2", "block": reverb}}),
    )
    .await;
    let res = next_res(&mut ws, 1).await;
    assert_eq!(res["ok"], true, "createRouting refused: {res}");
    assert_eq!(res["payload"]["routings"][0]["id"], "route1");
    assert_eq!(res["payload"]["routings"][0]["enabled"], true);
    assert_eq!(res["payload"]["routings"][0]["gain"].as_f64(), Some(1.0));

    // List.
    req(&mut ws, 2, "listRoutings", json!({})).await;
    let res = next_res(&mut ws, 2).await;
    assert_eq!(res["payload"]["routings"][0]["source"]["block"], amp);
    assert_eq!(res["payload"]["routings"][0]["dest"]["block"], reverb);

    // Toggle off.
    req(&mut ws, 3, "setEnabled", json!({"id": "route1", "enabled": false})).await;
    let res = next_res(&mut ws, 3).await;
    assert_eq!(res["ok"], true, "setEnabled refused: {res}");
    assert_eq!(res["payload"]["routings"][0]["enabled"], false);

    // Remove.
    req(&mut ws, 4, "removeRouting", json!({"id": "route1"})).await;
    let res = next_res(&mut ws, 4).await;
    assert_eq!(res["ok"], true, "removeRouting refused: {res}");
    assert_eq!(res["payload"]["routings"], json!([]));

    // Refused: same channel.
    req(
        &mut ws,
        5,
        "createRouting",
        json!({"source": {"channel": "ch1", "block": amp}, "dest": {"channel": "ch1", "block": delay}}),
    )
    .await;
    let res = next_res(&mut ws, 5).await;
    assert_eq!(res["ok"], false, "a same-channel routing must be refused: {res}");
    assert!(
        res["error"].as_str().is_some_and(|e| e.contains("cross channels")),
        "unexpected error: {res}"
    );

    // The refused routing never entered the list.
    req(&mut ws, 6, "getState", json!({})).await;
    let res = next_res(&mut ws, 6).await;
    assert_eq!(res["payload"]["routings"], json!([]), "a refused routing must not enter the list");
}
