// Story page for the reusable <Graph<T>> curve-editor component.
//
// Three self-contained configurations (see docs/design/graphic-eq.md section
// 2.3), all rendered with the same presentational component so the axes,
// series kinds, handles and overlay layers can be compared side by side:
//
//   1. Log-frequency EQ response (dB vs Hz, 10 draggable band handles)
//   2. Wave shaper transfer curve (x/y in [-1, 1], draggable control points)
//   3. Multiband region (gain reduction, vertical crossover markers)
//
// Run:  pnpm dev  and open the "demo" entry (ui/src/demo/index.html), or build
// with `pnpm run build` (ui/dist is generated and never committed).

import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import {
  Graph,
  type GraphAxis,
  type GraphAxes,
  type ChartPads,
  type GraphOverlay,
} from "../components/Graph";
import type { JSX } from "preact";

// ---------------------------------------------------------------------------
// Design tokens: the demo lives on the app's own colour system (Category hues
// from `ui/src/theme.ts`, so a graph added to the chain reads as its category).
// ---------------------------------------------------------------------------

const EQ_COLOR = "hsl(206 62% 58%)"; // Time — blue
const SHAPER_COLOR = "hsl(24 82% 62%)"; // Control — orange
const MULTIBAND_COLOR = "hsl(152 62% 58%)"; // Dynamics — green

const EQ_BANDS = [31.5, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000] as const;

// ---------------------------------------------------------------------------
// 1. Log-frequency EQ response
// ---------------------------------------------------------------------------

/**
 * Steady-state magnitude of the 10-band ISO cascade. This is the same kind of
 * curve the Graphic EQ editor renders: a bell centred at 1 kHz whose height is
 * the gain of the band at 1 kHz, with a gentle dip at 250 Hz and a boost at
 * 8 kHz, all computed in the UI (the sound itself belongs to the engine block).
 */
function eqResponse(gains: readonly number[]): number[] {
  return Array.from({ length: 200 }, (_, i) => {
    const t = i / 199;
    const f = 20 * (20000 / 20) ** t; // 200 log-spaced frequencies
    const dist = Math.log2(f / 1000);
    const bell = Math.exp(-0.5 * (dist / 0.2) ** 2);
    const dip = -2.4 * Math.exp(-0.5 * (Math.log2(f / 250) / 0.45) ** 2);
    const boost = 3.2 * Math.exp(-0.5 * (Math.log2(f / 7000) / 0.55) ** 2);
    return Math.round((gains[5] * bell + dip + boost) * 10) / 10;
  });
}

const eqBandMarkers = (
  w: number,
  h: number,
  _axes: GraphAxes<number>,
  pads: ChartPads,
): JSX.Element => {
  const { left, top, right, bottom } = pads;
  const chartW = w - left - right;
  const chartH = h - top - bottom;
  return (
    <g>
      {EQ_BANDS.map((f, i) => {
        const x = left + (Math.log10(f / 20) / Math.log10(20000 / 20)) * chartW;
        return (
          <g key={f}>
            <line
              x1={x}
              y1={top}
              x2={x}
              y2={top + chartH}
              stroke="var(--color-amber)"
              strokeWidth={1}
              strokeDasharray="4 4"
              opacity={0.8}
            />
            <text
              x={x}
              y={top + chartH + 34}
              textAnchor="middle"
              fill="var(--color-muted)"
              fontSize={9}
            >
              Band {i + 1} @ {f} Hz
            </text>
          </g>
        );
      })}
    </g>
  );
};

// ---------------------------------------------------------------------------
// 2. Wave shaper transfer curve
// ---------------------------------------------------------------------------

/** One editable control point of the shaper (x fixed, y draggable). */
interface ShaperPoint {
  x: number;
  y: number;
}

const SHAPER_POINTS: ShaperPoint[] = [
  { x: -0.6, y: -0.82 },
  { x: 0, y: 0 },
  { x: 0.6, y: 0.82 },
];

/**
 * Evaluate the piecewise-linear transfer curve at `x`. The curve is built by
 * hand from a few handles, so it is the true "editor" data: the series is
 * derived from these values on every render.
 */
function shaperAt(x: number, active: readonly number[]): number {
  // Each control point is a [x, y] pair; typing as number[][] (rather than
  // the tuple [number, number][]) keeps the spread-spread map element
  // ([p.x, active[i] ?? p.y]) assignable without a cast.
  const pts: number[][] = [
    [-1, -1],
    ...SHAPER_POINTS.map((p, i) => [p.x, active[i] ?? p.y]),
    [1, 1],
  ];
  for (let i = 0; i < pts.length - 1; i++) {
    const [x1, y1] = pts[i];
    const [x2, y2] = pts[i + 1];
    if (x >= x1 && x <= x2) {
      return y1 + (y2 - y1) * ((x - x1) / (x2 - x1));
    }
  }
  return pts[0][1];
}

const shaperTransfer = (
  active: readonly number[],
): number[] =>
  Array.from({ length: 200 }, (_, i) => {
    const x = -1 + (2 * i) / 199;
    return shaperAt(x, active);
  });

const yEqualsX = (
  w: number,
  h: number,
  _axes: GraphAxes<number>,
  pads: ChartPads,
): JSX.Element => {
  const { left, top, right, bottom } = pads;
  const chartW = w - left - right;
  const chartH = h - top - bottom;
  return (
    <g>
      <line
        x1={left}
        y1={top}
        x2={left + chartW}
        y2={top + chartH}
        stroke="var(--color-line-soft)"
        strokeWidth={1}
        strokeDasharray="4 4"
        opacity={0.8}
      />
      <line
        x1={left}
        y1={top}
        x2={left + chartW}
        y2={top + chartH}
        stroke="var(--color-muted)"
        strokeWidth={1}
        strokeDasharray="4 4"
        opacity={0.35}
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
      {/* The clip envelope: the two corners the shaper soft-clips around. */}
      <rect
        x={left + 0.82 * chartW}
        y={top}
        width={0.18 * chartW}
        height={chartH}
        fill="rgba(232,163,61,0.07)"
        stroke="none"
      />
      <rect
        x={left}
        y={top}
        width={0.18 * chartW}
        height={chartH}
        fill="rgba(232,163,61,0.07)"
        stroke="none"
      />
    </g>
  );
};

// ---------------------------------------------------------------------------
// 3. Multiband region with vertical crossover markers
// ---------------------------------------------------------------------------

const CROSSOVERS = [31.5, 500, 4000, 16000] as const;

/**
 * A gain-reduction curve: ~0 dB below the first crossover, peaking around the
 * middle of the band, back to ~0 above the last one.
 */
const multibandGr = (): number[] => {
  const gains: number[] = [];
  for (let i = 0; i < 200; i++) {
    const t = i / 199;
    const f = 30 * (16000 / 30) ** t;
    let g = 0;
    for (const c of CROSSOVERS) {
      if (f < c) {
        const m = (f / c - 1) / 0.5;
        g += -8 * Math.exp(-0.5 * m * m);
        break;
      }
    }
    gains.push(Math.round(g * 10) / 10);
  }
  return gains;
};

const crossoverMarkers = (
  w: number,
  h: number,
  _axes: GraphAxes<number>,
  pads: ChartPads,
): JSX.Element => {
  const { left, top, right, bottom } = pads;
  const chartW = w - left - right;
  const chartH = h - top - bottom;
  const logX = (f: number) =>
    left + (Math.log10(f / 30) / Math.log10(16000 / 30)) * chartW;
  const fmt = (f: number): string => (f >= 1000 ? `${(f / 1000).toFixed(1)}k` : String(f));

  return (
    <g>
      {CROSSOVERS.map((f) => (
        <g key={f}>
          <line
            x1={logX(f)}
            y1={top}
            x2={logX(f)}
            y2={top + chartH}
            stroke="var(--color-amber)"
            strokeWidth={1}
            strokeDasharray="4 4"
            opacity={0.85}
          />
          <text
            x={logX(f)}
            y={top - 8}
            textAnchor="middle"
            fill="var(--color-muted)"
            fontSize={9}
          >
            {fmt(f)} Hz
          </text>
        </g>
      ))}
      <text
        x={left + chartW / 3}
        y={top + chartH / 2 - 6}
        textAnchor="middle"
        fill="var(--color-muted)"
        fontSize={10}
      >
        LOW
      </text>
      <text
        x={left + 2 * chartW / 3}
        y={top + chartH / 2 - 6}
        textAnchor="middle"
        fill="var(--color-muted)"
        fontSize={10}
      >
        HIGH
      </text>
      <text
        x={left + chartW / 2}
        y={top + chartH + 34}
        textAnchor="middle"
        fill="var(--color-muted)"
        fontSize={9}
      >
        crossovers
      </text>
    </g>
  );
};

// ---------------------------------------------------------------------------
// Story shell
// ---------------------------------------------------------------------------

  /** Wrap a raw overlay render function as the { id, render } overlay object. */
  const ov = (
    id: string,
    render: (w: number, h: number, axes: GraphAxes<number>, pads: ChartPads) => JSX.Element,
  ): GraphOverlay<number> => ({ id, render });

export function GraphStory() {
  // ---- 1 ----
  const graphRef = useRef<SVGSVGElement>(null);
  const [eqGains, setEqGains] = useState<readonly number[]>(
    Array.from({ length: 10 }, () => 0),
  );
  useEffect(() => {
    // eslint-disable-next-line no-console
    console.log("Graph story mounted", graphRef.current);
  }, []);
  const setEqGain = useCallback(
    (i: number, v: number) => {
      setEqGains((prev) => prev.map((_, j) => (j === i ? v : prev[j])));
    },
    [],
  );
  const eqResponseData = eqResponse(eqGains);
  const eqHandles = EQ_BANDS.map((f, i) => ({
    id: `eq-${i}`,
    x: f,
    value: eqGains[i] ?? 0,
    label: `Band ${i + 1}`,
    color: EQ_COLOR,
    onChange: (_id: string, v: number) => setEqGain(i, v),
  }));

  // ---- 2 ----
  const [shaper, setShaper] = useState<readonly number[]>(
    SHAPER_POINTS.map((p) => p.y),
  );
  const shaperData = shaperTransfer(shaper);

  const shaperHandle = (i: number, x: number) => ({
    id: `shp-${i}`,
    x,
    value: shaper[i] ?? 0,
    label: `P${i + 1}`,
    color: SHAPER_COLOR,
    onChange: (_id: string, v: number) => {
      setShaper((prev) => prev.map((_, j) => (j === i ? v : prev[j])));
    },
  });

  // ---- 3 ----
  const multibandData = multibandGr();

  const axis = (x: GraphAxis<number>, y: GraphAxis<number>): GraphAxes<number> => ({ x, y });

  return (
    <div class="story bg-stage min-h-screen text-text">
      <header class="mx-auto max-w-7xl px-4 py-5">
        <h1 class="text-2xl font-semibold tracking-[2px]">Graph &lt;T&gt;</h1>
        <p class="mt-1 text-muted">
          Reusable curve editor — the same presentational component serves Graphic
          EQ, the waveshaper and the multiband compressor.
        </p>
      </header>

      <main class="mx-auto max-w-7xl px-4 pb-12">
        <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {/* --- 1. EQ response --- */}
          <section class="rounded-xl border border-line-soft bg-panel p-3 sm:p-4">
            <div class="flex items-center justify-between gap-2">
              <h2 class="font-semibold text-sm uppercase tracking-[0.8px] text-muted">
                Log-frequency EQ response
              </h2>
              <span class="text-[10px] text-muted">10 band handles</span>
            </div>
            <div class="mt-3 graph">
              <Graph
                axes={axis(
                  { type: "log", min: 20, max: 20000, units: "Hz", ticks: 8 },
                  { type: "linear", min: -12, max: 12, units: "dB", ticks: 6 },
                )}
                dataDomain={[20, 20000]}
                series={[
                  {
                    key: "eq-response",
                    label: "Response",
                    color: EQ_COLOR,
                    kind: "line+area",
                    data: eqResponseData,
                  },
                ]}
                handles={eqHandles}
                overlays={[ov("eq-band-markers", eqBandMarkers)]}
                xLabel="Frequency"
                yLabel="Gain"
              />
            </div>
          </section>

          {/* --- 2. Wave shaper --- */}
          <section class="rounded-xl border border-line-soft bg-panel p-3 sm:p-4">
            <div class="flex items-center justify-between gap-2">
              <h2 class="font-semibold text-sm uppercase tracking-[0.8px] text-muted">
                Wave shaper transfer curve
              </h2>
              <span class="text-[10px] text-muted">3 control points</span>
            </div>
            <div class="mt-3 graph">
              <Graph
                axes={axis(
                  { type: "linear", min: -1, max: 1, units: "" },
                  { type: "linear", min: -1, max: 1, units: "" },
                )}
                series={[
                  {
                    key: "shaper",
                    label: "Transfer",
                    color: SHAPER_COLOR,
                    kind: "line+area",
                    data: shaperData,
                  },
                ]}
                handles={[shaperHandle(0, -0.6), shaperHandle(1, 0), shaperHandle(2, 0.6)]}
                overlays={[ov("y-equals-x", yEqualsX)]}
                xLabel="in"
                yLabel="out"
              />
            </div>
          </section>

          {/* --- 3. Multiband region --- */}
          <section class="rounded-xl border border-line-soft bg-panel p-3 sm:p-4">
            <div class="flex items-center justify-between gap-2">
              <h2 class="font-semibold text-sm uppercase tracking-[0.8px] text-muted">
                Multiband gain-reduction region
              </h2>
              <span class="text-[10px] text-muted">crossover markers</span>
            </div>
            <div class="mt-3 graph">
              <Graph
                axes={axis(
                  { type: "log", min: 30, max: 16000, units: "Hz", ticks: 8 },
                  { type: "linear", min: -30, max: 0, units: "dB", ticks: 6 },
                )}
                series={[
                  {
                    key: "gr",
                    label: "Gain reduction",
                    color: MULTIBAND_COLOR,
                    kind: "area",
                    data: multibandData,
                  },
                ]}
                overlays={[ov("crossover-markers", crossoverMarkers)]}
                xLabel="Frequency"
                yLabel="Gain reduction"
                zeroLine
              />
            </div>
          </section>
        </div>

        <p class="mt-6 text-[11px] leading-relaxed text-muted">
          Each section drives the same reusable component. The EQ and shaper
          sections bind their handles to local state, so dragging a handle
          updates the curve in real time (mouse, touch and keyboard all work —
          handles are real SVG/HTML elements with roles, tab indexes and
          aria attributes). The region view demonstrates the overlay layer:
          vertical dashed crossover markers plus labelled regions, drawn in the
          chart's own coordinate frame.
        </p>
      </main>

      <footer class="mx-auto max-w-7xl px-4 pb-6 text-[10px] text-muted">
        <hr class="border-line-soft mb-2" />
        <div class="flex flex-wrap gap-4">
          <span>
            <strong>Ref:</strong> docs/design/graphic-eq.md (card t_f88ef464)
          </span>
          <span>
            Component: ui/src/components/Graph.tsx · GraphAxis/GraphAxes/
            GraphSeries/GraphHandle/GraphOverlay/GraphProps
          </span>
        </div>
      </footer>
    </div>
  );
}
