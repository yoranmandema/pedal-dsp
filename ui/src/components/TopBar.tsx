import { meters, presetsOpen, state, loadPreset, openModels, setSource, setInputGain, setMasterOutVolume, tunerOpen, bufferSizeOptions, setConfig, footswitch, footswitchOpen } from "../state";
import { HeaderKnob } from "./HeaderKnob";
import { TunerIcon } from "./Icon";

const SLOTS = ["A", "B", "C", "D", "E", "F", "G", "H"];

/** Peak meter: fast attack, slow release, drawn as a vertical bar. */
function Meter({ label, value }: { label: string; value: number }) {
  const pct = Math.min(100, value * 100);
  const hot = pct > 70;
  return (
    <div class="meter flex flex-col items-center gap-1">
      <div class="meter-track w-2 h-[22px] bg-inset border border-line-soft rounded-sm overflow-hidden flex items-end">
        <div
          class={`meter-fill w-full transition-[height] duration-60 ease-linear ${hot ? "bg-amber" : "bg-ok"}`}
          style={{ height: `${pct}%` }}
        />
      </div>
      <div class="meter-label text-[9px] tracking-[0.6px] text-muted">{label}</div>
    </div>
  );
}

export function TopBar() {
  const s = state.value;
  if (!s) return null;
  const idx = s.presets.indexOf(s.preset);

  const step = (d: number) => {
    const next = idx + d;
    if (next >= 0 && next < s.presets.length) void loadPreset(s.presets[next]);
  };

  return (
    <header class="topbar portrait:flex-nowrap! portrait:overflow-x-auto portrait:overflow-y-hidden portrait:[scrollbar-width:none] portrait:*:flex-none flex min-h-[var(--ui-topbar-h)] items-center gap-4.5 px-4 min-w-0 bg-linear-to-b from-[#1e2027] to-[#16171c] border-b border-line compact:flex-wrap desk:flex-wrap compact:gap-y-2 compact:px-3 compact:py-2">
      {/* The input trim knob: first child of the top bar, top left. */}
      <HeaderKnob
        label="Input"
        value={s.inputGain ?? 0}
        min={-60}
        max={24}
        defaultValue={0}
        onChange={(v) => void setInputGain(v)}
      />

      <div class="preset flex items-center gap-2 min-w-[230px] compact:flex-1 compact:min-w-0">
        <button class="nav size-[var(--ui-touch)] rounded-key border border-line bg-raised-2 leading-none text-muted hover:border-line-hi hover:text-text" onClick={() => step(-1)} title="Previous preset">
          ‹
        </button>
        <div class="preset-name flex flex-col min-w-0">
          <span class="text-lg font-semibold truncate">{s.preset || "Untitled"}</span>
          <small class="text-[10px] font-normal text-muted capitalize">{s.source}</small>
        </div>
        <button class="nav size-[var(--ui-touch)] rounded-key border border-line bg-raised-2 leading-none text-muted hover:border-line-hi hover:text-text" onClick={() => step(1)} title="Next preset">
          ›
        </button>
        <button class="nav presets-open size-[var(--ui-touch)] rounded-key border border-line bg-raised-2 leading-none text-muted hover:border-line-hi hover:text-text" onClick={() => (presetsOpen.value = true)} title="Browse, save, rename, and delete presets">
          ☰
        </button>
      </div>

      <div class="slots flex gap-1">
        {SLOTS.map((s2, i) => (
          <div
            key={s2}
            role="button"
            tabIndex={0}
            class={`slot size-[var(--ui-touch)] rounded-key bg-[#22242b] border border-line-soft text-muted text-[11px] font-semibold grid place-items-center ${idx === i ? "on bg-linear-to-b from-amber-hi to-amber-lo border-amber-edge text-amber-ink shadow-glow" : ""}`}
            title={s.presets[i] ?? "empty"}
            onClick={() => s.presets[i] && void loadPreset(s.presets[i])}
          >
            {s2}
          </div>
        ))}
      </div>

      <div class="top-right flex items-center gap-3.5 ml-auto compact:flex-wrap portrait:flex-nowrap! portrait:ml-0 compact:gap-y-2">
        <button
          class={`min-h-[var(--ui-touch)] px-2.5 flex items-center gap-1.5 rounded-md border text-[11px] font-semibold tracking-[0.6px] uppercase ${tunerOpen.value ? "on bg-[#2f323b] text-text border-line inset-shadow-[0_-2px_0_var(--color-amber)]" : "bg-[#1a1b21] text-muted border-line hover:text-text hover:border-line-hi"}`}
          onClick={() => (tunerOpen.value = true)}
          title="Tuner (T)"
          aria-label="Open the tuner"
          aria-expanded={tunerOpen.value}
        >
          <TunerIcon size={11} />
          <span>Tuner</span>
        </button>
        <button
          class={`models-btn min-h-[var(--ui-touch)] px-2.5 flex items-center gap-1.5 rounded-md border text-[10px] font-semibold tracking-[0.6px] uppercase bg-[#1a1b21] text-muted border-line hover:text-text hover:border-line-hi`}
          onClick={() => openModels()}
          title="Browse, filter, inspect and download models"
          aria-label="Open the model browser"
        >
          <svg class="w-3.5 h-3.5" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><rect x="1" y="1" width="14" height="14" rx="3" stroke="currentColor" fill="none"/><line x1="3" y1="8" x2="13" y2="8" stroke="currentColor"/><line x1="6" y1="3" x2="6" y2="8" stroke="currentColor"/><line x1="11" y1="3" x2="11" y2="8" stroke="currentColor"/></svg>
          <span>Models</span>
        </button>
        <button
          class="fs-pill min-h-[var(--ui-touch)] w-[64px] px-2 flex items-center justify-center rounded-md border border-line bg-[#1a1b21] text-[10px] font-semibold tracking-[0.6px] text-amber tabular-nums hover:border-line-hi"
          onClick={() => (footswitchOpen.value = true)}
          title="Footswitches and expression pedal"
          aria-label="Open the footswitch panel"
        >
          FS: {footswitch.value.up ? "U" : footswitch.value.down ? "D" : footswitch.value.expression >= 0.95 ? "E" : "—"}
        </button>
        <div class="source-toggle flex border border-line rounded-md overflow-hidden">
          {(["riff", "live", "silence"] as const).map((src) => (
            <button
              key={src}
              class={`chip min-h-[var(--ui-touch)] px-2.5 text-[11px] font-semibold tracking-[0.6px] uppercase bg-[#1a1b21] text-muted border-r border-line-soft last:border-r-0 ${s.source === src ? "on bg-[#2f323b] text-text inset-shadow-[0_-2px_0_var(--color-amber)]" : ""}`}
              onClick={() => void setSource(src)}
            >
              {src}
            </button>
          ))}
        </div>
        <Meter label="IN" value={meters.value.in} />
        <Meter label="OUT" value={meters.value.out} />
        <div class="cpu grid place-items-center gap-1" title="DSP load">
          <span class="cpu-value text-[11px] tabular-nums text-text">{meters.value.cpu.toFixed(0)}%</span>
          <span class="cpu-label text-[9px] tracking-[0.6px] text-muted">DSP</span>
        </div>
        {/* Latency readout: one-way IN/OUT and round-trip TOTAL, in ms. */}
        <div class="latency flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] font-mono tabular-nums bg-[#1a1b21] border border-line rounded-md px-2 py-1.5">
          <span class="text-muted">IN</span>
          <span class="text-amber">{Number.isFinite(s.oneWayLatencyMs) ? s.oneWayLatencyMs.toFixed(2) : "—"}</span>
          <span class="text-muted">OUT</span>
          <span class="text-amber">{Number.isFinite(s.roundTripLatencyMs) ? s.roundTripLatencyMs.toFixed(2) : "—"}</span>
          <span class="text-muted">TOT</span>
          <span class="text-amber">{Number.isFinite(s.roundTripLatencyMs) ? s.roundTripLatencyMs.toFixed(2) : "—"}</span>
          <span class="text-muted">ms</span>
        </div>
        {/* Buffer-size control: a dropdown populated from the engine's supported
            options (128/256/512 by default). Changing it re-routes the audio
            streams; the engine replies with a fresh snapshot. */}
        <label class="buffer-select grid items-center justify-items-center gap-1">
          <span class="ctrlName">BUFFER</span>
          <select
            class="textField min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3"
            value={s.bufferFrames}
            onChange={(e) => void setConfig(Number((e.target as HTMLSelectElement).value))}
            title="Change I/O buffer size"
          >
            {bufferSizeOptions.value.map((v) => <option key={v} value={v}>{v}</option>)}
          </select>
        </label>
        {/* The master output knob: last child of the top-right area, top right. */}
        <HeaderKnob
          label="Master"
          value={s.masterOutVolume ?? 0}
          min={-60}
          max={12}
          defaultValue={0}
          onChange={(v) => void setMasterOutVolume(v)}
        />
      </div>
    </header>
  );
}
