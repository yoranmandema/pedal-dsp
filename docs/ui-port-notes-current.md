# UI / engine-state audit — current pedal-dsp UI

Audit of the Preact + TypeScript UI in `ui/` and the engine-side Rust state
model, for the grid / drag-and-drop / cable-rendering / rerouting cards.
Deliverable: `docs/ui-port-notes-current.md` (this file). All paths are
relative to the repo root of the card worktree
`/Users/yoran/Documents/Repos/pedal-dsp/.worktrees/t_8f44a111`.

---

## 1. Component tree and file paths

### Entry points
- `ui/src/main.tsx` — renders `<App />` into `#app`, imports `ui/src/tailwind.css`.
- `ui/src/App.tsx` — the shell. Renders (in order): `TopBar`, the column
  `Rack` + `Chain` (in a responsive grid), `Editor`, `TunerModal`,
  `PresetBrowser`, `ModelBrowser`, `AddBlockModal`, `SelectionRing`, and the
  toast. Boot/connecting/error screens are separate JSX branches on
  `error.value` / `ready.value`.
- `ui/src/ui.ts` — shared CSS-class recipes (`navBtn`, `emptyNote`, `ctrlName`,
  `textField`, `modalBackdrop`, `modalShell`, …). Anything used more than once
  lives here; anything used once stays inline.

### Components (`ui/src/components/`)
| File | Role |
|---|---|
| `TopBar.tsx` | Header: input-trim `HeaderKnob`, preset navigator, 8 slot chips, tuner/model buttons, source toggle (riff/live/silence), IN/OUT meters, DSP%, latency readout, buffer-size `<select>`. |
| `Rack.tsx` | Left palette: `BlockList` grouped by category, "Add block" selects the type. |
| `Chain.tsx` | Centre panel: one `ChannelRow` per 4 slot channel, horizontal `.strip` scrollers, `RoutingLines` overlay, drag-ghost, insertion line, routing preview. |
| `ChannelRow` (in Chain.tsx) | Header (channel chip, volume slider, source toggle) + populated/empty `Channel` strip. |
| `Editor.tsx` | Bottom panel: selected block identity + bypass/reset + parameter controls (`Knob`, `ChoiceControl`, `FootswitchControl`, `ExpressionControl`, `ParametricEQ`, `GraphicEqEditor`, `LooperEditor`). |
| `Knob.tsx` | **The base editor knob.** Full details in §5. `MUST REMAIN UNCHANGED` (see constraint K1). |
| `HeaderKnob.tsx` | Compact header knobs (Input, Master). `Knob.tsx` is deliberately left untouched (it references this fact in a comment). |
| `FrequencyKnob.tsx` | Log-frequency variant of `Knob`. |
| `SelectionRing.tsx` | Selection/focus ring overlay drawn above the chain (never inside a clipping ancestor) — see §6. |
| `RoutingLines.tsx` | Cross-channel cable overlay: live preview line, real routing lines, target highlight, floating bar, context menu, Delete key. |
| `BlockList.tsx` | Rack rows: block type entries grouped by category. |
| `PresetBrowser.tsx`, `AddBlockModal.tsx`, `TunerModal.tsx`, `ModelBrowser.tsx` | Modals for presets, adding a block, tuning, and model browsing. |
| `Icon.tsx`, `Icon.tsx` | Icon set. |

### Other UI source files
- `ui/src/blocks/parametric-eq.ts` — parametric-EQ block spec (19 params), band
  structure (`EQ_BANDS`), fixed Q, and the client-side frequency-response curve
  maths (`biquadCoefficients`, `biquadGainDb`, `eqCurve`). Mirroring it is
  intentionally **not** required (the engine's registry is the source of truth);
  this module owns the fixed band structure and the curve the panel draws.
- `ui/src/dnd.ts` — the drag-and-drop contract (BLOCK_MIME, `BlockPayload`).
- `ui/src/touch.ts` — touch drag long-press timing + the window-level touch-drop
  router of a rack drag into a channel.
- `ui/src/graph/eq.ts`, `scale.ts`, `eq.test.ts`, `scale.test.ts` —
  pure graph/panel helpers + unit tests.
- `ui/src/theme.ts` — per-category hue map (`HUES`) + `catStyle`.
- `ui/src/state.ts`, `protocol.ts`, `model.ts`, `selection.tsx`, `ui.ts`,
  `tailwind.css` — see §§2,3,5,6,7.

---

## 2. State management

**Single source of truth, Preact signals (no Redux/Zustand):**
`@preact/signals` ^2.0.3. Everything flows through the bridge; no component
talks to the engine directly.

`ui/src/state.ts` defines the signals, the `Bridge` object, and all mutations.
The full rig state is one snapshot held in a single signal:

| Signal | Type | Purpose |
|---|---|---|
| `state` | `signal<State \| null>` | The rig snapshot (channels, blocks, routings, registry, params). `null` until the first `getState` reply. |
| `meters` | `signal<Meters>` | IN/OUT/CPU/XRUNs, pushed `event: meters` 20 Hz. |
| `looperFrac` | `signal(0)` | Looper playback position 0..1, updated ~10 Hz. |
| `ready` | `signal(false)` | `true` after a snapshot is applied. |
| `error` | `signal<string \| null>` | Boot failure. |
| `notice` | `signal<string \| null>` | Transient command rejection (toast). |
| `selected` | `signal<{channel,block} \| null>` | Selected block. |
| `selectedRouting` | `signal<string \| null>` | Selected cross-channel line (**UI-only**; the engine has no notion of selection). |
| `presetsOpen`, `addBlockFor`, `blockQuery`, `tuner`, `tunerOpen` | various | Modal/picker/tuner UI state. |
| `selectedBlockType`, `touchDragBlock`, `touchDragOver` | various | Keyboard and touch-drag plumbing (siblings `Rack` and `Chain` share these so a touch drag survives the parent/child boundary). |
| Model-browser signals (`modelsOpen`, `modelsView`, `modelsPage`, `modelsResults`, `modelsSelectedFacets`, `modelsBusy`, `modelsRefreshError`, `modelsIndexMeta`, `modelsStale`, `modelsDetailId`, `modelsDetail`, `modelLocal`, `modelsDownloading`, `modelsDownloadError`, …) | | Model browser: list vs grid (`modelsView: "list" \| "grid"`), paging, facets, downloads. |

**Reactivity:** read values directly off `state.value`/`…` (e.g.
`const s = state.value`), or subscribe to small targeted signals. The engine
pushes `event: state` snapshots through `bridge.subscribe`; the UI
`fold()`-s them into `state.value`, dropping any snapshot that is not newer
than the last applied `rev` (out-of-order delivery cannot roll it back; a
**lower** `rev` means the engine restarted and its counter began again — accept
that one).

**Where node positions live:** **nowhere in the UI.** A block is
`NodeDoc { id, block, params, bypass, pos: [f32;2], model }`; `pos` is applied
by the engine when it lays out the compiled graph (`Rig::add_block_as`). The UI
only reads `.pos`-agnostic things (block id, type, name, bypass, params, chain
index). Any grid/cable work must not add a UI-local position store unless the
engine-side `pos` is no longer honoured — that is a design decision for the
new cards.

**Where connections live:** the chain is an **ordered array of blocks** per
channel (`Channel.blocks`); signal edges are the consecutive windows
(`w[0] -> w[1]`). Channel inputs are `Channel.sources` (`"in"`, `"chN"`).
Cross-channel taps are the separate `Rig.routings` list. Nothing about
routing geometry is ever sent to the engine.

**Where channel routing / input sources live:** `Channel.sources` (resolved to
feed the head of each channel in `Rig::resolve_sources`, which treats empty
channels as transparent). Routings are over IPC as `createRouting` /
`removeRouting` / `setEnabled` / `setGain`, applied at engine compile time in
`Rig::apply_routings` (a `util.mix` is inserted in front of the destination
block; a `gain != 1.0` adds `util.gain` first).

---

## 3. UI ↔ engine / backend

### Transport
- **BSC**: WebSocket over loopback TCP, `ws://127.0.0.1:8737` (hard default).
  The engine binds `127.0.0.1` only (IPC, never external). A Vite dev proxy
  maps `/engine` to the same origin; a build-time placeholder is used for the
  self-contained binary. `ui/src/bridge.ts` `createBridge()` picks either
  `WebSocketBridge` (browser/dev) or `PluginBridge` (native shell injects
  `window.__pedal_native`), so the same UI runs under Tauri, in a browser, and
  inside the VST3 plugin. See `docs/UI-ARCHITECTURE.md`.

### Message envelope (docs/IPC-PROTOCOL.md)
- One JSON object per frame; `type` is always present.
- **Client→server:** `{ "type":"req", "id":…, "cmd":…, "args?":… }`.
- **Server→client:**
  - `{ "type":"res", "id":…, "ok":bool, "payload?":…, "error?":… }` — answer to
    a request. **Mutations return the full new state** in `payload`.
  - `{ "type":"event", "event":"state", "payload":… }` — snapshot push
    (after every mutation to every client, requester included).
  - `{ "type":"event", "event":"meters", "payload":{in,out,cpu,xruns} }` — 20 Hz.
  - `{ "type":"event", "event":"tuner", "payload":… }` — 20 Hz, read-only stream.
  - `{ "type":"event", "event":"error", "payload":{message} }`.

### Serialization
- The `Bridge` interface: `request(cmd, args): Promise<unknown>` and
  `subscribe(cb)`. `WebSocketBridge` JSON-parses frames; responses are correlated
  by `id`; `event`s are fan-out to all listeners.
- `protocol.ts` mirrors `docs/IPC-PROTOCOL.md`:
  `State`, `Channel`, `Block`, `Routing`, `RoutingEndpoint`, `ParamSpec`,
  `BlockSpec`, `Config`, `Meters`, `Tuner`, `ServerMessage`.
  The exact snapshot keys are pinned by `engine/tests/protocol_keys.rs`
  (`STATE_KEYS`, `BLOCK_KEYS`, `CHANNEL_KEYS`) — key drift is a compile-time
  silent no-op otherwise, so this test is the canary.

### Commands (closed set, docs/IPC-PROTOCOL.md §commands)
`setParam`, `setMasterVolume`, `setInputGain`, `setMasterOutVolume`,
`setConfig`, `setBypass`, `addBlock`, `removeBlock`, `moveBlock`, `setSources`,
`createRouting`, `removeRouting`, `setEnabled`, `setGain`, `listRoutings`,
`setSource`, `listPresets`, `loadPreset`, `savePreset`, `deletePreset`,
`renamePreset`, `getState`, `getConfig`, `listModels`, `getModel`,
`refreshIndex`, `downloadModel`, `cancelDownload`.

Mutating command replies carry the full new snapshot; non-mutating ones carry
their specific payload (`getConfig`, `listModels`, `getModel`,
`refreshIndex`, `listPresets`, `listRoutings`).

### UI-local state (no engine round trip)
- `selectedRouting`, `selectedBlockType`, `touchDragBlock`, `touchDragOver`,
  `touchDragOver`, `touchDragBlock` — UI-only selections/gestures.
- Model-browser transient UI state (query, facets, sort, paging, download
  progress/errors) — the engine owns the cached index and answers queries; the
  UI holds only the current page/facets/transient download state.

### Service-level note: `rev` and duplicate suppression
Every state snapshot carries a `rev` counter (engine `AtomicU64`, bumped on
**every** mutation — including `setParam`, which never rebuilds the graph).
The UI's `fold()` uses `rev` to drop stale/out-of-order deliveries. Because the
engine broadcasts every mutation to every client **and** returns the state in
the `res` payload, the duplicate is a no-op through `rev`, and the broadcast is
what keeps the UI alive if the reply is lost. This is the single most fragile
part of the sync; any new card must preserve it exactly.

---

## 4. Knob rendering components (exact files, props, SVG/canvas details, styles)

There are **three** knob variants. They share a face/arc/indicator geometry
pattern and a pointer-drag model; `ui/src/components/Knob.tsx` is the base and
**must remain unchanged** (K1). `HeaderKnob.tsx` documents this explicitly in a
comment ("`ui/src/components/Knob.tsx` is deliberately left untouched").

### `ui/src/components/Knob.tsx` (base editor knob)
- **Props:** `name, value, min, max, defaultValue?, unit?, format?, onChange,
  curve? = "linear"`. (`curve: "linear" | "log"`; a log curve maps the arc so
  equal perceptual steps read linearly — used for frequency/time-ish params.)
- **Pointer drag:** `onPointerDown` captures the pointer, records `{y, v}`;
  `onPointerMove` computes `dy / 150 * span` and `onChange(clamp(v + Δ, min, max))`.
  Drag threshold: none — vertical drag always drives the value.
- **Double-click:** resets to `defaultValue`.
- **SVG arc** (SVG, *not* canvas): `<svg viewBox="0 0 64 64" width/height={64}>`
  with two `<circle>`s: the track (`r=30`, `stroke-width=4`, `stroke="#2f323b"`)
  and the indicator (`stroke var(--amber)`, `stroke-linecap round`). Both use
  `pathLength={100}` and
  `stroke-dasharray="75 100"` (75 = the 270° travel window) plus a
  `transform="rotate(135 32 32)"`. The indicator's `stroke-dasharray` is
  `${t * 75} 100`, `t ∈ [0,1]` being the normalised value.
  (SVG presentation attributes must stay kebab-case; Preact preserves that.)
- **Indicator span:** `knob-indicator` — `absolute left-1/2 top-[8.33%]
  h-[41.67%] w-[5.56%] origin-[50%_100%]` with
  `transform: translateX(-50%) rotate(${angle}deg)`; angle = `-135 + t*270`.
- **End-stop pills:** `knob-end lo` (right) and `hi` (left), `text-pico`,
  `tabular-nums`, styled by `endLabel(min/max)` (no unit; the pills sit between
  the ring and the param name and must stay bare numbers).
- **Container:** `knob-unit` / `knob` div with class
  `rounded-full bg-[radial-gradient(circle_at_34%_28%,#454956,#1a1b20_72%)]
  shadow-knob cursor-ns-resize touch-none`, plus `dragging` class while down.
- **Name/value readout:** `knob-name` (uppercase `text-2xs`) over
  `knob-value` (`text-xs`, `tabular-nums`, `min-w-[56px]`).

### `HeaderKnob.tsx` (top-bar input/master knobs)
- Props: `label, value, min, max, defaultValue?, onChange`. `role="slider"`,
  `aria-valuemin/now/max`, full keyboard handling (ArrowUp/Down/Left/Right
  ±0.5, Home/End to min/max).
- SVG arc uses `r=22, stroke-width=5, viewBox 0 0 64 64, width/height=46`,
  `stroke-dasharray="75 100"`, `rotate(135 32 32)` (same 270° window).
- Wrapper div `size-[34px]`; label+readback collapse out below 820 px (`.compact`).
- **KNOB FACE IS SHARED:** the face, arc and indicator geometry deliberately
  mirror `Knob.tsx`; only the data-unit range and compact sizing differ.

### `FrequencyKnob.tsx` (log-frequency knob)
- Props: same as `Knob`. Travel is logarithmic: `t = (log(value) - log(min)) /
  (log(max) - log(min))`; pointer move steps `t` by `dy/150/span` (span = the
  log span) and `onChange(exp(log(min) + t * span))`.
- Adds keyboard decade-fraction stepping (±0.04 `t` per arrow; Home/End min/max;
  Enter/Space reset to default).
- SVG arc `r=30, stroke-width=4` (matching the base `Knob.tsx`), same
  `pathLength=100`, `dasharray="75 100"`, `rotate(135 32 32)`.

**Pattern summary (must not be changed by new cards):**
- rotation is a single CSS `transform` on the indicator (`rotate(${angle}deg)`),
  the arc is an SVG `<circle>` with `pathLength=100` + `stroke-dasharray`,
  `START = 135` and a 270° travel window (`-135`…`+135` deg),
  `RING_R = 30`, `RING_W = 4` in `Knob.tsx`.
- vertical drag = `dy / 150 * span`, clamped to min/max;
  double-click = reset to default.
- touch: `touch-none` on the knob so a finger drag reaches `pointermove`
  instead of scrolling the panel.

---

## 5. Layout, CSS / Tailwind setup, theme tokens

- **CSS framework:** Tailwind v4 (`@tailwindcss/vite` plugin, v4.3.5) on top of a
  single brand stylesheet `ui/src/tailwind.css` that imports `tailwindcss`
  (`@import "tailwindcss"`) and adds everything the utilities cannot express.
- **Design tokens:**
  - Breakpoints (mobile-first; a class with **no** prefix is the phone layout):
    `tab` >= 700 px (`--breakpoint-tab: 700px`), `desk` >= 840 px
    (`--breakpoint-desk: 840px`). A `compact` variant exists at < 820 px.
  - Palette: `--color-bg #101116`, `--color-panel #17181d`, `--color-panel-2`,
    `--color-line #2a2d36`, `--color-line-soft`, `--color-line-hi`,
    `--color-text #e8eaef`, `--color-muted #8b909c`, `--color-amber
    #e8a33d` (+ `amber-dim`, `amber-hi`, `amber-lo`, `amber-edge`,
    `amber-ink`), `--color-ok #4a8f5c`, `--color-danger*` (error), surfaces
    `--color-inset #121318`, `--color-well #14151a`, `--color-surface
    #1a1b21`, `--color-raised #22242b`, `--color-raised-2 #26282f`.
  - Type scale: pico 6.5 px → 3xs 8 px → 2xs 9 px → xs 10 px → sm 11 px →
    base 12 px → md 13 px → lg 14 px → xl 16 px → 2xl 28 px.
  - Radii: xs 3, sm 4, key 5, md 6, lg 7, card 9, xl 12 px.
  - Shadows: card / knob / glow / drag / toast / modal.
  - `--font-sans`/`--font-mono`.
- **Layout variables** (set on `:root` in the `@layer base` of
  `ui/src/tailwind.css`):
  ```
  --ui-touch 44px     minimum hit target (WCAG 2.5.8)
  --ui-ctrl   72px     narrowest control cell floor
  --ui-ctrl-max 96px   widest a control cell may grow
  --ui-ctrl-row  calc(7*var(--ui-ctrl-max) + 6*var(--spacing)*6)
                      widest the controls group may be
  --ui-bar    128px    desktop bar height
  --ui-topbar-h 58px
  --ui-id-w   clamp(144px, 16vw, 200px)   identity (rack) column
  --ui-knob   clamp(54px, 4.2vw, 62px)    editor knob face
  ```
  (Spacing increments are the Tailwind numbers: 1=4, 2=8, 3=12, 4=16, 6=24 px;
  nothing else is used — the previous audit found other steps in play and that
  is what broke alignment.)
- **Extra utilities in `tailwind.css`:** `@utility cat-scope`
  (`--cat`/`--cat-soft` derived from the card's `--cat-h`), `@utility
  bg-stage`, `@utility amber-key`, and element CSS for `.block-out` (the
  12×12 amber output dot — *the only* routing drag source, because the existing
  output dot is a CSS `after:` pseudo-element and cannot take pointer events),
  `.block-route-preview`, `.block-card.routing-target`,
  `.expression-slider` (full-width custom range input).
- **Category colour:** one hue per `Category` (modelling 268 violet, distortion
  10 ember red, tone 42 gold, dynamics 152 green, time 206 blue, modulation
  306 magenta, pitch 186 cyan, utility 222 slate blue, I/O 0 neutral, split 96
  olive, control 24 orange) applied as inline `--cat-h` via `theme.ts`
  `catStyle`. A card's accent, rack chips, and the selection ring all derive
  from `--cat-h`/`--cat`/`--cat-soft`.
- **Custom (non-Tailwind) CSS** (kept alongside the component):
  `ui/src/components/LooperEditor.css` (looper transport panel: banner, loop
  progress, triggers, knob row) and
  `ui/src/components/parametric-eq.css` (parametric-EQ panel: real range
  sliders, units, keyboard-operable, touch-first). The project keeps **one**
  global stylesheet for shared components; these two files stay with their
  implementations so they never touch the global file.
- **Responsive behaviour:** the chain is a 1-column grid that becomes
  2-column (`tab`) and narrows the bar (`desk`); the editor stacks on phone;
  the rack is a palette strip (`phone:max-h-[200px]`) on small screens.

---

## 6. Existing drag / interaction code

### `ui/src/dnd.ts` — the DnD contract
- `BLOCK_MIME = "application/x-block-type"`; `BlockPayload { type, name,
  config? }`.
- **HTML5 drag** (mouse): rack rows call `startDrag`/`endDrag` on
  `dragstart`/`dragend`; `dataTransfer.setData(BLOCK_MIME, …)` plus a
  `text/plain` mirror. `lastBlockDrag.dropped` records the outcome.
- **Pointer gesture** (touch *and* in-channel reorder): `pointerdown` +
  `pointermove` + `pointerup`, with `DRAG_THRESHOLD_PX = 4` for a mouse; a plain
  click stays a click.
- **Touch long-press:** `LONG_PRESS_MS = 350`; `LONG_PRESS_SLOP_PX = 10`
  (moving farther cancels the drag so a scroll is never a drag).
- `decodeBlockPayload` is lenient about field names (`type`/`typeId`/`blockType`,
  `name`/`label`), `rejectDrop` returns a `DropRejection` ("no-payload" |
  "bad-type" | "unknown-type") testable without a `DataTransfer`.

### `ui/src/touch.ts` — touch drag + drop router
- `useTouchDrag({ onStart, onMove, onEnd })` returns the four pointer handlers;
  the element needs `touch-action: none` (`touch-none` class).
- `useRackLongPress()` — rack-row gesture: long-press publishes the block on
  `touchDragBlock` and marks the row `.dragging`; moving out of the slop
  cancels before the timer fires.
- `useTouchDropRouter()` — **window-level routing:** while `touchDragBlock` is
  set, `pointermove` resolves the channel under the finger; each channel
  registers a `preview(slot)`/`commit(payload, slot)` target via
  `registerTouchTarget(id, target)` and `registerTouchChannel(id, el)`.
  `touchDragOver` is set while hovering a channel.

### `Chain.tsx` — reorder + routing
- Card reorder: `useTouchDrag` owns the gesture; `cardPress` on each
  `block-card` (`tabIndex={0}` so Enter/Space selects, `onKeyDown` handles
  Enter/Space on the card itself but lets inner buttons handle themselves);
  `drag?.id === b.id` adds `.dragging`, a `block-placeholder` shows the
  insertion slot, and a `drag-ghost` follows the pointer.
- Cross-channel routing drag: `onPointerDown` on `.block-out` (`startRouteDrag`),
  `onPointerMove` (hit-test the target card, add `.routing-target`, redraw the
  preview line via `routeCurve`), `onPointerUp` (re-hit-test at the release
  point, `createRouting` on a valid target). `e.stopPropagation()` on the
  handle keeps the card-reorder drag untouched. A `ResizeObserver` re-measures
  the preview.
- `insertBlock(channel, slot, payload)` (in `state.ts`) adds the block then
  moves it into place (the engine's `addBlock` appends).

### `RoutingLines.tsx` — cable rendering
- A sibling of the channel rows inside `.chain-scroll` (the positioning
  context), `absolute inset-0 z-10 pointer-events-none`, so lines are never
  clipped by the horizontal strip scrollers and scroll with the panel.
- Every line is measured **live from the DOM** (`getBoundingClientRect` of the
  two cards relative to the overlay) — no routing geometry is sent to the
  engine. Lines are drawn as dashed amber S-curves with an arrowhead at the
  destination; a disabled routing renders as a faded line with no arrowhead.
- The selected routing gets a floating action bar (Delete / Enable-Disable /
  dB gain slider) and a context menu; `Delete` removes the selected routing;
  arrow keys/context menu are the other controls. `SelectionRing.tsx` draws the
  selection/focus ring on top (`z-30`, `pointer-events:none`).
- **Constraint:** the only drag source for a routing is the real `.block-out`
  element; the existing output dot is a `after:` pseudo-element and cannot take
  pointer events — any new cable behaviour must reuse that handle or document
  why it cannot.

---

## 7. Tests and build tooling

### UI (`ui/package.json`)
- `build`: `tsc --noEmit && vite build` (typecheck then bundle).
- `dev`: `vite`; `dev:e2e`: `VITE_ENGINE_URL=ws://127.0.0.1:9400 vite
  --port 5273 --strictPort`; `preview`; `test:unit`: `node
  --experimental-strip-types --test src/graph/scale.test.ts src/graph/eq.test.ts`;
  `cypress:open`/`cypress:run`; `test:e2e` (starts the engine and runs Cypress).
- Dependencies: `preact` ^10.27 + `@preact/signals` ^2.0.3; dev
  (`@preact/preset-vite`, `@tailwindcss/vite`, `tailwindcss` ^4.3.5,
  `typescript` ^5.9, `vite` ^7.1.0, `cypress` 16.3.0, `ws`).

### Tests
- **UI unit tests:** `ui/src/graph/scale.test.ts`, `eq.test.ts` (pure scale /
  EQ helpers, run under Node's strip-types test runner — no browser needed).
- **Engine integration tests** (`engine/tests/*.rs`): `protocol_keys.rs`
  (pins the exact snapshot/block/channel key sets + that `rev` advances on
  param-only mutations), `routing.rs`, `channel_slots.rs`, `header_volume.rs`,
  `headless_smoke.rs`, `tuner.rs`, `ui_off_audio_path.rs` (the UI must never
  touch the audio path). The engine itself is a library + process;
  `pedal-engine` binary binds loopback WebSocket `8737`.

### Build tooling notes
- Vite + Preact preset + Tailwind v4 plugin; the shell embeds the built assets
  at build time (Tauri `wry`+`tao`, or the Pi 5 WebKitGTK app).
- `ui/dist/` is generated and ignored — never commit build output.
- A self-contained binary needs `--features custom-protocol`; plain
  `cargo build` is always a dev build that loads a Vite dev URL.

---

## 8. Engine-side Rust types that define the signal graph / routing

### `dsp-graph` crate (`graph/`)
- `graph/src/types.rs` — `ParamSpec { id, name, unit: Unit, min, max, default,
  curve: Curve }`, `Unit` (Decibels, Hertz, Milliseconds, Percent, Semitones,
  Cents, Ratio, Linear, Seconds, **Choice**), `Curve` (Linear, Log, AmpGain),
  `PortSpec`, `PortKind`, `Category`, **`BlockSpec`**, `BlockError`.
- `graph/src/doc.rs` — `Doc`, `EdgeDoc`, `NodeDoc`, `Registry`, `Graph`,
  `Routing`, `RoutingEndpoint` (and `NodeDoc.pos: Option<[f32;2]>`,
  `model: Option<...>`). `Doc::build_in`, `NodeDoc`, `EdgeDoc` tie the rig to
  the executor.
- `graph/src/registry.rs`, `blocks.rs`, `block.rs`, `executor.rs` — the block
  trait, built-in blocks, registry of specs, the no_std executor.
- The UI's `registry` snapshot is built from the engine's `dsp-graph` registry:
  `Registry` exposes `specs()` and `get(&block) -> Option<&BlockSpec>`; the
  server serialises each spec as `{ id, name, category, params[] }` where each
  param is `{ id, name, kind ("number" | "choice"), min, max, default, unit,
  choices? }`. Use that shape when adding new blocks or changing params —
  `engine/tests/protocol_keys.rs` will fail if the top-level keys drift.

### `engine` crate (`engine/src/`)
- **`channels.rs` — the channel model the UI edits:**
  - `Channel { id, name, blocks: Vec<NodeDoc>, sources: Vec<String>,
    master_volume, master_ceiling }` — 4 fixed slots, `CHANNEL_COUNT = 4`.
  - `Rig { name, channels, routings: Vec<Routing>, input_gain, master_volume }`.
  - `Routing { id, source: RoutingEndpoint, dest: RoutingEndpoint, enabled,
    gain (linear) }` — a routing is a **separate** list, not an edge: it taps a
    block's output and additionally feeds a block in another channel; the source
    keeps its own downstream connection.
  - `Rig::to_doc()` (lays channels out as a graph, skips empty slots, one
    summing node, empty channels are cheap), `Rig::from_doc()` (reads a saved
    project back), `Rig::add_block_as` (id derived from display name,
    `pos` is `Option<[f32;2]>` — the engine-side layout input), `Rig::move_block`,
    `Rig::remove_block` (also drops any routing touching the block), `Rig::
    apply_routings` (compiles at graph-build time: inserts `util.mix` in front
    of the destination, optional `util.gain` for `gain != 1.0`, re-wires),
    `Rig::validate_routing` (no self-route, no same-channel route, no duplicate,
    no feedback cycle via `routing_creates_cycle`), `Rig::set_enabled` (refuses
    to enable a cycle), `Rig::set_gain` (linear), `Rig::list_routings`.
  - Constants: `IN = "<in>"`, `OUT_MASTER_ID = "out__master"`,
    `ROUTING_MIX_PREFIX = "__routing_"`,
    `ROUTING_GAIN_PREFIX = "__routing_gain_"`.
- **`server.rs` — the IPC server:** `Shared { rig: Mutex<Rig>, store: Mutex<Store>,
  handle: Handle, registry: Arc<Registry>, source: AtomicU8, preset,
  buffer_frames, sample_rate, clients, bypass: Mutex<Option<Graph>>, rev:
  AtomicU64, models: Arc<Models> }`. `snapshot()` produces the state JSON;
  `serve()` binds loopback TCP 8737 and runs `handle_client`, `meters_loop`
  (20 Hz), `tuner_loop` (20 Hz). `Snapshot shape (protocol.ts / IPC
  protocol md): rev, preset, presets, source, inputGain, masterOutVolume,
  bufferFrames, sampleRate (read-only), oneWayLatencyMs, roundTripLatencyMs,
  registry, channels, routings`.
- **`audio.rs`** — the executor/`AudioThread` (real-time audio path; never
  touched by the UI). `host.rs` — device/pick/latency (`Config`,
  `pick_input`, `pick_output`, `one_way_latency_ms`,
  `round_trip_latency_ms`).
- **`store.rs`** — preset directory (JSON files), `Store` + `slugify`.
- **`headless.rs`, `tuner.rs`, `models.rs`, `model_index.rs`** — headless
  simulator, tuner, model-browsing index, and the `listModels`/`getModel`/
  `refreshIndex`/`downloadModel`/`cancelDownload` service.

### Data-model → UI mapping
- `Channel.sources` (UI: `Channel.sources` read-only) → the channel's input
  selector.
- `Channel.blocks[i].id` (unique within the channel; `util.master` ids are
  `chN__master`) → block identity, `data-block-id`.
- `Channel.blocks` ordering → the in-channel left-to-right signal flow and the
  strip slot index (`data-slot`).
- `Rig.routings` → cross-channel cables drawn by `RoutingLines.tsx`.
- `Registry`/`BlockSpec` (engine) → the block palette (rack) and the per-block
  parameter controls (editor).
- `rev` → snapshot ordering guard in `state.ts` `fold()`.
- `sampleRate` / `bufferFrames` / latency → read-only display + the `setConfig`
  buffer control.

---

## 9. Extension points and constraints

### Grid (e.g. model browser / chain list)
- **Already present:** the model browser already has a list↔grid switch via
  the `modelsView` signal (`"list" | "grid"`) with `appendModelsPage` for grid
  pagination, and the rack rows are currently a single flat list (`BlockList`).
  Grid styling already exists in the CSS (`modelsView === "grid"` is not yet
  wired to a grid layout — that is new code).
- **Extension points:** wire `modelsView` in `ModelBrowser.tsx` to CSS grid
  rows (the model record list is the only place a grid is asked for); do not
  change the chain, which is intentionally a fixed-width horizontal strip.
- **Constraints:** the block/browser payload is a flat list of `ModelRecord`;
  grid rows must reuse the existing `ModelRecord` contract, no new network
  shape. Chain-card slot indices (`data-slot`, `slotAtFor`, `boundaryX`,
  `insertBlock`) are index-based and strip-scrollable — a grid chain would
  require a separate layout/measurement path and is **not** part of the grid
  ask.

### Drag and drop (already implemented; extend)
- **Seam:** `dnd.ts` (payload contract + disposition), `Chain.tsx` (browser
  drop on empty/populated channels + `insertBlock`), `touch.ts` (rack long-press
  → channel commit), `state.ts` `insertBlock` (add then move into slot).
- **Constraints:** HTML5 DnD never fires on touch — all touch paths use the
  pointer gesture in `touch.ts` with `touch-action: none`. A dropped payload is
  validated against the engine registry (`rejectDrop`); refused drops are
  warnings, never thrown. `moveBlock`'s index is "after removal"; the UI's
  `insertBlock` already compensates. Any new drop target must register via
  `registerTouchTarget`/`registerTouchChannel` and call `insertBlock` — do not
  invent a second insertion path.

### Cable rendering (already implemented; extend for new cable types)
- **Seam:** `RoutingLines.tsx` (all geometry measured live from the DOM; no
  engine round trip), `.block-out` (only routing drag source), `RoutingLines`
  floating bar + context menu + `Delete` key, `SelectionRing` overlay.
- **Constraints:** cables must live in a positioned, never-clipping context
  (the current `.chain-scroll` absolute overlay, `z-10`), and must not be
  clipped by the horizontal `.strip` scrollers (they are siblings, not children).
  The existing output dot is a `after:` pseudo-element and cannot take pointer
  events — any new drag surface must reuse `.block-out` or a real element.
  Adding **in-channel** cable routing would be a new drawing phase and a new
  engine concept (`apply_routings` already handles cross-channel taps); it is
  **not** part of the current ask and should be pinned down before building.

### Rerouting (already implemented; extend)
- **Seam:** `createRouting` / `removeRouting` / `setEnabled` / `setGain`
  (engine commands), `state.ts` wrappers, `RoutingLines.tsx` drawing + bar +
  menu; engine `Rig::create_routing` / `set_enabled` / `set_gain` (the engine
  validates and applies at compile time).
- **Constraints:** engine rules that cannot be softened without design: no
  self-route, no same-channel route, no duplicate, no feedback cycle
  (`routing_creates_cycle`); a disabled routing stays in the list and project
  but contributes no node. `RoutingEndpoint { channel, block }` is the wire
  shape; `gain` is **linear**. The UI's `selectedRouting` is UI-only — the
  engine never receives selection state. Any new rerouting affordance must
  reuse these commands; do not send routing geometry to the engine.

### Knobs (`Knob.tsx` etc.)
- **K1 — MUST REMAIN UNCHANGED:** `ui/src/components/Knob.tsx` (and its geometry:
  `RING_R=30`, `RING_W=4`, `START=135`, 270° window, `pathLength=100`,
  `stroke-dasharray="75 100"`, indicator `rotate(${angle}deg)`, vertical drag
  `dy/150 * span` clamped, double-click reset). `HeaderKnob.tsx` documents this
  explicitly. When adding new block types/params, extend via the existing
  `Knob` + `ChoiceControl` + `FootswitchControl` + `ExpressionControl`
  sub-components in `Editor.tsx`; do not build a second knob.

### CSS/theme
- Tokens live in `tailwind.css` (`@theme`, `:root` layout variables, custom
  utilities); category hue in `theme.ts`. New components should use the
  `--ui-*` variables and the existing colour/radius/shadow tokens, not new
  magic numbers. The looper and parametric-EQ panels are the two global-CSS
  exceptions and should stay that way.

### Points of failure to preserve
1. `rev` ordering guard in `state.ts` `fold()` (any new snapshot consumer must
   use `rev`, not a timestamp).
2. Duplicate suppression: the engine replies to mutations **and** broadcasts;
   the `res` reply's state is identical to the broadcast, so `rev` turns the
   duplicate into a no-op. Do not change this without a migration plan.
3. `State`/`Channel`/`Block`/`Routing`/`RoutingEndpoint` key sets are pinned by
   `engine/tests/protocol_keys.rs`; changing a key name (e.g. the earlier
   `input_gain`→`inputGain` drift) fails that test, not a user's UI. All new
   fields must be additive only, and the Rust snapshot and `protocol.ts` must
   stay in lockstep.
4. The engine audio path is never touched by the UI — keep the bridge as the
   only seam; `ui_off_audio_path.rs` guards this.

---

## 10. Summary of what the engine would need to know (if a card asks)

- The engine does **not** store or receive block positions; layout input is
  `NodeDoc.pos: Option<[f32;2]>` applied once at compile. Any new UI grid must
  either keep using engine `pos` or define a new engine-side layout input.
- Routings are compile-time graph additions (`apply_routings`), not edges;
  `gain` is linear; `util.mix` + optional `util.gain` are inserted at the
  destination. A "reroute" edit is therefore a delete + recreate, not a wire
  move, and the engine refuses cycles.
- The channel count is fixed at 4; channels are slots, never added/removed.
- Snapshot/contract keys are pinned in `engine/tests/protocol_keys.rs`.
