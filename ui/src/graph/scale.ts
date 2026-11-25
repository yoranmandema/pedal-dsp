// Scale helpers for the graph components.
//
// Pure functions: no DOM, no JSX. Kept in their own module (rather than
// inside `Graph.tsx`) so the value<->pixel mapping — the part the renderer
// depends on — can be unit tested without a browser or a Preact renderer.
//
// `Graph.tsx` imports these; the three consumer editors do not need to import
// anything from here.

/** A map from data units to a pixel distance inside a chart area. */
export interface Scale {
  /** 'linear' or 'log' (base 10). */
  type: "linear" | "log";
  /** Data-unit domain, i.e. the axis. Log scales must not contain 0 or negatives. */
  domain: [number, number];
  /** Pixel range [0, pixels] where 0 is the axis minimum. */
  range: [number, number];
  /** data value -> pixel (0..rangeWidth). */
  toPixel: (v: number) => number;
  /** pixel -> data value. */
  toValue: (p: number) => number;
}

/** Linear data<->pixel map. Pixel 0 is `domain[0]`, pixel `range[1] - range[0]`
 *  is `domain[1]`. */
export function makeLinearScale(domain: [number, number], range: [number, number]): Scale {
  const [dMin, dMax] = domain;
  const [rMin, rMax] = range;
  const span = rMax - rMin;
  const denom = dMax - dMin;
  if (!(denom > 0)) {
    throw new RangeError(`Linear scale needs a non-zero domain span: [${dMin}, ${dMax}]`);
  }
  return {
    type: "linear",
    domain,
    range,
    toPixel: (v) => rMin + ((v - dMin) / denom) * span,
    toValue: (p) => dMin + ((p - rMin) / span) * denom,
  };
}

/** Logarithmic data<->pixel map (base 10). Pixel 0 is `domain[0]`, pixel
 *  `range[1] - range[0]` is `domain[1]`. Both domain bounds must be strictly
 *  positive; the scale maps `Math.log10(v)` linearly. */
export function makeLogScale(domain: [number, number], range: [number, number]): Scale {
  const [dMin, dMax] = domain;
  const [rMin, rMax] = range;
  if (!(dMin > 0 && dMax > 0)) {
    throw new RangeError(`Log scale domain must be strictly positive: [${dMin}, ${dMax}]`);
  }
  if (!(dMax > dMin)) {
    throw new RangeError(`Log scale needs a non-zero domain span: [${dMin}, ${dMax}]`);
  }
  const span = rMax - rMin;
  const logMin = Math.log10(dMin);
  const logMax = Math.log10(dMax);
  return {
    type: "log",
    domain,
    range,
    toPixel: (v) => rMin + ((Math.log10(v) - logMin) / (logMax - logMin)) * span,
    toValue: (p) => 10 ** (logMin + ((p - rMin) / span) * (logMax - logMin)),
  };
}

/** Clamp `v` to `[min, max]`. */
export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

/** Snap `v` to a whole step (e.g. 1 dB), keeping it inside the range. */
export function snap(v: number, step: number): number {
  if (step <= 0) return v;
  return Math.round(v / step) * step;
}

/**
 * "Nice" linear tick values spanning `[min, max]`, roughly `count` ticks.
 *
 *  Picks a round step (`1, 2, 2.5, 5, 10 × 10ⁿ`) so the ticks read cleanly on
 *  the axis, then draws the full set that covers the domain.
 */
export function niceTicks(min: number, max: number, count: number): number[] {
  const range = max - min;
  if (!(range > 0)) return [min, max];
  const rawStep = range / Math.max(1, Math.round(count) - 1);
  const mag = 10 ** Math.floor(Math.log10(rawStep));
  const norm = rawStep / mag;
  let nice: number;
  if (norm < 1.5) nice = 1;
  else if (norm < 3) nice = 2;
  else if (norm < 7) nice = 2.5;
  else nice = 5;
  const step = nice * mag;
  const first = Math.ceil(min / step) * step;
  const ticks: number[] = [];
  for (let v = first; v <= max + step * 0.5; v += step) {
    ticks.push(v);
  }
  return ticks;
}

/**
 * "Nice" logarithmic ticks spanning `[min, max]` (both strictly positive).
 *
 *  Ticks are equally spaced on a log axis (constant ratio between neighbours,
 *  the defining property of a log grid). `count` is the exact number of ticks
 *  spread across the domain, so the first label sits on `min` and the last on
 *  `max`: the tick set is always complete and the axis is never clipped.
 *  `count` is rounded to an integer and capped at a minimum of 2. */
export function logTicks(min: number, max: number, count: number): number[] {
  const decadeSpan = Math.log10(max / min);
  const target = Math.max(2, Math.round(count));
  // Constant ratio that spreads exactly `target` ticks across the decade span.
  const ratio = 10 ** (decadeSpan / (target - 1));
  const ticks: number[] = [];
  for (let v = min; v <= max * (1 + 1e-9); v *= ratio) {
    ticks.push(v);
  }
  return ticks;
}
