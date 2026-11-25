// Parametric EQ (tone.eq) — block spec, band structure, and client-side
// frequency-response curve.
//
// The 19-parameter table mirrors docs/design/effects-parametric-eq.md §3
// (ids, names, units, min/max, defaults). The engine's registry snapshot is
// the single source of truth for the rendered values; this module owns the
// fixed band structure and the curve the panel draws, so the UI never needs a
// `curve` field from the engine.

import type { BlockSpec } from "../protocol";

/** One fixed band of the 6-band chain (design §2). */
export interface EQBand {
  id: string; // band id, used for grouping in the panel
  name: string; // display name
  kind: "low_shelf" | "low_peak" | "high_peak" | "high_shelf" | "high_pass" | "low_pass";
  /** Parameter id carrying the corner/centre frequency (Hz). */
  freqId: string;
  /** Parameter id carrying the gain (dB); undefined for HP/LP. */
  gainId?: string;
  /** Parameter id carrying the Q; undefined for non-peaking bands. */
  qId?: string;
  /** Parameter id carrying the enabled switch (0/1). */
  enabledId: string;
}

/** Fixed band structure per design §2 (no per-band type knob). */
export const EQ_BANDS: EQBand[] = [
  { id: "low_shelf", name: "Low shelf", kind: "low_shelf", freqId: "low_shelf_freq", gainId: "low_shelf_gain", enabledId: "low_shelf_enabled" },
  { id: "low_peak", name: "Low peak", kind: "low_peak", freqId: "low_peak_freq", gainId: "low_peak_gain", qId: "low_peak_q", enabledId: "low_peak_enabled" },
  { id: "high_peak", name: "High peak", kind: "high_peak", freqId: "high_peak_freq", gainId: "high_peak_gain", qId: "high_peak_q", enabledId: "high_peak_enabled" },
  { id: "high_shelf", name: "High shelf", kind: "high_shelf", freqId: "high_shelf_freq", gainId: "high_shelf_gain", enabledId: "high_shelf_enabled" },
  { id: "high_pass", name: "High pass", kind: "high_pass", freqId: "high_pass_freq", enabledId: "high_pass_enabled" },
  { id: "low_pass", name: "Low pass", kind: "low_pass", freqId: "low_pass_freq", enabledId: "low_pass_enabled" },
];

/** Fixed (non-user-adjustable) Q per band kind, per design §3. */
export const FIXED_Q: Record<EQBand["kind"], number> = {
  low_shelf: 0.707,
  low_peak: 1.0,
  high_peak: 1.0,
  high_shelf: 0.707,
  high_pass: 0.707,
  low_pass: 0.54,
};

/** The 19-parameter block spec for `tone.eq` — the UI registry entry. */
export const EQ_BLOCK_SPEC: BlockSpec = {
  name: "Parametric EQ",
  category: "Tone",
  params: [
    { id: "low_shelf_freq", name: "Low shelf freq", kind: "number", min: 20, max: 200, default: 100, unit: "Hz" },
    { id: "low_shelf_gain", name: "Low shelf gain", kind: "number", min: -12, max: 12, default: 0, unit: "dB" },
    { id: "low_shelf_enabled", name: "Low shelf", kind: "number", min: 0, max: 1, default: 1, unit: "" },
    { id: "low_peak_freq", name: "Low peak freq", kind: "number", min: 100, max: 2000, default: 300, unit: "Hz" },
    { id: "low_peak_gain", name: "Low peak gain", kind: "number", min: -12, max: 12, default: 0, unit: "dB" },
    { id: "low_peak_q", name: "Low peak Q", kind: "number", min: 0.35, max: 3, default: 1, unit: "" },
    { id: "low_peak_enabled", name: "Low peak", kind: "number", min: 0, max: 1, default: 1, unit: "" },
    { id: "high_peak_freq", name: "High peak freq", kind: "number", min: 1000, max: 12000, default: 3000, unit: "Hz" },
    { id: "high_peak_gain", name: "High peak gain", kind: "number", min: -12, max: 12, default: 0, unit: "dB" },
    { id: "high_peak_q", name: "High peak Q", kind: "number", min: 0.35, max: 3, default: 1, unit: "" },
    { id: "high_peak_enabled", name: "High peak", kind: "number", min: 0, max: 1, default: 1, unit: "" },
    { id: "high_shelf_freq", name: "High shelf freq", kind: "number", min: 2000, max: 20000, default: 10000, unit: "Hz" },
    { id: "high_shelf_gain", name: "High shelf gain", kind: "number", min: -12, max: 12, default: 0, unit: "dB" },
    { id: "high_shelf_enabled", name: "High shelf", kind: "number", min: 0, max: 1, default: 1, unit: "" },
    { id: "high_pass_freq", name: "High pass freq", kind: "number", min: 20, max: 300, default: 80, unit: "Hz" },
    { id: "high_pass_enabled", name: "High pass", kind: "number", min: 0, max: 1, default: 1, unit: "" },
    { id: "low_pass_freq", name: "Low pass freq", kind: "number", min: 1000, max: 20000, default: 12000, unit: "Hz" },
    { id: "low_pass_enabled", name: "Low pass", kind: "number", min: 0, max: 1, default: 1, unit: "" },
    { id: "output_gain", name: "Output gain", kind: "number", min: -12, max: 12, default: 0, unit: "dB" },
  ],
};

/** Map a registry spec to the band structure (used by the panel to read
 *  `spec.params` by id and build controls + curve). */
export function bandControls(spec: BlockSpec): EQBand[] {
  const byId = new Map(spec.params.map((p) => [p.id, p]));
  return EQ_BANDS.map((b) => ({
    ...b,
    freqId: byId.get(b.freqId)!.id,
    gainId: b.gainId ? byId.get(b.gainId)!.id : undefined,
    qId: b.qId ? byId.get(b.qId)!.id : undefined,
    enabledId: byId.get(b.enabledId)!.id,
  }));
}

// --- Maths -----------------------------------------------------------------

export function dbToLin(db: number): number {
  return Math.pow(10, db / 20);
}

export function linToDb(mag: number): number {
  return mag <= 0 ? -120 : 20 * Math.log10(mag);
}

export function hzToRad(f: number, fs: number): number {
  return (2 * Math.PI * f) / fs;
}

export interface BiquadCoefs {
  b0: number;
  b1: number;
  b2: number;
  a0: number;
  a1: number;
  a2: number;
}

/** RBJ cookbook coefficients for a single band, matching
 *  dsp/src/biquad.rs `Biquad::make` (a0 is the un-normalised denominator). */
export function biquadCoefficients(
  kind: EQBand["kind"],
  f0: number,
  q: number,
  gainDb: number,
  fs: number,
): BiquadCoefs {
  const f0c = Math.min(Math.max(f0, 1.0), fs * 0.499);
  const qc = Math.max(q, 0.01);
  const a = dbToLin(gainDb * 0.5); // db_to_lin(gain_db * 0.5) in the engine
  const w0 = hzToRad(f0c, fs);
  const cw = Math.cos(w0);
  const sw = Math.sin(w0);
  const alpha = sw / (2 * qc);
  const sqrtA = Math.sqrt(a);

  switch (kind) {
    case "low_shelf": {
      const b0 = a * ((a + 1) - (a - 1) * cw + 2 * sqrtA * alpha);
      const b1 = 2 * a * ((a - 1) - (a + 1) * cw);
      const b2 = a * ((a + 1) - (a - 1) * cw - 2 * sqrtA * alpha);
      const a0 = (a + 1) + (a - 1) * cw + 2 * sqrtA * alpha;
      const a1 = -2 * ((a - 1) + (a + 1) * cw);
      const a2 = (a + 1) + (a - 1) * cw - 2 * sqrtA * alpha;
      return { b0, b1, b2, a0, a1, a2 };
    }
    case "high_shelf": {
      const b0 = a * ((a + 1) + (a - 1) * cw + 2 * sqrtA * alpha);
      const b1 = -2 * a * ((a - 1) + (a + 1) * cw);
      const b2 = a * ((a + 1) + (a - 1) * cw - 2 * sqrtA * alpha);
      const a0 = (a + 1) - (a - 1) * cw + 2 * sqrtA * alpha;
      const a1 = 2 * ((a - 1) - (a + 1) * cw);
      const a2 = (a + 1) - (a - 1) * cw - 2 * sqrtA * alpha;
      return { b0, b1, b2, a0, a1, a2 };
    }
    case "low_peak":
    case "high_peak": {
      const b0 = 1 + alpha * a;
      const b1 = -2 * cw;
      const b2 = 1 - alpha * a;
      const a0 = 1 + alpha / a;
      const a1 = -2 * cw;
      const a2 = 1 - alpha / a;
      return { b0, b1, b2, a0, a1, a2 };
    }
    case "high_pass": {
      const b0 = (1 + cw) * 0.5;
      const b1 = -(1 + cw);
      const b2 = (1 + cw) * 0.5;
      const a0 = 1 + alpha;
      const a1 = -2 * cw;
      const a2 = 1 - alpha;
      return { b0, b1, b2, a0, a1, a2 };
    }
    case "low_pass": {
      const b0 = (1 - cw) * 0.5;
      const b1 = 1 - cw;
      const b2 = (1 - cw) * 0.5;
      const a0 = 1 + alpha;
      const a1 = -2 * cw;
      const a2 = 1 - alpha;
      return { b0, b1, b2, a0, a1, a2 };
    }
    default:
      throw new Error(`unknown band kind: ${kind}`);
  }
}

/** Filter gain in dB at a single frequency, matching
 *  dsp/src/biquad.rs `Biquad::gain_db_at`. */
export function biquadGainDb(coefs: BiquadCoefs, f: number, fs: number): number {
  // Normalise a0 out (Biquad::new).
  const inv = 1 / coefs.a0;
  const nb0 = coefs.b0 * inv;
  const nb1 = coefs.b1 * inv;
  const nb2 = coefs.b2 * inv;
  const na1 = coefs.a1 * inv;
  const na2 = coefs.a2 * inv;
  const w = hzToRad(f, fs);
  const cw = Math.cos(w);
  const sw = Math.sin(w);
  const numRe = nb0 + nb1 * cw + nb2 * Math.cos(2 * w);
  const numIm = -(nb1 * sw + nb2 * Math.sin(2 * w));
  const denRe = 1 + na1 * cw + na2 * Math.cos(2 * w);
  const denIm = -(na1 * sw + na2 * Math.sin(2 * w));
  const denSq = denRe * denRe + denIm * denIm;
  const hRe = (numRe * denRe + numIm * denIm) / denSq;
  const hIm = (numIm * denRe - numRe * denIm) / denSq;
  const mag = Math.sqrt(hRe * hRe + hIm * hIm);
  return linToDb(mag);
}

// --- Curve ------------------------------------------------------------------

export interface CurvePoint {
  freq: number;
  db: number;
}

export interface BandCurve {
  band: EQBand;
  points: CurvePoint[];
}

/** Frequency points, log-spaced from 20 Hz to `fs/2`, shape-compatible with
 *  `pedal_dsp` offline renders (48 kHz default). */
export const CURVE_FS = 48_000;
export const CURVE_N = 480;
export const CURVE_F_LO = 20;
export const CURVE_F_HI = CURVE_FS / 2;

/** Frequency ticks, log-spaced (20 Hz … 24 kHz). Placing about one tick per
 *  decade keeps the axis readable without congestion. */
export const CURVE_F_TICKS = [20, 100, 1000, 10000, 20000];

function makeFlatCurve(n: number): CurvePoint[] {
  const pts: CurvePoint[] = [];
  for (let i = 0; i < n; i++) {
    const t = i / (n - 1);
    pts.push({ freq: CURVE_F_LO * (CURVE_F_HI / CURVE_F_LO) ** t, db: 0 });
  }
  return pts;
}

/** Per-band trace. Flat when the band is disabled or (for bands with a gain
 *  param) at 0 dB gain — the disable/0-gain state the design §1 describes. */
export function bandCurve(band: EQBand, params: Record<string, number>): CurvePoint[] {
  const enabled = params[band.enabledId] !== 0;
  if (!enabled) return makeFlatCurve(CURVE_N);

  const gainId = band.gainId;
  const gainDb = gainId !== undefined ? params[gainId] ?? 0 : 0;
  if (gainId !== undefined && Math.abs(gainDb) < 1e-6) return makeFlatCurve(CURVE_N);

  let q: number;
  if (band.qId !== undefined) {
    q = params[band.qId] ?? FIXED_Q[band.kind];
  } else {
    q = FIXED_Q[band.kind];
  }
  const f0 = params[band.freqId] ?? band.freqId;
  const coefs = biquadCoefficients(band.kind, f0, q, gainDb, CURVE_FS);
  const points: CurvePoint[] = [];
  for (let _i = 0; _i < CURVE_N; _i++) {
    const t = _i / (CURVE_N - 1);
    const f = CURVE_F_LO * (CURVE_F_HI / CURVE_F_LO) ** t;
    points.push({ freq: f, db: biquadGainDb(coefs, f, CURVE_FS) });
  }
  return points;
}

/** The full curve (one trace per band) as rendered by the panel. */
export function eqCurve(params: Record<string, number>): BandCurve[] {
  return EQ_BANDS.map((band) => ({ band, points: bandCurve(band, params) }));
}

/** Clamp a value into [min, max]. Kept for the panel's step + clamp helpers. */
export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}
