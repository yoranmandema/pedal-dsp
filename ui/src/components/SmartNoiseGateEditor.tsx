import { Knob } from "./Knob";
import { ReductionMeter } from "./ReductionMeter";
import { useMultibandMetersSource } from "../hooks/useParams";

interface SmartNoiseGateEditorProps {
  channel: string;
  block: { params: Record<string, number>; bypass: boolean };
  onChange: (param: string, v: number) => void;
}

/**
 * Parameter editor panel for `dynamics.smart_noise_gate`.
 *
 * Layout (per docs/design/smart-noise-gate.md):
 *   Top row: 4 knobs — Strength, Threshold, Release, Reduction
 *   Bottom row: 2 toggles — Hum Freq (50/60 Hz), Hum Notch (on/off)
 *   Reduction meter: horizontal bar showing peak gain reduction from engine
 */
export function SmartNoiseGateEditor({
  channel,
  block,
  onChange,
}: SmartNoiseGateEditorProps) {
  const params = block.params;
  const set = (id: string, v: number) => onChange(id, v);
  const meters = useMultibandMetersSource();
  const gateReduction = meters.gateReduction ?? 0;

  return (
    <div class="smart-noise-gate-editor" data-channel={channel}>
      {/* Top row — 4 knobs */}
      <div class="sng-knobs flex flex-wrap justify-center gap-4">
        <Knob
          name="Strength"
          value={params["strength"] ?? 75}
          min={0}
          max={100}
          defaultValue={75}
          unit="%"
          onChange={(v) => set("strength", v)}
          curve="linear"
        />
        <Knob
          name="Threshold"
          value={params["threshold"] ?? -50}
          min={-80}
          max={0}
          defaultValue={-50}
          unit="dB"
          onChange={(v) => set("threshold", v)}
          curve="linear"
        />
        <Knob
          name="Release"
          value={params["release"] ?? 2}
          min={0.1}
          max={10}
          defaultValue={2}
          unit="s"
          onChange={(v) => set("release", v)}
          curve="log"
        />
        <Knob
          name="Reduction"
          value={params["reduction"] ?? -60}
          min={-100}
          max={0}
          defaultValue={-60}
          unit="dB"
          onChange={(v) => set("reduction", v)}
          curve="linear"
        />
      </div>

      {/* Bottom row — 2 toggles */}
      <div class="sng-toggles flex justify-center gap-6 mt-4 pt-4 border-t border-line-soft">
        <ToggleUnit
          name="Hum Freq"
          on={params["hum_freq"] ?? 1}
          options={["50 Hz", "60 Hz"]}
          labels={["50 Hz", "60 Hz"]}
          onChange={(idx) => set("hum_freq", idx)}
        />
        <ToggleUnit
          name="Hum Notch"
          on={params["hum_notch_enabled"] ?? 0}
          options={["Off", "On"]}
          labels={["Off", "On"]}
          onChange={(idx) => set("hum_notch_enabled", idx)}
        />
      </div>

      {/* Reduction meter — peak gain reduction indicator */}
      <div class="sng-meter flex items-center justify-center gap-3 mt-4 pt-4 border-t border-line-soft">
        <div class="reduction-meter relative w-[200px] phone:w-full">
          <ReductionMeter reductionDb={gateReduction} />
        </div>
      </div>
    </div>
  );
}

/** A two-state toggle button (e.g. 50 Hz / 60 Hz, On / Off). */
function ToggleUnit({
  name,
  on,
  options,
  labels,
  onChange,
}: {
  name: string;
  on: number;
  options: string[];
  labels: string[];
  onChange: (idx: number) => void;
}) {
  const idx = Math.min(options.length - 1, Math.max(0, Math.round(on)));

  return (
    <div class="toggle-unit flex flex-col items-center gap-1 w-full">
      <span class="text-center text-2xs font-semibold tracking-[0.8px] text-muted uppercase">
        {name}
      </span>
      <div
        class="flex border border-line rounded-lg overflow-hidden bg-well"
        role="radiogroup"
        aria-label={name}
      >
        {options.map((opt, i) => (
          <button
            key={i}
            class={`flex-1 min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 text-[10px] font-semibold text-muted border-r border-line-soft last:border-r-0 whitespace-nowrap hover:text-text hover:bg-[#1e2027] transition-colors ${i === idx ? "on bg-linear-to-b from-amber-hi to-amber-lo border-amber-edge text-amber-ink" : ""}`}
            role="radio"
            aria-checked={i === idx}
            onClick={() => onChange(i)}
            title={`${name}: ${opt}`}
          >
            {labels[i] ?? opt}
          </button>
        ))}
      </div>
    </div>
  );
}
