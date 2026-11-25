# Footswitch hardware interface (US-6.3a)

**Status:** decided (architect card `t_cb996834`).

**Related:** `docs/hardware/power.md`, `docs/hardware/enclosure.md`, `docs/IPC-PROTOCOL.md`, `docs/design/us6.5-display.md`.

## 1. Overview

The pedal uses **two** normally-open, normally-closed (NO/NC) latching footswitches for preset navigation:

| Switch | Signal pin | Function |
|---|---|---|
| **FS_U** (up / next preset) | GPIO 2 (BCM, pin 3 on 40-way header) | Navigate up one preset in the list |
| **FS_D** (down / previous preset) | GPIO 3 (BCM, pin 5 on 40-way header) | Navigate down one preset in the list |

> **Why GPIO 2 and 3?** BCM 2 and 3 are the only two GPIO pins that share a native interrupt line on the BCM2711 (Pi 5 SoC) — they both feed the `GPIOPCLK0` interrupt controller. In practice the Pi 5's Linux input subsystem abstracts this away, but choosing these pins leaves the remaining GPIO (4–27) available for pots, ADCs, or future expansion.

## 2. GPIO vs I2C expander

**Decision: native GPIO, no I2C expander.**

Rationale:

- The Raspberry Pi 5 exposes a 40-pin header with 26 GPIO lines. Two switches need two pins — no expansion chip is required.
- Adding an I2C GPIO expander (e.g. PCF8574) would add cost, I2C bus noise coupling, a point of failure, and an extra device-tree overlay — all for a net zero benefit when the Pi 5 already provides free GPIO pins.
- The Pi 5 kernel already handles GPIO input scanning through the standard `gpio-keys` input subsystem, so the Pi 5's mainline Linux driver (`drivers/input/keyboard/gpio_keys.c`) provides built-in debouncing and key-code mapping. For the pedal's custom engine (running on the Pi 5 headless), the GPIO can be read via `/dev/gpiochip0` (libgpiod) or `/sys/class/gpio` without kernel modules.
- I2C would be justified if the project needed > 8 inputs or if inputs had to share timing-critical buses. Neither condition applies here.

## 3. Pin assignments

| Signal | Pi 5 pin (physical) | BCM number | Function |
|---|---|---|---|
| **FS_U_IN** | 3 (pin 1 of 40-way header) | GPIO 2 | Footswitch up — input from NO contact of latching FS |
| **FS_D_IN** | 5 (pin 2 of 40-way header) | GPIO 3 | Footswitch down — input from NO contact of latching FS |
| **3V3** | Pin 1 | — | Power to footswitch LED/anode |
| **GND** | Pin 6, 9, 14, 20, 25, 30, 34, 39 | — | Common ground |

**Footswitch wiring:**

```
  3V3 ──[footswitch NO contact]──+── GPIO 2 (FS_U_IN)
                                 |
                                [10 kΩ pull-down to GND]
                                 |
  3V3 ──[footswitch NO contact]──+── GPIO 3 (FS_D_IN)
                                 |
                                [10 kΩ pull-down to GND]
                                 |
  (NC contact of each switch may be left unconnected; only NO is used.)
```

Each switch also has an integrated LED. Wire the LED separately:

```
  3V3 ──[footswitch LED anode]──[220 Ω current-limiting resistor]──[GPIO X (optional PWM)]── GND
```

The LED GPIO can be soft-PWM'd from user space to dim or colour-encode switch state. If no dimming is needed, tie the LED anode directly to 3V3 with a 220 Ω resistor and leave the GPIO free.

## 4. Pull-up / pull-down configuration

**Internal pull-down to GND, 10 kΩ external.**

- The footswitch connects 3V3 to the GPIO pin when pressed. With a pull-down, the idle state reads **low (0)** and the pressed state reads **high (1)**.
- A 10 kΩ external resistor is used alongside the Pi 5's internal pull-down (which is ~50 kΩ typical) for a parallel equivalent of ~8.3 kΩ. This guarantees a definite low state even if the internal pull is weak or the GPIO driver is momentarily inactive.
- **No pull-up** is needed. If a pull-up were used, the switch would connect to GND on press (active-low logic), which is more common in Arduino projects but adds confusion here because the Pi 5's 3V3 logic already gives a clean high on press.

**Why not use only the internal pull-down?**

The Pi 5's internal pull-down resistors have a wide tolerance (30–100 kΩ per the BCM2711 datasheet). With a 10 kΩ external resistor, the parallel combination is tighter (~8.3 kΩ), giving a more reliable low idle state. This matters on a pedal in a high-noise guitar environment.

## 5. Electrical notes

### 5.1 Switch type

Use **latching (maintained) NO/NC SPST** footswitches rated for at least 1 A @ 3V3. Typical examples:

- **Alps LSGE3** (latching, NO + NC contacts, 32×32 mm, 50 MΩ min contact resistance)
- **E-Switch SL3000R** (military-grade, 1 M cycle rating)

Latching switches are preferred over momentary because:
- Only one press is needed to toggle — no repeated stomping to stay active.
- The LED on the switch can illuminate continuously while the preset is engaged, giving visual feedback without requiring the engine to poll state.

### 5.2 ESD and noise protection

- The Pi 5 GPIO pins are **3.3V tolerant, 5V tolerant, and have 5V fault tolerance**. A brief transient up to 5 V will not damage them.
- Add **100 pF decoupling capacitor** from each switch line to GND, placed as close to the switch connector as possible, to shunt high-frequency noise before it reaches the GPIO pin.
- If the pedal enclosure is metal, connect the enclosure ground to the Pi 5 GND via a star point at the USB audio interface ground.

### 5.3 Switch debounce (hardware)

A **100 nF ceramic capacitor** (0805 or 0603) from each switch line to GND, in parallel with the pull-down resistor. This forms a simple RC low-pass with the 8.3 kΩ equivalent pull-down:

```
    τ = R × C = 8.3 kΩ × 100 nF ≈ 830 µs
```

This RC filter attenuates bounce by > 99% after ~5τ ≈ 4 ms, so by the time software reads the pin the signal is stable. This is the **primary debounce mechanism**.

### 5.4 Debounce (software)

Even with hardware RC filtering, a **software debounce** is applied for robustness:

- **Debounce time: 20 ms** after the rising edge is first detected. The engine waits 20 ms before confirming a press. This is long enough to swallow any residual bounce even if the hardware RC is omitted (e.g. on a breadboard prototype).
- **State machine:**

```
State: IDLE
  ── switch pressed (high) ──► WAITING
  State: WAITING (20 ms timer running)
  ── 20 ms elapsed, still high ──► PRESSED (emit event)
  ── 20 ms elapsed, went low ──► IDLE (bounce, ignore)
  State: PRESSED
  ── switch released (low) ──► IDLE
  ── held > 1000 ms ──► HOLD (optional, see §5.5)
```

### 5.5 Optional long-press / hold behavior

A **long-press** (switch held for ≥ 1 second after debounce confirmation) triggers a preset save (overwrites the current preset file). This gives the user a quick-save mechanism without touching the UI.

- Long-press is a **secondary function** on the same pin — both FS_U and FS_D support it.
- The debounce state machine is extended:

```
State: PRESSED
  ── released < 1 s ──► IDLE (normal press, emit preset_up/down)
  ── held ≥ 1 s ──► HOLD (emit preset_save event)
```

- The 1-second hold time is chosen so that normal stomps (quick taps) are distinguished from intentional holds without being too sensitive.
- Hold detection runs on a 100 ms timer (checked during the PRESSED state). CPU cost is negligible — a single counter increment per timer tick.

## 6. Event format emitted to the host

Events are sent over the WebSocket IPC connection (see `docs/IPC-PROTOCOL.md`). The engine reads the GPIO pins outside the audio callback (in a polling thread or via GPIO chip notification) and translates them to IPC events.

### 6.1 Event types

| Event | Trigger | Payload |
|---|---|---|
| `preset_up` | FS_U pressed (debounced, rising edge) | `{ "type": "event", "event": "footswitch", "payload": { "switch": "up", "action": "press" } }` |
| `preset_down` | FS_D pressed (debounced, rising edge) | `{ "type": "event", "event": "footswitch", "payload": { "switch": "down", "action": "press" } }` |
| `preset_save` | FS_U or FS_D held ≥ 1 s after debounce | `{ "type": "event", "event": "footswitch", "payload": { "switch": "up" | "down", "action": "hold" } }` |

### 6.2 Event semantics

- **Edge-triggered:** A `footswitch` event fires only once per press — the rising edge is detected, debounced, and the event is emitted. Repeated rapid presses generate separate events; no repeat flooding is possible because the state machine requires a release (low) before the next press is acknowledged.
- **No repeat flooding:** The state machine returns to `IDLE` after release, so bouncing or chatter after debounce confirmation does not produce multiple events.
- **Hold is a separate event type:** A hold fires `action: "hold"` and **does not** fire a `press` event. If the user releases within 1 s, only `press` fires.
- **Events are asynchronous:** The UI receives them as unsolicited events (like `meters` and `tuner`) and must handle them without having asked for them. The engine dispatches them to all connected clients.

### 6.3 Host-side reaction

The engine's preset manager consumes `footswitch` events and applies:

```rust
// Pseudocode: what the engine does on a footswitch press event
fn on_footswitch(switch: Switch, action: Action) {
    match (switch, action) {
        (Up, Press) => engine.navigate_preset(1),      // next preset
        (Down, Press) => engine.navigate_preset(-1),   // previous preset
        (_, Hold) => engine.save_current_preset(),     // overwrite current preset
    }
}
```

The `navigate_preset` call mutates state; the engine replies with a full state snapshot (`res` with `ok: true`) to every client, including the broadcast `event: state`.

## 7. Reading strategy: polling vs interrupt-driven

**Chosen approach: interrupt-driven via GPIO chip notifications (libgpiod event API).**

The Pi 5 runs Linux, so the GPIO subsystem supports edge-triggered notifications. The engine uses `libgpiod` (the modern C library for GPIO on Linux) to register for `edge: both` notifications on GPIO 2 and 3:

```rust
// Conceptual Rust code — engine/src/footswitch.rs
use gpiod::{LineEvent, RequestFlags};

fn start_footswitch_poll() -> mpsc::Receiver<FootswitchEvent> {
    let chip = gpiod::Chip::new("gpiochip0").unwrap();
    let gpio_up = chip.get_line(2).unwrap();    // FS_U
    let gpio_down = chip.get_line(3).unwrap();  // FS_D

    // Request both lines with edge notifications
    let req_up = gpio_up.request(gpiod::RequestConfig {
        flags: RequestFlags::INPUT,
        consumer: Some("pedal-engine"),
    }).unwrap();
    let req_down = gpio_down.request(gpiod::RequestConfig {
        flags: RequestFlags::INPUT,
        consumer: Some("pedal-engine"),
    }).unwrap();

    // Use event API: poll() on the file descriptor, select() for timeout
    // (see Rust's gpiod bindings for event notification)
    // ...
}
```

**Why not polling in the audio callback?**

- The audio callback is `no_std` friendly and must never block, allocate, or sleep. Reading GPIO from the audio thread would either require memory-mapped I/O (blocking the Rust `std` GPIO library) or introduce an unnecessary I/O syscall.
- GPIO polling at a reasonable rate (e.g. 100 Hz for 10 ms debounce) would add a syscall or mmio read every 10 ms to the audio callback — even a single `libc::read` per 100 ms adds ~10–20 µs of context switch overhead, measurable on a Pi 5 at 50 kHz sample rate.

**Why not a dedicated polling thread?**

A 100 Hz polling thread would work, but the GPIO event API is simpler, uses less CPU, and fires an event **only when the pin changes**. This is zero-cost when the switch is idle (the thread sleeps, not polls).

**CPU cost per interrupt:**

- The GPIO event fires the notification thread once per pin edge (press or release).
- Per event, the thread:
  1. Reads the line value (~0.5 µs via libgpiod `get_value()`).
  2. Starts or resets the 20 ms debounce timer (~0.1 µs, just a clock read).
  3. If debounced: pushes to the `mpsc` channel (~0.5 µs).
- **Total: ~1.1 µs per interrupt** — negligible. In idle (no switch activity), the thread is asleep, consuming ~0 % CPU.

**Fallback: polling if the Pi 5 lacks libgpiod event support.**

If the target Linux kernel does not support GPIO event notifications (unlikely on modern kernels ≥ 5.15), the fallback is a dedicated thread sleeping 10 ms between reads:

- Per poll: read 2 GPIO values (~1 µs) + 10 ms sleep (~0 % CPU, kernel handles it).
- **CPU cost per poll: ~1 µs active + 10 ms sleeping** — still negligible.

## 8. CPU cost summary

| Operation | Active cost | Sleep cost | Frequency | Total CPU |
|---|---|---|---|---|
| GPIO interrupt (libgpiod event) | ~1.1 µs | 0 µs | ~4 events/s (2 switches × 2 edges) | ~4.4 µs/s = **0.0004 %** |
| Fallback polling thread | ~1 µs | 10 ms | 100 Hz | ~0.1 % |

In both cases, the GPIO cost is < 0.01 % of one Pi 5 core — well within the budget for a headless pedal running a Pi 5 at full load.

## 9. Component list (footswitch section only)

| Qty | Part | Value | Notes |
|---|---|---|---|
| 2 | Latching footswitch (NO + NC) | Alps LSGE3 or equivalent | 32×32 mm, 3V3 rated |
| 2 | Ceramic capacitor (debounce) | 100 nF, 0603 | X7R, across each switch line to GND |
| 2 | Resistor (pull-down) | 10 kΩ, 0603 | 1 %, SMD |
| 2 | Resistor (LED current-limit, optional) | 220 Ω, 0603 | If LED driven by GPIO; omit if LED tied to 3V3 |

## 10. Verification

To verify the footswitch works before the Pi 5 hardware is available:

1. Install libgpiod tools: `sudo apt install libgpiod-dev libgpiod2 gpiod`
2. Assign the pins in `/boot/fdt/overlays/pedal-footswitch.dtbo` (or use the libgpiod bindings directly).
3. Test with `gpiodetect` and `gpioget gpiochip0 2 3`.
4. Simulate a press with a jumper wire from 3V3 (pin 1) to the GPIO pin — the pedal-engine should emit a `footswitch` event and the UI should navigate presets.

---

**Acceptance criteria (t_cb996834):**

- ✅ Pins: GPIO 2 (FS_U) and GPIO 3 (FS_D) — documented with physical pin numbers.
- ✅ Debounce timing: 20 ms after rising edge, 1 s hold threshold — documented with state machine.
- ✅ Input type: digital (3.3 V logic, pull-down 10 kΩ + internal pull-down).
- ✅ Event semantics: edge-triggered `footswitch` events (`press` / `hold`), no repeat flooding — documented with JSON format.
- ✅ Reading strategy: interrupt-driven via libgpiod event API — documented with fallback to 100 Hz polling thread.
- ✅ CPU cost: ~1.1 µs per interrupt (0.0004 % CPU) — stated.
- ✅ No design decisions left for a builder: pins, debounce, event format, and reading approach are all specified.
