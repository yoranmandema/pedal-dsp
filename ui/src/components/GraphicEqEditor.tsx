import { useMemo } from "preact/hooks";
import { Graph, type GraphAxes, type GraphHandle, type GraphOverlay, type ChartPads } from "./Graph";
import type { JSX } from "preact";

import { EQ_CENTRES_HZ, bandParam, clampDb, flatWrites, keyStep } from "../graph/eq";
export { EQ_CENTRES_HZ };

/** The tone category hue (gold, per ui/src/theme.ts), used for the block accents. */
const EQ_COLOR = "hsl(42 62% 58%)";

/** Nominal host sample rate for the UI-side steady-state response preview. */
const FS = 48_000;

/**
 * Steady-state magnitude (dB) of one RBJ peaking biquad at frequency `f`.
 *
 * Mirrors `dsp::biquad::Biquad::make` (RBJ cookbook): `A = 10^(G/40)`, Q fixed
 * at 1.0 for the cascade. Used on the UI thread by the Graphic EQ editor to
 * give a real-time preview of the steady-state response; the sound is defined
 * by the engine block.
 */
function peakingDb(f: number, f0: number, q: number, gainDb: number, fs = FS): number {
  const w0 = (2 * Math.PI * f0) / fs;
  const cw = Math.cos(w0);
  const sw = Math.sin(w0);
  const alpha = sw / (2 * q);
  const A = Math.pow(10, gainDb / 40);
  const b0 = 1 + alpha * A;
  const b1 = -2 * cw;
  const b2 = 1 - alpha * A;
  const a0 = 1 + alpha / A;
  const a1 = -2 * cw;
  const a2 = 1 - alpha / A;
  const nb0 = b0 / a0;
  const nb1 = b1 / a0;
  const nb2 = b2 / a0;
  const na1 = a1 / a0;
  const na2 = a2 / a0;
  const w = (2 * Math.PI * f) / fs;
  const cw2 = Math.cos(w);
  const sw2 = Math.sin(w);
  const numRe = nb0 + nb1 * cw2 + nb2 * Math.cos(2 * w);
  const numIm = -(nb1 * sw2 + nb2 * Math.sin(2 * w));
  const denRe = 1 + na1 * cw2 + na2 * Math.cos(2 * w);
  const denIm = -(na1 * sw2 + na2 * Math.sin(2 * w));
  const denSq = denRe * denRe + denIm * denIm;
  const hRe = (numRe * denRe + numIm * denIm) / denSq;
  const hIm = (numIm * denRe - numRe * denIm) / denSq;
  const mag = Math.hypot(hRe, hIm);
  return 20 * Math.log10(Math.max(mag, 1e-12));
}

/**
 * Combined steady-state response (dB) of the 10-band cascade at `f`.
 *
 * The bands are wired in series, so magnitudes multiply and dB add. 0 dB per
 * band is identity; the response is the sum of the individual peaking responses
 * at the ISO centres (Q = 1.0).
 */
function eqResponse(gains: readonly number[]): number[] {
  return Array.from({ length: 200 }, (_, i) => {
    const t = i / 199;
    const f = 20 * Math.pow(20000 / 20, t); // 200 log-spaced frequencies 20..20000
    let db = 0;
    for (let b = 0; b < EQ_CENTRES_HZ.length; b++) {
      const gain = gains[b] ?? 0;
      db += peakingDb(f, EQ_CENTRES_HZ[b], 1.0, gain);
    }
    return Math.round(db * 10) / 10;
  });
}

/** Wrap a raw overlay render function as the { id, render } overlay object. */
const ov = (
  id: string,
  render: (w: number, h: number, axes: GraphAxes<number>, pads: ChartPads) => JSX.Element,
): GraphOverlay<number> => ({ id, render });

/**
 * Vertical dashed guide at each ISO centre + in-chart band labels.
 *
 * The handles already carry the per-band labels, so this overlay adds the
 * band-centre ticks (per docs/design/graphic-eq.md §2.4) without duplicating
 * the handle text.
 */
function eqBandMarkers(
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
      {EQ_CENTRES_HZ.map((f) => {
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
          </g>
        );
      })}
    </g>
  );
}

interface GraphicEqEditorProps {
  /** Channel the block lives in (passed to `setParam`); traced onto the
   *  root element as `data-channel` for debugging. */
  channel: string;
  /** Selected block (type is `tone.graphic_eq`). */
  block: { params: Record<string, number> };
  /** Write one band gain through the existing `setParam` contract. */
  onChange: (param: string, db: number) => void;
}

/** Format a dB value for the slider readout (normalises -0). */
function formatDb(v: number): string {
  const n = Math.round(v * 10) / 10;
  return `${Number.isNaN(n) ? "0" : n.toFixed(1)} dB`;
}

export function GraphicEqEditor({ channel, block, onChange }: GraphicEqEditorProps) {
  // Read the current gains from the block's params (default 0 = identity).
  const gains = useMemo(() => {
    const g = Array.from({ length: EQ_CENTRES_HZ.length }, (_, i) => block.params[`gain${i}`] ?? 0);
    return g;
  }, [block.params]);

  const response = useMemo(() => eqResponse(gains), [gains]);

  const eqHandles: GraphHandle<number>[] = EQ_CENTRES_HZ.map((f, i) => ({
    id: `eq-band-${i}`,
    // Fixed x coordinate on the x axis = the ISO centre frequency; the knob
    // moves the gain (the y axis), drawing a vertical dashed guide.
    x: f,
    value: gains[i] ?? 0,
    label: `Band ${i + 1} @ ${f} Hz`,
    color: EQ_COLOR,
    onChange: (_id: string, v: number) => onChange(bandParam(i), clampDb(v)),
  }));

  return (
    <div class="graphic-eq-editor" data-channel={channel}>
      <div class="flex justify-end mb-1">
        <button
          type="button"
          class="graphic-eq-reset min-h-[var(--ui-touch)] px-3 rounded-md text-[11px] font-semibold bg-raised border border-line text-muted hover:text-text hover:border-line-hi"
          title="Set all bands to 0 dB"
          onClick={() => flatWrites().forEach(([p, v]) => onChange(p, v))}
        >
          Reset flat
        </button>
      </div>
      <div class="graphic-eq-graph">
        <Graph
          axes={{
            x: { type: "log", min: 20, max: 20000, units: "Hz", ticks: 8 },
            y: { type: "linear", min: -15, max: 15, units: "dB", ticks: 6 },
          }}
          dataDomain={[20, 20000]}
          series={[{
            key: "eq-response",
            label: "Response",
            color: EQ_COLOR,
            kind: "line+area",
            data: response,
          }]}
          handles={eqHandles}
          overlays={[ov("eq-band-markers", eqBandMarkers)]}
          xLabel="Frequency"
          yLabel="Gain"
        />
      </div>

      {/* 10 band sliders, one row, min-width preserved. */}
      <div class="graphic-eq-sliders">
        {EQ_CENTRES_HZ.map((f, i) => (
          <div class="graphic-eq-slider-unit flex-1 min-w-[64px] min-w-0" key={i}>
            <div class="graphic-eq-slider-label text-center text-[10px] font-semibold uppercase tracking-[0.6px] text-muted">
              Band {i + 1} @ {f} Hz
            </div>
            <input
              class="graphic-eq-slider w-full min-w-0 cursor-ns-resize"
              type="range"
              min={-12}
              max={12}
              step={0.1}
              value={gains[i] ?? 0}
              aria-label={`Band ${i + 1} gain`}
              title={`Band ${i + 1} @ ${f} Hz: ${formatDb(gains[i] ?? 0)} (double-click to reset)`}
              onInput={(e) => onChange(bandParam(i), clampDb(parseFloat((e.target as HTMLInputElement).value)))}
              onDblClick={() => onChange(bandParam(i), 0)}
              onKeyDown={(e) => {
                if (e.key === "Home" || e.key === "Delete" || e.key === "Backspace") {
                  e.preventDefault();
                  onChange(bandParam(i), 0);
                } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
                  e.preventDefault();
                  const d = (e.key === "ArrowUp" ? 1 : -1) * keyStep(e.shiftKey);
                  onChange(bandParam(i), clampDb((gains[i] ?? 0) + d));
                }
              }}
            />
            <div class="graphic-eq-slider-readout text-[9px] font-mono tabular-nums text-muted">
              {formatDb(gains[i] ?? 0)}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
