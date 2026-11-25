# Running the app

How to get a running Pedal with a current UI, and how to unstick it when it
won't start. Everything here is verified on this machine.

## TL;DR

```bash
cd ~/Documents/Repos/pedal-dsp

./shell/run.sh dev     # live UI: edit ui/src, the app updates on save
./shell/run.sh ship    # the real thing: rebuilds and opens Pedal.app
```

Both start the engine for you and give every window its own engine on a free loopback port (no stale-port cleanup needed).

## Which one to use

| | `run.sh dev` | `run.sh ship` |
|---|---|---|
| UI source | `ui/src`, live | `ui/dist`, embedded at build time |
| Updates on edit | immediately (Vite HMR) | only after rebuilding |
| Window title | `Pedal` (dev URL) | `Pedal` |
| Engine | spawned by the shell | spawned by the shell |
| Use it for | playing with the UI, layout, colours | checking what actually ships |

**The app you double-click is a snapshot.** `Pedal.app` embeds a copy of the UI
made when it was built. Nothing in `ui/src` reaches it until `run.sh ship`
(or `shell/build-bundle.sh`) runs again. If the app "doesn't show the new
stuff", that is why — not a caching bug.

## What actually has to be running

Three processes, in this relationship:

```
Pedal (the shell)  ──spawns──>  pedal-engine  ──listens──>  ws://127.0.0.1:<free port, per shell>
      │                                              ▲
      └──renders the UI in a webview ──connects──────┘
```

- The **shell** (`pedal-shell`) is the app. It spawns the engine on launch and
  kills it on exit.
- The **engine** (`pedal-engine`) does the audio and owns the rig. The UI is a
  remote control; if the engine is down, the UI shows the boot screen and
  nothing you click does anything.
- **Vite** is only needed in `dev` mode.

So "no engine running" = the boot screen. The UI cannot show a rig without it.

## Manual steps (what the script does)

```bash
cd ~/Documents/Repos/pedal-dsp

# 0. clear anything already on the engine port
lsof -iTCP:8737 -sTCP:LISTEN -n        # see who holds it
kill -9 <pid>                          # a stale engine from an earlier run

# 1. dev mode — Vite first, then a *debug* build of the shell
cd ui && pnpm dev -- --port 5173 --strictPort &
cd ../ && cargo build -p pedal-shell   # NOTE: no --features custom-protocol
./target/debug/pedal-shell

# 2. shipped mode
./shell/build-bundle.sh                # rebuilds ui/dist + Pedal.app + .dmg
open target/release/bundle/macos/Pedal.app
```

### The `custom-protocol` trap

Tauri sets `dev = !custom-protocol`. So:

- `cargo build -p pedal-shell` → **dev build** → loads `devUrl`
  (`http://localhost:5173`). Needs Vite running. This is the live-reload mode.
- `cargo build -p pedal-shell --features custom-protocol` → **production
  build** → embeds `ui/dist`. Self-contained.

A debug build with no Vite running shows a **blank white window**, which looks
like a broken app but is just Tauri waiting for its dev server.

## Troubleshooting

### The UI says "Engine unavailable" / sits on "Connecting…"

The shell could not reach `ws://127.0.0.1:8737`. Check, in order:

```bash
lsof -iTCP:8737 -sTCP:LISTEN -n     # is an engine listening?
tail -30 /tmp/pedal-engine.log     # the engine's own diagnostics
```

- **Nothing listening, and the log ends with `Address already in use`.**
  A previous engine (or a test engine) still holds the port. Kill it:
  `lsof -iTCP:8737 -sTCP:LISTEN -n` → `kill -9 <pid>`, then relaunch.
  This is the single most common blocker.
- **Nothing listening and no log at all.** The engine never started. In
  shipped mode the bundle must contain `Contents/MacOS/pedal-engine`; in dev
  mode the shell looks in `target/debug/`, then `target/release/`, then on
  `PATH`. Run the engine by hand to see the error:
  `./target/debug/pedal-engine --rig 04-rainbow`
- **`client connected` appears, then the UI still shows the boot screen.**
  The UI is pointed at a *different* engine. In `dev` mode that means Vite is
  on a different port than the shell's `devUrl` (see below).

### Blank white window (dev mode)

Vite is not running, or not on 5173. Start it:

```bash
cd ui && pnpm dev -- --port 5173 --strictPort
```

`--strictPort` matters: without it Vite silently moves to 5174 when 5173 is
busy, the shell still loads 5173, and you get a blank window with no error.

### "The UI shows the wrong thing / someone else's session"

Same root cause — more than one Vite running. `run.sh dev` kills whatever is on
5173 first. By hand:

```bash
lsof -iTCP:5173 -sTCP:LISTEN -n
kill -9 <pid>
```

### Window opens tiny (e.g. 500×500) instead of 1280×760

macOS restored an old frame. Clear it:

```bash
rm -rf ~/Library/Saved\ Application\ State/app.pedal.savedState
```

Then relaunch. (The bundle identifier is `app.pedal`; it used to be
`com.pedal.app`, so old saved state under that name is dead weight.)

### Audio errors: `Device disconnected` / `buffer underrun or overrun`

The selected input/output device vanished or is shared — a KVM switching away
from the Mac does this, and so does a second engine fighting for the same
device. The engine keeps running and the UI still works; you just will not
hear anything. Quit stray engines and relaunch.

### Engine outlived the app

`pedal-shell` kills its engine on a normal quit. If the app is force-killed,
the engine survives and holds port 8737 and the audio device. Symptom: the
next launch says `Address already in use`. Kill it by port as above.

## Checking the UI is really talking to the engine

```bash
tail -5 /tmp/pedal-engine.log     # expect: client connected / <- getState
```

`<- getState` means the UI's `init()` ran. Anything you do in the UI shows up
here as `<- setParam`, `<- addBlock`, `<- moveBlock`, … — that log is the
ground truth when the UI appears to do nothing.

## Rebuilding after UI changes

```bash
cd ui && pnpm build                # writes ui/dist
./shell/build-bundle.sh            # embeds ui/dist into a fresh Pedal.app
```

`build-bundle.sh` already runs `pnpm build` itself, so the second command
alone is enough for shipped mode.

## Useful facts

- Engine port: a free loopback port per shell (the shell passes `--port`; 8737 is only the default for a bare `pedal-engine`). The shell injects the URL as `window.__PEDAL_ENGINE_URL__`.
- Engine log: **`/tmp/pedal-engine.log`** (stderr).
- Shipped bundles: `target/release/bundle/macos/Pedal.app`,
  `target/release/bundle/dmg/Pedal_0.1.0_aarch64.dmg`.
- Debug shell: `target/debug/pedal-shell`.
- Presets live in the repo (`presets/*.json`); `--rig <name>` picks one,
  default `04-rainbow`.
- Source selector in the top bar: **RIFF** (built-in test riff), **LIVE**
  (your guitar), **SILENCE** (no input — use this if you only want the UI).

## Testing the UI

The UI has a Cypress E2E suite that runs the real app against a fake engine (no
audio device, no sound):

```bash
cd ui && pnpm test:e2e
```

It covers boot, preset navigation and the preset browser, the rack, the
add-block picker, the signal chain (including both drag interactions), the
block editor, and keyboard selection. See
**[docs/E2E-TESTING.md](E2E-TESTING.md)** for what is covered, how to add a
spec, and how the fake engine works.

## Multiple shells / engine lifecycle

Each shell owns exactly one engine (own port, control pipe, log and pidfile in
the temp dir, keyed by `shell-<pid>`). On window close / quit it sends
`engine.wait`, waits 5 s, then kills; if the shell itself dies (even `kill -9`)
the engine sees the control pipe close and exits. Verify with
`scripts/verify-multi-shell.sh` (headless, silent): expects 2 / 1 / 0 engines.
