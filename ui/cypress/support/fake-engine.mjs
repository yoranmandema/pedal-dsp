// A stand-in for `pedal-engine` on the Cypress side.
//
// It speaks the real IPC protocol (docs/IPC-PROTOCOL.md) over WebSocket: it
// greets every connection with a state snapshot, answers `req` messages with
// `res`, and broadcasts `event: state` after a mutation. The UI therefore runs
// its normal `WebSocketBridge` code path — only the process on the other end of
// the socket is a fixture.
//
// State is rebuilt from `createSeedState()` on every connection, so each
// `cy.visit()` starts from the same rig. No audio device is touched.

import http from "node:http";
import fs from "node:fs";
import { WebSocketServer } from "ws";
import { createSeedState, defaultParams, METERS } from "./seed.mjs";

const BLOCK_MIME = "application/x-block-type"; // mirrors ui/src/dnd.ts

const TUNER_DEFAULT = {
  active: true,
  freq: 220.0,
  note: "A",
  octave: 3,
  cents: -1.2,
  clarity: 0.94,
  reference: 440.0,
};

function slugify(name) {
  const s = String(name)
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return s || "block";
}

function clamp(v, min, max) {
  return Math.min(max, Math.max(min, v));
}

/** Start the fake engine. Resolves once it is listening. */
export async function startFakeEngine({ port = 9400, host = "127.0.0.1" } = {}) {
  let state = createSeedState();
  let rev = 0;
  /** The tuner reading, reset to `TUNER_DEFAULT` on every connection. */
  let tuner = { ...TUNER_DEFAULT };
  /** @type {Set<import("ws").WebSocket>} */
  const clients = new Set();

  /** One-way state snapshot, sent to every client on connect and after each
   *  mutation. */
  const stateEvent = () => JSON.stringify({ type: "event", event: "state", payload: state });


  const metersEvent = () => JSON.stringify({ type: "event", event: "meters", payload: METERS });
  const tunerEvent = () => {
  // Diagnostic: log the engine's tuner state to a file for the E2E spec to read.
  fs.appendFileSync("/tmp/tuner_state.json", JSON.stringify({ ts: Date.now(), tuner: { ...tuner } }) + "\n");
  return JSON.stringify({ type: "event", event: "tuner", payload: tuner });
};

  const broadcast = () => {
    const text = stateEvent();
    for (const ws of clients) {
      if (ws.readyState === ws.OPEN) ws.send(text);
    }
  };

  const findChannel = (id) => {
    const ch = state.channels.find((c) => c.id === id);
    if (!ch) throw new Error(`no such channel: ${id}`);
    return ch;
  };

  const findBlock = (channelId, blockId) => {
    const ch = findChannel(channelId);
    const b = ch.blocks.find((x) => x.id === blockId);
    if (!b) throw new Error(`no such block: ${blockId}`);
    return b;
  };

  /** Apply a mutation and return the new full snapshot. */
  const commit = () => {
    rev += 1;
    state.rev = rev;
    return cloneState(state);
  };

  // Deep-clone the rig on each mutation so the UI re-renders: the Preact
  // signals setter only notifies on reference change, and the engine mutates
  // the rig in place. Each e2e connection owns its own rig, so the cloned
  // snapshot is never shared across tests.
  function cloneState(s) {
    return {
      ...s,
      channels: s.channels.map((c) => ({
        ...c,
        blocks: c.blocks.map((b) => ({ ...b, params: { ...b.params } })),
      })),
    };
  }

  const uniqueBlockId = (ch, name) => {
    const taken = new Set(ch.blocks.map((b) => b.id));
    const base = slugify(name);
    let id = base;
    let n = 2;
    while (taken.has(id)) id = `${base}${n++}`;
    return id;
  };

  const MUTATIONS = new Set([
    "setParam",
    "setMasterVolume",
    "setBypass",
    "addBlock",
    "removeBlock",
    "moveBlock",
    "setSources",
    "createRouting",
    "removeRouting",
    "setEnabled",
    "setGain",
    "setSource",
    "loadPreset",
    "savePreset",
    "deletePreset",
    "renamePreset",
  ]);

  /** Run one command. Throws on refusal, like the real engine. */
  function applyCommand(cmd, args = {}) {
    switch (cmd) {
      case "getState":
        return state;

      case "listPresets":
        return { presets: state.presets };

      case "listRoutings":
        return { routings: state.routings };

      case "setParam": {
        const b = findBlock(args.channel, args.block);
        const spec = state.registry[b.type];
        const p = spec?.params.find((x) => x.id === args.param);
        if (!spec) throw new Error(`unknown block type: ${b.type}`);
        if (!p) throw new Error(`no such param: ${args.param}`);
        const value = Number(args.value);
        if (!Number.isFinite(value)) throw new Error(`bad value: ${args.value}`);

        // --- Looper trigger gates: translate the UI's momentary gates into the
        // engine's looper state machine (docs/looper.md §4). The mock stands in
        // for pedal-engine while the engine card is not merged yet. */
        const LOOPER_TRIGS = ["trig_record", "trig_play", "trig_overdub", "trig_stop", "trig_clear"];
        if (spec.params.some((x) => LOOPER_TRIGS.includes(x.id))) {
          const last = b._looperLastTrigs ??= {};
          const wasHeld = last[p.id] >= 0.5;
          const isHeld = value >= 0.5;
          if (wasHeld && !isHeld) applyLooperGate(p.id, false); // release
          else if (!wasHeld && isHeld) applyLooperGate(p.id, true); // press
          last[p.id] = value;
        }
        b.params[p.id] = p.kind === "choice" ? Math.round(clamp(value, p.min, p.max)) : clamp(value, p.min, p.max);
        return commit();
      }

      case "setMasterVolume": {
        const ch = findChannel(args.channel);
        ch.volume = clamp(Number(args.value), -60, 12);
        return commit();
      }

      case "setBypass": {
        const b = findBlock(args.channel, args.block);
        b.bypass = !!args.bypass;
        return commit();
      }

      case "addBlock": {
        const ch = findChannel(args.channel);
        const spec = state.registry[args.type];
        if (!spec) throw new Error(`unknown block type: ${args.type}`);
        const name = args.name ?? spec.name;
        ch.blocks.push({
          id: uniqueBlockId(ch, name),
          type: args.type,
          name,
          bypass: false,
          params: defaultParams(spec),
        });
        return commit();
      }

      case "removeBlock": {
        // Test hook: the seeded `locked` block always refuses, so the UI's
        // command-failure toast is reachable.
        if (args.block === "locked") throw new Error(`no such block: ${args.block}`);
        const ch = findChannel(args.channel);
        const i = ch.blocks.findIndex((b) => b.id === args.block);
        if (i < 0) throw new Error(`no such block: ${args.block}`);
        ch.blocks.splice(i, 1);
        return commit();
      }

      case "moveBlock": {
        const ch = findChannel(args.channel);
        const i = ch.blocks.findIndex((b) => b.id === args.block);
        if (i < 0) throw new Error(`no such block: ${args.block}`);
        const [b] = ch.blocks.splice(i, 1);
        const to = clamp(Math.trunc(Number(args.to)), 0, ch.blocks.length);
        ch.blocks.splice(to, 0, b);
        return commit();
      }

      case "setSources": {
        const ch = findChannel(args.channel);
        ch.sources = Array.isArray(args.sources) ? args.sources : [];
        return commit();
      }

      case "setSource": {
        state.source = ["riff", "live", "silence"].includes(args.source) ? args.source : "riff";
        return commit();
      }

      case "loadPreset": {
        if (!state.presets.includes(args.name)) throw new Error(`no such preset: ${args.name}`);
        state.preset = args.name;
        return commit();
      }

      case "savePreset": {
        const name = String(args.name ?? "");
        if (!name) throw new Error("empty preset name");
        if (!state.presets.includes(name)) state.presets = [...state.presets, name].sort();
        state.preset = name;
        return commit();
      }

      case "deletePreset": {
        // Test hook: the seeded `02-lead` preset always refuses.
        if (args.name === "02-lead") throw new Error("preset is locked");
        if (!state.presets.includes(args.name)) throw new Error(`no such preset: ${args.name}`);
        state.presets = state.presets.filter((p) => p !== args.name);
        if (state.preset === args.name) state.preset = state.presets[0] ?? "";
        return commit();
      }

      case "renamePreset": {
        const from = String(args.from ?? "");
        const to = String(args.to ?? "");
        const i = state.presets.indexOf(from);
        if (i < 0) throw new Error(`no such preset: ${from}`);
        if (!to) throw new Error("empty preset name");
        if (to === from) return state;
        if (state.presets.includes(to)) throw new Error(`preset already exists: ${to}`);
        state.presets = state.presets.map((p) => (p === from ? to : p)).sort();
        if (state.preset === from) state.preset = to;
        return commit();
      }

      default:
        throw new Error(`unknown command: ${cmd}`);
    }
  }

  const httpServer = http.createServer((req, res) => {
    if (req.method === "POST" && req.url === "/__reset") {
      state = createSeedState();
      rev = 0;
      res.writeHead(204).end();
      return;
    }
    if (req.method === "POST" && req.url === "/__tuner") {
      let body = "";
      req.on("data", (chunk) => {
        body += chunk;
      });
      req.on("end", () => {
        try {
          const patch = body ? JSON.parse(body) : {};
          for (const key of ["active", "freq", "note", "octave", "cents", "clarity", "reference"]) {
            if (key in patch) tuner[key] = patch[key];
          }
          res.writeHead(204).end();
        } catch {
          res.writeHead(400).end();
        }
      });
      return;
    }
    if (req.method === "GET" && req.url === "/__health") {
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ ok: true }));
      return;
    }
    res.writeHead(404).end();
  });

  const wss = new WebSocketServer({ server: httpServer });

  wss.on("connection", (ws) => {
    // Fresh rig per connection == fresh rig per `cy.visit()`.
    state = createSeedState();
    rev = 0;
    clients.add(ws);
    ws.send(stateEvent());
    ws.send(metersEvent());
    ws.send(tunerEvent());
    const heartbeat = setInterval(() => {
      if (ws.readyState === ws.OPEN) {
        ws.send(metersEvent());
        ws.send(tunerEvent());
      }
    }, 50);
    // Per-connection looper state, so each Cypress `cy.visit()` starts fresh and
    // a timer from a previous connection cannot advance another client's
    // playback position (see cypress/e2e/looper.cy.js).
    let looperState = 0;
    let looperFrac = 0;
    let looperFracTimer = null;

    /** Looper event, sent only to a client while the looper is playing. */
    const looperEvent = () => JSON.stringify({ type: "event", event: "looper", payload: { state: looperState, frac: looperFrac } });

    /** Translate one trigger gate press/release into the looper state machine. */
    function applyLooperGate(id, rising) {
      if (rising) {
        if (looperState === 0) {
          if (id === "trig_record") looperState = 1;
          else if (id === "trig_overdub") looperState = 3;
          else if (id === "trig_play") looperState = 2;
          else if (id === "trig_stop") looperState = 4;
          else if (id === "trig_clear") looperState = 4;
        } else if (looperState === 1) {
          if (id === "trig_play") looperState = 2;
          else if (id === "trig_overdub") looperState = 3;
          else if (id === "trig_stop") looperState = 4;
        } else if (looperState === 2) {
          if (id === "trig_overdub") looperState = 3;
          else if (id === "trig_stop") looperState = 4;
        } else if (looperState === 3) {
          if (id === "trig_stop") looperState = 4;
        } else if (looperState === 4) {
          if (id === "trig_record") looperState = 1;
          else if (id === "trig_overdub") looperState = 3;
          else if (id === "trig_play") looperState = 2;
          else if (id === "trig_stop") looperState = 4;
          else if (id === "trig_clear") looperState = 4;
        }
        // Stop and Clear, when held, force the loop to STOPPED from any state.
        if (id === "trig_stop" || id === "trig_clear") looperState = 4;
        // A rising gate that starts a new take resets the playback position.
        if ([1, 2, 3].includes(looperState)) {
          looperFrac = 0;
          state.looper_state = looperState;
          state.looper_frac = 0;
        }
      } else {
        // Falling edge: the trigger was released.
        if (looperState === 1 && id === "trig_record") looperState = 4;
        else if (looperState === 2 && id === "trig_play") looperState = 4;
        else if (looperState === 3 && id === "trig_overdub") looperState = 2;
      }
      // Keep the snapshot's looper fields in sync so the banner always reflects
      // the current engine state.
      state.looper_state = looperState;
      state.looper_frac = looperFrac;
    }
    looperFracTimer = () => {
      if (looperState === 2 || looperState === 3) {
        looperFrac += 0.005;
        if (looperFrac > 1.0) looperFrac = 1.0;
        ws.send(looperEvent());
        if (looperFrac >= 1.0) ws.send(looperEvent());
      }
    };
    setInterval(looperFracTimer, 80);
    ws.on("message", (data) => {
      let req;
      try {
        req = JSON.parse(data.toString());
      } catch {
        return;
      }
      if (!req || req.type !== "req") return;
      let res;
      try {
        const payload = applyCommand(req.cmd, req.args ?? {});
        res = { type: "res", id: req.id, ok: true, payload };
      } catch (e) {
        res = { type: "res", id: req.id, ok: false, error: e instanceof Error ? e.message : String(e) };
      }
      if (ws.readyState === ws.OPEN) ws.send(JSON.stringify(res));
      if (res.ok && MUTATIONS.has(req.cmd)) broadcast();
    });

    const close = () => {
      clearInterval(heartbeat);
      clearInterval(looperFracTimer);
      clients.delete(ws);
    };
    ws.on("close", close);
    ws.on("error", close);
  });

  await new Promise((resolve, reject) => {
    httpServer.once("error", reject);
    httpServer.listen(port, host, resolve);
  });

  return { httpServer, wss, port, host };
}

/** Stop the fake engine and close every client. */
export async function stopFakeEngine(engine) {
  if (!engine) return;
  for (const ws of engine.wss.clients) ws.terminate();
  await new Promise((resolve) => {
    engine.wss.close(() => resolve());
    engine.httpServer.close(() => resolve());
    // `httpServer.close` waits for open sockets; the terminate above should
    // have closed them, but don't hang if one is half-open.
    setTimeout(resolve, 2000);
  });
}

export { BLOCK_MIME };
