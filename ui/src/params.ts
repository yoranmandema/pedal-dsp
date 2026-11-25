// Parameter schema for the multiband compressor.
//
// One module of truth for the UI: ranges, units, steps, and formatting helpers.
// The editor reads a `ParamSpec` (ui/src/protocol.ts) emitted by the engine, but
// the schema here is what turns a raw number into a labelled, snapped control — so
// it must mirror the engine's `MultibandCompressor::spec()` exactly (see
// docs/design/multiband-compressor.md, §3/§4, and graph/src/blocks.rs).
//
// The wire protocol's `ParamSpec` has `id: string`, which is not precise enough
// for a table-driven UI. This module defines a `ParamSchema` whose `id` is the
// precise `ParamId` union, and derives the global and per-band parameter sets from
// it. Every `formatByUnit` / `Format` helper here is shared by the panel, so a
// change to the schema is a one-line edit.

/** Every parameter id the multiband compressor exposes.
 *
 *  Global set:   bands, crossover_1..3, output_gain, dry_wet.
 *  Per-band set: threshold, ratio, attack, release, makeup, bypass, solo.
 */
export type ParamId =
  | "bands"
  | "crossover_1"
  | "crossover_2"
  | "crossover_3"
  | "output_gain"
  | "dry_wet"
  | `band_1_threshold`
  | `band_1_ratio`
  | `band_1_attack`
  | `band_1_release`
  | `band_1_makeup`
  | `band_1_bypass`
  | `band_1_solo`
  | `band_2_threshold`
  | `band_2_ratio`
  | `band_2_attack`
  | `band_2_release`
  | `band_2_makeup`
  | `band_2_bypass`
  | `band_2_solo`
  | `band_3_threshold`
  | `band_3_ratio`
  | `band_3_attack`
  | `band_3_release`
  | `band_3_makeup`
  | `band_3_bypass`
  | `band_3_solo`
  | `band_4_threshold`
  | `band_4_ratio`
  | `band_4_attack`
  | `band_4_release`
  | `band_4_makeup`
  | `band_4_bypass`
  | `band_4_solo`;

/** One parameter's contract: the range, unit, and step the UI must honour.
 *
 * Unlike the engine's wire `ParamSpec` (whose `id` is a loose `string`), this
 * schema carries the precise `ParamId` so table-driven renderers cannot index it
 * with an unknown string. The wire type is still the source of truth for what the
 * engine actually emits; this is the UI-side view of it.
 */
export interface ParamSchema {
  id: ParamId;
  name: string;
  kind: "number";
  min: number;
  max: number;
  default: number;
  unit: string;
  /** Snapping granularity for the control (knobs advance by this much). */
  step: number;
}

/** Build a parameter schema. */
export const makeParam = (
  id: ParamId,
  name: string,
  min: number,
  max: number,
  defaultValue: number,
  step: number,
  unit: string,
): ParamSchema => ({
  id,
  name,
  kind: "number",
  min,
  max,
  default: defaultValue,
  unit,
  step,
});

/** Global parameters, active for every band count.
 *
 * `bands` carries no unit: it is a small integer count, and the editor renders it
 * as a segmented row (see `Editor.tsx`'s `ChoiceControl`), not a knob. All other
 * globals have a unit that the formatter appends.
 */
export const GLOBAL_PARAMS: ParamSchema[] = [
  makeParam("bands", "Bands", 2, 4, 3, 1, ""),
  makeParam("crossover_1", "Crossover 1", 20, 2000, 200, 10, "Hz"),
  makeParam("crossover_2", "Crossover 2", 200, 20000, 4000, 100, "Hz"),
  makeParam("crossover_3", "Crossover 3", 200, 20000, 8000, 100, "Hz"),
  makeParam("output_gain", "Output gain", -24, 24, 0, 0.5, "dB"),
  makeParam("dry_wet", "Dry/Wet", 0, 100, 100, 1, "%"),
];

/** Per-band parameter schema for each band, in listing order (band 1 → 4).
 *
 * `band_<i>_bypass` and `band_<i>_solo` are on/off in the engine; the UI renders
 * them as toggles, so a value of 0 or 1 is the entire range.
 */
export const BAND_PARAM_SPECS: ParamSchema[] = [
  // ---- band 1 ----
  makeParam("band_1_threshold", "Threshold", -60, 0, -20, 0.5, "dB"),
  makeParam("band_1_ratio", "Ratio", 1, 20, 4, 0.5, "x"),
  makeParam("band_1_attack", "Attack", 0.1, 50, 1, 0.1, "ms"),
  makeParam("band_1_release", "Release", 1, 500, 50, 1, "ms"),
  makeParam("band_1_makeup", "Makeup", 0, 30, 0, 0.5, "dB"),
  makeParam("band_1_bypass", "Bypass", 0, 1, 0, 1, ""),
  makeParam("band_1_solo", "Solo", 0, 1, 0, 1, ""),
  // ---- band 2 ----
  makeParam("band_2_threshold", "Threshold", -60, 0, -20, 0.5, "dB"),
  makeParam("band_2_ratio", "Ratio", 1, 20, 4, 0.5, "x"),
  makeParam("band_2_attack", "Attack", 0.1, 50, 1, 0.1, "ms"),
  makeParam("band_2_release", "Release", 1, 500, 50, 1, "ms"),
  makeParam("band_2_makeup", "Makeup", 0, 30, 0, 0.5, "dB"),
  makeParam("band_2_bypass", "Bypass", 0, 1, 0, 1, ""),
  makeParam("band_2_solo", "Solo", 0, 1, 0, 1, ""),
  // ---- band 3 ----
  makeParam("band_3_threshold", "Threshold", -60, 0, -20, 0.5, "dB"),
  makeParam("band_3_ratio", "Ratio", 1, 20, 4, 0.5, "x"),
  makeParam("band_3_attack", "Attack", 0.1, 50, 1, 0.1, "ms"),
  makeParam("band_3_release", "Release", 1, 500, 50, 1, "ms"),
  makeParam("band_3_makeup", "Makeup", 0, 30, 0, 0.5, "dB"),
  makeParam("band_3_bypass", "Bypass", 0, 1, 0, 1, ""),
  makeParam("band_3_solo", "Solo", 0, 1, 0, 1, ""),
  // ---- band 4 ----
  makeParam("band_4_threshold", "Threshold", -60, 0, -20, 0.5, "dB"),
  makeParam("band_4_ratio", "Ratio", 1, 20, 4, 0.5, "x"),
  makeParam("band_4_attack", "Attack", 0.1, 50, 1, 0.1, "ms"),
  makeParam("band_4_release", "Release", 1, 500, 50, 1, "ms"),
  makeParam("band_4_makeup", "Makeup", 0, 30, 0, 0.5, "dB"),
  makeParam("band_4_bypass", "Bypass", 0, 1, 0, 1, ""),
  makeParam("band_4_solo", "Solo", 0, 1, 0, 1, ""),
];

/** One flat list of every parameter schema, in the order the editor lists them
 *  (globals first, then each band's columns). */
export const ALL_PARAMS: ParamSchema[] = [...GLOBAL_PARAMS, ...BAND_PARAM_SPECS];

/** All parameter ids, in the order the editor lists them. */
export const ALL_PARAM_IDS: ParamId[] = ALL_PARAMS.map((p) => p.id);

/** The band index-space the panel renders: 1..bands, 1-based. */
export function activeBandIds(bands: number): number[] {
  const n = Math.min(4, Math.max(2, Math.round(bands)));
  return Array.from({ length: n }, (_, i) => i + 1);
}

/** The active band count, clamped to the engine's [2, 4] range. */
export function clampBands(bands: number): number {
  return Math.min(4, Math.max(2, Math.round(bands)));
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/**
 * Formatting helpers shared across the multiband panel.
 *
 * Each returns a single string with its unit baked in, so the value readout
 * beneath a control is always one cell wide and never misaligns with its
 * neighbours (the spacing audit in `tailwind.css` depends on it).
 */
export const Format = {
  /** Decibels. Integer at |v|>=10, one decimal below. */
  db: (v: number): string => {
    const decimals = Math.abs(v) >= 10 ? 0 : Math.abs(v) >= 1 ? 1 : 2;
    return `${v.toFixed(decimals)} dB`;
  },
  /** Milliseconds (attack/release). */
  ms: (v: number): string => `${v.toFixed(2)} ms`,
  /** Ratio. */
  ratio: (v: number): string => `${v.toFixed(2)}x`,
  /** Hertz. */
  hz: (v: number): string => `${v.toFixed(1)} Hz`,
  /** Percent (dry/wet). */
  percent: (v: number): string => `${v.toFixed(1)}%`,
  /** Linear value (bypass/solo on/off, bands). */
  linear: (v: number): string => `${v.toFixed(0)}`,
};

/** Format a value by its schema unit. */
export function formatByUnit(unit: string, v: number): string {
  switch (unit) {
    case "dB":
      return Format.db(v);
    case "ms":
      return Format.ms(v);
    case "x":
      return Format.ratio(v);
    case "Hz":
      return Format.hz(v);
    case "%":
      return Format.percent(v);
    default:
      return Format.linear(v);
  }
}
