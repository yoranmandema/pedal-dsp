import { useMemo } from "preact/hooks";
import type { Block, BlockSpec } from "../protocol";
import { ctrlName, emptyNote } from "../ui";
import { Knob } from "./Knob";
import { BandStrip } from "./BandStrip";
import { CrossoverAxis } from "./CrossoverAxis";
import { Format, ALL_PARAMS, activeBandIds, type ParamId } from "../params";
import {
  LocalParamSource,
  useMultibandParams,
  useMultibandMetersSource,
} from "../hooks/useParams";
import { MultibandMeter } from "./MultibandMeter";

/** Format a value by its schema unit (shared by the global knobs). */
function formatByUnit(unit: string, v: number): string {
  switch (unit) {
    case "dB":
      return Format.db(v);
    case "ms":
      return Format.ms(v);
    case "x":
      return Format.ratio(v);
    case "Hz":
      return Format.hz(v);
    case "%":
      return Format.percent(v);
    default:
      return Format.linear(v);
  }
}

/**
 * The multiband compressor's control panel.
 *
 * A single block in the channel strip. Assembles the reusable panel sections —
 * the global controls (`bands`, crossover frequencies, output gain, dry/wet),
 * the `CrossoverAxis` (draggable, keyboard-controlled log crossover markers),
 * one `BandStrip` per active band (threshold / ratio / attack / release /
 * makeup plus bypass / solo), and the per-band gain-reduction (`GR`) meter
 * region — into one consistent layout.
 *
 * Binding: every control reads from a `ParamSource` and commits through
 * `setValue` — the same seam every other editor uses via `setParam`. With no
 * engine present the panel is driven by `LocalParamSource` / `LocalMetersSource`
 * (in-memory stubs), so it renders and can be tested headless. Connecting the
 * engine is a one-line swap: pass an `EngineParamSource` that reads the rig
 * snapshot and writes the `setParam` command.
 *
 * Engine binding (how to connect real parameters & meter values):
 *   * Parameters: the panel resolves every value from the block's `params`
 *     map (schema defaults fill gaps until the engine lands) and commits through
 *     `setValue` → `source.setValue` → `setParam(channel, block, id, v)`.
 *     The engine card (`t_*_eng_1`) mirrors the schema (`docs/design/multiband-compressor.md
 *     §3/§4`) so the same `ParamId`s are used on both sides; no engine, the
 *     stub renders defaults.
 *   * Meters: `useMultibandMetersSource` returns an `EngineMetersSource` that
 *     listens to the `event: meters` broadcast on the bridge and applies the
 *     extended `Meters` payload (`{ in, out, cpu, xruns, bands: [{ band,
 *     gr_db, out_db }] }`, see `docs/design/multiband-compressor.md §7`). When
 *     no engine is present it falls back to the `LocalMetersSource` stub
 *     (`bands` empty → placeholder note). `MultibandMeter` renders the dB
 *     peak-hold scale; `grDbToPercent` (`meter.ts`) maps the raw dB to a
 *     0–100% fill.
 */
export function MultibandCompressorEditor({
  channel,
  block,
  spec,
}: {
  channel: string;
  block: Block;
  spec: BlockSpec;
}) {
  const bands = block.params.bands ?? 3;
  const activeBands = activeBandIds(bands);

  // Resolve every param from the block's params map (schema defaults fill the
  // gaps until the engine lands). Values are the single source of truth for
  // the UI; `setValue` commits through the source.
  const params = useMemo(() => {
    const p = {} as Record<ParamId, number>;
    for (const schema of ALL_PARAMS) {
      p[schema.id] = block.params[schema.id] ?? schema.default;
    }
    return p;
  }, [block]);
  const meters = useMultibandMetersSource();
  const source = useMemo(() => new LocalParamSource(params), [params]);
  const { values, setValue } = useMultibandParams(source, block);

  const g = (id: ParamId): number => values[id];

  // Per-band accent HSL (hue 152°) matched to the CrossoverAxis region
  // shading; only the lightness varies per band.
  const bandColor = (band: number): string => {
    const lightness = 46 + Math.min(band - 1, 3) * 4;
    return `hsl(152 55% ${lightness}%)`;
  };

  return (
    <div class="multiband-editor flex flex-col w-full min-w-0 gap-3" data-channel={channel}>
      {/* Identity banner: block name, type, bypass. */}
      <div
        class="cat-scope grid grid-cols-[minmax(0,1fr)] items-center justify-between gap-1 pl-3 pr-2 pt-2 min-h-[var(--ui-touch)]"
        style={spec ? { "--cat-h": String(152) } : undefined}
      >
        <div class="flex min-w-0 items-center gap-2">
          <span class="text-base font-semibold truncate" title={block.name}>
            {block.name}
          </span>
          <span class="editor-type text-[10px] tracking-[0.8px] text-muted">
            dynamics.multiband_compressor
          </span>
        </div>
        {block.bypass && (
          <span class="editor-bypass flex-none px-2 text-[9px] text-amber border border-amber-dim rounded-sm font-medium">
            bypassed
          </span>
        )}
      </div>

      {/* Global controls: bands, crossover frequencies, output gain, dry/wet. */}
      <div class="global-controls editor-controls grid grid-cols-[repeat(auto-fit,minmax(var(--ui-ctrl),1fr))] content-center justify-center gap-x-6 gap-y-4 min-w-0 w-full">
        <BandsControl value={g("bands")} onChange={(v) => setValue("bands", v)} />
        <div class="grid grid-cols-[repeat(auto-fit,minmax(var(--ui-ctrl),1fr))] content-center justify-center gap-x-6 gap-y-4 min-w-0 w-full">
          <ParamKnob
            id="output_gain"
            label="Output gain"
            value={g("output_gain")}
            onChange={(v) => setValue("output_gain", v)}
          />
          <ParamKnob
            id="dry_wet"
            label="Dry/Wet"
            value={g("dry_wet")}
            onChange={(v) => setValue("dry_wet", v)}
          />
        </div>
      </div>

      {/* Crossover axis: a log-scale axis with the band markers (drag or
          keyboard). Bound to the engine's `crossover_1..N` params via
          `setParam` — the engine stays the source of truth. */}
      <CrossoverAxis channel={channel} block={block.id} />

      {/* Per-band parameter strips. */}
      <div class="bands">
        {activeBands.map((band) => (
          <BandStrip
            key={band}
            band={band}
            values={values}
            onParamChange={setValue}
            bandColor={bandColor(band)}
          />
        ))}
      </div>

      {/* Per-band gain-reduction meter region. */}
      <div class="meters">
        <div class="flex items-center justify-between">
          <span class={`${ctrlName} ctrlName`}>Gain reduction (GR)</span>
        </div>
        <div class="meter-grid grid grid-cols-2 gap-3 sm:grid-cols-4">
          {meters.bands.map((m) => (
            <MultibandMeter key={m.band} band={m.band} grDb={m.gr_db} />
          ))}
          {meters.bands.length === 0 && (
            <div class={`${emptyNote} editor-empty emptyNote`}>
              Select a multiband block to see per-band gain-reduction meters.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// --- Global segmented control (bands) -------------------------------------------

/** A small segmented row for the integer-only `bands` param. */
function BandsControl({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const options = [2, 3, 4] as const;
  return (
    <div class="band-control grid items-center justify-items-center gap-1 w-full col-span-2">
      <span class={`${ctrlName} ctrlName`}>Bands</span>
      <div class="segmented flex border border-line rounded-lg overflow-hidden bg-well" role="radiogroup" aria-label="Number of bands">
        {options.map((opt) => (
          <button
            key={opt}
            class={`seg flex-1 min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 text-[10px] font-semibold text-muted border-r border-line-soft last:border-r-0 whitespace-nowrap hover:text-text hover:bg-[#1e2027] ${opt === value ? "on bg-linear-to-b from-amber-hi to-amber-lo border-amber-edge text-amber-ink" : ""}`}
            role="radio"
            aria-checked={opt === value}
            onClick={() => onChange(opt)}
            title={`Bands: ${opt}`}
          >
            {opt}
          </button>
        ))}
      </div>
    </div>
  );
}

// --- Per-band strip helpers -----------------------------------------------------

/** A knob for a single global parameter, formatted by its unit. */
function ParamKnob({
  id,
  label,
  value,
  onChange,
}: {
  id: ParamId;
  label: string;
  value: number;
  onChange: (v: number) => void;
}) {
  const p = ALL_PARAMS.find((s) => s.id === id)!;
  return (
    <div class="knob-unit grid items-center justify-items-center gap-1 w-full">
      <span class={`${ctrlName} ctrlName`}>{label}</span>
      <Knob
        name={p.name}
        value={value}
        min={p.min}
        max={p.max}
        step={p.step}
        defaultValue={p.default}
        unit={p.unit}
        format={(v) => formatByUnit(p.unit, v)}
        onChange={onChange}
      />
    </div>
  );
}
