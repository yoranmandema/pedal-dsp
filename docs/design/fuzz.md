# Fuzz effect — design spec

**Status:** design (not implemented). Owner: `pedal-architect`. Target: feature
card + engine card + UI card.

Scope: a single graph block `fuzz.fuzz` in the **drive** category, with a
`model` selector that chooses among three circuit-derived variants —
Fuzz Face, Big Muff, Tone Bender / FZ-1. All three share one block, one
parameter set, and one DSP path (input HP → per-variant waveshaper →
passive tone stack → output LP), with the variant differences isolated in
the waveshaper functions inside a new `pedal_dsp::fuzz` processor.

---

## 1. What ships

One new block in the **drive** category:

| | |
|---|---|
| id | `fuzz.fuzz` |
| name | `Fuzz` |
| category | `Drive` |
| ports | `in` (audio) → `out` (audio) |

It wraps a new `pedal_dsp::fuzz::Fuzz` processor (mono, no_std, zero alloc)
that runs the per-variant waveshaper at 4× oversampled rate, then the shared
passive tone stack as RBJ biquads, with DC blocking at in and out. The graph
block maps a small parameter set and the `model` selector to the processor.

The effect is **mono per channel** (like every other block in the graph;
stereo is introduced after the amp, so a mono fuzz sits cleanly in a mono
slot).

---

## 2. Parameters

| id | name | kind | unit | min | max | default | taper |
|---|---|---|---|---|---|---|---|
| `model` | Model | choice | choice | 0 | 2 | 0 | linear |
| `drive` | Drive | number | linear | 0.1 | 10 | 2.5 | log |
| `bass` | Bass | number | dB | −12 | +12 | 0 | ampGain |
| `mid` | Mid | number | dB | −12 | +12 | 0 | ampGain |
| `treble` | Treble | number | dB | −12 | +12 | 0 | ampGain |
| `level` | Level | number | dB | −40 | +12 | −6 | ampGain |

- **`model`** selects: 0 = Fuzz Face, 1 = Big Muff, 2 = Tone Bender.
  The choice maps to a `FuzzModel` enum in `dsp/fuzz.rs`; the UI renders it
  as a segmented row, exactly like `amp.basic`'s `voice`.
- **`drive`** is the input gain into the first clipping stage. It is the
  main "fuzz" control: higher = more haze, more even harmonics, more
  "fuzz".
- **`bass` / `mid` / `treble`** are dB gains on the passive tone stack.
  In a real pedal the tone stack is passive (no knob power); these are
  active approximations of the Big Muff's mid-scoop and the Fuzz Face's /
  Tone Bender's tone shaping.
- **`level`** is the output level after the soft limiter.

The parameter table rides on the node as it does for every other block, so
no protocol change, no preset-schema change.

---

## 3. Shared signal path

```text
guitar ──► input high-pass (20 Hz) ──► per-variant waveshaper ──► tone stack
                                                                      │
                                                                 (4× oversampled)
                                                                      │
                                                                  output LP ──► soft limiter ──► out
```

1. **Input high-pass (20 Hz, 2nd order, Q = 0.707)** — removes DC so the
   transistor pair and the codec never see a DC component. Shared by all
   variants.
2. **Per-variant waveshaper** — the nonlinear core. See §5.
3. **Passive tone stack (3 RBJ biquads: bass shelf 200 Hz, mid peak 1 kHz,
   treble shelf 4 kHz, Q = 0.7)** — the Big Muff's mid-scoop signature, and
   the tone-shaping approximation for the other two variants. See §5 for
   per-variant tone assignments.
4. **Output low-pass (gentle 4 kHz high shelf)** — gives the fuzz its
   characteristic "rolled off" top end and guarantees the soft limiter sees
   a band-limited signal.
5. **Output soft limiter (cubic)** — the same `soft_limit()` used in
   `EffectChain`: unity slope at 0, reaches ±1 with zero slope, so the knee
   adds no aliasing of its own.

The waveshaper runs at **4× oversampled** rate (two cascaded 63-tap
half-band FIRs, the same `Oversampler4` as `Shaper`); the tone stack runs at
native rate. The result is downsampled with the half-band FIR before the
output DC block and `level`.

---

## 4. Aliasing mitigation plan

- The waveshapers are **memoryless nonlinearities** (the Fuzz Face's pair
  and the Tone Bender's clip are one sample; the Big Muff's cascade is two
  memoryless stages). Memoryless nonlinearities generate harmonics that
  fold back as inharmonic aliases.
- **Plan:** run the nonlinear chain at 4× oversampling (`Oversampler4`,
  HALFBAND_TAPS = 63 taps per stage). The half-band FIR pushes images above
  0.5×4× out of the audio band; downsampling then removes them. This is the
  same mechanism `Shaper` already uses and is measured to drop a hard-clip
  3rd harmonic by ~78×.
- **Default:** uniform 4× for all three models to keep one code path.
  The gated Tone Bender has the sharpest transitions and aliases most, so
  2× is **not** acceptable for it; 4× covers all three. The Fuzz Face's
  smooth knee could pass at 2×, but a single path is simpler and cheaper to
  keep correct.
- **DC blocking:** 20 Hz 2nd-order high-pass at input and output, exactly
  like `EffectChain` (`dsp/src/chain.rs`).
- **Test:** extend `dsp/tests/effects.rs`'s aliasing test to each fuzz
  waveshaper; assert `e_os < e_naive * 0.2` (the measured 78× margin).
  Also assert all outputs finite and peak ≤ 1.2 × `level` + 1e-6.

---

## 5. Variant designs

### 5.1 Fuzz Face (Shin-ei FU-725S) — model 0

**Circuit:** two-transistor germanium feedback pair (common-base input
stage + common-emitter stage with the output fed back through germanium
diodes). Extremely high input impedance. The diodes turn on at ~0.2–0.3 V,
giving an asymmetric soft clip with a sharp knee near zero and different
slopes on the positive and negative halves. The bias point of the pair is
held by a resistor divider; when bias is lost (volume turned down, the
famous "starve"), the pair leaves its operating point and the output
collapses first in the low end — that is the character.

**Signal flow (per variant):** high-pass in → transistor-pair
waveshaper → output shelf (the pair's own high-pass roll-off ≈ 3–6 dB/oct
above 2–4 kHz) → soft limiter. No passive tone stack: tone is set by the
pair itself, approximated by the shared `tone` params as a gentle output
shelf.

**Waveshaper (nonlinearity, dimensionless):**

```
y(x) = x                              for |x| < Vd          (Vd ≈ 0.25)
y(x) = Vd + (x − Vd) * (1 − (x − Vd)/(1 − Vd))   for x > Vd
y(x) = −Vd − (x + Vd) * (0.6 + 0.4*(x + Vd)/Vd)  for x < −Vd
y = sign(x) * clamp_positive(|x|)
```

- The positive side has a soft knee just past 0; the negative side is
  slightly harder (the diode turn-on), so the curve is asymmetric — the
  source of the even-harmonics-rich "dirty" character.
- The bias is applied as a **drive-dependent offset** on the input: at low
  drive the pair sits near the knee (the "sweet" character); as drive
  rises the pair saturates hard (the "screechy" character). This is
  modelled by adding a small positive bias `b(drive)` that shrinks as
  drive grows — i.e. the curve is `y(x + b)` with `b` decreasing in
  drive. The user does not see `b`; it is folded into `drive` and `mid`.
- **`mid` param** maps to the bias/starve position: negative `mid` deepens
  the "starved" low end, positive `mid` re-biases the pair to the sweet
  spot. This is the one genuinely circuit-derived param in the shared set.
- **`drive`** is log-taper; typical "fuzz" is 2–6.

**Filters:** input 20 Hz HP shared; output shelf via the shared
`tone` params (bass/mid/treble treated as a gentle output EQ); no
dedicated DI / tone-stack circuitry beyond this.

**Notes / approximations:** germanium diode exponential is approximated by a
piecewise rational knee (the exact `exp()` form is not needed — the knee
shape is what matters for the harmonic content). The transistor pair is a
static curve; no dynamics/biasing state is carried per sample, so the
"starve" is a static gain shape, not a time-varying one. A true
time-varying starvation model would need a bias state and a volume-fold
model — deliberately out of scope for v1.

### 5.2 Big Muff (Electro-Harmonix / Riverside) — model 1

**Circuit:** cascaded transistor gain stages into a diode-reset (clamp) stage
feeding ground, plus the classic passive tone stack (bass +12 dB, mid −12 dB,
treble +6 dB at full scoop) and a feedback network that provides the
sustain. The two-stage cascade (soft soft-clip, then hard-ish clamp) gives
a steep, near-square transfer; the feedback and clamp are what make the
note sing and "sweep" as the gain is raised.

**Signal flow (per variant):** high-pass in → gain stage 1 (soft clip,
`tanh`) → diode-clamp stage 2 → passive tone stack → soft limiter. The
cascaded memoryless stages produce the dense, singing sustain.

**Waveshaper (nonlinearity, dimensionless):**

```
y1(x) = tanh(k1 * x)                 with k1 ≈ 3.0
y2(x) = x                            for |x| < Vd    (Vd ≈ 0.6)
y2(x) = sign(x) * (Vd + k2*(|x| − Vd))   for |x| >= Vd   (k2 ≈ 1.8)
y(x)  = y2(y1(x))
```

- Stage 1 is a gentle soft clip; stage 2 is a knee that becomes a hard
  clamp above `Vd`. The cascade is slightly asymmetric (stage 2's clamp is
  a bit steeper on one side), which keeps the even harmonics in check
  while the square-like top creates the Big Muff's overtone wash.
- The **sustain** is the feedback network inside stage 2: modelled as a
  drive-dependent `k2` that increases with `drive` (more feedback = more
  sustain = more clipping), so the curve steepens as you turn Drive up.
  This is the "sweep" the user asked for — not a glide, but a continuous
  change in the clipping shape.
- **`mid` param** maps to the tone stack's mid scoop (the Big Muff's
  signature: bass +12, mid −12, treble +6 at full negative `mid`).
- **`bass` / `treble`** map directly onto the passive tone stack
  biquads. Default `bass = 0`, `treble = 0`, `mid = 0` is "neutral".
- Typical Drive 3–8, with `level` rolled back to −6..−3 at high drive.

**Filters:** input 20 Hz HP (shared); tone stack = 3 RBJ biquads (bass
shelf 200 Hz, mid peak 1 kHz, treble shelf 4 kHz, Q = 0.7); output a gentle
4 kHz high-shelf roll-off. The 4 kHz output shelf is the "dark" Big Muff
character.

**Notes / approximations:** the passive tone stack is modelled as active
gains; no capacitor-frequency/time-delay modelling (the real Big Muff's
tone cap is a gently saturating nonlinearity — out of scope for v1). The
clamp's feedback constant `k2` is a static curve, not a dynamic state.

### 5.3 Tone Bender / Maestro FZ-1 — model 2

**Circuit:** 2–3 transistor gain stages (the FZ-1 has three: a preamp,
a feedback-clipper transistor, and an output driver), with hard, **gated**
asymmetric clipping. The FZ-1's defining trait is "squelch": a zener (or
transistor) clamps the output only when the instantaneous voltage exceeds
a threshold, so the signal alternates between nearly unclipped and fully
hard-clamped. That alternation produces octave-2 harmonics and the gated,
pulsing texture. (The Octave Fuzz took a different path — diode-reset
integration for octave-up harmonics — but the FZ-1's gated-clip character
is the one requested.)

**Signal flow (per variant):** high-pass in → gain stage 1 (soft clip) →
gated clipper (zener-style clamp that engages only above a threshold) →
output shelf → soft limiter. The gated clipper is the heart of the
character.

**Waveshaper (nonlinearity, dimensionless):**

```
y1(x) = tanh(k1 * x)                 with k1 ≈ 1.5
y2(x) = x                            for |x| < Vz
y2(x) = sign(x) * Vz                 for |x| >= Vz
y(x)  = y2(y1(x))
```

- `Vz ≈ 0.7` is the zener threshold. Below it the signal passes with
  gentle soft-clip character; above it the output is **hard-clamped** to
  `±Vz` — there is no knee, which is what produces the gated, pulsing
  "squelch". The hard edge is the most aliased thing in the design, which
  is why 4× oversampling (§4) is mandatory for this model.
- The FZ-1's "fuzz" middle (the third transistor in the FZ-1 schematic)
  is approximated by the drive-dependent `k1` of stage 1: more drive =
  softer stage-1 knee = more signal reaching the zener gate = more gated
  time. This is the continuous "sweep" the user asked for.
- **`mid` param** maps to a gentle mid-scoop on the output shelf (the FZ-1
  has no true mid control; this is the closest active approximation).
- **`bass` / `treble`** map onto the output shelf as a simple tilt.
- Typical Drive 1.5–4, `level` near 0 (the FZ-1's output stage is a
  follower, so it is naturally hot).

**Notes / approximations:** the zener clipper is modelled as a hard
threshold with a `sign()` function — there is no zener exponential curve.
The "gating" is a static voltage threshold, not a time-varying gate;
designing a true envelope-following gate (a diode-reset integrator or a
voltage-controlled switch) is a future variant (the "Octave Fuzz" path).

---

## 6. Shared effect struct / enum layout

All three variants live in one processor. The code is new in `dsp/src/`;
the graph block is new in `graph/src/blocks.rs`.

**`dsp/src/fuzz.rs`:**

```
pub enum FuzzModel { Face, Muff, Bender }

pub struct Fuzz {
    pub model: FuzzModel,
    pub drive: f32,          // 0.1 .. 10
    pub bass: f32, mid: f32, treble: f32,   // dB
    pub level: f32,          // linear 0..4 (set from the dB `level` param)
    pub oversampling: Oversampling,      // X4
    up2: Upsampler2, down2: Downsampler2, up4: Oversampler4,
    dc_in: DcBlocker, dc_out: DcBlocker,
    tone_low: Biquad, tone_mid: Biquad, tone_high: Biquad,
    // Output soft limiter (shared with EffectChain).
}
```

- **Semantics mirror `Shaper`** (`dsp/src/waveshape.rs`): `new`/`default`,
  `process`, `reset`, plain `f32` fields so the control loop can update
  them without allocating. `Fuzz` is `no_std` and allocator-free, like
  `Shaper` and `EffectChain`.
- **`model` maps to `FuzzModel`** — a small `Copy` enum; the `Default` is
  `Face` (matching the parameter default of model 0).
- The waveshaper functions for each model are **private methods on
  `Fuzz`** (`fn face(self, x) -> f32`, etc.), each operating on a single
  `f32`. The cascade is one method call deep, so the function per variant is
  documented in §5.
- **Oversampling:** default `Oversampling::X4` for all models (§4).

**`graph/src/blocks.rs` — the `Fuzz` block:**

```
BlockSpec {
    id: "fuzz.fuzz",
    name: "Fuzz",
    category: Category::Drive,
    inputs: [PortSpec::audio("in")],
    outputs: [PortSpec::audio("out")],
    params: [
        ParamSpec { id: "model", name: "Model", unit: Unit::Choice(FUZZ_MODELS), min: 0, max: 2, default: 0, curve: Curve::Linear },
        // drive, bass, mid, treble, level as in §2
    ],
}
```

- `prepare(sr, _max)` creates the `Fuzz` processor, the DC blockers and the
  tone biquads; `reset()` clears filter state.
- `process(ctx)` reads the 5 params, maps them onto the processor fields
  (dB → linear for `level`), calls `fuzz.process(ctx.inputs[0][i])` per
  sample, and writes `ctx.outputs[0]`.
- **Registration:** one line in `builtin_registry()`:
  `r.add::<Fuzz>();` — the engine serves `registry_json(&Registry)` over IPC
  and the UI builds the rack and the add-block picker from that snapshot.
  No protocol change, no preset-schema change.

---

## 7. Tests

`graph/tests/fuzz.rs`:

1. **registry** — `fuzz.fuzz` is present with name `Fuzz`, category `Drive`,
   audio in/out ports, and the params above (including the `model` choice).
2. **dry path** — `level = -infinity` (or `level = 0` then the soft limiter
   is bypassed) returns the input unchanged when `drive = 0` and `model =
   Face` — the wet/dry mix is wired the right way round (the engine's
   `gain` block already has this test pattern).
3. **asymmetric character** — for each model, run a sine through
   `io.input → fuzz.fuzz → io.output` at a fixed drive; assert:
   - output is finite and peak < 2.0;
   - the 2nd-harmonic energy is higher than the 3rd (even-harmonic
     heavy), which is the fuzz signature, for models 0 and 1;
   - the Tone Bender (2) has a strong 2nd harmonic; the Big Muff (1) has
     both 2nd and 3rd; the Fuzz Face (0) is the most even-harmonic-heavy.
   (A crude harmonic probe by DFT bin, not a full FFT.)
4. **aliasing** — reuse the existing `dsp/tests/effects.rs` aliasing method
   (compare 9 kHz sine hard-clipped with/without 4× oversampling);
   assert `e_os < e_naive * 0.2` for every model.
5. **persistence** — a `Doc` (JSON) carrying the 6 params round-trips
   (`Doc::from_json` → `to_json` → `from_json`) and compiles/runs finite.
6. **drive sweep** — a 1 kHz sine swept from drive 0.5 → 8.0 over the
   render; assert the output is bounded and the 2nd-harmonic energy stays
   above the 3rd (no sudden foldback into a lower, wrong harmonic).

---

## 8. Known knock-on

- The existing `dsp/tests/effects.rs` name is a fuzz-style test for the
  `EffectChain` (distortion kind). It is independent of this block and
  stays green; it is not a child of this card.
- The `tone` parameter names in `EffectChain` (`tone` = −1..+1) coexist;
  `fuzz.fuzz` uses `bass`/`mid`/`treble` in dB, which is a different,
  physically meaningful control. No collision.

---

## 9. Out of scope

- Stereo / multi-voice fuzz (the graph is mono per channel).
- A true time-varying bias/starve model (the Fuzz Face "starve" is static
  in v1).
- A gated envelope following the input (the Tone Bender's "squelch gate"
  stays a static threshold in v1; the Octave-fuzz integrator path is a
  separate variant).
- Per-block icon (the `Fuzz` block inherits the `Drive` category glyph).
- Any change to how the registry reaches the UI.

---

## 10. Builder cards (to be created; this doc is the contract)

The fuzz effect is one block, so the implementation splits into three
builder cards across the dsp, graph (engine palette) and UI layers. The
engine crate itself needs **no code change**: it serves the graph crate's
`builtin_registry()` over IPC, so registering the block in
`graph/src/blocks.rs` is what makes it available to the engine and the UI.

| # | Card | Assignee | Files | Approach | Acceptance criteria |
|---|---|---|---|---|---|
| 1 | `fuzz-dsp-processor` — dsp crate: add `fuzz.rs` | pedal-builder | `dsp/src/fuzz.rs` (new) | Implement `Fuzz` struct, `FuzzModel` enum, per-variant waveshaper methods, `process`, `reset`, 4× oversampling + DC blockers + tone biquads. | `cargo test -p pedal-dsp` green; `process` test: sine through each model → finite, bounded, asymmetric (even-harmonic-heavy) output; alias test green; round-trip test green. |
| 2 | `fuzz-graph-block` — graph crate: Fuzz block + register in `builtin_registry()` | pedal-builder | `graph/src/blocks.rs` (edit) | Add the `Fuzz` block spec, `prepare/process/reset`, and one line `r.add::<Fuzz>()` in `builtin_registry()` (the engine's palette). | `graph/tests/fuzz.rs` passes: registry, dry path, asymmetric character, aliasing, persistence, drive sweep; `cargo test --workspace` green. |
| 3 | `fuzz-ui-wire` — UI: render the Fuzz block + preset | pedal-ui-builder | `ui/src/components/Editor.tsx`, a Fuzz preset | UI renders the `Model` segmented row (Fuzz Face / Big Muff / Tone Bender) + Drive/Tone/Level knobs; the block appears in the drive category of the add-block picker. | `pnpm run build` green; the UI shows `Fuzz` in the drive category with the `Model` segmented row and 3 knobs; a Fuzz preset loads and is selectable. |

**Card order.** 1 (dsp) before 2 (block, which calls dsp) before 3 (UI, which
needs the block registered in the palette). Card 3 depends on card 2 as
well as card 1, since the UI reads the block spec from `registry`.

---

## 11. Try it

- `cargo test --workspace --exclude pedal-shell` — all tests green, incl.
  `graph/tests/fuzz.rs`.
- `pnpm run build` — UI builds with the new block.
- Open the app and add a `Fuzz` block from the drive category; select the
  `Model` segmented row (Fuzz Face / Big Muff / Tone Bender); drag the
  `Drive` knob past 4 and listen for the continuous sweep (no glide, no
  jump) — that is the drive-dependent clipping-shape change.
- Deliberately unplug a Fuzz Face in a real pedal: the output drops
  first in the low end. The approximation captures this as a static
  bias/starve shape; it is not a time-varying simulation.

## 12. Decisions recorded

1. Single `fuzz.fuzz` block with a `model` choice, not three separate
   blocks. Decision: the codebase convention is one block type per
   category (e.g. `amp.basic` has a `voice` choice), and the shared
   parameter set is a better fit for the flat IPC state shape and the UI's
   single-editor-per-selected-block model. The variants are isolated in
   the waveshaper methods, so the difference is still one block implementation
   choice away.
2. Uniform 4× oversampling for all models. Decision: keep one code path;
   the gated Tone Bender's hard edge makes 2× unacceptable, and the shared
   path costs nothing extra (the oversampler is already there).
3. `tone` split into `bass`/`mid`/`treble` dB params. Decision: the shared
   set must be a single coherent table for the IPC state and the UI; the
   per-variant mapping is documented in §5 so each variant can special-case
   its own interpretation.
4. The Fuzz Face's "starve" is static in v1. Decision: the user asked for
   the "bias/starve behavior" character; a static bias+drive-mapped
   nonlinearity captures the *position* of the starve without a full
   time-varying bias model. Added as a known limit in §5.1.
5. No protocol or preset-schema change. Decision: `fuzz.fuzz` is one more
   block in the existing `channels[].blocks[]` state, exactly like every
   other block.

---

*Author: pedal-architect. Status: design (not implemented).*
