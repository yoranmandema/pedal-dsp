# Overdrive design & voicing specs

Status: draft v0.1 — design before code
Depends on: `docs/ARCHITECTURE.md`, `docs/IPC-PROTOCOL.md`, `docs/DSP-BLOCKS-AND-SIMULATOR.md`

## 0. Conventions & naming

- Block id convention: `<category>.<name>`. New overdrive variants use the
  `dist.` prefix. Existing baseline: `dist.overdrive` (simple tanh overdrive).
- Every overdrive variant is a one-input/one-output `Block` (`graph/src/block.rs`)
  in category `Distortion`, registered in `graph/src/blocks.rs` via
  `builtin_registry()`.
- Param table shape: `id`, `name`, `unit`, `min`, `max`, `default`, `curve`
  (defined in `graph/src/types.rs`).
- Units: `Linear` = linear ratio (blank unit, value 0..1 or 0.1..10),
  `Decibels` = dB with `AmpGain` curve, `Hertz` = Hz with `Log` curve,
  `Milliseconds` = ms, `Percent` = 0..100 (percent sign shown).
- Curves: `AmpGain` (dB-linear), `Log` (log scale), `Linear`. **Note:** the
  current UI (`ui/src/components/Knob.tsx`, `Editor.tsx`) transmits and renders
  raw linear real values and does not apply `curve`; treat `curve` as
  informational. Do not build value-normalisation into a block.
- Registry & IPC: `graph/src/blocks.rs` → `builtin_registry()`; engine exposes
  the registry to the UI via `engine/src/server.rs::registry_json()`. No UI
  change is needed — the registry JSON is consumed verbatim by the existing
  `ui/src/protocol.ts`, `Editor.tsx` and `Knob.tsx`.
- Shared DSP primitives live in `dsp/` (`biquad`, `svf`, `fir`, `oversample`,
  `waveshape`, `dc_blocker`, `util`).

## 1. Shared overdrive building blocks

All six shared blocks live in `dsp/`, and are reused by every variant.

### B1 — Input high-pass (DC + rumble), fixed
- Implementation: `dsp/src/biquad.rs` → `Biquad::make(BiquadKind::HighPass, sr,
  20.0, 0.707, 0.0)` (RBJ cookbook). 20 Hz, Q 0.707, unity gain.
- Pin: 20 Hz, Q 0.707, no parameter. Hard-coded at the top of every chain.

### B2 — Pre/post EQ (tone shaping)
RBJ biquads from `dsp/src/biquad.rs` (`Biquad::make(kind, sr, f0, q, gain_db)`),
all `Decibels`, `AmpGain` curve, default `0.0`:
- `pre_mid` — peaking at 720 Hz: `Peak`, f0 720, Q 1.2.
- `pre_bass` — low shelf 200 Hz: `LowShelf`, f0 200, Q 0.707.
- `pre_treble` — high shelf 4 kHz: `HighShelf`, f0 4000, Q 0.707.
- `post_tone` — presence/low shelf (the "Tone" control): `LowShelf`, f0 3200,
  Q 0.707, gain -12..+12 dB.
- `presence` — presence high shelf (Precision Drive): `HighShelf`, f0 5000,
  Q 0.8, gain -12..+12 dB.
- `big_muff_tone` — variable peaking (Big Muff): `Peak`, f0 from a `Hertz`
  parameter, Q 0.7, gain -18..+12 dB.

### B3 — Waveshapers (from `dsp/src/waveshape.rs`)
- Symmetric: `ShaperKind::SoftClip` (`u/(1+|u|)`), `ShaperKind::Tanh`
  (`tanh_approx(u)`).
- Asymmetric / tube (Tube Screamer): `ShaperKind::Tube` — `u>=0 ? tanh_approx(u)
  : tanh_approx(u*0.6)*0.75`.
- Fuzz (Big Muff, when fuzz ≥ 0.5): `ShaperKind::Fuzz` — `biased = u*0.7+0.15;
  saturate(biased*1.6)`.
- Diode-feedback clipper (Precision Drive): new `ShaperKind::DiodeClip`,
  transfer `y = tanh_approx(g * tanh_approx(x))` — a two-stage "diode-like" clip
  with drive `g`. 4x oversampling wraps it. Do **not** implement a real
  delay-feedback loop for v1 (out of scope; changes aliasing behaviour); the
  asymmetric `Tube` shaper already captures the "diode clipping in the feedback
  loop" character for the Tube Screamer.

### B4 — Anti-aliasing (oversampling)
- Use `dsp::oversample::Oversampler4` (`dsp/src/oversample.rs`, two cascaded
  2× half-band FIRs, `HALFBAND_TAPS=63`).
- Every nonlinearity (waveshaper) runs 4× sampled; downsample back to host rate.
  Filters (B2) run at host rate. This matches the existing pattern in
  `dsp/src/waveshape.rs`.
- Latency: ≈ 30 samples @48 kHz. Declare `latency_samples() = 30` on each
  overdrive block so the latency pass accounts for it.

### B5 — Tone stage
- Cascade of B2 biquads. Design coefficients once at `prepare` time (`sr`
  passed to `Block::prepare`); never per-sample. `diy` gain values are applied
  via `band.set_gain(g)` (or equivalent setter) before processing.

### B6 — Level/gain staging
- `drive` (Linear ratio) pre-clip gain: applied as `x * drive` before the
  waveshaper.
- `level` (Decibels) output trim after the final stage, `AmpGain`, `db_to_lin`.
- Global math: `out = db_to_lin(level) * block_chain(input * inputGain_header *
  drive)`. `inputGain` (header trim, -60..24 dB) and `masterOutVolume` (master
  out) live on the engine `Rig`, outside the block.
- Helpers: `dsp/src/util.rs` (`db_to_lin`, `lin_to_db`, `clamp`,
  `tanh_approx`, `saturate`, `lerp`, `mix`).

## 2. Variants

### 2.1 Tube Screamer — block id `drive.tube_screamer`
Stages, in order: input HPF (B1) → pre-EQ (B2: pre_mid 720 Hz + pre_bass +
pre_treble) → waveshaper (B3 Tube, scaled by drive) → post-tone (B2 tone) →
level (B6).

| id | name | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| `drive` | Drive | Linear | 0.1 | 10.0 | 1.0 | Log |
| `pre_mid` | Pre Mid | Decibels | -12.0 | 12.0 | 0.0 | AmpGain |
| `pre_bass` | Pre Bass | Decibels | -12.0 | 12.0 | 0.0 | AmpGain |
| `pre_treble` | Pre Treble | Decibels | -12.0 | 12.0 | 0.0 | AmpGain |
| `tone` | Tone | Decibels | -12.0 | 12.0 | 0.0 | AmpGain |
| `level` | Level | Decibels | -30.0 | 12.0 | 0.0 | AmpGain |

"Diode clipping in the feedback loop" → `ShaperKind::Tube` (asymmetric soft
clip). A real delay-feedback loop is deferred to v2.

### 2.2 Precision Drive — block id `drive.precision_drive`
Stages: input HPF → diode-feedback clipper (B3 DiodeClip + punch envelope) →
tone (post_tone low shelf 2.6 kHz + presence high shelf 5 kHz) → volume.

Transient pump ("Punch", exact): `|x|` is smoothed by two one-poles in series:
- `k = 1.0 - (-1.0 / (tau * sr)).exp()`, `tau = time_ms / 1000.0`.
- `attack_env`: one-pole on `|x|` with `tau = attack` ms (fast).
- `pump_env`: one-pole on `attack_env` with `tau = release` ms; when
  `attack_env > pump_env` it tracks up fast (`k_att`), else decays toward 0
  (`k_rel`).
- `e = clamp(pump_env / 0.5, 0, 1)` (0.5 = "loud" reference).
- `drive_eff = gain * (1 - tightness * e)`.
- Output = clipper(`drive_eff * x`), 4× sampled.

| id | name | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| `gain` | Gain | Linear | 0.1 | 20.0 | 2.0 | Log |
| `tightness` | Punch | Linear | 0.0 | 1.0 | 0.5 | Linear |
| `attack` | Attack | Milliseconds | 1.0 | 200.0 | 10.0 | Linear |
| `release` | Release | Milliseconds | 100.0 | 3000.0 | 600.0 | Linear |
| `tone` | Tone | Decibels | -12.0 | 12.0 | 0.0 | AmpGain |
| `presence` | Presence | Decibels | -12.0 | 12.0 | 0.0 | AmpGain |
| `volume` | Volume | Decibels | -30.0 | 12.0 | 0.0 | AmpGain |

### 2.3 Klon — block id `drive.klon`
Transparent, symmetric soft clip: `y = softclip(x * (1 + feel*0.5))`, where
`softclip(u) = u / (1 + |u|)` (ShaperKind::SoftClip).

| id | name | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| `drive` | Drive | Linear | 0.1 | 10.0 | 1.0 | Log |
| `feel` | Feel | Linear | 0.0 | 1.0 | 0.0 | Linear |
| `tone` | Tone | Decibels | -12.0 | 12.0 | 0.0 | AmpGain |
| `level` | Level | Decibels | -30.0 | 12.0 | 0.0 | AmpGain |

### 2.4 Big Muff — block id `drive.big_muff`
Fuzz-ish drive with variable brightness. Waveshaper: `fuzz < 0.5 ? Tube :
Fuzz`, scaled by drive. Tone = variable peaking at `f0`.

| id | name | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| `drive` | Drive | Linear | 0.1 | 10.0 | 1.0 | Log |
| `fuzz` | Fuzz | Linear | 0.0 | 1.0 | 0.5 | Linear |
| `tone` | Tone | Hertz | 200.0 | 10000.0 | 2000.0 | Log |
| `blend` | Blend | Linear | 0.0 | 1.0 | 0.5 | Linear |
| `level` | Level | Decibels | -24.0 | 24.0 | 0.0 | AmpGain |

## 3. Blocks to implement (file layout)

All four variant blocks are self-contained `struct`s in `graph/src/blocks.rs`
(the same file that holds `Overdrive`). Shared helpers:

- `dsp/src/one_pole.rs` (new) — per-sample one-pole filter, `k` computation,
  envelope follower (used by the Precision Drive pump).
- `dsp/src/waveshape.rs` — add `ShaperKind::DiodeClip`.
- `graph/src/overdrive.rs` (new) — reusable pipeline helpers (B1–B6 stubs).

Variation-specific files: each variant is added to `graph/src/blocks.rs` only.

## 4. Builder card list

The existing cards (assignees are `pedal-builder` except `t_d08e5d26` =
`pedal-ui-builder` and `t_fee366ad` = `pedal-reviewer`; no cards to create):

| Card id | Title | Files | Depends on | Independent? |
|---|---|---|---|---|
| `t_452a6f48` | Implement shared overdrive DSP primitives | `dsp/src/one_pole.rs`, `dsp/src/waveshape.rs`, `graph/src/overdrive.rs` | `t_e44fa418` (this card) | yes (root of the chain) |
| `t_29593488` | Tube Screamer effect | add `TubeScreamer` to `graph/src/blocks.rs` | `t_452a6f48` | independent of PD/UI |
| `t_4022181a` | Precision Drive effect | add `PrecisionDrive` to `graph/src/blocks.rs` | `t_452a6f48` | independent of TS/UI |
| `t_d08e5d26` | UI overdrive parameter controls | `ui/src/components/Editor.tsx`, `ui/src/components/Knob.tsx` | `t_29593488`, `t_4022181a` | independent of engine variants |
| `t_fee366ad` | End-to-end review | — | `t_d08e5d26` | independent of TS/PD implementations |
| `t_9431e38d` | Root feature | — | all of the above | leaf; runs last |

Dependencies: `t_452a6f48` → `{t_29593488, t_4022181a}` → `t_d08e5d26` → `t_fee366ad` → `t_9431e38d`.
`t_29593488` and `t_4022181a` and `t_d08e5d26` are parallel branches.

## 5. Verification

- `cargo test -p dsp` green (new unit tests for `one_pole`, `DiodeClip`,
  clipping bounds, asymmetry, filter response sanity, no NaN / denormals,
  silence in → silence out).
- `cargo test -p dsp-graph` green.
- `cargo build` (whole workspace) green.
- `host-sim --graph RIG.json --in input.wav --out output.wav`, with a rig that
  plants each variant between `io.input` / `io.output`. Checks:
  - DC at 20 Hz attenuated > 40 dB;
  - no NaN/inf at max settings;
  - TS: `pre_mid` +6 dB at 720 Hz shows mid-emphasis before clipping;
  - PD: `tightness` 1.0 pulls down low-end transients vs a flat `tightness` 0
    reference at the same `gain`;
  - Big Muff: `tone` high cutoff brightens and adds "sizzle" vs low cutoff;
  - all: `level` scales output without hard clipping (master limiter caps).
- UI: `getState` snapshot shows the four new `registry` entries with the exact
  params above; Editor panel renders knobs with correct labels/range/default/
  units; `setParam` per-param path works end to end via the bridge.
- Latency: each variant declares `latency_samples() = 30` (≈0.6 ms @48 kHz);
  `roundTripLatencyMs` unchanged versus the bypass graph.
