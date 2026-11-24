# Gain Staging Tool

## Overview

A guided gain staging assistant, accessible from the input volume knob area.
Helps users set proper input levels for their instrument and saves reusable
profiles.

## Architecture

```
┌─────────────────────────────────────────────┐
│                  TopBar                      │
│   ┌─────────┐    ┌──────────────────────┐   │
│   │HeaderKnob│◄──►│  GainStagingTrigger  │   │
│   │(input    │    │  [ 🎚 ] icon button  │   │
│   │ volume) │    └──────────┬───────────┘   │
│   └─────────┘               │               │
│                             ▼               │
│                   ┌─────────────────┐       │
│                   │  GainStagingModal│      │
│                   │  (3-step wizard) │      │
│                   └─────────────────┘       │
└─────────────────────────────────────────────┘
```

## Block: GainStaging

A new block type registered as `"gain_staging"` in category `"utils"`.

- **Audio**: Pass-through (identity). Does not modify the signal.
- **Visual**: Renders a real-time level meter in the block card.
- **Placement**: Sits logically after the input stage, before or alongside
  other blocks. Can appear on any channel.

### Block Spec

```typescript
BlockSpec {
  type: "gain_staging",
  name: "Gain Staging",
  category: "utils",
  params: [
    { id: "instrument",      kind: "choice", choices: [...], default: "none" },
    { id: "target_level_dbfs", kind: "number", min: -60, max: 0,  default: -18, unit: "dBFS" },
    { id: "calibrated_gain_db",  kind: "number", min: 0,  max: 60, default: 0,   unit: "dB" },
    { id: "profile_name",    kind: "choice", choices: [],          default: "",      },
  ]
}
```

### Rust struct

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GainStaging {
    pub instrument: InstrumentType,
    pub target_level_dbfs: f64,
    pub calibrated_gain_db: f64,
    pub profile_name: String,
}
```

### Instrument types (enum)

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentType {
    None,
    ElectricGuitar,
    Bass,
    AcousticGuitar,
    Microphone,
    LineIn,
    ActivePickup,
    PassivePickup,
}
```

### Instrument default target levels

| Type              | Target (dBFS) | Reason                                    |
|-------------------|---------------|-------------------------------------------|
| ElectricGuitar    | -12           | Hot pickups drive harder; leave headroom  |
| Bass              | -15           | Mid-range peak, some dynamics             |
| AcousticGuitar    | -18           | Dynamic range; avoid clipping transients  |
| Microphone        | -20           | Mic preamps are sensitive; max headroom   |
| LineIn            | -18           | Standard line level                       |
| ActivePickup      | -15           | Stronger signal than passive              |
| PassivePickup     | -12           | Drives preamp harder                      |
| None              | -18           | Default                                 |

## Gain Staging Profiles

A profile is a reusable preset of instrument + recommended settings.

```typescript
GainProfile {
  id: string,        // auto-generated UUID
  instrument: InstrumentType,
  recommended_input_gain_db: number,  // what to set the channel's input_gain to
  target_level_dbfs: number,
  notes: string,   // optional user notes
  created_at: string, // ISO timestamp
}
```

### State extension

```rust
pub struct State {
    // existing fields...
    pub gain_profiles: Vec<GainProfile>,
    pub active_profile_id: Option<String>,
}
```

### IPC: profile operations

New `req` type `"gain_staging"` with sub-operations in a `cmd` field:

| cmd           | action                                              |
|---------------|-----------------------------------------------------|
| `start_measure`   | Begin collecting input level samples for 5 seconds  |
| `stop_measure`    | Stop collection, send results                       |
| `save_profile`    | Save a profile from current results                 |
| `load_profile`    | Apply profile settings to the block params          |
| `delete_profile`  | Remove a profile by id                              |
| `list_profiles`   | Return current profile list                         |

The server responds with `res` containing the relevant data.

### IPC: real-time level data

While measuring, the server pushes `event` frames:
```json
{"ev": {"type": "gain_staging_level", "level_dbfs": -24.5, "peak_dbfs": -18.2}}
```

## WebSocket Protocol Changes

### New enum: GainStagingCmd

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GainStagingCmd {
    StartMeasure { channel: usize },
    StopMeasure,
    SaveProfile { name: String, notes: Option<String> },
    LoadProfile { profile_id: String, channel: usize },
    DeleteProfile { profile_id: String },
    ListProfiles,
}
```

### New enum: GainStagingEvent

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GainStagingEvent {
    LevelUpdate { channel: usize, level_dbfs: f64, peak_dbfs: f64 },
    MeasureComplete { channel: usize, avg_dbfs: f64, peak_dbfs: f64, recommended_gain_db: f64 },
}
```

### State changes

```rust
struct State {
    // existing...
    pub gain_profiles: Vec<GainProfile>,
    pub measure_state: Option<MeasureState>,
}

struct MeasureState {
    pub channel: usize,
    pub samples: Vec<(f64, f64)>,  // (time_sec, level_dbfs)
    pub started_at: f64,
    pub peak_dbfs: f64,
}
```

## Gain Staging Algorithm

The tool measures the user's playing over a 5-second window and calculates:

1. **Average RMS level** across the window
2. **Peak level** (max instantaneous)
3. **Recommended gain** to bring the peak to the target level

```
recommended_gain_db = target_level_dbfs - peak_dbfs_measured
recommended_gain_db = clamp(recommended_gain_db, 0, 60)
```

If the measured signal is already at or below target:
- recommended_gain_db = 0 (don't add unnecessary gain)

The recommended gain is applied as a suggestion: clicking "Apply" sets
`calibrated_gain_db` on the block AND syncs the channel's `input_gain`.

## UI: Gain Staging Modal

A modal overlay triggered from the input volume knob area.

### Step 1: Instrument Selection

- Grid of 7 instrument type buttons (icons + labels)
- "None / Other" option at the end
- Each shows the recommended target level

### Step 2: Level Measurement

- Large animated level meter (bar or circular)
- Shows real-time dBFS value
- "Hold for 5s" progress bar
- Peak level display
- "Playing" indicator

### Step 3: Results & Save

- Shows: measured average, measured peak, recommended gain
- Visual comparison: "Your level" vs "Recommended"
- "Apply Gain" button → sets block params + channel input_gain
- "Save as Profile" → opens name input
- "Save as Profile" creates a profile entry
- Profiles list is accessible for loading/deleting

### Integration with HeaderKnob

The HeaderKnob area (input volume trim) gets a small adjacent icon/button:
- A small "🎚" or similar icon next to the input volume knob
- Or a thin vertical line divider with a small "GS" label
- Clicking opens the GainStagingModal

### Integration with Rack

When a GainStaging block is on a channel:
- It renders as a slim block card (like DC blocker)
- Shows a real-time level meter inside the card
- Shows instrument type label (small)
- Clicking opens the modal with the block's current settings

## Try It

1. Run `source scripts/agent-env.sh` then launch engine + Vite dev server
2. Click the Gain Staging button near the input volume knob
3. Select "Electric Guitar" as the instrument
4. Play through for 5 seconds
5. View recommended gain settings
6. Click "Save as Profile" and "Load Profile" to test profile persistence
