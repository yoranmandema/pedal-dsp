import { useEffect } from "preact/hooks";
import {
  expressionTarget, footswitch, footswitchAction, footswitchOpen, pressFootswitch, releaseFootswitch,
  setExpression, state, type FsAction,
} from "../state";
import { modalBackdrop, modalHead, modalShell, modalTitle, navBtn, textField } from "../ui";

const ACTIONS: { v: FsAction; label: string }[] = [
  { v: null, label: "-" },
  { v: "preset-up", label: "Preset up" },
  { v: "preset-down", label: "Preset down" },
];

/** Every numeric param in the current rig, as "channel|block|param" options. */
function targets() {
  const s = state.value;
  const out: { key: string; label: string }[] = [];
  if (!s) return out;
  for (const ch of s.channels)
    for (const b of ch.blocks)
      for (const p of s.registry[b.type]?.params ?? [])
        if (p.kind === "number") out.push({ key: `${ch.id}|${b.id}|${p.id}`, label: `${b.name || b.id} · ${p.name}` });
  return out;
}

/** The default binding: the pitch-delay's rate, when the rig has one. */
function defaultTarget(): string {
  const s = state.value;
  if (!s) return "";
  for (const ch of s.channels)
    for (const b of ch.blocks)
      if (b.type === "time.pitchdelay") return `${ch.id}|${b.id}|rate`;
  return "";
}

const tap = (which: "up" | "down") => {
  pressFootswitch(which);
  setTimeout(() => releaseFootswitch(which), 120);
};

/** Footswitch + expression controls (US-6.3). Keys: F1 = up, F2 = down, E = heel/toe. */
export function FootswitchPanel() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && /^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName)) return;
      if (e.key === "F1") { e.preventDefault(); tap("up"); }
      else if (e.key === "F2") { e.preventDefault(); tap("down"); }
      else if (e.key === "e" || e.key === "E") setExpression(footswitch.value.expression >= 0.5 ? 0 : 1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Pick the default target once a rig with a pitch-delay is present.
  useEffect(() => {
    if (!expressionTarget.value || !targets().some((t) => t.key === expressionTarget.value)) {
      expressionTarget.value = defaultTarget();
    }
  });

  if (!footswitchOpen.value || !state.value) return null;
  const close = () => (footswitchOpen.value = false);
  const fs = footswitch.value;
  const act = footswitchAction.value;
  const opts = targets();

  const sel = (which: "up" | "down", label: string) => (
    <label class="grid gap-1 text-[11px] text-muted">
      {label}
      <select
        class={`${textField} min-h-[var(--ui-touch)] px-3`}
        value={act[which] ?? ""}
        onChange={(e) => {
          const v = (e.target as HTMLSelectElement).value;
          footswitchAction.value = { ...act, [which]: v === "" ? null : (v as FsAction) };
        }}
      >
        {ACTIONS.map((a) => <option key={a.label} value={a.v ?? ""}>{a.label}</option>)}
      </select>
    </label>
  );

  return (
    <div class={`${modalBackdrop} modalBackdrop modal-backdrop`} onClick={close}>
      <div class={`${modalShell} modalShell modal footswitch-panel`} onClick={(e) => e.stopPropagation()}>
        <header class={`${modalHead} modalHead modal-head`}>
          <h2 class={`${modalTitle} modalTitle modal-title`}>Footswitches &amp; expression</h2>
          <button class={navBtn} onClick={close} aria-label="Close">✕</button>
        </header>
        <div class="grid gap-4 p-3.5">
          <div class="grid grid-cols-2 gap-3">
            {sel("up", "Footswitch U")}
            {sel("down", "Footswitch D")}
          </div>
          <div class="flex gap-2">
            <button class={`${navBtn} px-3 ${fs.up ? "text-amber" : ""}`} onClick={() => tap("up")} title="Simulate FS_U (F1)">Test U (F1)</button>
            <button class={`${navBtn} px-3 ${fs.down ? "text-amber" : ""}`} onClick={() => tap("down")} title="Simulate FS_D (F2)">Test D (F2)</button>
          </div>
          <label class="grid gap-1 text-[11px] text-muted">
            Expression bind
            <select
              class={`${textField} min-h-[var(--ui-touch)] px-3`}
              value={expressionTarget.value}
              onChange={(e) => (expressionTarget.value = (e.target as HTMLSelectElement).value)}
            >
              <option value="">no target</option>
              {opts.map((o) => <option key={o.key} value={o.key}>{o.label}</option>)}
            </select>
          </label>
          <label class="grid gap-1 text-[11px] text-muted">
            Expression (heel 0 → toe 1): {fs.expression.toFixed(2)} (E toggles)
            <input
              type="range" min="0" max="1" step="0.01" value={fs.expression}
              onInput={(e) => setExpression(Number((e.target as HTMLInputElement).value))}
            />
          </label>
        </div>
      </div>
    </div>
  );
}
