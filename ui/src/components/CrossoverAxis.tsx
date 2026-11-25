// CrossoverAxis — a draggable, keyboard-controlled logarithmic crossover
// frequency axis for the multiband compressor panel.
//
// The axis spans ~20 Hz … 20 kHz (log), shows tick labels, shades the band
// regions, and places one draggable marker per active crossover edge
// (`bands - 1`, derived from the block's `bands` param). Each marker is bound
// to its engine parameter (`crossover_1…N`) and dispatches `setParam` on every
// move, so the engine is the single source of truth again.
//
// Pointer dragging uses pointer events with capture (same gesture the rest of
// the UI uses for touch). Keyboard: ArrowLeft/Right = fine steps,
// Shift+arrows = coarse steps, PageUp/PageDown = coarse steps, Home/End = jump
// to the parameter's min/max. Markers enforce ascending order and a 1-octave
// minimum spacing, and clamp to each parameter's range.
//
// Accessibility: each marker is a `role="slider"` with `aria-label`,
// `aria-valuemin/max/now/valuetext`, `tabIndex={0}` and full keyboard support.

import { useEffect, useMemo, useRef, useState, useCallback } from "preact/hooks";
import { state, setParam } from "../state";
import { logTicks, makeLogScale, type Scale } from "../graph/scale";
import {
  activeCrossoverParamIds,
  formatCrossover,
  formatTickValue,
  resolveCrossoverValues,
  type CrossoverParam,
} from "./crossover";
import type { State } from "../protocol";

import "./crossover-axis.css";

export interface CrossoverAxisProps {
  /** Channel the multiband block lives in. */
  channel: string;
  /** Multiband compressor block id. */
  block: string;
  /** Optional extra class on the root. */
  className?: string;
}

// --- Constants -------------------------------------------------------------
const BLOCK_TYPE = "dynamics.multiband_compressor";
const AXIS_MIN = 20;
const AXIS_MAX = 20000;
const TICK_COUNT = 8;

// Static tick set (same layout for every axis instance).
const AXIS_TICKS = logTicks(AXIS_MIN, AXIS_MAX, TICK_COUNT);

/** Log ratios for keyboard step sizes. */
const FINE_RATIO = 1.05;
const COARSE_SHIFT_RATIO = 1.15;
const COARSE_PAGE_RATIO = 2.0;

// --- Derivation -------------------------------------------------------------
interface DerivedCrossover {
  paramIds: readonly string[];
  specs: readonly CrossoverParam[];
  values: readonly number[];
}

/**
 * Read the multiband block from the global state and derive the ordered
 * crossover set. Falls back to the parameter defaults while the engine
 * registry snapshot hasn't arrived yet.
 */
function deriveCrossover(
  s: State | null,
  channel: string,
  block: string,
): DerivedCrossover | null {
  if (!s) return null;
  const channelObj = s.channels.find((c) => c.id === channel);
  if (!channelObj) return null;
  const blockObj = channelObj.blocks.find((b) => b.id === block);
  if (!blockObj) return null;

  const spec = s.registry[BLOCK_TYPE];
  if (!spec) return null;

  const paramIds = activeCrossoverParamIds(blockObj.params["bands"] ?? 3);
  const specs = paramIds.map((id) => {
    const p = spec.params.find((x) => x.id === id);
    return p ?? { id, min: 0, max: 0, default: 0 };
  });
  const raw = specs.map((p) => blockObj.params[p.id] ?? p.default);
  const values = resolveCrossoverValues(raw, specs);
  return { paramIds, specs, values };
}

// --- Component -------------------------------------------------------------
export function CrossoverAxis({ channel, block, className }: CrossoverAxisProps) {
  const track = useRef<HTMLDivElement>(null);
  const drag = useRef<{ index: number; pointerId: number } | null>(null);
  const [measure, setMeasure] = useState({ width: 0 });
  const [values, setValues] = useState<readonly number[]>([]);
  const valuesRef = useRef<readonly number[]>([]);

  const derived = deriveCrossover(state.value, channel, block);
  if (derived) {
    valuesRef.current = derived.values;
    if (values.length === 0) setValues(derived.values);
  }

  const xScale: Scale = useMemo(() => {
    if (measure.width <= 0) return makeLogScale([AXIS_MIN, AXIS_MAX], [0, 1]);
    return makeLogScale([AXIS_MIN, AXIS_MAX], [0, measure.width]);
  }, [measure.width]);

  // --- responsive measurement (ResizeObserver) ------------------------------
  useEffect(() => {
    const el = track.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setMeasure({ width: el.clientWidth });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // --- dragging -------------------------------------------------------------
  const startDrag = (index: number, e: PointerEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    e.preventDefault();
    drag.current = { index, pointerId: e.pointerId };
  };

  const moveDrag = useCallback(
    (e: PointerEvent) => {
      const d = drag.current;
      if (!d || e.pointerId !== d.pointerId) return;
      const rect = track.current?.getBoundingClientRect();
      if (!rect) return;
      const px = clamp(e.clientX - rect.left, 0, rect.width);
      const v = xScale.toValue(px);
      const next = [...valuesRef.current];
      next[d.index] = v;
      const resolved = resolveCrossoverValues(next, derived!.specs);
      valuesRef.current = resolved;
      setValues(resolved);
    },
    [xScale, derived?.specs],
  );

  const endDrag = useCallback(() => {
    drag.current = null;
  }, []);

  // --- keyboard -------------------------------------------------------------
  const onKeyDown = (index: number) => (e: KeyboardEvent) => {
    const cur = valuesRef.current[index];
    if (cur == null) return;
    const specs = derived?.specs;
    if (!specs) return;
    let raw: number;
    if (e.key === "Home") {
      // Jump to the parameter's minimum.
      raw = specs[index].min;
    } else if (e.key === "End") {
      // Jump to the parameter's maximum.
      raw = specs[index].max;
    } else {
      let ratio = FINE_RATIO;
      if (e.shiftKey) ratio = COARSE_SHIFT_RATIO;
      else if (e.key === "PageUp" || e.key === "PageDown") ratio = COARSE_PAGE_RATIO;
      raw = cur * ratio;
    }
    const nextValuesArr = [...valuesRef.current];
    nextValuesArr[index] = raw;
    // Same resolution as dragging: clamp to ranges, then enforce ascending
    // order and the minimum octave spacing across the whole set.
    const next = resolveCrossoverValues(nextValuesArr, specs);
    valuesRef.current = next;
    setValues(next);
    if (derived) {
      void setParam(channel, block, derived.paramIds[index], next[index]);
    }
    e.preventDefault();
  };

  return (
    <div className={`crossover-axis ${className ?? ""}`}>
      <div className="crossover-axis-track" ref={track}>
        {/* Tick labels (log, 20 Hz … 20 kHz). */}
        <div className="crossover-tick-labels">
          {AXIS_TICKS.map((t) => (
            <div
              key={`${t}`}
              className="crossover-tick-label"
              style={{ left: `${xScale.toPixel(t)}px` }}
            >
              {formatTickValue(t)}
            </div>
          ))}
        </div>

        {/* Band-region shading. */}
        <div className="crossover-shading">
          {derived && !drag.current && (
            <>
              {[...Array(derived.values.length + 1)].map((_, i) => {
                const leftF =
                  i === 0 ? AXIS_MIN : derived.values[i - 1];
                const rightF =
                  i === derived.values.length
                    ? AXIS_MAX
                    : derived.values[i];
                const left = xScale.toPixel(
                  i === 0 ? AXIS_MIN : Math.max(leftF, AXIS_MIN),
                );
                const right = xScale.toPixel(
                  i === derived.values.length ? AXIS_MAX : rightF,
                );
                const lightness = 46 + Math.min(i, 3) * 4;
                return (
                  <div
                    key={`r${i}`}
                    className="crossover-region"
                    style={{
                      left: `${left}px`,
                      width: `${Math.max(1, right - left)}px`,
                      background: `hsl(152 55% ${lightness}%)`,
                    }}
                  />
                );
              })}
            </>
          )}
        </div>

        {/* Draggable crossover markers. */}
        <div className="crossover-markers">
          {derived &&
            derived.values.map((v, i) => {
              const min = derived.specs[i].min;
              const max = derived.specs[i].max;
              const px = xScale.toPixel(v);
              return (
                <div
                  key={`${derived.paramIds[i]}_${v}`}
                  className="crossover-marker"
                  role="slider"
                  aria-label={`${derived.paramIds[i]}, ${formatCrossover(v)}`}
                  aria-valuemin={min}
                  aria-valuemax={max}
                  aria-valuenow={v}
                  aria-valuetext={formatCrossover(v)}
                  aria-orientation="horizontal"
                  tabIndex={0}
                  style={{ left: `${px}px` }}
                  onKeyDown={onKeyDown(i)}
                  onPointerDown={(e) => startDrag(i, e)}
                  onPointerMove={moveDrag}
                  onPointerUp={endDrag}
                  onPointerCancel={endDrag}
                  >
                  <div className="crossover-marker-guide" />
                  <div
                    className="crossover-marker-knob"
                    aria-hidden="true"
                    title={`Drag to move ${derived.paramIds[i]}`}
                  >
                    <div className="crossover-marker-knob-ring" />
                    <div className="crossover-marker-knob-face" />
                  </div>
                  <div className="crossover-marker-value">
                    {formatCrossover(v)}
                  </div>
                </div>
              );
            })}
        </div>
      </div>
    </div>
  );
}

/** Clamp `v` into `[min, max]`. */
function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}
