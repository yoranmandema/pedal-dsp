import {
  BAND_PARAM_SPECS,
  Format,
  type ParamId,
  type ParamSchema,
  formatByUnit,
} from "../params";
import { Knob } from "./Knob";

/** The per-band parameter keys in listing order (threshold → solo). */
const BAND_KEYS = ["threshold", "ratio", "attack", "release", "makeup", "bypass", "solo"] as const;
type BandKey = (typeof BAND_KEYS)[number];

/** Build a per-band parameter id, type-safely from a band index and a key. */
const bandParam = (band: number, key: BandKey): ParamId => `band_${band}_${key}` as ParamId;

/**
 * Build a Map of every per-band parameter id -> schema so table-driven renderers
 * index a parameter with a (band, key) pair rather than a loose string. The map
 * is derived from `BAND_PARAM_SPECS` once per import.
 */
const BAND_SCHEMA_BY_ID = new Map<ParamId, ParamSchema>();
for (const schema of BAND_PARAM_SPECS) {
  BAND_SCHEMA_BY_ID.set(schema.id, schema);
}

/**
 * Look up the `ParamSchema` for one parameter of a band.
 *
 * @throws if `band` is out of range or `key` does not belong to this band.
 */
function bandSchema(band: number, key: BandKey): ParamSchema {
  const schema = BAND_SCHEMA_BY_ID.get(bandParam(band, key));
  if (!schema) {
    throw new RangeError(`No per-band param "${key}" for band ${band}`);
  }
  return schema;
}

/** Return a band's parameter label (the schema's `name`). */
function bandKeyLabel(band: number, key: BandKey): string {
  return bandSchema(band, key).name;
}

/**
 * A knob for a single per-band parameter, formatted by its unit + step.
 *
 * Rendered as a keyboard-operable `role="slider"` (see `Knob.tsx`), so the band
 * strip is fully operable without a pointer.
 */
function BandParamKnob({
  band,
  key,
  value,
  onChange,
}: {
  band: number;
  key: BandKey;
  value: number;
  onChange: (v: number) => void;
}) {
  const p = bandSchema(band, key);
  return (
    <div class="knob-unit grid items-center justify-items-center gap-1 w-full">
      <span class="text-center text-2xs font-semibold tracking-[0.8px] text-muted uppercase">
        {p.name}
      </span>
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

/**
 * A toggle cell for a 0/1 band param (bypass / solo). Rendered as a toggle
 * button (`role="button"` + `aria-pressed`) so assistive tech reports it as an
 * on/off toggle it can activate with Enter/Space.
 */
function BandToggle({
  band,
  key,
  values,
  onCommit,
}: {
  band: number;
  key: BandKey;
  values: Readonly<Record<ParamId, number>>;
  onCommit: (v: number) => void;
}) {
  const on = values[bandParam(band, key)] !== 0;
  return (
    <div class="flex-1 min-w-0 toggle-unit">
      <span class="text-center text-2xs font-semibold tracking-[0.8px] text-muted uppercase">
        {bandKeyLabel(band, key)}
      </span>
      <button
        class={`toggle-chip flex-1 min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 py-2 text-[10px] font-semibold rounded-md border text-muted border-line-soft hover:text-text hover:border-line-hi transition-colors focus-visible:outline-2 focus-visible:outline-amber focus-visible:outline-offset-2 ${on ? "on text-amber border-amber dim bg-[#2a2318] border-amber-edge shadow-[0_0_0_1px_var(--color-amber-dim)]" : ""}`}
        role="button"
        aria-pressed={on}
        aria-label={`${bandKeyLabel(band, key)} band ${band}: ${on ? "on" : "off"}`}
        title={`${bandKeyLabel(band, key)} band ${band}: ${on ? "on" : "off"}`}
        onClick={() => onCommit(on ? 0 : 1)}
      >
        {on ? "On" : "Off"}
      </button>
    </div>
  );
}

/**
 /** One band's control strip: threshold / ratio / attack / release / makeup plus
  *  bypass and solo toggles. Rendered once per active band of the multiband
  *  compressor panel.
  *
  * The `bandColor` (an HSL string) is the per-band accent: it colors the header
  * underline, the toggle chips and the knob-grid background so each band reads
  * as its own region on the crossover axis. The tone (hue 152°) matches the
  * CrossoverAxis band-region shading; only the lightness varies per band.
  *
  * All controls bind to the band's parameters through `values` + `onParamChange`,
  * so the strip is driven by a `ParamSource` (see `hooks/useParams.ts`) and needs
  * no direct knowledge of the engine.
  */
export function BandStrip({
  band,
  values,
  onParamChange,
  bandColor,
}: {
  band: number;
  values: Readonly<Record<ParamId, number>>;
  onParamChange: (id: ParamId, v: number) => void;
  /** Per-band accent HSL (hue 152°), used to color-code the strip. */
  bandColor: string;
}) {
  const header = (
    <div class="flex items-center justify-between mb-1">
      <span class="text-center text-2xs font-semibold tracking-[0.8px] text-muted uppercase">
        Band {band}
      </span>
      <span class="text-[10px] font-mono tabular-nums text-muted">
        {Format.db(values[bandParam(band, "threshold")])}
      </span>
    </div>
  );

  return (
    <div
      class="band-strip editor-controls flex flex-col w-full min-w-0 gap-3"
      style={{background: `${bandColor}12`, borderTop: `1px solid ${bandColor}33`}}
    >
      {header}
      {/* Parameter knobs: wrap into a grid on the available width. */}
      <div class="knob-grid grid grid-cols-[repeat(auto-fit,minmax(var(--ui-ctrl),1fr))] content-center justify-center gap-x-6 gap-y-4" style={{background: `${bandColor}0d`}}>
        {BAND_KEYS.slice(0, 5).map((key) => (
          <BandParamKnob
            band={band}
            key={key}
            value={values[bandParam(band, key)]}
            onChange={(v) => onParamChange(bandParam(band, key), v)}
          />
        ))}
      </div>
      {/* Bypass / solo toggles: wrap onto a new row on narrow widths. */}
      <div class="toggle-row flex flex-wrap items-center gap-3">
        <BandToggle
          band={band}
          key="bypass"
          values={values}
          onCommit={(v) => onParamChange(bandParam(band, "bypass"), v)}
        />
        <BandToggle
          band={band}
          key="solo"
          values={values}
          onCommit={(v) => onParamChange(bandParam(band, "solo"), v)}
        />
      </div>
    </div>
  );
}
