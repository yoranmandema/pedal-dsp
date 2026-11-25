// Cross-channel routing lines.
//
// A routing is a tap from one block's output to a block in another channel.
// It is drawn as a dashed amber curve from the source card's output dot to the
// destination card's input dot, above the channel rows. That is what makes it
// read as something added on top of the chain rather than as part of it: the
// in-channel flow is only the left-to-right card order and is never drawn.
//
// The overlay is a sibling of the channel rows inside `.chain-scroll` (which is
// the positioning context), so it is not clipped by the horizontal `.strip`
// scrollers and it scrolls with the panel. Every coordinate is measured live
// from the DOM — `getBoundingClientRect` of the two cards relative to the
// overlay — so a line follows its blocks when a strip scrolls, the panel
// scrolls, the window resizes, or the rig changes. Nothing about a routing's
// geometry is sent to the engine.
//
// Interactive controls for the selected routing: delete (Delete key, context
// menu, or the bar's trash button), enable/disable (bar's toggle), and gain
// (bar's dB slider). The bar is a sibling rendered on top of the lines, so it
// never intercepts block drags or line selection outside its own area. It is
// `pointer-events: auto` (the SVG overlay is `pointer-events: none`, so this
// widget must opt back in).

import { useEffect, useRef, useState } from "preact/hooks";
import { selectedRouting, selectRouting, state } from "../state";
import type { State } from "../protocol";
import { PowerIcon } from "./Icon";
import { removeRouting, setEnabled, setGain } from "../state";

/** The app's amber accent. Routings are the one thing drawn in it, so a line
 *  never reads as part of a block's own category colour. */
const AMBER = "#e8a33d";

/** Arrowhead length and half-width, in px. */
const ARROW_LEN = 4;
const ARROW_HALF_WIDTH = 2.5;

/** S-curve shared by the live preview line and the real routings. */
export function routeCurve(x1: number, y1: number, x2: number, y2: number): string {
  const dx = x2 - x1;
  const bend = Math.abs(dx) < 8 ? 6 : Math.max(20, Math.min(Math.abs(dx) * 0.5, 90));
  return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`;
}

/** The app's amber accent. */
export const ROUTING_AMBER = "#e8a33d";

/** The measured geometry of one line; styling is applied at render time. */
interface Line {
  id: string;
  d: string;
  /** Midpoint of the line (relative to the overlay box). Where the bar sits. */
  mid: { x: number; y: number };
  /** Arrowhead polygon at the destination; null for a disabled routing. */
  arrow: string | null;
  disabled: boolean;
}

/** Does `endpoint` name a block that is still in the rig? */
function hasBlock(s: State, e: { channel: string; block: string }): boolean {
  return (s.channels.find((c) => c.id === e.channel)?.blocks ?? []).some((b) => b.id === e.block);
}

/**
 * Measure every routing that can be drawn.
 *
 * Endpoints are resolved against `state.channels` first, so a routing whose
 * block has been deleted is skipped rather than drawn to a stale card. Block
 * ids are unique across the rig, so one scan of the chain's cards is enough.
 */
function measure(overlay: HTMLElement): Line[] {
  const s = state.value;
  if (!s) return [];
  const box = overlay.getBoundingClientRect();
  const cards = new Map<string, HTMLElement>();
  overlay.parentElement
    ?.querySelectorAll<HTMLElement>("[data-block-id]")
    .forEach((el) => cards.set(el.dataset.blockId ?? "", el));

  const lines: Line[] = [];
  for (const r of s.routings ?? []) {
    const { source, dest } = r;
    if (!hasBlock(s, source) || !hasBlock(s, dest)) continue;
    const from = cards.get(source.block);
    const to = cards.get(dest.block);
    if (!from || !to) continue;

    const a = from.getBoundingClientRect();
    const b = to.getBoundingClientRect();
    // The cards carry their input/output dots just outside the border box
    // (`-left-1` / `-right-1`, 5px wide), so the line meets the dot, not the
    // card's edge.
    const x1 = a.right - box.left + 1.5;
    const y1 = a.top + a.height / 2 - box.top;
    const x2 = b.left - box.left - 1.5;
    const y2 = b.top + b.height / 2 - box.top;

    const dx = x2 - x1;
    // A horizontal S-curve. The destination is always entered from the left,
    // along its input dot, so the arrowhead points straight into the card.
    const bend = Math.abs(dx) < 8 ? 6 : Math.max(20, Math.min(Math.abs(dx) * 0.5, 90));
    const d = `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`;
    const arrow = r.enabled
      ? `${x2},${y2} ${x2 - ARROW_LEN},${y2 - ARROW_HALF_WIDTH} ${x2 - ARROW_LEN},${y2 + ARROW_HALF_WIDTH}`
      : null;

    lines.push({ id: r.id, d, mid: { x: (x1 + x2) / 2, y: (y1 + y2) / 2 }, arrow, disabled: !r.enabled });
  }
  return lines;
}

/**
 * The routing overlay. Mounted once, inside the chain's scroll content, and
 * re-measured on every scroll, resize and rig change.
 */
export function RoutingLines() {
  const [lines, setLines] = useState<Line[]>([]);
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  // The floating action bar for the selected routing; it stays in step with
  // the page because it is re-rendered on every state change and is positioned
  // from live DOM measurements.
  const s = state.value;
  const sel = selectedRouting.value;
  const routing = sel !== null ? s?.routings.find((r) => r.id === sel) ?? null : null;

  useEffect(() => {
    let raf = 0;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let last = "";

    // The overlay's own box, the chain container, the scrolling panel and
    // every strip: any of them changing size moves the cards.
    const watch = () => {
      const el = ref.current;
      if (!el) return;
      ro.observe(el);
      const chain = el.parentElement;
      if (!chain) return;
      ro.observe(chain);
      if (chain.parentElement) ro.observe(chain.parentElement);
      chain.querySelectorAll(".strip").forEach((strip) => ro.observe(strip));
    };

    const read = () => {
      const el = ref.current;
      if (!el) return;
      watch();
      const next = measure(el);
      // After a block is deleted the engine prunes every routing touching it —
      // the selected id can vanish from `state.routings`. Clear the selection
      // and drop the bar (architect spec: "do not draw the bar").
      if (sel !== null && !next.some((l) => l.id === sel)) {
        selectRouting(null);
        return;
      }
      const key = next.map((l) => `${l.id}|${l.d}|${l.arrow}`).join("\n");
      if (key === last) return;
      last = key;
      setLines(next);
    };

    const schedule = () => {
      if (!raf) raf = requestAnimationFrame(() => { raf = 0; read(); });
      // A tab that is not being painted produces no frames, so `rAF` alone
      // could leave the lines stale; the timer is the fallback.
      if (!timer) timer = setTimeout(() => { timer = null; read(); }, 120);
    };

    const ro = new ResizeObserver(schedule);

    const onScroll = () => schedule();
    const onResize = () => schedule();
    const detachState = state.subscribe(schedule);
    window.addEventListener("resize", onResize);
    // `capture` so a scroll in any scroller — the panel or a strip — re-measures.
    window.addEventListener("scroll", onScroll, true);
    // A freshly mounted card may still be moving; re-measure as layout settles.
    const timers = [50, 150, 400].map((t) => setTimeout(schedule, t));
    // The first measurement is synchronous, so the lines are on screen with the
    // first paint rather than a frame later.
    read();
    schedule();

    return () => {
      if (raf) cancelAnimationFrame(raf);
      if (timer) clearTimeout(timer);
      timers.forEach(clearTimeout);
      detachState();
      ro.disconnect();
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, []);

  // --- Keyboard: Delete removes the selected routing --------------------------
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (sel !== null && e.key === "Delete") {
        const target = e.target as HTMLElement | null;
        // Never hijack Delete in a focused text field.
        if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) return;
        e.preventDefault();
        if (sel) void removeRouting(sel);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [sel]);

  // --- Context menu: hides itself when the user clicks away ---------------------
  useEffect(() => {
    if (menu === null) return;
    const onMouseDown = (e: PointerEvent) => {
      const target = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
      if (!target) { setMenu(null); return; }
      // Outside the menu and not on a routing line: close it.
      if (!target.closest("[data-menu]") && !target.closest("[data-routing-id]")) setMenu(null);
    };
    window.addEventListener("pointerdown", onMouseDown);
    return () => window.removeEventListener("pointerdown", onMouseDown);
  }, [menu]);

  // Touch has no right-click: a 500ms press on a line opens the same menu.
  const pressTimer = useRef<number | undefined>(undefined);
  const pressStart = (id: string) => (e: PointerEvent) => {
    if (e.pointerType === "mouse") return;
    const { clientX: x, clientY: y } = e;
    window.clearTimeout(pressTimer.current);
    pressTimer.current = window.setTimeout(() => openMenu(id, x, y), 500);
  };
  const pressEnd = () => window.clearTimeout(pressTimer.current);

  const openMenu = (id: string, x: number, y: number) => {
    const r = routingAt(id);
    if (!r) return;
    selectRouting(id); // the bar + Delete key now act on this line as well
    const maxX = Math.max(0, window.innerWidth - 220);
    const maxY = Math.max(0, window.innerHeight - 320);
    setMenu({ id, x: Math.min(x, maxX), y: Math.min(y, maxY) });
  };

  const routingAt = (id: string): State["routings"][number] | null => s?.routings.find((r) => r.id === id) ?? null;

  const deleteRouting = () => {
    if (sel !== null) void removeRouting(sel);
    setMenu(null);
  };

  const toggleRouting = () => {
    if (routing === null) return;
    void setEnabled(sel ?? "", !routing.enabled);
  };

  const setGainDb = (db: number) => {
    if (routing === null || sel === null) return;
    const gain = 10 ** (db / 20);
    void setGain(sel, gain);
  };

  // Position the bar near the selected line's midpoint, clamped inside the
  // overlay so it is never clipped by the scroll content.
  const barLeft = useRef(0);
  const barTop = useRef(0);
  if (sel !== null && routing !== null) {
    const l = lines.find((x) => x.id === sel);
    if (l) {
      const barW = 280;
      const barH = 46;
      let left = l.mid.x - barW / 2;
      let top = l.mid.y - barH / 2;
      const overlay = ref.current;
      const w = overlay?.clientWidth ?? 0;
      const h = overlay?.clientHeight ?? 0;
      if (left < 4) left = 4;
      else if (left + barW > w) left = Math.max(4, w - barW);
      if (top < 4) top = 4;
      else if (top + barH > h) top = Math.max(4, h - barH);
      barLeft.current = left;
      barTop.current = top;
    }
  }

  const line = sel !== null ? lines.find((l) => l.id === sel) ?? null : null;

  // dB read from the linear gain stored by the engine: dB = 20*log10(gain).
  const routingGainDb = routing !== null && routing.gain > 0
    ? Math.round(20 * Math.log10(routing.gain))
    : 0;

  return (
    <div ref={ref} class="pointer-events-none absolute inset-0 z-10 overflow-hidden">
      {lines.map((l) => {
        const selected = sel === l.id;
        const opacity = l.disabled ? 0.35 : selected ? 1 : 0.7;
        return (
          <svg key={l.id} class="absolute inset-0 h-full w-full overflow-visible" data-routing-id={l.id}>
            <g class="group">
              <path
                d={l.d}
                fill="none"
                stroke={AMBER}
                stroke-width={selected ? 2.5 : 1.5}
                stroke-dasharray={selected ? undefined : "5 4"}
                opacity={opacity}
                
                style={{
                  filter: selected
                    ? "drop-shadow(0 0 6px rgba(232,163,61,0.95))"
                    : "drop-shadow(0 0 4px rgba(232,163,61,0.55))",
                }}
              />
              {l.arrow && <polygon points={l.arrow} fill={AMBER} opacity={opacity} />}
              {/* Invisible wide stroke: the click target. The overlay itself is
                  `pointer-events: none`, so only this stroke takes clicks. */}
              <path
                d={l.d}
                fill="none"
                stroke={AMBER}
                stroke-width={28}
                opacity={0}
                class="cursor-pointer [pointer-events:stroke]"
                onClick={() => selectRouting(l.id)}
                onPointerDown={pressStart(l.id)}
                onPointerUp={pressEnd}
                onPointerCancel={pressEnd}
                onPointerLeave={pressEnd}
                onContextMenu={(e) => { e.preventDefault(); openMenu(l.id, e.clientX, e.clientY); }}
              >
                <title>{`Routing ${l.id}`}</title>
              </path>
              {l.arrow && (
                <polygon
                  points={l.arrow}
                  fill={AMBER}
                  opacity={opacity}
                  onContextMenu={(e) => { e.preventDefault(); openMenu(l.id, e.clientX, e.clientY); }}
                />
              )}
            </g>
          </svg>
        );
      })}

      {/* Floating action bar for the selected routing. Rendered above the
          lines, and `pointer-events: auto` so it regains hit-testing in its own
          area while never intercepting the blocks or lines beneath it. */}
      {sel !== null && routing !== null && line !== null && (
        <div
          class="pointer-events-auto absolute z-20 flex flex-wrap items-center gap-1.5 rounded-lg border border-line bg-panel px-2.5 py-1.5 shadow-xl"
          style={{ left: barLeft.current, top: barTop.current, width: 280 }}
        >
          {/* Delete: keyboard, context menu or bar. */}
          <button
            class="flex items-center gap-1.5 rounded-md px-2.5 min-h-[var(--ui-touch)] text-sm font-semibold text-danger-text transition hover:bg-danger-bg"
            title="Delete routing (Delete)"
            onClick={() => deleteRouting()}
          >
            <svg class="flex-none size-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width={2} stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/></svg>
            Delete
          </button>

          {/* Toggle: label reflects the target state; "Disable" while on. */}
          <button
            class={`flex items-center gap-1.5 rounded-md px-2.5 min-h-[var(--ui-touch)] text-sm font-semibold transition ${routing.enabled ? "text-text hover:bg-[#26282f]" : "text-muted hover:text-text hover:bg-[#26282f]"}`}
            title={routing.enabled ? "Disable routing" : "Enable routing"}
            onClick={() => toggleRouting()}
          >
            <PowerIcon size={14} />
            {routing.enabled ? "Disable" : "Enable"}
          </button>

          {/* Gain: dB slider, −60…+24, step 1. Wire value is linear gain
              `10 ** (dB / 20)`, matching the engine's `20 * log10(gain)`.
              Default position for a new routing (linear 1.0) is 0 dB. */}
          <div class="flex flex-1 items-center gap-2">
            <span class="text-[11px] font-medium text-muted">Gain</span>
            <input
              class="w-full min-w-0 max-w-[140px] rounded border border-line bg-[var(--color-inset)] px-1.5 text-right text-[11px] font-mono tabular-nums text-text"
              type="range"
              min={-60}
              max={24}
              step={1}
              value={routingGainDb}
              title={`Gain ${routingGainDb} dB`}
              onInput={(e) => {
                const el = e.target as HTMLInputElement;
                setGainDb(parseFloat(el.value));
              }}
            />
            <span class="text-[11px] font-mono tabular-nums text-text">{routingGainDb} dB</span>
          </div>
        </div>
      )}

      {/* Context menu for the selected routing. */}
      {menu !== null && routingAt(menu.id) !== null && (
        <div
          data-menu
          class="fixed z-50 min-w-[200px] rounded-lg border border-line bg-panel p-1 shadow-xl"
          style={{ left: menu.x, top: menu.y }}
        >
          <div class="border-b border-line-soft" />
          <button
            class="flex w-full items-center gap-2 rounded-md px-3 min-h-[var(--ui-touch)] text-left text-sm text-muted transition hover:bg-[#26282f] hover:text-text"
            onClick={() => {
              if (menu.id) void removeRouting(menu.id);
              setMenu(null);
            }}
          >
            <svg class="flex-none size-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width={2} stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/></svg>
            Delete routing
          </button>
          <button
            class={`flex w-full items-center gap-2 rounded-md px-3 min-h-[var(--ui-touch)] text-left text-sm transition ${routingAt(menu.id)!.enabled ? "text-text hover:bg-[#26282f]" : "text-muted hover:text-text hover:bg-[#26282f]"}`}
            onClick={() => {
              const r = routingAt(menu.id);
              if (r !== null) void setEnabled(menu.id, !r.enabled);
              setMenu(null);
            }}
          >
            <PowerIcon size={14} />
            {routingAt(menu.id)!.enabled ? "Disable routing" : "Enable routing"}
          </button>
        </div>
      )}
    </div>
  );
}
