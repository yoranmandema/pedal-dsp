import { useMemo, useState } from "preact/hooks";
import { Graph, type ChartPads, type GraphAxes, type GraphHandle, type GraphOverlay } from "./Graph";
import type { JSX } from "preact";

/** One editable control point of the shaper (x fixed, y draggable). */
export interface ShaperPoint {
  x: number;
  y: number;
}

/**
 * Nominal control-point count for the shaper editor.  The x positions are
 * evenly spaced across [-1, 1]; each handle IS one control point (its value
 * is the y, clamped to [-1, 1]).
 */
export const SHAPER_POINT_COUNT = 41;

/** The tone category hue (ember red, per ui/src/theme.ts). */
const SHAPER_COLOR = "hsl(10 62% 58%)";

// Nominal host sample rate for bounds checks (kept here for parity with the
// reference design; the response is computed in dB and is sample-rate
// invariant at the dB values shown).

/**
 * Evaluate the piecewise-linear transfer curve at `x` through the control
 * points (sorted by x).
 */
function shaperAt(x: number, points: readonly ShaperPoint[]): number {
  // Clamp x into the control-point domain.
  let first = points[0];
  let last = points[points.length - 1];
  if (x <= first.x) return first.y;
  if (x >= last.x) return last.y;
  for (let i = 0; i < points.length - 1; i++) {
    const a = points[i];
    const b = points[i + 1];
    if (x >= a.x && x <= b.x) {
      const t = (x - a.x) / (b.x - a.x);
      return a.y + t * (b.y - a.y);
    }
  }
  return last.y;
}

/**
 * Build the transfer-curve data (200 linear samples in [-1, 1]) from the
 * control points. Derived on every render, so dragging a handle updates the
 * curve in real time.
 */
function shaperTransfer(points: readonly ShaperPoint[]): number[] {
  return Array.from({ length: 200 }, (_, i) => {
    const x = -1 + (2 * i) / 199; // 200 linearly spaced x in [-1, 1]
    return shaperAt(x, points);
  });
}

/** Wrap a raw overlay render function as the { id, render } overlay object. */
const ov = (
  id: string,
  render: (w: number, h: number, axes: GraphAxes<number>, pads: ChartPads) => JSX.Element,
): GraphOverlay<number> => ({ id, render });

/**
 * The y = x reference line (dashed, muted) plus the clip-envelope shading
 * (light fill near the edges where the shaper clamps).
 */
function shaperOverlays(
  w: number,
  h: number,
  _axes: GraphAxes<number>,
  pads: ChartPads,
): JSX.Element {
  const { left, top, right, bottom } = pads;
  const chartW = w - left - right;
  const chartH = h - top - bottom;

  return (
    <g>
      {/* y = x reference diagonal, dashed. */}
      <line
        x1={left}
        y1={top}
        x2={left + chartW}
        y2={top + chartH}
        stroke="var(--color-muted)"
        strokeWidth={1}
        strokeDasharray="4 4"
        opacity={0.5}
      />
      <text
        x={left + chartW / 2}
        y={top + chartH / 2 - 6}
        textAnchor="middle"
        fill="var(--color-muted)"
        fontSize={9}
      >
        y = x
      </text>
      {/* Clip-envelope shading: light fill at the edges where the transfer is
          clamped to [-1, 1]. Left/right strips near x = ±1. */}
      <rect
        x={left}
        y={top}
        width={0.18 * chartW}
        height={chartH}
        fill="rgba(232,163,61,0.08)"
        stroke="none"
      />
      <rect
        x={left + 0.82 * chartW}
        y={top}
        width={0.18 * chartW}
        height={chartH}
        fill="rgba(232,163,61,0.08)"
        stroke="none"
      />
    </g>
  );
}

interface WaveShaperEditorProps {
  /** Control points (x, y in [-1, 1]); x values are evenly spaced by the editor. */
  points: ReadonlyArray<{ x: number; y: number }>;
  /** Called with the full control-point list whenever a point moves. */
  onChange: (points: ReadonlyArray<{ x: number; y: number }>) => void;
}

/**
 * Draggable control points are rendered as `<Graph>` handles with a fixed `x`
 * (the evenly spaced input position) and a draggable `value` (the output y).
 * Each handle IS a control point of the shaper, so `onChange` mutates the
 * shaper's control-point list directly (the shaper owns its parameters).
 */
export function WaveShaperEditor({ points, onChange }: WaveShaperEditorProps) {
  // Local state owns the control points; the prop seeds it.
  const [localPoints, setLocalPoints] = useState<ShaperPoint[]>(() =>
    points.map((p) => ({ x: p.x, y: p.y })),
  );

  const pointsRef = useMemo(() => localPoints, [localPoints]);

  // Transfer curve derived from the current control points.
  const transfer = useMemo(() => shaperTransfer(pointsRef), [pointsRef]);

  // 41 evenly spaced x positions in [-1, 1].
  const xPositions = useMemo(() => {
    const xs: number[] = [];
    for (let i = 0; i < SHAPER_POINT_COUNT; i++) {
      xs.push(-1 + (2 * i) / (SHAPER_POINT_COUNT - 1));
    }
    return xs;
  }, []);

  // Clamp a dragged y to [-1, 1] and update the control-point list.
  const updatePoint = (index: number, v: number): void => {
    const clamped = Math.max(-1, Math.min(1, v));
    setLocalPoints((prev) => {
      const next = prev.map((p, i) => (i === index ? { x: p.x, y: clamped } : p));
      onChange(next as ReadonlyArray<{ x: number; y: number }>);
      return next;
    });
  };

  const shaperHandles: GraphHandle<number>[] = xPositions.map((x, i) => ({
    id: `shp-${i}`,
    // x fixed at the evenly spaced input position (vertical guide).
    x,
    value: pointsRef[i]?.y ?? 0,
    label: `P${i + 1}`,
    color: SHAPER_COLOR,
    onChange: (_id: string, v: number) => updatePoint(i, v),
  }));

  return (
    <div class="wave-shaper-editor">
      <div class="wave-shaper-graph">
        <Graph
          axes={{
            x: { type: "linear", min: -1, max: 1, units: "" },
            y: { type: "linear", min: -1, max: 1, units: "" },
          }}
          series={[{
            key: "shaper-transfer",
            label: "Transfer",
            color: SHAPER_COLOR,
            kind: "line+area",
            data: transfer,
          }]}
          handles={shaperHandles}
          overlays={[ov("shaper-clip", shaperOverlays)]}
          xLabel="in"
          yLabel="out"
        />
      </div>
    </div>
  );
}
