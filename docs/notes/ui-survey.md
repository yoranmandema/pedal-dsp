# ui/ frontend survey

Scope: Preact + TypeScript + Tailwind v4 frontend in `ui/`, rendered in the Tauri
native webview shell. No UI code was changed — this is a read-only survey for a
designer proposing a per-block theming mechanism.

Repo state of the art this survey reflects: the `t_6169bbaf` worktree (the
branch this card runs on). Note: the **main checkout** carries four extra
per-block editors (`FuzzEditor`, `LooperEditor`, `WaveShaperEditor`,
`WaveShaperGraph`) that are **absent from this worktree**, where only one block
editor exists. The survey describes the worktree.

Related docs: `docs/ARCHITECTURE.md`, `docs/UI-ARCHITECTURE.md`,
`docs/BLOCK-BROWSER-REUSE.md`, `docs/IPC-PROTOCOL.md`,
`docs/RUN-THE-APP.md`.

---

## 1. Where block editors live & how each is structured

**Only one block editor exists in this worktree: `ui/src/components/Editor.tsx`.**

- **File**: `ui/src/components/Editor.tsx` (256 lines).
- **Host**: rendered by `App()` (`ui/src/App.tsx`) into the `editor` grid row
  (`grid-rows-[auto_minmax(0,1fr)_auto]`). It is a `<footer>` element.
- **Data**: reads the full engine snapshot via `state.value` and the
  `selected` signal (`{ channel, block }`). The selected block is resolved from
  `s.channels`, then `s.registry[block.type]` gives its spec.
- **Structure** (per selected block):
  1. `editor-id` — identity column: `editor-name` (truncated block name),
     `editor-bypass` pill, `side-btn` Bypass + `side-btn` Reset.
  2. `editor-controls` — grid (`repeat(auto-fit,minmax(var(--ui-ctrl),1fr))`)
     holding the control widgets.
     - **Choice params** → `ChoiceControl` (see below).
     - **Number params** → `Knob` (see below).
     - **Whammy block extras** → `FootswitchControl` (engage/disengage) and
       `ExpressionControl` (heel/toe) rendered after the grid.
- **Networks/associations**: none — the editor is driven entirely by `selected`
  + `state`, no props drilling.

### Widget components

| Component | File | Props | Rendering |
|---|---|---|---|
| `Editor` | `Editor.tsx` | none (reads `state`) | footer panel: `editor-id` + `editor-controls` grid |
| `Knob` | `Knob.tsx` | `name, value, min, max, defaultValue?, unit?, format?, onChange` | rotary SVG knob: pointer drag, `dblclick` resets to `defaultValue` |
| `ChoiceControl` | `Editor.tsx` | `p: ParamSpec, value, onChange` | `role="radiogroup"` + `role="radio"` buttons (choice ≤ 5) **or** `<select class="select">` (choice > 5) |
| `ExpressionControl` | `Editor.tsx` | `value, onChange` | `<input type="range">` 0–100 with "Heel"/"Toe" labels |
| `FootswitchControl` | `Editor.tsx` | `active, onChange` | `role="switch"` button, `aria-checked` |

### How knobs/controls are rendered

- **Knob** (`Knob.tsx`):
  - A `<div class="knob">` with `style={{ "--angle": `${angle}deg` }}` driving an
    SVG arc (`<circle>` with `pathLength={100}`, `stroke-dasharray` set to
    `t*75 100`) and an indicator `<span>` rotated to the same angle.
  - End-stop labels sit at `right/left` via absolute positioning.
  - Unit is carried in the value readout (`knob-value`), not the end labels.
  - **No `role`, `tabIndex`, or `aria-*`** on the knob root — it communicates
    only via `title` and pointer gestures.
- **Choice** (`ChoiceControl`): segmented row of buttons with
  `role="radio"`/`aria-checked`; when `choices.length > 5` it falls back to a
  `<select>` with `<option>`s.
- **Expression** (`ExpressionControl`): `<input type="range" min={0} max={100}
  step={1}>` styled by `.expression-slider` in `tailwind.css`.
- **Footswitch**: `<button role="switch" aria-checked>`.

---

## 2. Current styling mechanisms

One stylesheet: **`ui/src/tailwind.css`** (9996 bytes). Tailwind v4, loaded by
the `@tailwindcss/vite` plugin (`vite.config.ts`).

**No separate config file.** The Tailwind setup lives entirely in the CSS
itself:

- **`@theme`** — design tokens:
  - *Breakpoints* (mobile-first, so unprefixed class = phone): `--breakpoint-tab: 700px`, `--breakpoint-desk: 840px`. No `--breakpoint-*` for <700px; `phone` is `@custom-variant`.
  - *Palette*: `--color-bg`, `--color-panel`, `--color-line`,
    `--color-line-soft`, `--color-line-hi`, `--color-text`, `--color-muted`,
    `--color-amber` + `--color-amber-dim/hi/lo/edge/ink`, `--color-ok`,
    `--color-inset`, `--color-well`, `--color-surface`,
    `--color-raised`, `--color-raised-2`, `--color-danger` + danger text/bg.
  - *Type scale*: `--text-pico` (6.5px) … `--text-2xl` (28px) plus `--text-base` (12px).
  - *Radius*: `--radius-xs` … `--radius-xl`.
  - *Shadows*: `--shadow-card`, `--shadow-knob`, `--shadow-glow`,
    `--shadow-drag`, `--shadow-toast`, `--shadow-modal`.
  - Fonts: `--font-sans`, `--font-mono`.
- **`@theme inline`** — maps `--color-cat`/`--color-cat-soft` to
  `var(--cat)`/`var(--cat-soft)`. These are derived per-element from the block
  category hue (`--cat-h`).
- **`@custom-variant`** — `phone` (`@media (width < 700px)`), `compact`
  (`< 820px`), `desk` (`>= 840px`).
- **`@layer base`** — `:root` sets the geometry `CSS variable` scale:
  `--ui-touch: 44px` (WCAG 2.5.8 touch target), `--ui-ctrl: 72px` (cell floor),
  `--ui-ctrl-max: 96px`, `--ui-ctrl-row: calc(7*var(--ui-ctrl-max)+6*var(--spacing)*6)`,
  `--ui-bar: 128px`, `--ui-topbar-h: 58px`,
  `--ui-id-w: clamp(144px,16vw,200px)`, `--ui-knob: clamp(54px,4.2vw,62px)`,
  `--amber`, `--space-2`, `--space-6`.
- **`@utility`** — multi-property recipes: `cat-scope` (derives
  `--cat`/`--cat-soft` from `--cat-h`), `bg-stage` (vignette),
  `amber-key` (lit anchor).
- **Element selectors** (not utilities): `.block-out` (routing output handle),
  `.block-route-preview`, `.block-card.routing-target`,
  `.expression-slider` + `input[type=range]` track/thumb.

**Theming / dark mode**: dark is the only theme. All palette tokens are dark
(`--color-bg #101116`, `--color-panel #17181d`). There is no light mode, no
theme toggle, and no `prefers-color-scheme` handling.

**Category accent mechanism** (the existing theming hook):
- `ui/src/theme.ts` — `HUES` record (one hue 0–360 per category) and
  `catStyle(category)` which returns inline `style={{ "--cat-h": String(hue) }}`.
- Elements that want the category accent apply the `cat-scope` class (or the
  `style` from `catStyle`). `@theme inline` then maps the semantic colors
  `--color-cat`/`--color-cat-soft` to the hue-derived values.
- `Editor.tsx` sets `style={spec ? catStyle(spec.category) : undefined}` on the
  `editor-id` column, which cascades to the block glyph, the selection ring, and
  the drag ghost.

**No component-level CSS files, no CSS modules.** All styling is: single CSS
file + Tailwind utility classes in TSX + a few element selectors + inline
`style` props (for the category hue and the per-block `--cat-h`).

---

## 3. Shared wrapper / layout components used by block editors

- **`ui/src/ui.ts`** — class-recipe constants shared across components:
  `navBtn`, `emptyNote`, `ctrlName`, `textField`, `modalBackdrop`,
  `modalShell`, `modalHead`, `modalTitle`, `modalFoot`. (Used by
  `PresetBrowser`, `AddBlockModal`, `TunerModal`, `Editor`.)
- **`ui/src/App.tsx`** — root layout: `app` grid
  `grid-rows-[auto_minmax(0,1fr)_auto]`; `TopBar` (row 1), `.main`
  (`grid-cols-1 grid-rows-[auto_minmax(0,1fr)]`, `tab:grid-cols-[240px_minmax(0,1fr)]`)
  containing `Rack` + `Chain`, then `Editor` (row 3), plus `TunerModal`,
  `PresetBrowser`, `AddBlockModal`, `SelectionRing`, and the `notice` toast.
- **`ui/src/components/Editor.tsx`** — the block editor footer panel itself
  (see §1).
- **`ui/src/components/BlockList.tsx`** — shared block catalogue renderer
  (grouped by category, one button per block). Reused by the always-visible
  rack and the add-block modal picker.
- **`ui/src/components/Rack.tsx`** — left rack wrapper (`<aside class="rack">`).
- **`ui/src/components/Chain.tsx`** — the chain panel: one `ChannelRow` per
  channel, `RoutingLines` overlay sibling.
- **`ui/src/components/RoutingLines.tsx`** — cross-channel routing overlay:
  measures card geometry live (`getBoundingClientRect`) and re-measures on
  scroll/resize/state change; SVG lines with an invisible wide stroke for
  hit-testing.
- **`ui/src/components/SelectionRing.tsx`** — keyboard focus + block
  selection ring drawn as a fixed overlay at the top level so it is never
  clipped by scrolling ancestors.
- **Modal shells**: `PresetBrowser`, `AddBlockPicker` (rendered inside
  `Editor` after an add-block request; the picker's veil is editor-scoped
  z-index 0, shell z-index 999), `TunerModal` — all use the `modalBackdrop` +
  `modalShell` recipes plus the `textField`/`navBtn` recipes.

---

## 4. Knob/slider/control components — accessibility state

| Component | ARIA / focus treatment | Keyboard | Verdict |
|---|---|---|---|
| `Knob` (editor) | none (`role`, `tabIndex`, `aria-label` absent); SVG arc `aria-hidden` | pointer drag + `dblclick` reset only | **Inaccessible as a control** |
| `HeaderKnob` (top bar) | `role="slider"`, `aria-label`, `aria-valuemin/max/now` | ArrowUp/Down/Left/Right, Home, End | Full |
| `ChoiceControl` | `role="radiogroup"` + `role="radio"` + `aria-checked` + `aria-label` + `title` | native button activation (Enter/Space) | Full |
| `ExpressionControl` | `aria-label` on `<input type="range">` | native range keys | Full |
| `FootswitchControl` | `role="switch"`, `aria-checked`, `aria-label` | native button | Full |
| `Chain.ChannelHead` | volume range `aria-label`; source toggle `aria-pressed` + `aria-label` | native | Full |
| `RoutingLines` | SVG lines carry `<title>` (accessible name); invisible wide stroke (`[pointer-events:stroke]`) for hit testing | click/contextmenu on line | Full |
| `TunerModal` | static readouts (note, cents needle, signal bar) | Esc/T toggle (app-level) | n/a |
| `PresetBrowser` | `textField`, buttons, `aria-hidden` on icons | native | Full |
| `BlockList` rows | `aria-pressed` (chosen), `draggable`, `title` | `onKeyDown` Enter/Space → `selectBlockType` (keyboard insert path) | Partial (drag is pointer only) |

**Summary**: the per-block `Knob` (the control used by every block in every
editor) is the weakest accessible control — no role, no `tabIndex`, no
keyboard support beyond pointer drag + double-click. Only the top-bar
`HeaderKnob` is a proper `role="slider"` with full keyboard support. Any
per-block theming that styles or restyles the `Knob` should be aware it is
currently not a keyboard-operable slider.

---

## 5. prefers-reduced-motion

**None.** A repo-wide grep for `prefers-reduced-motion`, `reduce-motion`, and
`motion` in `src/` and `docs/` returned **zero hits**. There is no
`@media (prefers-reduced-motion: reduce)` rule anywhere.

Motion is pervasive (many `transition-[...]` / `transition-colors` utilities,
`duration-*` transitions in `Chain`, `RoutingLines`, `SelectionRing`), but no
reduced-motion path exists. Any per-block theming that introduces animation
should opt into a reduced-motion media query if it adds non-essential motion.

---

## 6. Constraints & pitfalls for adding per-block theming

**Bundle setup**
- `ui/package.json`: Preact 10.27 + `@preact/signals` 2.0.3, Vite 7, Tailwind
  v4 (`@tailwindcss/vite`), `@preact/preset-vite`.
- `vite.config.ts`: `base: "./"` (assets load from the embedded Tauri protocol
  or a `file://` open); `/engine` proxy → engine WebSocket; `__ENGINE_URL__`
  defined at build time; `VITE_ENGINE_URL` env var overrides.
- Dev/build: `cd ui && pnpm run build` → `tsc --noEmit && vite build` (strict
  TS: noUnusedLocals/Parameters, noFallthroughCasesInSwitch). `ui/dist/` and
  `ui/node_modules/` are generated and ignored — never commit them.
- The UI never touches the audio path: it is a WebSocket client to the engine
  on `ws://127.0.0.1:8737` (loopback only).

**CSS / theming plumbing**
- Single stylesheet `ui/src/tailwind.css`, Tailwind v4. Test: a new theme
  axis must land in `tailwind.css` as `@theme` tokens / `@utility` / classes,
  or via inline `style` props (there are no component CSS files).
- Per-category theming already exists via `ui/src/theme.ts` (`HUES` + `catStyle`
  → `--cat-h`) + `@theme inline` (`--color-cat`/`--color-cat-soft`).
- Adding **per-block** (not just per-category) theming needs a per-instance CSS
  variable on the element — the only precedent is `SelectionRing`, which reads
  `--cat-h` from a card's inline `style`. The `style` prop flows everywhere in
  this codebase, so an inline per-block var (e.g. `--cat-h` or a new
  `--block-h`) is the natural hook.
- A designer-proposed theme mechanism should extend `theme.ts` + `tailwind.css`
  tokens — consistent with the existing category-hue pattern and minimal new
  plumbing.

**SVG import support**
- No external SVG assets anywhere. All icons are inline JSX (`Icon.tsx`,
  `BlockList.tsx`, `RoutingLines.tsx`, knob arcs in `Knob.tsx`/`HeaderKnob.tsx`).
  `fill="none"`, `currentColor`, `stroke-width`, round caps — so glyphs inherit
  the category hue from the parent. No `<img>` or asset pipeline; a theme that
  changes hue works by CSS `currentColor`/CSS variables, not by swapping files.

**Build-tooling gotchas**
- `tsc --noEmit` is part of the build; strict options enforced (`noUnusedLocals`,
  `noUnusedParameters`, `noFallthroughCasesInSwitch`, `isolatedModules`,
  `verbatimModuleSyntax`). New theme utilities must type-check.
- `class` (not `className`) is used throughout the TSX — Preact 10's
  `react-jsx` transform accepts `class`. Do not introduce `className`.
- The editor shell is a `<footer>` in an app grid; modal pickers render inside
  `Editor`'s `position: relative` root, so z-index scoping (veil vs. floating
  shell) is load-bearing — a theme that adds overlays must preserve the
  `z-40` / `z-999` + `pointer-events` layering.
- The chain panel is `overflow-auto` and strips are `overflow-x-auto`; custom
  theme surfaces (e.g. block cards) must not widen the grid track without
  `min-w-0` — existing code comments in `Chain.tsx` flag this as load-bearing.

**What is NOT in the worktree (observation)**
- `ui/src/components/` in this worktree has **12** files: AddBlockModal,
  BlockList, Chain, Editor, HeaderKnob, Icon, Knob, PresetBrowser, Rack,
  RoutingLines, TopBar, TunerModal. The four per-block editors
  (`FuzzEditor`, `LooperEditor`, `WaveShaperEditor`, `WaveShaperGraph`) exist
  only in the main checkout. The only block editor here is `Editor.tsx`, which
  auto-renders controls from the registry spec (`choice` → `ChoiceControl`,
  `number` → `Knob`), so **new blocks inherit their controls without touching
  the editor** — the main exception being the whammy block, which needs the
  local `sweep_gate` state + `ExpressionControl`/`FootswitchControl` wiring.
