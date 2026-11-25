import { catStyle } from "../theme";
import { Knob } from "./Knob";
import { selected, state, setParam, looperFrac } from "../state";

/**
 * Banner state words per docs/looper.md §4:
 *   0 = Idle      → STOPPED (neutral)
 *   1 = Recording → RECORDING (amber)
 *   2 = Playing   → PLAYING (amber)
 *   3 = Overdubbing → OVERDUBBING (amber)
 *   4 = Stopped/Cleared → STOPPED (neutral)
 */
function looperBannerWord(s: number): string {
  switch (s) {
    case 1:
      return "RECORDING";
    case 2:
      return "PLAYING";
    case 3:
      return "OVERDUBBING";
    default:
      return "STOPPED";
  }
}

/** The trigger button that is "driven" by the current engine state, so the
 *  transport is legible even though the gates are momentary. */
function downByState(s: number): string | null {
  switch (s) {
    case 1:
      return "trig_record";
    case 2:
      return "trig_play";
    case 3:
      return "trig_overdub";
    default:
      return null;
  }
}

const TRIGGERS = [
  { id: "trig_record", label: "Record" },
  { id: "trig_play", label: "Play" },
  { id: "trig_overdub", label: "Overdub" },
  { id: "trig_stop", label: "Stop" },
  { id: "trig_clear", label: "Clear" },
] as const;

/** A momentary transport trigger rendered as a 2-state pill. */
function TriggerButton({
  trigger,
  gate,
  downState,
  setGate,
}: {
  trigger: (typeof TRIGGERS)[number];
  gate: number;
  downState: string | null;
  setGate: (p: string, v: number) => void;
}) {
  const down = gate >= 0.5 || downState === trigger.id;
  return (
    <button
      class="looper-trigger"
      role="button"
      aria-pressed={down}
      aria-label={`Looper ${trigger.label}`}
      title={trigger.label}
      onPointerDown={(e) => {
        e.preventDefault();
        setGate(trigger.id, 1);
      }}
      onPointerUp={() => setGate(trigger.id, 0)}
      onPointerCancel={() => setGate(trigger.id, 0)}
      onPointerLeave={() => setGate(trigger.id, 0)}
      onKeyDown={(e) => {
        if (e.key === "Escape" || e.key === "Enter") setGate(trigger.id, 0);
      }}
    >
      {trigger.label}
    </button>
  );
}

/** Format a knob value with the engine's unit (ms, %), e.g. `35.0 %`. */
function formatParam(p: { name: string; unit: string; min: number; max: number }, v: number): string {
  const decimals = Math.abs(v) >= 100 ? 0 : Math.abs(v) >= 10 ? 1 : 2;
  return `${v.toFixed(decimals)}${p.unit ? ` ${p.unit}` : ""}`;
}

/** The looper transport panel — one column, fills the editor width. */
export function LooperEditor() {
  const s = state.value;
  if (!s) return null;
  const sel = selected.value;
  if (!sel) return null;

  const channel = s.channels.find((c) => c.id === sel.channel);
  const block = channel?.blocks.find((b) => b.id === sel.block);
  if (!block) return null;

  const spec = s.registry[block.type];
  const params = spec?.params ?? [];
  const trigIds = new Set<string>(TRIGGERS.map((t) => t.id));
  const knobIds = params.filter((p) => p.kind !== "choice" && !trigIds.has(p.id) && !["half", "reverse"].includes(p.id));
  const toggleIds = params.filter((p) => !trigIds.has(p.id) && ["half", "reverse"].includes(p.id));

  const looperState = s.looper_state ?? 0;

  const setGate = (param: string, value: number): void => {
    void setParam(sel.channel, sel.block, param, value);
  };

  const setToggle = (param: string, value: number): void => {
    void setParam(sel.channel, sel.block, param, value);
  };

  const timeMs = knobIds.find((k) => k.id === "time")?.default ?? 1000;
  const remainingSec = (timeMs / 1000) * (1 - Math.min(1, Math.max(0, looperFrac.value)));
  const remainingLabel = remainingSec > 0.01 ? mmss(remainingSec) : "0:00";

  return (
    <div
      class="looper-editor"
      role="region"
      aria-label={`${block.name} controls`}
      style={spec ? catStyle(spec.category) : undefined}
    >
      {/* Block identity header, matching the shared Editor banner contract so
       * the block-selection command can wait for it (and other blocks reuse
       * the same selector). */}
      <div class="editor-name flex items-center gap-2 min-w-0 max-w-full">
        <span class="min-w-0 truncate text-xl font-semibold">{block.name}</span>
      </div>

      {/* Banner: block name + current state word, colour per state. */}
      <div class="looper-banner">
        <div class="looper-banner-name" title={block.name}>
          {block.name}
        </div>
        <div class="looper-banner-state" aria-live="polite" data-state={looperBannerWord(looperState)}>
          {looperBannerWord(looperState)}
        </div>
      </div>

      {/* Loop panel: progress bar + remaining time. */}
      <div class="looper-panel">
        <div class="looper-progress-row">
          <div
            class="looper-progress-track"
            role="progressbar"
            aria-valuenow={Math.round(looperFrac.value * 100)}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label="Loop position"
          >
            <div
              class="looper-progress-fill"
              style={{ width: `${Math.min(100, looperFrac.value * 100)}%` }}
            ></div>
          </div>
          <div class="looper-remaining">{remainingLabel}</div>
        </div>
      </div>

      {/* Five momentary transport buttons. */}
      <div class="looper-triggers" role="group" aria-label="Looper transport">
        {TRIGGERS.map((t) => (
          <TriggerButton
            key={t.id}
            trigger={t}
            gate={block.params[t.id] ?? 0}
            downState={downByState(looperState)}
            setGate={setGate}
          />
        ))}
      </div>

      {/* Continuous knobs: Time (log), Feedback, Mix, Level. */}
      <div class="looper-knobs">
        {knobIds.map((p) => (
          <Knob
            key={p.id}
            name={p.name}
            value={block.params[p.id] ?? p.default}
            min={p.min}
            max={p.max}
            defaultValue={p.default}
            unit={p.unit}
            format={(v) => formatParam(p, v)}
            onChange={(v) => setGate(p.id, v)}
          />
        ))}
      </div>

      {/* Two-option toggle chips: Half speed, Reverse. */}
      <div class="looper-chips">
        {toggleIds.map((p) => (
          <div class="choice-unit">
            <span class="ctrlName">{p.name}</span>
            <div class="segmented">
              <button
                key={p.id}
                class="seg"
                role="switch"
                aria-checked={(block.params[p.id] ?? 0) >= 0.5}
                aria-label={`Looper ${p.name}`}
                title={p.name}
                onClick={() => setToggle(p.id, ((block.params[p.id] ?? 0) >= 0.5) ? 0 : 1)}
              >
                {p.choices?.[0]} / {p.choices?.[1]}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Format seconds as m:ss (e.g. 95 s → 1:35, 0.5 s → 0:00). */
function mmss(s: number): string {
  const total = Math.max(0, Math.round(s));
  const m = Math.floor(total / 60);
  const sec = total % 60;
  return `${m}:${sec.toString().padStart(2, "0")}`;
}
