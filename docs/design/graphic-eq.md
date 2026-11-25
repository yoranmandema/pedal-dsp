# Graphic EQ block + reusable graph (curve-editor) component

**Status:** design. Card: `t_f88ef464` (this card). Children: the engine
implementation card and the UI components card.
**Author:** pedal-architect.

---

## 0. What ships

1. A new audio block, **`tone.graphic_eq`** (10-band, ISO-centre peaking EQ),
   built on the existing `pedal_dsp::biquad` primitives — no new DSP.
2. A **reusable `<Graph<T>>` curve-editor component** (Preact + TypeScript) in
   `ui/src/components/Graph.tsx`, feature-agnostic. The same component serves:
   - the Graphic EQ frequency response + 10 draggable band handles/sliders,
   - a waveshaper transfer curve (x/y in [-1,1]),
   - a multiband compressor gain-reduction display (crossover markers,
     per-band GR/threshold).

The engine and UI parts are two separate build cards (section 4), each with its
own acceptance checks.

---

## 1. Engine: the `tone.graphic_eq` block

### 1.1 Block identity

| | |
|---|---|
| id | `tone.graphic_eq` |
| name | `Graphic EQ` |
| category | `Tone` |
| ports | `in` (audio) → `out` (audio) |
| filter topology | series cascade of peaking biquads, no inter-band feedback |
| DSP primitive | `pedal_dsp::biquad::Biquad` (RBJ cookbook) — **no new primitive** |

### 1.2 Band count and centre frequencies

**Decision: 10 bands, ISO 1/1-octave centres between 20 Hz and 20 kHz.**

| Band | Centre (Hz) |
|---|---|
| 1 | 31.5 |
| 2 | 63 |
| 3 | 125 |
| 4 | 250 |
| 5 | 500 |
| 6 | 1000 |
| 7 | 2000 |
| 8 | 4000 |
| 9 | 8000 |
| 10 | 16000 |

Rationale: 10 bands is the classic guitar/singer EQ shape and half the filter
count of a 31-band room treatment; every band is one biquad, so 10 biquads is
a tiny per-block cost on the Pi 5. The block is parameterised by
`N_BANDS` + a centre table, so a 31-band 1/3-octave variant is the same code
with `N_BANDS = 31` and an ISO 1/3-octave table — no engine change, only the
table and a compile-time constant.

### 1.3 Filter topology

Each band is a single **peaking biquad** (`BiquadKind::Peak`) with
**Q fixed at 1.0** and its centre frequency taken from the table.
Bands are wired in **series (cascade)**: band 1 → band 2 → … → band 10 →
output. There is no feedback between bands and no summing node: the cascade
is exactly one node's input→output path, like the existing `ToneStack`
(`graph/src/blocks.rs`).

**Decision: Q fixed at 1.0.** No per-band Q control in v1. Bandwidth is the
documented shaping character of the EQ; per-band Q is a later card.

**Decision: gain is applied *through* the biquad**, not as a post-gain.
Each band's coefficients are the RBJ peaking set for (f0, Q, gain) from
`dsp::biquad::Biquad::make`, updated in place with
`Biquad::design` each sample exactly like `ToneStack`
(`graph/src/blocks.rs` `process`). The coefficients change with the gain,
so the resonance shape follows the knob; 0 dB is exactly identity (provable,
and asserted in the tests). Per-sample design of 10 biquads is <1 µs on the
Pi 5 at 48 kHz and is the same cost as the existing `ToneStack`.

Stereo: the block is mono, like every other block in the graph; a stereo
graphic EQ is a later card.

**Out of scope:** per-band frequency tuning (centres are ISO, fixed),
per-band Q, stereo, mid/side variants.

### 1.4 Per-band gain range

**±12 dB around 0 dB, default 0 dB**, curve `AmpGain` (linear in dB).
The UI shows one control per band from −12 to +12 dB. Cross-band clipping
of the cascade output is not clipped in the block (the `util.master` node
limiter caps the rig, as today).

### 1.5 Parameter naming and serialization

Ten parameters, one per band, ids `gain0`…`gain9`, ordered by ascending
centre frequency:

| id | name | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| `gain0` | Band 1 | dB | −12 | +12 | 0 | AmpGain |
| `gain1` | Band 2 | dB | −12 | +12 | 0 | AmpGain |
| `gain2` | Band 3 | dB | −12 | +12 | 0 | AmpGain |
| `gain3` | Band 4 | dB | −12 | +12 | 0 | AmpGain |
| `gain4` | Band 5 | dB | −12 | +12 | 0 | AmpGain |
| `gain5` | Band 6 | dB | −12 | +12 | 0 | AmpGain |
| `gain6` | Band 7 | dB | −12 | +12 | 0 | AmpGain |
| `gain7` | Band 8 | dB | −12 | +12 | 0 | AmpGain |
| `gain8` | Band 9 | dB | −12 | +12 | 0 | AmpGain |
| `gain9` | Band 10 | dB | −12 | +12 | 0 | AmpGain |

**Naming rule:** the id is `gain` + the band index in ascending
centre-frequency order; the display name is `Band N` (human label, no
"gain" words, so the slider readout reads "−6 dB" not "−6 gain dB").

**Serialization:** params ride on the existing `Doc`/`NodeDoc` payload
(`node.params: BTreeMap<String, f32>` in `graph/src/doc.rs`) and on the
engine's `state.registry[<block_id>].params` snapshot (server.rs). A node of
type `tone.graphic_eq` stores `{"gain0": 0.0, …, "gain9": 0.0}`.
No new preset schema field, no migration, no protocol change, no new
command: a band change is the existing `setParam` command
`{channel, block, param, value}` → `Cmd::SetParam` applied on the live
executor between blocks (real-time safe, same path as `tone.stack` →
`setParam("mid", …)`). Missing params take the block default (0 dB), so old
presets that predate this block load unchanged.

### 1.6 Registration with the effect/chain system

* One line: `r.add::<GraphicEq>();` in
  `graph/src/blocks.rs::builtin_registry()`. The engine serves the block
  through the existing registry snapshot (`engine/src/server.rs::registry_json`);
  the UI's add-block picker (`ui/src/components/AddBlockModal.tsx`), the chain
  catalogue (`BlockList.tsx`) and the channel palette all discover it
  automatically. Nothing else changes: no new node, no new edge type, no
  preset-schema change, no new IPC command.

### 1.7 Tests

`graph/tests/graphic_eq.rs` (run with `cargo test -p dsp-graph`):
1. **registry** — id/name/category 10 params, audio in/out ports.
2. **0 dB identity** — cascade passes a sine through unchanged
   (max abs error < 1e-6 over 4096 samples @48 kHz).
3. **band shape** — +12 dB at band 3 raises the response at 250 Hz by ≈12 dB
   and −12 dB cuts it by ≈12 dB; bands do not interact.
4. **param schema** — 10 params, exact ranges and denormalise mapping.
5. **doc round-trip** — `NodeDoc.params` {gain0..gain9} round-trips through
   `Doc::from_json`/`to_json` and re-compiles finite.

---

## 2. UI: survey, evaluation and decision

### 2.1 What already exists in `ui/`

| Layer | What it is |
|---|---|
| Framework | Preact 10.27 + `@preact/signals@2` + TypeScript 5.9 |
| Styling | Tailwind CSS v4 via `@tailwindcss/vite`; CSS custom properties in `ui/src/theme.ts` and `ui/src/tailwind.css` (spacing, `--cat-h` per category, `--amber`, `--ui-*` tokens) |
| Components | `Knob` (editor rotary + SVG arc), `HeaderKnob` (58 px top bar), `Editor` (rack control panel), `Chain` (channel rows + drag-and-drop strip), `BlockList`/`Rack` (block catalogue), `AddBlockModal`, `RoutingLines` (hand-rolled SVG overlay), `TunerModal`, `ModelBrowser`, `PresetBrowser`, `Icon` |
| Interaction | pointer-events driven drag-and-drop (HTML5 drag + pointer + touch fallback in `touch.ts`), `<input type="range">` sliders (`ChannelHead`), native `<select>` |
| No third-party UI libs | no charting library, no component kit, no date/number picker; nothing external beyond Preact and its signals binding |

The only vector graphics rendered by hand are the **Knob** arc (SVG, zero
dependency) and the **RoutingLines** overlay (SVG, DOM-measured geometry),
so SVG is already the accepted convention for small custom vector views.

### 2.2 Options considered for a curve editor

| Option | Verdict | Reason |
|---|---|---|
| **Hand-rolled SVG**, like `Knob`/`RoutingLines` | **Chosen** | zero new dependency; the view is a tiny vector scene; handles are real SVG/HTML elements (pointer events, a11y, selection and focus); vector output is crisp at every size, including the 1440 px displays; the reactor pattern already proved in this repo; controllable from Preact + TypeScript with no interop shim. |
| Canvas 2D bitmap | Rejected | no per-element hit-testing for draggable handles; redraws on every drag frame; text/axis edges rasterise; invisible at 1x and blurry at 2x. |
| Charting library (recharts, victory, nivo) | Rejected | peer-dependency conflict with Preact 10, 100+ kB bundle, chart concepts (margins, cartesian grids, axis components as React nodes) that do not map to a guitar-pedal EQ view, and it would add a rendering framework surface the app does not need. |
| Native `<input type="range">` for the sliders + SVG for the curve only | Part of chosen | sliders are native inputs; only the response curve is SVG. |

### 2.3 The reusable component: `<Graph<T>>`

**File:** `ui/src/components/Graph.tsx` (new, single file).

It is a **pure presentational component**: data in via props, choice out via
callbacks, no knowledge of the engine, the bridge, or the app state. It is
deliberately small — axes, series, handles, one overlay hook — and the three
editors are thin consumers.

**Types and props (exact signatures):**

```ts
// ui/src/components/Graph.tsx

/** One axis. T is the data type (usually number). */
export interface GraphAxis<T = number> {
  type: 'linear' | 'log';   // log is required for frequency axes
  min: T;
  max: T;
  /** Display units rendered on the tick labels. */
  units?: string;
  /** Desired count of major ticks; the renderer picks a grid-aligned set. */
  ticks?: number;
}

export interface GraphAxes<T = number> {
  x: GraphAxis<T>;
  y: GraphAxis<T>;
}

/** One data series. */
export interface GraphSeries<T = number> {
  key: string;              // stable id for re-render diffs
  label: string;            // legend entry
  color: string;            // CSS color
  kind: 'line' | 'area' | 'points' | 'line+area';
  data: ReadonlyArray<T>;   // samples, in data units, same count as the axis domain
  yAxis?: 'y' | 'x';        // which axis the values map onto (default 'y')
}

/** A draggable handle (vertical guide + value readout). */
export interface GraphHandle<T = number> {
  id: string;
  value: T;
  label: string;            // e.g. "Band 3 · −6 dB"
  color: string;
  onChange: (id: string, value: T) => void;   // called while dragging
}

/** A render overlay: band labels, crossover markers, the y=x reference. */
export interface GraphOverlay<T = number> {
  id: string;
  render: (w: number, h: number, axes: GraphAxes<T>) => JSX.Element;
}

export interface GraphProps<T = number> {
  /** px; defaults to the container width (responsive). */
  width?: number;
  minWidth?: number;        // default 320
  /** px fallback when the host needs an explicit height (default: auto) */
  height?: number;
  minHeight?: string;       // CSS, default '240px'
  axes: GraphAxes<T>;
  series: GraphSeries<T>[];
  handles?: GraphHandle<T>[];        // optional; dragging a handle calls onChange
  overlays?: ReadonlyArray<GraphOverlay<T>>;
  grid?: boolean;             // default true
  zeroLine?: boolean;         // highlight y = 0 (default false)
  xLabel?: string;
  yLabel?: string;
  /** Crosshair: passed in by the host on mouse move; optional. */
  cursor?: { value: T; label: string } | null;
  onCursorChange?: (value: T) => void;  // mousedown → data value under the pointer
  /** Required for log scales (domain must not contain zero or negatives). */
  dataDomain?: [T, T];
  className?: string;
}
```

**Behaviour contract the implementors rely on:**
- Sizing is container-driven: the component measures its own box
  (`ResizeObserver` + `getBoundingClientRect`) and lays out an SVG whose
  `viewBox` is computed from `width`/`minWidth` and `minHeight`; nothing is
  fixed in px except the per-handle glyph sizes.
- The axis grid is drawn in the SVG; tick labels use `units` and the
  `ticks` hint; the `log` axis skips 0/negative and requires `dataDomain`.
- Handles are rendered as real SVG/HTML elements so `pointermove` fires on
  them; `onChange` is called on every position change, not on release.
- `cursor`/`onCursorChange` give the host a read-back of the data value
  under the pointer (used by the EQ to show the current band, by the shaper
  to show the output at the hovered x).
- `series` with `kind: 'area'` renders a flat base under the line (0 or the
  axis min); `line+area` is a short-hand for "fill + line".

### 2.4 Consumer editors (each a separate file, all on `<Graph>`)

**Graphic EQ — `ui/src/components/GraphicEqEditor.tsx`**
- Axes: x = log 20 Hz … 20 kHz (`units: 'Hz'`, `ticks: 8`), y = linear
  −15 … +15 dB (`units: 'dB'`).
- Series: one series, `kind: 'line+area'`, the combined response curve — the
  steady-state magnitude of the 10-band cascade computed by the UI from the
  band gains with the RBJ closed form (the same formula as
  `dsp::biquad::Biquad::make`, evaluated at 200 log-spaced points plus the
  band-centre ticks). It is a *graphic proxy*: steady-state magnitude only,
  no filter state; the sound is defined by the block.
- Handles: 10 handles, one per band: vertical dashed guide at the ISO centre,
  label `Band N <freq>` (e.g. "Band 3 @ 125 Hz"), draggable vertically
  −12…+12 dB; the drag value is written through `setParam(channel, block,
  `gain${i}`, v)` — the existing `setParam` contract.
- Below 720 px the per-band label wraps to a single row; the 10 band sliders
  stay in one row (min-width preserved); nothing clips.

**Waveshaper — `ui/src/components/WaveShaperEditor.tsx`**
- Axes: x = linear −1…+1 (`units: ''` — arbitrary units), y = linear
  −1…+1.
- Series: the transfer curve (`line+area`), computed by evaluating the
  shaper's piecewise-linear curve at 200 points; plus an overlay of the
  y=x reference line (dashed, `color: muted`) so the "no change" shape is
  readable.
- Handles: 41 control points (x evenly spaced in [−1, 1], y = output,
  clamped to [−1, 1]), draggable in the chart; each handle IS a control
  point of the shaper, so `onChange` mutates the shaper's control-point
  list directly (the shaper component owns its parameters).
- Overlay: the clip envelope (light shading near the edges).

**Multiband compressor — `ui/src/components/MultibandCompressorEditor.tsx`**
- Axes: x = log 20 Hz…20 kHz (`ticks: 8`), y = linear −30…0 dB
  (`units: 'dB'`, `zeroLine: true`).
- Series: one `area` series of the gain-reduction curve (the sum of the
  active bands' reduction vs frequency); band shading optional.
- Overlays: one vertical dashed `crossover` marker per band
  (`render` implementation draws only the crossing line + label).
- Belt and braces: a small table beside the graph (band id, crossover Hz,
  threshold dB, gain reduction dB, band gain) with per-band sliders — the
  graph is a *display* of the compressor's reduction curve; the band
  controls are the parameters. (The multiband compressor itself is a
  separate feature card; this card solely defines the graph consumption.)

### 2.5 Responsive sizing

- `Graph` sizes with the container, not with fixed px: `minWidth` default
  320, `minHeight` default `240px`, `width` falls back to the container.
- The three editors live in the bottom editor panel (same shell as
  `Editor.tsx`), each a `min-h` scroll container with `overflow-y: auto`;
  the graph is the cross-panel anchor and keeps its aspect ratio on every
  resize (CSS `aspect-ratio` via the SVG `viewBox`).
- Narrow viewports (< 720 px): band labels in the EQ editor wrap to one line
  per band; the handle row keeps its min-width; the curve never clips — the
  panel scrolls instead.

---

## 3. API signatures to implement

### 3.1 Engine (Rust, `graph` crate = `dsp-graph`)

```rust
// graph/src/blocks.rs

pub struct GraphicEq {
    bands: [Biquad; 10],   // per-band peaking biquad, Q = 1.0, f0 from table
    gains: [f32; 10],      // current linear gains (0 dB = 1.0)
}

impl Block for GraphicEq {
    fn spec(&self) -> BlockSpec {
        // id: "tone.graphic_eq", name: "Graphic EQ", category: Category::Tone,
        // inputs: [audio "in"], outputs: [audio "out"],
        // params: gain0..gain9, dB, -12..+12, default 0, curve AmpGain
    }
    fn prepare(&mut self, sr: f32, _max: usize) -> Result<(), BlockError> { /* design bands at 0 dB (identity) */ }
    fn process(&mut self, ctx: &mut BlockCtx<'_>) -> Result<(), BlockError> {
        // cascade: x = input; for each band { x = band.design(Peak, sr, f0, 1.0, db); x = band.process(x) }
    }
    fn reset(&mut self) { /* zero biquad state */ }
}

// const: G_EQ_CENTRES_HZ: [f32; 10] = [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

// graph/src/blocks.rs :: builtin_registry():
r.add::<GraphicEq>();   // served automatically by the existing registry snapshot
```

Tests: `graph/tests/graphic_eq.rs` — registry, 0 dB identity, band shape,
param schema, doc round-trip.

### 3.2 UI (TypeScript, `ui/`)

```tsx
// ui/src/components/Graph.tsx          (new, presentational)
export interface GraphAxis<T = number> { type: 'linear'|'log'; min: T; max: T; units?: string; ticks?: number; }
export interface GraphAxes<T = number> { x: GraphAxis<T>; y: GraphAxis<T>; }
export interface GraphSeries<T = number> { key: string; label: string; color: string; kind: 'line'|'area'|'points'|'line+area'; data: ReadonlyArray<T>; yAxis?: 'y'|'x'; }
export interface GraphHandle<T = number> { id: string; value: T; label: string; color: string; onChange: (id: string, value: T) => void; }
export interface GraphOverlay<T = number> { id: string; render: (w: number, h: number, axes: GraphAxes<T>) => JSX.Element; }
export interface GraphProps<T = number> {
  width?: number; minWidth?: number; height?: number; minHeight?: string;
  axes: GraphAxes<T>; series: GraphSeries<T>[];
  handles?: GraphHandle<T>[]; overlays?: ReadonlyArray<GraphOverlay<T>>;
  grid?: boolean; zeroLine?: boolean; xLabel?: string; yLabel?: string;
  cursor?: { value: T; label: string } | null;
  onCursorChange?: (value: T) => void;
  dataDomain?: [T, T]; className?: string;
}

// ui/src/components/GraphicEqEditor.tsx        (new)
interface GraphicEqEditorProps {
  channel: string; block: 'tone.graphic_eq';
  onChange: (param: `gain${number}`, db: number) => void;   // -> bridge.setParam
}
// renders: <Graph axes={{x: log 20..20000 Hz, y: linear -15..+15 dB}} series={[combined response]} handles={10 band handles} /> + slider row (native input type="range" per band, -12..+12)

// ui/src/components/WaveShaperEditor.tsx       (new)
interface WaveShaperEditorProps {
  points: ReadonlyArray<{ x: number; y: number }>;  // control points, x,y in [-1,1]
  onChange: (points: ReadonlyArray<{ x: number; y: number }>) => void;
}
// renders: <Graph axes={{x: linear -1..1, y: linear -1..1}} series={[transfer curve line+area]} handles={41 draggable control points} overlays={[y=x diagonal, clip envelope]} />

// ui/src/components/MultibandCompressorEditor.tsx (new)
interface MultibandCompressorEditorProps {
  bands: ReadonlyArray<{ id: string; crossover: number; threshold: number; gainReduction: number; gain: number }>;
  onBandChange: (id: string, field: 'threshold'|'gain', value: number) => void;
}
// renders: <Graph axes={{x: log 20..20000 Hz, y: linear -30..0 dB}} series={[GR area curve]} overlays={[crossover markers]} /> + band table + per-band sliders
```

---

## 4. Builder cards

### Card A — engine implementation (pedal-builder)

- **Files:** `graph/src/blocks.rs` (add `GraphicEq` struct, spec, prepare,
  process, reset; `G_EQ_CENTRES_HZ` constant; `r.add::<GraphicEq>()` in
  `builtin_registry()`), `graph/tests/graphic_eq.rs` (new).
- **Approach:** series cascade of 10 `BiquadKind::Peak` (Q 1.0, ISO centres);
  gain baked into coefficients each sample, exactly like `ToneStack`; params
  ride on `NodeDoc.params`; register in `builtin_registry()`; 5 tests in
  `graph/tests/graphic_eq.rs`.
- **Acceptance:** `cargo test --workspace --exclude pedal-shell` green (the
  dsp-graph tests included); 0 dB is sample-exact identity; ±12 dB bands show
  ≈12 dB response at their centre with no inter-band interaction; 10 params
  with exact ranges; `NodeDoc` params round-trip through JSON; the block
  appears in the engine's `state.registry` snapshot.
- **Verify:** `source scripts/agent-env.sh && cargo test --workspace
  --exclude pedal-shell` from the worktree root.
- **Try it:** run the app, open a channel, add `Graphic EQ` from the Tone
  palette; the bottom panel shows the 10 band sliders ±12 dB; move any band
  and the response follows (the curve is a UI preview; the sound is the
  block's).

### Card B — UI components (pedal-ui-builder)

- **Files:** `ui/src/components/Graph.tsx` (new), `ui/src/components/
  GraphicEqEditor.tsx` (new), `ui/src/components/WaveShaperEditor.tsx`
  (new), `ui/src/components/MultibandCompressorEditor.tsx` (new),
  plus any wiring needed in `ui/src/state.ts` for the band sliders to call
  `setParam` (the existing contract).
- **Approach:** one presentational `<Graph<T>>` (axes, series, handles,
  overlays, responsive sizing, cursor); three thin consumer editors on top
  of it; `<Graph>` has no engine/bridge knowledge.
- **Acceptance:** `cd ui && pnpm run build` typechecks and builds green;
  `<Graph>` renders a log-frequency EQ view with 10 draggable band handles
  and the −12…+12 dB slider row; the shaper editor renders x/y in [−1,1]
  with the y=x reference; the compressor editor renders the GR curve with
  crossover markers; nothing new is added to the engine or the protocol.
- **Verify:** `cd ui && pnpm run build` (from the worktree root; note
  `ui/dist` is generated and ignored — never commit it).
- **Try it:** in the app, select a `Graphic EQ` block: the editor shows the
  curve with the 10 band handles plus sliders; dragging a handle moves the
  curve and writes through `setParam`. Switch to the `WaveShaper` editor and
  drag a control point; the curve responds in real time.

---

## 5. Decisions recorded (v1 scope)

| # | Decision | Why |
|---|---|---|
| D1 | 10 bands, ISO 1/1-octave 31.5…16 kHz | half the filter count of 31, classic guitar shape; parameterised table so 31 is a table change only |
| D2 | Series cascade of peaking biquads, Q = 1.0 | no inter-band feedback, no summing node, mirrors `ToneStack` |
| D3 | ±12 dB per band, curve `AmpGain` | the rated shaping range; 0 dB identity (asserted) |
| D4 | `gain${i}` param ids, `Band N` labels | stable ids on the existing `NodeDoc`/registry payload; no schema change |
| D5 | UI: hand-rolled SVG `<Graph<T>>`, no chart lib | Preact 10 + zero deps; handles are real elements; crisp vector; already proven in this repo |
| D6 | Curve is steady-state magnitude, UI-side RBJ preview | the sound is defined by the block; the curve is a graphic proxy |
| D7 | Narrow-viewport: graph keeps aspect ratio, labels wrap | no clipped controls on the 1440 px monitors or 800 px |

---

*Design record for card `t_f88ef464`. Children: the engine implementation card
(A) and the UI components card (B).*
