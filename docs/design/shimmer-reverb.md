# Shimmer Reverb block — design

Status: **design (owner: t_89b06f04)**. Draft for builders. No code changed yet.
This spec is the owner of record: the engine card and the UI card must implement
exactly the parameter IDs, ranges, defaults and units below, nothing more.

---

## 1. Sound and concept

A **shimmer reverb**: a reverb tank whose **feedback path** contains a pitch
shifter. Each repeat the wet signal returns through is pitched by the chosen
interval, so the tail climbs in pitch by that interval per cycle — a classic
"snowflake" colour built on top of a normal (or long) decay.

* The reverb tank is stereo, Schroeder/Freeverb-style (parallel combs + series
  allpasses), with a low-pass in the loop.
* The **Shimmer** knob is the gain of the pitch-shifted signal fed back into
  the tank. At 0 the block is a normal reverb; at 100 the feedback is entirely
  pitch-shifted (the loop still stays stable because the pitch shifter is a
  norm-approximating relay and the loop gain is clamped/damped).
* **Interval** is a discrete choice of static ratios: `+12 st` (octave up,
  default), `+7 st` (fifth up), `−12 st` (down an octave). These are the three
  shimmer positions from the shimmer-reverb family. (Continuous sweep is a
  separate, later card — it belongs to the pitch-delay only.)
* **Tone (damping)** is a high-frequency rolloff applied *inside the feedback
  loop*, which thins the shimmer colour and prevents harsh high-frequency
  build-up.

### Signal flow

```text
input ──► ShimmerTank (stereo FDN) ──► wet ──► Mix(dry, wet, mix) ──► out
            │
            └─► pitch shift (interval ratio) ──► Shimmer gain ──►
                  │                                      │
                  └────── feedback loop gain limiter ────┘
```

### Stability (feedback-loop gain limiting)

The pitch shifter is a **fixed-ratio resampler**, so it is a norm-preserving
relay with no net gain; the loop has exactly one gain stage:
`Shimmer` (linear 0..0.95). Stability is guaranteed by three independent
measures:

1. **`Shimmer` (feedback gain) is clamped to 0.95** — never more.
2. **`Tone` (damping)** applies a low-pass inside the loop, bounding
   high-frequency energy that the pitch shifter can fold back.
3. **An explicit energy guard** computes the wet RMS over the block and, if the
   wet would exceed a fixed cap, scales the feedback contribution down for
   that block. This is the last line of defence and is what makes the block
   provably non-divergent even at extreme settings.

---

## 2. Parameters

All parameters are declared exactly like the existing blocks (see
`graph/src/blocks.rs` `ParamSpec` / `Unit` / `Curve`). Defaults are the block's
`Default` values. Values stored in presets are the **real** numbers (dB, ms,
seconds, %), not normalised.

| id | name | unit | min | max | default | curve | smoothing |
|---|---|---|---|---|---|---|---|
| `mix` | Mix | Percent (%) | 0 | 100 | 40 | Linear | τ 60 ms |
| `size` | Size | Percent (%) | 0 | 100 | 70 | Linear | τ 60 ms |
| `decay` | Decay | Seconds | 0.2 | 8.0 | 2.5 | Linear | τ 60 ms |
| `shimmer` | Shimmer | Percent (%) | 0 | 100 | 55 | Linear | τ 60 ms |
| `interval` | Interval | Choice (3) | 0 | 2 | 0 | Linear | off (instant) |
| `tone` | Tone | Percent (%) | 0 | 100 | 50 | Linear | τ 60 ms |
| `pre_delay` | Pre-delay | Milliseconds (ms) | 0 | 80 | 0 | Linear | τ 60 ms |

Choice labels (index → ratio):

| index | label | ratio `r = 2^(interval/12)` |
|---|---|---|
| 0 | `+12 st up` | 2.0 |
| 1 | `+7 st up` | ≈1.4983 |
| 2 | `−12 st down` | 0.5 |

**Semantics of the knobs** (affects are cumulative, applied in this order):

1. `pre_delay` — fractional delay line at the tank input, in ms.
2. `size` — room size 0..1 (maps to the tank's base comb feedback gain, master).
3. `decay` — target decay time in seconds (0.2–8.0 s); the tank's loop gain is
   reduced so the physical decay approaches this value.
4. `tone` — high-frequency damping applied *inside* the feedback loop.
5. `shimmer` — linear gain of the pitch-shifted wet fed back.
6. `mix` — dry/wet balance, `out = dry*(1-m) + wet*m` with `m = mix/100`.

**Important equivalences** (mirror the family):

* `mix = 0` → **exact bypass**: output equals input sample-for-sample, for
  *any* value of every other knob. The block never introduces gain or NaN at
  mix 0.
* `shimmer = 0` → the block is an ordinary reverb (no pitch in the loop);
  the tank still produces a tail.
* `size = 0` and `decay = 0.2` → the tank is essentially dead (only direct
  signal through `mix`), which is how a silent bypass is represented.

**Smoothing rule**: continuous parameters use a per-sample exponential moving
average with τ = 60 ms, applied in the audio thread to the *real* value before
it is fed to the tank. `interval` is a discrete choice and is applied
instantly (no smoothing). Smoothing is optional on any param — a builder may
disable τ on `shimmer` and `tone` if the 60 ms tail sounds muddy, as long as
the value is still written with a fast path; the test suite pins `mix=0`
bypass exactly (bypass is never smoothed into silence).

---

## 3. Engine implementation

### 3.1 Block spec

Add a new `Block` in `graph/src/blocks.rs` (new struct `ShimmerReverb`):

* `id`: `"time.shimmer"`
* `name`: `"Shimmer"`
* `category`: `Category::Time` (the picker groups by category; **Time** comes
  after **Dynamics**, before **Modulation** — see `BlockList.tsx`)
* ports: `in` (audio) → `out` (audio)
* params: exactly the 7 rows in §2 above, `ParamSpec{id,name,unit,min,max,default,curve}`.
  Units are the existing `Unit` variants (`Percent`, `Seconds`, `Milliseconds`,
  `Choice(&[&str])`); curves are `Linear`.

### 3.2 Registration

One line in `builtin_registry()`:

```rust
r.add::<ShimmerReverb>();
```

The registry is built once (`Arc<Registry>`, `builtin_registry()`, called from
`engine/src/audio.rs:511`) and shared read-only. The UI receives the full
registry snapshot on every connect and on every `setParam`/`rebuild`, so **no
new UI code is required** — the block appears in the picker under Time with
all 7 controls rendered automatically.

### 3.3 Preset serialization

* The engine's `Rig::to_doc()` stores each block instance as a `NodeDoc` with
  `params: BTreeMap<String, f32>` keyed by param **id** (real values).
* `Doc::to_json()` serialises the whole graph, so a `ShimmerReverb` block and
  all its params are written to `presets/*.json` automatically. No hand-written
  encode/decode.
* `Doc::build_in()` (engine `server.rs::compile`/`rebuild`) reads back params by
  id and validates against `spec.param_index` → an unknown param is an error
  (pinned by the tests), missing params fall back to `spec.default`.
* The UI sends `setParam({channel, block, param, value})` with the **real**
  value; `server.rs::set_param` updates `NodeDoc.params` and pushes
  `Cmd::SetParam { node, param, value }` to the audio thread.
* A preset round-trips with the block's id and every param untouched.

### 3.4 Implementation pointers

* Wrap the existing `pedal_dsp` building blocks — do not invent new DSP:
  * `pedal_dsp::reverb` (comb/allpass), `pedal_dsp::delay::FractionalDelay`
    (pre-delay), `pedal_dsp::resample` (pitch shifter).
* The pitch shifter is a **fixed-ratio resampler** (`ratio = 2^(interval/12)`),
  applied per block to the wet signal that is fed back. It is a pure relay, so
  it can live inside the feedback loop without a phantom gain.
* Pre-delay is a fractional delay line at the tank input.
* The feedback loop gain is written once per block with a fast clamp (0.95) and
  the per-block RMS guard. No allocation, no mutex on the audio path.

---

## 4. UI implementation

The UI needs **no new components**. The block picker and the editor both read
the registry snapshot (`state.registry` in `ui/src/state.ts`) and render from
it:

* **Block picker** (`ui/src/components/BlockList.tsx`): `groupBlocks` groups the
  registry by category in `CATEGORY_ORDER` and includes **every** block spec.
  Adding `time.shimmer` (with `category: Category::Time`) automatically puts it
  under **Time**. Its default config is taken straight from the spec
  (`defaultConfig`), so the picker shows the correct defaults.
* **Editor** (`ui/src/components/Editor.tsx`):
  * parameters are split into `knobs` (numbers) and `choices` (the `interval`
    segmented row / dropdown).
  * every knob is rendered from `(id, name, min, max, default, unit, curve)` —
    all 7 params now render with no hand-written JSX.
  * `setParam` is the single command the engine consumes; the UI sends the
    **real** value.

### What must not move or change

* The existing block types, their ids, categories and units are unaffected by
  this addition.
* `Electron`/theme tokens (`--ui-*`) are untouched; the shimmer block reuses
  the same knobs/choice controls as `time.delay` and `time.reverb`.

---

## 5. Tests

### Engine tests (`graph/tests/shimmer.rs` — run with
`cargo test -p dsp-graph --test shimmer`)

1. **registry** — `time.shimmer` present: name `Shimmer`, category `Time`,
   audio in/out, params match the §2 table exactly (ids, ranges, defaults,
   units, curves).
2. **default state** — a fresh block is prepared at 48 kHz and its params are
   within 1e-6 of the spec defaults.
3. **interval ratios** — `interval` 0/1/2 produce ratios ≈2.0 / ≈1.4983 / 0.5.
   (Measured by the closed-loop pitch of an impulse echo; pitch of each echo is
   `dry + interval × n` for the nth echo.)
4. **silence in → silence out** — silence input for 1000 samples gives output
   with RMS < 1e-9 at every other param.
5. **mix=0 bypass** — `mix=0` returns the input unchanged sample-exact for the
   full param range (tests 1–4 run with `mix=0` to pin this).
6. **no NaN** — process a 10 s ramp at 48 kHz with every param at max; assert
   no NaN/inf in output and peak bounded (stability).
7. **max stability** — `size`=max, `decay`=8.0 s, `shimmer`=max, `tone`=0:
   impulse in → bounded output (peak < 2.0), no NaN, ring decays to silence.
8. **interval choice clamp** — out-of-range `interval` value is clamped to 0..2.
9. **pre-delay** — a `pre_delay` of 40 ms shifts the start of the wet tail by
   ≈40 ms (measured vs. an impulse response peak).
10. **preset round-trip** — build a `Doc` with one `ShimmerReverb` node at
    non-default params, `to_json` → `from_json` → rebuild; every param is
    unchanged (and an unknown param would be rejected, not silently dropped).

### UI tests (run with `pnpm`)

1. **`cd ui && pnpm run build`** — typechecks and builds with the new block
   present (no new components to compile).
2. **picker** — the "Shimmer" block appears in the add-block picker under the
   **Time** category.
3. **defaults** — the picker row's default config equals the spec table.
4. **editor** — selecting the block renders all 7 controls: 6 knobs, one
   segmented `interval` choice with 3 entries.
5. **setParam wire** — changing a knob sends `setParam({channel, block,
   param: <id>, value})` with the *real* value.

### Manual / product-owner "Try it"

1. Start the app (`npm run dev` or the Tauri bundle).
2. Build a chain: `Time → Shimmer`.
3. Drag `Size`/`Decay` up: the reverb tail grows.
4. Drag `Shimmer` up: the tail's repeats climb in pitch (each echo is an
   octave — or fifth/down-octave — higher than the last).
5. Set `Shimmer` = 0: the block is now a plain reverb.
6. Set `Mix` = 0: the block is silent — **exact bypass**.
7. Save a preset with the Shimmer block at a custom setting; reload the preset;
   the knob positions are restored.

---

## 6. Out of scope

* Continuous pitch sweep (cents/sec, "never settles") and Shepard ramps — these
  belong to the pitch-delay/Rainbow Machine effort, not this card.
* Stereo/parallel shimmer, per-note pitch, or modulation of the interval.
* MIDI learn, CV/ECL via the plugin shell, or firmware port (Pi 5 later).
* A `Shimmer` category — the block is `Time`.

---

### Decisions recorded (these are fixed for the builders)

| # | Decision | Rationale |
|---|---|---|
| 1 | Block id `time.shimmer`, category `Time` | aligns with `time.reverb`/`time.pitchdelay` |
| 2 | Interval is a 3-way choice (not a continuous knob) | matches the shimmer family's static-position design; continuous sweep is a separate card |
| 3 | Stability = clamp 0.95 + tone damping + per-block RMS guard | provable non-divergence with no extra DSP |
| 4 | Resampler pitch shifter in the feedback loop | pure relay, no phantom gain, no STFT latency |
| 5 | Units chosen from the existing `Unit` enum | the UI renders units from the spec, no new text |
| 6 | Presets store real values keyed by param id | `Doc`/`serde` already does this; nothing new required |
