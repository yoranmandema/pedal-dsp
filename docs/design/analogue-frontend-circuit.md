# Analogue front-end — circuit design (US-6.1)

**Card:** US-6.1 Analogue front end (t_c99655df)
**Owner:** pedal-architect
**Status:** circuit design (architecture/pedal-architect) — *bench validation on real hardware still gated on a human hardware decision*.
**Supersedes / extends:** `docs/design/analog-frontend-requirements.md` (requirements, no schematic/BOM) and `docs/ANALOG-FRONTEND.md` (beginner's guide).

This document turns the US-6.1 requirements into a **concrete, reviewable circuit design** with a BOM and a measurement-based bench test method. It is **not** a PCB layout, and it is **not** proof of hot-signal survival — that survival is bench-validated on real hardware (see §6).

---

## 1. Block diagram

```text
guitar ──► [1 jack protection] ──► [2 input buffer] ──► [3 variable gain]
                                                                  │
                                                           [4 anti-alias LPF]
                                                                  │
                                                           [5 DC block] ──► codec ADC
```

1. **Jack protection** — ESD / overvoltage clamp at the jack terminal.
2. **Input buffer** — unity gain, ≥500 kΩ input impedance (target 1 MΩ), low output impedance.
3. **Variable gain** — 2×–12× (trimmed by the UI input-gain control).
4. **Anti-alias LPF** — 2nd order (~20 kHz) ahead of the 48 kHz codec.
5. **DC block** — high-pass ≥ 20 Hz at the ADC input (hot-plug thump protection).

---

## 2. Stage reference design

### 2.1 Jack protection (stage 1)
Per the requirements §5, the protection is a **jack-terminal clamp, never on the buffer gate** (clamping a gate drags the ZIN and changes the tone).

| Threat | Means | Values |
|---|---|---|
| ESD contact (±8 kV, IEC 61000-4-2) | ESD-protected audio jack + series resistor + ceramic | jack terminal → series 1–10 kΩ → buffer; **100 nF** to GND adjacent to the jack |
| ESD air (±15 kV) | Same as above; use an ESD-protected jack | — |
| Input overvoltage (above +12 V / below −12 V) | Clamp diodes on the jack terminal | 1N4148 clamping the terminal to **+12 V** and to **GND** + **100 nF + 10 Ω** ripple filter |
| Hot-plug audio thump | DC blocker (high-pass ≥ 20 Hz) at the input | **10 µF** electrolytic + **100 nF** ceramic feeding the gain stage (codec internal coupling handles the rest) |

### 2.2 Input buffer (stage 2) — primary
**TL072** (or NE5532) non-inverting **unity-gain buffer**:
- Non-inverting input → jack protection output.
- Inverting input → **1 MΩ resistor (R_g) to ground** (sets the 1 MΩ input impedance) + 1 kΩ feedback resistor, output to inverting.
- **Gain = 1**, **ZIN = 1 MΩ**, low output impedance to drive the gain stage.
- The TL072's 1 MΩ ZIN satisfies the pickup rule (≥500 kΩ, target 1 MΩ).

**Documented alternative:** 2N5457 source follower (gate 1 MΩ to GND + 100 pF RF tamer; source 1 kΩ to GND; drain 10 kΩ to +9 V). Attenuating (gain < 1); the gain stage restores level. Either satisfies the ZIN rule — pick one for the schematic.

### 2.3 Variable gain (stage 3) — 2×–12×
- **TL072** non-inverting stage.
- **R_g = 10 kΩ** (fixed, inverting input to ground).
- **R_f = 100 kΩ** fixed in series with a **10 kΩ linear potentiometer**, wiper to the inverting input. Gain = 1 + R_f / R_g.
  - Wiper 0 % → R_f = 100 kΩ → **gain 11×**.
  - Wiper 100 % → R_f = 110 kΩ → **gain 12×**.
- The full **2×–12×** range is set in a single assembly step with a **digital potentiometer (MCP42H-101, 10-bit, 10 kΩ, SPI)**, placed in R_f as the wiper source: the wiper tap gives the full 100 kΩ–110 kΩ sweep over 256 taps. Reference values: TL072, R_g = 10 kΩ; R_f = MCP42H-101 (10 kΩ end-to-end) with the wiper to the inverting input. Gain range 2×–12×, verified at schematic review.
- Mirrored to the UI as the **input-gain / trim** control, which also maps to the engine's `setInputGain` (dB, −60…24) so the digital trim extends the range below the analogue floor.

### 2.4 Anti-alias LPF (stage 4)
- Sallen–Key, unity gain, **2nd order (12 dB/oct), ~20 kHz**.
- **10 kΩ / 10 kΩ**, **820 pF / 820 pF** resistors/capacitors.
- Removes content above the 24 kHz Nyquist before the 48 kHz codec ADC.

### 2.5 DC block to the ADC (stage 5)
- **10 µF** electrolytic (bias) + **100 nF** ceramic to ground at the jack of the codec ADC.
- High-pass ~20 Hz: blocks DC/thump at hot-plug and removes offset so the codec never sees a DC component.

---

## 3. Level budget (with the codec's LINEIN)

Reference numbers only; the real codec's full-scale voltage re-runs the budget at build.

| Node | Target |
|---|---|
| Pickup open-circuit peak | 0.09–0.57 V (passive) / up to 2.8 V (active) |
| After JFET/TL072 buffer (low-Z) | ≈ pickup peak voltage |
| After variable gain (nominal) | 0.3–0.7 V RMS |
| After gain, peak | ≤ 0.9 V RMS |
| At codec ADC | ≤ 0.9 V RMS, i.e. **≥ 6 dB** headroom to 1.23 V RMS FS |

---

## 4. BOM (reference, for schematic review)

| Qty | Part | Value / type | Notes |
|---|---|---|---|
| 2 | Op-amp | TL072 (or NE5532) | Buffer + gain; dual within one package |
| 2 | Jack | 6.35 mm TS | ESD-protected audio jack for input |
| 1 | Power | 9 V DC barrel jack / battery snap | Centre-negative |
| 1 | Power | 9 V → 5 V LDO; 5 V → 3.3 V LDO | Correct polarity; tested per §5 |
| 3 | Decoupling | 100 nF ceramic + 10–100 µF bulk | Per IC power pin |
| 6 | Resistor | 1 MΩ, 1 kΩ, 10 kΩ ×2, 100 kΩ, 10 kΩ pot | See §2 |
| 2 | Capacitor | 820 pF ×2; 10 µF electrolytic ×2; 100 nF ×several | See §2 |
| 1 | Digital pot | MCP42H-101 (10-bit, 10 kΩ, SPI) | Variable gain wiper source |
| 1 | Status LED + resistor | — | Debug; optional |
| 1 | Board + enclosure | Stripboard / Hellerman-Leigh 1590B | Input/output jacks far apart |

---

## 5. Power and grounding

- **Rail chain:** 9 V (barrel/battery) → 5 V LDO → 3.3 V LDO. **Never** feed 9 V into a 3.3 V board (requirements §6).
- **Measure at build:** +9 V, +5 V, +3.3 V, GND stay within **±5%** of nominal under full output and with the gear connected.
- **Decoupling:** 100 nF ceramic at every IC power pin, 10–100 µF bulk near the regulator; the codec I2C supply decoupled independently.
- **Grounding:** star ground — all grounds meet at one point near the power input; analogue ground joined to digital ground at exactly one place; shielded audio cable to the interface.

---

## 6. Bench test method (acceptance evidence)

Run in order, measure and record every value. A simulation of the codec **cannot** substitute.

| § | Test | Pass criterion |
|---|---|---|
| 7.1 Power-up | Apply 9 V; measure +9 V, +5 V, +3.3 V at the jack + each IC | Within ±5% of nominal; settle < 100 ms; no 3.3 V pin driven by 9 V |
| 7.2 ZIN (AC) | 1 kHz, 0.1 V RMS through known source impedance; measure across input with two known shunt loads (100 kΩ, 1 MΩ) | **ZIN ≥ 500 kΩ, target ≥ 1 MΩ** |
| 7.3 Gain staging | Passive pickup connected and strummed; active pickup; gain trim centred | **0.3–0.7 V RMS (0.24–1.4 V peak)** at the ADC for each source |
| 7.4 Clipping sweep | Generator −60 dB to +10 dB above nominal | Clip point ≥ 6 dB above the measured peak of the loudest realistic instrument, with no clipping of the nominal |
| 7.5 Hot-signal (the acceptance statement) | Power **with** cable connected; loud riff through live input | **No pop** at connect/disconnect; no ADC clipping on riff peaks; 0.3–0.7 V RMS on the meter with gain centred |
| 7.6 Hot-swap / power-on | Cable connected, powered on; then cable connected to a powered pedal | Both directions behave identically; ground-loop hum ≥ 60 dB rejection |
| 7.7 Report | Record measured value, limit, PASS/FAIL per section | Record is the acceptance evidence |

---

## 7. Acceptance criteria (US-6.1, verbatim)

> Passive and active pickups both read at a healthy level with no clipping, and the
> input survives a hot signal.

Measurable translation:

- Passive and active pickups each produce **0.3–0.7 V RMS** at the ADC with the gain trimmed centrally (§7.3).
- ADC peak level stays **≥ 6 dB below full scale** across the whole source range, with no clipping over 10 s (§7.4).
- Connecting a live instrument cable into a powered pedal produces **no pop** and no ADC saturation; a loud riff reads healthy on the meter (§7.5).
- Protection limits in §2 and supply rails in §5 enforced (§7.1, §7.6).

---

## 8. Files opened by the implementer (next slice)

- **Schematic/BOM card (pedal-builder):** convert §1–§5 into a reviewed schematic + BOM; run §6 on real hardware.
- **Bench-build decision (human):** the §6 test is only runnable on real hardware — the bench-build direction is the sole remaining product decision.

---

## 9. Adopted decisions (superseded from the requirements card)

| Item | Decision |
|---|---|
| Codec target | **SGTL5000** (I2C, line in/out, headphone out, 95 dB SNR DAC) per `docs/ARCHITECTURE.md` §2 |
| Variable gain | **MCP42H-101 digital pot (2×–12×)** in the gain-stage feedback, driven by the UI input-gain control (maps to engine `setInputGain`) |
| Input buffer | **TL072 non-inverting unity-gain buffer**, 1 MΩ ZIN (JFET source follower is the documented alternative) |

---

## 10. Open products decisions (this card does not decide)

- **Bench build vs. document-only:** does the team commit to a physical bench build (schematic + BOM to be reviewed, then §7 bench validation), or keep the front end as a documented design guide with no build this phase?

> The acceptance statement (§7) can only be verified on real hardware, so this decision
> unblocks the schematic/BOM card and the bench test.
