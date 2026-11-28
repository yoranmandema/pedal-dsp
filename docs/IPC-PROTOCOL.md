# IPC Protocol

The wire contract between the **UI** (Preact, in a webview) and the **engine**
(Rust, the process that owns the audio interface).

This protocol is shared by every shell:

| Shell | Transport | Bridge |
|---|---|---|
| Desktop / Pi (Tauri) | WebSocket over loopback TCP | `WebSocketBridge` |
| Dev / browser | WebSocket over loopback TCP | `WebSocketBridge` |
| VST3 (iPlug2) | in-process C ABI | `NativeBridge` |

Only the bridge differs. The message format, the commands, and the state shape
are identical everywhere.

## Transport

- **WebSocket**, text frames, one JSON object per frame.
- **Endpoint:** `ws://127.0.0.1:8737/` (loopback only).
- The engine binds `127.0.0.1`; no external interface.
- A webview cannot open a Unix domain socket, so the transport is loopback TCP
  on every platform — including macOS and Linux.
- Binary frames are not used.

A frame is always a single JSON object. No batching, no arrays of messages.

## Message envelope

Every message has a `type`:

```jsonc
// Client -> server: a command, answered by exactly one `res` with the same id
{ "type": "req", "id": 7, "cmd": "setParam", "args": { ... } }

// Server -> client: the answer to a `req`
{ "type": "res", "id": 7, "ok": true, "payload": { ... } }
{ "type": "res", "id": 7, "ok": false, "error": "no such node" }

// Server -> client: unsolicited
{ "type": "event", "event": "state",    "payload": { ... } }
{ "type": "event", "event": "meters",   "payload": { ... } }
{ "type": "event", "event": "error",    "payload": { "message": "..." } }
```

Rules:

- `id` is chosen by the client, unique among in-flight requests. It is echoed
  back verbatim; the client correlates by id.
- `res.payload` depends on the command. For mutating commands it is the **full
  new state** — see below.
- Events are server-initiated. The client must handle them without having asked.
- Every state snapshot carries a `rev` counter, bumped by the engine on each
  successful graph rebuild. A client must ignore a snapshot whose `rev` is not
  newer than the last one it applied, so an out-of-order delivery cannot roll
  it back. A *lower* `rev` means the engine restarted and its counter began
  again; accept that one.

## Why full state instead of deltas

The whole rig state is a few kilobytes. Sending it after every mutation is
cheap, and it makes a reconnecting UI trivial: it never has to replay a delta
log, it just applies the latest snapshot.

Deltas would save bandwidth at a scale this UI will never reach, at the cost of
a much harder reconnection story. **Snapshot, not deltas.**

## Connection lifecycle

1. Client connects.
2. Server sends one `event: state` with the full snapshot (no request needed).
3. Client sends `req`s; server answers each with a `res`. For mutations the
   answer carries the new state **and** the server broadcasts an `event: state`
   to every client, the requester included. The two are identical, so the
   client's `rev` guard turns the duplicate into a no-op — and if the reply is
   ever lost, the broadcast still lands.
4. Server sends `event: meters` at 20 Hz while audio is running, and
   `event: tuner` at 20 Hz while a client is connected (see below).
5. On disconnect the engine keeps running. On reconnect the client gets a
   fresh snapshot.

## State shape

```jsonc
{
  "preset": "04-rainbow",            // current preset name, "" if unsaved
  "presets": ["00-blank", "01-clean", "..."],
  "source": "riff",                  // "riff" | "live" | "silence"
  "inputGain": 0.0,                  // pedal input trim, dB
  "masterOutVolume": 0.0,            // pedal output volume, dB
  "bufferFrames": 128,               // I/O buffer size, frames
  "sampleRate": 48000,               // read-only: fixed at engine startup
  "oneWayLatencyMs": 5.33,           // computed from bufferFrames + sampleRate
  "roundTripLatencyMs": 10.67,       // ditto
  "registry": {                      // static: block types the UI can offer
    "amp.basic": {
      "name": "Amp",
      "category": "amp",             // amp | cab | drive | tone | delay | reverb | util | io
      "params": [
        { "id": "voice", "name": "Voice", "kind": "choice",
          "choices": ["Clean", "Crunch", "Lead"],
          "min": 0, "max": 2, "default": 1 },
        { "id": "gain",  "name": "Gain",  "kind": "number",
          "min": 0, "max": 10, "default": 5, "unit": "linear" }
      ]
    }
  },
  "channels": [
    {
      "id": "ch1",
      "name": "Channel 1",
      "volume": 0.0,                 // dB, -60..12
      "sources": ["in"],             // "in" | "ch1" | "ch2" | ...
      "blocks": [
        {
          "id": "amp",               // unique within the channel
          "type": "amp.basic",       // registry key
          "name": "Amp",             // display name (from the registry)
          "bypass": false,
          "col": 0,                  // grid column (0..7); absent on legacy presets (defaults to 0)
          "params": { "voice": 0, "gain": 3.5, "bass": 0, "mid": 0,
                      "treble": 0, "level": -5.0 }
        }
      ]
    }
  ],
  "routings": [
    {
      "id": "route1",
      "source": { "channel": "ch1", "block": "amp" },
      "dest":   { "channel": "ch2", "block": "delay" },
      "enabled": true,
      "gain": 1.0
    }
  ]
}
```

`registry` is static for a given engine build; the UI may cache it.

### Cross-channel routings

A routing is **not** an edge. An edge replaces a block's wire; a routing taps a
block's output and *additionally* feeds a block in another channel, so the
source block's own downstream connection is untouched. Routings are a separate
list on the state and in the saved project file.

- `source` / `dest` are `{ channel, block }`. Both endpoints must exist.
- `enabled` defaults to `true`; `gain` is linear and defaults to `1.0`.
- Refused with `ok: false` and a message: a block routing to itself, a routing
  within one channel, an exact duplicate, or one that would close a feedback
  loop. A refused routing never enters the list.
- Invalid routings in a loaded project are dropped, not applied.

`channels` always has exactly 4 entries, in slot order. A channel is never
added or removed: an unused slot is present with an empty `blocks` array, not
missing from the list.

An empty slot is also *cheap*. It is absent from the compiled graph, so it has
no node to address: `setMasterVolume` on an empty channel is accepted and is a
no-op on the audio path (the value is stored, and takes effect when a block
lands). When *every* slot is empty the engine runs a bypass graph — input to
output limiter, no blocks — rather than compiling the rig, so an untouched
pedalboard costs no compile and no block instantiation. The first block dropped
into any slot makes the rig compile normally again, on the same connection.

## Commands

All commands are `req` messages. Mutating commands return the full state in
`payload`. Non-mutating commands return their specific payload.

The command set is closed: there is no command that adds or removes a channel.
Every channel-addressed command takes one of the existing slot ids (`ch1`…`ch4`)
and is refused with `no such channel: …` otherwise, so the channel count cannot
be changed from the client.

| Cmd | args | Effect | payload |
|---|---|---|---|
| `getState` | — | — | full state |
| `setParam` | `{channel, block, param, value}` | set one parameter | full state |
| `setMasterVolume` | `{channel, value}` | channel volume (dB) | full state |
| `setInputGain` | `{value}` | global input trim, dB, clamped −60..24 | full state |
| `setMasterOutVolume` | `{value}` | global output volume, dB, clamped −60..12 | full state |
| `setConfig` | `{buffer_frames}` | I/O buffer size, frames, clamped 32..1024 | full state |
| `getConfig` | — | current config + latency | `{bufferFrames, sampleRate, oneWayLatencyMs, roundTripLatencyMs, supported}` |
| `setBypass` | `{channel, block, bypass}` | bypass a block | full state |
| `addBlock` | `{channel, type, name, pos}` | append a block | full state |
| `removeBlock` | `{channel, block}` | remove a block | full state |
| `moveBlock` | `{channel, block, to}` | reorder | full state |
| `setSources` | `{channel, sources}` | channel inputs | full state |
| `createRouting` | `{source: {channel, block}, dest: {channel, block}}` | tap a block into another channel | full state |
| `removeRouting` | `{id}` | delete a routing | full state |
| `setEnabled` | `{id, enabled}` | toggle a routing | full state |
| `setGain` | `{id, gain}` | routing gain (linear) | full state |
| `listRoutings` | — | — | `{"routings": [...]}` |
| `setSource` | `{source}` | riff / live / silence | full state |
| `listPresets` | — | — | `{"presets": [...]}` |
| `loadPreset` | `{name}` | load a preset | full state |
| `nextPreset` / `prevPreset` | – | step to the neighbouring preset in the sorted list (no-op at either end) | full state |
| `savePreset` | `{name}` | save the current rig | full state |
| `deletePreset` | `{name}` | delete a preset | full state |
| `renamePreset` | `{from, to}` | rename a preset | full state |

Errors return `{ "ok": false, "error": "..." }`. A failed mutation never
changes state, so the client can keep its current snapshot.

## Meters

```jsonc
{ "type": "event", "event": "meters",
  "payload": { "in": 0.42, "out": 0.38, "cpu": 12.3, "xruns": 0 } }
```

- `in`, `out`: peak levels, 0..1, held with a fast attack and slow release.
- `cpu`: percentage of one core used by the DSP, measured over the last second.
- `xruns`: cumulative buffer underrun count.
- `bands`: one `{band: "band_1".., gr_db, out_db}` per active band of the
  multiband compressor (gain reduction dB >= 0, output peak dBFS); empty when
  no multiband block is in the rig.

20 Hz is enough for a meter; it is not a control path.

## Tuner

The tuner is a **read-only stream**, like the meters: the client never asks for
it and never enables it. The engine analyses the instrument input continuously
and pushes a reading at 20 Hz to every connected client.

```jsonc
{ "type": "event", "event": "tuner",
  "payload": {
    "active": true,        // false = no reliable note
    "freq": 220.01,        // Hz, 0.0 when inactive
    "note": "A",           // "C"…"B", "" when inactive
    "octave": 3,           // 0 when inactive
    "cents": 0.1,          // −50…+50, signed: negative = flat
    "clarity": 0.94,       // 0…1, how periodic the input is
    "reference": 440.0     // the fixed A4 the note name is derived from
  } }
```

- The reading is **not** part of `state` and carries no `rev`. It changes far
  faster than the rig does, and a reconnecting client must see it immediately.
- It is not affected by the `RIFF` / `LIVE` / `SILENCE` selector: the tuner
  listens to the physical instrument input, not to the selected source.
- There is no `setTuner` command. See `docs/design/instrument-tuner.md`.
- An inactive reading is a valid answer ("play a note"), not an error.

## Real-time safety

- Parameter changes are applied by the **audio thread** between blocks. The UI
  never runs in the callback.
- Structural changes (add/remove/reorder/load preset) compile a new executor
  and queue it; the audio thread swaps at a block boundary. No gap, no glitch.
- The UI process may be killed at any moment. The engine does not depend on it.

## Versioning

The protocol has no version field. Both sides ship together; the state shape
is additive (new fields are ignored by older clients). If a breaking change is
ever needed, it gets a new port and a new `protocol` crate.

## Grid layout

Each channel strip is a row in a grid of 8 columns (0..7). Blocks are laid out
in a column via the `col` field on every block in `state.channels[].blocks` and
via the optional `col` argument to `addBlock` and the required `col` argument
to `placeBlock`. Columns are unique within a channel — a block placed in an
occupied column is refused with `ok: false`.

### `addBlock` with `col`

The `addBlock` command now accepts an optional `col` argument. When present it
places the block directly into that column; when absent the engine picks a
column:

- If the channel is empty, column 0.
- Otherwise the lowest free column to the right of the highest existing column
  (i.e. `max_col + 1`).
- If all columns to the right are full, the lowest free column anywhere.
- If all 8 columns are full, returns `ok: false` with `"channel full (8 blocks)"`.

### `placeBlock` — cross-channel move

Moves a block from one channel to a column in another channel. The source block
is removed from its original channel, and a new entry is added to the target
channel at the given column. The block's id, type, name, and params are preserved.

Args: `{ channel, block, toChannel, col }`. Errors include `no such channel`,
`no such block`, `slot out of range` (col >= 8), `slot occupied`.

### Later-slot rule

A routing (split) must always point to a block in a **later** column.
`createRouting` returns `ok: false` with `"a split must go to a later slot"`
when `dest.col <= source.col`. This is checked in addition to the existing
self-reference, same-channel, duplicate, and cycle checks.

### Prune-on-move

When `moveBlock` or `placeBlock` changes a block's column, every routing whose
source or dest block now has `dest.col <= source.col` is **pruned** (removed).
Similarly, if a block is moved so that both ends of a routing land in the same
channel, that routing is pruned. The prune happens silently — the command
returns the updated full state without an error.
