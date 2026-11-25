# Pitch whammy block — design

Status: **designed** (architect card `t_02019ec2` (feature)). This document freezes the block, its parameters, the DSP approach, the engine registration, the UI contract, and the tests so the builder cards can be implemented without re-litigating any design decision.

Related: `docs/ARCHITECTURE.md`, `docs/IPC-PROTOCOL.md`, `dsp/src/delay.rs` (`PitchShifter`, `FeedbackDelay`, `PitchMode`), `graph/src/blocks.rs` (block conventions), `docs/design/tremolo.md` and `docs/design/chorus.md` (the sibling effect designs).

## 1. What ships

One new block in the **Pitch** category. It is a self-contained integration of the existing `pedal-dsp` primitives — no new DSP primitive is added:

| | |
|---|---|
| id | `time.pitchwhammy` |
| name | `Pitch Whammy` |
| category | `Pitch` |
| ports | `in` (audio) → `out` (audio) |
| primitive | `pedal_dsp::delay::FeedbackDelay<60000, 2400>` (`dsp/src/delay.rs`) |

The block holds exactly one `FeedbackDelay<60000, 2400>` — the same object `time.pitchdelay` uses — plus its own dry/wet `mix`. The delay line is 60000 samples (max 1250 ms at 48 kHz); the pitch shifter inside it is `PitchShifter<2400>` (2400-sample analysis window). Feedback is taken from the `FeedbackDelay` `feedback` field, so every repeat is shifted again and the pitch climbs — the Rainbow-Machine character the pedal is targetting.

## 2. Parameters

The full parameter table. Every parameter is a `ParamSpec` with `id`, `name`, `unit`, `min`, `max`, `default`, `curve` (`Linear`, `Log` or `AmpGain`), and the `controls_in`/`controls_out` vectors are empty. Parameter values are stored **normalised** (0..1) by the graph and **denormalised** to real values by the executor before the block reads them — the block always sees real values (semitones, cents/sec, ms, %).

| id | name | unit | min | max | default | curve | wire `kind` | wire `unit` | effect |
|---|---|---|---|---|---|---|---|---|---|
| `pitch` | Pitch | Semitones | -24.0 | 12.0 | 0.0 | Linear | number | st | Set point of the shift; the footswitch/expression sweeps around it. -24 st = 3 octaves down, +12 st = 2 octaves up. |
| `rate` | Sweep rate | Cents/sec | -480.0 | 480.0 | 120.0 | Linear | number | cents/s | Speed of a continuous sweep. Negative = down, positive = up. |
| `mode` | Mode | Choice | 0.0 | 2.0 | 0.0 | Linear | choice | — | `["Classic","Sweep","Harmony"]` |
| `glide` | Glide | Milliseconds | 0.0 | 3000.0 | 80.0 | Linear | number | ms | Time to return to the set point after the footswitch is released (Classic). |
| `feedback` | Feedback | Percent | 0.0 | 95.0 | 60.0 | Linear | number | % | Feedback into the delay line; repeats climb in pitch. 0 = plain pitch shift. |
| `mix` | Mix | Percent | 0.0 | 100.0 | 50.0 | Linear | number | % | Dry/wet blend of the shifted signal against the dry path. |

### Mapping into the primitive

```text
d.set_delay_ms(380.0)          // fixed 380 ms delay, not a knob (see §4)
d.set_feedback(feedback / 100.0)
d.set_pitch_cents(pitch * 100.0)      // cents; ±24 st → ±2400 cents
d.set_pitch_rate(rate)                // cents per second
d.set_pitch_glide_ms(glide)           // portamento, ms
d.set_pitch_mode(mode)                  // Classic -> Target, Sweep -> Sweep; Harmony voices three chord offsets over the same feedback loop
d.set_sweep_gate(gated)                 // footswitch / expression position
out[i] = mix(input[i], d.process(input[i]), mix / 100.0)
```

### Semantics

**`pitch` (set point).** The value the player tunes the effect to. Negative = down, positive = up. In **Classic** and **Harmony** modes the set point is the anchor the footswitch/expression sweeps around and returns to. In **Sweep** mode it is the centre of the range: the shift stays inside ±24 st and the footswitch picks the direction.

**`rate`.** Sweep speed in cents per second, continuous and never settling. Mode-dependent behaviour:

- **Sweep** (1): the only pitch control that matters. The pitch climbs (or falls) at `rate` and never returns to a target.
- **Classic** (0): the footswitch gates the climb — the pitch ramps up at `rate` while the footswitch is down, then returns to the set point.
- **Harmony** (2): not used; the footswitch is a sustain gate, and the chord is fixed at root +300 cents +700 cents.

**`mode`.** Three labelled behaviours:

- **Classic** (0): the footswitch is a position control. Down = pitch sweeps up away from the set point at `rate`; up = pitch glides back to the set point within `glide` ms. This is the page-3 "whammy" feel: a fast up-sweep and a smooth fall-back.
- **Sweep** (1): continuous. The pitch climbs (or falls) at `rate` and never settles — exactly the "continuous sweep rate (cents/sec) that never settles" the product wants. The footswitch is a direction gate: down = climb, up = fall. A fresh press re-triggers in the new direction.
- **Harmony** (2): the `pitch` knob selects the root offset; the effect voices the root plus a fixed chord (root +300 cents, root +700 cents) and mixes them over the delay. The footswitch is a sustain gate: down = chord voiced, up = root only. No continuous sweep in this mode.

**`glide`.** Portamento time toward the set point, 0 = instant, 3000 ms = slow. Used only on release in Classic and on the return-to-centre of the expression pedal in Sweep/Harmony.

**`feedback`.** Percentage of the wet signal fed back into the delay line. 0 = no accumulation (single shifted repeat). 60 = the Rainbow-Machine climb. The `FeedbackDelay` clamps to 0.99 so a runaway never saturates.

**`mix`.** Dry/wet blend, 0 = dry pass-through, 100 = fully wet. Matches the `time.pitchdelay` and `mod.chorus` convention.

### Parameter smoothing (zipper noise)

No parameter ever jumps. The `pedal_dsp::delay::PitchShifter` advances the working pitch by a **maximum one-sample step per sample** (`pitch_step = (pitch_cents − pitch_current) / samples`), so a change in `pitch`, `rate` or `glide` is always ramped over `glide` ms and never produces a step in the audio. The block must apply parameter changes by calling the `set_pitch_*` family (not by rewriting `pitch_cents` directly mid-block) so the ramp is honoured. `feedback` and `mix` are feed-forward gains and are applied per-sample; they are clamped, not smoothed.

## 3. Engine registration

One line in `builtin_registry()` in `graph/src/blocks.rs`:

```rust
r.add::<PitchWhammy>();
```

That is the whole "make it available to users" step. The engine serves `registry_json(&Registry)` over IPC, and the UI builds the rack and the add-block picker from that snapshot.

### Parameter IDs exposed to the UI

The IPC state snapshot carries `registry`, whose `time.pitchwhammy` entry is the exact `BlockSpec`:

```jsonc
"time.pitchwhammy": {
  "name": "Pitch Whammy",
  "category": "Pitch",
  "params": [
    { "id": "pitch",    "name": "Pitch",  "kind": "number", "min": -24, "max": 12, "default": 0, "unit": "st" },
    { "id": "rate",     "name": "Sweep rate", "kind": "number", "min": -480, "max": 480, "default": 120, "unit": "cents/s" },
    { "id": "mode",     "name": "Mode", "kind": "choice", "min": 0, "max": 2, "default": 0, "unit": "", "choices": ["Classic", "Sweep", "Harmony"] },
    { "id": "glide",    "name": "Glide", "kind": "number", "min": 0, "max": 3000, "default": 80, "unit": "ms" },
    { "id": "feedback", "name": "Feedback", "kind": "number", "min": 0, "max": 95, "default": 60, "unit": "%" },
    { "id": "mix",      "name": "Mix", "kind": "number", "min": 0, "max": 100, "default": 50, "unit": "%" }
  ]
}
```

No new IPC command and no preset-schema change. Parameter values ride on the node in the preset `Doc` JSON, exactly like `time.pitchdelay` (`"pitch"`, `"rate"`, `"mode"`, `"mix"`).

## 4. DSP approach and trade-offs

### Chosen approach: dual delay-line, crossfaded shifter

The effect is a **delay-line pitch shifter with two read heads half a buffer apart**, crossfaded with constant power. This is `pedal_dsp::delay::PitchShifter<2400>` from `dsp/src/delay.rs`.

Why this, over the alternatives already in the crate:

| Approach | Location | Trade-off |
|---|---|---|
| **Dual delay-line crossfaded** (chosen) | `PitchShifter<2400>` | Cheap, sample-continuous, no spectral aliasing; the constant-power crossfade (`cos`/`sin` weights) keeps the loop power constant so the repeats do not ring-modulate. |
| Time-stretch + resample (phase vocoder) | `pedal_dsp::pitch_vocoder` | Bonus: no aliasing, but `PITCH_VOCODER_SIZE` 1024 at 48 kHz = ~11 ms group delay and ~50 KB per stack; heavier for a per-block instance. |
| Bin remap | `pedal_dsp::pitch_remap` | Cheapest, but aliases badly at the extremes (220 Hz → 440 Hz produces a strong 252 Hz image). Rejected for a whammy where the extremes are the selling point. |

The delay-line shifter's known weakness is also documented and accepted: at ±24 st the shifted spectrum is reconstructed from a 2400-sample window, so very high shift ratios colour the tone (a "swirly" upper midrange). The user's range of −24..+12 st is exactly where this appears; the spec therefore fixes the default to 0 and the UI labels the extreme of the knob as "full up".

### Latency

The shifter's two read heads sit between `N/2` and `N` samples behind the write head, so the algorithmic latency is time-varying; `latency_samples()` returns the midpoint, `N/2` = 1200 samples ≈ 25 ms at 48 kHz. The `FeedbackDelay` compensates by subtracting `pitch.latency_samples()` from the effective delay when any pitch state is active, so the **perceived delay time does not move as the pitch changes**. This is intentional, undeclared latency (the delay's own time is the sound): the executor's latency pass leaves the branch alone, the same as `time.delay` and `time.pitchdelay`.

### Buffer sizes

| Buffer | Size | Reason |
|---|---|---|
| Pitch shifter window `N` | 2400 samples (≈50 ms at 48 kHz) | Narrow enough to sound "pitch-shifted" rather than "flanged"; the window is the analysis frame of the shifter. |
| Delay line `N` | 60000 samples (1250 ms at 48 kHz) | Longest repeat a player expects before the pitch climbs out of the audible range. |
|| Stack | ~2400 + 60000 floats = ~336 KB | Fits the pedal's ~120 KB of audio-thread-appropriate RAM per instance. The `time.pitchdelay` block in `graph/src/blocks.rs` boxes the same `FeedbackDelay<60000, 2400>` for the same reason: the primitive's `process` needs `&mut self`, so it cannot be a stack object. Our block owns exactly one instance.

### Artifact trade-offs (accepted)

- **Extreme shift colouration.** At +12 st and +24 st the reconstruction from a 2400-sample window is not transparent; the top of the knob is labelled as such.
- **Stereo.** The block is mono (one buffer, one shifter), like every other block in the graph and the rig. Stereo widening is out of scope.
- **Phase of the shift.** The shifter preserves timing (a 5 Hz envelope comes back at 5 Hz), which is what a whammy should do; unlike a phase vocoder.

## 5. UI contract

### Controls (editor)

| Control | Type | Range | Default |
|---|---|---|---|
| Pitch | rotary knob | -24..+12 st | 0 |
| Sweep rate | rotary knob | -480..+480 cents/s | 120 |
| Mode | segmented button | Classic / Sweep / Harmony | Classic |
| Glide | rotary knob | 0..3000 ms | 80 |
| Feedback | rotary knob | 0..95 % | 60 |
| Mix | rotary knob | 0..100 % | 50 |

### Expression/position control

- **Footswitch**: a physical footswitch (the pedal's `PC13` ready input) drives the Classic/Sweep gate — down = sweep position, up = return. The UI mirrors this with a "Footswitch" button that engages/desengages the whammy mode.
- **Expression pedal** (heel/toe): when the whammy is engaged, moving the expression pedal (TRRS tip = wiper, one end to 3V3, the other to GND, 10 kΩ, 100 nF across) produces a **continuous position-controlled shift** — the pitch moves up or down at `rate`, and releases back toward the set point. It is a second position control, wired to `sweep_gate` and `rate` by the engine card. It must never produce a step: the expression value is filtered by the same `PitchShifter` glide.

### Layout and states

- Controls sit in one row in the block editor: Pitch, Rate, Mode, Glide, Feedback, Mix, followed by the Footswitch button. The expression pedal control is a full-width slider below the row (it needs more track than a knob).
- **~800 px wide**: the row compresses to the biggest compact set — Pitch, Rate, Mode, Glide; Feedback and Mix items shrink to icons. The expression slider keeps its full width below the row. Nothing clips; the block card does not grow past the editor canvas.
- **~1440 px wide**: the row is fully expanded with all six knobs at their readable sizes (knob face ≥ 34 px, value readout beside it). Footswitch button ≥ 44 px tall.
- **States**: hover and active on every knob (amber), pressed on the footswitch button, disabled while the effect is bypassed, and the mode button shows the active segment. The block card does not change its position or size in the chain when any control changes.
- **Consistency**: the knobs use the existing `Knob` geometry (ring radius 30, 270° arc, end labels `min`/`max` without unit, value display with unit). The expression slider reuses the `HeaderKnob` range styling. The mode button uses the category cyan (`--cat-h: 186`) accent so the Pitch category reads as a group.

## 6. Test plan and acceptance criteria

### Engine card (`pedal-builder`)

- **Registry**: `time.pitchwhammy` present with name `Pitch Whammy`, category `Pitch`, the six params above, audio in/out ports.
- **Clamp**: `pitch` clamped to [−24, +12] st; `rate` to ±480 cents/s; `glide` to 0..3000 ms; `feedback` to 0..95 %; `mix` to 0..100 %.
- **Smoothing**: a 1000-sample render with `pitch` stepped from −12 st to +12 st; the output must be finite and monotone in pitch — no sample-to-sample jumps beyond one sample of latency. A unit test asserts the maximum |Δ(pitch)| per sample ≤ 1.0.
- **Classic mode**: footswitch-down ramps pitch up at `rate`; release glides back to the set point within `glide` ms; the output is finite and has no clicks.
- **Sweep mode**: with the footswitch down, pitch climbs monotonically at `rate` c/s and never returns to the set point within a 5-second render (the "never settles" requirement).
- **Harmony mode**: three voices at root, +300, +700 cents are present in the spectrum; the chord ratio is stable at 60 dB SNR over the render.
- **Feedback**: at `feedback` = 60 %, the first repeat's energy is ≈ 60 % of the dry and the repeat level decays within a finite bound (no runaway). Two unit tests lock the behaviour: `feedback_levels_energy_ratio` (first repeat ≈ 60 % of dry at 60 % feedback) and `feedback_raises_no_runaway` (finite output with feedback at the maximum).
- **Persistence**: a `Doc` JSON carrying the six params round-trips (`from_json` → `to_json` → `from_json`) and compiles and runs finite.
- **Lane**: — (test outcome reported in the card's metadata; `cargo test --workspace --exclude pedal-shell` stays green).

### UI card (`pedal-ui-builder`)

- The add-block picker offers `time.pitchwhammy` under the **Pitch** category with the six controls above and the correct ranges/offsets (no rework needed by the picker's generic code).
- The block editor renders all six controls; the `mode` picker shows three options with the exact labels; the footswitch button toggles the gate.
- The expression pedal slider completes a full position sweep at `rate` and returns to the set point on release, at 20 Hz and 400 Hz sine inputs.
- At ~800 px and ~1440 px the control row does not clip; the block card's position is unchanged.

## 7. Out of scope

- No per-block glyph (Pitch is the archetype of the Pitch category; inherit `BLOCK_GLYPHS` from the category).
- No stereo widening, no multi-voice expansion beyond the three-voice Harmony chord, no MIDI learn, no per-block icon.
- No changes to the shared effect framework, the registry mechanism, or the preset format.
- No footswitch debouncing in the UI: the engine applies the gate per-sample and the hardware layer debounces.

## 8. Known limits (accepted, not fixed)

- The 2400-sample window colours the extreme shifts; the knob is labelled so.
- The block is mono; the rig is mono per channel.
- `FeedbackDelay<60000, 2400>` allocates on the heap; a pedal with more than one whammy in a rig would exceed audio-thread RAM. (Multiple whammies in one rig are out of scope.)
