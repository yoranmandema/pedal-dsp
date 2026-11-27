import { useEffect } from "preact/hooks";
import { error, init, notice, ready } from "./state";
import { TopBar } from "./components/TopBar";
import { Rack } from "./components/Rack";
import { Chain } from "./components/Chain";
import { Editor } from "./components/Editor";
import { PresetBrowser } from "./components/PresetBrowser";
import { ModelBrowser } from "./components/ModelBrowser";
import { AddBlockModal } from "./components/AddBlockModal";
import { TunerModal } from "./components/TunerModal";
import { FootswitchPanel } from "./components/FootswitchPanel";
import { GainStagingModal } from "./components/GainStagingModal";
import { SelectionRing } from "./selection";

export function App() {
  useEffect(() => {
    void init();
  }, []);

  if (error.value) {
    return (
      <div class="boot grid place-items-center h-full bg-stage">
        <div class="boot-card text-center grid gap-2">
          <h1 class="text-2xl tracking-[6px] font-bold">Pedal</h1>
          <p>Engine unavailable: {error.value}</p>
          <p class="boot-hint text-muted">
            Start the engine with <code class="px-1.5 py-0.5 rounded-sm bg-[#121318] border border-line-soft text-xs">./target/debug/pedal-engine</code> and reload.
          </p>
        </div>
      </div>
    );
  }

  if (!ready.value) {
    return (
      <div class="boot grid place-items-center h-full bg-stage">
        <div class="boot-card text-center grid gap-2">
          <h1 class="text-2xl tracking-[6px] font-bold">Pedal</h1>
          <p>Connecting to the engine…</p>
        </div>
      </div>
    );
  }

  return (
    <div class="app grid h-full grid-rows-[auto_minmax(0,1fr)_auto]">
      <TopBar />
      <div class="main grid min-h-0 grid-cols-1 grid-rows-[auto_minmax(0,1fr)] tab:grid-cols-[240px_minmax(0,1fr)] tab:grid-rows-1">
        <Rack />
        <Chain />
      </div>
      <Editor />
      <TunerModal />
      <PresetBrowser />
      <FootswitchPanel />
      <ModelBrowser />
      <AddBlockModal />
      <GainStagingModal />
      {/* Drawn above the chain but outside every scrolling container, so the
          ring is never clipped by the strip or the panel. */}
      <SelectionRing />
      {/* Command failures surface here instead of failing silently. It shares
          the bar's own grid cell, so it tracks the bar's height at every width
          (a fixed `bottom` would only be right while the bar is `--ui-bar`
          tall) and is lifted clear of it by one `--space-2`. */}
      {notice.value && (
        <div class="toast col-start-1 row-start-3 self-start justify-self-end mr-4 -translate-y-[calc(100%+var(--space-2))] z-50 max-w-[min(420px,calc(100vw-24px))] rounded-lg bg-danger-bg border border-danger-line px-3 py-2 text-danger-text text-base cursor-pointer shadow-toast" onClick={() => (notice.value = null)}>
          {notice.value}
        </div>
      )}
    </div>
  );
}