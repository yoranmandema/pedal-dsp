// Pure crossover-frequency logic. No DOM, no Preact; mirror `graph/scale.ts`.
//
// One CrossoverAxis presents the multiband compressor's ordered crossover
// frequencies as a log frequency axis. Each marker is bound to one engine
// param (`crossover_1…N`), and the markers must stay ordered (left → right =
// ascending) with a measured minimum spacing. This module owns that
// resolution so the component and its tests can reason about it without a
// browser.

/** Fallback param spec for `crossover_1..3`, mirrored from the engine's
 *  `BlockSpec` (see docs/design/multiband-compressor.md §4). Only used while
 *  the engine registry snapshot hasn't arrived yet. */
export interface CrossoverParam {
  id: string;
  min: number;
  max: number;
  default: number;
}

/** The multiband crossover parameters, in the order the engine names them.
 *  `crossover_1` is the lowest edge and anchors band 1; `crossover_N` is the
 *  topmost edge. */
export const CROSSOVER_PARAMS: readonly CrossoverParam[] = [
  { id: "crossover_1", min: 20, max: 2000, default: 200 },
  { id: "crossover_2", min: 200, max: 20000, default: 4000 },
  { id: "crossover_3", min: 200, max: 20000, default: 8000 },
];

/**
 * Active crossovers = `bands - 1`. `bands` is a count param (2..4, default 3),
 * so the number of active marker positions is 1..3, and the engine's `bands`
 * param is exactly `crossovers + 1`.
 */
export function activeCrossoverParamIds(bands: number): readonly string[] {
  const n = Math.min(4, Math.max(2, Math.floor(bands) | 0)) - 1;
  return CROSSOVER_PARAMS.slice(0, n).map((p) => p.id);
}

/**
 * Resolve raw (client-supplied) crossover values into a valid, ordered set.
 *
 * Rules:
 *   * each value is first clamped to its own parameter range;
 *   * consecutive values are arranged left→right in ascending order;
 *   * no two adjacent values are closer than `params[i].min` apart (enforced as
 *     a constant log ratio — 1 octave — which is what makes a multiband
 *     crossover set sound sensible).
 *
 * The input array is read-only; the output is a new array of the same length
 * (position `i` corresponds to `specs[i]`). `specs` and `raw` must have the
 * same length.
 */
export function resolveCrossoverValues(
  raw: readonly number[],
  specs: readonly CrossoverParam[],
): number[] {
  const n = Math.min(raw.length, specs.length);
  // Work on a mutable copy.
  const resolved = raw
    .slice(0, n)
    .map((v, i) => Math.min(specs[i].max, Math.max(specs[i].min, v)));

  // Clamp passes. The two constraints (left neighbour × ratio, right neighbour
  // ÷ ratio) are coupled, so sweep until stable — at most a few iterations for
  // a 1–3 marker axis.
  let stable = false;
  while (!stable) {
    stable = true;

    // Left → right: raise a marker if it is too close to (or below) its left
    // neighbour.
    for (let i = 1; i < n; i++) {
      const target = Math.max(specs[i].min, resolved[i - 1] * MIN_SPACING_RATIO);
      if (resolved[i] < target) {
        resolved[i] = target;
        stable = false;
      }
    }

    // Right → left: lower a marker if it is too close to (or above) its right
    // neighbour.
    for (let i = n - 2; i >= 0; i--) {
      const target = Math.min(specs[i].max, resolved[i + 1] / MIN_SPACING_RATIO);
      if (resolved[i] > target) {
        resolved[i] = target;
        stable = false;
      }
    }
  }
  return resolved;
}

/** Minimum spacing between adjacent crossover edges, measured in log ratio
 *  (≈1 octave). Matches the default 4k→8k split and keeps each band at least
 *  ~one octave wide. */
export const MIN_SPACING_RATIO = 2.0;

/** Format a crossover frequency for a marker label: 20 Hz, 1.5 kHz, 8 kHz. */
export function formatCrossover(v: number): string {
  const a = Math.abs(v);
  if (a >= 1000) {
    const s = (a / 1000).toFixed(1).replace(/\.0$/, "");
    return `${s} kHz`;
  }
  const s = a >= 10 ? a.toFixed(0) : a.toFixed(1);
  return `${s} Hz`;
}

/** Compact tick label: "20", "100", "1.0k", "10k", "20k" (the host appends the
 *  unit, so the tick renders "20 Hz", "1.0kHz", …). */
export function formatTickValue(v: number): string {
  const a = Math.abs(v);
  if (a >= 1000) return (a / 1000).toFixed(1).replace(/\.0$/, "") + "k";
  if (a >= 10) return a.toFixed(0);
  return a.toFixed(1);
}

/** Validate that a proposed set of marker values is well-formed: counts match,
 *  each is inside its spec, ordered, and spaced. */
export function validateCrossoverSet(
  values: readonly number[],
  specs: readonly CrossoverParam[],
): string | null {
  if (values.length !== specs.length) {
    return `expected ${specs.length} markers, got ${values.length}`;
  }
  for (let i = 0; i < values.length; i++) {
    if (values[i] < specs[i].min || values[i] > specs[i].max) {
      return `${specs[i].id}: ${values[i]} outside [${specs[i].min}, ${specs[i].max}]`;
    }
  }
  for (let i = 1; i < values.length; i++) {
    if (values[i] <= values[i - 1]) {
      return `crossovers out of order at ${specs[i].id}`;
    }
    if (values[i] < values[i - 1] * MIN_SPACING_RATIO) {
      return `${specs[i].id} within ${MIN_SPACING_RATIO}x of ${specs[i - 1].id}`;
    }
  }
  return null;
}
