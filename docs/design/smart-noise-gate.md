# Smart Noise Gate — architecture & data model

**For:** `pedal-dsp` (engine `dsp_graph`/`engine`, UI Preact).
**Effect id:** `dynamics.smart_noise_gate` · **Category:** `Dynamics` (display order 5th: after Multiband Compressor, before Time).
**Status:** designed, no code changed. Builder cards follow.

---

## 1. Object model & registration

* The Smart Noise Gate is **one block in a channel strip**, exactly like
  `dynamics.compressor` / `dynamics.multiband_compressor`. It is not a group of
  blocks.
* It is registered in `dsp_graph::blocks::builtin_registry` (engine) and mirrored
  into the UI registry (`registry` in every `state` snapshot) so the **param
  editor is identical on both sides** — the UI reads `BlockSpec.params` from the
  engine, it does not know the block's name/shape by convention.
* Serialization: a smart noise gate block is a `NodeDoc` entry (`id`, `block:
  "dynamics.smart_noise_gate"`, `params: BTreeMap<String, f32>`, `bypass`).
  All numeric params are stored by name.

---

## 2. Signal flow

```
input ──► [line-frequency notch] ──► [overlap-add STFT processor] ──► [dry/wet] ──► output
                              │ (pre-filter)              │ (spectral gating)
                                                              │
                                                         [meter: reduction dB]
```

### 2.1 Line-frequency notch (pre-filter)

Before the STFT stage, a **dual-notch filter** removes mains hum (50 Hz or 60 Hz)
and its first two harmonics. This is a cheap, time-domain pre-process that targets
the most common noise source and reduces the load on the spectral stage.

* **Q:** 30 (narrow, musical — preserves nearby notes).
* **Fundamental:** 50 Hz (EU) or 60 Hz (NA), selectable via the `hum_freq`
  parameter (see §4). Defaults to 60 Hz.
* **Harmonics:** 2× and 3× the fundamental (100 / 120 Hz in EU; 120 / 180 Hz in
  NA).
* Each notch is a **biquad** built with `pedal_dsp::biquad::Biquad::design()`
  using the standard notch transfer function.
* Three cascaded NotchFilter instances per channel (fundamental + H2 + H3).
* If `hum_notch_enabled` is false (default), this stage is a bypass (gain = 1).

### 2.2 Overlap-add STFT processor (core)

The heart of the effect is a **block-based STFT** (Short-Time Fourier Transform)
with overlap-add (OLA) reconstruction. The frame/hop parameters are fixed at
design time (not per-block parameters) so the FFT size, window type, and hop
ratio are compile-time constants.

* **FFT size:** N = 1024 samples (compile-time const, uses `pedal_dsp::fft::Fft<1024>`).
  At 48 kHz this gives ~21 Hz bin resolution — sufficient to separate noise bins
  from most musical content.
* **Hop ratio:** 4:1 overlap (hop = 256 samples). This means a new FFT frame every
  256 samples.
* **Window:** Hann window (pre-multiplied into input before FFT, post-multiplied
  into output before OLA sum). Equal-power analysis/synthesis window.
* **OLA reconstruction:** overlap-add with 4:1 ratio → latency = 3 × hop = 768
  samples (~16 ms at 48 kHz). The executor compensates for this.
* **Latency:** declared as `latency_samples = 768` (the OLA half-window delay).
  The engine's latency tracker adds this to the channel's total.

#### Per-frame processing pipeline

```
1. Take hop=256 new samples (or fill with overlap from last frame).
2. Multiply by Hann window (analysis).
3. FFT (Fft<1024>) → complex spectrum [re, im].
4. Compute per-bin energy: magnitude[k] = sqrt(re[k]² + im[k]²).
5. For each bin k:
   a. Convert energy to dB: energy_db = 20 * log10(magnitude[k] + floor).
   b. Compare to threshold (in dB).
   c. If energy_db > threshold: gain[k] = 1.0 (keep signal).
   d. If energy_db ≤ threshold: gain[k] = attenuate(energy_db - threshold).
6. Apply gain: re[k] *= gain[k], im[k] *= gain[k].
7. Inverse FFT → time-domain samples.
8. Multiply by Hann window (synthesis).
9. Overlap-add into output buffer.
```

#### Gain curve (§2.2 detail)

The gain curve is a **smooth exponential falloff** for bins below threshold,
inspired by ShutUp's per-bin thresholding but implemented from first principles:

```
delta = threshold_db - energy_db    // positive means "below threshold"
if delta <= 0:
    gain = 1.0                      // above threshold: no change
else:
    gain = 10^(-(delta * reduction_db / 20.0))   // exponential
    gain = max(gain, 10^(reduction_db / 20.0))   // floor at max reduction
```

This gives a smooth, continuous curve — bins just below threshold are lightly
attenuated; bins far below are heavily reduced. At `reduction_db = -60`, a bin
40 dB below threshold gets 60 dB of attenuation.

### 2.3 Envelope follower (smoothing)

Per-bin gain is tracked with an **envelope follower** using `pedal_dsp::envelope::EnvelopeFollower`.
This smooths the gain curve over time, preventing pumping and artifacts when noise
levels fluctuate.

* **Attack:** derived from the `release` parameter (fast attack: 5 ms fixed, no
  attack param — noise onset is always instant).
* **Release:** user-adjustable (0.1–10.0 s, default 2.0 s). Slow release avoids
  the pumping artifacts that ShutUp avoids by using 6 s defaults.
* The envelope tracks per-bin gain and applies smoothing between FFT frames.

### 2.4 Dry/wet crossfade (final mix)

The processed signal is mixed with the original input using an **equal-power
crossfade** controlled by the `strength` parameter.

* `strength = 0` → dry signal (full pass-through, no noise reduction).
* `strength = 100` → wet signal (full noise reduction applied).
* Intermediate values blend continuously between the two.
* Uses `pedal_dsp::util::mix()` (same as existing blocks).

---

## 3. Parameter set

### 3.1 Global parameters

| param | id | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| strength | `strength` | Percent | 0 | 100 | 75 | Linear |
| threshold | `threshold` | dB | -80 | 0 | -50 | AmpGain |
| release | `release` | Seconds | 0.1 | 10.0 | 2.0 | Log |
| reduction | `reduction` | dB | -100 | 0 | -60 | AmpGain |
| hum freq | `hum_freq` | enum | 0 | 1 | 1 | Linear |
| hum notch | `hum_notch_enabled` | on/off | 0 | 1 | 0 | Linear |

* **Strength** (0–100%, default 75): Wet/dry mix control. At 0% = dry (transparent
  bypass). At 100% = full noise reduction. Intermediate values let the user dial
  in aggressiveness.
* **Threshold** (-80 to 0 dB, default -50): Noise floor detection threshold in dB.
  Bins below this level are attenuated. The wider range vs. the classic Gate (-60
  to 0) accommodates lower noise floors.
* **Release** (0.1–10.0 s, default 2.0): Envelope release time. Slow release
  avoids pumping. Fast release (0.1 s) for tight noise reduction on quiet passages.
* **Reduction** (-100 to 0 dB, default -60): Maximum attenuation per noise bin.
  -100 = near-silence for below-threshold bins; 0 = no attenuation. This replaces
  ShutUp's "Strength" parameter as a dedicated control — the actual effect is
  split between Strength (mix) and Reduction (intensity).
* **Hum freq** (0 = 50 Hz, 1 = 60 Hz, default 1): Selects European (50 Hz) or
  North American (60 Hz) mains frequency. Sets the fundamental notch frequency.
* **Hum notch** (off/on, default off): Enables the line-frequency pre-filter.
  When on, applies three cascaded biquad notch filters at fundamental and first
  two harmonics. This is separate from the spectral stage — it handles the most
  common noise source before FFT processing.

### 3.2 Attack parameter — deliberate omission

No attack parameter is exposed. The attack is **fixed at 5 ms** (envelope follower
attack time) because:
* Fast attack on noise reduction is always desirable — you want noise removed
  immediately when it appears.
* Slow attack would cause noise to "bleed through" before the gate closes, which
  is undesirable for hum/noise removal.
* The release parameter provides sufficient user control (0.1–10 s covers the
  useful range).

---

## 4. Data model

```rust
pub struct SmartNoiseGate {
    // FFT state
    fft: Fft<1024>,
    /// Input buffer for windowing (1024 samples).
    in_buf: [f32; 1024],
    /// Accumulator for overlap-add (1024 samples).
    /// Filled with zeros on reset, incremented each frame.
    acc: [f32; 1024],
    /// Current write position in acc (advances by hop=256, wraps at 1024).
    write_pos: usize,
    /// Frame counter (advances by hop=256 each frame; used for phase alignment).
    frame_counter: usize,

    // Per-bin energy tracking
    /// Per-bin magnitude estimates (1024 bins, power-of-two size).
    mag: [f32; 1024],
    /// Per-bin phase (complex FFT output, accumulated in acc).
    phase_re: [f32; 1024],
    phase_im: [f32; 1024],

    // Envelope follower (applied per-bin)
    env: EnvelopeFollower,

    // Parameters
    threshold_db: f32,
    release_s: f32,
    reduction_db: f32,
    strength: f32,
    hum_freq_hz: f32,          // 50.0 or 60.0
    hum_notch_enabled: bool,

    // Line-frequency notch filters (pre-filter)
    notch1: Biquad,             // fundamental
    notch2: Biquad,             // 2× harmonic
    notch3: Biquad,             // 3× harmonic
}
```

### 4.1 FFT scratch buffers

The scratch buffers (`in_buf`, `acc`, `mag`, `phase_re`, `phase_im`) are **1024
elements each**. The executor provides `MAX_BLOCK = 64` samples per frame; the
block accumulates `1024` samples internally and performs one FFT per frame. The
block stores its own `write_pos` and `frame_counter` — these are updated in
`process()`, not by the executor.

### 4.2 Sample rate handling

* FFT size is fixed at compile time (1024). Bin resolution = sr / 1024.
* Hop size is fixed at compile time (256 = 1024 / 4).
* Notch filter frequencies are independent of sample rate (50/60 Hz).
* Envelope follower attack/release time constants are recomputed in `prepare(sr,
  max)` (like existing blocks).
* Latency (768 samples at 48 kHz) is reported as `sr * 0.016` — proportional to sr.

---

## 5. Real-time safety

* **No allocations in the audio thread.** All DSP state is stack-allocated
  (fixed arrays sized to 1024). No `Vec` or `String` in `process`.
* **No locks.** The block holds only `f32` coefficients + per-bin `EnvelopeFollower`
  (no atomics, no mutex). Parameter reads within a `process` call are safe because
  the executor snapshots params into the context and applies updates between frames.
* **Denormal handling:** `mag[k]` is floored at `1e-4` (matching existing Gate /
  Compressor) so `log10`/`lin_to_db` never receive denormals. The envelope follower
  input is also floored.
* **Sample-rate changes:** envelope time constants and notch filter coefficients are
  recomputed in `prepare(sr, max)`. No param is reactive to sr at runtime.
* **FFT size:** fixed at compile time (1024). The block does not resize at runtime.
* **Block size:** fixed to `MAX_BLOCK = 64`; the executor normalises all blocks
  to this size.

---

## 6. Metering

* **Peak reduction:** per-frame, the block computes the maximum gain reduction
  applied (in dB) across all bins. This is stored as an `AtomicU32` (f32 bits),
  matching the existing `Meters` pattern.
* **UI:** the block's param editor shows a small **reduction meter** (horizontal
  bar, 0 to -60 dB, green → red) below the controls. This is updated from the
  `event: meters` event (every 100 ms).
* **Meters extension:** extend `engine/src/audio.rs` `Meters` with:
  ```
  { in_peak, out_peak, xruns, reduction_db: f32 }
  ```
  where `reduction_db` is the peak gain reduction applied by the noise gate.

---

## 7. Test plan

1. **FFT round-trip.** Run a 1 kHz sine wave (below threshold) through the block
   with `strength = 100`, `threshold = -100` (all bins above threshold). Assert:
   output equals input to within **0.01 dB** (numerical round-trip error of the
   Hann window + FFT).
2. **Spectral noise reduction.** Feed broadband noise (flat spectrum, -40 dBFS)
   plus a 1 kHz tone at -20 dBFS. Set `threshold = -30`, `reduction = -60`.
   Assert: the 1 kHz tone is preserved (within 0.5 dB); noise floor is reduced
   by **≥ 50 dB** (at spectral bins where energy < threshold).
3. **Threshold sweep.** Drive with broadband noise at -20 dBFS. Sweep `threshold`
   from -80 to 0. Assert: the output noise level follows the threshold linearly
   (within 2 dB) when threshold > noise level.
4. **Release time accuracy.** Feed noise that starts at 0 dBFS and drops to -60 dBFS
   at sample 0. Set `release = 2.0 s`. Assert: the gain envelope follows a 2 s
   decay (within 10% of target at t = 0.5, 1.0, 1.5, 2.0 s).
5. **Hum notch.** Feed a 60 Hz sine wave at -20 dBFS + a 1 kHz tone at -20 dBFS.
   Set `hum_notch_enabled = on`, `hum_freq = 60`. Assert: 60 Hz is attenuated by
   **≥ 30 dB**; the 1 kHz tone is preserved (within 0.5 dB).
6. **Strength / dry-wet mix.** Same input as test 2, with `strength = 50`. Assert:
   the output noise reduction is approximately half of the full-effect reduction
   (linear crossfade: within 3 dB of the expected 50% reduction).
7. **Silence / NaN safety.** 10 s of silence → output all-zero, no NaN/Inf, no
   xruns. NaN input → no crash/assert.
8. **Sample-rate change.** Run at 48 kHz → 44.1 kHz → 48 kHz mid-run; state
   parameters stay finite and the noise reduction is consistent at each rate.
9. **Latency reporting.** After `prepare`, assert `spec.latency_samples == ceil(sr * 0.016)`.

---

## Builder cards

All cards are one session of work (≈1–4 files), concrete, and leave the app green.

### Engine cards (assignee: `pedal-builder`)

1. **`t_*_eng_1 — smart noise gate block core`**
   Add `SmartNoiseGate` to `graph/src/blocks.rs`: `BlockSpec` id
   `dynamics.smart_noise_gate` (category `Dynamics`), params = `strength`
   (Percent, 0–100, default 75), `threshold` (dB, -80–0, default -50, AmpGain),
   `release` (Seconds, 0.1–10, default 2.0, Log), `reduction` (dB, -100–0,
   default -60, AmpGain), `hum_freq` (enum 0–1, default 1),
   `hum_notch_enabled` (on/off, default 0). Signal flow = §2.2 overlap-add STFT
   processor (Fft<1024>, Hann window, hop=256, OLA) with per-bin thresholding,
   envelope smoothing (EnvelopeFollower, attack=5 ms fixed, release from param),
   dry/wet crossfade (util::mix), and optional pre-filter line-frequency notch
   (three cascaded biquad notch filters). `prepare(sr, max)` builds the FFT
   tables (from pedal_dsp::fft::Fft<1024>), per-bin energy state, and notch
   filters. `process` is allocation-free and lock-free. Latency = 768 samples
   (sr × 0.016). Acceptance: `cargo test` green + unit tests for flat-sum (§7.1)
   and threshold sweep (§7.3).

2. **`t_*_eng_2 — metering, registry registration & headless smoke`**
   Register the block in `graph/src/blocks.rs::builtin_registry` (call
   `registry.add::<SmartNoiseGate>()` in `builtin_registry()`). Extend
   `engine/src/audio.rs` `Meters` with `reduction_db: f32` (§6); the smart noise
   gate writes its peak gain reduction each frame. Patch the block to snap all
   params by value at `process` entry and floor detector input at `1e-4` (§5).
   Add a headless audio-path smoke test feeding noise + tone through the gate.
   Acceptance: `cargo test` green; saved preset round-trips params; the block
   registers without panics.

### UI cards (assignee: `pedal-ui-builder`)

3. **`t_*_ui_1 — smart noise gate in the chain`**
   The engine adds its `BlockSpec` → the UI registry; `Chain.tsx` renders the
   gate row with the block icon, name, bypass toggle (amber), and a reduction
   meter chip. Updates `state.registry` automatically via `getState`. Acceptance:
   select the block in the chain; the parameter columns appear.

4. **`t_*_ui_2 — gate parameter editor`**
   `Editor.tsx` renders the gate's controls: Strength (knob, 0–100%), Threshold
   (knob, -80 to 0 dB), Release (knob, 0.1–10 s), Reduction (knob, -100 to 0 dB),
   Hum Freq (toggle: 50 / 60 Hz), Hum Notch (on/off toggle). Values are read from
   `params`, mirrored from engine preset. Acceptance: dragging any knob updates
   engine state and the val panel shows the new value.

5. **`t_*_ui_3 — reduction meter`**
   A small horizontal meter bar (0 to -60 dB, green → red) is rendered in the
   editor below the controls, updated from the `reduction_db` field in the
   `event: meters` payload. Acceptance: feed noise; the meter moves and the val
   panel matches the engine reading.

---

## Try it (product owner)

Run the app (see `docs/RUN-THE-APP.md`), open a project, drag **Smart Noise Gate**
into a channel, and:

1. Pick a noisy recording (DI guitar with mains hum or broadband noise).
2. Set **Threshold** to -50 dB and **Strength** to 75%. Listen — the noise floor
   should recede while the guitar tone remains clear.
3. Sweep **Threshold** lower to reduce more noise, higher to restore signal. The
   effect should be smooth, not gated or pumping.
4. Enable **Hum Notch** and toggle **Hum Freq** between 50 / 60 Hz. If there's
   60 Hz hum, it should be removed without affecting nearby notes.
5. Sweep **Release** from 0.1 s to 5 s. Slow release = smooth fade; fast release
   = more aggressive but can cause pumping on quiet passages.
6. Set **Strength** to 0% — the effect should be transparent (dry bypass).
7. Save a preset (`File → Save`) and reload to confirm params round-trip.

---

## Known limits (by design)

* **Fixed FFT size (1024):** bin resolution is ~21 Hz at 48 kHz. Low-frequency
  noise (e.g., 50 Hz) occupies 2–3 bins — sufficient for hum removal but not for
  sub-harmonic precision. The pre-filter notch handles hum directly.
* **No hum-frequency auto-detection:** the user selects 50 / 60 Hz manually.
  Auto-detection would require a long analysis window and state management that
  complicates the audio thread.
* **Attack is fixed (5 ms):** no user control. Fast attack is always appropriate
  for noise removal; this is an intentional simplification.
* **Peak detection only (no RMS mode):** ShutUp supports both Peak and RMS
  detection, but this implementation uses peak energy per bin only. RMS mode
  can be added in a future iteration.
* **Latency: ~16 ms at 48 kHz (768 samples):** the OLA half-window delay. This
  is acceptable for studio use but may be noticeable for live monitoring.
* **No spectral subtraction:** bins below threshold are attenuated, not zeroed.
  This avoids the "musical noise" artifacts of traditional spectral subtraction
  (Chowning/Smirnov), at the cost of less aggressive noise reduction.
