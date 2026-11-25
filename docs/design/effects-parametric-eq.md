# Parametric EQ block — design

Status: **design frozen** for card `t_33fc7d95` (feature). Committed at
`docs/design/effects-parametric-eq.md`. **No implementation code is added by this
card** — the implementation lives in the separate cards `t_22f4d514`
(engine) and `t_fbe32e4b` (UI), which are the only ones that may touch the Rust
or TypeScript sources. This doc is the contract both cards must implement
exactly.

## 1. What ships

One block in the graph engine, `tone.eq`, in the `Tone` category, audio in →
audio out. It is self-contained: it uses only existing `pedal_dsp::biquad`
primitives (`dsp/src/biquad.rs`, RBJ cookbook). It adds no new `pedal-dsp`
primitive and no allocation on the audio path (per `graph/src/block.rs`).

The design doc is the only new file for this card. The engine card registers the
block in `builtin_registry()`; the UI card renders the params from the registry.

## 2. Bands

Six fixed-type bands (no per-band type knob). Every band has an `enabled`
switch; peaking bands add a `q` knob; shelving and filter bands use the
standard RBJ slope (fixed Q):

| # | band id | type | extra knob |
|---|---|---|---|
| 1 | `low_shelf` | low shelf | — |
| 2 | `low_peak` | peaking | Q |
| 3 | `high_peak` | peaking | Q |
| 4 | `high_shelf` | high shelf | — |
| 5 | `high_pass` | high-pass | — |
| 6 | `low_pass` | low-pass | — |

All bands share one global output gain (`output_gain`, dB) applied after the
bands.

## 3. Parameters — ids, names, units, ranges, defaults

Follows the existing `ParamSpec` model (`graph/src/types.rs`): `Unit` is the fixed
set {dB, Hz, ms, %, st, cents, ratio, linear, choice}; `Curve` is {Linear, Log,
AmpGain}. Every parameter is a flat `ParamSpec`; the UI, the JSON loader and the
validator all read the same description. Real values (dB, Hz, …), never
normalised.

| id | name | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| `low_shelf_freq` | Low shelf freq | Hz | 20 | 200 | 100 | Log |
| `low_shelf_gain` | Low shelf gain | dB | −12 | 12 | 0 | AmpGain |
| `low_shelf_enabled` | Low shelf | — | 0 | 1 | 1 | Linear |
| `low_peak_freq` | Low peak freq | Hz | 100 | 2000 | 300 | Log |
| `low_peak_gain` | Low peak gain | dB | −12 | 12 | 0 | AmpGain |
| `low_peak_q` | Low peak Q | — | 0.35 | 3.0 | 1.0 | Linear |
| `low_peak_enabled` | Low peak | — | 0 | 1 | 1 | Linear |
| `high_peak_freq` | High peak freq | Hz | 1000 | 12000 | 3000 | Log |
| `high_peak_gain` | High peak gain | dB | −12 | 12 | 0 | AmpGain |
| `high_peak_q` | High peak Q | — | 0.35 | 3.0 | 1.0 | Linear |
| `high_peak_enabled` | High peak | — | 0 | 1 | 1 | Linear |
| `high_shelf_freq` | High shelf freq | Hz | 2000 | 20000 | 10000 | Log |
| `high_shelf_gain` | High shelf gain | dB | −12 | 12 | 0 | AmpGain |
| `high_shelf_enabled` | High shelf | — | 0 | 1 | 1 | Linear |
| `high_pass_freq` | High pass freq | Hz | 20 | 300 | 80 | Log |
| `high_pass_enabled` | High pass | — | 0 | 1 | 1 | Linear |
| `low_pass_freq` | Low pass freq | Hz | 1000 | 20000 | 12000 | Log |
| `low_pass_enabled` | Low pass | — | 0 | 1 | 1 | Linear |
| `output_gain` | Output gain | dB | −12 | 12 | 0 | AmpGain |

Fixed (non-user-adjustable) Q values: shelving bands 0.707 (RBJ standard
slope), high-pass 0.707, low-pass 0.54 (24 dB/oct). `enabled` is a binary real
value (0/1) — there is no bool type in the model; the engine card stores it as
`0.0`/`1.0` in the block's param map and the UI card renders it as a toggle.
`enabled` is clamped to [0, 1] inside `process`.

## 4. Biquad topology and coefficient formulas (RBJ cookbook)

The block cascades one `Biquad` (transposed direct form II, `dsp/src/biquad.rs`)
per enabled band, in fixed order `low_shelf → low_peak → high_peak → high_shelf →
high_pass → low_pass`, then applies `output_gain`:

```text
x ──► [low_shelf] ──► [low_peak] ──► [high_peak] ──► [high_shelf] ──► [high_pass] ──► [low_pass] ──► [output_gain] ──► y
```

Coefficient design uses the canonical RBJ cookbook, per band per block:

- `w0 = 2π·f0/fs`, `α = sin(w0)/(2Q)`, `A = 10^(gain_db/40)`.
- `LowShelf` / `HighShelf`: shelf gain `A`, Q fixed 0.707, `a = A` in the
  cookbook formulas (the code halves the dB to get `A = db_to_lin(gain_db·0.5)`).
- `Peak`: `b0 = 1 + αA`, `b1 = −2cos(w0)`, `b2 = 1 − αA`, `a0 = 1 + α/A`.
- `HighPass`: `b0 = (1+cos w0)/2`, `b1 = −(1+cos w0)`, `b2 = (1+cos w0)/2`,
  denominator `1 + α`.
- `LowPass`: `b0 = (1−cos w0)/2`, `b1 = 1−cos w0`, `b2 = (1−cos w0)/2`,
  denominator `1 + α`.

All biquads are normalised (`a0` removed) by `Biquad::new` and processed in
transposed DF2 with state `z1/z2` — this is the **zero-latency** topology:
no algorithmic delay enters the graph (`latency_samples()` stays 0).

### Sample-rate changes

Coefficients are re-computed **on every block** from `ctx.sample_rate`
(ToneStack pattern, `graph/src/blocks.rs:84-94`): `low_peak.design(...)`,
… the block never caches coefficients across blocks, so a host rate change is
correct at the next block with no state migration. `Biquad::make` clamps
`f0 ∈ [1 Hz, fs·0.499]` and `Q ≥ 0.01`, so a sliding knob can never push
`f0` past Nyquist (unstable). The design is unchanged at 44.1/48/96 kHz; only
the regular `Biquad::make` coefficient computation differs, as expected.

### Parameter-change click avoidance

1. **Per-block redesign with state preserved** (`z1/z2` carry over): at a block
   boundary only the coefficients change, so the step is limited to the filter's
   natural impulse decay — a fade, never a pop. This is what the existing
   `ToneStack` and `Cab` blocks already do.
2. **Enable/disable inserts an identity biquad** into the cascade at the band's
   position: `s → identity(s) = s`, so switching a band off/on produces no
   step. Band type is fixed per band, so no filter kind changes on the audio
   thread.
3. **Sliding frequency is clamped by `f0 ≤ fs·0.499`**, so the corner can never
   cross Nyquist while dragging.
4. **No coefficient crossfade in the hot path** (that would double the filter
   work and add latency). `CrossfadeBiquad` exists in `dsp/src/biquad.rs` for
   the rare cases where a fade is explicitly wanted. `bypass` is handled at the
   rig level (`graph`), not by the EQ block.

## 5. Parameter IDs and the engine/UI contract

- Effect id: `tone.eq`; display name `Parametric EQ`; category `Tone`
  (`Category::Display` → `"Tone"`); ports `in` (audio) → `out` (audio).
- The engine and UI card must agree on the exact 19-parameter table of §3:
  ids, names, units, min/max, default and curve, including the wire-format rules
  of §7. The engine card's registry is the single source of truth for the
  values; the UI card renders them 1:1.
- The `enabled` flag is a real 0/1 value (no bool type exists), stored in the
  block's param map and rendered by the UI as a toggle.
- Fixed Q values (shelves 0.707, HP 0.707, LP 0.54) are hardcoded in the block,
  not params.

## 6. Test plan

New file `graph/tests/eq.rs` (run with `cargo test -p dsp-graph --test eq`).
All tests are host-only, offline renders — never routed to sound hardware:

1. **Registry** — `tone.eq` present with name `Parametric EQ`, category `Tone`,
   ports `in`/`out` audio; the 19 params match §3 exactly (id, name, unit,
   min, max, default, curve).
2. **Unity at 0 dB** — all params at default (0 dB gain, bands enabled at
   default f0, `output_gain` 0): the output equals the input
   sample-exact (`abs difference < 1e-6`) for a full-scale sine and for white noise.
3. **Frequency response at key points** — for each band, enable only that band,
   run a 10 s offline render at 48 kHz (64-frame blocks) of `io.input →
   tone.eq → io.output`, and measure output level at `f0` and ±1 octave against
   the RBJ formulas to ±0.2 dB: peaking and shelving gain at `f0`; shelves at
   < f0 and > f0; HP/LP at −3 dB at `f0`.
4. **Q effect** — at `f0` with +6 dB gain, Q 0.35 vs 3.0 widens the −3 dB
   bandwidth by the expected ratio (~4.9× for a peaking filter in the
   low-Q regime).
5. **Stability at extremes** — `f0` at 20 Hz and at 20 kHz (48 kHz host),
   `Q` 0.35 and 3.0, gain ±12 dB: output finite (no NaN), bounded, and a dirty
   impulse decays to zero within a few ms and stays inside ±0.01.
6. **Enable/disable continuity** — switching a band off then on at a block
   boundary produces no step: the output at the boundary differs from the
   pre-switch value by < 1e-4.
7. **Sample-rate change** — the same block compiled at 44.1 kHz and 48 kHz
   yields coefficients that differ only where `Biquad::make` requires
   (`f0` clamped to `fs·0.499`), and the rendered response follows the
   corresponding RBJ formula (verified with `Biquad::gain_db_at` at a few
   points).
8. **Output gain** — `output_gain` +6 dB raises the unity-response level by
   6 dB; −6 dB lowers it by 6 dB (measured on a swept sine).

## 7. Serialization and the wire contract (engine ↔ UI)

- Registry snapshot (`engine/src/server.rs`): `registry` is a `BTreeMap<id,
  {name, category, params:[{id, name, kind, min, max, default, unit,
  choices?}]>}`.
- `unit` on the wire is `Unit::to_string()`: `"dB"`, `"Hz"`, `"ms"`, `"%"`,
  `"st"`, `"cents"`, `"x"`, or `""` for `Linear` and `Choice`.
- `kind` is `"choice"` + `choices` for `Unit::Choice`, else `"number"`.
  **No `curve` field is sent** — the UI derives it from `unit`:
  `Hz`, `ms` → Log; `dB` → AmpGain; `Percent`, `Ratio`, `Linear`, `Cents`,
  `Semitones` → Linear.
- `category` on the wire is `Category::Display`: `"I/O"`, `"Split/Merge"`,
  `"Dynamics"`, `"Tone"`, `"Distortion"`, `"Modulation"`, `"Pitch"`, `"Time"`,
  `"Modelling"`, `"Utility"`, `"Control"`. EQ is `"Tone"`.
- `channels[].blocks[].params` are **real values** (dB, Hz, …), never
  normalised (the engine's `Executor` and the JSON loader both store real
  values; `setParam` over IPC is real too).
- The `enabled` param is `0.0`/`1.0` in the rig's param map; the UI renders it
  as a toggle.

## 8. Out of scope (deliberately, per acceptance)

- No coefficient crossfade in the hot path.
- No per-band type selection (types are fixed by band id).
- No stereo/multi-voice EQ; one mono band chain.
- No UI change beyond the effect panel itself (the UI card `t_fbe32e4b` owns
  the rendering).
- No implementation code in this card — see §1 and the child cards.
