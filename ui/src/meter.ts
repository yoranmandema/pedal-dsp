// Pure gain-reduction meter helpers.
//
// The multiband compressor's per-band `gr_db` is a peak (the envelope detector
// only moves in one direction: once a band is reduced, the value holds while
// the signal continues). `smoothPeak` turns a stream of such peaks into a
// display value that attacks fast (so transients register quickly) and
// releases slowly (so the needle doesn't jitter at the engine's ~20 Hz rate).
// The component that renders this keeps the update loop on requestAnimationFrame
// instead of reacting to the 20 Hz metering stream directly, so the UI thread
// never does work on the audio cadence.

/**
 * One band's meter values as produced by the engine (extended `Meters`, see
 * docs/design/multiband-compressor.md §7): `gr_db` is the peak gain reduction
 * of the band (dB, negative = reduction), `out_db` the peak output level (dBFS).
 */
export interface MultibandMeter {
  band: string;
  gr_db: number;
  out_db: number;
}

/**
 * Smooth a peak value with fast attack / slow release.
 *
 * @param current  last displayed value (dB; negative = reduction)
 * @param target   raw peak value to move toward (dB)
 * @param dt       elapsed seconds since the last call (must be <= 0.1)
 * @param attackMs attack time constant in milliseconds (default 10)
 * @param releaseMs release time constant in milliseconds (default 120)
 * @returns the smoothed value
 */
export function smoothPeak(
  current: number,
  target: number,
  dt: number,
  attackMs = 10,
  releaseMs = 120,
): number {
  if (!Number.isFinite(current)) current = 0;
  if (!Number.isFinite(target)) target = 0;
  if (dt <= 0) return current;
  const k = target <= current
    ? 1 - Math.exp(-dt / (attackMs / 1000))
    : 1 - Math.exp(-dt / (releaseMs / 1000));
  return current + (target - current) * k;
}

/** The multiband compressor's per-band gain-reduction meter range (dB). */
export const GR_DB_MIN = -60;
/** The multiband compressor's per-band gain-reduction meter range (dB). */
export const GR_DB_MAX = 0;

/** Convert a raw gain-reduction dB value into a 0..100 percent track fill. */
export function grDbToPercent(grDb: number, min = GR_DB_MIN, max = GR_DB_MAX): number {
  const v = Math.max(min, Math.min(max, grDb));
  return ((v - min) / (max - min)) * 100;
}
