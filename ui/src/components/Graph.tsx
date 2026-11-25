// A feature-agnostic curve editor: `<Graph<T>>`.
//
// Data in via props, callbacks out. No knowledge of the engine, the bridge, or
// the app state, so the same component serves the Graphic EQ response, the
// waveshaper transfer curve and the multiband compressor gain-reduction
// display.
//
// Props and types follow docs/design/graphic-eq.md. Rendering contract:
//   * The component measures its own box (ResizeObserver) and lays out an SVG
//     whose viewBox is computed from `width`/`minWidth` and `minHeight`;
//     nothing is fixed in px except the per-handle glyph sizes.
//   * Axes may be linear or log; tick labels use `units` and the `ticks` hint.
//   * Handles are real SVG/HTML elements (pointer + keyboard), `onChange`
//     fires on every position change, and they are keyboard accessible
//     (role="slider", arrow keys, aria attributes).
//   * Overlays are rendered by the host on the SVG.
//   * Series: line, area (flat base under the line), points, line+area.

import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import { clamp, logTicks, makeLinearScale, makeLogScale, niceTicks, type Scale } from "../graph/scale";
import type { JSX } from "preact";


// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

const MIN_WIDTH = 320; // px, default container width floor

/**
 * Reserve room for the axis text outside the plot:
 * - left/right for the x tick grid (the log axis's labels are the widest),
 * - top for the y axis label and tick grid,
 * - bottom for the x axis label and tick grid.
 */
export interface ChartPads {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export function chartPads(axes: GraphAxes<number>, xLabel?: string, yLabel?: string): ChartPads {
  let left = axes.x.type === "log" ? 88 : 64; // x tick labels + units
  const right = 16;
  let top = 12;
  let bottom = 40;
  if (xLabel) bottom += 26;
  if (yLabel) left += 26;
  return { left, right, top, bottom };
}

// ---------------------------------------------------------------------------
// Pixel helpers
// ---------------------------------------------------------------------------

/**
 * Position of sample `i` inside an axis domain (`n` samples across the
 * domain). For a log axis the samples are log-spaced (constant ratio), which
 * is what makes a log-frequency response curve land on the curve.
 */
export function domainSample(domain: [number, number], n: number, i: number, log = false): number {
  if (n < 2) return domain[0];
  const t = i / (n - 1);
  return log ? domain[0] * (domain[1] / domain[0]) ** t : domain[0] + t * (domain[1] - domain[0]);
}

/**
 * Build the SVG path (and the ordered pixel points) for one series.
 *
 * The series is drawn on the `y` axis by default (`yAxis: 'y'`); when
 * `yAxis: 'x'` the values map onto the x axis and the sample index maps onto
 * the y axis, which is how a control-point list of x,y pairs can be rendered
 * as a line.
 */
export interface SeriesDraw {
  kind: GraphSeries<number>["kind"];
  d: string;
  points: [number, number][];
}

export function buildSeries(
  s: GraphSeries<number>,
  xScale: Scale,
  yScale: Scale,
  xAxisLog: boolean,
): SeriesDraw {
  const { data, kind, yAxis = "y" } = s;
  const n = data.length;
  if (n === 0) return { kind, d: "", points: [] };

  const points: [number, number][] = [];
  for (let i = 0; i < n; i++) {
    const x = xScale.toPixel(domainSample(xScale.domain, n, i, xAxisLog));
    const y = yAxis === "x"
      ? yScale.toPixel(domainSample(yScale.domain, n, i, false))
      : yScale.toPixel(data[i]);
    points.push([x, y]);
  }

  let d = `M ${points[0][0].toFixed(2)} ${points[0][1].toFixed(2)}`;
  for (let i = 1; i < points.length; i++) {
    d += ` L ${points[i][0].toFixed(2)} ${points[i][1].toFixed(2)}`;
  }
  return { kind, d, points };
}

// ---------------------------------------------------------------------------
// Tick formatting
// ---------------------------------------------------------------------------

/**
 * Compact, readable number for a tick (k/M suffixes, no unit). The host
 * appends `units` afterwards, e.g. 1234 -> "1.2k" + "Hz" -> "1.2kHz".
 */
export function formatTickValue(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1_000_000_000_000) return (v / 1e12).toFixed(2) + "T";
  if (abs >= 1_000_000) return (v / 1e6).toFixed(2) + "M";
  if (abs >= 1_000) return (v / 1e3).toFixed(1) + "k";
  if (abs >= 10) return v.toFixed(1);
  if (abs >= 1) return v.toFixed(2);
  return v.toFixed(2);
}

// The shared types used by the three consumer editors (docs/design/graphic-eq.md).
// The shared types used by the three consumer editors (docs/design/graphic-eq.md).
export interface GraphAxis<T = number> {
  type: "linear" | "log";
  min: T;
  max: T;
  /** Display units rendered on the tick labels. */
  units?: string;
  /** Desired count of major ticks; the renderer picks a grid-aligned set. */
  ticks?: number;
}

export interface GraphAxes<T = number> {
  x: GraphAxis<T>;
  y: GraphAxis<T>;
}

/** One data series: line, filled area, points or line+area. */
export interface GraphSeries<T = number> {
  key: string;
  label: string;
  color: string;
  kind: "line" | "area" | "points" | "line+area";
  data: ReadonlyArray<T>;
  /** Which axis the values map onto (default 'y'). */
  yAxis?: "y" | "x";
}

/** A draggable handle (vertical guide + value readout). */
export interface GraphHandle<T = number> {
  id: string;
  /** The draggable value on `axis` (default 'y'). */
  value: T;
  label: string;
  color: string;
  /** Which axis the value maps onto (default 'y'). */
  axis?: "x" | "y";
  /** Fixed coordinate on the other axis (for a vertical guide). */
  x?: T;
  onChange: (id: string, value: T) => void;
}

/** A render overlay (crossover markers, threshold lines, y=x reference).
 *
 * The render function draws in the chart's LOCAL coordinate frame: the
 * origin is the top-left corner of the plot area (pads.left / pads.top of
 * the SVG), so vertical crossover markers and band labels land on the
 * frequencies/axes they annotate. `w`/`h` is the full SVG size. */
export interface GraphOverlay<T = number> {
  id: string;
  render: (w: number, h: number, axes: GraphAxes<T>, pads: ChartPads) => JSX.Element;
}

/** Props for `<Graph<T>>`. */
export interface GraphProps<T = number> {
  /** px; falls back to the container width (responsive). */
  width?: number;
  minWidth?: number;
  /** px fallback when the host needs an explicit height (default: auto via minHeight). */
  height?: number;
  /** CSS min-height (default "240px"). */
  minHeight?: string;
  axes: GraphAxes<T>;
  series: GraphSeries<T>[];
  /** Optional draggable handles; `onChange` is called while dragging. */
  handles?: GraphHandle<T>[];
  overlays?: ReadonlyArray<GraphOverlay<T>>;
  grid?: boolean;
  zeroLine?: boolean;
  xLabel?: string;
  yLabel?: string;
  /** Crosshair passed in by the host on mouse move. */
  cursor?: { value: T; label: string } | null;
  /** Mousedown -> data value under the pointer (used for readouts). */
  onCursorChange?: (value: T) => void;
  /** Required for log scales: domain must not contain zero or negatives. */
  dataDomain?: [T, T];
  className?: string;
}

// ---------------------------------------------------------------------------
// The component
// ---------------------------------------------------------------------------

export interface CursorValue {
  value: number;
  label: string;
}

export function Graph<T extends number = number>(props: GraphProps<T>): JSX.Element {
  const {
    width,
    minWidth = MIN_WIDTH,
    height,
    minHeight = "240px",
    axes,
    series,
    handles = [],
    overlays = [],
    grid = true,
    zeroLine = false,
    xLabel,
    yLabel,
    onCursorChange,
    dataDomain,
    className,
  } = props;

  const svgRef = useRef<SVGSVGElement>(null);
  const [box, setBox] = useState({ width: 0, height: 0 });
  const [dragging, setDragging] = useState<string | null>(null);
  const [crossHair, setCrossHair] = useState<{ x: number; y: number } | null>(null);

  // ---- measurement --------------------------------------------------------
  useEffect(() => {
    const el = svgRef.current?.parentElement;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setBox(() => ({ width: el.clientWidth, height: el.clientHeight }));
    });
    ro.observe(el);
    let raf = 0;
    const tick = () => {
      setBox(() => ({ width: el.clientWidth, height: el.clientHeight }));
      if (raf) cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        raf = 0;
        tick();
      });
    };
    tick();
    return () => {
      ro.disconnect();
      if (raf) cancelAnimationFrame(raf);
    };
  }, []);

  // ---- sizes --------------------------------------------------------------
  const parsedMinHeight = parseInt(minHeight, 10) || 240;
  const viewWidth = Math.max(minWidth, width ?? box.width);
  const viewHeight = height ?? Math.max(parsedMinHeight, box.height);

  const pads = chartPads(axes, xLabel, yLabel);
  const chartW = Math.max(12, viewWidth - pads.left - pads.right);
  const chartH = Math.max(12, viewHeight - pads.top - pads.bottom);

  // ---- scales -------------------------------------------------------------
  const xScale = axes.x.type === "log"
    ? makeLogScale([axes.x.min, axes.x.max], [0, chartW])
    : makeLinearScale([axes.x.min, axes.x.max], [0, chartW]);
  const yScale = axes.y.type === "log"
    ? makeLogScale([axes.y.min, axes.y.max], [chartH, 0])
    : makeLinearScale([axes.y.min, axes.y.max], [chartH, 0]);

  if ((axes.x.type === "log" || axes.y.type === "log") && (!dataDomain || dataDomain[0] <= 0 || dataDomain[1] <= 0)) {
    throw new RangeError("Graph: a log axis requires dataDomain with both bounds > 0");
  }

  // Tick sets: log axes get a complete log-spaced grid (first label on min,
  // last label on max); linear axes get a "nice" rounded step.
  const xTickCount = axes.x.ticks ?? (axes.x.type === "log" ? 8 : 6);
  const yTickCount = axes.y.ticks ?? 6;
  const xTicks = axes.x.type === "log"
    ? logTicks(axes.x.min, axes.x.max, xTickCount)
    : niceTicks(axes.x.min, axes.x.max, xTickCount);
  const yTicks = axes.y.type === "log"
    ? logTicks(axes.y.min, axes.y.max, yTickCount)
    : niceTicks(axes.y.min, axes.y.max, yTickCount);

  // The line a series is filled down to: 0 when the y axis spans 0 (so an
  // EQ response fills around the 0 line instead of the plot's bottom edge),
  // otherwise the axis minimum.
  const areaBase = (): number =>
    yScale.domain[0] <= 0 && yScale.domain[1] >= 0 ? 0 : yScale.domain[0];

  // ---- events -------------------------------------------------------------
  const chartRect = useCallback(() => {
    const el = svgRef.current;
    if (!el) return { left: 0, top: 0, width: viewWidth, height: viewHeight };
    const r = el.getBoundingClientRect();
    return { left: r.left, top: r.top, width: r.width, height: r.height };
  }, [viewWidth, viewHeight]);

  const moveCursor = useCallback(
    (e: PointerEvent) => {
      const rect = chartRect();
      if (e.clientX < rect.left || e.clientX > rect.left + rect.width ||
          e.clientY < rect.top || e.clientY > rect.top + rect.height) return;
      const px = clamp(e.clientX - rect.left, 0, rect.width);
      const py = clamp(e.clientY - rect.top, 0, rect.height);
      setCrossHair({ x: px, y: py });
      // The host reads the data value under the pointer (used for readouts
      // such as the current band in the Graphic EQ).
      if (onCursorChange) onCursorChange(xScale.toValue(px) as T);
    },
    [chartRect, xScale, yScale, onCursorChange],
  );

  const startDrag = useCallback((id: string, e: PointerEvent) => {
    e.preventDefault();
    (e.target as Element).setPointerCapture(e.pointerId);
    setDragging(id);
  }, []);

  const dragMove = useCallback((e: PointerEvent) => {
    if (!dragging) return;
    const rect = chartRect();
    const px = clamp(e.clientX - rect.left, 0, rect.width);
    const py = clamp(e.clientY - rect.top, 0, rect.height);
    const h = handles.find((hd) => hd.id === dragging);
    if (!h) return;
    // The value maps to the chosen axis: handle.value -> `axis` (default 'y'),
    // handle.x -> the other axis.
    const value = h.axis === "x" ? xScale.toValue(px) : yScale.toValue(py);
    // Drag clamping: the value is clamped to the mapped axis domain.
    const clamped = clamp(value, h.axis === "x" ? xScale.domain[0] : yScale.domain[0], h.axis === "x" ? xScale.domain[1] : yScale.domain[1]);
    h.onChange(h.id, clamped as T);
  }, [dragging, handles, xScale, yScale]);

  const endDrag = useCallback(() => setDragging(null), []);

  // ---- render -------------------------------------------------------------
  const xAxisLog = axes.x.type === "log";
  return (
    <div className={`graph ${className ?? ""}`}>
      <svg
        ref={svgRef}
        className="graph-svg h-full w-full"
        viewBox={`0 0 ${viewWidth} ${viewHeight}`}
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label={`${axes.x.units ?? ""} ${axes.x.min} to ${axes.x.max}  ·  ${axes.y.units ?? ""} ${axes.y.min} to ${axes.y.max}`}
        onPointerMove={moveCursor}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <desc>{`${series.length} series, ${handles.length} handles, ${overlays.length} overlays`}</desc>

        {/* Chart background + grid + axis lines */}
        <rect
          x={pads.left}
          y={pads.top}
          width={chartW}
          height={chartH}
          fill="var(--color-panel)"
          stroke="var(--color-line-soft)"
          strokeWidth={1}
        />
        {grid && (
          <g className="graph-grid">
            {/* vertical grid + x ticks */}
            {xTicks.map((t) => {
              const x = xScale.toPixel(t);
              return (
                <g key={`x${t}`}>
                  <line x1={x} y1={pads.top} x2={x} y2={pads.top + chartH} stroke="var(--color-line-soft)" strokeWidth={1} strokeDasharray="3 4" />
                  <text
                    x={x}
                    y={pads.top + chartH + 18}
                    textAnchor="middle"
                    dominantBaseline="ideographic"
                    fill="var(--color-muted)"
                    fontSize={10}
                  >
                    {formatTickValue(t)} {axes.x.units ?? ""}
                  </text>
                </g>
              );
            })}
            {/* horizontal grid + y ticks */}
            {yTicks.map((t) => {
              const y = yScale.toPixel(t);
              return (
                <g key={`y${t}`}>
                  <line x1={pads.left} y1={y} x2={pads.left + chartW} y2={y} stroke="var(--color-line-soft)" strokeWidth={1} strokeDasharray="3 4" />
                  <text
                    x={pads.left - 10}
                    y={y + 4}
                    textAnchor="end"
                    dominantBaseline="middle"
                    fill="var(--color-muted)"
                    fontSize={10}
                  >
                    {formatTickValue(t)} {axes.y.units ?? ""}
                  </text>
                </g>
              );
            })}
          </g>
        )}

        {/* Zero line */}
        {zeroLine && yScale.domain[0] <= 0 && yScale.domain[1] >= 0 && (
          <g>
            <line x1={pads.left} y1={yScale.toPixel(0)} x2={pads.left + chartW} y2={yScale.toPixel(0)} stroke="var(--color-amber)" strokeWidth={1.5} strokeDasharray="5 4" />
            <text x={(pads.left + chartW) / 2} y={yScale.toPixel(0) - 8} textAnchor="middle" fill="var(--color-muted)" fontSize={10}>0</text>
          </g>
        )}

        {/* Axis labels */}
        {yLabel && (
          <text
            x={8}
            y={pads.top + chartH / 2}
            textAnchor="middle"
            transform={`rotate(-90 8 ${pads.top + chartH / 2})`}
            fill="var(--color-muted)"
            fontSize={11}
          >
            {yLabel}
          </text>
        )}
        {xLabel && (
          <text
            x={pads.left + chartW / 2}
            y={viewHeight - 8}
            textAnchor="middle"
            fill="var(--color-muted)"
            fontSize={11}
          >
            {xLabel}
          </text>
        )}

        {/* Series */}
        {series.map((s) => {
          const { d, points } = buildSeries(s, xScale, yScale, xAxisLog);
          const base = areaBase();
          const last = points[points.length - 1];
          const first = points[0];
          const areaPath = d + ` L ${last[0].toFixed(2)} ${base.toFixed(2)} L ${first[0].toFixed(2)} ${base.toFixed(2)} Z`;
          return (
            <g key={s.key}>
              {s.kind === "line" && <path d={d} fill="none" stroke={s.color} strokeWidth={2} />}
              {s.kind === "area" && <path d={d} fill={s.color} opacity={0.25} stroke={s.color} strokeWidth={2} />}
              {s.kind === "points" && (
                <>
                  <path d={d} fill="none" stroke={s.color} strokeWidth={1.5} />
                  {points.map((p, i) => (
                    <circle key={i} cx={p[0]} cy={p[1]} r={2.5} fill={s.color} />
                  ))}
                </>
              )}
              {s.kind === "line+area" && (
                <>
                  <path d={areaPath} fill={s.color} opacity={0.2} stroke={s.color} strokeWidth={2} />
                  <path d={d} fill="none" stroke={s.color} strokeWidth={2} />
                </>
              )}
            </g>
          );
        })}

        {/* Handles */}
        {handles.map((h) => {
          // For a y-axis handle (the default) the guide is a vertical line at
          // `x` and the knob moves up and down; for an x-axis handle the guide
          // is horizontal at `y` and the knob moves left and right. The other
          // coordinate of the knob is the guide position, so the knob always
          // sits on the guide line.
          const axisScale = h.axis === "x" ? xScale : yScale;
          const guideX = h.axis === "x" ? undefined : xScale.toPixel(h.x ?? 0);
          const guideY = h.axis === "y" ? undefined : yScale.toPixel(h.x ?? 0);
          const value = h.value as number;
          const handleX = h.axis === "x" ? xScale.toPixel(value) : undefined;
          const handleY = h.axis === "y" ? yScale.toPixel(value) : undefined;
          const min = axisScale.domain[0];
          const max = axisScale.domain[1];
          return (
            <g
              key={h.id}
              role="slider"
              aria-label={h.label}
              aria-valuenow={value}
              aria-valuemin={min}
              aria-valuemax={max}
              aria-orientation="vertical"
              tabIndex={0}
              className="graph-handle"
              onPointerDown={(e) => startDrag(h.id, e)}
              onPointerMove={dragMove}
              onPointerUp={endDrag}
              onPointerCancel={endDrag}
              onKeyDown={(e) => {
                // Keyboard moves: active only while this handle is being dragged,
                // or when it holds focus (arrow-key accessibility) and no drag is
                // in flight.
                if (dragging !== h.id && !e.currentTarget.contains(e.target as Node | null)) return;
                let step = (max - min) / 200;
                if (step < 1e-6) step = 0.01;
                let v = value;
                switch (e.key) {
                  case "ArrowUp":
                    v = clamp(v + step, min, max); break;
                  case "ArrowDown":
                    v = clamp(v - step, min, max); break;
                  case "ArrowLeft":
                    v = clamp(v - step, min, max); break;
                  case "ArrowRight":
                    v = clamp(v + step, min, max); break;
                  default: return;
                }
                e.preventDefault();
                h.onChange(h.id, v as T);
              }}
            >
              {/* guide line: vertical when the value maps to y, horizontal when
                  it maps to x. `guideX`/`guideY` is the fixed side; the knob is
                  placed on it. */}
              {guideX === undefined ? (
                <line x1={handleX ?? 0} y1={pads.top} x2={handleX ?? 0} y2={pads.top + chartH} stroke={h.color} strokeWidth={1} strokeDasharray="4 4" opacity={0.7} />
              ) : (
                <line x1={pads.left} y1={guideY ?? 0} x2={pads.left + chartW} y2={guideY ?? 0} stroke={h.color} strokeWidth={1} strokeDasharray="4 4" opacity={0.7} />
              )}
              {/* knob: sits at the intersection of the guide and the value. */}
              <circle
                cx={handleX ?? guideX ?? 0}
                cy={handleY ?? guideY ?? 0}
                r={8}
                fill={h.color}
                stroke="var(--color-bg)"
                strokeWidth={2}
              />
              <circle cx={handleX ?? guideX ?? 0} cy={handleY ?? guideY ?? 0} r={4} fill="var(--color-bg)" />
              <text
                x={handleX ?? guideX ?? 0}
                y={handleY ?? guideY ?? 0}
                fill={h.color}
                fontSize={10}
                textAnchor="start"
                dominantBaseline="central"
              >
                {h.label}
              </text>
            </g>
          );
        })}

        {/* Crosshair */}
        {crossHair && (
          <g className="graph-crosshair" pointerEvents="none">
            <line x1={crossHair.x} y1={pads.top} x2={crossHair.x} y2={pads.top + chartH} stroke="var(--color-amber)" strokeWidth={1} strokeDasharray="3 3" opacity={0.8} />
            <line x1={pads.left} y1={crossHair.y} x2={pads.left + chartW} y2={crossHair.y} stroke="var(--color-amber)" strokeWidth={1} strokeDasharray="3 3" opacity={0.8} />
            <circle cx={crossHair.x} cy={crossHair.y} r={3} fill="var(--color-amber)" opacity={0.9} />
          </g>
        )}

        {/* Host overlays */}
        {overlays.map((ov) => (
          <g key={ov.id}>{ov.render(viewWidth, viewHeight, axes, pads)}</g>
        ))}
      </svg>
    </div>
  );
}

export default Graph;
