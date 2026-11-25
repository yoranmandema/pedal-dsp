# Block visual styles

Design doc for per-block theming of the pedal UI (Preact + TypeScript + Tailwind
v4, in `ui/`). It is the single source of truth for how a block is painted so
that a builder can implement it without further design decisions.

**Scope of the palette.** The app has exactly one theme — dark. All palette
tokens quoted here are the dark tokens from `ui/src/tailwind.css`. Do not add a
light mode, a theme toggle, or `prefers-color-scheme` handling.

**Sections.** (1) Type-to-theme mapping — the `blockTheme` registry. (2)
Theme mechanism — CSS variables, Tailwind tokens, SVG linework, control layout.
(3) Shared wrapper component API — `<BlockEditorFrame>`. (4) Accessibility
constraints. (5) The full block-type list with a one-line style concept per
type. (6) Follow-up implementation cards.

Related docs: `docs/ARCHITECTURE.md`, `docs/IPC-PROTOCOL.md`,
`docs/notes/ui-survey.md`, `docs/notes/block-type-inventory.md`.

---

## 1. Type-to-theme mapping (`blockTheme` registry)

The registry maps a **block-type id** (the `type` field the engine sends, e.g.
`"amp.basic"`) to a theme object. It lives in one file, `ui/src/themes.ts`,
which is the only place a theme is *declared*. Everything that paints a block
(card artwork, controls, identity pill) reads from this registry through the
single entry point `blockTheme(type)`.

```ts
// ui/src/themes.ts
import type { JSX } from "preact";
import type { Category, ParamSpec } from "./protocol";

/** How the block physically reads. Controls the material, the artwork glyph
 *  and the default stroke/fill family of the artwork. */
export type Material = "metal" | "wood" | "plastic" | "glass" | "paper";

/** The palette a theme exposes. Named so every control component reads the
 *  same variables. */
export interface BlockPalette {
  /** Block faceplate / artwork background (the raised plate). */
  base: string;
  /** Recessed surfaces inside the artwork (grilles, valve bodies, rear panel). */
  surface: string;
  /** Solid inset behind every control so chrome has a predictable 3:1 target. */
  bg: string;
  /** Primary linework — a block's edge, grilles, jack markers. */
  line: string;
  /** Secondary linework — mounting screws, trace, inner detail. */
  lineSoft: string;
  /** Highlight linework — edge highlight, lit state. */
  lineHi: string;
  /** The category/hue accent (e.g. the violet of the amp). */
  accent: string;
  /** Tint of `accent` for disabled/ghosted accents. */
  accentSoft: string;
  /** Text on dark artwork. */
  text: string;
  /** Secondary text (labels, muted readouts). */
  textMuted: string;
  /** The knob / slider / footswitch chrome colour. */
  control: string;
  /** The "bypassed / off" variant of the chrome. */
  controlOff: string;
  /** The material this theme reads. */
  material: Material;
}

/** Coordinates inside the artwork for a knob that sits *in* the artwork.
 *  x/y are a 0..100 square, top-left origin, relative to the artwork's
 *  `viewBox`. `anchor` describes the knob's pivot (centre, bottom-right…). */
export interface BlockAnchor {
  x: number;            // 0..100
  y: number;            // 0..100
  anchor: "centre" | "bottom-right" | "top-left" | "bottom-left" | "top-right";
}

/** The theme for one block type. */
export interface BlockTheme {
  palette: BlockPalette;
  /** Inline SVG artwork as a data URL. `undefined` = use a structural default
   *  (a plain rounded-rect plate with the category hatch) derived from
   *  `material` + `palette`. */
  artwork?: string;
  /** Per-state artwork; only `on` | `off` are ever observed. */
  artworkState?: Record<"on" | "off", string>;
  /** Knob positions that sit *in* the artwork (see §2). */
  anchors?: Record<string, BlockAnchor>;
  /** CSS variables that override the default derivation for this type. */
  vars?: Partial<Record<BlockVar, string>>;
}

/** The fixed set of CSS custom properties a theme owns. */
export const enum BlockVar {
  hue        = "--block-h",
  material   = "--block-material",
  state      = "--block-state",   // "on" | "off" | "dim"
  palette    = "--block-palette", // JSON-serialised palette (rarely used)
}

/**
 * The block-theme registry: the palette the UI, the theme resolver and the
 * accessibility pass all consults. One entry per *built-in* block type. A
 * plugin / third-party block that is not in this record is handled by the
 * fallback (§1.4), never by a hard-coded "unknown" grey.
 */
export interface BlockThemeRegistry {
  /** Exact registry id → theme. */
  get(type: string): BlockTheme | undefined;
  /** Fall back to the block's *category* theme. */
  byCategory(category: Category): BlockTheme;
  /** Fall back to the unknown-type theme — readable, never a hard-coded grey. */
  fallback(type: string, category: Category): BlockTheme;
}

/** The single entry point: given a block type id, return the theme to paint
 *  with. Lookup order: exact `type` → category theme → unknown-type fallback.
 *  The engine never sees this; it only reads state.registry / params. */
export function blockTheme(type: string, category: Category): BlockTheme {
  return registry.get(type) ?? registry.byCategory(category) ?? registry.fallback(type, category);
}
```

### 1.1 Lookup order & fallback

- **Exact id.** `registry.get("amp.basic")` wins. This is the only place a
  builder may special-case a type.
- **Category fallback.** `blockTheme` then falls back to the theme of the
  block's `Category` (the `"Tone"`, `"Distortion"`… string in `state.registry[]`).
  This means a brand-new built-in block that merely *adds* an entry to the
  registry inherits a family theme without a new artwork.
- **Unknown-type fallback.** Any type that is not in the registry — plugin,
  third-party, or a typo'd registry key — resolves to the **unknown-type
  fallback theme**, which is *derived from the block's category*, never a
  hard-coded grey. Concretely, the fallback derives the hue from the passed
  `Category` and builds a readable palette around it:
  ```ts
  export function fallbackTheme(type: string, category: Category): BlockTheme {
    const hue = categoryHue(category);
    return {
      palette: buildPalette(hue),
      artwork: undefined,
      vars: { "--block-h": String(hue) },
    };
  }
  ```
  The `buildPalette(hue)` helper produces a coherent dark palette (see §2.1
  for the palette fields) from a single hue, so a plugin block that lands in,
  say, `Category::Dynamics` reads the green accent family and is at least
  *readable* — and it can never clash with a built-in hue.

### 1.2 Per-type entry shape

Each entry is a typed object with a comment naming the intended palette,
material and linework motif. Example (`amp.basic`):

```ts
ampBasic: {
  palette: {
    base: "#241a12", surface: "#1a120c", bg: "#151008", line: "#d8b46b", lineSoft: "#a88352",
    lineHi: "#f0c886", accent: "#e8a33d", accentSoft: "#8a5f2a",
    text: "#1a120c", textMuted: "#d8b46b", control: "#f0c886", controlOff: "#5a3a1c",
    material: "metal",
  },
  artwork: ampFaceplateSvg, // data URL, see §2
  anchors: {
    voice:  { x: 17, y: 74, anchor: "centre" },
    gain:   { x: 33, y: 74, anchor: "centre" },
    bass:   { x: 50, y: 74, anchor: "centre" },
    mid:    { x: 67, y: 74, anchor: "centre" },
    treble: { x: 83, y: 74, anchor: "centre" },
    level:  { x: 50, y: 88, anchor: "bottom-right" },
  },
},
```

### 1.3 File layout

- `ui/src/themes.ts` — the registry, the `BlockTheme` family of types, the
  lookup function and the fallback. **Do not split the registry across files.**
- `ui/src/themes-inline.ts` — the *data URL* for each artwork, inlined by the
  helper in §2. Writing a theme entry is one line: `artwork: ampFaceplateSvg`.
- `ui/src/components/BlockArtwork.tsx` — the only component that renders an
  `<svg>` from a theme's data URL.
- `ui/src/tailwind.css` — the **CSS-level** variables (§2), not the palette
  itself. The palette stays in `themes.ts`.

### 1.4 What the registry is *not*

- It is **not** a colour palette the UI hard-codes. The UI only reads
  `blockTheme(type).palette.*` and the CSS-level variables.
- It is **not** a place to add per-instance params. Parameters live on the
  block (`Block::params`), and controls read them through the wrapper's
  render-prop (§3) and the `ParamSpec` from `state.registry[]`.
- It does **not** encode layout. Artwork coordinates are laid out by the
  wrapper (§3), not by the theme.

---

## 2. Theme mechanism

### 2.1 CSS variables & naming conventions

Naming is the contract between `themes.ts` and `tailwind.css`. Every variable
is a single token, written `--block-<kebab-field>`.

| Variable | Set by | Read by | Meaning |
|---|---|---|---|
| `--block-h` | `themes.ts` (`vars["--block-h"]`) | CSS + `<BlockArtwork>` | Block-type hue, 0–360 (0 = red). Falls back to `--cat-h`. |
| `--block-material` | `themes.ts` | CSS | `metal` \| `wood` \| `plastic` \| `glass` \| `paper`; drives the structural default. |
| `--block-state` | `BlockEditorFrame` (bypass) | CSS + artwork | `"on"` \| `"off"` \| `"dim"`; switches the artwork to its `artworkState.off` data URL / muted stroke tone. |
| `--block-palette` | `themes.ts` (rare) | CSS | JSON-serialised palette, for any CSS that must read a colour programmatically. |
| `--block-base` | `themes.ts` | CSS + artwork | Faceplate / artwork background (raised plate). |
| `--block-surface` | `themes.ts` | CSS + artwork | Recessed surfaces inside the artwork (grilles, valve bodies, rear panel). |
| `--block-line` | `themes.ts` | CSS + artwork | Primary linework — edge, grilles, jack markers. |
| `--block-line-soft` | `themes.ts` | CSS + artwork | Secondary linework — mounting screws, trace, inner detail. |
| `--block-line-hi` | `themes.ts` | CSS + artwork | Highlight linework — edge highlight, lit state. |
| `--block-accent` | `themes.ts` | CSS + artwork | The category/hue accent (e.g. the violet of the amp). |
| `--block-accent-soft` | `themes.ts` | CSS + artwork | Tint of `accent` for disabled/ghosted accents. |
| `--block-text` | `themes.ts` | CSS + controls | Text on dark artwork (labels, values). |
| `--block-text-muted` | `themes.ts` | CSS + controls | Secondary text (labels, muted readouts). |
| `--block-control` | `themes.ts` | CSS + controls | The knob / slider / footswitch chrome colour. |
| `--block-control-off` | `themes.ts` | CSS + controls | The "bypassed / off" variant of the chrome. |
| `--block-bg` | `themes.ts` | CSS + controls | Solid inset behind every control so chrome has a predictable 3:1 target. |
| `--cat-h` | `theme.ts` (existing, inline) | existing `catScope` | Category hue; the **fallback** for `--block-h` and the *category* accent only. |
| `--cat` / `--cat-soft` | `tailwind.css` `@theme inline` | existing elements | Derived from `--cat-h`; the *category* accent and soft tint only. |
| `--color-*` | `tailwind.css` `@theme` | app chrome (background, panel, line, text, amber, danger…) | The app chrome, **not** the block artwork. |

Rules:

- A block card is **always** a descendant of `.cat-scope` (it is the only
  element that owns `--cat-h`) **and** owns `--block-h` directly. `--cat-*` /
  `--color-cat-*` therefore still express the *category* (for the rack chips,
  drag ghost, selection ring); `--block-*` express the *type*.
- No existing `--color-*` token is repurposed by a block theme. A block's
  artwork reads `--block-*` only.
- Knobs inherit theme variables, they are never hard-coded to amber: the
  `Knob` component root sets `style={{ "--knob-color": "var(--block-control)" }}`
  (or reads it from an ancestor), so the knob chrome is whatever the theme
  declares — e.g. the amp plate's brass `line-hi` or the cab's silver
  `control`. The same variable drives the artwork's accent highlight ridge,
  so a lit knob sits *on* the artwork, not floating over it.

### 2.2 Tailwind tokens used by block theming

The block frame and controls use these existing `@theme` tokens (listed here
so builders do not invent tokens):

```
--block-h
--block-material
--block-state
--block-palette
--cat-h            (existing)
--cat, --cat-soft  (existing)
--color-bg, --color-panel, --color-line, --color-line-soft, --color-line-hi
--color-text, --color-muted, --color-amber (+ -dim/-hi/-lo/-edge/-ink)
--color-ok, --color-inset, --color-well, --color-surface, --color-raised,
--color-raised-2, --color-danger (+ -text/-bg/-line)
--ui-touch, --ui-ctrl, --ui-ctrl-max, --ui-ctrl-row, --ui-bar, --ui-topbar-h,
--ui-id-w, --ui-knob
--radius-*, --font-*, --text-*, --shadow-*, --spacing
```

### 2.3 SVG assets: organisation, loading, inheritance

**No external SVG files exist today** — all icons are inline JSX with
`fill="none"`, `currentColor`, `stroke-width` and round caps, so glyphs inherit
the category hue. Per-block artwork (amp faceplate, cab grille, …) continues
that rule, with one rule added:

- **Storage.** Author artwork as a normal `.svg` file in
  `ui/src/assets/artwork/<type>.svg`. A design-time helper
  `Asset.inline(path)` base64-encodes it and emits the string constant that
  `themes.ts` then references. This is an *author-time* step (a script),
  **not** a runtime fetch: the committed `themes.ts` carries the data URL.
- **Inlining.** `<BlockArtwork url={theme.artwork} />` decodes the data URL
  into a single `<svg>` element per block type. The `<svg>` is a DOM element,
  so all its strokes/fills can be `var(--block-*)` — you cannot `currentColor`
  an `<img>`.
- **Rendering contract of `<BlockArtwork>`:**
  ```tsx
  <svg
    aria-hidden="true"
    focusable="false"
    viewBox="0 0 100 100"
    preserveAspectRatio="xMidYMid meet"
    width="100%"
    height="100%"
    style={{ color: "var(--block-line)" }}
  >
    {decodeDataUrl(url)}   // <g> + shapes, no <title>/<desc>
  </svg>
  ```
  - `aria-hidden="true"` and `focusable="false"` so the artwork is
    **decorative only** (§4).
  - `viewBox="0 0 100 100"` is the fixed plane; controls anchor to it (§2.4).
  - The artwork's primary `stroke` is `var(--block-line)` and its primary
    `fill` is `var(--block-base)`; accent details use
    `var(--block-accent)`.
  - For the `off` state, the frame sets `style={{ "--block-state": "off" }}`
    and the artwork switches to its `artworkState.off` data URL (or a muted
    `var(--block-line-soft)` stroke tone) so a bypassed amp reads as "off",
    not "on but greyed".
- **Categories inherit through CSS, not files.** The category hatch on the
  structural default is drawn with `currentColor` so it reads the block hue
  from `--block-h`, and the category accent (`--cat`) keeps drawing the rack
  chips. A theme never re-declares a category's hue.

### 2.4 Control layout over the artwork

Controls are laid out in **two layers** so knobs sit *on* the artwork while
non-artwork controls tile around it.

1. **In-work-artwork knobs** (amp knobs on the faceplate, cab knobs on the
   speaker cloth). The theme declares `anchors` mapping a param id →
   `{ x, y, anchor }` (0–100 plane, top-left origin). The frame positions each
   knob absolutely inside `.block-frame` (which fills the cell) using:
   ```ts
   const style: React.CSSProperties = {
     position: "absolute",
     left:   `${x}%`,
     top:    `${y}%`,
     transform:
       anchor === "centre"        ? "translate(-50%, -50%)" :
       anchor === "bottom-right"  ? "translate(-100%, -50%)" :
       anchor === "bottom-left"   ? "translate(0, -50%)" :
       anchor === "top-right"     ? "translate(-100%, 0)" :
                                    "translate(0, 0)",           // top-left
   };
   ```
   The knob is a whole `var(--ui-knob)` cell, so it snaps to the artwork's
   grid and never overlaps its edges. Two knobs on the same artwork never
   overlap because the theme author places them on the 0–100 plane with at
   least one cell's clearance.
2. **Out-of-work-artwork controls** (gain, voicing, the whammy's
   footswitch/expression). The frame lays these out on a CSS grid over the
   card frame:
   ```css
   .block-controls {
     position: relative;
     display: grid;
     grid-template-columns: repeat(auto-fit, minmax(var(--ui-ctrl), 1fr));
     gap: var(--space-2);
     align-items: center;
   }
   ```
   Each param is a `data-param` slot: `<div class="knob-unit" data-param="gain">`.
   The frame iterates `spec.params` and renders the bound control into the slot
   whose `data-param` matches. Controls that are not bound to an anchor slot
   use the grid so they never collide with the artwork; controls that *are*
   bound (through `anchors`) render absolutely positioned over the artwork.
   The frame **reserves** `var(--ui-ctrl)` cells for each control and never
   lets a control widen the grid track (the chain panel is `overflow-auto` and
   `min-w-0` is load-bearing — see §2.6 of `ui-survey.md`).
3. **Identity + header.** `.block-identity` holds the block name (truncated)
   and the bypass pill, styled with the block's `--block-palette` so it
   never reads the app's default panel colour. It sits as a thin header above
   the artwork; on the phone the frame stacks so the identity is always
   reachable.

### 2.5 Knobs & controls inherit the theme

The `Knob` (used for every `number` param, and the weakest accessible
control — see §4) reads:

- arc: `stroke: var(--block-control)` / `stroke-dasharray: t*75 100` (the
  existing `pathLength` trick), indicator `fill: var(--block-line)`;
- end-stop labels: `color: var(--block-text)`;
- the `title` and `aria-label` carry the param name, never the theme.

`ChoiceControl`, `ExpressionControl` and `FootswitchControl` read
`--block-control` (segment on / track) and `var(--block-text)`. The
`ExpressionControl` slider's track uses `var(--block-line-soft)` and its
thumb `var(--block-control)` with a `var(--block-bg)` border so it is always
visible over artwork. Nothing in a theme hard-codes `var(--color-amber)` for
a control.

### 2.6 What the frame never does

- Never changes the graph, the engine, or the IPC.
- Never sets `pointer-events: none` on the artwork in a way that hides controls
  (the artwork itself is `pointer-events: none`; controls are real elements).
- Never adds a new CSS file, CSS module, or `className` — the app uses `class`
  (Preact 10) throughout.
- Never renders a second `.block-frame` tree inside an existing block editor
  without the migration recipe in §3 (nested frames are a leading cause of
  z-index escalation).

---

## 3. Shared wrapper component API — `<BlockEditorFrame>`

`<BlockEditorFrame>` is the single wrapper every block editor uses. It owns
the identity, the artwork, the control grid and the bypass state; the *per-block*
editor supplies only the controls.

### 3.1 Props

```tsx
// ui/src/components/BlockEditorFrame.tsx
import type { JSX } from "preact";
import type { Block, ParamSpec } from "../protocol";

export interface BlockEditorFrameProps {
  /** Registry id of the block, e.g. "amp.basic". */
  type: string;
  /** The block instance (id, type, name, bypass, params). */
  block: Block;
  /** The channel id (from `state.selected.channel`) used for mutations. */
  channel: string;
  /** The full param list for `type`, from the engine registry. */
  params: ParamSpec[];
  /** Current real-valued params for this block. */
  value: Record<string, number>;
  /** Route a param change to the engine. */
  onChange: (channel: string, block: string, param: string, value: number) => void;
  /**
   * Render-prop over the block's params. `slot.param` is the spec; `slot.value`
   * is the current value; `slot.onChange(v)` writes `param`. Return a component
   * to bind it, or `null` to skip (the frame shows a placeholder for
   * un-rendered params). Any param with no binding falls back to
   * `defaultControl`.
   */
  children: (slot: {
    param: ParamSpec;
    value: number;
    onChange: (v: number) => void;
  }) => JSX.Element;
  /** Fallback control for a param with no explicit binding. */
  defaultControl?: (p: ParamSpec, value: number, onChange: (v: number) => void) => JSX.Element;
  /** Extra class for the frame root (the parent sizes the bar cell). */
  className?: string;
}
```

### 3.2 Slots & rendering contract

The frame renders exactly four layers; a parent CSS rule may only set the
frame's **width**, never its height (the frame is sized by its bar cell).
The frame is a **relative** sizing context.

```
.block-frame                 (relative, square 1/1 desk / 4/3 phone)
├── .block-identity          (name + bypass pill; thin header)
├── .block-artwork           (absolute inset:0; the <BlockArtwork> svg)
├── .block-controls          (the param grid; data-param slots)
└── .block-postartwork       (optional: footswitch/expression for the whammy)
```

Rendering contract:

- The frame resolves the spec from `state.registry[block.type]` (it reads the
  same store as `Editor.tsx`, so it needs no new props for the spec).
- For each param the frame calls `children({ param, value, onChange })`. The
  *existing per-block editors* (`GraphicEqEditor`, `MultibandCompressorEditor`,
  `WaveShaperEditor`, `LooperEditor`) bind their existing controls to the
  matching param ids and return the frame's `defaultControl` for everything else.
- `defaultControl` is keyed by **kind**, not by id: `number` → `Knob`,
  `choice` → `ChoiceControl`. A builder who only wants to add
  artwork to a block that already has controls does not write any control
  code — the default mapping suffices — and only needs to add the artwork
  entry to `themes.ts`.
- The whammy (`time.pitchdelay`) is the only block that binds special
  controls: its `mode` param (a `choice`) uses `ChoiceControl`, and its
  `rate` param (a `number`) can optionally bind an `ExpressionControl`
  when an expression pedal is mapped via MIDI. These bindings are
  supplied by the editor's render prop, not by `defaultControl`.
- Bypass is owned by the frame (`--block-state` → `"off"`), so the artwork and
  controls respond without the editor touching the artwork code.

### 3.3 Layout contract

- The frame fills its bar cell. The parent (the rack bar) sizes the cell to
  `var(--ui-bar)` (128 px on desk, 100% on phone); the frame itself has no
  fixed width/height and no `aspect-ratio`. It is a **relative** sizing context.
- `.block-artwork` is `position: absolute; inset: 0; width: 100%; height: 100%`
  and `overflow: hidden`. Artwork anchors are measured against this plane.
- `.block-controls` is `display: grid; grid-template-columns:
  repeat(auto-fit, minmax(var(--ui-ctrl), 1fr))`. A bound anchor control
  renders `position: absolute` inside `.block-frame` (the grid container only
  reserves cells; it does not paint the anchors).
- The frame adds `aria-label` to every interactive control it renders through
  `defaultControl` (or the child's `aria-label`).
- The frame never wraps its children in `Fragment` shadow DOM; it renders
  **one** real DOM tree into the bar.
- For blocks with many params (e.g. `dynamics.multiband_compressor` with 34),
  the grid overflows into `.block-postartwork` and the parent bar is
  `overflow-x: auto` so the user can scroll horizontally. The frame never
  forces a minimum cell width that would break the rack layout.

### 3.4 Migration recipe (existing block editors)

The current `Editor.tsx` hard-codes the control mapping. The migration is:

1. **Add the defaults.** Add `Knob` and `ChoiceControl` (already in the
   worktree) to the frame's `defaultControl` mapping by param kind.
2. **Make the knobs accessible first** (§4) — give `Knob` `role="slider"`,
   `aria-valuenow/min/max`, `aria-label` and arrow-key handling.
3. **Wrap.** Replace the per-block body in each editor with:
   ```tsx
   <BlockEditorFrame
     type={block.type}
     block={block}
     channel={sel.channel}
     params={spec.params}
     value={block.params}
     onChange={setParam}
   >
     {slot => <LegacyControls {...slot} />}
   </BlockEditorFrame>
   ```
   where `LegacyControls` binds each of its existing controls to the matching
   param id and returns the frame's `defaultControl` for the rest. No new
   props are introduced; `setParam` already exists.
4. **Remove the whammy special-casing.** The whammy (`time.pitchdelay`)
   currently spawns `FootswitchControl` / `ExpressionControl` as always-on
   orphaned controls outside the artwork. After migration, its `mode` param
   (a `choice`) binds to `ChoiceControl` via the frame's default, and its
   `rate` param (a `number`, "Sweep rate") binds to an `ExpressionControl`
   **only when an expression pedal is mapped via MIDI** — the frame does
   not render expression/footswitch controls unless the editor's render prop
   explicitly returns them for that param. The `isGraphicEq` split becomes a
   `tone.graphic_eq` slot that returns `<GraphicEqEditor />` (it is an
   `<svg>` control over the frame, no artwork override needed).
5. **Downstream.** `Rack`, `Chain`, `SelectionRing` and the dragging
   interactions are unaffected — they read `state`, not the editor DOM.

### 3.5 Small example (`amp.basic`)

```tsx
function AmpEditor() {
  const s = state.value;
  const sel = selected.value;
  const spec = s?.registry[sel?.type];

  return (
    <BlockEditorFrame
      type={sel?.type ?? ""}
      block={sel?.block ?? { id: "", type: "", name: "", bypass: false, params: {} }}
      channel={sel?.channel ?? ""}
      params={spec?.params ?? []}
      value={sel?.block?.params ?? {}}
      onChange={setParam}
    >
      {slot => {
        switch (slot.param.id) {
          case "voice":
            return <ChoiceControl p={slot.param} value={slot.value} onChange={v => slot.onChange(v)} />;
          case "gain":
          case "bass":
          case "mid":
          case "treble":
          case "level":
            return (
              <Knob
                name={slot.param.name}
                value={slot.value}
                min={slot.param.min}
                max={slot.param.max}
                defaultValue={slot.param.default}
                unit={slot.param.unit}
                format={v => formatParam(slot.param, v)}
                onChange={v => slot.onChange(v)}
              />
            );
          default:
            // every non-choice param in v1 is a knob
            return (
              <Knob
                name={slot.param.name}
                value={slot.value}
                min={slot.param.min}
                max={slot.param.max}
                defaultValue={slot.param.default}
                unit={slot.param.unit}
                format={v => formatParam(slot.param, v)}
                onChange={v => slot.onChange(v)}
              />
            );
        }
      }}
    </BlockEditorFrame>
  );
}
```
The theme for `amp.basic` (§1.2) places the six knobs at the 0–100 plane, so
the knobs render **on** the amp faceplate rather than beside it. `GraphicEq`
stays a slot component and does not add a theme entry.

---

## 4. Accessibility constraints

These constraints are **hard** for every block theme. A theme that fails any
of them is not usable; the accessibility pass (implementation card #8) verifies
them with the vision tool at 800 px and 1440 px and with the contrast tool.

### 4.1 Contrast ratios (WCAG 1.4.3 / 1.4.11)

- **Text:** all label text on artwork ≥ **4.5:1** (normal) / **3:1** (large).
  This is measured at the control's bounding box, never against the frame.
- **Controls (non-text):** knob ring, slider track, thumb, footswitch, choice
  segment, focus ring ≥ **3:1** against the *local artwork background* where
  the control sits. The frame supplies `var(--block-control)` and
  `var(--block-control-off)` for all chrome, and `var(--block-text)` /
  `var(--block-textMuted)` for all labels. **Never** use a block's `accent`
  for control text.
- A control whose artwork background is a photographic/complex glyph must
  still carry a solid `var(--block-bg)`-coloured inset so the control chrome
  has a predictable 3:1 target.

### 4.2 Focus

- Every interactive control inside the frame is keyboard operable:
  `Knob` gains `role="slider"`, `aria-label`, `aria-valuenow/min/max` and
  arrow-key handling (following the `HeaderKnob` precedent — the only other
  custom knob today). `ChoiceControl`, `ExpressionControl` and `FootswitchControl`
  already have their ARIA roles (`radiogroup`/`radio`, `range`, `switch`) and
  are unchanged semantically.
- `:focus-visible` draws a visible 3px `var(--block-accent)` ring offset 2px on
  every control, matching the app's existing focus style but using the block's
  accent colour.
- The artwork SVG is `focusable="false"` and `aria-hidden="true"`, so it never
  steals focus, contributes to the tab order, or presents a focus ring by
  itself.

### 4.3 Controls stay legible & focusable

- Controls are never translucent-over-artwork: each control root sits on a
  `var(--block-bg)`-coloured inset.
- The frame never uses `pointer-events: none` on the artwork in a way that
  would prevent a control from receiving pointer or keyboard events.
- `Knob` is the weakest accessible control today (no role, no tabIndex, no
  keyboard support). It is **in scope** for this doc: the migration in §3.4
  and the re-skin in card #5 add full slider semantics to it.

### 4.4 Prefers-reduced-motion

- The frame adds `@media (prefers-reduced-motion: reduce)` that removes the
  knob rotation sweep, the artwork's lit-state pulse, and any fade-in on the
  frame. No non-essential motion is introduced by a block theme; a theme that
  wants a "lit" highlight uses a colour change, not an animation.
- The app has **zero** `prefers-reduced-motion` rules today; this doc pins
  the one the frame needs. Do not add more.

### 4.5 Forced-colors / high-contrast

- In `forced-colors` the artwork paints as a two-tone plate
  (`var(--block-base)` fill, `var(--block-line)` stroke) and all controls fall
  back to the UA glyph.
- The fallback theme maps all colours to the nearest WCAG-contrast pair, so a
  Windows high-contrast theme never loses the control affordances.
- The accessibility pass verifies forced-colors mode by simulating it in the
  browser (no OS-level setting needed: the browser applies the UA forced-color
  stylesheet in a force-dark emulation).

---

## 5. Block type list

Every built-in block type from the engine registry (`graph/src/blocks.rs`
`builtin_registry()` — 26 types), with a one-line style concept. Palette,
material and linework motif are per-type; the family is per-category (the
category hue drives the rack chips and the category accent, per §2).

**I/O**

| type | style concept |
|---|---|
| `io.input` | Neutral grey metal plate; one `input` chevron + one `output` arrow; the only "dead" channel in the interface, so the plate is a plain bracket with a cut corner. |
| `io.output` | Neutral grey metal plate; circular output jacks drawn **into** the plate; the plate is a receptacle, not a body. |

**Split/Merge**

| type | style concept |
|---|---|
| `util.sum` | Amber-tinted metal hatch; 4 weighted input pads (A–D) and one output in a daisy-chain hop; linework is a 4-node summer with a hop glyph. |

**Dynamics**

| type | style concept |
|---|---|
| `dynamics.compressor` | Brushed-metal plate; threshold/level as vertical laser-etched scales, attack & release as two narrow sliders; linework = a compressor "squash" hinge. |
| `dynamics.noise_gate` | Matte-black metal; threshold slider capped at the plate; linework = a shutter that closes with the threshold. |
| `dynamics.multiband_compressor` | Silver metallic plate; 4 band strips (B1–B4) each with threshold/ratio/attack/release/makeup/bypass/solo; linework = a spectral ridge with 4 notches. |

**Tone**

| type | style concept |
|---|---|
| `tone.stack` | Gold/brass metal; 3 band-pass sliders with a stack glyph; linework = a 3-band tone arm. |
| `tone.graphic_eq` | Silver plate; 10 vertical faders with notch markers at centre frequencies; linework = the classic 10-band EQ line. |

**Distortion**

| type | style concept |
|---|---|
| `dist.overdrive` | Ember-red metal; knobs: Drive (log), Level; linework = a warm radial glow ridge. |
| `drive.tube_screamer` | Tube-violet metal; 6 knobs (Drive, Pre Mid, Pre Bass, Pre Treble, Tone, Level); linework = a tube-rectifier silhouette with a blue-glow node on engagement. |
| `drive.precision_drive` | Slate metal with a punch-pump motif; knobs: Gain, Punch, Attack, Release, Tone, Presence, Volume; linework = a diode-feedback clipper trace. |
| `drive.klon` | Cool gunmetal; 4 knobs (Drive, Feel, Tone, Level); linework = a symmetric di-equidistant-clipper trace. |
| `drive.big_muff` | Dark-green rubber plate; 5 knobs (Drive, Fuzz, Tone, Blend, Level); linework = a fuzz-fade envelope + a "vibe" waveform trace. |

**Modelling**

| type | style concept |
|---|---|
| `amp.basic` | Warm brass amp faceplate: a glowing valve/tube silhouette, a 6-knob row (Voice, Gain, Bass, Mid, Treble, Level), a foot-switch trough; linework = a profile with a speaker cutout below. The brass `line-hi` is the lit accent. |
| `cab.sim` | Burlap/wood cabinet plate; 4 knobs (Low cut, Resonance, Presence, High cut); linework = a folded-speaker-cloth grille. |
| `mod.nam` | Brushed-steel plate; 3 knobs (Input gain, Output level, Mix); linework = a small model-chassis hatch that lights amber when a `.nam` model is loaded. |

**Time**

| type | style concept |
|---|---|
| `time.reverb` | Glass-fronted speaker grille (lossy plate); one knob Mix; linework = a Schroeder comb/allpass motif. |
| `time.shimmer` | Deep-blue silk; 7 knobs (Mix, Size, Decay, Shimmer, Interval, Tone, Pre-delay); linework = a shimmer sparkle glyph. |
| `time.delay` | Slate metal; 3 knobs (Time, Feedback, Mix); linework = a flat-echo "tap" motif. |

**Modulation**

| type | style concept |
|---|---|
| `mod.chorus` | Copper-gold; 4 knobs (Rate, Depth, Mix, Feedback); linework = a shimmer-flutter wave. |
| `mod.tremolo` | Gunmetal; 3 knobs (Rate, Depth, Mix); linework = a louvre/fan blade whose pitch changes with rate. |

**Pitch**

| type | style concept |
|---|---|
| `time.pitchdelay` | Brass plate; 6 controls (Time, Feedback, Pitch, Sweep rate, Mode, Mix); linework = a delay-tape-clock face where the Sweep-rate marker moves with rate. The footswitch (engage/disengage) sits in the plate's trough. |
| `pitch.octaver` | Cyan metal; 3 faders (Dry, Oct Down, Oct Up); linework = a sine-up/down trace. |

**Utility**

| type | style concept |
|---|---|
| `util.gain` | Matched silver knob, single knob Gain (dB); linework = a simple gain triangle. |
| `util.mix` | Matte-black; 2 weighted knobs A/B + a ratio crossfade line. |
| `util.master` | Brass; 2 knobs (Volume, Ceiling); linework = a limiter threshold flag. |

**Notes on the inventory**

- The count is **26** in the current `builtin_registry()`. Every type above is a
  real `BlockSpec` in the engine (`id`, `name`, `category`, `params` with real
  `unit`, `min`, `max`, `default`). Categories match the `Category` enum in
  `graph/src/types.rs` and `ui/src/protocol.ts`.
- `time.pitchdelay` (category **Pitch**) is the "whammy block" referenced by
  the ui-survey: its `mode` param (a `choice`) and `rate` param (a `number`)
  are the only params that may bind special controls via the editor's render
  prop.
- The `live` crate in the repo is a *separate engine feature* and does not add
  built-in registry entries to dsp-graph; the UI never sees them here.

---

## 6. Follow-up implementation cards

Small, precisely specified cards for the builders. Each is roughly 1–4 files.
The first four map onto **already-queued builder cards**; the remaining four are
new cards to be created.

1. **`theme: blockTheme registry + fallback`** → **t_1ebbda5e** (pedal-builder)
   - *Scope:* `ui/src/themes.ts` — the `BlockTheme`/`BlockPalette`/`BlockAnchor`
     types, `blockTheme(type, category)` lookup (exact → category → fallback),
     and the unknown-type fallback derived from the category hue.
   - *Acceptance:* registry covers all 26 types with no `undefined` hits;
     a plugin/third-party type resolves to a readable category-derived theme;
     `themes.ts` type-checks under `pnpm run build` (tsc --noEmit).

2. **`theme: BlockEditorFrame component`** → **t_eeff4892** (pedal-ui-builder)
   - *Scope:* `ui/src/components/BlockEditorFrame.tsx` — props above, the
     `.block-identity/.block-artwork/.block-controls/.block-postartwork`
     layout contract, `--block-state` bypass propagation, render-prop binding,
     `data-param` slots.
   - *Acceptance:* builds; unselected block renders the empty state; a selected
     block renders its theme's artwork + a knob per non-choice param; bypass
     toggles the artwork off-state.

3. **`theme: SVG/CSS infrastructure`** → **t_b6f103d2** (pedal-builder)
   - *Scope:* `ui/src/components/BlockArtwork.tsx` + the inlining helper
     (`ui/src/themes-inline.ts`, `ui/src/assets/artwork/`); wire
     `--block-h`/`--block-material`/`--block-state` onto `.block-frame`;
     `Knob` reads `var(--block-control)`/`var(--block-text)`.
   - *Acceptance:* all controls inherit the theme (no hard-coded amber);
     artwork is `aria-hidden`/`focusable=false`; SVG is a DOM element so
     colours cascade; `pnpm run build` green.

4. **`theme: per-type themes`** → **t_e25e818e** (pedal-builder)
   - *Scope:* theme entries for all 26 types grouped by category
     (I/O, Split/Merge, Dynamics, Tone, Distortion, Modulation, Pitch, Time,
     Modelling, Utility), one data-URL artwork each (amp faceplate, cab
     grille, etc.).
   - *Acceptance:* every builtin type renders its artwork; contrast ≥ 4.5:1
     text / ≥ 3:1 control chrome; forced-colors fallback present under #4.5.

5. **`theme: knob & control re-skin`** (new card — pedal-ui-builder)
   - *Scope:* rewire `Knob`/`ChoiceControl` to inherit `var(--block-*)`;
     add full accessibility to `Knob` (`role="slider"`, `aria-*`, arrow keys).
   - *Acceptance:* every block control is keyboard-operable; no control text
     is hard-coded to amber; `prefers-reduced-motion` drops knob sweeps; build
     green.

6. **`ui: migrate existing editors to BlockEditorFrame`** (new card — pedal-ui-builder)
   - *Scope:* refactor `ui/src/components/Editor.tsx` to wrap each block in
     `BlockEditorFrame` (choice/knob slots), keep `tone.graphic_eq`
     as a slot, remove the whammy orphaned controls.
   - *Acceptance:* Rainbow / Graphic EQ / Multiband editors still compile and
     edit; no hard-coded `isGraphicEq` branch; the whammy footswitch/
     expression bind through the frame, not as always-on orphans; `pnpm run
     build` green.

7. **`theme: artwork assets`** (new card — pedal-ui-builder)
   - *Scope:* per-type artwork SVGs (amp, cab, etc.) as data URLs in
     `themes.ts`; a design-time inliner for authoring
     (`scripts/inline-artwork.mjs`).
   - *Acceptance:* portrait (phone) and landscape (desk) aspect ratios render
     without clipping; each artwork is decorative (`aria-hidden`) and
     non-focusable; the amp faceplate's knobs land on the 0–100 plane.

8. **`a11y: contrast & focus audit`** (new card — pedal-builder)
   - *Scope:* measure contrast of all 26 artworks' labels/controls via a small
     script that extracts the palette from `themes.ts` and computes ratios;
     fix any ratio < 4.5:1; re-verify focus rings at 800 px and 1440 px.
   - *Acceptance:* every theme passes a 4.5:1 text / 3:1 control audit;
     `:focus-visible` ring visible on all frames; forced-colors simulation
     keeps every control visible.

---

## Try it

Run the app per `docs/RUN-THE-APP.md`. Open the editor, select any block type
from the rack/picker, and you should see the block's artwork with its knobs
positioned on the 0–100 plane inside the artwork (e.g. the amp's six knobs
sit on the faceplate), never floating beside it. Choose a bypassed block to
see the artwork switch to its `off` state.

## Not done / known limits

- Plugin / third-party block authoring is out of scope: those resolve to the
  unknown-type fallback theme and are documented in §1.1.
- The `time.pitchdelay` whammy's `sweep_gate` footswitch/expression slots are
  bound through the migration recipe (card #6); until that card lands the
  controls appear as the current orphaned orphans.
- A global theme toggle (dark/light) is deliberately **not** in this doc; the
  app has one dark theme and the doc pins the dark tokens only.
