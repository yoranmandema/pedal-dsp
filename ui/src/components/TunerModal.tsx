import { useEffect } from "preact/hooks";
import { tuner, tunerOpen, presetsOpen, addBlockFor } from "../state";
import {
  modalBackdrop,
  modalFoot,
  modalHead,
  modalShell,
  modalTitle,
  navBtn,
} from "../ui";

/** The instrument tuner. Always mounted (returns `null` when closed) so its
 *  keyboard effect persists across closes; it only renders the panel when
 *  `tunerOpen` is true. Read-only: it receives `event: tuner` from the engine
 *  and sends nothing back. */
export function TunerModal() {
  const t = tuner.value;
  const absCents = Math.abs(t.cents);

  // Always mounted so the keyboard effect below persists across closes, but
  // render nothing while closed -- without this the backdrop is permanently
  // on screen.
  if (!tunerOpen.value) return null;

  // Keyboard: T/t toggles, Escape closes. The modal is always mounted, so the
  // listener persists across closes and reopens; the handler only acts while
  // the panel is open, and defers to the preset browser and add-block picker,
  // which own their own Escape.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      const active = document.activeElement;
      if (
        active instanceof HTMLInputElement ||
        active instanceof HTMLTextAreaElement ||
        active instanceof HTMLSelectElement ||
        (active instanceof HTMLElement && active.isContentEditable)
      ) {
        return;
      }
      if (presetsOpen.value || addBlockFor.value) return;
      if (e.key === "t" || e.key === "T") {
        tunerOpen.value = !tunerOpen.value;
      } else if (e.key === "Escape") {
        tunerOpen.value = false;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Recompute the note colour from the live reading on every event.
  const noteColour =
    absCents <= 5 ? "text-ok" : absCents <= 15 ? "text-amber" : "text-danger-text";
  const needleColour =
    absCents <= 5 ? "var(--color-ok)" : absCents <= 15 ? "var(--color-amber)" : "var(--color-danger-text)";

  return (
    <div
      class={`${modalBackdrop} modalBackdrop`}
      onClick={() => (tunerOpen.value = false)}
    >
      <div
        class={`${modalShell} modalShell modal tuner w-[min(440px,calc(100vw-48px))]`}
        onClick={(e) => e.stopPropagation()}
      >
        <header class={`${modalHead} modalHead`}>
          <h2 class={`${modalTitle} modalTitle`}>Tuner</h2>
          <span
            class={`size-2 rounded-full ${t.active ? "bg-ok" : "bg-muted"}`}
            title="listening"
          />
          <button
            class={`${navBtn} navBtn nav`}
            onClick={() => (tunerOpen.value = false)}
            title="Close"
          >
            ×
          </button>
        </header>

        <div class="flex-1 overflow-y-auto">
          {/** Note readout: always in the DOM. While active it shows the note
              * name and octave; when inactive an em dash in text-muted. */}
          <div class="tuner-note">
            <span
              class={`text-64 leading-none font-bold tabular-nums ${t.active ? noteColour : "text-muted"}`}
            >
              {t.active ? t.note : "—"}
            </span>
            <span
              class="text-2xl font-semibold tabular-nums"
              style={{ verticalAlign: "baseline" }}
            >
              {t.active ? t.octave : ""}
            </span>
          </div>

          {t.active ? (
            <>
              {/* Verdict pill. */}
              {absCents <= 5 ? (
                <div
                  class={
                    "tuner-verdict rounded-full border px-2.5 py-1 text-2xs font-semibold border-line-hi text-ok"
                  }
                >
                  In tune
                </div>
              ) : absCents <= 15 ? (
                <div
                  class={
                    "tuner-verdict rounded-full border px-2.5 py-1 text-2xs font-semibold border-line-hi text-amber"
                  }
                >
                  {t.cents < 0 ? "flat" : "sharp"}
                </div>
              ) : null}

              {/* Cents gauge. */}
              <div
                class={
                  "tuner-gauge relative mx-auto h-2 w-full max-w-[320px] rounded-full bg-well border border-line-soft"
                }
              >
                <div
                  class="tuner-needle absolute top-1/2 h-4 w-[3px] -translate-y-1/2 -translate-x-px rounded-full transition-[left] duration-100 ease-linear"
                  style={{
                    left: `${50 + (t.cents / 50) * 50}%`,
                    backgroundColor: needleColour,
                  }}
                />
                <div class="absolute left-1/2 -top-1 h-5 w-px bg-line-hi" />
                <div class="mt-1 flex justify-between text-2xs text-muted">
                  <span>-50</span>
                  <span class="font-semibold">0</span>
                  <span>+50</span>
                </div>
              </div>

              {/* Frequency line. */}
              <div class="tuner-freq text-sm tabular-nums text-muted">
                {t.freq.toFixed(1)} Hz · A4 = {t.reference.toFixed(0)} Hz
              </div>

              {/* Signal bar. */}
              <div class="flex items-center gap-2">
                <div class="h-1 w-full max-w-[320px] rounded-full bg-well">
                  <div
                    class="h-full rounded-full bg-ok"
                    style={{ width: `${t.clarity * 100}%` }}
                  />
                </div>
                <span class="text-2xs text-muted">signal</span>
              </div>
            </>
          ) : (
            <div class="tuner-idle flex flex-col items-center py-6 text-center">
              <p class="text-xl text-muted">Play a note</p>
              <p class="text-sm text-muted">
                The tuner listens to the instrument input, whatever the source
                is set to.
              </p>
            </div>
          )}
        </div>

        <footer class={`${modalFoot} modalFoot`}>
          <span class="text-2xs text-muted">Esc closes · T toggles</span>
        </footer>
      </div>
    </div>
  );
}
