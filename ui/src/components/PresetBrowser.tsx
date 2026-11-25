import { useEffect, useRef, useState } from "preact/hooks";
import { deletePreset, loadPreset, presetsOpen, renamePreset, savePreset, state } from "../state";
import { emptyNote, modalBackdrop, modalFoot, modalHead, modalShell, modalTitle, navBtn, textField } from "../ui";

/** Mirror of the engine's file-name slugging, for duplicate checks. */
function slugify(name: string): string {
  return name.replace(/[^a-zA-Z0-9_-]/g, "-");
}

/**
 * The preset browser (US-2.9): list every preset, load one, save the current
 * rig under a new name, rename, and delete. Every action goes through the
 * bridge; the engine answers with a fresh snapshot.
 */
export function PresetBrowser() {
  const s = state.value;
  const [newName, setNewName] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameTo, setRenameTo] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const renameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (renaming) renameRef.current?.focus();
  }, [renaming]);

  if (!presetsOpen.value || !s) return null;

  const close = () => {
    presetsOpen.value = false;
    setErr(null);
    setRenaming(null);
  };

  const run = async (p: Promise<unknown>) => {
    setBusy(true);
    setErr(null);
    try {
      await p;
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };

  const save = async () => {
    const slug = slugify(newName.trim());
    if (!slug) return;
    await run(savePreset(slug));
    setNewName("");
  };

  const rename = async (from: string) => {
    const slug = slugify(renameTo.trim());
    if (!slug || slug === from) {
      setRenaming(null);
      return;
    }
    await run(renamePreset(from, slug));
    setRenaming(null);
  };

  const remove = async (name: string) => {
    if (!window.confirm(`Delete preset "${name}"?`)) return;
    await run(deletePreset(name));
  };

  const slug = slugify(newName.trim());
  const duplicate = slug !== "" && s.presets.includes(slug);

  return (
    <div class={`${modalBackdrop} modalBackdrop modal-backdrop`} onClick={close}>
      <div class={`${modalShell} modalShell modal`} onClick={(e) => e.stopPropagation()}>
        <header class={`${modalHead} modalHead modal-head`}>
          <h2 class={`${modalTitle} modalTitle modal-title`}>Presets</h2>
          <button class={`${navBtn} navBtn nav`} onClick={close} title="Close">×</button>
        </header>

        {err && <div class="modal-error mx-3.5 mt-2.5 px-2.5 py-1.75 rounded-md bg-danger-bg border border-danger-line text-danger-text text-[11px]">{err}</div>}

        <div class="preset-list flex-1 overflow-y-auto p-3.5 grid gap-1">
          {s.presets.length === 0 && <div class={`${emptyNote} editor-empty emptyNote`}>No presets yet — save the current rig below.</div>}
          {s.presets.map((name) => (
            <div key={name} class={`preset-row flex items-center gap-2 px-1 py-1 rounded-lg border transition-[background-color,border-color] ${name === s.preset ? "current border-amber-dim" : "border-transparent hover:bg-panel-2 hover:border-[#33363f]"}`}>
              {renaming === name ? (
                <input class={`${textField} textField preset-rename flex-1 px-2 py-1.75`} ref={renameRef} value={renameTo} title="New preset name" onInput={(e) => setRenameTo((e.target as HTMLInputElement).value)} onKeyDown={(e) => { if (e.key === "Enter") void rename(name); if (e.key === "Escape") setRenaming(null); }} onBlur={() => setRenaming(null)} />
              ) : (
                <button class={`preset-load flex-1 text-left text-base font-medium px-2 min-h-[var(--ui-touch)] rounded-md text-text hover:bg-[#26282f] ${name === s.preset ? "text-amber" : ""}`} onClick={() => void run(loadPreset(name))} title={`Load ${name}`}>
                  {name}
                  {name === s.preset && <span class="preset-current ml-2 text-[9px] tracking-[0.8px] uppercase text-muted">current</span>}
                </button>
              )}
              <button class="mini min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 text-xs text-muted hover:text-text" onClick={() => { setRenaming(name); setRenameTo(name); }} title="Rename preset">Rename</button>
              <button class="mini danger min-h-[var(--ui-touch)] min-w-[var(--ui-touch)] px-3 text-xs text-muted hover:text-danger" onClick={() => void remove(name)} title="Delete preset">Delete</button>
            </div>
          ))}
        </div>

        <footer class={`${modalFoot} modalFoot modal-foot`}>
          <input class={`${textField} textField preset-new flex-1 px-2 min-h-[var(--ui-touch)]`} placeholder="New preset name" value={newName} title="Name for the current rig" onInput={(e) => setNewName((e.target as HTMLInputElement).value)} onKeyDown={(e) => { if (e.key === "Enter") void save(); }} />
          <button class="btn px-3.5 min-h-[var(--ui-touch)] rounded-md text-base font-semibold bg-linear-to-b from-amber-hi to-amber-lo text-amber-ink border border-amber-edge disabled:opacity-40 disabled:cursor-default" disabled={!slug || duplicate || busy} onClick={() => void save()}>Save</button>
          {duplicate && <span class="modal-hint text-[10px] text-amber">That name already exists.</span>}
        </footer>
      </div>
    </div>
  );
}