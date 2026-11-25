# Building, testing and deploying

## Toolchain

The same host toolchain is used for macOS development, Linux/Pi 5
deployment and any target cpal can reach. No embedded cross-compilation
toolchain is needed for the active path.

| Tool | Purpose |
|---|---|
| rustc / cargo | Host compiler for all crates |
| cpal | Audio device I/O (CoreAudio / ALSA / WASAPI) |
| Vite + pnpm | UI bundling (shell/) |

## Build

```bash
# engine — the audio process (build once, runs on any cpal host)
cargo build -p pedal-engine --release

# simulator — offline signal generation, no audio device needed
cargo run -p host-sim --release -- --rig demo --bench
```

## Run the desktop app

From the repo root:

```bash
./shell/run.sh dev        # dev: Vite UI + engine, debug shell
./shell/run.sh ship       # rebuild and open the shipped Pedal.app
```

`dev` loads Vite's dev URL and needs the Vite server to stay running; `ship`
produces a self-contained app with the custom-protocol feature enabled. See
`docs/RUN-THE-APP.md`.

## Testing

```bash
cargo test --workspace       # dsp, graph, host-sim, engine suites
```

Cross-platform CI runs this on macOS and Linux.

## Pi 5 deployment (later, hardware-dependent)

When the Pi 5 and a USB audio interface are available, the same engine
binary builds and runs under cpal's ALSA backend:

```bash
cargo build -p pedal-engine --release
./target/release/pedal-engine --rig <preset>
```

The DSP, graph and host-sim crates need no changes. Packaging the
fullscreen touchscreen app and its control bindings is parked until the
hardware is available (see `docs/ROADMAP.md`).
