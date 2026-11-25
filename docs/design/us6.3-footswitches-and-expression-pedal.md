# US-6.3 Footswitches and expression pedal

**Design card (parent: US-6.3).** Slices:
- `t_3f53a306` — hardware interface spec + firmware interface design (owner:
  pedal-builder)
- `t_347b52f6` — engine/firmware/UI implementation + stomp-without-dropouts test
  (owner: pedal-builder)

Goal (product owner): **hardware control of preset up/down and an expression
pedal input.** Acceptance: **stomping changes presets without audio dropouts.**

---

## 1. What is already built (so the spec does not re-derive it)

| Area | Reality |
|---|---|
| Engine IPC | `docs/IPC-PROTOCOL.md`; commands `getState`, `setParam`, `loadPreset`, `listPresets`, … |
| Preset list | `engine/src/store.rs` — sorted by filename, `Store::presets()` |
| Preset load | `engine/src/server.rs::load_preset` — swaps the rig and recompiles at a **block boundary** (audio thread never stops) |
| UI preset stepping | `ui/src/components/TopBar.tsx::step(-1)/step(1)` → `loadPreset(s.presets[next])` |
| Headless test template | `engine/tests/headless_smoke.rs` — spawns `pedal-engine --headless --port P --bench 3`, drives commands over WebSocket, asserts `xruns == 0` |
| Expression/control concept | `docs/PRODUCT-MODEL.md` §3.1 (`Control` port), §8 (control bindings, not MIDI channels), `docs/ANALOG-FRONTEND.md` §7 |
| Footswitch wiring | `docs/ANALOG-FRONTEND.md` §7 — footswitch on the audio interface's footswitch input, debounced on the host (10–20 ms) |

Nothing below re-derives the engine or the bridge: it *adds* the missing
commands and the hardware binding onto that foundation.

---

## 2. Hardware interface

### 2.1 Footswitch (digital, momentary)

- **Two** physical footswitches: **FS_U** (preset up) and **FS_D** (preset
  down). One switch cannot carry both directions without a mode toggle, which
  is slower to find under stomping.
- Wiring (Pi 5 target): each switch is a momentary push-to-close on a GPIO,
  with a pull-up resistor to 3V3 and a 1 kΩ series resistor on the GPIO input;
  GPIO reads `1` released, `0` pressed.
- **Debounce on the host** (10–20 ms, per `docs/ANALOG-FRONTEND.md` §7), not in
  the audio callback.
- Transmission to the control-binding layer: **edge events** at ~100 Hz
  (10 ms poll), i.e. one `fs_up` / `fs_down` event per press/release. The host
  must never allocate here.

### 2.2 Expression pedal (analog, continuous)

- TRS jack: **tip = wiper**, outer pins **3V3** and **GND**. 10 kΩ linear
  potentiometer, 100 nF across the ends, 1 kΩ series resistor on the ADC pin
  (per `docs/ANALOG-FRONTEND.md` §7).
- Mapping: heel = 0 V → value `0.0`; toe = 3 V3 → value `1.0`. Read at 100 Hz,
  one 32-bit integer per poll (no float in the hot path), no allocation.
- On the **Mac-first** phase, the same logical input is produced by the UI's
  control-binding layer (on-screen slider + keyboard: F1 = FS_U, F2 = FS_D,
  E = expression). The engine sees the identical commands; hardware is a
  later binding on the same interface. This is what lets "stomping without
  dropouts" be verified on the Mac before any Pi hardware exists.

### 2.3 Control-binding contract (the shared interface)

A binding is a 4-tuple emitted by the host/firmware and consumed by the UI:

```
(input, target, range 0..1, curve)
```

- `input`: `fs_up` | `fs_down` | `expression`
- `target`: an engine command (`nextPreset`, `prevPreset`) or a block param
  id (`time.pitchdelay.rate`, `time.reverb.mix`, …)
- `range 0..1`: the host's normalised control value (debounced, edge-detected
  for switches)
- `curve`: `linear` (default) or the block param's declared curve
  (`Curve::Log` for frequencies, `Curve::AmpGain` for dB-style knobs)

The UI renders **one control per binding**; the host computes the target
value and sends it as a standard command over the existing WebSocket bridge
(`setParam` for params, `nextPreset`/`prevPreset` for presets). No new
engine IPC surface is introduced for expression — `setParam` already applies
at the next block boundary, which is exactly the same smooth path the
widgets use.

**CPU budget:** host-side debounce + 100 Hz poll + edge detection + one
`setParam`/`nextPreset` per event. Zero allocation in the audio callback, and
the whole loop is measured in the headless stomp test (`meters.cpu`).

---

## 3. Engine additions (implementation card `t_347b52f6`)

### 3.1 New IPC commands — `nextPreset` / `prevPreset`

File: `engine/src/server.rs`, inside `fn apply(sh, cmd, args)`.

```
req { "type":"req", "id":N, "cmd":"nextPreset" }
req { "type":"req", "id":N, "cmd":"prevPreset" }
```

Semantics (`Store::presets()` is the sorted filename list; current preset is
`sh.preset`):

- At a bound (first/last preset): no-op — the reply is still the current full
  state; nothing is loaded, nothing is rebuilt.
- Else: load the neighbour with the existing `load_preset` machinery, i.e.
  `Cmd::Swap` at the next block boundary → **no audio dropout, no gap, and
  the UI's `rev` bumps exactly once** so `ui/src/state.ts` keeps the display
  in sync.

Error handling: any failure returns `{ "ok": false, "error": "..." }` and
changes nothing. `args` is ignored.

Reference shape for the arms (mirror `loadPreset` in the same file):

```rust
"nextPreset" => {
    let names = sh.presets();
    let cur = sh.preset.lock().unwrap().clone();
    if let Some((i, _)) = names.iter().position(|n| *n == cur) {
        if i + 1 < names.len() {
            load_preset(sh, &names[i + 1])?;
        }
    }
}
"prevPreset" => {
    let names = sh.presets();
    let cur = sh.preset.lock().unwrap().clone();
    if let Some((i, _)) = names.iter().position(|n| *n == cur) {
        if i > 0 {
            load_preset(sh, &names[i - 1])?;
        }
    }
}
```

### 3.2 No new audio-thread commands

`nextPreset`/`prevPreset` are plain IPC commands that end in `load_preset`
→ `rebuild` → `Cmd::Swap`. The audio thread never sees them directly; the
Swap is queued and applied between blocks, which is the established
drop-out-free path (`engine/src/audio.rs` "Graph swaps").

### 3.3 Headless stomp test — `engine/tests/stomp_presets.rs`

Template: `engine/tests/headless_smoke.rs` (same spawn, same WebSocket
client, same bench window). The new test:

1. Spawns `pedal-engine --headless --port P --bench 3` with `PEDAL_PRESETS`
   pointing at the repo's `presets/` dir.
2. Connects a WebSocket client; asserts the greeting snapshot is preset
   `04-rainbow` and `event: meters` reports `xruns == 0`.
3. Sends `nextPreset` 10 times; asserts the `event: state` snapshot walks the
   sorted list `00-blank → 07-nam-od808` and each reply is `ok: true`.
4. Sends `prevPreset` 10 times; asserts it walks back to `04-rainbow`.
5. Asserts `xruns == 0` in every `event: meters` received across the whole
   run (no underruns while stomping), and `rev` advanced monotonically in
   every `event: state`.
6. Prints the bench line as the evidence (`callbacks`, `frames`, `xruns`,
   `cpu`, `realtime`).

The existing `engine/tests/headless_smoke.rs` stays untouched.

---

## 4. UI additions (implementation card `t_347b52f6`)

### 4.1 Preset bank display (TopBar)

`ui/src/components/TopBar.tsx` keeps the existing `‹`/`›` stepping (they are
the UI-side reference for the hardware binding). Add a read-only "footswitch"
state read-out next to the preset name:

- `FS: —` (inactive), `FS: U` (FS_U pressed, debounced), `FS: D` (FS_D
  pressed), `FS: E` (expression pedal at toe). Rendered as small amber
  pill(s); nothing below this area must move when it appears.

### 4.2 Controls panel (new component, e.g. `ui/src/components/FootswitchPanel.tsx`)

A small panel (opened from the TopBar's preset browser or a "Controls" tab)
with:

- **Footswitch assignment** selector: `-` (disabled) | `Preset up` | `Preset
  down` | `Chain on/off` — default `-`. (Scope of US-6.3 = preset up/down;
  the other two are named for a later slice.)
- **Expression** section: a continuous 0..1 slider labelled with the bound
  target; default target `time.pitchdelay.rate` when that block exists in
  the current rig (otherwise "no target"), plus a "bind" dropdown listing
  all `params` from `state.value.registry` (the same registry the block
  browser uses).
- A **Test** button that simulates FS_U/FS_D/expression locally (keyboard
  F1/F2/E) so the feature is verifiable on the Mac with no hardware.

### 4.3 State + actions

`ui/src/state.ts` adds:

- `footswitch: { up: boolean; down: boolean; expression: number }` (0..1,
  debounced 10 ms at the UI layer)
- `footswitchAction: "preset-up" | "preset-down" | null` (the binding the
  panel chose)
- actions `nextPreset()` / `prevPreset()` that call the bridge (reuse
  `loadPreset` on `s.presets[next]`)
- the existing `loadPreset` stays the single engine entry point; no new
  commands from the UI side.

### 4.4 Binding wiring

Feed the simulated/hardware controls into the existing actions:

- `step(-1)` / `step(1)` (TopBar) become the *actions* bound by the panel;
  the footer display shows the debounced `footswitch` state;
- expression slider → `setParam(master or target channel, bound block, param,
  expression)` (or the bound target's param) — the same `setParam` path the
  knobs already use, so it inherits per-block curve smoothing and the
  block-boundary application already in the engine.

---

## 5. Acceptance checks (verify before the card can be considered done)

| # | Check | How |
|---|---|---|
| C1 | Stomping changes presets: 10× `nextPreset` walks the sorted preset list by one index each time | `stomp_presets.rs` step 3 + UI display |
| C2 | No audio dropouts: `xruns == 0` across the whole stomp run, while `source: "riff"` | `stomp_presets.rs` step 5 / meters event |
| C3 | Stomping while audio is active (riff) shows no gap or silence | bench window + meters `xruns` |
| C4 | `prevPreset` walks back to the starting preset | stomp test step 4 |
| C5 | Display keeps syncing: every reply/broadcast carries the new `preset` and a newer `rev` | `state.ts` `fold` + TopBar |
| C6 | Expression maps continuously to the bound param: set expression 0→1 and the bound param moves 0→1 along its declared curve | UI Controls panel test button + state snapshot |
| C7 | Headless engine boots and serves the UI on an ephemeral port (no Scarlett needed) | `stomp_presets.rs` step 1 |

---

## 6. Open products decisions (this card does not decide)

- Which parameter the expression pedal controls by default — recommend
  `time.pitchdelay.rate` for the reference rig (05-endless-rise); the binding
  panel makes any param selectable.
- Whether a footswitch should double as a "chain on/off" — out of scope for
  US-6.3; the selector exists for the later slice.
- Pi pinout and ADC channel numbers — hardware-specific, documented on the
  card `t_3f53a306` so builders can choose per board.

---

## 7. Files touched (for the implementers)

**Engine** (`engine/src/server.rs` — `apply()` arms `nextPreset`/`prevPreset`;
`engine/tests/stomp_presets.rs` — new headless test; `engine/src/store.rs` —
no change, `Store::presets()` is already sorted).

**UI** (`ui/src/state.ts` — `footswitch` signal + `nextPreset`/`prevPreset`
actions; `ui/src/components/TopBar.tsx` — footswitch state read-out; new
`ui/src/components/FootswitchPanel.tsx` — controls panel + binding test).

**Firmware/host** (later slice; for `t_3f53a306`): GPIO + ADC read loop at
100 Hz, 10–20 ms debounce, edge events, no allocation in the audio path;
binding table `(input, target, range 0..1, curve)`; Pi pinout per its
schematic.
