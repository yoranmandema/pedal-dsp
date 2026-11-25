# Touch-screen readiness audit — 800×480 (US-2.6)

**Card:** `t_320ec699` (state of `ui/`)
**Viewport audited:** 800 × 480 portrait (a 7" touch display; width 800, height 480).
**Rule of thumb:** the audit target is a **≥44×44 px hit area** (WCAG 2.5.8) and **no interaction that needs a pointer device that touch does not give you**.
**Assumption:** US-6.5 Display design is already on `main` and is out of scope.

---

## 1. Where 800×480 lands in the current layout

The app is a three-row CSS grid (`.app`):

```
grid-rows=[auto · minmax(0,1fr) · auto]
row 1  TopBar (auto, min-h 58px)
row 2  main    (1fr) → rack (auto) + chain (1fr)
row 3  Editor (auto)
```

Breakpoints (from `tailwind.css`, width-based, mobile-first):

| Band | CSS                     | Applies at 800×480? |
|------|-------------------------|---------------------|
| phone | `< 700px` (`phone:`)   | no (800 ≥ 700)      |
| tab   | `700–840px` (`tab:`)    | **yes**             |
| desk  | `≥ 840px` (`desk:`)     | no                  |
| compact | `< 820px` (`compact:`) | **yes** (820 > 800) |

So at 800×480 the app is **in "tab" but in "compact"** (tight 800px width and very short 480px height). Several `phone:` caps therefore **do not apply**, which is one of the biggest problems below.

Derived values at 800px (from `tailwind.css` tokens):

- identity column: `--ui-id-w = clamp(144px, 16vw, 200px) = 128px`
- main panel width: `800 − 128 = ~672px`
- knob face: `--ui-knob = clamp(54px, 4.2vw, 62px) = 54px`
- editor controls floor: `--ui-ctrl = 72px` per cell; the densest block (5 knobs + a 2-cell choice = 7 cells) just about fits 7 cells across 672px with gutters — no wrapping slack at 800px.

Vertical budget (480px) is the real constraint:

- topbar: wrapped compact content can be 3–4 lines (≈140–170px)
- editor: `max-h-[62vh]` ≈ 297px, but only the space left after topbar + main
- chain: the `1fr` row becomes small whenever the rack is tall

---

## 2. Recommended layout approach

### 2.1 Tokens to define

| Variable        | Value             | Purpose                                         |
|-----------------|-------------------|-------------------------------------------------|
| `--ui-touch`    | `44px` (keep)     | minimum hit target for every interactive thing  |
| `--ui-target`   | `44px` (new alias)| easier to grep for                                      |
| `--ui-icon-btn` | `44px` (new alias)| smallToggle buttons; scale up from 24px        |
| `--ui-btn-pad`  | `4px`             | vertical pad that takes a control to 44px       |

Keep the existing 4px spacing grid — it is the reason layout is consistent at every width. The only change is that **every interactive element** gets at least 44px in both axes, and the few 24px/26px/32px/36px targets become 44px.

### 2.2 Breakpoints

The current breakpoint system is **width-only** and the phone caps are <700px. A 800×480 portrait screen behaves like a phone in that the width is 800 but the **height** is the binding constraint. Add one custom variant so the phone-equivalent caps can be applied at 800:

```css
@custom-variant portrait (@media (width <= 900px) and (height <= 540px));
```

At 800×480 that flag is true. Then add `(portrait:)` equivalents of every phone cap:

- rack: `portrait:max-h-[220px] portrait:overflow-auto phone`-equivalent (see §3.2)
- topbar: `portrait:overflow-auto` so it never eats the main row
- editor/main: the 3-row constraint is retained in portrait mode

Keep the existing `phone`/`tab`/`desk` bands for landscape and desktop.

### 2.3 Touch-target size variable

Define `--ui-touch: 44px` (already present) as the floor **on every interactive element**: buttons, icon buttons, sliders, segments, selects, slots, handles. Where a 24/26/32px control already exists, multiply the pad so the total stays 44px (e.g. `size-8` → `size-[var(--ui-touch)]`; `size-6` → `size-[var(--ui-touch)]`; `size-6.5` → `size-[var(--ui-touch)]`).

### 2.4 Layout rules for a short, wide view

1. The top bar must **not** force a third row under the main panel: cap it and let it scroll if needed.
2. The chain must **always** remain reachable: cap the rack height in portrait so the chain's `1fr` row has room.
3. Tabs (the chain scrollers) remain the way to reach more blocks horizontally; but vertical space must not be taken by the rack.
4. Every control that is a 44px target must look like a control (clear tap target, visible active state) — touch users cannot see a hover.

---

## 3. Checklist by component/file

### 3.1 `ui/src/components/TopBar.tsx` — top bar

| # | Issue | Severity | Proposed fix |
|---|-------|----------|--------------|
| 1 | Two `HeaderKnob`s (input trim, master) have **34px faces** (`size-[34px]`) and **no visible value readout at 800px** (`compact:hidden` hides the label + value). A touch user cannot read or position these. | High | Expose the label+value in portrait (remove `compact:hidden` under `portrait:`), and wrap the knob face with `var(--ui-knob)`; add ±(tap on the value) step fallback so a tap is usable, not only a drag. |
| 2 | Preset **step buttons** ‹ › and the **presets-open ☰** are `size-6.5` = **26px**. | High | `size-[var(--ui-touch)]`; add a text label or icon + label so they are obvious on touch. |
| 3 | Preset **slots** are `size-6.5` = **26px** divs with `onClick`. | High | `size-[var(--ui-touch)]`; use a `<button>`/`role=button`; show the active (amber) state for the selected slot. |
| 4 | **Source-toggle chips** (`riff`/`live`/`silence`) are `px-2.5 py-1.25 text-[10px]` ≈ **17px tall**. | High | `min-h-[var(--ui-touch)]` (+ `py` to reach it); bump `text` to at least 11px. |
| 5 | Right column is **crowded**: tuner, source chips, meters, CPU, latency, buffer select, master knob. In `compact` it wraps to 3–4 rows and the topbar grows, crowding the chain. | Medium | Cap the topbar (`portrait:overflow-auto` / `max-h`), and move the meters/latency/CPU into a collapsible mini-panel (`topbar` settings drawer) so the bar stays one line. |
| 6 | Buffer-size `<select>` already meets 44px — OK. Tuner button has `min-h[--ui-touch]` but its **10px** "T" label is small — tighten to ≥11px. | Low | Bump tuner label to 11px; otherwise OK. |

### 3.2 `ui/src/components/Rack.tsx` + `ui/src/components/BlockList.tsx` — block browser

| # | Issue | Severity | Proposed fix |
|---|-------|----------|--------------|
| 7 | The rack has **no height cap in "tab" mode** (`portrait:max-h` does not exist) yet holds ~8 categories / ~20 rows. In portrait the tall rack steals vertical space, the chain's `1fr` row collapses and the chain becomes unreachable. | High | Add a `portrait` cap mirroring `phone:max-h-[200px] phone:overflow-auto` (e.g. `portrait:max-h-[220px] portrait:overflow-auto`), and keep the rack scrollable. |
| 8 | Rack block rows are `py-1.75` + `text-[12px]` ≈ **40px tall** and the row height grows with content — below the 44px floor on the short axis. | Medium | Raise row padding/pad to reach `--ui-touch` (≈ `py-2.5`), or set an explicit `min-h[--ui-touch]` on the row. |
| 9 | The rack rows are `draggable` + a **long-press** drag (already touch-implemented), plus `title` tooltips. Long-press is the correct touch path. | Low | Keep; only ensure the row itself is a 44px target (see #8) and the `title` is also shown as live text under the block name. |

### 3.3 `ui/src/components/Chain.tsx` — channel strip + block cards

| # | Issue | Severity | Proposed fix |
|---|-------|----------|--------------|
| 10 | The **bypass / remove** buttons in each card are `size-6` = **24px**, `opacity-0` by default, shown only on `hover`. On touch `hover` never fires, so the whole `.card-actions` area is `pointer-events-none` and these actions are **invisible and unreachable**. | High | Make bypass/remove **always visible** at 44px (remove the `opacity-0` / `hover:` tricks), or gate them behind a **long-press on the card** with a temporary reveal. |
| 11 | The same problem on the card itself: the card is a button but a 24px × 24px **output handle** (`.block-out`, 12px × 12px amber dot) is the **only drag source for cross-channel routing**. It is far too small to tap/hold on touch. | High | Make the handle a long-press target with a visible highlight, or replace it with a long-press on the card edge; keep the 12px dot for desktop only. |
| 12 | **Channel strip** header: `channel-chip` is `w-6 h-6` (**24px**) and the **source** button is `w-6 h-6` (**24px**). | Medium | Bump both to `--ui-touch` (or make chip a plain label; if clickable, a real button). |
| 13 | **Channel volume** `<input type=range>` is `w-24` = 96px, but the default webview thumb stays **~15px**. | Medium | Style `::-webkit-slider-thumb` / `::-moz-range-thumb` to **~44px** (e.g. width/height 28–32px + padding), and consider tapping-the-track-as-click. |
| 14 | **Add-block `+` button** is `size-8` = **32px**. | High | `size-[var(--ui-touch)]` with an amber `+` and `aria-label`. |
| 15 | Card itself is `w-[116px] h-[66px]`, strip `min-h-[102px]` — these are fine as wide/tall targets. The card's `title` tooltip is the only "hover" hint; the card is a real button with keyboard support. | Low | Keep; the card drag is already pointer-based. |

### 3.4 `ui/src/components/Editor.tsx` + `ui/src/components/Knob.tsx` + `HeaderKnob.tsx`

| # | Issue | Severity | Proposed fix |
|---|-------|----------|--------------|
| 16 | **Editor knob face** `var(--ui-knob) = clamp(54px, 4.2vw, 62px)` = **54px at 800px** — acceptable but the **value readout** is `min-w-[56px]` and the only way to change it is a **precise vertical drag** on a 54px face. | Medium | Add **tap-to-step** (+/-) next to knobs, and a **long-press** for coarse step; keep the drag. |
| 17 | **HeaderKnob** (top bar): 34px face, `compact:hidden` hides the label and value at 800px, `title` tooltip shown only on hover. | High | See #1: show label+value at 800px; add tap-to-step. |
| 18 | **Choice segmented buttons** are `min-h[--ui-touch] min-w[--ui-touch]` = 44px — OK. But `text-[10px]` on the segment label is small. | Low | Bump to 11px. |
| 19 | **Expression slider**: range thumb is **20px × 20px**; the Heel/Toe labels are `text-[9px]`. | Medium | Thumb to 44px; bump labels to 10–11px; allow **tap-on-track** (the track is full width). |
| 20 | **Footswitch button** `min-h[--ui-touch] min-w[--ui-touch]` = 44px — OK. | Low | None. |
| 21 | **Bypass / Reset side buttons** `min-h[--ui-touch] min-w[--ui-touch]` = 44px — OK. | Low | None. |
| 22 | Editor panel: `max-h-[62vh] overflow-y-auto` makes the params scrollable — reachable in principle, but at 800 the **controls grid is exactly 7 columns with no slack**, so a block with 8+ params wraps and the second row is right at the bottom edge. | Medium | Give the controls grid `minmax(var(--ui-ctrl), 1fr)` a `gap` that still fits 8 columns at 800, or allow the editor row to grow (the main row is `1fr`, so the editor can steal height from it). |

### 3.5 `ui/src/components/PresetBrowser.tsx`

| # | Issue | Severity | Proposed fix |
|---|-------|----------|--------------|
| 23 | **Preset-load** row `text-base (12px) px-2 py-1.75` ≈ **18px tall** — below 44px. | High | `min-h[--ui-touch]` and `py` to reach it; the row is already text-left so it stays easy to hit. |
| 24 | **Rename input** `[textField]` `px-2 py-1.75 text-md` ≈ **18px tall**. | High | Bump `py` to reach `--ui-touch`; the `textField` is currently 12px text. |
| 25 | **Close "×"** button `size-6.5` = **26px**. | High | `size-[var(--ui-touch)]`. |
| 26 | Modal shell `w-[460px]` — fits at 800 (max-w 752px). OK. | Low | None. |

### 3.6 `ui/src/components/TunerModal.tsx`

| # | Issue | Severity | Proposed fix |
|---|-------|----------|--------------|
| 27 | **Close "×"** button `size-6.5` = **26px**. | High | `size-[var(--ui-touch)]`. |
| 28 | Tuner note readout `text-64`, gauge `max-w-[320px]` — fine at 800. | Low | None. |

### 3.7 `ui/src/components/RoutingLines.tsx` — cross-channel routings

| # | Issue | Severity | Proposed fix |
|---|-------|----------|--------------|
| 29 | **Routing output handle** = 12px × 12px dot (see #11). | High | As #11. |
| 30 | **Context menu** opens only on **right-click**; no touch equivalent. | High | Add a **long-press** on a routing line to open the same menu, and/or a visible "⋮" toggle that stays in the floating bar. |
| 31 | Routing **centerline is 1.5px** and the click hit uses an invisible 12px stroke. At 800 the line is hard to see and the hit target is invisible. | Medium | Make the hit target a **visible faint pill** behind/around the line, and a slightly thicker (2–3px) centerline; the current invisible stroke works but cannot be seen to be there. |
| 32 | Floating action bar (**Delete / Enable / Gain**) buttons `py-1 text-sm` ≈ **18px tall**; the dB slider thumb ~15px. | Medium | `py` to reach `--ui-touch`; dB thumb to ~44px; gain slider should also keep a clear label. |
| 33 | The routing line has `.group-hover:opacity-90` (hover-only fade). | Low | Replace with a **selection** state (the bar + brighter line already give it away). Otherwise hide the class. |

### 3.8 Global

| # | Issue | Severity | Proposed fix |
|---|-------|----------|--------------|
| 34 | `body { overflow: hidden }` + wrapped compact topbar → at 800 the topbar can **clip its right-hand controls** instead of scrolling. | High | Cap the topbar (`portrait:overflow-auto` / `max-h`), see #5. |
| 35 | Rack has no portrait cap (see #7) → **chain pushed off-screen and unreachable**. | High | `portrait:max-h-[220px] portrait:overflow-auto` on the rack, as in the recommended layout. |
| 36 | The **notification toast** sits in the editor's grid cell (`row-start-3`) and is lifted by one `--space-2` above the bar. At 800 it is reachable; verified fine. | Low | None. |
| 37 | Small-but-real: the **tuner idle** ("Play a note") and the **blank-rack hint** use `text-base (12px)` placeholders — increase to 12–13px for readability. | Low | Bump size. |

---

## 4. Summary table (files, issues, fixes)

| File | Issue | Fix |
|------|-------|-----|
| `TopBar.tsx` | 34px knob faces, no readout | expose label+value; tap-to-step |
| `TopBar.tsx` | 26px nav, slots, presets ☰ | `size[--ui-touch]` |
| `TopBar.tsx` | source chips ~17px, crowding | 44px chips; cap topbar |
| `Rack.tsx` | no portrait cap → chain unreachable | `portrait:max-h + overflow-auto` |
| `BlockList.tsx` | 40px rows | reach `--ui-touch` |
| `Chain.tsx` | bypass/remove invisible on touch | always-visible 44px |
| `Chain.tsx` | 12px routing handle | long-press + highlight |
| `Chain.tsx` | 24px chip/source buttons | 44px |
| `Chain.tsx` | 32px + button | 44px |
| `Chain.tsx` | volume thumb | ~44px thumb |
| `Editor.tsx` | 54px knob, precise drag | tap/long-press step |
| `Knob.tsx` | 54px face at 800 | tap-to-step + readout |
| `HeaderKnob.tsx` | 34px face, hidden readout | show readout + tap-to-step |
| `Editor.tsx` | dense blocks wrap with no slack | allow editor to grow |
| `PresetBrowser.tsx` | 18px load/rename rows | 44px |
| `PresetBrowser.tsx` | 26px close × | 44px |
| `TunerModal.tsx` | 26px close × | 44px |
| `RoutingLines.tsx` | 12px dot, right-click menu only | long-press menu; visible hit pill |
| `RoutingLines.tsx` | 18px bar buttons, 15px slider thumb | 44px |
| global | body overflow hidden clips topbar | cap topbar |
| global | rack→chain crowding | portrait cap |

---

## 5. Scope of this card

This card **only writes documentation**. No UI source was changed. The changes it describes belong in `ui/src/**/*.tsx` / `ui/src/tailwind.css` as follow-up builder cards, and in the design tokens (the `portrait` custom variant). The card will be closed once `docs/touchscreen-audit.md` is committed on `main`.
