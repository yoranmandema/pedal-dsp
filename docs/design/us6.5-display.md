# US-6.5 Display — readout and readability spec

**Status:** design (architecture/pedal-architect)
**Goal** (user): A display showing the current preset and chain, **readable in
a dark venue**.
**Acceptance:** readable in a dark venue; current preset + chain displayed.

> Owner: pedal-builder (display firmware + readout); the "readable in a dark
> venue" spec is owned by pedal-architect. One session, no new slice.

---

## 1. What the display shows

The display is a small, always-on readout on the front panel of the pedal. It
shows two things:

| Element | Content | Example |
|---|---|---|
| Preset name | Current preset filename (no extension) | `04-rainbow` |
| Chain | The signal chain of the active channel, as `In → BlockType → … → Out` | `In → Drive: Fuzz → Delay → Out` |

- The preset name is the primary, first line — it is the single most important
  piece of information (the player wants to know "what am I playing?").
- The chain is a compact readout of the first channel's blocks (up to ~6 blocks);
  longer chains are truncated with `…`. Back-action: if the active channel has
  no blocks, the chain area shows `—` (empty).
- The readout is **read-only**: the player cannot edit the preset or chain from
  it. Preset/scene selection stays in the UI (and, on the Pi 5, a touchscreen
  fullscreen app). The display is a glance readout, not the control surface.

### Chain block naming
Blocks are shown as `Type: Name` where:
- `Type` is the block's category or simple name (e.g., `Drive`, `Delay`,
  `Tone`, `Reverb`, `Amp`, `Cab`, `Nam`, `Gain`, `Mix`).
- `Name` is the block's param name when a block has one prominent param
  (e.g., `Delay: Time`), or the block name otherwise (e.g., `Amp`, `Cab IR`).

The chain readout is derived from the engine's state snapshot (`preset` +
`channels[0].blocks`), which the display firmware receives over IPC. See
§6 for the messages.

---

## 2. Display hardware

The Pi 5 pedal (the settled hardware target) runs the same app as the Mac, but
with a **borderless fullscreen window on the touchscreen**. The "display" in
this card is the **front-panel readout** (a separate, always-on element), not
the touchscreen UI. The readout is implemented as a dedicated graphics path so
it can stay on while the UI is off / in a low-power state.

```text
         +---------------------+
         |  Display readout    |   <- always on, even when UI is off
         |  "04-rainbow"       |
         |  In → Drive: Fuzz   |
         |  Delay → Out        |
         +---------------------+
```

### Recommended hardware
- **Display type:** monochrome OLED (self-emissive — readable at any light
  level, no backlight to drain power). If an LCD is preferred, it must have an
  adjustable backlight with a ceiling that prevents burn-in/low-peak issues.
- **Resolution:** 128×32 or 128×64 (common small OLED/SMDP modules; the
  128×64 gives more room for the chain readout).
- **Interface:** I2C (simplest, fewest pins) with a level shifter for the Pi
  5's 3V3 logic. SPI is an alternative if the display has no I2C.
- **Backlight (LCD only):** PWM-controlled, 0–100 %, adjustable from the UI
  (top-bar brightness knob) and a hardware knob on the enclosure.
- **Driver:** the display driver firmware runs on the Pi 5 (the same SoC that
  runs the app), not a separate MCU — the Pi is the "controller" for this
  card, and the readout is a graphics overlay on the Pi's framebuffer or a
  small dedicated display controller (e.g., SSD1306/I2C OLED for a dedicated
  readout).

### Power
- The readout draws from the Pi 5's 3V3 rail. A software "adapter" (blank
  display) turns the readout off when the pedal is idle (no audio signal) to
  save power and avoid a bright display in a dark venue when not in use.
- The readout is **always on** while the pedal is in use (audio running) so
  the display is immediately readable.

### Viewpoint / enclosure
- Mounted on the top or front panel of the enclosure, at roughly eye level
  when the pedal is on a pedalboard or floor. Viewing angle ≥ 160° (wide
  viewing angle OLED).
- The enclosure is designed so the readout is visible from the player's usual
  position without head tilting.

---

## 3. Readable in a dark venue — specification

This is the acceptance for the card. All values are measured at the display's
typical reading position (main viewing distance, 300–450 mm).

### 3.1 Brightness and contrast
- **Peak luminance (with backlight at 100%):** ≥ 500 cd/m² for LCD (or the
  OLED's typical 600–1000 cd/m²). This is the key figure for a dark venue —
  a dark room is typically 0.1–1 cd/m², so the display must be ~100–1000×
  brighter.
- **Contrast ratio:** ≥ 1000:1 (typ 10000:1 for OLED).
- **Backlight adjustment:** 0–100 % in ≥ 8 steps (hardware knob + UI slider).
  The UI slider defaults to 70 %.

### 3.2 Text legibility
- **Minimum text size:** 2.5 mm (~7 px at 128×32) for the preset name;
  chain labels ≥ 1.8 mm.
- **Font:** fixed-width or simple sans-serif with clear glyph separation
  (avoid ambiguous characters like `0`/`O`/`o`; `1`/`l`/`I`).
- **Colour contrast:** pure black text on a near-black (inactive) display vs.
  white/amber text on a bright (active) display. Nadir luminance ≤ 1 cd/m²
  (so the "off" text is essentially invisible when not showing a preset).
- **Active-state colour:** amber or white on the dark background — high
  contrast and colour-temperature neutral (avoids the cool-white "glare" of
  blue/white LEDs in a dark room).

### 3.3 Viewing angle
- Viewing angle ≥ 160° (H×V). OLEDs natively meet this; LCDs need a
  wide-viewing-angle film.

### 3.4 Response time
- Display response time ≤ 1 ms (rise + fall). OLED: < 1 ms; LCD: need a
  multiple-ms response — **prefer OLED** for this card.

### 3.5 Power and state
- **Active:** display on while audio is running (any preset loaded, engine
  running). Measured illuminance at the main viewing position ≥ 300 cd/m² on
  the preset name.
- **Idle:** blank (adapter) after ~60 s of no audio signal, to save power.
  The blanking is not triggered by the UI's screen-off; it is driven by the
  engine's audio activity (a `state`/`meters` heartbeat or a dedicated
  `adapter` command).
- **Blinking:** the preset name does not blink when idle; the chain area is
  blanked.

### 3.6 Test
`A1 (dark-venue readability)` — run the engine headless with the readout
live, with the room at 1 cd/m², and read the preset name at 450 mm in the
dark. The preset name must be legible without a flashlight. Also record the
LED driver's measured lux at the readout surface (target ≥ 300 lx at
surface).

---

## 4. Readout data (what the firmware reads)

The display firmware reads from the engine's state. The engine pushes
`event: state` (~20 Hz) across the WebSocket bridge — the same snapshot the
UI uses. No new engine state fields are required for this card; the preset and
channel data already exist in `State` (see `docs/IPC-PROTOCOL.md` §State shape).

```
state.preset            → "04-rainbow"        (current preset name)
state.channels[0].blocks → [{id, type, name, bypass, params}, …]
```

The display firmware maps this to the readout:
- Line 1: `state.preset` (blank if `""`).
- Line 2-4: the chain of `channels[0].blocks` as `In → BlockType[:Name] → … → Out`.

If the firmware cannot parse the state (e.g., it does not run the engine),
the UI sends a dedicated readout command: a new `getReadout` (server) →
`event: readout` (client) pair, with payload `{preset, chain:[…]}`. This is a
fallback for the Pi 5 path and is out of scope for this design session.

---

## 5. Persistence and presets

The display mirrors the engine's current state. On a preset load, the display
updates automatically (the engine's `event: state` broadcast). No persistent
display settings are stored in the preset file — the display has no user data.

---

## 6. IPC for the readout

The display firmware is a separate process on the Pi 5 that rides on the same
loopback WebSocket as the UI. It receives:

| Direction | Message | Payload / effect |
|---|---|---|
| UI → engine (or engine → firmware) | `event: state` | full state snapshot; the firmware extracts `preset` + `channels[0].blocks` and renders the readout |
| UI → engine | `req getReadout` | → `res { ok, payload: {preset, chain:[{type,name}] }}` (optional: used for the Pi 5's standalone display controller) |

The readout is a **rendering path**, not an IPC mutation: the engine does not
store any display state. The UI's `event: state` stream is the single source
of truth, so the display and UI can never diverge.

> Note: the display firmware is a Pi-5-specific process. The design here is
> the contract it satisfies; the implementation is a later slice owned by
> pedal-builder.

---

## 7. Acceptance criteria (verify before done)

| # | Check | How |
|---|---|---|
| A1 | Readable in a dark venue | Room at 1 cd/m², engine headless with readout live; preset name legible at 450 mm without a flashlight |
| A2 | Current preset displayed | Load a preset; the readout's line 1 matches the preset name |
| A3 | Chain displayed | The chain readout matches the active channel's blocks |
| A4 | Brightness adjustable for dark-venue reading | Backlight 0–100 % (UI slider + knob); ≥ 300 cd/m² on the preset name |
| A5 | Contrast and text legibility | Black-on-white check; fixed-width sans-serif; no ambiguous glyphs |
| A6 | Wide viewing angle | ≥ 160° (H×V); readable from the side |
| A7 | Fast response | Preset change updates the readout within ~100 ms |
| A8 | Power saving | Adapter blanks the display after ≥ 60 s of no audio; restores immediately on audio activity |

---

## 8. Files to be opened by the implementer (display firmware + readout)

- Engine: new server command `getReadout` (optional; fallback path).
- Pi 5 app: new process `pedal-display` (or a module inside the Pi app) that
  - listens to `event: state`, extracts `preset` + `channels[0].blocks`;
  - renders the readout (fixed layout: preset name line 1, chain lines 2-4);
  - drives the display hardware (I2C/OLED or LCD) at ~20 Hz;
  - implements the adapter (blank after 60 s idle) driven by audio activity.

This is a spec-only card for this session: no code changes, so `cargo
test --workspace` and `pnpm run build` stay green. The implementation is the
responsibility of pedal-builder (display firmware + readout), using this spec
as the contract.
