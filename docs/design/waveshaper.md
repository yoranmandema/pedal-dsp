# Waveshaper effect — design

Status: design (not implemented). Owner: `pedal-architect`. Card: `t_ad1fd9cc` (feature). Children that build on this: `t_db0c3ca5` (engine), `t_45bfd1b7` (UI), `t_f67e17d9` (review). Root: `t_4bf51994`.

One new block in the **Distortion** category that shapes the input through a **user-editable transfer curve**. The curve is an ordered list of control points \((x,y)\in[-1,1]^2\), with an interpolation mode, input gain, output gain, dry/wet mix, and output clamp. It is data-driven: the same engine code serves any curve, and the UI edits the curve as an interactive line graph.

> Note: this is *not* the existing `Shaper`/`ShaperKind` family in `dsp/src/waveshape.rs` (fixed algorithmic curves). `dist.waveshaper` is a separate, data-driven block described here.

---

## 1. What ships

| | |
|---|---|
| **id** | `dist.waveshaper` |
| **name** | `Waveshaper` |
| **category** | `Distortion` |
| **ports** | `in` (audio) → `out` (audio) |
| **block type** | pure-parameter, no new `pedal-dsp` primitive (reuses `dsp::oversample` for anti-aliasing) |

It is a self-contained block: no new DSP primitive, no new channel type, no new shell behavior. It plugs into the chain exactly like `Overdrive`, `Shaper`, `Compressor`, `Tremolo`.

---

## 2. Parameters

Declared in `spec().params` **in this order** (the index in the array is the param index read by `BlockCtx::param`):

| idx | id | kind | unit | min | max | default | curve | role |
|---|---|---|---|---|---|---|---|---|
| 0 | `input_gain` | number | Linear | 0.125 | 8.0 | 1.0 | Linear | pre-curve gain (linear) |
| 1 | `output_gain` | number | Linear | 0.125 | 8.0 | 1.0 | Linear | post-curve gain (linear) |
| 2 | `mix` | number | Percent | 0.0 | 100.0 | 100.0 | Linear | dry/wet blend |
| 3 | `clamp` | number | (0/1 checkbox) | 0 | 1 | 1 | Linear | clamp output to [-1,1] |
| 4 | `interpolation` | choice | — | 0 | 1 | 0 | Linear | `0` = linear, `1` = monotone cubic |
| 5 | `curve` | **composite** | — | — | — | — | — | transfer-curve payload (see §3) |

The first five params are read from `ctx.params` in the audio thread:
- `input_gain  = ctx.param(0)`
- `output_gain = ctx.param(1)`
- `mix         = ctx.param(2) / 100.0`
- `clamp       = ctx.param(3) != 0`
- `interpolation = ctx.param(4) as usize ` → `Linear` if 0, `Cubic` if 1.

The `curve` param (idx 5) is a **composite** param; it is not a scalar and is not read from `ctx.params`. It carries the structured payload, set at build time via `configure("curve", json)` and at run time via `setParam(channel, block, "curve", <payload>)`.

**Default curve** (identity): `[[-1, -1], [1, 1]]`. With the identity curve and all gains 1.0, the block is a transparent pass-through.

**Built-in curve presets** (named, loaded into the UI line graph; stored in presets the same way):

| preset | points (x, y) |
|---|---|
| `Identity` | `[[-1,-1],[1,1]]` |
| `Soft clip` | `[[-1,-1],[-0.6,-0.95],[-0.2,-0.25],[0.0,0.0],[0.2,0.25],[0.6,0.95],[1.0,1.0]]` |
| `Hard clip` | `[[-1,-1],[-0.1,-1.0],[0.1,1.0],[1.0,1.0]]` |
| `Fold` | `[[-1,-1],[-0.5,0.5],[0.0,0.0],[0.5,0.5],[1.0,1.0]]` |

---

## 3. Data model — the transfer curve

**Control points** (`points`):
- An ordered list of \((x,y)\) pairs, each \(x,y\in[-1,1]\).
- Sorted ascending by `x` (strictly increasing — no duplicate x values).
- At least 2 points. The first point's `x` must be ≥ -1; the last's `x` must be ≤ 1 (values outside are clamped in the lookup).
- Stored as a JSON array of 2-element numeric arrays: `[[x0,y0],[x1,y1],...]`.

**Curve payload** (the value of param `curve`, on the wire and in presets):
```json
{ "points": [[x0, y0], [x1, y1], …] }
```
The interpolation mode is a *separate* scalar param (`interpolation`), so the UI can switch linear/cubic without re-serialising the points.

**Math per sample** (with `t` the input, `n = ctx.n`):
```text
u = clamp(t * input_gain, -1, 1)      // apply pre-gain, clamp to table domain
y = table_lookup(u, interpolation)    // index into the 2048-entry table
y = y * output_gain
y = clamp ? clamp(y, -1, 1) : y
out[i] = mix(t, y, mix)                // dry/wet
```
`table_lookup` performs a per-sample interpolation (linear or monotone cubic) into the precomputed table; it does **no allocation, no lock, no I/O**.

---

## 4. Engine approach

**Lookup table.** Precompute a fixed-size table of `N = 2048` entries over the normalized input axis `x ∈ [-1, 1]`, spacing `2/(N-1)`. Build it off the audio thread, in `Block::prepare` (compile time, `BlockCtx`-free):

```text
for i in 0..N:
    u = -1.0 + 2.0 * i / (N-1)
    table[i] = interpolate(points, u, interpolation) * output_gain
```
`input_gain` is applied *at lookup time* (it is a block field, not table index), so it is covered by the same table. The table is rebuilt only when `input_gain`, `output_gain`, `interpolation`, or `points` changes.

**Interpolation modes**.
- `linear`: standard linear interpolation between table neighbours. Fast, single index.
- `cubic`: monotone cubic Hermite interpolation (Fritsch–Butcher) using a 4-point stencil around the sample. Preserves the monotonicity of the control points (no new local extrema), which avoids overshoot/ringing between points. Cost: ~4 lookups + a few multiplies per sample.

**Anti-aliasing (optional, recommended default X4).** The nonlinear curve is a source of in-band aliasing. Reuse the existing `dsp::oversample` stage (`Upsampler2`, `Upsampler4`, `Downsampler2`, `Downsampler4`, half-band FIR) exactly as `Shaper` does:
- The block holds `up4`/`down4` (+ `up2`/`down2`) and runs the table lookup at 2x or 4x the host rate, then downsamples back.
- Default `oversampling = X4`; `X2` and `None` are selectable the same way as the existing `Shaper`.
- The nonlinearity runs on oversampled input; the table is identical (it maps `[-1,1]`); only the sampling rate changes.

**Real-time safety.** This is a hard requirement, not a preference:
- **No allocation on the audio thread.** The table is a fixed-size `Vec<f32>` allocated in `prepare`. `process` is pure arithmetic + table lookup.
- **No lock on the audio thread.** Parameters flow through the existing executor path (`ex.set_param_at`); the block reads its own cached fields per block. There is no lock in `process`.
- **No I/O on the audio thread.** Everything is f32 arithmetic.
- **Table swap is safe by construction.** The block rebuilds its table lazily, at the start of `process`, only when a relevant param changed since the last block. This is a per-block cost (≈2048 f32 writes, a few µs), never on the per-sample path, and it can never be interrupted mid-rebuild. The audio callback sees either the old table (if the rebuild is queued before the sample) or the new one (after) — never a torn table.
- **Curve payload updates happen at block boundaries.** The structured `curve` JSON is parsed into `points`/`interpolation` block fields by `setParam("curve", …)` on the UI thread and applied to the block; the audio thread only reads the resulting fields. There is no in-flight mutation of the curve list.

**Threading contract (the only concurrency rule).** The block has no `Mutex`, no `Atomic`, no `RefCell`. It is `Send` and the executor drives it single-threaded per instance. All concurrency safety is inherited from the existing `Executor`/`Handle` model (params via `Cmd::SetParam`, swaps via `Cmd::Swap`, trash via a lock-free queue).

---

## 5. Serialization / message contract

### 5.1 UI → engine (the `setParam` command)

The curve is carried on the **existing closed command set** by reusing `setParam` with the composite param id `"curve"`. The client sends:

```json
{ "type": "req", "id": 7, "cmd": "setParam",
  "args": { "channel": "ch1", "block": "wsh1", "param": "curve",
            "value": { "points": [[-1,-1],[0.5,0.4],[1,1]] } } }
```

The scalar params (`input_gain`, `output_gain`, `mix`, `clamp`, `interpolation`) use the existing `setParam` path exactly as every other block.

**Engine change required (server.rs — 6 lines).** In `engine/src/server.rs::set_param`, the existing helper casts `value` to `f32` (`f()`). Add a special-case for `param == "curve"`: read `args["value"]` as `serde_json::Value` (not f32), validate, and store the block's curve state. All other params keep the existing f32 path (unchanged). No new IPC command is added, so the wire protocol stays closed and backward compatible.

### 5.2 Preset / project format

Presets are `Doc` JSON files (`engine/src/store.rs` / `graph/src/doc.rs`). `NodeDoc.params` becomes `BTreeMap<String, serde_json::Value>` (additive change: every existing scalar param still serialises as a JSON number). The waveshaper's curve is stored under the key `"curve"` as the JSON array/scalar described in §3. Existing presets (no `"curve"` key) load with the identity curve — **backward compatible, no error**.

**Deserialization/validation on load:**
1. `curve` must be an object with a `points` array of ≥ 2 elements.
2. Each element must be a 2-element numeric array; each `x,y ∈ [-1,1]`.
3. `x` must be sorted ascending, strictly increasing.
4. If invalid → reject the request / drop the preset entry with a log message; **never** apply a corrupt curve (the protocol's "failed mutation never changes state" rule is non-negotiable).
5. Missing `curve` → identity. Missing scalar params → spec defaults.

### 5.3 Snapshot / UI read

`snapshot()` (server.rs) and the UI (`state.ts`) carry the curve payload under `params.curve`, and the UI renders it. The block's scalar params continue to appear as plain numbers.

---

## 6. UI approach

A Preact + TypeScript component, `WaveShaperGraph`, wired into the effect panel for the `dist.waveshaper` block (the engine card adds the block; the UI card wires it in).

**Graph**. SVG, x-axis = input `[-1,1]`, y-axis = output `[-1,1]`, with:
- grid lines, axes, tick labels, and a diagonal identity reference (`y = x`).
- the curve drawn as a polyline through the control points.
- draggable point handles (mouse + pointer/touch), each clamped to `[-1,1]` during drag.
- a selected point highlight + delete affordance.

**Interactions**
- `add`: click/tap on the empty canvas inserts a point at the pointer's snapped position.
- `drag`: pointer drag moves a point (clamped to bounds); mouse + touch handled with pointer events so one gesture path works for both.
- `remove`: `Delete`/`Backspace` key or right-click on a selected point removes it (minimum 2 points — never go below 2).
- `snap`: drag points snap to a 0.05 grid for a tidy UI; the stored value is the snapped f32.
- presets: `Identity` / `Soft clip` / `Hard clip` / `Fold` buttons load the named point lists (§2).
- `reset`: restore the identity curve.
- `z-index`/`layers`: the curve stays under the point handles.

**Input-level readout (feasible).** A small readout showing the peak input level of the selected source in dB (`-inf..0`, updated at the panel's refresh rate). Useful for judging whether the curve is being reached; no new engine feature — it reads the existing meters/source.

**Accessibility**
- points are keyboard-operable: arrow keys nudge a focused point (0.01 steps), `Delete`/`Backspace` removes, `Escape` clears selection.
- `aria-label` on the SVG, on each point and the axes; sufficient colour contrast (the curve and grid are dark-on-light; focus rings visible).
- responsive layout; the graph reflows at narrow widths.

**Wire-up & performance.** Curve updates are sent to the engine on change, **throttled during drag** (e.g. requestAnimationFrame + debounce, not per-sample). The block never sees per-sample writes.

---

## 7. Integration

**Registry (one line).** In `graph/src/blocks.rs::builtin_registry()`:

```rust
r.add::<WaveShaper>();
```

- `id` = `"dist.waveshaper"`, name `"Waveshaper"`, category `Distortion`.
- Ports: `in` (audio) → `out` (audio).
- Params: the six from §2. The block owns the curve data (`points`, `interpolation`, `input_gain`, `output_gain`, `mix`, `clamp`) as its own fields; the scalar params are updated from `ctx.params` each block, and the curve is applied via `configure("curve", json)` and `setParam("curve", …)`.

**Cadence.** The block follows the existing `Block` trait (`spec`, `prepare`, `process`, `reset`) and is a pure-parameter block like `Compressor` / `Tremolo`: `prepare` builds the table, `process` does the lookup, `reset` clears any internal state.

---

## 8. Acceptance criteria (checks for the engine + UI)

Engine (`cargo test -p dsp-graph --test <waveshaper test>`):
1. **Spec+lifecycle** — `dist.waveshaper` is present with name `Waveshaper`, category `Distortion`, audio in/out, and the six params above.
2. **Identity** — identity curve × full scale passes the input unchanged (within tolerance), at several mix values.
3. **Scaling** — `input_gain` scales the curve's effective input; `output_gain` scales the output.
4. **Hard clip** — a hard-clip curve clips as expected (output bounded by the target), at 2x and 4x sample rates.
5. **Monotone cubic** — a cubic-interpolated curve introduces no new extrema between points (monotonicity preserved).
6. **Bounds** — out-of-range input is clamped at the table edges safely.
7. **Real-time** — `process` allocates nothing (no `Vec::push`, no `format!`, no `println!`); runs at 48 kHz with the engine's 64-frame block within budget (`cargo test -p pedal-engine` real-time budget test).
8. **Validation** — invalid curves (too few points, unsorted x, out-of-range coords) are rejected; stale/serialised curves round-trip through presets.
9. **Oversampling** — with X4, aliasing from the curve is below the X2/None case (same assertion as `oversample` tests).

UI (`cd ui && pnpm run build` + component tests):
1. **Render** — the graph draws with an identity curve, grid, axes, identity diagonal.
2. **Add** — clicking the canvas adds a point (minimum 2 enforced).
3. **Drag** — dragging a point moves it; it snaps to the 0.05 grid; clamping to [-1,1].
4. **Remove** — `Delete`/right-click removes a point but never leaves fewer than 2.
5. **Presets** — buttons set the exact point lists from §2.
6. **Keyboard** — arrow-nudge, focus ring, ARIA labels, keyboard delete.
7. **End-to-end** — editing the curve in the UI actually changes the audio output through the engine (measured on a small rig with a real source).
8. **Responsive** — layout holds at ~800 px and ~1440 px wide (visual QA with the vision tool).

**Try it** (product owner): open the app, add a `Waveshaper` block to a channel, select it, drag control points on the curve, switch interpolation, adjust `input_gain`/`output_gain`/`mix`, and listen for the change in character. Load a preset from the three-dot menu.

---

## 9. Decisions recorded

1. Table size **2048**, default oversampling **X4** (matches the existing `Shaper`); `X2`/`None` available.
2. `curve` carried on the **closed** `setParam` command as the composite param `"curve"` (no new IPC command).
3. `NodeDoc.params` extended to `serde_json::Value` (scalar params stay numbers; the curve is a JSON payload); preset load drops invalid curves.
4. Lazy, per-block table rebuild; `process` is allocation-free and lock-free. No `Atomic`/`Mutex` in the block.
5. Anti-aliasing: reuse `dsp::oversample`; X4 default (documented, not enforced by the engine — the block supports X2/None via the spec's parameter).

## 10. Out of scope (for later cards)

- Multi-voice / stereo waveshaper.
- Per-block glyph and per-block preset tokens (a cosmetic follow-on).
- A splitscreen "curve vs. original" A/B comparison (exposed via a routing in a future card).
