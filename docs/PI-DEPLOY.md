# Deploying to the Pi 5

The Pi 5 runs the same engine process as the desktop shell: audio + IPC, no
UI. The webview shell connects to it over `ws://127.0.0.1:8737`. This page is the
path from a fresh Pi to a measured US-3.2 / US-5.4.

## What you need

| Item | Note |
|---|---|
| Raspberry Pi 5, 64-bit Raspberry Pi OS | `aarch64`. 32-bit OS is not supported. |
| A USB audio interface | The pedal's input and output. The Pi's own audio is not used. |
| `libasound.so.2` | Package `libasound2t64`; already on Raspberry Pi OS. |
| Network or a stick | To copy the binary (or the whole repo, to build on the Pi). |

The engine binary needs nothing else — no Rust toolchain, no Python, no Node.

## 1. Get the binary onto the Pi

**Option A — build on the Pi (simplest).** Copy the repo, then:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
cargo build --release -p pedal-engine
./target/release/pedal-engine --headless --bench 10
```

**Option B — cross-build from the Mac.** One script assembles an aarch64
toolchain, libasound for aarch64, and the engine:

```sh
engine/cross-build.sh
```

It prints the resulting ELF and its `NEEDED` libraries. Copy the binary:

```sh
scp target/aarch64-unknown-linux-gnu/release/pedal-engine pi@raspberrypi.local:~/pedal/
```

## 2. Smoke test it (no interface needed)

The engine can run with no audio device at all. `--headless` replaces the cpal
streams with a simulated device — real callback, real executor, output
discarded — and `--bench <seconds>` reports the CPU cost of the rig:

```sh
./pedal-engine --headless --port 8737 --bench 10
```

Expected output (the exact numbers depend on the rig and the machine):

```
pedal-engine: preset 04-rainbow, ipc on ws://127.0.0.1:8737
pedal-engine: headless (no audio device), source riff
pedal-engine: ipc listening on ws://127.0.0.1:8737
pedal-engine: bench 1s cpu 0.9% of one core, frames 27392 (19.0% real time), xruns 0
...
pedal-engine: bench done seconds=10.0 callbacks=356 cpu=0.9 frames=273920 realtime=19.0 xruns=0
```

`cpu` is the fraction of **one core** spent inside the audio callback. That is
the US-5.4 number: the headroom left on the Pi after the DSP.

The same test runs as a Rust integration test, and is the one to run on the Pi
if the repo is there:

```sh
cargo test -p pedal-engine --test headless_smoke
```

It spawns the real binary, connects a real WebSocket client, turns a knob, and
checks the bench counters — the whole path, with no audio hardware.

## 3. US-3.2 — audio I/O

Attach the interface, then run the engine normally (no `--headless`):

```sh
./pedal-engine --rig 04-rainbow
```

The engine lists what it sees and what it picked:

```
  in [0] Scarlett 2i2 USB
  out [0] Scarlett 2i2 USB
using input: Scarlett 2i2 USB
using output: Scarlett 2i2 USB
input : Scarlett 2i2 USB @ 48000 Hz, 4 ch
output: Scarlett 2i2 USB @ 48000 Hz, 2 ch
```

Selection prefers a device whose name contains `Scarlett`, then any `usb`
device, then the first one. It picks 48 kHz if both devices support it, else
44.1 kHz. If the pick is wrong, the names printed are the ones to match.

Then check the sound path with the UI (or any WebSocket client): the engine
greets a new client with a full `state` snapshot, and `setParam` replies with
the new state.

## 4. US-5.4 — CPU headroom and latency

Run the headless bench for a minute on the real rig, with nothing else running:

```sh
./pedal-engine --headless --bench 60
```

Compare the last `cpu=` against the budget. At 48 kHz with 64-frame blocks the
executor runs 750 blocks/s; one core is 100%, so `cpu` is directly the share of
a core the DSP needs. Headroom per core is `100 - cpu`.

Latency is fixed by the engine and does not depend on the Pi: the UI applies a
command between blocks, so worst-case UI-to-sound latency is one block —
64 frames = 1.33 ms at 48 kHz (see US-2.10 in `docs/WORKLOG.md`). The remaining
term is the device buffer, which is the interface's own setting.

## 5. Running it as the pedal

The shell (`shell/`) is the Tauri webview that spawns the engine and loads the
Preact UI. On the Pi it runs borderless fullscreen on the touchscreen (US-3.1,
US-3.4) — that's `shell/tauri.linux.conf.json`, a platform-specific override
merged over the base `shell/tauri.conf.json` only when building for Linux (the
Pi's OS). The base config stays a normal resizable, decorated window, which is
what macOS dev/ship builds use. Until those are done, any WebSocket client can
drive the engine; the UI in `ui/` is the intended one.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `no audio input devices` | No interface attached, or the Pi sees it but ALSA doesn't. Check `arecord -l` / `aplay -l`. |
| `pedal-engine: Address already in use` | A stale engine holds 8737. `lsof -nP -iTCP:8737 -sTCP:LISTEN`, kill it, or pass `--port`. |
| `error while loading shared libraries: libasound.so.2` | `sudo apt install libasound2t64`. |
| `realtime` well under 100% | The simulated device's sleep overshoots on some hosts. It does not affect the `cpu` figure. On the Pi it should be near 100%. |
