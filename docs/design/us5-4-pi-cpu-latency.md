# US-5.4 — Pi 5 CPU headroom and latency (measurement methodology)

## Goal

A full chain plus a NAM model runs on the Pi 5 with headroom:

> Under 70 percent CPU at 48 kHz / 64-sample blocks with a NAM block in the
> chain.

## Rig selection

Use the preset `07-nam-od808` ("NAM OD808 + cab") — a full chain (input → NAM
model → reverb → master → output) with a real downloaded NAM model:

```text
"model": "../models/MES DRECF RD Gain2 BAL O808 CAB.nam"
```

This is the acceptance rig: a full chain *plus* a NAM block. Confirm the NAM
model loads and the rig compiles before starting the bench.

## Config

| Setting | Value |
|---|---|
| Sample rate | 48 000 Hz |
| Engine block size (executor) | 64 frames |
| Headless pace (CB) | 256 frames (simulated device) |
| Bench warm-up | 10 s |
| Bench measured | 60 s |
| Musical source | `riff` (source = 0) |
| Audio backend | cpal → ALSA, USB interface |

Run on the Pi with:

```sh
./pedal-engine --headless --rig 07-nam-od808 --bench 60
```

The bench reports `cpu` per second and an aggregate `cpu=` for the measured
window; the acceptance number is the **aggregate** `cpu` over the 60 s
measured window.

## Measurement methodology

Make the benchmark reproducible:

1. **Pi identity**: exact board (Raspberry Pi 5), RAM size, OS (64-bit Raspberry
   Pi OS), kernel version, date, hostname.
2. **Power state**: PSU (e.g., 5 V / 3 A USB-C), no other loads, run at a fixed
   ambient temperature.
3. **Thermal state**: record idle CPU/GPU temperature immediately before the
   run; record the maximum sustained temperature during the measured window.
4. **Audio backend**: ALSA device list, cpal picked rate/channels (from the
   engine's startup output).
5. **Configuration**: `--rig 07-nam-od808`, 48 kHz, 64-frame blocks, source
   `riff`.
6. **NAM model + chain**: `07-nam-od808` with the OD808 model present and
   resolvable (index `local_path` or legacy path).
7. **Warm-up + run duration**: 10 s warm-up (excluded from the metric), then
   60 s measured.
8. **CPU metric**: `cpu` = fraction of **one core** spent inside the audio
   callback, aggregated over the measured window (`cpu=` in the `bench done`
   line). Report per-second values too.
9. **Xruns**: cumulative buffer underrun count (`xruns`). Must be 0 for the
   acceptance window.
10. **Latency measurement**: `oneWayLatencyMs` and `roundTripLatencyMs` from
    the engine state (`bufferFrames` + `sampleRate`); the audio path itself is
    64-frame blocks. Report the device buffer if it differs from 64.

## Headroom definition

Headroom is **not** a short peak snapshot. It is defined across:

- **Sustained load**: the lowest `cpu` over the 60 s measured window, and the
  dropout (xruns > 0) if any;
- **Thermal throttling**: the CPU/GPU packages must not exceed the throttle
  threshold (e.g., 80 °C). If the Pi throttles, `cpu` rises and that is the
  headroom boundary.

Acceptance: **aggregate `cpu` < 70%** over the 60 s measured window, **0
xruns**, and no thermal throttling (temp stays under threshold throughout the
measured window).

## Mac-host proxy

The same bench runs on the development Mac with `PEDAL_PRESETS` pointed at the
checkout's `presets/`:

```sh
PEDAL_PRESETS=/path/to/presets ./target/release/pedal-engine --headless \
  --rig 07-nam-od808 --bench 60
```

The Mac number is a **proxy only** — the headless pacer overshoots on the Mac
(`realtime` < 100%), so the `cpu` there understates the real callback rate.
Report the Mac number **only if clearly labelled as a proxy**, and lead with
the Pi number as the acceptance metric.

## Exit criteria

- Aggregate `cpu` < 70% over the 60 s measured window → US-5.4 meets its
  acceptance.
- `xruns` = 0 for the measured window.
- No thermal throttling during the measured window.
