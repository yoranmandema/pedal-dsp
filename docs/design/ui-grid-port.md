# UI grid port: the Lovable "splitpath" mockup → pedal-dsp

Source of truth for the cards under feature card t_d8c4180b (map in §7).

- Live mockup: https://splitpath-signal-studio.lovable.app (React 19 + TanStack Start +
  Tailwind v4 + lucide icons; one route, all state in `useState`, no backend).
- Mockup app code, recovered from the production bundle: `docs/design/mockup/mockup-app.js`
  (identifiers renamed by the minifier; name map at the top of that file).
- Screenshot at 1440×900: `docs/design/mockup/mockup-1440.png`.
- Current UI audit: `docs/ui-port-notes-current.md`.

Product owner's rule: **use as much of the mockup as possible, but keep our knob
rendering.** `ui/src/components/Knob.tsx`, `HeaderKnob.tsx` and `FrequencyKnob.tsx` are not
modified; the mockup's own knob (`wt` in the bundle) is not ported.

---

## 1. The model in one paragraph

The rig is a fixed **4 rows × 8 columns grid**. A row is a channel (our 4 fixed channels).
A block sits in one cell (`row`, `col`); cells can be empty, so a row can have gaps. Audio
flows left → right along a row through its blocks in column order; empty cells pass audio
through. Each row has an **IN** toggle (feed it from the main input), a **mute** and a
**trim**. A **split** (mockup "link", our engine `Routing`) taps a block's output and also
feeds a block in a *different row* at a *later column*; the source row keeps flowing.
Every row's end is summed on a vertical **output bus** into the master out.

Mapping to what we already have:

| Mockup | pedal-dsp today | Change |
|---|---|---|
| row 0..3 | `Channel` (4 fixed) | none |
| block `{id,type,row,col,bypass,values}` | `Block {id,type,name,bypass,params}` in `Channel.blocks` (ordered) | **add `col` (0..7)** — t_d165d1a2 |
| drag block to any empty cell (also another row) | `moveBlock {channel, block, to}` (same channel, index) | **new `placeBlock`** cross-channel — t_d165d1a2 |
| link `{id,from,to}` | `Routing {id, source, dest, enabled, gain}` | add rule `dest.col > source.col`; prune on move — t_d165d1a2 |
| channel `input` | `"<in>"` in `Channel.sources` | none (toggle only that entry, keep other entries) |
| channel `trim` (dB) | `Channel.volume` via `setMasterVolume` (dB −60..12) | none — the trim control drives channel volume |
| channel `mute` | — | **new `muted` + `setMute`** — t_cd53f9be |
| tray (`k` list) | engine `registry` (`BlockSpec.category`) | none — the tray stays registry-driven |
| footer editor | `Editor.tsx` | restyle identity column only; controls unchanged |

The mockup's block catalogue (`k`) and its param defaults are illustration only. Our block
types, names, params and categories always come from the engine registry.

---

## 2. Canvas geometry (exact values from the bundle)

All values in CSS px, canvas-local coordinates. Put them in `ui/src/grid/geometry.ts`;
no component may hard-code them.

| Constant | Value | Meaning |
|---|---|---|
| `ROWS` / `COLS` | 4 / 8 | grid size |
| `ROW_H` | 112 | row pitch; `rowY(r) = r*112 + 56` (row centre) |
| `HEADER_W` | 116 | channel header column (left of the grid) |
| `GRID_X0` | 160 | left edge of column 0 |
| `COL_W` | 116 | column pitch; `colX(c) = 160 + c*116 + 58` (column centre) |
| `BLOCK_W` × `BLOCK_H` | 92 × 60 | block / empty-cell box, centred on (`colX`, `rowY`) |
| `IN_X` | 138 | x of the row input node |
| `BUS_X` | 1110 | x of the row output node and the vertical output bus |
| `OUT_X` | 1144 | end of the master-out stub (arrowhead tip at 1148) |
| `CANVAS_W` × `CANVAS_H` | 1150 × 448 | canvas size (`448 = 4*112`) |
| `HANDLE_DX` | 8 | split source anchor sits 8 px right of the block's right edge |

Derived: block left = `colX(c) − 46`, top = `rowY(r) − 30`.
Split anchors: source `(colX(src.col) + 46 + 8, rowY(src.row))`, target
`(colX(dst.col) − 46, rowY(dst.row))`.
Cable path (`cablePath(x1,y1,x2,y2)`): `k = max(30, (x2 − x1) * 0.5)`;
`M x1 y1 C x1+k y1, x2−k y2, x2 y2`.

Layering (bottom → top): dotted background → guides + row wires + bus + cables (one SVG,
`pointer-events:none`) → cable hit paths (second SVG; only the paths take pointer events)
→ channel headers, empty cells, blocks (HTML) → hover hint chip.

The canvas is a fixed 1150×448 box centred (`mx-auto my-4`) inside a scroll container
(`overflow:auto`) that carries the dotted background. Narrow windows scroll; the canvas
never scales.

---

## 3. Visual spec

### 3.1 Tokens (mockup `:root`, oklch)

| Token | Value | Use |
|---|---|---|
| background | `oklch(15.5% .006 260)` | app/canvas background, cable halo, flow dashes |
| panel | `oklch(18.5% .006 260)` | header, tray, footer |
| panel-2 | `oklch(22.5% .007 260)` | block body, hint chip |
| foreground | `oklch(93% .005 260)` | text |
| muted | `oklch(26% .007 260)` | chips, trim track, hover bg |
| muted-foreground | `oklch(62% .01 260)` | secondary text |
| border | `oklch(28% .007 260)` | borders, grid guides |
| primary / wire | `oklch(80% .13 80)` (amber) | active wire, accent, active slot chip |
| primary-foreground | `oklch(20% .02 80)` | text on primary |
| wire-dim | `oklch(34% .008 260)` | inactive wires/nodes |

Category colours (replace the hue table in `ui/src/theme.ts`):
Modelling `oklch(65% .17 295)`, Distortion `oklch(68% .16 40)`, Tone `oklch(80% .14 90)`,
Dynamics `oklch(72% .14 150)`, Time `oklch(70% .11 245)`, Modulation `oklch(68% .17 340)`,
Pitch `oklch(75% .11 195)`, Utility `oklch(65% .12 265)`. Categories the mockup lacks
(I/O, Split/Merge, Control) use `oklch(70% .1 <current hue>)`.

Fonts: IBM Plex Sans (400/500/600) for UI, IBM Plex Mono (400/500) for numbers/readouts.
**Bundle them** (`@fontsource/ibm-plex-sans`, `@fontsource/ibm-plex-mono`) — the app runs
offline in Tauri and on the Pi; never load Google Fonts at runtime.

Dotted background (`.grid-dots`): `radial-gradient(circle, oklch(100% 0 0 / .05) 1px,
transparent 1px)`, `background-size: 16px 16px`.

Flow animation (`.wire-flow`): `stroke-dasharray: 2 10; animation: flow 1.2s linear
infinite;` `@keyframes flow { to { stroke-dashoffset: -24px } }`. It is a 2 px
background-coloured dashed stroke drawn *over* an amber wire, so the wire looks like
moving dashes. Under `prefers-reduced-motion: reduce` the animation is off (static
dashes).

### 3.2 Canvas furniture

- **Guides**: 9 vertical lines at `x = 160 + t*116` (t = 0..8), y 8→440, stroke border,
  opacity .35, `stroke-dasharray 2 6`.
- **Row base wire**: `IN_X → BUS_X` at `rowY`, wire-dim, 2 px. Always drawn.
- **Row live wire**: from `rowWireStart(row)` to `BUS_X`, wire colour, 2 px, opacity .9
  (.3 when the channel is muted), plus a `.wire-flow` overlay when live and not muted.
  `rowWireStart(row)`: `IN_X` if the channel has `"<in>"` in `sources`; else the left
  edge (`colX(col) − 46`) of the left-most block in this row that is the *dest* of an
  **enabled** routing; else none (row has no signal, only the base wire).
  Blocks left of that start get no signal (they are drawn normally).
- **Input node**: circle r 5 at (`IN_X`, `rowY`), fill background, stroke 2 px wire when IN
  is on else wire-dim.
- **Output node**: circle r 5 at (`BUS_X`, `rowY`): filled + stroked wire when the row is
  live and not muted, else fill background / stroke wire-dim.
- **Bus**: vertical wire-dim 2 px line at `BUS_X` from `rowY(0)` to `rowY(3)`; master stub
  `BUS_X → OUT_X` at y 224 in wire colour; arrowhead polygon
  `1140,219 1148,224 1140,229` filled wire.

### 3.3 Channel header (one per row)

Absolute at left 0, top `rowY − 18`, 116×36, flex row, gap 8 px, vertically centred:
1. channel number `1..4`, mono 12 px, muted-foreground, width 16.
2. **mute** button: 14 px `volume-2` icon, or `volume-x` when muted. Muted → Distortion
   category colour; otherwise muted-foreground, hover foreground. `title="Mute"`,
   `aria-pressed`.
3. **trim**: mono 10 px readout (`+3.0 dB`, `0.0 dB`, `−3.0 dB`; `+` only above 0) over a
   48×3 px rounded track (muted); fill from the left to the value (muted-foreground, primary
   on hover); 1 px foreground/50 tick at 0 dB. Drag vertically: `v = v0 + (y0 − y)/4`,
   rounded to 0.5 dB, clamped to the channel-volume range (−60..+12); pointer capture;
   double-click → 0 dB; `cursor: ns-resize`; `title="Channel trim — drag up/down,
   double-click to reset"`. Additions over the mockup: `role="slider"`, `aria-valuenow`,
   ArrowUp/Down ±0.5 dB, Home/End. Sends `setMasterVolume {channel, value}` (throttle to
   one request per animation frame while dragging).
4. **IN** toggle: mono 9 px, `tracking-wider`; on → primary, off → muted-foreground/60,
   hover foreground; `title="Feed from main input"`, `aria-pressed`. Toggles only the
   `"<in>"` entry of `Channel.sources` (`setSources`), preserving any `"chN"` entries. If a
   channel has `"chN"` sources (legacy feature the mockup lacks), show a read-only
   `from CH n` mono 9 px label after IN.

### 3.4 Blocks

92×60, radius 6 px, `flex-col items-center justify-center gap-1`, `cursor: grab`
(`grabbing` while dragging), `transition-all`.
- Border 1 px `color-mix(in oklab, <cat> 45%, var(--border))`.
- Background panel-2 with `linear-gradient(180deg, color-mix(in oklab, <cat> 18%,
  transparent), transparent)`.
- Accent bar: absolute left 0, top 8, bottom 8, width 3, right corners rounded, `<cat>`.
- Icon 18 px in `<cat>` (our existing `Icon.tsx` set), label 11 px medium, `leading-none`,
  the block's display name (truncate with ellipsis, full name in `title`).
- Hover (not selected): `filter: brightness(1.25)`.
- Selected: 2 px ring in `<cat>` with a 2 px background-coloured offset. Must not be
  clipped (the scroll container is the only clipping ancestor and the canvas has ≥ 24 px
  around every block).
- Bypassed: opacity .4.
- Valid split target while a split drag is active: 2 px ring primary/70.
- No per-card bypass/remove buttons (the mockup removed them; the footer has Bypass /
  Reset / Delete).
- **Split handle**: 12 px circle centred on the right edge (`right:-7px; top:50%;
  translateY(-50%)`), 2 px background-coloured border, fill muted-foreground,
  `cursor: crosshair`, `title="Drag to another channel to split"`. Hidden (opacity 0) until
  the block is hovered; on handle hover: scale 1.5, fill primary. Additions over the mockup
  for touch: also visible when the block is selected and always on `(pointer: coarse)`, with
  an invisible ≥ 32 px hit area.

### 3.5 Empty cells

Every empty cell is a 92×60 drop zone with a centred indicator:
- idle: 24×24 dashed box, transparent border, 12 px `+` transparent;
- hover: border muted-foreground/40, `+` muted-foreground;
- drag over (valid drop): fills the cell, border primary, background primary/10, `+` 18 px
  primary.
Additions: the cell is a `<button>` with `aria-label="Channel N, slot M, empty"`. Click /
Enter: if a block type is selected in the tray (`selectedBlockType`), place it here;
otherwise open `AddBlockModal` targeting this cell.

### 3.6 Cables (splits)

Per enabled or disabled routing, in the cable SVG:
1. halo: `cablePath`, stroke background, 6 px;
2. wire: stroke wire, 2 px (3 px when hovered);
3. flow overlay: stroke background, 2 px, `.wire-flow`;
4. source dot: circle r 4 at the source anchor, fill wire.
Disabled routing (ours; the mockup has none): wire at opacity .35, no flow overlay.
Hit path (second SVG): same path, transparent stroke 14 px, `pointer-events: stroke`,
`cursor: pointer`. Hover → that cable is "hovered" and a hint chip appears at the
canvas top-right (`right 8, top 0`, panel-2, 10 px muted-foreground, `✕ click cable to
remove`).

---

## 4. Interactions

| # | Gesture | Mockup behaviour | Engine call | Port additions |
|---|---|---|---|---|
| I1 | Drag a tray item onto an empty cell | creates the block with default params and selects it | `addBlock {channel, type, name, col}` | touch: existing long-press path (`touch.ts`) must work |
| I2 | Drag a block onto an empty cell (any row) | moves it; then removes every link that became invalid | `placeBlock {channel, block, toChannel, col}` (engine prunes) | touch: pointer drag (`useTouchDrag`); occupied cells are not targets (no swap/insert) |
| I3 | Click a block | selects it | — | Enter/Space on a focused block selects |
| I4 | Click empty canvas background | clears selection | — | Escape also clears |
| I5 | Delete / Backspace | removes the selected block and its links | `removeBlock` (engine drops routings) | ignore when focus is in `input`, `textarea`, `select` or `[contenteditable]` |
| I6 | Press the split handle and drag | dashed preview `cablePath(anchor → pointer)`, stroke wire 2 px, `dasharray 4 4`; valid targets ringed | — | pointer capture so the drag survives leaving the canvas |
| I7 | Release on a valid target | creates the link | `createRouting {source, dest}` | engine rejection → existing `notice` toast |
| I8 | Release anywhere else | cancels | — | Escape cancels |
| I9 | Hover a cable | thickens to 3 px, hint chip shows | — | — |
| I10 | Click a cable | removes it immediately | `removeRouting {id}` | right-click → existing routing menu (Enable/Disable, gain, Delete); focusable hit path, Delete removes, Enter opens the menu |
| I11 | Mute / trim / IN in the channel header | as §3.3 | `setMute`, `setMasterVolume`, `setSources` | keyboard as §3.3 |

**Valid split target** (`isValidSplitTarget(src, dst, routings)`):
`dst.row ≠ src.row` **and** `dst.col > src.col` **and** no routing `src → dst` exists.
Because every split goes strictly right, cycles are impossible; the engine still runs its
own cycle check.

**Prune rule after a move** (engine-side, mirrored in a pure UI helper for tests):
drop every routing whose endpoints now satisfy `same row` or `dest.col ≤ source.col`.

There is no undo, no multi-select, no zoom/pan, no copy/paste in the mockup; not in scope.

---

## 5. Shell layout (header, tray, footer)

- **App frame**: column flex, full height: header 80 px / main (tray + canvas) / footer
  160 px. Desk (≥ 840 px): tray 208 px left of the canvas. Below 840 px keep today's
  responsive behaviour (tray becomes the existing palette strip above the canvas; canvas
  scrolls; editor stacks).
- **Header** (h 80, border-b, panel, px 20, gap 24): Input `HeaderKnob` (label right) ·
  divider (1×40 border) · preset navigator (prev chevron 28×28 · name block 128 wide: preset
  name 14 px semibold, subtitle 11 px muted-foreground = current source `Riff` / `Live` /
  `Silence` · next chevron · list button → `PresetBrowser`) · 8 slot chips `A`–`H` (32×32,
  radius 4, mono 12; active primary on primary-foreground; idle muted on muted-foreground,
  hover foreground) · spacer · `TUNER` button (mic icon, border, px 12 py 8, 12 px,
  `tracking-widest`; opens `TunerModal`) · `DSP <cpu>%` (value foreground) and
  `LAT <oneWayLatencyMs> ms` (value primary), mono 11 px, real values · divider · Master
  `HeaderKnob` (label left).
  Everything today's `TopBar` has that the mockup lacks (source toggle, IN/OUT meters,
  buffer-size select, Models button, Footswitch button) stays, grouped between `TUNER` and
  the DSP/LAT readouts in the same mono 11 px style; below 820 px they collapse with the
  existing `.compact` rules.
- **Tray** (`Rack.tsx`, w 208, panel, border-r, px 12 py 12, scrolls): one group per
  registry category (in the mockup order Modelling, Distortion, Tone, Dynamics, Time,
  Modulation, Pitch, Utility, then the rest); group header 10 px semibold uppercase
  `tracking-widest` in the category colour, mb 4; items 13 px foreground/85, 14 px icon in
  the category colour, gap 10, px 8 py 6, radius 4, hover muted, `cursor: grab`. Existing
  search, keyboard selection and touch long-press stay.
- **Footer** (`Editor.tsx`, h 160 on desk, border-t, panel, px 24, gap 40): identity column
  w 160 — icon 16 px + block name 16 px semibold; `Channel N · Slot M` 11 px
  muted-foreground (M = `col + 1`); buttons `Bypass` (active: primary border, primary/15
  bg, primary text) / `Reset` / `Delete`, 11 px, radius 4, border, px 8 py 4. Controls area
  (flex-1, centred, gap 32) renders today's controls **unchanged** (Knob, Choice,
  Footswitch, Expression, EQ, Looper editors). No selection → centred 14 px
  muted-foreground hint: `Select a block to edit · drag from the tray to add · drag a
  block's right edge to another channel to split`.

---

## 6. What is not ported, and why

- The mockup's knob (`wt`): product-owner rule — keep ours.
- The mockup's hard-coded block list/defaults: the engine registry is the source of truth.
- HTML5 drag-and-drop as the *only* drag mechanism: it never fires on touch (Pi
  touchscreen); we keep HTML5 DnD for mouse and the existing pointer/long-press path for
  touch.
- Google Fonts CDN: offline app; fonts are bundled.
- Click-to-delete as the only cable control: kept as the primary gesture, but the existing
  enable/disable + gain controls move to the cable's context menu so no capability is lost.

---

## 7. Card map and ownership

| Card | Role | Owns | Depends on |
|---|---|---|---|
| t_484ef954 | builder | this doc + `docs/design/mockup/*` | — |
| t_d165d1a2 | builder (engine) | `col`, `placeBlock`, split rule + prune; `engine/`, `graph/` (`NodeDoc.col`), `docs/IPC-PROTOCOL.md` | — |
| t_cd53f9be | builder (engine) | `muted`, `setMute` | — |
| t_9ec98ec1 | ui-builder | `protocol.ts`, `state.ts` wrappers, `ui/src/grid/geometry.ts` (+ tests), Cypress fake engine | t_484ef954, t_d165d1a2, t_cd53f9be |
| t_1a540c56 | ui-builder | `GridCanvas.tsx`, `ChannelHeader.tsx`, grid tokens/utilities in `tailwind.css` (`--color-wire*`, `.grid-dots`, `.wire-flow`), retiring `Chain.tsx` | t_9ec98ec1 |
| t_0fe28a1e | ui-builder | `GridCables.tsx`, split handle + drag, cable menu; retiring `RoutingLines.tsx` | t_1a540c56 |
| t_f6300961 | ui-builder | `TopBar.tsx`, `Rack.tsx`, `Editor.tsx` identity column, `theme.ts`, palette/font tokens in `tailwind.css`, `App.tsx` frame | t_1a540c56 |
| t_d8c4180b | reviewer | end-to-end check on `main` | all of the above |
