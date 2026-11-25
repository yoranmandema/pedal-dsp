# Pedal DSP — architecture & reference design

A from-scratch, Rust, native guitar pedal: a VST plugin and a
standalone app, with Raspberry Pi 5 as the deployment target. The DSP and
graph crates are cross-platform with no platform dependencies; the host app
drives them through a WebSocket IPC server running in a separate audio
process.

```text
dsp/               pedal-dsp — no_std DSP library, host-tested (portable)
graph/             dsp-graph — block graph + executor (portable)
host-sim/          CLI signal simulator (no platform deps)
pedal-engine       the audio process: cpal I/O, graph executor, WebSocket IPC
shell/             Tauri desktop shell: embeds ui/, spawns the engine
ui/                Preact + TypeScript frontend
```

> The `dsp`, `graph`, `host-sim` and `pedal-engine` crates compile unchanged
> for macOS (dev), Linux (deployment) and any other host cpal can reach.
> Platform-specific glue lives only in the thin audio layer and the shell;
> there is no bare-metal controller in the active path.

## 1. Platform decision

| Platform | Verdict | Notes |
|---|---|---|
| **Native VST + standalone** | Current target | The shipped app: a Tauri shell hosting the Preact UI, with the engine as a helper process. cpal picks the host's audio device. |
| **Raspberry Pi 5 (Linux, ALSA)** | Deployment target | The same engine binary runs here. Pi 5 is the settled hardware direction. |

The DSP, graph and host-sim crates are scheduler- and hardware-agnostic:
they depend only on `no_std` Rust + `alloc` and compile identically on every
host. Nothing downstream cares which platform runs the engine.

## 2. Signal path

```text
  guitar ──► [JFET buffer] ─► [anti-alias LPF] ─► USB audio interface (codec) ─► cpal ─► pedal-engine
                                                                                          │
                                                                                     graph
                                                                                          │
  pedal-engine ◄── cpal ◄── USB audio interface (codec) ◄── [reconstruction LPF] ◄── [output buffer] ◄──┘
                                                                                          │
                                                                                     UI (Preact)
```

* **guitar → codec**: a JFET input buffer (1 MΩ) and a short gain stage, then
  the USB interface's ADC. The analogue front end is designed in
  `docs/ANALOG-FRONTEND.md`.
* **engine → speakers**: the engine writes the codec's DAC through cpal,
  then a reconstruction filter and output buffer.

The DAC/ADC choice is a USB audio interface matter, not a microcontroller
matter. **Primary: SGTL5000** (I2C control, line in/out, headphone out, 95 dB
SNR DAC). Alternatives: WM8731, AK4558, CS4272.

## 3. Block graph (engine)

```text
in ─► DC block ─► distortion (4× oversampled) ─► tone stack ─► delay ─► reverb ─► DC block ─► out
```

* **DC block** — 20 Hz high-pass, removes offset so the distortion stage and
  the codec never see a DC component.
* **Distortion** — tanh / soft-clip / hard-clip / tube / fuzz / rectify,
  run through a 4× oversampling stage (two cascaded half-band FIRs) to push
  aliasing out of the audio band. Measured: the aliased 3rd harmonic of a
  hard-clipped 9 kHz tone drops by ~78×.
* **Tone stack** — a 200 Hz low shelf and a 4 kHz high shelf (RBJ biquads),
  ±12 dB.
* **Delay** — stereo feedback delay, up to 100 ms, with fractional reads.
* **Reverb** — Freeverb (8 combs + 4 allpasses), 132 KB of buffers.

## 4. Hardware (overview)

The active hardware is a USB audio interface with a stereo codec plus the
physical controls and enclosure. Development runs on the user's Mac
(Scarlett 2i2); the production target is a Pi 5 with the same class of USB
audio interface.

| Thing | Detail |
|---|---|
| Audio interface | USB class audio device with an I2S-capable stereo codec (SGTL5000, WM8731, AK4558, CS4272) |
| Analogue front end | JFET input buffer (1 MΩ), gain stage, anti-alias LPF, output reconstruction filter — see `docs/ANALOG-FRONTEND.md` |
| Controls | 3× 10 kΩ pots, 1× expression pedal, 1× footswitch, joined to the engine through the UI's control-binding layer |
| Power (Pi 5) | 5 V via USB or barrel; the audio interface carries 3V3 for the codec |

> Design notes (input impedance, gain staging, filters, switching, enclosure)
> live in `docs/ANALOG-FRONTEND.md`. There is no MCU pinout to spec here.

## 5. Audio pipeline

* cpal streams open the host's default input and output devices (optionally
  with a fixed small buffer size to keep latency low — see
  `docs/latency-analysis.md`).
* The audio callback is lock-free and allocation-free; the input→output ring
  is 2 blocks.
* The executor runs a flat `Vec<Step>` over a pre-allocated buffer pool — no
  allocation in the callback.
* The DSP, graph and host-sim crates have zero platform dependencies; only
  the engine's audio layer and the shell are host-specific.

## 6. Build

```bash
# engine — the audio process (build once, runs on any cpal host)
cargo build -p pedal-engine --release

# simulator — offline signal generation, no audio device needed
cargo run -p host-sim --release -- --rig demo --bench
```

## 7. Run the desktop app

From the repo root:

```bash
./shell/run.sh dev        # dev: Vite UI + engine, debug shell
./shell/run.sh ship       # rebuild and open the shipped Pedal.app
```

`dev` loads Vite's dev URL and needs the Vite server to stay running; `ship`
produces a self-contained app with the custom-protocol feature enabled. See
`docs/RUN-THE-APP.md`.

## 8. Testing

```bash
cargo test --workspace        # dsp, graph, host-sim, engine suites
```

The DSP tests run on the host, so the algorithms can be validated without
hardware. `pedal-dsp` is `no_std` and allocator-free (const-generic buffers).
The host-sim and engine unit tests exercise the graph, executor and latency
path without any audio device.

## Host-side engine registry (multi-instance)

`pedal_engine::registry::Registry` is the host's table of live engines keyed by
instance id. `open(id, rig)` spawns an engine with its own port and control
pipe and returns the port; `close(id)` sends `engine.wait` (then kills on
timeout) without touching other engines. Engine->host events (`Announced`,
`Closed`) arrive over one `mpsc` channel. Dropping the registry, or the host
dying (control pipes close -> EOF), shuts every engine down.

Manual orphan check: after closing a shell run `pgrep -fl pedal-engine`; only
engines of still-open shells should be listed, and `ls /tmp/pedal-engine-*.pid
/tmp/pedal-reg-*` should show no files for closed instances.
