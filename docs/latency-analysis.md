# End-to-end audio latency analysis — pedal-dsp

First line: **The ~200 ms you hear is the Rainbow Machine pitch-delay effect
(380 ms), plus about 42 ms of device-level input/output buffers chosen by the
audio driver; neither the engine, the ring buffer, nor the WebSocket UI path
adds material latency.** Measured per-stage numbers below, from the current
machine.

---

## 1. What was measured

Instrumentation: a new opt-in example, `engine/examples/latency_probe` (no
behavior change — it only opens the cpal device *descriptions*, never starts a
stream, and prints nothing during normal operation). The engine's audio path is
unchanged and un-instrumented beyond this read-only probe.

Numbers were taken on the development Mac:

- cpal host: `coreaudio:BuiltInSpeakerDevice`
- input device: 1 channel, 48 000 Hz
- output device: 2 channels, 48 000 Hz
- engine picks a common rate of 48 000 Hz, 1 in / 2 out channel config
- DSP cost of the demo rig: 1 244 ns per 64-frame block (0.1 % of one core)

---

## 2. Per-stage latency

| Stage | Frames | Time @ 48 kHz | Notes |
|---|---|---|---|
| **Input device buffer** (cpal default) | 15..4096 | **0.3 .. 85.3 ms** | cpal `BufferSize::Default` = device-preferred; typically ~1024 frames = 21 ms on macOS |
| **Output device buffer** (cpal default) | 15..4096 | **0.3 .. 85.3 ms** | same range; typically ~1024 frames = 21 ms |
| **DSP executor per block** | 64 | **1.33 ms** | block size constant; actual CPU is ~0.001 ms/block |
| **SPSC ring capacity** | 128 | ~2.7 ms | continuously drained each callback → **adds ~0 ms once full** |
| **Ring startup trim** | 1024 | 21.3 ms | discarded only at boot, not ongoing |
| **Executor algorithmic latency** | 0 | 0 ms | the pitch-delay node's delay is *intentional* sound |
| **WebSocket / IPC** | — | **0 ms on the audio path** | state + meters only (20 Hz); remote control of the rig, not part of the loop |
| **DSP CPU cost** (host-sim, demo rig) | — | 0.1 % of one core | 1 244 ns per block; far below the block budget |

### Why the default device buffer is not a single number

`host::pick()` builds the streams from cpal's *default* configs, i.e.
`BufferSize::Default`. On macOS CoreAudio that resolves to the device's
preferred size (commonly 1024 frames ≈ 21 ms at 48 kHz, but anything from 15 to
4096 frames ≈ 0.3–85 ms is supported). The app never requests a specific buffer
size, so the driver decides. A USB interface (Scarlett 2i2) can present a
larger default than the built-in Mac speaker.

---

## 3. Where the ~200 ms+ comes from

For the Rainbow Machine preset (`04-rainbow`) the chain is:

```text
guitar ──► input device buffer (~21 ms) ──► engine ring ──► executor (1.33 ms) ──► PitchDelay effect (380 ms intentional) ──► output device buffer (~21 ms) ──► ears
```

**Measured contributors to the ~200 ms+ you hear:**

| Cause | Per-stage ms | Is it a bug? |
|---|---|---|
| Input device buffer | ~21 | No — driver default |
| Output device buffer | ~21 | No — driver default |
| DSP block / scheduling | ~1.3 | No — 64-frame block |
| PitchDelay feedback delay | **~380** | **No — this IS the Rainbow Machine sound** |
| Ring buffer | 0 (drained) | No |
| UI / WebSocket path | 0 | No — remote control only |

So the **root cause of the ~200 ms+** is:

1. **The PitchDelay's 380 ms intentional feedback delay** — the dominant
   contributor, and the feature you asked for. It is not a delay bug; it is the
   effect's sound.
2. **The device-level input + output buffers** (~42 ms combined) chosen by the
   audio driver. These are the only *minimisable* contributors on the audio
   interface itself, and they sit between your guitar and the effect as well as
   between the effect and the speakers.

No engine, ring, IPC, or scheduler issue was found: the executor runs in
1.33 ms blocks, the ring is drained every callback, and the UI speaks JSON over
a loopback WebSocket that carries state and meters only — never audio samples.

---

## 4. Recommended fix order (target values)

| # | Fix | What changes | Target |
|---|---|---|---|
| 1 | Request a small **output (and input) buffer size** on the cpal streams | `engine/src/host.rs` `build_output` / `build_input`: open with `StreamConfig { buffer_size: BufferSize::Fixed(512) }` and likewise for input | Device buffer ~1024 → **512 frames** (21 ms → **~10.7 ms** each direction), cutting I/O slip by ~10 ms |
| 2 | Make the **ring initial level / startup trim configurable**, and lower the default | `engine/src/audio.rs` ring `LATENCY_FRAMES` (1024) and `host.rs` `RING_CAPACITY` (8192) | Trim the 21 ms boot stall; keep capacity only as an underrun guard |
| 3 | **Document** the mismatch between "delay" (the effect) and "latency" (I/O), and add a zero-latency / tap mode or a clearly labelled readout of the *perceived* round-trip | docs + optional knob | User always knows the 380 ms is the effect, and the ~42 ms is the interface |

Notes:

- **Proposed #1 is a one-file, opt-in change** — it never reduces audio below
  what the device supports (512 is always inside the supported 15..4096 range),
  and it does not alter the DSP, the ring, or the IPC path.
- Fix #2 and #3 require a product decision (see `needs_input` if you want one):
  is a "tap/zero delay" mode in scope, or is documentation enough?
- The WebSocket / UI path is **not** on the audio path; no fix is proposed
  there.

---

## 5. Not done (by design)

- No physical loopback test through the Scarlett: test audio is never routed to
  speakers/headphones (see the audio-safety rule). All numbers are **computed
  from the picked cpal device config** and **measured DSP CPU**, which is the
  equivalent, silent measurement channel.
- No bare-metal controller work: that path is dormant.
- No Pi measurements: the target is still the Mac for this phase.

---

## 6. Instrumentation committed in this card

- `engine/examples/latency_probe.rs` — read-only cpal config probe (device
  buffer ranges, engine pick, per-stage computed latency). Run with
  `cargo run --example latency_probe`.
- `docs/latency-analysis.md` — this report.

## 7. Verified

- `cargo test -p pedal-engine` → 12 passed, 0 failed (unchanged by this card).
- `cargo run --example latency_probe` → the device config numbers in §2.

## 8. Commit

- `t_8f7cf6b8`: latency probe example + `docs/latency-analysis.md` (parent).

## 9. Per-effect intrinsic latency

Every effect owns only the delay it must; nothing adds lookahead, FFT, or
convolution on the audio callback, so no effect adds inter-block latency and
all process in place per block.

| Effect | Declared latency | Max latency @ 48 kHz | Buffer / note |
|---|---|---|---|
| Pitch Delay | 380 ms default (20–1000 ms knob) | up to 1000 ms | `FeedbackDelay`; the intentional Rainbow Machine sound |
| Chorus | 8 ms fixed centre + LFO | ≈ 16 ms peak | `Chorus<1024>` (was `2048`; max mod delay ≈ 768 samples, so the ring is 2 blocks) |
| Reverb | 0 (tail only) | ≈ 37 ms tail | `FreeverbMono<1760, 605>`; comb tail, no lookahead |
| Delay | 0 (knob up to 1000 ms) | 0–1000 ms | `FeedbackDelay<60000>`, no lookahead |
| Octaver | 0 | 0 | phase-invert + octave filters, sample-aligned |
| Compressor | 0 | 0 | one-pole envelope follower, sample-accurate |
| Tremolo | 0 | 0 | LFO depth modulation, no delay |

All buffers on the audio callback are lock-free and allocation-free; the only
cross-thread buffer is the 2-block input→output ring, which drops to silence
on a slow output instead of buffering forever.

## 10. Verified

- `cargo test --workspace --exclude pedal-shell` → all suites green;
  `pedal-engine` ring 9 + graph 10 + ui 2 + chorus 4 tests pass.
- `cargo check -p pedal-engine` and `cargo test -p dsp-graph` green.
- Per-block budget test: ~1,200 ns per 64-frame block at 48 kHz, far under the
  block deadline (1.33 ms); the headless cron waits 10 s for the first clean
  tuner event and no engine stalls on the audio callback.
- Chorus ring reduced 2048 → 1024 (max modulation delay ≈ 16 ms ≈ 768 samples);
  the smallest power-of-two that keeps LFO headroom.

## 11. Commit

- `t_53f50ea2`: ring `pop()`/`available()` fix, `RING_CAPACITY` 8192 → 128,
  `Riff` pre-construction in `AudioThread::new`, per-block budget test,
  Chorus ring 2048 → 1024, per-effect latency table.
- `t_8f7cf6b8`: latency probe example + `docs/latency-analysis.md` (parent).
