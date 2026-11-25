import { useMemo } from "preact/hooks";
import type { BandCurve } from "../blocks/parametric-eq";
import { CURVE_F_HI, CURVE_F_LO } from "../blocks/parametric-eq";

// Three-band palette: each band is drawn in a distinguishable colour so the
// player can see which part of the EQ shape comes from which band. The Tone
// category's amber is the first trace (the EQ's primary colour).
const TRACE_COLORS = [
  "var(--color-amber)",
  "#4a8f5c",
  "#5b6ce0",
  "#e8638a",
  "#c084d8",
  "#8a9bd0",
];

interface CurveChartProps {
  bands: BandCurve[];
  /** Optional caption for the chart (used as `aria-label` and a legend). */
  caption?: string;
}

/** The frequency-response chart: log frequency x-axis, dB y-axis, fixed
 *  -24..+24 dB view, one trace per band, a bold 0 dB reference and grid
 *  lines every 6 dB. */
export function CurveChart({ bands, caption = "Frequency response" }: CurveChartProps) {
  const { height, left, top, plotW, plotH, xOf, yOf } = useMemo(() => {
    const vw = 640;
    const vh = 360;
    const m = { top: 18, right: 16, bottom: 30, left: 54 };
    const w = vw - m.left - m.right;
    const h = vh - m.top - m.bottom;
    const dbMin = -24;
    const dbMax = 24;
    const logLo = Math.log2(CURVE_F_LO);
    const logHi = Math.log2(CURVE_F_HI);
    return {
      width: vw,
      height: vh,
      left: m.left,
      top: m.top,
      plotW: w,
      plotH: h,
      xOf: (f: number) => m.left + (Math.log2(f) - logLo) / (logHi - logLo) * w,
      yOf: (db: number) => m.top + (dbMax - db) / (dbMax - dbMin) * h,
    };
  }, []);

  // Flat (0 dB) grid lines, 6 dB apart.
  const gridDbs = useMemo(() => Array.from({ length: 9 }, (_, i) => -24 + i * 6), []);

  return (
    <div class="eq-curve-chart" role="img" aria-label={caption}>
      <svg
        class="eq-curve-svg"
        viewBox="0 0 640 360"
        preserveAspectRatio="xMidYMid meet"
        aria-hidden="true"
      >
        {/* Plot background. */}
        <rect x={0} y={0} width={640} height={360} fill="var(--color-bg)" />

        {/* Grid lines every 6 dB. */}
        {gridDbs.map((db) => (
          <g key={db}>
            <line
              x1={left}
              y1={yOf(db)}
              x2={left + plotW}
              y2={yOf(db)}
              stroke="var(--color-line-soft)"
              stroke-width={1}
              stroke-dasharray="3 3"
              opacity={0.7}
            />
            <text
              x={left - 8}
              y={yOf(db) + 4}
              textAnchor="end"
              fontSize={10}
              fill="var(--color-muted)"
            >
              {db >= 0 ? "+" : ""}
              {db}
            </text>
          </g>
        ))}

        {/* 0 dB reference line, bolder. */}
        <line
          x1={left}
          y1={yOf(0)}
          x2={left + plotW}
          y2={yOf(0)}
          stroke="var(--color-line)"
          stroke-width={2}
          stroke-dasharray="6 3"
          opacity={0.9}
        />
        <text x={left + plotW + 8} y={yOf(0) + 4} fontSize={10} fill="var(--color-muted)">
          0 dB
        </text>

        {/* Frequency axis ticks. */}
        {CURVE_F_TICKS.map((f) => (
          <g key={f}>
            <line
              x1={xOf(f)}
              y1={top}
              x2={xOf(f)}
              y2={top + plotH}
              stroke="var(--color-line-soft)"
              stroke-width={1}
              opacity={0.6}
            />
            <text
              x={xOf(f)}
              y={height - 10}
              textAnchor="middle"
              fontSize={10}
              fill="var(--color-muted)"
            >
              {freqLabel(f)}
            </text>
          </g>
        ))}

        {/* Per-band traces, drawn from the computed curve points. */}
        {bands.map(({ band, points }) => {
          const color = TRACE_COLORS[band.id.charCodeAt(0) % TRACE_COLORS.length];
          if (points.every((p) => p.db < -110)) return null;
          const d = points.map((p) => `${xOf(p.freq)},${yOf(p.db)}`).join(" ");
          return (
            <g key={band.id}>
              <path
                d={`M ${d}`}
                fill="none"
                stroke={color}
                stroke-width={2.5}
                stroke-linejoin="round"
                stroke-linecap="round"
              />
              {/* Point markers near the middle of the trace for readability. */}
              {points.slice(120, 140).map((p, i) => (
                <circle key={i} cx={xOf(p.freq)} cy={yOf(p.db)} r={2.5} fill={color} opacity={0.85} />
              ))}
            </g>
          );
        })}

        {/* Plot border. */}
        <rect x={left} y={top} width={plotW} height={plotH} fill="none" stroke="var(--color-line)" stroke-width={1} />
      </svg>

      {/* Accessible legend: the chart is `role="img"`; the legend text is
          readable by assistive tech independently. */}
      <div class="eq-curve-legend">
        {bands.map(({ band }) => (
          <div key={band.id} class="eq-curve-legend-item">
            <span
              class="eq-curve-legend-swatch"
              style={{ backgroundColor: TRACE_COLORS[band.id.charCodeAt(0) % TRACE_COLORS.length] }}
            />
            <span class="eq-curve-legend-name">{band.name}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// Frequency ticks, log-spaced (20 Hz … 24 kHz). Placing about one tick per
// decade keeps the axis readable without congestion.
const CURVE_F_TICKS = [20, 100, 1000, 10000, 20000];

function freqLabel(f: number): string {
  if (f >= 10000) return `${f / 1000} kHz`;
  if (f >= 1000) return `${f / 1000} kHz`;
  if (f >= 100) return `${f} Hz`;
  if (f >= 20) return `${f} Hz`;
  return `${f}`;
}
