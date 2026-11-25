// Touch drag: the long-press timing, and the routing of a rack drag into a
// channel.
//
// HTML5 drag-and-drop does not work on a touchscreen: a finger drag is a
// scroll and no `dragstart` ever fires. So a touch drag is built from Pointer
// Events instead, and it is the same gesture the cards use to reorder — only
// the start condition differs:
//
//   mouse   — moving past `DRAG_THRESHOLD_PX` starts the drag (a plain click
//             stays a click);
//   touch   — holding still for `LONG_PRESS_MS` starts it, and the finger may
//             then move freely.
//
// A touch drag that starts on a rack row ends over the chain, so its move and
// up events are listened for on `window` and resolved to the `.channel` element
// at the pointer. `Chain` installs the router; each channel registers its own
// preview/commit callbacks.

import { useEffect, useRef } from "preact/hooks";
import { DRAG_THRESHOLD_PX, LONG_PRESS_MS, LONG_PRESS_SLOP_PX } from "./dnd";
import { touchDragBlock, touchDragOver } from "./state";

// --- Gesture timing --------------------------------------------------------

export interface TouchDragCallbacks {
  /** The press became a drag. `x`/`y` is the pointer at that moment. */
  onStart: (x: number, y: number) => void;
  /** The drag is moving. Only fires after `onStart`. */
  onMove: (x: number, y: number) => void;
  /** The gesture ended — a drop, a cancel, or a plain click. */
  onEnd: (dragged: boolean, x: number, y: number) => void;
}

export interface TouchDragHandlers {
  onPointerDown: (e: PointerEvent) => void;
  onPointerMove: (e: PointerEvent) => void;
  onPointerUp: (e: PointerEvent) => void;
  onPointerCancel: (e: PointerEvent) => void;
}

interface Press {
  x: number;
  y: number;
  pointerId: number;
  /** The long-press timer, while it is still pending. */
  timer: ReturnType<typeof setTimeout> | null;
  /** True once the press has become a drag. */
  dragged: boolean;
}

/**
 * Wire a long-press/drag gesture onto an element.
 *
 * Returns the four pointer handlers to spread onto it. The element needs
 * `touch-action: none` (the `touch-none` class) so a touch drag reaches
 * `pointermove` instead of scrolling the panel.
 *
 * `onEnd` receives the pointer position so a drop can be resolved to the
 * element under the finger — a touch drag that starts on a rack row ends over
 * the chain, not over the row.
 */
export function useTouchDrag({ onStart, onMove, onEnd }: TouchDragCallbacks): TouchDragHandlers {
  const cb = useRef({ onStart, onMove, onEnd });
  cb.current = { onStart, onMove, onEnd };

  const press = useRef<Press | null>(null);

  const clear = () => {
    const p = press.current;
    if (p?.timer != null) clearTimeout(p.timer);
    press.current = null;
  };

  // If the element unmounts mid-gesture, do not leave a timer or a drag behind.
  useEffect(() => clear, []);

  return {
    onPointerDown: (e) => {
      // Only the primary button / a touch or pen contact starts a gesture.
      if (e.pointerType === "mouse" && e.button !== 0) return;
      if (press.current) return;
      const el = e.currentTarget as HTMLElement;
      press.current = { x: e.clientX, y: e.clientY, pointerId: e.pointerId, timer: null, dragged: false };
      // A synthetic or already-released pointer throws; the gesture still works
      // through the window-level move/up listeners.
      try {
        el.setPointerCapture?.(e.pointerId);
      } catch {
        /* no active pointer to capture */
      }
      // A mouse press becomes a drag on movement alone; the timer is for a
      // real touch or pen contact. An absent `pointerType` (a synthetic event)
      // is treated as a mouse, so a drag threshold applies.
      if (e.pointerType === "touch" || e.pointerType === "pen") {
        press.current.timer = setTimeout(() => {
          const p = press.current;
          if (!p) return;
          p.dragged = true;
          p.timer = null;
          cb.current.onStart(p.x, p.y);
        }, LONG_PRESS_MS);
      }
    },
    onPointerMove: (e) => {
      const p = press.current;
      if (!p || e.pointerId !== p.pointerId) return;
      const dx = Math.abs(e.clientX - p.x);
      const dy = Math.abs(e.clientY - p.y);
      if (!p.dragged) {
        // Only a real touch/pen contact uses the long-press path.
        const isTouch = e.pointerType === "touch" || e.pointerType === "pen";
        if (!isTouch) {
          if (dx < DRAG_THRESHOLD_PX && dy < DRAG_THRESHOLD_PX) return;
          p.dragged = true;
          cb.current.onStart(e.clientX, e.clientY);
          return;
        }
        // Touch: a long press that moves too far is a scroll, not a drag.
        if (p.timer != null && (dx >= LONG_PRESS_SLOP_PX || dy >= LONG_PRESS_SLOP_PX)) {
          clearTimeout(p.timer);
          p.timer = null;
        }
        return;
      }
      cb.current.onMove(e.clientX, e.clientY);
    },
    onPointerUp: (e) => {
      const p = press.current;
      if (!p || e.pointerId !== p.pointerId) return;
      clear();
      cb.current.onEnd(p.dragged, e.clientX, e.clientY);
    },
    onPointerCancel: (e) => {
      const p = press.current;
      if (!p || e.pointerId !== p.pointerId) return;
      clear();
      cb.current.onEnd(p.dragged, e.clientX, e.clientY);
    },
  };
}

// --- Rack long press -------------------------------------------------------

/**
 * A rack row's long-press handlers.
 *
 * Touch only: a mouse drag is HTML5 drag (`dragstart`/`dragend` on the row).
 * Holding a finger still for `LONG_PRESS_MS` publishes the block on
 * `touchDragBlock` and marks the row `.dragging`; moving out of the slop
 * before the timer cancels it, so a scroll is never mistaken for a drag.
 */
export function useRackLongPress(): TouchDragHandlers {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const origin = useRef<{ x: number; y: number } | null>(null);

  useEffect(
    () => () => {
      if (timer.current != null) clearTimeout(timer.current);
    },
    [],
  );

  return {
    onPointerDown: (e) => {
      // A mouse drag is HTML5 drag (`dragstart`/`dragend` on the row), not this.
      if (e.pointerType !== "touch" && e.pointerType !== "pen") return;
      const row = e.currentTarget as HTMLElement;
      origin.current = { x: e.clientX, y: e.clientY };
      try {
        row.setPointerCapture?.(e.pointerId);
      } catch {
        /* no active pointer to capture */
      }
      if (timer.current != null) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        timer.current = null;
        const type = row.dataset.blockType ?? "";
        const name = row.dataset.blockName ?? "";
        if (!type) return;
        touchDragBlock.value = { type, name };
        row.classList.add("dragging");
      }, LONG_PRESS_MS);
    },
    onPointerMove: (e) => {
      if (timer.current == null || !origin.current) return;
      const dx = Math.abs(e.clientX - origin.current.x);
      const dy = Math.abs(e.clientY - origin.current.y);
      if (dx >= LONG_PRESS_SLOP_PX || dy >= LONG_PRESS_SLOP_PX) {
        clearTimeout(timer.current);
        timer.current = null;
      }
    },
    onPointerUp: () => {
      if (timer.current != null) clearTimeout(timer.current);
      timer.current = null;
      origin.current = null;
    },
    onPointerCancel: () => {
      if (timer.current != null) clearTimeout(timer.current);
      timer.current = null;
      origin.current = null;
    },
  };
}

// --- Routing a rack drag into a channel ------------------------------------

/** A channel's strip, registered by the channel component. */
interface TouchChannel {
  el: HTMLElement | null;
}

/** The registered channels, by id. */
const channels = new Map<string, TouchChannel>();

/** What the chain does when a touch drag moves over / ends in a channel. */
export interface TouchDropTarget {
  /** Preview the insertion slot; `null` clears the highlight. */
  preview: (slot: number | null) => void;
  /** Insert the payload at `slot`. */
  commit: (payload: { type: string; name: string }, slot: number) => void;
}

const targets = new Map<string, TouchDropTarget>();

/** Register a channel's preview/commit callbacks. Returns a detach fn. */
export function registerTouchTarget(id: string, target: TouchDropTarget): () => void {
  targets.set(id, target);
  return () => {
    if (targets.get(id) === target) targets.delete(id);
  };
}

/** Keep the router's geometry for a channel up to date. */
export function registerTouchChannel(id: string, el: HTMLElement | null): () => void {
  if (el) channels.set(id, { el });
  else channels.delete(id);
  return () => channels.delete(id);
}

/** The channel currently under the finger. */
let active: { id: string; target: TouchDropTarget } | null = null;

/** Clear the active channel's highlight, if any. */
function clearActive(): void {
  if (active) active.target.preview(null);
  active = null;
}

/** The `.channel` element under a viewport point, or null. */
function channelAt(x: number, y: number): HTMLElement | null {
  const el = document.elementFromPoint(x, y);
  return (el?.closest(".channel") as HTMLElement | null) ?? null;
}

/** The slot under `x` in a channel: the first card whose midpoint is right. */
function slotAt(ch: TouchChannel, clientX: number): number {
  if (!ch.el) return 0;
  const cards = Array.from(ch.el.querySelectorAll(".block-card")) as HTMLElement[];
  for (const el of cards) {
    const r = el.getBoundingClientRect();
    if (clientX < r.left + r.width / 2) return Number(el.dataset.slot);
  }
  return cards.length;
}

/** Resolve a channel id to its geometry, or null. */
function geometryOf(id: string): TouchChannel | null {
  return channels.get(id) ?? null;
}

/**
 * Install the window-level routing. Call once, from `Chain`.
 *
 * While `touchDragBlock` is set, moving over a channel previews the slot and
 * lifting inside one inserts. Lifting outside every channel does nothing.
 */
export function useTouchDropRouter(): void {
  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (!touchDragBlock.value) return;
      const el = channelAt(e.clientX, e.clientY);
      const id = el?.dataset.channelId ?? null;
      const next = id ? targets.get(id) : null;
      if (!id || !next) {
        if (active) {
          clearActive();
          touchDragOver.value = null;
        }
        return;
      }
      if (!active || active.id !== id) {
        clearActive();
        active = { id, target: next };
        touchDragOver.value = id;
      }
      const ch = geometryOf(id);
      if (ch) active.target.preview(slotAt(ch, e.clientX));
    };

    const finish = (e: PointerEvent) => {
      const payload = touchDragBlock.value;
      if (!payload) return;
      const el = channelAt(e.clientX, e.clientY);
      const id = el?.dataset.channelId ?? null;
      const a = active;
      clearActive();
      touchDragBlock.value = null;
      touchDragOver.value = null;
      if (!id || !a || a.id !== id) return; // dropped outside every channel
      const ch = geometryOf(id);
      if (!ch) return;
      a.target.commit(payload, slotAt(ch, e.clientX));
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      clearActive();
    };
  }, []);
}
