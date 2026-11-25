# HANDOFF — pedal-dsp

**Written:** 2026-11-11
**For:** continuing this project in a fresh session (possibly on a local model).
**Read this first, then `PRODUCT-MODEL.md`, `EXECUTOR.md`, `DSP-BLOCKS-AND-SIMULATOR.md`.**

---

## 1. What this is

A from-scratch, Rust, **modelling guitar pedal**. The product idea is a
Quad-Cortex-like box: a free-form signal graph (amp/cab/effects/modulation),
editable on a screen, driven by a foot controller and an expression pedal.

The user is a **software-comfortable, electronics-beginner "product owner"**.
They want to *vibe-code* this, so the agent should explain electronics and
hardware choices in plain language and not assume EE knowledge. They want to
make the musical/UX decisions; the agent does the engineering.

### The one idea that defines the software

**A preset is a graph.** Nodes have typed audio ports; edges connect them; a
document is nodes + edges + per-node params, serialised as JSON. Parallel
branches, splitters, side-chains, "compressor after the amp but before the
cab", "pitch shifter after everything" — all of that is just graph topology.
This is the user's explicit requirement and it is already designed and partly
built.

---

## 2. Hardware status — **Raspberry Pi 5**

The user was leaning **Raspberry Pi 5** and already owns:
- a **M-VAVE "Chocolate"** Bluetooth MIDI foot controller (4 buttons)
- a **cheap expression pedal**
- some **I2S hardware**

### Research findings (from the earlier session)

| Topic | Finding |
|---|---|
| Quad Cortex engine | Analog Devices **SHARC ADSP-2156x** DSP, not a general-purpose CPU. Proves a modelling pedal does not need a Linux application processor. |
| Pi 5 as a pedal | Workable. **PiPedal** already turns a Pi into a NAM pedal with a web UI. But the user wants to write their own software, so the "just install PiPedal" route is out — PiPedal is inspiration, not a dependency. |
| Latency | The real risk on Linux. NAM's own guidance is 32×4 audio buffers; that is ~2.7 ms at 48 kHz, before the model itself. Needs real measurement on the Pi 5. |
| Open-source references | **PiPedal**, **Pi-MFX**, **OpenRig**, **coyopedal** — all permissively licensed; read them for architecture, don't vendor them. |
| Bluetooth MIDI | **Will not work** on a Pi 5 for this use: the built-in BT shares the USB bus with Ethernet and adds jitter. Use the Chocolate over **USB-C wired** (it enumerates as a standard USB-MIDI device). |
| Platform decision | Settled: native VST/standalone + Pi 5 via a USB audio interface. The DSP library is portable, so nothing is lost by deciding late. |

**The bare-metal controller path is dropped.** The Pi 5 is the target: NAM models are far beyond a small MCU, and the Pi's CPU headroom is the whole point.

### The architectural consequence (already applied)

**The DSP and graph crates have zero platform dependencies.** No ALSA, no
CoreAudio, no filesystem, no `std::process`. They compile identically for
macOS (dev), Linux/Pi 5 (deployment). The bare-metal controller path is dropped;
deployment). The platform-specific code lives in thin shells:

| Crate | Platform deps | Mac | Linux/Pi 5 |
|---|---|---|---|
| `dsp` (pedal-dsp) | none | ✅ | ✅ |
| `graph` (dsp-graph) | none | ✅ | ✅ |
| `host-sim` | none (CLI) | ✅ | ✅ |
| `live` (pedal-live) | cpal, egui, eframe | ✅ dev rig | ✅ |
| `firmware` (archived) | — | dropped | dropped |

**This is why the hardware decision can wait.** Nothing downstream cares.

### Settled decisions (from `PRODUCT-MODEL.md` §10)

1. **Sample rate: 48 kHz fixed.** NAM models are trained at 48 kHz; other rates
   need resampling (CPU + latency).
2. **Mono in, stereo out.** Stereo is introduced *after* the amp (reverb/delay).
3. **Block size: 64 frames** = 1.33 ms at 48 kHz. Tunable; measure on hardware.
4. **Feedback loops allowed**, but each loop must contain exactly one
   delay-owning node that owns the feedback buffer.
5. **Parameter units: fixed set** — dB, Hz, ms, %, semitones, cents, ratio, linear.
6. **Preset format: JSON** for v1.

---

## 3. Repo layout

```
~/Documents/Repos/pedal-dsp/          (git repo)

Cargo.toml                            workspace: dsp, graph, host-sim
                                      (firmware/ and live/ are EXCLUDED — separate workspaces)

dsp/                                  pedal-dsp — no_std DSP library
  src/biquad.rs                       RBJ biquads + shelving/peak/notch
  src/svf.rs                          state-variable filter
  src/delay.rs                        fractional delay, FeedbackDelay, PitchShifter
  src/pitch_vocoder.rs                NEW — phase vocoder pitch shifter
  src/pitch_remap.rs                  NEW — bin-remap pitch shifter
  src/bin_shift.rs                    NEW — simple bin shift
  src/fft.rs                          NEW — small FFT for the vocoder
  src/fir.rs                          windowed-sinc FIR, half-band decimator
  src/oversample.rs                   2x/4x oversampling + half-band
  src/waveshape.rs                    tanh/atan/soft-clip, Chebyshev drive
  src/envelope.rs                     ADSR + envelope follower
  src/lfo.rs                          sine/saw/square/triangle LFO
  src/reverb.rs                       feedback comb/allpass reverb
  src/dc_blocker.rs                   DC removal
  src/chain.rs                        helper chain builder
  src/util.rs                         db_to_lin, clamp, etc.
  examples/                           pitch_probe, vocoder_probe, wav_process, alias_check

graph/                                dsp-graph — node/edge graph + executor
  src/types.rs                        BlockSpec, PortSpec, ParamSpec, Unit, Curve
  src/block.rs                        Block trait, BlockCtx, BlockError
  src/blocks.rs                       Gain, ToneStack, Overdrive, Reverb, Delay,
                                      IoInput, IoOutput, MixSum
  src/graph.rs                        Graph: nodes, edges, validate, topo_sort
  src/executor.rs                     Executor: compile → step list + buffer pool
  src/registry.rs                     Registry: id → BlockSpec, builds blocks
  tests/graph.rs                      11 integration tests

host-sim/                             CLI simulator (no platform deps)
  src/main.rs                         rig builder, bench, palette, impulse, WAV
  src/blocks.rs                       re-exports dsp-graph blocks

engine/                               pedal-engine — the audio process
  src/audio.rs                        CoreAudio/cpal streams, device pick
  src/channels.rs                     Rig: channels of blocks, routing, to/from Doc
  src/store.rs                        preset directory, test riff
  src/server.rs                       WebSocket IPC + state snapshots
  src/headless.rs                     simulated device (no audio hardware)
shell/                                Tauri desktop app: embeds ui/, spawns the engine
ui/                                   Preact + TypeScript frontend (shared by every shell)
presets/                              *.json, shared by engine and host-sim

firmware/                             archived — dropped bare-metal controller
  src/main.rs, audio.rs, board.rs, codec.rs, controls.rs

live/                                 pedal-live — Mac/Pi real-time rig runner
  src/main.rs                         cpal audio I/O + egui/eframe UI

docs/
  ARCHITECTURE.md                     native architecture overview
  ANALOG-FRONTEND.md                  analogue front-end design
  BUILD-AND-FLASH.md                  firmware build/flash
  PRODUCT-MODEL.md                    ← the product spec (read this)
  EXECUTOR.md                         ← executor design (read this)
  DSP-BLOCKS-AND-SIMULATOR.md         ← block + simulator design (read this)
  HANDOFF.md                          ← this file
```

---

## 4. Where it stands — **all tests green**

```
cargo test --workspace     →  99 passed, 0 failed
                              (dsp 62, graph 20+5+4+5, examples 2, 1 ignored)
```

### What is built and verified

**Hardware target: Raspberry Pi 5.** The bare-metal controller path is
dropped. The current phase is
**prove the concept on the Mac with a UI**, then move to the Pi.

**`dsp` crate** — a real, tested, no_std DSP library (biquads, SVF, FIR,
oversampling, waveshaping, envelope, LFO, reverb, DC blocker, FFT, pitch
shifters).

**`graph` crate** — the engine:

- `Block` trait with typed audio ports, params, `prepare/process/reset`,
  `latency_samples()`, and a `configure(key, value)` hook for resources.
- `Graph` with validation (missing inputs, double-driven inputs, cycles) and
  Kahn topological sort. I/O nodes are located **by type**, not by position.
- `Executor` — compiles to a flat `Vec<Step>` over a pre-allocated buffer pool.
  **Zero allocation in `run()` and in every block's `process()`**, enforced by
  tests that install a counting allocator. `MAX_PORTS = 8` per side.
- **Latency compensation is implemented.** `latency_samples()` → the executor
  inserts delay lines on faster branches so parallel paths recombine in phase.
  Verified with a latent test block (100-sample delay across a block boundary).
- `Doc` — the preset document (JSON), with real-valued params and a `model`
  path for NAM blocks. `Doc::build` compiles to a `Graph`; `Doc::build_in`
  additionally resolves **relative** model paths against the preset's own
  directory, so a preset carrying `../models/foo.nam` still works after the
  folder is moved to another machine. Absolute paths pass through unchanged.
- Blocks: `IoInput`, `IoOutput`, `Gain`, `MixSum`, `Sum`, `Master` (limiter),
  `Overdrive`, `Amp` (2-stage oversampled, 3 voices), `Cab`, `ToneStack`,
  `Delay`, `PitchDelay`, `Reverb`, **`Nam`**.

**Engine + shell** — the shipped app. `shell/` is a Tauri window hosting the
Preact UI in `ui/`; it spawns `engine/` as a child process on launch and stops
it on exit. The UI never touches the audio path — it sends commands over
`ws://127.0.0.1:8737` and the engine applies them between blocks.

- **Channel strips.** A channel is an ordered strip of blocks; drag to
  reorder, click to select, `+ Block` palette, Delete to remove. The **Source**
  row routes audio in: `In` and/or another channel. A rig always has 4
  channels — they are fixed slots, never added or removed; an empty one is
  simply left out of the compiled graph. Each has its own master volume and
  limiter, summed through one limiter at the output.
- cpal audio I/O on the Scarlett at 48 kHz; riff / live / silence source;
  in/out meters; preset load/save to `presets/*.json`.
- Every structural change rebuilds the executor on the UI thread and hands it
  to the audio thread over a lock-free queue. The old executor is freed on the
  UI thread, never in the callback.
- `--selftest --rig <name>` renders 4 s of audio with no interface attached:
  peak/RMS per preset, non-finite count, CPU. All six presets pass and sound
  different.

**Presets** in `presets/`: 01-clean, 02-crunch, 03-two-amps, 04-rainbow,
05-endless-rise, 06-nam-amp.

---

## 5. What happened in the last session (the two bugs)

### Bug 1 — parameters were normalised but blocks read them as real

The graph stored params normalised (0..1); the executor passed them straight
through. So `set_param_real("drive", "drive", 4.0)` reached the block as `0.4`,
and `set_param_real("mix", "a", 0.7)` reached it as `0.00007`. The parallel rig
was essentially silent (peak 0.0001).

**Fix:** the executor now denormalises at compile time and on every
`set_param_real`. `Executor::params_real` holds the real values; `BlockCtx`
gets those. Added a regression test (`params_are_denormalised_before_reaching_the_block`).

### Bug 2 — the phase vocoder shifted to the wrong pitch

`pitch_vocoder.rs` measured the phase deviation between analysis frames but
computed the expected advance with the **synthesis** hop `hs = hop_a * ratio`
instead of the **analysis** hop `hop_a`. The wrapping then produced a bin
frequency that was wrong by a ratio-dependent factor.

Symptom: 220 Hz → 407 Hz on an octave up (should be 440), 440 Hz → 252 Hz on
an octave down (should be 220). Exactly the 133/239-cent errors the tests
reported.

**Fix:** two characters — `hs` → `ha` in the deviation and the division. Both
octave tests now pass within 10 cents; the probe shows ±1.6 cents.

**Lesson for the next session:** the host simulator and the probe examples are
what caught both bugs. Keep using them.

---

## 6. Known issues and next steps (in priority order)

### P0 — the product

1. **A real `.nam` model to validate against. DONE.** `MES DRECF RD Gain2 BAL
   O808 CAB.nam` (a `SlimmableContainer`, 2 WaveNet submodels, 48 kHz) loads,
   compiles, and renders clean audio: 3 s at 44.1/48/96 kHz, no non-finite
   samples, deterministic across runs, output 3.89× dry RMS. See
   `graph/examples/nam_check.rs` and `nam_render.rs`. The remaining unknown is
   only whether it *sounds* right — that needs ears, not tests.
2. **Sample-rate mismatch is now handled.** `Nam` resamples host↔model rate
   (cubic, measured −71 dB mirror) and the executor surfaces the mismatch to
   the UI, which shows a warning. Still worth confirming the resampled path
   sounds right on a model whose rate differs from the host.
3. **Cycle handling is "reject" only.** `validate()` returns `Cycle`. The design
   says a loop must contain exactly one delay-owning node.
4. **Stereo.** Everything is mono. Plan: mono until after the amp, then stereo
   for reverb/delay. `PortSpec` needs a channel count; the buffer plan needs
   2 slots per stereo buffer.
5. **Cab IR.** Convolution with a wav IR; `fir.rs` and `fft.rs` already exist.
   The `Cab` block is a filter approximation until this lands.

### P1 — hardware (Pi 5)

6. **Measure latency on the Pi 5.** Target < 10 ms round-trip, ideally < 5 ms.
   The Mac numbers (4–8% of one core per rig) suggest the Pi will manage, but
   the callback jitter is the real question.
7. **Analogue front-end for the Pi.** I2S ADC + DAC (e.g. PCM3060 or a
   combined codec) — not yet designed. The analogue front-end design in `ANALOG-FRONTEND.md`
   is the reference for the input conditioning.

### P2 — polish

8. **Preset store, hot-swap, boot, read-only rootfs** (Pi path).
9. **Control bindings.** A binding maps a physical input to a parameter with a
   range and curve; the same table drives GPIO, expression pedal, MIDI, or a
   web button. The M-VAVE Chocolate should be used **wired USB-MIDI**, not
   Bluetooth.
10. **Web UI** over a JSON API, for the Pi's touchscreen. The `Doc` format is
    already the API's body.

---

## 7. Conventions the next session must know

- **Params in the graph are normalised (0..1).** `set_param_real` denormalises;
  blocks always receive real values. Never pass a normalised value to a block.
- **`Unit::Percent` real values are 0–100**, and the block divides by 100.
  `Unit::Ratio` real values are 0–1.
- **`Unit::Choice(&[&str])`** is a discrete choice; the real value is the
  option index, `min = 0`, `max = len - 1`.
- **`cargo test --workspace`** builds `dsp`, `graph`, `host-sim`, `app`.
  `firmware/` is archived; `live/` runs the cpal/egui rig on Mac and Pi 5.
- **The UI edits a `Rig` (channels of blocks), not a `Graph`.** `Rig::to_doc`
  lays channels out as a graph; `Rig::from_doc` reads a preset back. A
  `util.master` block is the channel's output, never a strip block.
- **A channel's Source row is the routing.** A channel fed by several sources
  gets a `util.mix` split node; all channels sum through one `util.sum` and one
  output limiter.
- **The audio callback allocates nothing.** Parameter changes go to the
  running executor by node/param index (`set_param_at`), so turning a knob
  never recompiles the graph. Structural changes rebuild on the UI thread.
- The repo has a `.gitignore`d `target/`; `output.wav` is a generated artifact.
- No passwordless sudo on this Mac — privileged commands must be handed to the
  user.

---

## 8. Commands cheat-sheet

```bash
cd ~/Documents/Repos/pedal-dsp

# everything
cargo test --workspace
cargo build --workspace --release

# the desktop app (see docs/RUN-THE-APP.md)
./shell/run.sh dev        # live UI: Vite + the debug shell
./shell/run.sh ship       # rebuild and open the shipped Pedal.app

# the engine on its own
./target/debug/pedal-engine --rig 04-rainbow
./target/debug/pedal-engine --headless --source silence --bench 3

# the simulator
cargo run --release -p host-sim -- --rig demo --bench
cargo run --release -p host-sim -- --rig parallel --bench

# DSP probes
cargo run --release -p pedal-dsp --example pitch_probe
cargo run --release -p pedal-dsp --example wav_process -- in.wav out.wav

# PEDAL_PRESETS overrides the preset directory.
PEDAL_PRESETS=~/my-presets ./target/debug/pedal-engine
```

---

## 9. Open questions for the user

1. **A `.nam` file.** Which amp did you capture, and where is the file? This
   unblocks real validation of the modelling path.
2. **Screen on the Pi?** A touchscreen is the fastest route to a
   Quad-Cortex-like device; the web UI would drive it.
3. **How much of the analogue front-end do you want to build?** You are a
   beginner at electronics; the Pi path needs an I2S codec plus input
   conditioning, which is a real design task.
4. **NAM licensing.** NAM models are community-trained; confirm you are happy
   to use them in a device you might sell.
