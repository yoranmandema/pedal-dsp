# End-to-end testing the UI

The UI has a Cypress E2E suite that drives the real app in a browser: the same
Preact components, the same `WebSocketBridge`, the same IPC JSON. The only thing
that is not the shipping product is the process on the other end of the socket,
which is a fixture. No audio device is opened and no sound is produced.

```
Cypress  ──HTTP──>  Vite dev server (pnpm dev:e2e, port 5273)
   │                     │
   │                     └── serves the app, which connects to
   │                          ws://127.0.0.1:9400
   └──────────────>  fake engine (started by cypress.config.js)
```

## Run it

```bash
cd ui
pnpm test:e2e          # start Vite + fake engine, run every spec headlessly
pnpm cypress:open      # interactive runner against a dev server you start yourself
pnpm dev:e2e           # just the dev server the suite expects (port 5273)
```

`pnpm test:e2e` is the single command: it waits for Vite to answer, runs Cypress,
then kills the dev server. It uses port **5273** and engine port **9400** so it
never fights the app's own dev server on 5173 or an engine on 8737.

Run it from a clean checkout with `pnpm install` first. Cypress downloads its
binary on install; `ui/pnpm-workspace.yaml` allows Cypress's postinstall.

## What is covered

| Spec | Flows / components |
|---|---|
| `boot.cy.js` | connecting to the engine, the rig rendering, meters, the engine-unavailable screen |
| `tuner.cy.js` | the tuner top-bar button and modal: open/close, note readout, cents needle, verdict, frequency line, signal bar, idle state, keyboard and viewport geometry |
| `presets.cy.js` | preset name/source display, next/previous, slot loading, source toggle, preset browser list/load/save/rename/delete, refused-delete error |
| `rack.cy.js` | catalogue grouping and ordering, click-to-add |
| `add-block.cy.js` | picker open/autofocus, name and type filtering, Enter-to-add, click-to-add, empty state, Escape/backdrop/× close |
| `chain.cy.js` | four channels, empty-channel hint, select, bypass, remove, refused-remove toast, channel volume, input source, pointer drag-to-reorder, HTML5 drag from the rack |
| `dnd-edge-cases.cy.js` | refused drops (no payload, non-block payload, unknown type) with a console warning, `dragend` cleanup after a refused/cancelled drag, a drop outside every channel, the touch long-press fallback, the keyboard insert |
| `editor.cy.js` | identity header, segmented choice, knob drag, knob double-click reset, reset-all, editor bypass, `<select>` for >5 choices |
| `keyboard.cy.js` | Enter/Space selection, selection ring |

## How it stays deterministic

- **Seeded state.** `cypress/support/seed.mjs` builds the rig. The fake engine
  rebuilds it from that seed on *every* WebSocket connection, and Cypress loads
  the page afresh in every test, so each test starts from the same rig.
- **No arbitrary waits.** Assertions retry until the UI catches up. The only
  fixed delay in the suite is the app's own 5 s "engine unreachable" timeout,
  which the boot-error test waits out with a 15 s assertion timeout.
- **Stable selectors.** Specs use the app's semantic classes, roles and titles
  (`block-card`, `rack-item`, `preset-load`, `picker-search`, `side-btn`,
  `input[type=range]`, `[title="Next preset"]`, …). They do not depend on
  markup order or on generated ids.
- **No real engine, no sound.** The fake engine speaks `docs/IPC-PROTOCOL.md`:
  it greets with `event: state`, answers `req` with `res`, and broadcasts
  `event: state` after a mutation. It implements the commands the UI uses and
  nothing else.

### Fake-engine test hooks

Two refusals are baked into the fixture so the UI's error paths are reachable:

- `removeBlock` for the seeded block id `locked` always fails → the global
  toast (channel remove).
- `deletePreset` for `02-lead` always fails → the preset browser's refused
  action.

`util.voices` is a fixture-only block with a six-option choice parameter; the
real registry has no choice with more than five options, and the editor renders
those as a `<select>`. It exists to exercise that branch.

## Adding a test

1. Add `cypress/e2e/<name>.cy.js`. `cypress.config.js` picks up
   `cypress/e2e/**/*.cy.js` automatically.
2. Start with `cy.boot()` — it visits `/`, waits for the top bar, and accepts
   `window.confirm` so preset deletion is deterministic.
3. Prefer `cy.get`/`:contains`/titles over nth-child. If a new component needs
   an ambiguous hook, give it a class or `title` and use that; do not reach
   into the DOM structure.
4. If the test needs the engine to behave differently, extend
   `cypress/support/fake-engine.mjs` (or the seed), not the spec.

Useful commands:

```js
cy.boot();                       // fresh page, seeded rig
cy.openPresetBrowser();          // open the preset modal
cy.selectBlock(0, 0);            // click a card, wait for the editor
```

## CI

This repository has no CI configured (no `.github/workflows`, no git remote), so
the suite is not wired into a pull-request pipeline. To run it in CI:

```bash
cd ui && pnpm install && pnpm test:e2e
```

`pnpm test:e2e` exits non-zero when any spec fails, so it fails the build as-is.
It needs a browser: it uses Cypress's bundled Electron by default, and
`--browser chrome` works where Chrome is installed.

## Known app bug the suite found (not fixed here)

A bypassed block card is rendered collapsed (about 18px wide, pinned to the top
left of the strip). `Chain.tsx` puts the class `bypass` on the card, and
`styles.css` scopes the bypass *button* with the same `.bypass` class
(`position: absolute; width: 16px`), so the card inherits those declarations.
The E2E suite asserts the card's `bypass` class and the editor's "bypassed"
state, but not its size, so the suite is green while the layout is broken.
Fixing it is out of scope for the E2E task; the fix is to namespace the button
rule (e.g. `.block-card .bypass`).
