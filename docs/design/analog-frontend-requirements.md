# Analogue front-end — requirements, gain structure & test method (US-6.1)

**Card:** US-6.1 Analogue front end (t_c99655df)
**Owner:** pedal-architect
**Status:** spec card — *requires a human hardware/product decision before any circuit build*.
**Supersedes / extends:** `docs/ANALOG-FRONTEND.md` (beginner's guide) and `docs/ARCHITECTURE.md` §2/§4.

This document turns the US-6.1 acceptance statement into measurable requirements.
It is architecture/requirements only: it does not contain a schematic or BOM, and it
must **not** be read as proof that the front end survives a real instrument. A
reviewed schematic + BOM and bench validation are the gating items at the end.

---

## 1. Scope

**In scope**
- Instrument input (JFET buffer + gain stage) reading passive and active pickups.
- Anti-alias low-pass in front of the codec ADC.
- Input protection (ESD, overvoltage, hot-plug, hot signal).
- Power distribution and supply rails.
- A bench test method with pass/fail acceptance criteria.

**Out of scope (deliberately left for later cards)**
- Output stage / reconstruction filter (already covered in `docs/ANALOG-FRONTEND.md`
  §4 and `docs/ARCHITECTURE.md` §2).
- PCB layout, enclosure, mounting, physical jack placement.
- MCU pinout (there is no MCU in the active path of this repo — see `docs/ARCHITECTURE.md` §1).

---

## 2. Measurable input assumptions

Every figure below is an assumption to be confirmed by measurement on the target USB
audio interface before the bench test is finalised. The test method in §7 measures
against these.

| Source | Typical open-circuit voltage | Source impedance | Notes |
|---|---|---|---|
| Passive humbucker / single-coil | 60–400 mV RMS (peaks ~85–565 mV) | 2–10 kΩ (can rise at low freq) | High-Z source: must see ≥500 kΩ, target 1 MΩ |
| Active pickup | 0.6–2 V RMS | 100–500 Ω | Needs attenuation/care so the following stage does not clip |
| Instrument line (USB interface DI) | 0.5–1.2 V RMS | 10–600 Ω | Nominal 0.5–1 V RMS |
| Codec LINEIN capability | full scale ≈ 1.23 V RMS (0 dBFS) | — | Biased to GND; the ADC sees the difference between +V and −V rails |

**Rules**
- The first element the signal meets must have input impedance **≥ 500 kΩ**, target
  **1 MΩ** (see `docs/ANALOG-FRONTEND.md` §1). Below 500 kΩ the highs die on a passive
  pickup.
- The codec's **ADC input** must see **0.3–0.7 V RMS nominal**, with peak headroom of
  at least **6 dB** before clipping.
- The ADC is a pseudo-differential, single-ended input: bias the centre of the rail and
  couple the signal with a DC-blocking capacitor. Never feed the codec's positive rail
  directly into the ADC input pin.

---

## 3. Gain structure

```
guitar ──► [JFET input buffer] ─► [variable gain stage] ─► [anti-alias LPF] ─► codec ADC
```

1. **JFET buffer** — unity gain, ≥1 MΩ input impedance, low output impedance. Sets the
   front-end ZIN and presents a low-Z source to the rest of the chain. Use a source
   follower (2N5457 / J201 / MPF102) or a TL072 non-inverting buffer (1 MΩ input
   resistor).
2. **Variable gain stage** — adjustable around **2×–12×** (dual supply, or single 9 V
   biased at half-supply 4.5 V) so that any source can be brought to the target 0.3–0.7
   V RMS without the user fiddling. This is the pedal's main "volume" number, mirrored
   as the UI input gain / trim control.
3. **Anti-alias LPF** — 2nd-order (12 dB/oct), ~20 kHz low-pass (Sallen-Key, 10 kΩ /
   10 kΩ, 820 pF / 820 pF) ahead of the 48 kHz codec.
4. **DC blocking** — couple the gain stage's output to the ADC input with a 10 µF+
   electrolytic to bias and a 100 nF ceramic to ground.

**Level budget (reference numbers only; verify on the real interface)**

| Node | Target |
|---|---|
| Pickup open-circuit peak | 0.09–0.57 V (passive) / up to 2.8 V (active) |
| After JFET buffer (low-Z output) | ≈ pickup peak voltage |
| After variable gain (nominal) | 0.3–0.7 V RMS |
| After gain, peak | ≤ 0.9 V RMS |
| At codec ADC | ≤ 0.9 V RMS, i.e. ≥ 6 dB headroom to 1.23 V RMS FS |

If the real codec's LINEIN sits at a different full-scale voltage, re-run the level
budget and confirm the headroom rule (no more than 6 dB below full scale on peaks).

---

## 4. Clipping / headroom criteria

The front end must not clip on realistic instrument output, and must have headroom to
absorb an accidental hot line level.

- **No clipping on instrument peaks:** for the full range of sources in §2, peak
  levels at the ADC must stay below full scale with **≥ 6 dB** of headroom over a
  10 s continuous test.
- **No clipped harmonics in the audible band:** run a 1 kHz test tone up to the
  clipping point and measure THD+N. Above the usable headroom the 2nd/3rd harmonics of
  a clipped ADC must not exceed the pre-clip noise floor by more than 6 dB.
- **Hot line level:** a 1.23 V RMS + 6 dB (i.e. +6 dBFS) line injection at 1 kHz must
  return full scale without intermod or distortion products exceeding 1% THD+N in the
  20–20 kHz band.
- **No clipping when adjusting the gain trim:** from the bottom to the top of the
  variable-gain range, the peak level at the ADC must ride over the gain change without
  clipping and without disappearing into the noise.

---

## 5. Protection limits

These are the damage limits the front end must enforce before they can reach the codec
or the 9 V supply. They are **protective circuit requirements**, not measured-analogue
levels.

| Threat | Limit | Means |
|---|---|---|
| ESD contact (IEC 61000-4-2) | ±8 kV | ESD-safe connector + 1–10 kΩ series at the jack terminal + 100 nF ceramic to GND adjacent to the jack |
| ESD air (IEC 61000-4-2) | ±15 kV | Same as above; use an ESD-protected audio jack |
| Input overvoltage | above +12 V / below −12 V | Clamp diodes on the jack terminal (not on the buffer gate) — clamp to +12 V and to GND; add a 100 nF + 10 Ω ripple filter |
| Hot-plug audio thump | step ≥ 2 V across the ADC in < 1 ms | Gate/soft-start the codec ADC (host-side soft unmute recommended); add a DC blocker (high-pass ≥ 20 Hz) at the input if the codec lacks one |
| Ground-loop hum | ≥ 60 dB rejection of 50/60 Hz | Star ground; analogue ground joined to digital ground at one point; shielded audio cable to the interface |
| Power sequence | regulator latches / 3.3 V rail driven from 9 V | Use a proper 9 V → 3.3 V regulator; never feed 9 V into a 3.3 V board (see `docs/ANALOG-FRONTEND.md` §5) |

**Do not** put ESD/overvoltage clamp diodes directly on the op-amp or JFET gate —
they would drag the input impedance and change the tone. Clamp at the jack terminal
and let the buffer (with its 1 MΩ resistor to ground) be the last element in the chain.

---

## 6. Supply rails

- **Input power:** 9 V DC, centre-negative barrel jack or battery.
- **Regulate:** 9 V → 5 V (LDO), then 5 V → 3.3 V (LDO) for the logic and the codec
  I2C. Never feed 9 V into a 3.3 V board.
- **Decoupling:** 100 nF ceramic (0.1 µF) at every IC power pin, plus 10–100 µF bulk
  near the regulator. Decouple the codec I2C supply independently.
- **Measure at build:** +9 V, +5 V, +3.3 V, and GND at the jack — each must stay within
  ±5% of nominal under full output and with the gear connected.

---

## 7. Bench test method (the acceptance test)

This is how US-6.1 is verified on real hardware. It requires a real instrument, a
signal generator, a scope / calibrated AC voltmeter, and the finished pedal. A
simulation of the codec cannot substitute: run the sections below in order and record
every measured value.

### 7.1 Power-up
1. Apply 9 V; measure +9 V, +5 V, +3.3 V at the jack and at each IC. Record values
   (must be within ±5% of nominal, and settle within 100 ms).
2. Confirm no 3.3 V pin is driven by 9 V.

### 7.2 Input impedance (AC method)
1. Apply a 1 kHz, 0.1 V RMS sine from a signal generator through a known source
   impedance (e.g. 1 kΩ).
2. Measure voltage across the input jack with a high-impedance scope probe.
3. Repeat with two different known load impedances (e.g. 100 kΩ and 1 MΩ) shunting
   the input.
4. Solve for ZIN from the two voltage readings. **Pass: measured ZIN ≥ 500 kΩ, target
   ≥ 1 MΩ.**

### 7.3 Gain staging
1. Set the variable gain to its nominal mid position.
2. With a passive pickup connected and strummed, measure the peak at the ADC input.
   **Pass: 0.24–1.4 V peak** (0.3–0.7 V RMS) or, for any source in §2, the value that
   keeps the level meter in the green zone.
3. Repeat with an active pickup.

### 7.4 Clipping test
1. Sweep the signal generator from -60 dB to +10 dB above nominal.
2. Find the clip point: the level at which the ADC zero-cross region or THD+N jumps.
3. **Pass: clip point is ≥ 6 dB above the measured peak of the loudest realistic
   instrument, with no clipping of the 0.3–0.7 V RMS nominal.**

### 7.5 Hot-signal / hot-plug test (the US-6.1 acceptance statement)
1. Power the pedal **with** a live instrument cable connected (cold/ hot plug).
2. Listen (and record with a scope) for pop/crackle at connect/disconnect.
3. Play a loud riff through the live input; verify:
   - no pop at the moment of connection;
   - no ADC clipping on the riff peaks;
   - signal present on the UI meter (0.3–0.7 V RMS) with the gain trim centred.
4. **Pass:** no pop, no clipping, healthy meter on a loud riff.

### 7.6 Hot-swap / power-on test
1. Connect the cable to a already-powered pedal, then reverse: power on with cable
   connected (7.5). Both directions must behave identically.
2. Optional but recommended: connect a spare instrument via an **unpowered** cable while
   the pedal is on, to check for ground-loop hum (≥60 dB rejection).

### 7.7 Report
Record for each section: measured value, limit, and PASS/FAIL. The record is the
acceptance evidence for US-6.1.

---

## 8. Acceptance criteria (US-6.1, verbatim)

> Passive and active pickups both read at a healthy level with no clipping, and the
> input survives a hot signal.

**Measurable translation**
- Passive and active pickups each produce **0.3–0.7 V RMS** at the ADC with the gain
  trimmed centrally (§7.3).
- ADC peak level stays **≥ 6 dB below full scale** across the whole source range,
  with no clipping over 10 s (§4, §7.4).
- Connecting a live instrument cable into a powered pedal produces **no pop** and no
  ADC saturation; a loud riff reads healthy on the meter (§7.5).
- Protection limits in §5 are enforced; supply rails in §6 stay within ±5% (§7.1).

---

## 9. Open items — need a human decision

1. **Bench build vs. document-only** (the only remaining blocker): does the team
   commit to a physical bench build — reviewed schematic + BOM (see
   `docs/design/analogue-frontend-circuit.md` §8), then the §7 bench test — or keep
   the front end as a documented design guide with no build this phase?

The other two open items from the previous spec have been decided and are
recorded in `analogue-frontend-circuit.md` §9 (codec = SGTL5000 per
`ARCHITECTURE.md` §2; variable gain = MCP42H-101 digital pot 2×–12×, driven by the
UI input-gain control).
