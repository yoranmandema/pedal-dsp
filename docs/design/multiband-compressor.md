# Multiband Compressor — architecture & data model

**For:** `pedal-dsp` (engine `dsp_graph`/`engine`, UI Preact).
**Effect id:** `dynamics.multiband_compressor` · **Category:** `Dynamics` (display order 4th: after Tone, before Time).
**Status:** designed, no code changed. Builder cards follow.

---

## 1. Object model & registration

* The multiband is **one block in a channel strip**, exactly like `dynamics.compressor`
  (`dynamics.compressor` is `Category::Dynamics`). It is not a group of blocks.
* It is registered in `dsp_graph::blocks::builtin_registry` (engine) and mirrored into the
  UI registry (`registry` in every `state` snapshot) so the **param editor is identical on
  both sides** — the UI reads `BlockSpec.params` from the engine, it does not know the block's
  name/shape by convention.
* Serialization: a multiband block is a `NodeDoc` entry (`id`, `block: "dynamics.multiband_compressor"`,
  `params: BTreeMap<String, f32>`, `bypass`). All numeric per-band params are stored by name, so
  a 3-band preset and a 4-band preset are the same schema with different keys.

## 2. Signal flow (crossover split into N bands)

```
input ──► [crossover] ──► N band signals ──► [per-band compressors] ──► [recombine sum]
                                                                     │
                                                                     ▼
                                                                  output_gain ──► dry/wet ──► output
```

* Band count is global: `bands` ∈ 2..4, **default 3**. Changing `bands` is a parameter
  change, not a block swap. The number of active crossover frequencies is `bands - 1`.
* Each crossover edge is **Linkwitz-Riley 4th-order**:
  * RL4 = two cascaded 2nd-order Butterworth sections (Q = 1/√2 ≈ 0.707), one low-pass, one
    high-pass, high-pass sign-inverted, summed. Two stages per edge → 24 dB/oct.
  * Implemented with `pedal_dsp::biquad::Biquad::design(BiquadKind::LowPass/HighPass, sr,
    f0, 0.707, 0.0)`. The LP/HP pair at each edge sums flat (0 dB at the edge).
  * Band `i` (1-based) is the cascade of one RL4 LP or HP per edge:
    * edges below `i` → **HP** (handles the band's lower side),
    * edges above `i` → **LP** (handles the band's upper side),
    * the band's own edges → **both LP and HP** (the pair sums flat, so the band is
      continuous with its neighbours).
    * So for 3 bands with `crossover_1`, `crossover_2`:
      * Band 1 = LP(crossover_1)
      * Band 2 = HP(crossover_1) · LP(crossover_2)
      * Band 3 = HP(crossover_2)
  * Recombination sums all band outputs; because the band filters are complementary and each
    edge pair sums flat, the total is the unfiltered input to within ~±0.3 dB. Bypass (§5)
    removes a band from the sum; solo (§5) replaces the sum with one band.

## 3. Per-band compressor parameters

Each band `i` (1..bands) has the same 7 params, all stored in the block's `params` map under
`band_<i>_<param>` and mirrored into each `NodeDoc.params`:

| param | id | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| threshold | `band_<i>_threshold` | dB | -60 | 0 | -20 | AmpGain |
| ratio | `band_<i>_ratio` | x | 1 | 20 | 4 | Linear |
| attack | `band_<i>_attack` | ms | 0.1 | 50 | 1 | Log |
| release | `band_<i>_release` | ms | 1 | 500 | 50 | Log |
| makeup | `band_<i>_makeup` | dB | 0 | 30 | 0 | AmpGain |
| bypass | `band_<i>_bypass` | on/off | 0 | 1 | 0 | Linear |
| solo | `band_<i>_solo` | on/off | 0 | 1 | 0 | Linear |

The UI renders the per-band parameter columns only for the currently active band count.

## 4. Global parameters

| param | id | unit | min | max | default | curve |
|---|---|---|---|---|---|---|
| bands | `bands` | count | 2 | 4 | 3 | Linear |
| crossover 1 | `crossover_1` | Hz | 20 | 2000 | 200 | Log |
| crossover 2 | `crossover_2` | Hz | 200 | 20000 | 4000 | Log |
| crossover 3 | `crossover_3` | Hz | 200 | 20000 | 8000 | Log |
| output gain | `output_gain` | dB | -24 | 24 | 0 | AmpGain |
| dry/wet | `dry_wet` | Percent | 0 | 100 | 100 | Linear |

* `crossover_1` is used for all band counts; `crossover_2`, `crossover_3` are active only when
  `bands >= 3` / `bands >= 4`. The UI and engine apply only the first `bands - 1`.
* `dry_wet = 100` → fully processed; `0` → dry, full pass-through. `dry = 100 - wet`.
* `output_gain` is a final trim (dB) applied after dry/wet mixing and before the master output,
  so it does not change the dry-to-wet balance.

## 5. Envelope detector & gain computer

* **Detector:** per-band **peak** follower, reused from the existing single-band
  `dynamics.compressor`: `pedal_dsp::envelope::EnvelopeFollower` (`process(input[i].abs())`),
  one instance per band. Attack/release time constants are computed in `prepare` from the host
  sample rate and stored by value. No dynamic allocation; the envelope state is `f32` only.
* **Detector level (dB):** `level_db = lin_to_db(max(level, 1e-4))` — matches the existing
  `dynamics.compressor` floor (`max(level, 1e-4)` ⇒ ≈ −80 dB) so silence maps to a finite,
  negative dB value and never reaches `lin_to_db`.
* **Gain computer (dB domain, no knee in v1):**
  ```
  excess   = level_db - threshold
  gr_db    = max(0, excess) * (1 - 1/ratio)   // linear in dB
  gain_db  = -gr_db + makeup
  gain     = db_to_lin(gain_db)
  out      = input * gain
  ```
* **Soft knee:** *not implemented in v1* (stated as the known limit, e.g. an open TODO).
  The design is a transparent, linear-in-dB compressor like `dynamics.compressor`.
* **Bypass / solo:**
  * `bypass` on band `i` → the band's compressor is skipped; its output is the raw input band
    (lossless pass-through). The band still occupies its crossover slot, so bypass does not
    change the crossover topology.
  * `solo` on band `i` → the output sum is replaced by band `i`'s output only; all other bands
    are muted (even if not bypassed). If multiple bands are soloed, they are summed.
  * Combine: out = dry_wet_mix * (sum of active band outputs + output_gain) + dry * input,
    where active band outputs exclude bypassed/soLo-muted bands and include solo-selected bands.

## 6. Real-time safety

* **No allocations in the audio thread.** All DSP state is stack-allocated (fixed arrays sized
  to the executor's `MAX_BLOCK = 64`; the executor already chunks to this size). No `Vec` or
  `String` in `process`.
* **No locks.** The block holds only `f32` coefficients + per-band `EnvelopeFollower` (no atomics,
  no mutex). Parameter reads within a `process` call are safe because the executor snapshots
  params into the context and applies updates between frames/blocks.
* **Denormal handling:** the detector floors its input at `1e-4` (matching the existing
  `dynamics.compressor`) so the envelope follower never receives denormals; `db_to_lin`/
  `lin_to_db` use `libm` and are well-conditioned. The
  biquad coefficients are computed with `libm` and are normalized at design time, so no
  denormal passes into the recombine sum. (Host may still flush denormals-to-zero; the block
  is denormal-clean as written.)
* **Sample-rate changes:** all time constants and filter coefficients are recomputed in
  `prepare(sr, max)`. The block is recreated on sr change; no param is reactive to sr at
  runtime, and `prepare` is only called by the executor's prepare path (not the audio callback).
* **Block size:** fixed to `MAX_BLOCK = 64`; the executor normalises all blocks to this size.

## 7. Metering (gain reduction per band → UI)

* Each band maintains its own **peak gain reduction** (`gr_db`) and **peak output** (`out_db`),
  tracked with `std::sync::atomic::AtomicU32` (f32 bits), matching the existing `Meters` pattern
  (`in_peak`, `out_peak`).
* After each process block the block snapshots per-band values into a `Meters`-shaped
  per-block struct; the executor copies it into the global `Meters` (the server broadcasts this
  every 100 ms as `event: meters`).
* `Meters` is extended from `{ in_peak, out_peak, xruns }` (see `engine/src/audio.rs`) to:
  ```
  { in_peak, out_peak, xruns, bands: ReadonlyArray<{ band: string; gr_db: number; out_db: number }> }
  ```
  where `band` is `band_1`…`band_N` and `gr_db`/`out_db` are the latest peak values.
* **UI:** the multiband param editor shows a per-band "GR" column rendered as a horizontal
  scale (0 dB at centre, red below −3 dB). The metering panel shows one meter per active band.
  (UI detail lives in the UI builder cards.)

## 8. Test plan

1. **Crossover flat-sum.** Run the block with a flat 0 dBFS sine sweep (e.g. 20 Hz→20 kHz,
   log-spaced, ≥ 1 s). Recombine all bands and compare to the input: within **±0.5 dB** at
   every frequency, and each band's −3 dB point is at its `crossover_<i>` within **±1%**.
   Also assert `sum(band outputs) == input` to within 0.3 dB for the 3-band default.
2. **Per-band gain-reduction correctness.** Drive band `i` with a 1 kHz tone `6 dB` above its
   threshold (others zeroed via solo or a quiet source). Assert:
   `gr_db ≈ (level_db − threshold) * (1 − 1/ratio) − makeup`, exact to **0.1 dB** for
   ratio 2, 4, 10. Sweep ratio {2,4,10} and threshold {−40,−20,−10}.
3. **Bypass.** With `band_<i>_bypass = 1`, assert band `i`'s output equals its input to within
   0.1 dB (no gain/attenuation), all other bands behave normally.
4. **Solo.** With `band_<i>_solo = 1`, assert only band `i` appears at the output to within
   0.1 dB; mute the other bands. Repeat with 2 simultaneous solos (sum).
5. **Silence / NaN safety.** 10 s of silence → output all-zero, no NaN/Inf, no xruns. NaN input
   → no crash/assert; NaN propagates only through the filtered path as designed.
6. **Sample-rate change.** Run at 48 kHz → 44.1 kHz → 48 kHz mid-run; state parameters stay
   finite and the crossover transposes correctly (repeat flat-sum test at each rate).
7. **Output meter.** After a loud transient, `out_peak` matches the measured peak to within
   0.1 dB (reuse the existing headless/audio-path test harness).

---

## Builder cards

All cards are one session of work (≈1–4 files), concrete, and leave the app green.

### Engine cards (assignee: `pedal-builder`)

1. **`t_*_eng_1 — multiband compressor block core`**
   Add `MultibandCompressor` to `graph/src/blocks.rs`: `BlockSpec` id `dynamics.multiband_compressor`
   (category `Dynamics`), params = `bands` (2-4, default 3), `crossover_1..3` (Hz, Log),
   `output_gain` (dB), `dry_wet` (Percent), plus the `band_<i>_*` set (§3). Signal flow = §2
   crossover/§5 gain computer, with per-band bypass/solo (§5). Times: `prepare(sr, max)` builds
   the RL4 biquads and per-band `EnvelopeFollower`; `process` is allocation-free and lock-free.
   Acceptance: `cargo test` green + a new unit test for flat-sum and one for per-band GR.

2. **`t_*_eng_2 — per-band metering & real-time safety`**
   Extend `engine/src/audio.rs` `Meters` with `bands: [{ band, gr_db, out_db }]` (§7); the
   multiband block writes per-band peak atoms each frame. Patch the multiband block to snapshot
   every param by value at `process` entry (no cross-frame stale read) and to floor detector
   input at `1e-9` (§6). Acceptance: `cargo test` green; no allocations on the audio path.

3. **`t_*_eng_3 — preset serialization & headless smoke`**
   Make the multiband block serialize/deserialize through `NodeDoc.params` (already automatic —
   carry through the existing patch). Add a headless + UI-off audio-path smoke test feeding a
   3-band rig through the multiband block. Acceptance: `cargo test` green; a saved preset round-trips.

### UI cards (assignee: `pedal-ui-builder`)

4. **`t_*_ui_1 — multiband block in the chain`**
   The engine adds its `BlockSpec` → the UI registry; `Chain.tsx` renders the multiband row with
   the block icon, name, bypass toggle (amber), and a per-band label chip. Updates
   `state.registry` automatically via `getState`. Acceptance: select the block in the chain; the
   per-band parameter columns appear.

5. **`t_*_ui_2 — per-band parameter editor`**
   `Editor.tsx` renders, for the multiband block, one row of controls per active band:
   threshold/ratio/attack/release/makeup knobs + bypass/solo toggles (numeric `0/1` params as
   checkboxes), plus the global `bands`, crossover frequencies, `output_gain`, `dry_wet`.
   Values are read from `params` (§3/§4), mirrored from the engine preset. Acceptance: dragging
   any knob updates engine state and the val panel shows the new value.

6. **`t_*_ui_3 — per-band GR metering`**
   `TopBar.tsx` / new `MultibandMeter` component renders one peak GR scale per active band
   (red < −3 dB, 0 dB centre), updated from the extended `Meters.bands` event. Acceptance:
   compress a signal; the per-band GR meters move and the val panel matches.

---

## Try it (product owner)

Run the app (see `docs/RUN-THE-APP.md`), open a project, drag **Multiband Compressor** into a
channel, and:

1. Pick a preset or leave defaults (3 bands, 200 Hz / 4 kHz).
2. Play a wide-spectrum signal. The per-band meters (if `t_*_ui_3` is done) show each band's
   gain reduction; the GR val panel reflects it.
3. Sweep a single band's **threshold** and **ratio**; listen for controlled attenuation, not a
   fast auto-wah (≥1 ms attack, 50 ms release defaults).
4. Set **band_1_bypass** to compare mixed vs. clean low end; set **band_2_solo** to hear one
   band in isolation.
5. Change **bands** 2→4 and move **crossover_1..3**; the crossover sweeps without clicks.
6. Save a preset (`File → Save`) and reload it to confirm the `band_*` params round-trip.

## Known limits (by design)

* No soft knee (transparent linear-in-dB compressor only).
* Peak-only envelope (no RMS/peaking hold). Per-band metering is peak GR of the band, not the
  whole signal.
* Crossover is split on the current sample rate; band boundaries shift with sr (acceptable for
  v1; the flat-sum test is repeated at each rate).
