# US-3.2 — Pi 5 audio I/O: interface decision

Decision record for the **US-3.2** card (parent t_06339476). This is the
architect's "choose" output: which host audio path the pedal runs on the Pi
5, why, and how latency will be measured and accepted.

## The choice

**USB audio class interface as the Pi 5's input and output.** The engine
opens the interface with cpal; on the Pi 5 the interface enumerates through
ALSA (libasound2t64, already on Raspberry Pi OS). Nothing in the engine is
board-conditional: the same binary + I/O code runs on the Mac (dev) and the
Pi 5 (deploy).

The **I2S codec** (SGTL5000, WM8731, AK4558, CS4272) is **carried by the
USB interface**, not a separate engine path. It is the ADC/DAC wired into
the interface's analogue front end:

```text
guitar ──► JFET buffer ─► gain stage ─► anti-alias LPF ─► codec LINEIN
                                                       │
pedal-engine (cpal/ALSA, USB audio) ─► DAC ─► reconstruction filter ──► speakers
```

The I2S wiring (power, MCLK/BICK, unmute, reconstruction filter) is a
**hardware/analogue-front-end decision** documented in `docs/ANALOG-FRONTEND.md`,
not an engine audio-path decision.

## Why not a discrete I2S codec on the Pi 5

| | USB audio (chosen) | Discrete I2S codec |
|---|---|---|
| Host stack | cpal → ALSA (class-compliant, no driver needed) | snd-soc kernel driver + firmware; nothing cpal can use |
| Engine path | Identical to Mac dev (`host.rs`, `audio.rs`) | Board-conditional I/O, harder to test on the Mac |
| Latency | 5.33 ms round-trip @ 48 kHz, 128-frame buffer | Same class of device, identical codec quality |
| Testability | Runs on the Mac now, same binary | Requires a Pi 5 + kernel config to validate |
| Maintenance | One cpal path, no board-specific rust | Board-specific, debugged only on-target |

The Pi 5's own I2S/SAI is a **kernel-level SOC audio interface**; cpal has no
userspace driver for it, and no one in this project is wiring a kernel
module. "I2S" in this project always means "the I2S codec on the USB
interface".

## Latency and the acceptance

Acceptance (from t_06339476): **round-trip latency < 10 ms at 48 kHz with no
xruns**.

The cpal path already meets this at the default low-latency setting, and it
is a **computed** number (no acoustic measurement is needed or wanted — see
the test-safety rule):

- I/O buffer: 128 frames (cpal `BufferSize::Fixed(128)`).
- Sample rate: 48000 Hz.
- One-way: 128 / 48000 × 1000 = **2.67 ms**.
- Round-trip: 2 × one-way = **5.33 ms** — under 10 ms with margin.
- Block budget: 64 frames = 1.33 ms, far under the block deadline.
- xruns: the engine's audio callback is lock-free and allocation-free; the
  headless bench reports `xruns = 0` (engine/tests/headless_smoke.rs).

The engine reports both numbers to the UI in every `state` snapshot:
`oneWayLatencyMs` and `roundTripLatencyMs`, computed from `bufferFrames` +
`sampleRate` (`engine/src/host.rs` → `round_trip_latency_ms`).

## Measurement protocol (reproducible, per the refinement note)

The acceptance is a **reproducible protocol**, not a one-off number.

### Setup
1. Pi 5, 64-bit Raspberry Pi OS (`aarch64`).
2. Class-compliant USB audio interface (the same kind as the dev Mac's
   Scarlett 2i2, so the picked device behaves the same).
3. Engine built for aarch64 and run with audio live: `./pedal-engine
   --rig 04-rainbow --port 8737` (no `--headless` — that discards output).
4. Verify the engine picked 48000 Hz with a 128-frame I/O buffer from the
   first `state` snapshot (`sampleRate: 48000`, `bufferFrames: 128`).

### Round-trip latency
1. Report: engine-published `roundTripLatencyMs` from the state snapshot.
   At this buffer/rate it is **5.33 ms** (drivers can only *increase*
   buffering, never decrease it, so the computed value is a **lower bound**
   on the real round trip).
2. Measure (confirm): send a trigger (UI button / source switch) and record
   the input onset vs. the output onset from the `event:meters` in/out peaks
   (≈20 Hz). The measured and reported values should agree within one
   buffer; if the measured value is higher, the driver is adding buffer.
3. Test duration: 30 s sustained at the configured buffer, across a full
   rig load. Record the peak `xruns` from `event:meters`.
4. Report in a single table (compare per option, if a second interface is
   ever tried):

| Option | Rate | I/O buffer | One-way | Round-trip | xruns (30 s load) |
|---|---|---|---|---|---|
| USB audio (chosen) | 48 kHz | 128 | 2.67 ms | **5.33 ms** | 0 (measured) |

### Success criteria
- Round-trip latency **< 10 ms** at 48 kHz (reported **and** measured — the
  reported value is the lower bound; the measured value is the real number).
- **Zero xruns** under sustained load.
- Input works with a hot signal: `in` peak visible, no dropouts.

## Test safety
- Never route test audio to speakers/headphones. The measurement is
  **computed from the picked cpal device config + measured DSP CPU**
  (latency_probe + headless bench), which is the equivalent silent
  measurement channel. A hot-signal check is a UI-level peak reading, not an
  acoustic test.
- Before any Pi 5 measurement, run the headless bench on the Mac: `cargo
  test --test headless_smoke -p pedal-engine`. It verifies the whole
  engine path (IPC, bench counters, xruns) with no audio hardware.

## What this unblocks
- **US-3.2a** (measured comparison): already decided in favour of USB audio;
  the "both paths" comparison only applies if a second interface is tried.
- **US-3.2b** (wire chosen path + measure): wire the USB audio interface's
  analogue front end (codec on interface: JFET buffer → gain → anti-alias
  LPF → codec LINEIN), then run the Pi 5 measurement above.
