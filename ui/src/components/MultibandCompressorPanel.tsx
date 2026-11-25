// MultibandCompressorPanel — the interactive editor for the multiband
// compressor block.
//
// Composition: the CrossoverAxis (drag-and-keyboard crossover markers, bound to
// the engine's `crossover_1…N` params) sits above the per-band compressor
// controls. The panel reads the block's parameters from the global state and
// re-derives on every state event, so the engine stays the single source of
// truth. Every control dispatches `setParam` through the existing binding
// layer; nothing here holds engine state locally.
//
// The parent card's `MultibandCompressorEditor` is display-only and is
// intentionally not used here — this panel wires the CrossoverAxis to real
// parameters (the "scaffold + binding layer" the card asks for).

import { useMemo } from "preact/hooks";
import { state, setParam } from "../state";
import { CrossoverAxis } from "./CrossoverAxis";
import { activeCrossoverParamIds } from "./crossover";


const BLOCK_TYPE = "dynamics.multiband_compressor" as const;

/** A single-band on/off control (bypass / solo). */
interface BandSwitch {
  label: string;
  param: string;
}

interface MultibandCompressorPanelProps {
  channel: string;
  block: string;
}

function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

/** Small linear readout slider, consistent with the panel design tokens. */
function ParamSlider({
  name,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: {
  name: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format?: (v: number) => string;
  onChange: (v: number) => void;
}) {
  const display = format
    ? format(value)
    : `${value.toFixed(1)}${name.includes("gain") || name.includes("dB") ? " dB" : ""}`;

  return (
    <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
      <span class="text-[9px] font-mono tabular-nums text-muted">{name}</span>
      <input
        class="range flex-1 min-w-[48px] cursor-pointer rounded border border-line bg-inset py-0.5 pr-1 pl-1 text-[9px] font-mono tabular-nums text-text"
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-label={name}
        title={`${name}: ${display}`}
        onInput={(e) => onChange(parseFloat((e.target as HTMLInputElement).value))}
      />
      <span class="w-[48px] text-right text-[9px] font-mono tabular-nums text-muted">
        {display}
      </span>
    </div>
  );
}

/** A binary on/off control as a small toggle. */
function BandSwitch({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void; }) {
  return (
    <button
      class={`switch flex-1 min-w-[48px] cursor-pointer rounded border px-1 py-0.5 text-[9px] font-semibold uppercase tracking-[0.5px] transition-colors ${checked ? "on border-amber text-amber" : "border-line text-muted"}`}
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      onClick={() => onChange(!checked)}
    >
      {label}
    </button>
  );
}

export function MultibandCompressorPanel({ channel, block }: MultibandCompressorPanelProps) {
  const s = state.value;
  const channelObj = s?.channels.find((c) => c.id === channel);
  const blockObj = channelObj?.blocks.find((b) => b.id === block);
  if (!s || !channelObj || !blockObj) return null;

  // Active crossovers = bands - 1; the CrossoverAxis binds to these params.
  const crossoverParamIds = activeCrossoverParamIds(blockObj.params["bands"] ?? 3);

  // Per-band display config derived from the block's registry spec.
  const bandConfig = useMemo(() => {
    const spec = s.registry[BLOCK_TYPE];
    if (!spec) return [];
    const byId = new Map(spec.params.map((p) => [p.id, p]));
    const count = Math.min(4, Math.max(2, Math.floor(blockObj.params["bands"] ?? 3))) - 1;
    return Array.from({ length: count }, (_, i) => {
      const p = (n: string) => byId.get(`band_${i + 1}_${n}`)!;
      return {
        label: `Band ${i + 1}`,
        threshold: { id: p("threshold").id, min: p("threshold").min, max: p("threshold").max, step: 1, fmt: (v: number) => `${v.toFixed(1)} dB` },
        ratio: { id: p("ratio").id, min: p("ratio").min, max: p("ratio").max, step: 1, fmt: (v: number) => `${v.toFixed(1)} : 1` },
        attack: { id: p("attack").id, min: p("attack").min, max: p("attack").max, step: 0.1, fmt: (v: number) => `${v.toFixed(1)} ms` },
        release: { id: p("release").id, min: p("release").min, max: p("release").max, step: 1, fmt: (v: number) => `${v.toFixed} ms` },
        makeup: { id: p("makeup").id, min: p("makeup").min, max: p("makeup").max, step: 1, fmt: (v: number) => `${v.toFixed(1)} dB` },
        bypass: p("bypass").id,
        solo: p("solo").id,
      };
    });
  }, [s, blockObj.params["bands"]]);

  return (
    <div class="multiband-compressor-panel">
      {/* Crossover axis: log 20 Hz…20 kHz with draggable, keyboard markers. */}
      <div class="crossover-axis-block">
        <CrossoverAxis channel={channel} block={block} />
        <div class="flex items-center justify-between mt-2">
          <span class="text-[9px] font-semibold uppercase tracking-[0.5px] text-muted">
            Crossovers {crossoverParamIds.length ? `(${crossoverParamIds.join(", ")})` : "n/a"}
          </span>
          <span class="text-[9px] font-mono tabular-nums text-muted">Bands {blockObj.params["bands"] ?? 3}</span>
        </div>
      </div>

      {bandConfig.map((cfg) => (
        <div class="border-t border-line-soft pt-3 first:pt-0" key={cfg.label}>
          <div class="flex items-center justify-between gap-2">
            <span class="text-[9px] font-semibold uppercase tracking-[0.5px] text-muted">{cfg.label}</span>
            <BandSwitch label={`${cfg.label} solo`} checked={blockObj.params[cfg.solo] !== 0} onChange={(v) => void setParam(channel, block, cfg.solo, v ? 1 : 0)} />
          </div>
          <div class="grid grid-cols-2 gap-x-2 gap-y-1 mt-1">
            <ParamSlider name="Threshold" value={blockObj.params[cfg.threshold.id] ?? cfg.threshold.min} min={cfg.threshold.min} max={cfg.threshold.max} step={cfg.threshold.step} format={cfg.threshold.fmt} onChange={(v) => void setParam(channel, block, cfg.threshold.id, clamp(v, cfg.threshold.min, cfg.threshold.max))} />
            <ParamSlider name="Ratio" value={blockObj.params[cfg.ratio.id] ?? cfg.ratio.min} min={cfg.ratio.min} max={cfg.ratio.max} step={cfg.ratio.step} format={cfg.ratio.fmt} onChange={(v) => void setParam(channel, block, cfg.ratio.id, clamp(v, cfg.ratio.min, cfg.ratio.max))} />
            <ParamSlider name="Attack" value={blockObj.params[cfg.attack.id] ?? cfg.attack.min} min={cfg.attack.min} max={cfg.attack.max} step={cfg.attack.step} format={cfg.attack.fmt} onChange={(v) => void setParam(channel, block, cfg.attack.id, clamp(v, cfg.attack.min, cfg.attack.max))} />
            <ParamSlider name="Release" value={blockObj.params[cfg.release.id] ?? cfg.release.min} min={cfg.release.min} max={cfg.release.max} step={cfg.release.step} format={cfg.release.fmt} onChange={(v) => void setParam(channel, block, cfg.release.id, clamp(v, cfg.release.min, cfg.release.max))} />
            <ParamSlider name="Makeup" value={blockObj.params[cfg.makeup.id] ?? cfg.makeup.min} min={cfg.makeup.min} max={cfg.makeup.max} step={cfg.makeup.step} format={cfg.makeup.fmt} onChange={(v) => void setParam(channel, block, cfg.makeup.id, clamp(v, cfg.makeup.min, cfg.makeup.max))} />
          </div>
        </div>
      ))}
    </div>
  );
}
