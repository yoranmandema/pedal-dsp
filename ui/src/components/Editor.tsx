import type { JSX } from "preact";
import { Knob } from "./Knob";
import { LooperEditor } from "./LooperEditor";
import { catStyle } from "../theme";
import { selected, setBypass, setParam, setGraphicEqBand, state, addBlockFor } from "../state";
import { useState } from "preact/hooks";
import type { BlockSpec, ParamSpec } from "../protocol";
import { ctrlName, emptyNote } from "../ui";
import { AddBlockPicker } from "./AddBlockModal";
import { GraphicEqEditor } from "./GraphicEqEditor";
import { MultibandCompressorEditor } from "./MultibandCompressorEditor";
import { ParametricEQ } from "./ParametricEQ";

/** Format a parameter value with the unit the engine declares. */
function formatParam(p: ParamSpec, v: number): string {
  if (p.kind === "choice") return p.choices?.[Math.round(v)] ?? String(v);
  const decimals = Math.abs(v) >= 100 ? 0 : Math.abs(v) >= 10 ? 1 : 2;
  return `${v.toFixed(decimals)}${p.unit ? ` ${p.unit}` : ""}`;
}

/** A `choice` parameter. A knob is the wrong control for a small set of named
 *  options, so these render as a segmented row (or a dropdown when there are
 *  too many to lay out). */
function ChoiceControl({
  p,
  value,
  onChange,
}: {
  p: ParamSpec;
  value: number;
  onChange: (v: number) => void;
}) {
  const choices = p.choices ?? [];
  const current = Math.min(choices.length - 1, Math.max(0, Math.round(value)));

  if (choices.length === 0) return null;

  if (choices.length > 5) {
    return (
      <label class="select-unit grid items-center justify-items-center gap-1 w-full col-span-2">
        <span class={`${ctrlName} ctrlName`}>{p.name}</span>
        <select
          class="select textField min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3"
          value={current}
          onChange={(e) => onChange(Number((e.target as HTMLSelectElement).value))}
        >
          {choices.map((c, i) => (
            <option key={c} value={i}>{c}</option>
          ))}
        </select>
      </label>
    );
  }

  return (
    <div class="choice-unit grid items-center justify-items-center gap-1 w-full col-span-2">
      <span class={`${ctrlName} ctrlName`}>{p.name}</span>
      <div class="segmented flex border border-line rounded-lg overflow-hidden bg-well" role="radiogroup" aria-label={p.name}>
        {choices.map((c, i) => (
          <button
            key={c}
            class={`seg flex-1 min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 text-[10px] font-semibold text-muted border-r border-line-soft last:border-r-0 whitespace-nowrap hover:text-text hover:bg-[#1e2027] ${i === current ? "on bg-linear-to-b from-amber-hi to-amber-lo border-amber-edge text-amber-ink" : ""}`}
            role="radio"
            aria-checked={i === current}
            onClick={() => onChange(i)}
            title={`${p.name}: ${c}`}
          >
            {c}
          </button>
        ))}
      </div>
    </div>
   );
}

/**
 * The heel/toe expression pedal: a full-width position control that drives the
 * whammy sweep gate. 0 = heel (return to the pitch set point), 100 = toe (max
 * sweep position). The value is fed to the engine through the existing
 * `setParam` model on `sweep_gate` — the engine card (t_02019ec2's build step)
 * exposes this param, so the binding works once it lands.
 */
function ExpressionControl({
  value,
  onChange,
}: {
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <div class="expression-unit grid items-center justify-items-center gap-1 w-full pt-3">
      <span class={`${ctrlName} ctrlName`}>Expression pedal</span>
      <div class="expression-slider flex w-full items-center gap-3">
        <input
          class="expression-range flex-1 rounded border border-line bg-[var(--color-inset)] px-1.5 text-[10px] font-mono tabular-nums text-text"
          type="range"
          min={0}
          max={100}
          step={1}
          value={Math.round(value * 100)}
          aria-label="Expression pedal: continuous position-controlled shift"
          title="Expression pedal: heel return, toe sweep"
          onInput={(e) => onChange(parseFloat((e.target as HTMLInputElement).value) / 100)}
        />
        <span class="text-[9px] tracking-[0.6px] text-muted font-semibold uppercase">Heel</span>
        <span class="text-[9px] tracking-[0.6px] text-muted font-semibold uppercase">Toe</span>
      </div>
    </div>
  );
}

/**
 * The footswitch: a toggle that engages/disengages the whammy sweep gate.
 * Driven by the pedal's PC13 ready input; the UI mirrors the button.
 * The value is fed to the engine through `setParam` on `sweep_gate` (see the
 * `ExpressionControl` binding note for the same pattern).
 */
function FootswitchControl({
  active,
  onChange,
}: {
  active: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div class="footswitch-unit grid items-center justify-items-center gap-1 w-full">
      <span class={`${ctrlName} ctrlName`}>Footswitch</span>
      <button
        class={`footswitch flex-1 min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 text-[10px] font-semibold rounded-md border text-muted border-line-soft hover:text-text hover:border-line-hi transition-colors ${active ? "on text-amber border-amber-dim bg-[#2a2318] border-amber-edge shadow-[0_0_0_1px_var(--color-amber-dim)]" : ""}`}
        role="switch"
        aria-checked={active}
        aria-label="Footswitch: engage or disengage the whammy sweep"
        title={active ? "Disengage whammy sweep" : "Engage whammy sweep"}
        onClick={() => onChange(!active)}
      >
        {active ? "Disengage" : "Engage"}
      </button>
    </div>
  );
}

/** The bottom panel: the selected block's identity, then its controls. */
export function Editor() {
  const s = state.value;
  const sel = selected.value;
  if (!s) return null;

  if (!sel) {
    return (
      <footer class="editor row-start-3 col-start-1 grid grid-cols-[minmax(0,var(--ui-id-w))_minmax(0,1fr)] items-center gap-3 border-t border-line bg-linear-to-b from-[#1a1b21] to-[#14151a] p-4 max-h-[62vh] portrait:max-h-[32vh] overflow-y-auto overscroll-contain desk:h-[var(--ui-bar)] desk:min-h-[var(--ui-bar)] desk:max-h-none desk:overflow-visible phone:grid-cols-1 phone:items-start">
        <div class={`${emptyNote} editor-empty emptyNote`}>Select a block in the chain to edit it.</div>
      </footer>
    );
  }

  const channel = s.channels.find((c) => c.id === sel.channel);
  const block = channel?.blocks.find((b) => b.id === sel.block);
  if (!block) {
    return (
      <footer class="editor row-start-3 col-start-1 grid grid-cols-[minmax(0,var(--ui-id-w))_minmax(0,1fr)] items-center gap-3 border-t border-line bg-linear-to-b from-[#1a1b21] to-[#14151a] p-4 max-h-[62vh] portrait:max-h-[32vh] overflow-y-auto overscroll-contain desk:h-[var(--ui-bar)] desk:min-h-[var(--ui-bar)] desk:max-h-none desk:overflow-visible phone:grid-cols-1 phone:items-start">
        <div class={`${emptyNote} editor-empty emptyNote`}>That block is gone.</div>
      </footer>
    );
  }

  const isGraphicEq = block.type === "tone.graphic_eq";
  const spec: BlockSpec | undefined = s.registry[block.type];
  const params = spec?.params ?? [];
  const knobs = params.filter((p) => p.kind !== "choice");
  const choices = params.filter((p) => p.kind === "choice");

  const [sweepGate, setSweepGate] = useState(0);

  /** The footswitch and expression drive the block's sweep gate. The engine
   *  card (t_02019ec2's build step) exposes `sweep_gate` on the whammy block;
   * the UI binds through the existing `setParam` model, so this works once it
   * lands. Until then the local state is truth and the commands fail loudly
   * (surfaced as a toast) instead of going silent. */
  const setGate = (v: number): void => {
    setSweepGate(v);
    void setParam(sel.channel, sel.block, "sweep_gate", v);
  };

  const toggleFootswitch = (engaged: boolean): void => {
    setGate(engaged ? 1 : 0);
  };

  if (block.type === "time.looper") {
    return <LooperEditor />;
  }

  const resetAll = (): void => {
    for (const p of params) void setParam(sel.channel, sel.block, p.id, p.default);
  };

  let body: JSX.Element;
  if (spec?.name === "Multiband Compressor") {
    body = <MultibandCompressorEditor channel={sel.channel} block={block} spec={spec} />;
  } else {
    body = (
    <>
      <div class="editor-id cat-scope grid grid-cols-[minmax(0,1fr)] items-center justify-items-start gap-1 pl-3 min-h-0 phone:flex phone:flex-wrap phone:items-center phone:gap-x-3 phone:gap-y-1 phone:px-2 phone:py-2" style={spec ? catStyle(spec.category) : undefined}>
        <div class="editor-name flex items-center gap-2 min-w-0 max-w-full">
          <span class="min-w-0 truncate text-xl font-semibold">{block.name}</span>
          {block.bypass && <span class="editor-bypass flex-none px-2 text-[9px] text-amber border border-amber-dim rounded-sm font-medium">bypassed</span>}
        </div>
        <div class="editor-side flex flex-wrap gap-1 mt-0">
          <button class={`side-btn sideBtn min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 rounded-md text-[11px] font-semibold bg-raised border border-line text-muted hover:text-text hover:border-line-hi ${block.bypass ? "on text-amber border-amber-dim bg-[#2a2318]" : ""}`} onClick={() => void setBypass(sel.channel, sel.block, !block.bypass)} title="Bypass this block">
            {block.bypass ? "Bypassed" : "Bypass"}
          </button>
          <button class="side-btn sideBtn min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 rounded-md text-[11px] font-semibold bg-raised border border-line text-muted hover:text-text hover:border-line-hi" onClick={resetAll} title="Reset every parameter to its default">
            Reset
          </button>
        </div>
      </div>

      <div class="editor-controls grid grid-cols-[repeat(auto-fit,minmax(var(--ui-ctrl),1fr))] content-center justify-center gap-x-6 gap-y-4 min-w-0 w-full max-w-[var(--ui-ctrl-row)] mx-auto">
        {block.type === "tone.eq" ? (
          <ParametricEQ />
        ) : isGraphicEq ? (
          <GraphicEqEditor
            channel={sel.channel}
            block={block}
            onChange={(param: string, db: number) => {
              const band = parseInt(param.replace("gain", ""), 10);
              void setGraphicEqBand(sel.channel, band, db);
            }}
          />
        ) : (
          <>
            {params.length === 0 && <div class={`${emptyNote} editor-empty emptyNote`}>This block has no parameters.</div>}
            {choices.map((p) => (
              <ChoiceControl key={p.id} p={p} value={block.params[p.id] ?? p.default} onChange={(v) => void setParam(sel.channel, sel.block, p.id, v)} />
            ))}
            {knobs.map((p) => (
              <Knob key={p.id} name={p.name} value={block.params[p.id] ?? p.default} min={p.min} max={p.max} defaultValue={p.default} unit={p.unit} format={(v) => formatParam(p, v)} onChange={(v) => void setParam(sel.channel, sel.block, p.id, v)} />
            ))}
            <FootswitchControl active={sweepGate > 0} onChange={toggleFootswitch} />
          </>
        )}
      </div>
      <ExpressionControl value={sweepGate} onChange={setSweepGate} />
    </>
  );
}
  return (
    <footer
      class="editor relative row-start-3 col-start-1 grid grid-cols-[minmax(0,var(--ui-id-w))_minmax(0,1fr)] items-center gap-3 border-t border-line bg-linear-to-b from-[#1a1b21] to-[#14151a] p-4 min-h-0 max-h-[62vh] portrait:max-h-[32vh] overflow-y-auto overscroll-contain desk:h-[var(--ui-bar)] desk:min-h-[var(--ui-bar)] desk:max-h-none desk:overflow-visible phone:grid-cols-1 phone:items-start"
    >
      {sel.block ? body : (
        <div class={`${emptyNote} editor-empty emptyNote`}>Select a block in the chain to edit it.</div>
      )}
      {addBlockFor.value && (
        <>
          {/* Veil over the editor area (Editor root is `position: relative`):
              the semi-transparent dim the picker sits on top of. */}
          <div
            class="modalBackdrop modalBackdrop modal-backdrop"
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              height: "100%",
              zIndex: 0,
            }}
          ></div>
          <AddBlockPicker />
        </>
      )}
    </footer>
  );
}