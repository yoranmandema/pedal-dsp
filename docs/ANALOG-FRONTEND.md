# Analogue front-end — a beginner's guide

The digital side is easy to simulate. The analogue side is where pedals go
wrong: noise, hum, pops, and blown inputs. This is the practical version.

---

## 1. The input problem: impedance

A passive guitar pickup is a high-inductance source. It *must* see a high
input impedance or the high frequencies die.

| Circuit | Input impedance | Result |
|---|---|---|
| Direct into a codec LINEIN | ~10–30 kΩ | Thin, dead, lifeless |
| Into a 1 MΩ buffer | ~1 MΩ | Correct — this is what a real pedal does |

**Rule: the first thing your signal meets must be ≥ 500 kΩ, ideally 1 MΩ.**

### The JFET buffer (the classic solution)

A single JFET source follower gives ~1 MΩ input impedance, unity gain, and a
low output impedance to drive the rest of the circuit.

```
         +9V
          │
         R_D (10k)
          │
gate ─────┼───► source ──► output (via 1µF)
          │
         R_S (1k)
          │
         GND
```

* Gate: 1 MΩ resistor to ground (sets the input impedance), plus a 100 pF
  capacitor to tame radio-frequency pickup.
* Source: 1 kΩ to ground, output taken from the source through a DC-blocking
  capacitor.
* Drain: 10 kΩ to +9 V.

**Parts:** 2N5457, J201, or MPF102 (any small N-channel JFET). A TL072 can
also be used as a non-inverting buffer with a 1 MΩ input resistor, but the
JFET is the traditional choice and sounds right.

---

## 2. Gain staging

The codec's line input expects roughly **0.5–1 V RMS**. A guitar pickup puts
out **0.1–0.5 V** on peaks, more with an active pickup.

```
JFET buffer ──► [gain stage] ──► [anti-alias filter] ──► codec LINEIN
```

A simple non-inverting op-amp stage (TL072, NE5532, OPA2134) with a gain of
2–10× gets you into the right range. Use a dual supply or bias the input at
half-supply (4.5 V) for a single 9 V rail.

---

## 3. Anti-alias filter

The ADC samples at 48 kHz, so anything above 24 kHz folds back into the audio
band. A **2nd-order (12 dB/octave) low-pass at ~20 kHz** in front of the codec
removes the worst of it. The codec's internal filter does the rest.

Sallen–Key, unity gain, 10 kΩ / 10 kΩ, 820 pF / 820 pF → ~20 kHz.

---

## 4. Reconstruction filter (output)

The DAC output is a staircase. A **2nd-order low-pass at ~20 kHz** smooths it.
Same Sallen–Key topology.

Many codecs (including the SGTL5000) have a decent internal filter; an
external one is still good practice for a quiet output.

---

## 5. Power

Pedals run on **9 V DC** (the standard centre-negative barrel jack, or a
battery).

```
9V ──► [reverse-polarity protection] ──► [78L05 / LDO] ──► 5V ──► [LDO] ──► 3V3
                                                              └─► [charge pump] ──► -9V (for op-amps)
```

* **Never** feed 9 V straight into a 3.3 V board. Use a regulator.
* Decouple every IC: 100 nF ceramic at each power pin, plus 10–100 µF bulk
  near the regulator.
* The Pi 5 board wants 5 V on its USB/VIN pins; the audio interface supplies 3V3 for the codec — check the schematic before powering.

### Grounding

Hum and buzz come from ground loops.

* Use a **star ground**: all grounds meet at one point near the power input.
* Keep the digital ground and analogue ground joined at exactly one place.
* Use a ground plane on a PCB; on stripboard, use a continuous ground rail.

---

## 6. Switching and pops

* **True bypass** with a mechanical switch means the effect is physically out
  of the signal path. Simple, but the switching transient can click.
* **Buffered bypass** keeps a buffer always in the path; the effect is
  switched in software. More flexible, needs a mute relay or a click-free
  analogue switch.
* A **soft-start** on the codec (the SGTL5000 has a slow volume ramp) and a
  DC blocker at the input kill the "thump" when engaging.

---

## 7. Controls

| Control | Wiring |
|---|---|
| Potentiometer | 3V3 — wiper — GND; wiper to an ADC pin. 10 kΩ linear. Add 100 nF across the ends to stop crackles. |
| Expression pedal | Same as a pot, but a TRS jack: tip = wiper, one end to 3V3, the other to GND. |
| Footswitch | Footswitch on the audio interface's footswitch input, bound to a control through the UI's control-binding layer and debounced on the host (10–20 ms). |

**Always** put a 100 nF capacitor across a pot's ends and a 1 kΩ series
resistor on the ADC pin. It costs nothing and removes noise and ESD risk.

---

## 8. Enclosure

* Die-cast aluminium (Hammond 1590B is the classic "compact pedal" size).
* Inputs on the right, output on the left, power at the top.
* Keep the input and output jacks far apart to avoid oscillation.
* Use shielded cable for the audio paths; keep digital wiring short.

---

## 9. Debugging checklist

If it doesn't work, check in this order:

1. **Power** — is 3V3 actually 3V3? Is the regulator warm (shorted)?
2. **Ground** — is everything connected? A floating ground gives phantom
   behaviour.
3. **Audio interface** — does it enumerate and power up? Check its status LED
   first.
4. **Codec** — does I2C ack? Read the chip ID (should be 0xA0xx).
5. **I2S** — is MCLK present? Is BICK toggling? A scope or logic analyser
   answers this in seconds.
6. **Audio** — is the codec powered? Is the DAC unmute? Is the output filter
   connected?

A **logic analyser** (even a $10 one) with I2S decoding will save you hours.
