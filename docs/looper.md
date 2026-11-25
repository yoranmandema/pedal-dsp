# Looper effect — design

Status: **design only, no code**. Author: pedal-architect. Card: `t_e9470c77`.
Follow-up builders: `t_8f27e62e` (Rust engine + tests) then `t_af684d86` (Preact UI).
The two builder cards already exist from the auto-decomposer; read this doc first
and add it as a comment on each card. No framework change is required for the
parameter/command interface — the looper uses the existing `setParam` command,
the `ParamSpec` model and the `NodeDoc.params` per-node map.

---

## 1. What ships

One new block in the **Time** category, registered like the other effects:

| | |
|---|---|
| id | `time.looper` |
| name | `Looper` |
| category | `Time` |
| ports | `in` (audio) → `out` (audio) |
| base primitive | none — the block owns its own fixed circular record/playback buffer (`LoopBuffer`) |

The engine serves it to the UI through `event: state` (`registry`), exactly like
`time.delay`, `time.pitchdelay`, `mod.chorus`, `mod.tremolo`. The UI builds the
rack, the add-block picker and the bottom editor from that snapshot — nothing new
on the wire.

---

## 2. Parameters

Params are ordered; the block reads them **by position** from `BlockCtx::params`
(the same order as every other block). The UI and the JSON round-trip by id.

| idx | id | name | unit | min | max | default | curve | semantics |
|---|---|---|---|---|---|---|---|---|
| 0 | `trig_record` | Record | Linear (gate) | 0 | 1 | 0 | Linear | momentary record trigger (see §2.1) |
| 1 | `trig_play` | Play | Linear (gate) | 0 | 1 | 0 | Linear | momentary play trigger |
| 2 | `trig_overdub` | Overdub | Linear (gate) | 0 | 1 | 0 | Linear | momentary overdub trigger |
| 3 | `trig_stop` | Stop | Linear (gate) | 0 | 1 | 0 | Linear | momentary stop trigger |
| 4 | `trig_clear` | Clear | Linear (gate) | 0 | 1 | 0 | Linear | momentary clear trigger |
| 5 | `time` | Loop length | ms | 250 | 10000 | 1000 | Log | how long the take is |
| 6 | `feedback` | Feedback | % | 0 | 95 | 35 | Linear | repeat decay per loop |
| 7 | `mix` | Mix | % | 0 | 100 | 100 | Linear | dry/wet: 0 = only the live dry signal, 100 = only the loop playback |
| 8 | `level` | Level | % | 0 | 100 | 100 | Linear | output level of the loop playback tail |
| 9 | `half` | Half speed | 0/1 gate | 0 | 1 | 0 | Linear | playback at 0.5× `time` (one octave up) |
| 10 | `reverse` | Reverse | 0/1 gate | 0 | 1 | 0 | Linear | playback runs backwards |

### 2.1 Trigger gates (params 0–4)

* Each trigger is a **momentary gate**: while the key is held the UI sends `1.0`,
  on release sends `0.0`. There are no separate "pressed" and "released" states
  — the engine consumes the *edges*.
* A transition is registered on a **rising edge** (`prev == 0` and current
  `>= 0.5`) and the gate is de-asserted on the falling edge. A button that is
  still held does not retrigger.
* The UI renders each trigger as a momentary button with a visible "down" state:
  Record/Play/Overdub/Stop/Clear. The knock-on of this contract is that the UI
  keeps the gate `1.0` while the pointer/focus is held and releases on pointerup
  or Escape.
* `Stop` and `Clear` are the only two that, while held, force a transition out of
  the current state (see §3). `Record`/`Play`/`Overdub` are idempotent while
  already in that state.

### 2.2 Range rationale

* **`time` 250–10 000 ms (log)**: 250 ms is the shortest useful take; 10 s is a
  full overshoot of a short line. Log spacing gives equal perceptual steps.
* **`feedback` 0–95 %**: 0 = one repeat, 95 ≈ the loudest sustained loop the
  buffer can hold without clipping (see §3).
* **`mix` 0–100 %**: dry/wet, matches every other Time block (`time.delay`,
  `time.pitchdelay`, `mod.chorus`).
* **`level` 0–100 %**: the loop playback tail's own level, independent of the
  dry signal — a looper's repeat volume is not the same thing as the input
  level.
* **`half`/`reverse`**: 0/1 gates, so the UI can render them as two small
  toggle buttons rather than knobs.

### 2.3 Trigger flow (the only commands the engine supports)

```
UI → engine   setParam   { channel, block, param, value }
                     e.g. { "channel": "ch1", "block": "loop1", "param": "trig_record", "value": 1.0 }
engine → UI   event: state   { rev, looper_state, looper_frac }   (bumped on every transition)
engine → UI   event: looper  { state, frac }    (sent ~10 Hz while state is Playing/Overdubbing)
```

No new command types, no new IPC message shape.

---

## 3. Real-time-safe buffer strategy

### 3.1 Allocation policy — no allocation in the audio path

* The record/playback buffer is a **const-generic fixed array**,
  `LoopBuffer<const N: usize>` holding `N` `f32`s. It lives inside the block
  struct, which is allocated once per node at registry build time — it is never
  touched by `Vec`/`String`/`Box` and never allocates in `process`.
* `process()` holds only scalar state (a few `usize`/`f32` and the gate
  sample-and-holds): a couple of hundred bytes of stack per call.
* `reset()` is called on preset change/bypass (never in the audio path) and
  zeroes the buffer with `std::ptr::write_bytes`.
* The block declares `latency_samples()` = 0 (the loop's delay *is* the sound,
  exactly like `time.delay`/`time.pitchdelay`/`mod.chorus`). The latency pass
  therefore leaves the branch alone.

### 3.2 Sizing — max loop length

* `N = 480_000` samples = **10 s at 48 kHz** (`LOOP_MAX_SAMPLES`).
  At 44.1 kHz that is 10.88 s; at 96 kHz 5.42 s. The UI reports the current
  sample rate and the *time* knob clamps to 10 s at the current rate, so the
  buffer never overflows.
* `N` is chosen so the write head cannot wrap once during a single block
  (`N >> max_block = 256`).
* The buffer is a **single const-generic array in the block struct** (not
  `Vec::with_capacity`), so a channel node that holds a looper pays exactly
  `N * 4` bytes and nothing else.

### 3.3 State held per block

| field | meaning |
|---|---|
| `cursor` | write/head position (where the next recorded sample goes) |
| `read` | current playback read position |
| `delay` | samples from `read` to the tail (`cursor`), in the direction of play (shrinks forward, grows reverse) |
| `state` | `Idle` (0) / `Recording` (1) / `Playing` (2) / `Overdubbing` (3) / `Stopped` (4) |
| `gate_held[5]` | sample-and-hold of the five trigger gates for edge detection |
| `fade` | 512-sample fade-in envelope (linear 0→1), applied at the start of every take |

### 3.4 Sample-accurate wrap

All wrap is modulo-`N` arithmetic with a non-negative result:

```rust
fn w(i: usize) -> usize { ((i % N) + N) % N }
```

`cursor` and `read` are always advanced by exactly ±1 sample per processed
sample, and `delay` is derived from them, never clamped except at the state
transitions. There is no fractional sample and no `floor` jitter in the loop
timing — the read head lands on sample boundaries.

### 3.5 Click-free crossfade at the loop boundary

Playback uses the same two-read-head technique that `dsp/src/delay.rs`'s
`PitchShifter` already validates (see `read_at` and `process`):

* Two read heads at `r` and `(r + N/2) % N`, each sample-accurate.
* The wrap region is the last N/2 samples of the take (and the first N/2 after
  the wrap). Inside it the two heads overlap and their weights glide as a
  **constant-power** pair `cos(θ), sin(θ)`, with `θ` advancing linearly across
  the region. Hann-style `sin²/cos²` would dip the summed power twice per
  cycle and ring in the feedback, so `cos/sin` is mandated.
* The seam (tail→head) is therefore never a step, whatever `feedback` does.
* A **512-sample fade-in** (linear 0→1) starts every take so pressing Play
  never sounds a hard click, and `mix` (dry/wet) already smooths the loop's
  entry into the live signal.
* A short **fade-out** (linear 1→0) of the loop playback at the end of the take
  prevents a pop when the read head reaches the tail.

### 3.6 Processing per sample

```text
record phase (state Recording):
    // The user hears the dry signal while recording; the loop itself stays
    // silent (writing live audio into the buffer would let it feed back into
    // the take and grow without control).
    buf.write(cursor, input[i])
    cursor = w(cursor + 1)

play phase (state Playing/Overdubbing):
    r  = read
    read = w(r + dir)               // dir = +1 forward, −1 reverse
    a  = buf[r]      ;  b = buf[w(r + N/2)]
    t  = (r mod N/2) / (N/2)        // 0..1 across the wrap region
    out = cos(t·π/2) * a  +  sin(t·π/2) * b
    out *= fade(state = start-of-take) * fadeout(state = end-of-take)
    if state == Overdubbing: buf.write(cursor, input[i] + out * feedback)   // overdub takes grow the loop
    else:                    buf.write(cursor, out * feedback)             // repeats
    cursor = w(cursor + 1)
    delay  = (cursor − r + N) % N   // shrinks forward, grows reverse

stopped (state Stopped): no writes to the buffer, no reads; the block passes
the dry input through at unity so the player still hears their signal after a
loop has finished (the loop is no longer part of the sound).
```

Note the order above is illustrative; the concrete implementation must write to
`buf` **after** computing the read, and the crossfade must use the same `r`
before and after the advance (see the reference implementation in
`dsp/src/delay.rs` for the exact two-head arithmetic).

### 3.7 Overdub

`Overdubbing` is the same as `Playing` *plus* a new take recorded on top of the
existing loop: each new input sample is written to `cursor` **and** the
feedback of the played-back loop is added in (exactly the `FeedbackDelay`
structure, since the loop *is* a feedback delay with `feedback` decay). When
the `Overdub` button is released the take is finalised into the buffer and the
state becomes `Playing` (the combined take, original + new, is immediately
replayable). `Stop` ends the overdub with the take in the buffer, same as a
normal take.

---

## 4. State machine

States: **I** = Idle, **R** = Recording, **P** = Playing, **O** = Overdubbing,
**S** = Stopped/Cleared.

| From | Trigger (rising edge) | → To | Trigger (falling edge) | → To |
|---|---|---|---|---|
| I | Record | R | Record | I (no-op) |
| I | Overdub | O | Overdub | I (no-op) |
| I | Play | P | Play | S |
| I | Stop | S | — | — |
| I | Clear | S | — | — |
| R | Play | P | Record (falling) | S |
| R | Overdub | O | Stop | S |
| R | Stop | S | — | — |
| P | Overdub | O | Stop | S |
| P | Stop | S | Play (falling) | S |
| O | Overdub (falling) | P | Stop | S |
| O | Stop | S | — | — |
| S | Record | R | — | — |
| S | Overdub | O | — | — |
| S | Play | P | — | — |
| S | Stop | S | — | — |
| S | Clear | S | — | — |

Notes:

* `Clear` zeroes the buffer and goes to `S` from *any* state. While in `S`, a
  held `Clear` does nothing (idempotent).
* `Recording` auto-ends when the buffer is **full** (i.e. `cursor` has caught up
  with the head of the take, `delay == 0`) — the take is complete, state →
  `S`. This is a safety stop, not a transition the UI sends.
* `Half` and `Reverse` are *parameters*, not states: they change the playback
  direction/rate while `P` or `O` is active. Both can be active at once
  (`half` + `reverse` = reverse at half speed).
* The engine reports the current state as an integer in the snapshot
  (`looper_state`), bumped whenever `rev` is bumped; `looper_frac` (0.0–1.0) is
  reported while `P` or `O` (see §5).

---

## 5. Control flow UI → engine, engine → UI

### 5.1 UI → engine

* Every knob/toggle/button is a `setParam` command (§2.3). There is **no**
  separate "transport command": `trig_record`/`trig_play`/`trig_overdub`/
  `trig_stop`/`trig_clear` are params, so the UI's existing `setParam`
  implementation covers them, and the engine's `set_param`/`Cmd::SetParam`
  path already handles them. Builders do not add new IPC commands.
* The UI writes the continuous params (`time`, `feedback`, `mix`, `level`,
  `half`, `reverse`) the same way, exactly like `time.delay`.

### 5.2 Engine → UI

* **State**: the engine sets `looper_state` (0..4 per §4) on every transition;
  the snapshot's `rev` is bumped on every mutation (`setParam` included,
  matching the existing `setParam`/`setBypass`/`addBlock` behaviour). The UI's
  `fold()` in `ui/src/state.ts` already enforces monotonic `rev`, so the
  transport buttons update with the knobs.
* **Position**: while `P` or `O`, the engine sends `event: looper`
  `{ state, frac }` at ~10 Hz, where `frac` is the fraction of the take already
  played, `frac = (elapsed / delay)`. The UI draws a progress bar under the
  loop panel (and a remaining-time readout if it wants one).
* The new snapshot field is `looper_state: u8` and the new event type is
  `event: looper` — both are **extensions**, ignored by any client that does not
  read them, backward compatible with the existing protocol.

---

## 6. Test plan

All tests run with `cargo test --workspace --exclude pedal-shell` (the engine
card's verify command).

### 6.1 Engine card tests (`graph/tests/looper.rs`)

1. **registry** — `time.looper` present, `Time` category, audio in/out, 11 params
   with the exact ids/units/ranges/default above.
2. **dry path** — with all triggers `0` and `mix = 0`, the output is sample-identical
   to the input (loop is silent, so the block is a pass-through).
3. **record → play reproduces the input** — drive the *same* executor in two
   phases on one block: (a) `trig_record = 1` for `T` samples of a sine burst,
   (b) `trig_play = 1` for the same length. Assert the second-phase output
   correlates (cross-correlation peak) with a copy of the input delayed by
   `time_samples` (the `time` param, up to 10 s), and that the peak is not
   delayed by any other amount. Asserts: recording wrote the take, playback
   reads it back at the right delay, no drift.
4. **wrap at the correct length** — render exactly `N` samples of a tone
   (or a ramp) through a lone looper with `time = 10 s`, and assert the read
   head wraps at the boundary: the output must remain finite, with no NaN/
   Inf, and the gain across the wrap must not dip (constant-power crossfade).
5. **overdub sums with decay** — record take A, start playback, then while
   playing set `trig_overdub = 1` briefly and release; assert the take now has
   the new material added (level increase in the new material band) and that
   `feedback` shapes the repeat decay of the *combined* take.
6. **clear resets** — record a take, `trig_clear = 1`, then read back the
   buffer: it must be digitally silent (all zeros), and the state must be `S`.
7. **state transitions** — a scripted gate sequence (record → play → overdub →
   stop → clear → record → play) that proves each transition lands in the
   state the §4 table predicts, and that no transition fires on a button
   release.
8. **max length** — set `time` to the max and render a few seconds; assert the
   buffer does not overflow and the take still plays back in full.
9. **bypass** — `mix = 0` is the bypass equivalent: output == input
   sample-exact (see test 2).

### 6.2 Unit tests for the buffer (if extracted)

* `LoopBuffer` wrap: write `N` samples, read `N` samples back — the read
  sequence is the write sequence, sample-exact.
* Crossfade: verify no sample of the crossfade window falls below `1/sqrt(2)`
  of the input power (constant-power property).
* Fade-in/out: assert the first/last 512 samples are scaled by the linear
  ramps.

### 6.3 Engine-level

* `cargo test --workspace --exclude pedal-shell` green, no new clippy warnings.
* Headless run with a sine input: the looper's output is finite for the whole
  buffer and the loop's repeat decay matches `feedback` (peak after one loop at
  `feedback = 35` is above the peak after two loops).

### 6.4 UI card tests

* `pnpm run build` clean (typecheck + build of `ui/`).
* Component test for the Looper panel: triggers render as buttons, the record
  button is "down" in `Recording`, the Play button is "down" in `Playing`,
  `looper_state` drives the banner, the progress bar advances during play.
* Cypress + existing stub engine: pressing Record records a take; Play plays it
  back; Overdub appends; Clear empties it (the UI card owns the e2e file).

---

## 7. UI intent

* The looper exists inside the **bottom editor** of the selected block
  (same `<footer>` shell as `Editor.tsx`), not as a new panel. The generic
  `Editor` renders all blocks from `s.registry` with `Knob` (non-choice params)
  and `ChoiceControl` (choice params); for `time.looper` the editor replaces the
  knob grid with a **looper control row**:
  * banner: block name + current state word (`RECORDING`, `PLAYING`, `OVERDUBBING`,
    `STOPPED`/cleared label) with colour per state;
  * the loop panel: a progress bar showing `looper_frac` (0–100 %) plus the
    remaining-time readout (`mm:ss`);
  * 5 **momentary trigger buttons** in a row: Record, Play, Overdub, Stop, Clear —
    amber "down" state, `aria-pressed`/`role="button"` focus ring, no label that
    clips (short pill labels);
  * the existing knobs (`time`, `feedback`, `mix`, `level`) below, then two
    small toggle chips (`half`, `reverse`) — the same style as `ChoiceControl`
    with 2 options;
  * the block's knob labels and readouts use the engine's `unit` strings
    (`ms`, `%`) exactly like every other block.
* Layout is a single column that fills the editor width: banner row → progress
  row → trigger row → knob row(s). At narrow widths (phone) the trigger row
  wraps to two rows; nothing in the looper row may push the block banner off
  screen (`min-w-0` on the editor as per the existing UI).
* The `Looper` panel must keep the focus flow of the app: Tab moves between
  block cards, the editor receives focus, Tab then moves through the five
  trigger buttons.

---

## 8. Out of scope (deliberately not specified here)

* No stereo/multi-voice looper; the loop is mono per channel, exactly like every
  other block in the rig (a channel is a linear strip).
* No per-loop save as a named preset; the loop stays a *live state* of the block,
  stored in the project's preset JSON as a node's `params` (the buffer never
  serialises — it is reconstructed on load/reset).
* No footswitch/MIDI transport mapping in this card; the triggers are engine
  params, and any external control maps by setting the same params.
* No UI icon override (see the existing `Icon.tsx` `BLOCK_GLYPHS` selection rule;
  the Time category hue is blue, so `time.looper` inherits it).

## 9. Files this changes

| Area | Files |
|---|---|
| Engine (register + block + tests) | `graph/src/blocks.rs` (add `Loop`, `LoopBuffer`, register in `builtin_registry`), `graph/tests/looper.rs` (new), `docs/looper.md` (this doc) |
| UI | `ui/src/components/LooperEditor.tsx` (new), `ui/src/components/Editor.tsx` (dispatch to it), `ui/src/protocol.ts` / `ui/src/state.ts` (snapshot + event types), cypress (UI card) |
