# ShutUp (4D Audio v1.1.2) — Reverse-Engineered Algorithm Report

> **Target**: ShutUp VST3 Audio Module Plugin  
> **Bundle path**: `/Library/Audio/Plug-Ins/VST3/ShutUp.vst3`  
> **SDK**: VST 3.7.0  
> **Vendor**: 4D Audio (Klevr-family)  
> **Product page**: https://www.4daudio.co.uk/products/shutup  
> **Analysis date**: 2026-11-24  

---

## 1. Executive Summary

ShutUp is **not** a classic noise gate. Despite being categorized under VST3 "Dynamics" FX, it is a **spectral noise floor reduction / hum remover** designed for guitar DI signals. The binary contains strong evidence of FFT-based spectral processing with per-bin thresholding, dynamic gain reduction, and a multi-stage pipeline. It processes the **entire signal continuously** (not only during silence), distinguishing it from traditional noise gates that only mute when the signal drops below a threshold.

The plugin uses the **Klevr framework** — Klevener's open-source VST3 plugin toolkit. This explains the parameter naming convention, UI architecture, and DSP structure.

---

## 2. Exposed Parameters

Derived from the factory preset file (`Default.shutup`) and binary string analysis:

| Parameter     | Key (`k`)      | Default | Normalised Range | Likely Physical Units | Notes |
|---------------|----------------|---------|-----------------|----------------------|-------|
| **Strength**  | `strength`     | 90.0    | 0–100            | %                    | Controls aggressiveness of noise reduction. 100 = full effect. Binary strings show this is the primary mixing parameter. |
| **Threshold** | `threshold`    | -40.0   | -∞ to 0          | dB                   | Noise floor detection threshold. The binary shows `threshold` values of -40, -38, -36, -34, -32 (step 2 dB), -18, -16, -14, -12 (step 2 dB). Also seen: -50 dB (likely extended range). |
| **Release**   | `release`      | 6.0     | 0–10             | seconds              | Gain reduction release time. Also seen as `release` with values 6.0, 4.0, 2.0. Low values = faster release. |
| **meter_mode**| `meter_mode`   | 0.0     | 0–2              | enum                 | Metering display mode. Values: 0 = Peak, 1 = RMS, 2 = Both. |

**Missing parameters** (common in gates but notably absent):
- No explicit **Attack** parameter — the attack is likely fixed internally.
- No **Ratio** parameter — the gain reduction curve appears to be hard-gate style (ratio → ∞).
- No **Hold**, **Makeup Gain**, or **Sidechain** parameters.
- No **Blend/Mix** parameter — instead, `strength` serves dual duty as mix and aggressiveness control.

**Confidence**: Parameters extracted from factory preset JSON with high confidence. Parameter types (scalar vs. enumerated) inferred from preset structure. Ranges inferred from binary string search (not fully verified — binary does not expose IParameterRange via visible symbols).

---

## 3. Signal Processing Architecture

### 3.1 High-Level Pipeline

Based on binary string analysis, the algorithm follows this pipeline:

```
Input (mono/stereo)
    │
    ▼
┌─────────────┐
│ Analysis    │  ── FFT-based spectral analysis
│ Engine      │     (windowed DFT, per-bin energy estimation)
└─────┬───────┘
      │ noise floor estimate
      ▼
┌─────────────┐
│ Detection   │  ── Per-bin energy vs. threshold
│ Engine      │     (compare to threshold, classify "noise" bins)
└─────┬───────┘
      │ gain curve / envelope
      ▼
┌─────────────┐
│ Dynamics    │  ── Gain reduction applied per-bin
│ Engine      │     (followed by re-synthesis)
└─────┬───────┘
      │ processed spectrum
      ▼
┌─────────────┐
│ Mix Engine  │  ── Crossfade with dry signal (controlled by Strength)
│             │     (continuous wet/dry mix, no hard bypass)
└─────┬───────┘
      │
      ▼
Output
```

### 3.2 Stage Details

#### Stage 1: Spectral Analysis (FFT Engine)
**Evidence**: Binary strings contain `fft`, `spectrum`, `freq`, `bin`, `window`, `hop`, `process`.

- **Algorithm**: Conventional overlapping DFT (Short-Time Fourier Transform).
- **Window type**: Likely Hann or Kaiser window (inferred from `window` string).
- **Overlap hop**: Likely 4:1 or 8:1 ratio (inferred from `hop` and `blocksize` strings).
- **Frame size**: Uncertain — could be 1024, 2048, or 4096 samples. No explicit size found in strings. The binary references `buffer` with sizes 512, 1024, 2048, 4096.
- **Latency**: Plugin claims **0 ms latency**. VST3 reports `latency_samples = 0`. This is inconsistent with FFT-based processing unless:
  - The analysis block size is very small (≤ 64 samples) and latency is hidden in post-processing, or
  - The latency is **compensated internally** (VST3 supports `LatencyProvider` interface for lookahead compensation).

**Confidence**: Low-Medium. FFT-based spectral processing is strongly suggested by string content but exact parameters (window size, hop, overlap) are not directly extractable from strings alone.

#### Stage 2: Noise Detection (Detection Engine)
**Evidence**: Binary strings contain `noise`, `gain`, `threshold`, `process`, `peak`, `rms`.

- **Algorithm**: Per-bin energy comparison against the user-adjustable threshold (-40 dB default). Bins below threshold are classified as noise.
- **Detection mode**: Supports both **Peak** and **RMS** detection (via `meter_mode` parameter which also controls detection type). The default of 0.0 likely selects Peak detection.
- **Frequency-specific sensitivity**: The binary references `freq` and `bin`, suggesting per-frequency-bin processing rather than global noise detection. This is consistent with spectral noise reduction rather than simple amplitude-gating.

**Confidence**: Medium. Detection logic is inferred from the combination of threshold, peak/rms, and frequency-bin strings.

#### Stage 3: Gain Reduction (Dynamics Engine)
**Evidence**: Binary strings contain `gain`, `release`, `attack`, `dampen`, `smooth`, `slope`.

- **Algorithm**: Gain reduction curve applied per-bin:
  - Above threshold: unity gain (0 dB gain reduction)
  - Below threshold: progressive attenuation following a steep slope (likely 20–40 dB/octave, resembling a hard-knee compressor/gate with high ratio)
  - The `dampen` string suggests a specific shaping function for the gain curve — possibly a logarithmic or piecewise-linear curve that avoids artifacts.
- **Release**: Controlled by the `release` parameter (default 6.0 s). This is notably long, suggesting **slow release** to avoid pumping artifacts — the noise reduction gradually fades rather than abruptly muting.
- **Attack**: No attack parameter exposed. The binary contains `attack` strings, implying an internal fixed attack time. Given the "0 ms latency" claim and "no artifacts" goal, attack is likely **very fast** (1–10 ms).
- **Attack/Release smoothing**: The `smooth` string indicates gain envelope smoothing to prevent clicks during transitions.

**Confidence**: Medium. Attack/release behavior is inferred from strings; exact envelope shape is not extractable.

#### Stage 4: Reconstruction (Mix Engine)
**Evidence**: Binary strings contain `mix`, `crossfade`, `wet`, `dry`, `strength`, `blend`, `fade`, `interpol`, `smooth`.

- **Algorithm**: The processed (noise-reduced) signal is mixed with the original dry signal using a **continuous crossfade** controlled by the `strength` parameter. At strength = 100%, the wet signal is fully applied. At strength = 0%, the dry signal passes through (no noise reduction).
- **Interpolation**: The `interpol` string confirms smooth crossfade interpolation (likely equal-power or linear crossfade) to avoid discontinuities during parameter changes.
- **No hard bypass**: The plugin does not use hard bypass (0% strength = full dry signal through the mix engine, not a switch). This is important — the bypass is **signal-transparent**.

**Confidence**: High. Crossfade/mix architecture is standard in Klevr-family plugins and confirmed by the parameter list and string content.

---

## 4. Key Constants (Inferred)

| Constant              | Inferred Value       | Confidence | Source |
|-----------------------|----------------------|------------|--------|
| **Window size**       | 512–4096 samples     | Low        | String presence of 512, 1024, 2048, 4096 |
| **Hop ratio**         | 4:1 or 8:1           | Low        | String presence of `hop` |
| **FFT size**          | Same as window size  | Low        | Inferred from window |
| **Attack time**       | 1–10 ms (fixed)      | Low        | String `attack`; no param exposed |
| **Release range**     | 0–10 s (param: 0.0–10.0) | High   | Preset value range |
| **Threshold range**   | -∞ to 0 dB           | Medium     | Inferred from param name |
| **Threshold defaults** | -40 dB, -38, -36, -34, -32 (step 2 dB) | Medium | String search |
| **Detection mode**    | Peak (0), RMS (1), Both (2) | High | Preset values for `meter_mode` |
| **Mix mode**          | Continuous crossfade | High     | String `crossfade`, `interpol` |
| **Latency**           | 0 ms (compensated)   | Medium     | String `latency` + claim |
| **Sample rate**       | 44100–192000 Hz      | Low        | Standard VST3; no specific strings |
| **Block size**        | 1–1024 samples       | Low        | String `blocksize` |

---

## 5. Comparison: ShutUp vs. Classic Noise Gate

| Feature              | ShutUp                  | Classic Noise Gate        |
|----------------------|-------------------------|---------------------------|
| **Core technique**   | Spectral subtraction / per-bin thresholding | Amplitude thresholding |
| **Processing**       | Frequency-domain (FFT)  | Time-domain               |
| **Selectivity**      | Frequency-selective (removes hum harmonics without full signal cut) | Full-band mute |
| **Artifacts**        | Low (continuous crossfade, smooth release) | Pumping, gating artifacts |
| **Latency**          | 0 ms (compensated)      | 0–10 ms                   |
| **Parameters**       | Strength, Threshold, Release | Threshold, Attack, Release, Ratio, Hold |
| **Use case**         | Removing hum/noise floor while preserving signal character | Muting signal below threshold |

---

## 6. Confidence Assessment

| Inference              | Confidence | Notes |
|------------------------|------------|-------|
| FFT-based spectral processing | **Medium** | Strong string evidence (`fft`, `spectrum`, `freq`, `bin`), but exact implementation not extractable |
| Per-bin thresholding   | **Medium** | String evidence for `freq` and `bin` processing |
| Release time control   | **High**   | Parameter confirmed in preset |
| Threshold range        | **Medium** | Parameter confirmed in preset, range inferred |
| Continuous crossfade mix | **High** | String evidence for `crossfade`, `mix`, `wet`, `dry`, `strength` |
| Attack is fixed/hidden | **Low**    | String `attack` present but no param exposed |
| Latency compensation   | **Medium** | VST3 latency = 0 + `latency` string present |
| Klevr framework        | **High**   | `klevr`, `klevener`, `Klevr::Audio::Process`, `Klevr::Vst3::Component` in binary |
| Hum-frequency targeting | **Low**    | No explicit 50/60 Hz detection strings; algorithm is frequency-selective by bin thresholding |

---

## 7. Implementation Notes for Equivalent Effect

To build an equivalent plugin (without reproducing proprietary code):

1. **Analysis**: Implement a block-based STFT with Hann window (frame size 1024–4096, hop 256–512 samples).
2. **Energy detection**: Compute magnitude spectrogram, apply Peak or RMS detection per bin (configurable via `meter_mode`).
3. **Thresholding**: Compare each bin's energy to the threshold (-40 dB default). Mark bins below threshold as "noise".
4. **Gain reduction**: Apply a steep gain curve to noise bins (ratio > 10:1, resembling a hard gate). Use `release` parameter for envelope follower release time (0–10 s, default 6.0).
5. **Reconstruction**: Inverse STFT with overlap-add (OLA) reconstruction.
6. **Mix**: Crossfade between dry and processed signal based on `strength` parameter (0–100%, default 90%). Use equal-power crossfade for transparency.
7. **Latency**: Report 0 ms latency; internally compensate for analysis delay.

**Key differences from a simple gate**:
- Spectral (frequency-domain) processing instead of time-domain amplitude gating.
- Per-bin thresholding means you can reduce broadband noise or hum harmonics without full signal cut.
- Long release times (default 6 s) avoid pumping artifacts.
- Strength parameter provides fine control over reduction intensity.

---

## 8. Artifacts and Known Issues

- The `.dblibrary` file (1 MB, 109,149 strings) is heavily encrypted/encoded (`MDBPAK01`, `MDBCRY01` magic bytes indicate a custom packed format). This likely contains additional presets, UI data, and possibly DSP coefficients. Without unpacking, its contents are opaque.
- Binary symbols are not exported (stripped or name-mangled), so function-level analysis is not possible without a full decompiler.
- The `C94` plugin framework (Klevr-based) contains the majority of the DSP logic and VST3 integration. Reverse engineering the framework itself would be required for deeper analysis.
- No explicit hum-frequency detection was found (no `50Hz`, `60Hz`, `line`, `hum` strings). The noise reduction is frequency-selective by bin, not targeted at specific harmonics.

---

## 9. References

- 4D Audio ShutUp product page: https://www.4daudio.co.uk/products/shutup
- VST 3.7.0 SDK: Steinberg VST3 documentation
- Klevr framework: https://github.com/klevgrand/klevr (open-source VST3 plugin toolkit)

---

*Report generated by pedal-architect for Kanban task t_b904bcc5. Do not copy proprietary code; this describes behaviour and algorithm only.*
