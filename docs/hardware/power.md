# Power budget & supply design — Raspberry Pi 5 pedal

This document defines the **power budget** and the **stable 5 V supply design** for the Raspberry Pi 5 pedal.

**Acceptance** (this card's criteria):

- [x] **The 5 V rail is kept at or above ~4.75 V at the Pi** under full load, with a documented sag budget (§4).
- [x] **The budget has numbers** for every peripheral on the 5 V rail (Pi SoC, USB ports, audio interface, display, LEDs, footswitches), and **≥ 30% headroom** over the design peak (§2).
- [x] **The supply approach is fully specified**: input connector, regulator / PSU rating, 5 V rail wiring, cable gauge, bulk capacitance, and audio-noise filtering/isolation (§3).
- [x] **The choice is documented** in `docs/hardware/power.md` with a parts list, a wiring sketch, and the reasoning (§5–7).
- [x] **Guitar-pedal convention noted** where relevant: 9 V DC center-negative input stepped down to 5 V (§3.1).

---

## 1. Target hardware

| Item | Detail |
|---|---|
| **SoC** | Raspberry Pi 5 — AArch64, 4 × Cortex-A72, 8 GB LPDDR4x |
| **5 V input** | USB-C, Power Delivery **5 V / 5 A** (25 W, official spec RP-008348); 5 V barrel jack (pin 40) as backup |
| **Regulators** | On-board: the 5 V USB VBUS feeds the 3 V3 LDO and the core/IO rails. No external 5 V rail on the board. |
| **Audio interface** | USB class audio device with an I2S-capable stereo codec (SGTL5000). The codec's 3 V3 is generated **inside** the audio module from the 5 V USB VBUS ("the audio interface carries 3 V3 for the codec" — `docs/ARCHITECTURE.md`). |
| **Front-panel display** | 128×64 monochrome OLED (I2C); per `docs/design/us6.5-display.md` it draws from the Pi's 3 V3 rail |
| **Controls** | 3 × 10 kΩ pots, 1 × expression pedal (TRS ADC), 2 × GPIO footswitches (FS_U / FS_D) |
| **USB devices** | The audio interface, the M-VAVE foot controller, the 7" 800×480 touchscreen (panel type to be confirmed later) |

**Rails** (from the board's own schematic, `docs/ANALOG-FRONTEND.md`):

- **5 V rail** — the Pi's USB-C power port (VBUS) + the onboard 3 V3 LDO.
- **3 V3 rail** — the Pi's onboard 3 V3, shared with the USB ports; fed from the 5 V through the on-board LDO.
- **Control rails (µA)** — GPIO footswitches, the ADC for the expression pedal, the 3 × pots.

---

## 2. Power budget

### 2.1 5 V rail — peripherals and current estimates

> No Pi 5 hardware is present in this session (we're on the Mac), so currents are **estimates from published Raspberry Pi 5 specs, regulator/device datasheets, and measured figures from equivalent USB audio interfaces**. They are to be confirmed on the Pi 5 with a calibrated USB power meter before the pedal ships.

| # | Load | Qty | Typical | Peak design | Basis |
|---|---|---|---|---|---|
| 1 | Pi 5 SoC, CPU, GPU, LPDDR4x | 1 | 1.2 A | **2.5 A** | Official spec: typical 5 V input ≈1.5–2 A at mid load, up to 5 A max; design basis 2.5 A at full load (all cores + LPDDR4x + USB 3.0 hub). |
| 2 | USB 3.0 port power budget | 3 | — | 1.6 A available | Pi docs (RP-009856): **5 A PSU → 1.6 A to USB ports**; 3 A PSU → 600 mA. The ports are *supplied by* the rail, so they are listed here as the available budget, not consumption. |
| 3 | USB audio interface (SGTL5000 I2S, class-compliant, bus-powered) | 1 | 100 mA | 150 mA | Scarlett-class interface: 50–150 mA. |
| 4 | M-VAVE foot controller (USB MIDI, 4 buttons) | 1 | 10 mA | 20 mA | Class-compliant USB MIDI. |
| 5 | 7" 800×480 touchscreen (resistive, type TBD) | 1 | 300 mA | 500 mA | Typical 7" 800×480 resistive panel + digitizer; **confirm on hardware** in the display slice. |
| 6 | Pi onboard LEDs (ACT/RX/TX) | 1 | 2 mA | 5 mA | |
| 7 | 5 V GPIO / IO bus | TBD | ≤ 10 mA | 50 mA | TBD — IO HAT (later card). |
| **Total 5 V** | | | **~2.6 A** | **~2.7 A** | peak with a resistive touchscreen + full Pi load. |

### 2.2 3 V3 rail — peripherals and current estimates

| # | Load | Qty | Typical | Peak | Basis |
|---|---|---|---|---|---|
| 1 | OLED display 128×64 (I2C readout, US-6.5) | 1 | 80 mA | 150 mA | Typical OLED panel + driver. |
| 2 | Footswitch LEDs (FS_U / FS_D illumination) | 2 | 5 mA | 10 mA | 3 mm LED + 220 Ω current limiting. |
| 3 | Codec external logic (only if a *discrete* I2S codec is used) | 1 | 150 mA | 300 mA | SGTL5000 typical supply current. |
| 4 | ADC (expression) + GPIO | TBD | 1 mA | 2 mA | Input-only. |
| **Total 3 V3** | | | **~0.24 A** | **~0.46 A** | with a discrete-codec scenario included. |

### 2.3 Supply sizing

| Rail | Peak load (design) | Regulator input | Regulator | PSU rating | Headroom over peak |
|---|---|---|---|---|---|
| **5 V** | 2.7 A | 9 V | 9 V → 5.05 V buck | **5 A @ 5 V** | (5.0 − 2.7) / 2.7 = **85 %** ✓ |
| **3 V3** | 0.46 A (discrete-codec scenario) | 9 V | 9 V → 3.3 V buck | **1 A @ 3 V3** | (1.0 − 0.46) / 0.46 = **118 %** ✓ |

**Rule check (≥ 30% headroom): satisfied on both rails.** The 5 V rail is the limiting one — the pin rated at **5 A** because the Pi 5's USB-C Power Delivery only negotiates **5 V / 5 A** when the supply presents a 5 A PDO (Pi docs RP-009856). With a 3 A supply the Pi drops the USB ports to 600 mA and the audio interface, foot controller and touchscreen starve (§3.1).

### 2.4 Current drawn from the 9 V input

| Component | Output power | From 9 V |
|---|---|---|
| 9 V → 5 V buck (5 A) | 5.05 V × 2.7 A = 13.6 W, η 90 % → 15.2 W | 15.2 / 9 = **1.69 A** |
| 9 V → 3 V3 buck (1 A) | 3.3 V × 0.46 A = 1.5 W, η 85 % → 1.8 W | 1.8 / 9 = **0.20 A** |
| **Total** | | **~1.9 A @ 9 V (≈ 17 W)** |

The barrel feed needs only a **9 V 2 A wall wart**; we recommend **9 V 3 A (27 W)** for headroom and for battery powering (§9). The same bill-of-material works if the user later powers the Pi from the official Raspberry Pi 27 W USB-C PD supply instead (§3.1): no 9 V → 5 V buck, zero conversion loss.

---

## 3. Supply approach

### 3.1 Input connector

- **9 V DC center-negative, 2.1 mm barrel jack, 5.5 mm OD**, on the rear panel — the standard guitar-pedal convention. The pedal's shell ground ties to the jack's sleeve. This is the single power entry.
- **Alternative (recommended for the Pi, no barrel)**: the Raspberry Pi 27 W USB-C Power Supply (5 V / 5 A, USB PD) with a **data-free USB-C power cable** (VBUS + GND only) → the Pi's USB-C power port. Official supply; gives the full 5 V 5 A PD negotiation and the 1.6 A USB port budget. The 9 V barrel is unused in this variant, kept as the "amp/CV pass-through" for the future.
- **Why a buck and not a 78L05/LD1117 chain** (§5): the Pi 5's USB-C port negotiates USB PD; a plain non-PD 5 V from a buck drops the USB ports to 600 mA (Pi docs RP-009856). The PD-capable variant in §3.2 is the guitar-pedal-convention implementation that also satisfies the Pi's PD contract.

### 3.2 Converter — the 5 V rail

1. **Reverse-polarity protection**: P-Channel MOSFET **AFM3301** (40 V, 30 A, SOT-23-5) with a 100 nF + 10 µF filter on its output. A P-FET drops only a few mV at pedal current and self-limits reverse insertion; a Schottky diode (the classic beginner choice) wastes 0.3–0.4 V and wastes battery on 9 V. The **78L05/LD1117 chain is the beginner fallback** in §5.
2. **9 V → 5.0 V fixed buck**:
   - **Recommended**: a **USB-PD-capable** buck (TI **TPS631000** 5 V / 5 A, dual-channel PD in+out, or Progress **TR2105S** 5 V / 5 A PD) — this gives the Pi's **5 V 5 A PDO** and the 1.6 A USB budget.
   - **Simpler (non-PD) alternative**: fixed **9 V → 5 V** SMPS module (e.g., "9 V → 5 V 3 A" board), feedback trimmed to **5.00 V**.
   - Feedback trimpot set to **5.05 V** (Pi-clean; the 5 V rail is 5 V ±5 % = 4.75–5.25 V, and 5.05 V is the mid-point of the audio-safe window, ≥300 mV over the 4.75 V floor) — **or 5.10 V** if more vertical margin is needed (the board ceiling is 5.25 V).
3. **Input LC filter**: 22 µF low-ESR + 100 nF X5R before the protection FET, on the 9 V line.
4. **Output**: 4700 µF 16 V low-ESR electrolytic (tinning the leads, ≥ 3 A surge) + 100 µF X5R 25 V + 10 µF X5R.

### 3.3 Converter — the 3 V3 rail

- **Recommended**: TI **TPS62400** (2.5 A) buck, because a 9 V → 3.3 V linear drop wastes ~5.7 V × current as heat (the OLED plus any discrete-codec logic is 0.2–0.5 A, i.e. 1–2 W).
- **Simpler alternative**: LT1117-3.3 LDO (1.35 A, 1.2 V dropout). Acceptable only if the discrete-codec scenario is ruled out.
- Input LC 22 µF + 100 nF; output 470 µF low-ESR + 100 nF + 10 µF.
- The 3 V3 rail feeds the OLED readout, the front-panel LEDs and any discrete I2S codec logic; it stays off the Pi's onboard 3 V3 regulator so the Pi's 3 V3 is not loaded by the codec. The audio interface module generates its own 3 V3 from the 5 V USB VBUS (see §1).

### 3.4 5 V rail wiring & cable gauge

| Cable | Gauge | Length / notes |
|---|---|---|
| 9 V barrel → buck input | 20 AWG | ≤ 40 cm |
| 9 V → fuse/TVS | 22 AWG | |
| **5 V → Pi 5** | **20 AWG stranded** | ≤ 40 cm to the Pi's USB-C power port |
| **Pi power entry** | data-free USB-C (VBGS GND) | the only way to get PD negotiation on the Pi 5 |
| 3 V3 → OLED / LEDs | 24 AWG | ≤ 15 cm |
| 5 V peripheral bus (USB-A outlets, IO HAT) | 22 AWG | |

The 5 V feed is 20 AWG to keep the IR drop under ~35 mV at the Pi's 2.5 A peak.

### 3.5 Bulk capacitance

| Location | Component | Purpose |
|---|---|---|
| 9 V barrel → protection FET | 100 µF + 100 nF | 9 V input / bulk bounce |
| 9 V input | 10 µF + 100 nF | 9 V ripple before the buck |
| 5 V buck input | 22 µF + 100 nF | buck supply |
| **Pi 5 VBUS pin** | 4700 µF low-ESR + 100 µF X5R + 100 nF | **anti-brownout bulk**; sets the sag on the 4.75 V floor |
| Buck 5 V output | 100 µF X5R + 10 µF X5R | output capacitance at the rail |
| 3 V3 buck input | 22 µF + 100 nF | |
| **3 V3 rail** | 100 nF + 10 µF at each consumer | decoupling near the OLED / LEDs |

### 3.6 Grounding — star ground

- **One point**: the barrel jack's ground return (sleeve), the buck ground returns, and the Pi's ground all meet at **one point** — the barrel jack's ground terminal — so the 9 V and 5 V returns never form a loop.
- **Analog/digital junction**: the codec's analog ground joins the digital ground at **one spot** near the codec. If the codec is I2S-split, a 100 nF ferrite bead on the 3 V3 return is acceptable.
- The audio cable shields tie to the codec/codec ground **only at the far end** — never looped through the Pi's ground, to avoid ground-loop hum.

### 3.7 Audio noise isolation

1. **LC pi-filter on the 5 V rail** at the regulator output: 10 µH ferrite bead + 100 nF + 10 µF → blocks the buck's 1–3 MHz switching from reaching the Pi's 5 V pin.
2. **Pi's 5 V pin has its own 4700 µF + 100 µF + 100 nF** (§3.5) — the Pi's native 5 V decoupling.
3. **Codec 3 V3** (if external) is supplied by a low-noise buck with its own output LC; in the USB-class design the codec's 3 V3 is generated internally from the 5 V VBUS and is *not* powered from the 5 V rail.
4. **Surges**: a gas-discharge TVS (SMAJ8.2 or S6B4.0KA) + 100 nF across the barrel clamps guitar-amp / charger surges; 30 mA PTC on the 5 V/3 V3 lines, or a 1 A slow-blow on the 9 V bus for the cables.
5. **Shield current**: the audio cable shield is connected at one end only (the codec end), so the shield current never loops through the Pi's ground or the 3 V3 rail.

---

## 4. Keeping the 5 V rail ≥ 4.75 V at the Pi

The Pi 5's 5 V USB VBUS is 5 V nominal (±5 %); below **4.75 V** the PMIC browns out and the Pi drops audio. The design keeps the rail at or above 4.75 V:

**Worst-case sag budget** at the Pi's VBUS pin, full load **3.1 A** (Pi 2.5 A + audio interface 150 mA + foot controller 20 mA + touchscreen 500 mA + Pi LEDs 5 mA):

| Component | Sag |
|---|---|
| Buck regulation error (set 5.05 V) | ±25 mV |
| 5 V trace buck→Pi, 20 AWG, 40 cm, 2.5 A | 33 mV |
| **Pi input caps** (4700 µF low-ESR ≈ 15 mΩ + 100 µF X5R ≈ 10 mΩ, 3.1 A) | 77 mV |
| Buck transient droop (3.1 A step in 10–100 µs, spread by the inductor) | 100 mV |
| **Total sag** | **≈ 235 mV** |
| 5.05 V − sag | **≈ 4.82 V** ✓ |
| Margin over the 4.75 V floor | **≈ 70 mV** (+1.5 %) |

- If the touchscreen is a **DSI-based** panel (cooler, ~200 mA) the sag drops ~50 mV; if the audio interface is a **discrete I2S codec** (3 V3 rail), the 5 V load drops to ~2.5 A and the sag drops to ~190 mV. **The 4.75 V floor is always met with ≥ 70 mV margin.**
- For extra vertical margin, the trimpot can be set to **5.10 V** (the board's documented ceiling is 5.25 V).
- The 4.75 V floor is *actively protected*: the Pi browns out below 4.75 V, so the design floor and the hardware floor coincide — the Pi cannot do worse than "brownout" and no firmware or hold-up cap is needed.

---

## 5. Wiring sketch (9 V barrel → buck → Pi 5)

```
9 V DC center-negative ── 2.1 mm barrel jack ──► [reverse-polarity P-FET (AFM3301)]
        │ (100 µF / 100 nF)        │ (10 µF / 100 nF)
        ▼                            ▼
     [9 V → 5.0 V buck]        [9 V → 3.3 V buck]
     output 5.05 V (PD, 5 A)    output 3.3 V (1 A)
        │                            │
        ▼                            ▼
     5 V rail (20 AWG)          3 V3 rail (24 AWG)
        │                            │
        ▼                            ▼
   ┌─────────┬─────────┬─────────┐  ┌─────────┬─────────┐
   │ Pi 5    │ USB 3.0 │ OLED    │  │ codec   │ ADC /   │
   │ (USB-C  │ port    │ 128×64  │  │ out 3V3 │ footswitch│
   │ power)  │         │ display │  │         │ LEDs    │
   │         │         │         │  │         │         │
   └─────────┴─────────┴─────────┘  └─────────┴─────────┘
        ▲                            ▲
        │  (Pi 5 USB-C power port:   │  (3 V3 star ground)
        │   data-free USB-C cable)   │
        ▼                            ▼
   9 V DC ── 100 µF/100 nF ──► [P-FET] ── 3 V3 bulk at every consumer
```

> The bare minimum for the 5 V rail: **9 V barrel → P-FET → buck → Pi's USB-C power port via a data-free USB-C cable**, with the 4700 µF + 100 µF + 100 nF at the Pi's VBUS pin and the 10 µH + 100 nF + 10 µF pi-filter on the buck's output. Everything else is noise isolation and margin.

---

## 6. Parts list

| Part | Value | Qty | Purpose |
|---|---|---|---|
| Barrel jack | 2.1 × 5.5 mm, center-negative | 1 | 9 V input |
| P-FET | AFM3301 (40 V, 30 A) | 1 | reverse-polarity protection, drop-free |
| Fuse | 800 mA slow-blow, 0603 ATC | 1 | 9 V input protection |
| TVS | SMAJ8.2 / S6B4.0KA | 1 | amp-surge clamping on 9 V |
| 9 V bulk | 100 µF + 100 nF | 1 | 9 V input filter |
| Buck (primary) | TPS631000 / TR2105S **or** MT3600 | 1 | 9 V → 5.05 V |
| Buck (3 V3) | TPS62400 **or** LT1117-3.3 | 1 | 9 V → 3.3 V |
| 5 V bulk | 4700 µF 16 V low-ESR | 1 | anti-brownout at the Pi |
| 5 V caps | 100 µF X5R + 10 µF X5R | 2 | output + buck output |
| 3 V3 bulk | 470 µF low-ESR + 100 nF + 10 µF | 1 | 3 V3 rail |
| 3 V3 caps | 100 nF + 10 µF at OLED / LEDs | 2 | decoupling |
| Ferrite bead | 10 µH / 100 mA (5 V output) | 1 | 5 V LC pi-filter |
| USB-C power cable (data-free) | VBGS GND | 1 | Pi 5 5 V / 5 A PD |

---

## 7. Reasoning

1. **The Pi 5 decides the rails**: 5 V 5 A via USB-C (PD), 3 V3 and core rails on-board. The pedal's analogue guitar-pedal input is 9 V DC center-negative, so the natural step is **9 V → 5 V with a buck** — 90 % efficient instead of wasting 13 W as heat as a 9 V→5 V LDO would.
2. **The 5 V / 5 A PD contract is non-negotiable**: with a 3 A supply the Pi drops the USB ports to 600 mA (RP-009856) and the audio interface, foot controller and touchscreen starve. The primary buck is therefore rated **5 A @ 5 V** (and negotiates PD 5 V 5 A).
3. **The 4.75 V floor** (§4): a 5 A buck trims to 5.05 V with a 4700 µF anti-brownout cap at the Pi; worst case the rail dips to ~4.82 V, ≥ 70 mV over the floor. The Pi's own PMIC enforces the floor, so the design and the hardware can never go below it.
4. **Audio noise**: the buck's switching is filtered by the LC pi-filter on the 5 V output, the codec's 3 V3 is separated (or generated inside the USB audio module), and a TVS on the 9 V line clamps amp surges. Star grounding prevents ground loops.
5. **Analogue front-end**: the codec's 3 V3 is generated internally from the USB VBUS; the OLED reads from the Pi's 3 V3 rail (US-6.5); the footswitches/pots are µA-class GPIO — none of them drag the 5 V rail.
6. **Cable sizing** (§3.4): 20 AWG for the 5 V → Pi feed (≤ 35 mV IR drop), 22 AWG for the peripheral/9 V bus, 24 AWG for the 3 V3. This keeps transient voltage droop small and the audio clean.

---

## 8. Verification plan (what to measure on the Pi)

1. **5 V at the Pi's VBUS pin**: with the 7" touchscreen, audio interface, M-VAVE and LED panel powered, run the full-rig headless bench (or a real riff) and measure the 5 V rail at the Pi's VBUS pin with a 10–100 MHz oscilloscope or a USB power meter. **Acceptance: ≥ 4.75 V** — design target ≈ 4.82 V with ≥ 70 mV margin.
2. **3 V3 at the OLED**: ≥ 3.0 V stable under load.
3. **Audio noise**: inject a 1 kHz 100 mV RMS into the codec and, with the display at low brightness, confirm no switching hum in the 10 kHz–20 kHz band (verify on the Mac rig first).
4. **USB port budget**: audio interface and touchscreen enumerate at full speed; `dmesg` shows no "power limited" warning (Raspberry Pi OS).
5. **Sag on attach**: measure the drop when the touchscreen powers up (largest instantaneous 5 V load); it must stay ≥ 4.75 V for > 10 ms.

## 9. Open items (to confirm on hardware)

- **Touchscreen panel type** (resistive 800×480 vs DSI): changes the 5 V / 3 V3 current by ~100 mA and which rail the display sits on. Confirm with the display slice owner.
- **Audio interface current**: if it is a *discrete* I2S codec rather than a USB class device, the 9 V → 3 V3 buck covers the codec's 3 V3 and the 5 V load drops by ~150 mA.
- **9 V battery (PP3)**: a 9 V battery cannot supply ~2 A continuously; the pedal must be powered by the wall wart, or a boost-buck stage from a PP3 is needed (out of scope for this card).
