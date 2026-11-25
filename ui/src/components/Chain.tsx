import { Fragment } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { BlockIcon, CloseIcon, PlusIcon, PowerIcon, PortIcon, SpeakerIcon, SourceIcon } from "./Icon";
import { RoutingLines, routeCurve, ROUTING_AMBER } from "./RoutingLines";
import { catStyle } from "../theme";
import { decodeBlockPayload, isBlockDrag, rejectDrop, type BlockPayload } from "../dnd";
import { registerTouchChannel, registerTouchTarget, useTouchDropRouter, useTouchDrag } from "../touch";
import {
  insertBlock,
  moveBlock,
  openBlockPicker,
  removeBlock,
  select,
  selectRouting,
  createRouting,
  selected,
  setBypass,
  setMasterVolume,
  setSources,
  state,
  meters,
} from "../state";

/** Live state of a pointer-driven block drag (a card reorder). */
interface Drag {
  /** Block being dragged. */
  id: string;
  /** Index it started at, in the channel's block array. */
  from: number;
  /** Slot it would land in, counting the gaps between cards (0..len). */
  insert: number;
  /** Pointer position, for the floating ghost. */
  x: number;
  y: number;
}

/** Live state of a block dragged from the browser hovering this channel. */
interface Drop {
  /** Slot the insertion line sits at (0..len). */
  slot: number;
  /** Content-space x of that slot, for the indicator. */
  x: number;
}

/** One point in the window's content coordinate space. */
interface Point {
  x: number;
  y: number;
}

/** Live state of a cross-channel routing drag. */
interface RouteDrag {
  /** Block being routed out of (the only drag source for a routing). */
  source: { channel: string; block: string };
  /** Last pointer position, in content space. */
  pointer: Point;
  /** The last card that was a valid drop target, or null. */
  target: { channel: string; block: string } | null;
  /** The `.block-card` currently highlighted as the drop target, or null. */
  over: HTMLElement | null;
}

/** Warn about a refused drop. One line, never a thrown error. */
function warnDrop(reason: string): void {
  console.warn(`[chain] ignoring drop: ${reason}`);
}

/** Read and validate a drop's payload. Returns the payload, or null after
 * logging why it was refused. */
function payloadFromDrop(
  dt: DataTransfer | null,
  registry: Record<string, unknown>,
): BlockPayload | null {
  const payload = decodeBlockPayload(dt);
  const rejection = rejectDrop(payload, registry);
  if (rejection === "no-payload") warnDrop("no block payload on the data transfer");
  else if (rejection === "bad-type") warnDrop("payload has no block type id");
  else if (rejection === "unknown-type") warnDrop(`unknown block type "${payload?.type}"`);
  return rejection ? null : payload;
}

/** The slot under the pointer: the first card whose midpoint is to the right.
 * Shared by both branches — a populated channel has cards to measure, an empty
 * one has none and so always answers 0, which is the only slot it has. */
function slotAtFor(strip: HTMLDivElement | null, clientX: number, count: number): number {
  const els = Array.from(strip?.querySelectorAll(".block-card") ?? []) as HTMLElement[];
  for (const el of els) {
    const r = el.getBoundingClientRect();
    if (clientX < r.left + r.width / 2) return Number(el.dataset.slot);
  }
  return count;
}

/** One channel row: header above a horizontally scrolling strip of blocks. */
// `min-w-0` on the channel (and on its `.strip`) is load-bearing, not
// defensive: the strip is an `overflow-x-auto` row of fixed-width cards, so its
// min-content width is the entire row laid out end to end. A grid item
// defaults to `min-width: auto`, which hands that min-content width to the
// grid track — the channel then grows past the viewport and the whole chain
// panel scrolls horizontally instead of the strip scrolling on its own. Both
// the channel and the strip therefore opt out of the automatic minimum.
function ChannelRow({ channel, chain }: { channel: import("../protocol").Channel; chain: { current: HTMLDivElement | null } }) {
  // An empty channel is a fixed slot that happens to hold nothing, not a missing
  // one. So it still has to be *there*: header, signal path, the empty hint and
  // the "+" button all stay, and the slot keeps its place in the layout. What
  // it must not do is pay for any of the per-block machinery.
  // Everything below the header is therefore split on this one check. The
  // populated branch is the behaviour this component always had; the empty
  // branch renders the same shell with no cards and no card pointer handlers to
  // bind (it keeps the drop handlers, because an empty channel is still where a
  // dragged block lands, and it registers with the touch router, because a long
  // press from the rack can land there too). Dropping a block in flips
  // `blocks.length`, the branch swaps on the next snapshot, and the channel
  // resumes normal processing — no reload, and the row itself is not remounted.
  if (channel.blocks.length === 0) return <EmptyChannel channel={channel} />;
  return <PopulatedChannel channel={channel} chainRefEl={chain} />;
}

/** The shell of a channel with nothing in it.

 * Deliberately a separate component: it has no drag state and no card pointer
 * handlers, so Preact mounts a handful of elements and nothing more. It is not
 * `null` and not `display: none` — the slot stays visible and keeps its height,
 * which is what makes an empty channel "existent" in the layout.
 */
function EmptyChannel({ channel }: { channel: import("../protocol").Channel }) {
  const s = state.value!;
  const strip = useRef<HTMLDivElement>(null);
  const [dropActive, setDropActive] = useState(false);

  // The touch router resolves the channel under the finger and calls these.
  useEffect(
    () =>
      registerTouchTarget(channel.id, {
        preview: (slot) => setDropActive(slot !== null),
        commit: (payload, slot) => {
          setDropActive(false);
          void insertBlock(channel.id, slot, payload);
        },
      }),
    [channel.id],
  );

  // Keep the router's geometry current: it measures `.block-card` children.
  useEffect(() => registerTouchChannel(channel.id, strip.current), [channel.id]);

  const dragEnter = (e: DragEvent) => {
    const dt = e.dataTransfer;
    if (!dt || !isBlockDrag(dt)) return;
    e.preventDefault();
    dt.dropEffect = "copy";
    setDropActive(true);
  };

  const dragOver = (e: DragEvent) => {
    const dt = e.dataTransfer;
    if (!dt || !isBlockDrag(dt)) return;
    // Required, or the browser will refuse the drop.
    e.preventDefault();
    dt.dropEffect = "copy";
    setDropActive(true);
  };

  const dragLeave = (e: DragEvent) => {
    const next = e.relatedTarget as Node | null;
    if (next && (e.currentTarget as HTMLElement).contains(next)) return;
    setDropActive(false);
  };

  const drop = (e: DragEvent) => {
    e.preventDefault();
    setDropActive(false);
    const payload = payloadFromDrop(e.dataTransfer, s.registry);
    if (!payload) return;
    void insertBlock(channel.id, slotAtFor(strip.current, e.clientX, 0), payload);
  };

  return (
    <div
      class={`channel is-empty min-w-0 flex flex-col rounded-card border ${dropActive ? "drop-target-active border-amber-dim bg-[rgba(232,163,61,0.05)] shadow-[0_0_0_1px_var(--color-amber-dim),0_0_22px_rgba(232,163,61,0.22)]" : "border-line-soft bg-[rgba(255,255,255,0.012)]"} p-2.5 transition-[border-color,box-shadow,background-color] duration-120`}
      data-channel-id={channel.id}
      onDragEnter={dragEnter}
      onDragOver={dragOver}
      onDragLeave={dragLeave}
      onDrop={drop}
    >
      <ChannelHead channel={channel} />
      {/* `min-w-0` is not cosmetic: the strip scrolls its own overflow, so its
          min-content width (every card laid out in a row) must not be allowed
          to widen the grid track. Without it the channel grows past the
          viewport and the whole chain panel scrolls instead. */}
      <div class="strip flex min-h-[102px] min-w-0 items-center gap-2 overflow-x-auto px-2 py-[18px]" ref={strip}>
        <span class="port flex-none" aria-hidden="true">
          <PortIcon size={14} />
        </span>
        <div class="empty-zone flex-1" />
        <AddBlockButton channel={channel} />
        <span class="port flex-none" aria-hidden="true">
          <PortIcon size={14} />
        </span>
      </div>
    </div>
  );
}

/** The channel's "+" button: opens the block picker. */
function AddBlockButton({ channel }: { channel: import("../protocol").Channel }) {
  return (
    <button
      class="add-block flex-none size-[var(--ui-touch)] grid place-items-center rounded-full border border-dashed border-[#3f424d] text-muted hover:text-text hover:border-amber"
      data-add={channel.id}
      onClick={() => openBlockPicker(channel.id)}
      title="Search and add a block"
      aria-label="Search and add a block"
    >
      <PlusIcon size={18} />
    </button>
  );
}

/** The always-present header: the channel name. */
function ChannelHead({ channel }: { channel: import("../protocol").Channel }) {
  const numeral = channel.name.replace(/\D/g, "");
  const inputOn = channel.sources.includes("<in>");
  return (
    <div class="channel-head flex items-center gap-2 min-h-[var(--ui-touch)] mb-2">
      <span
        class="channel-chip flex-none w-[var(--ui-touch)] h-[var(--ui-touch)] grid place-items-center rounded-md bg-[#1a1b21] border border-line text-[12px] font-semibold tracking-[0.4px] tabular-nums text-muted"
        title={channel.name}
        aria-label={channel.name}
      >
        {numeral || channel.name}
      </span>
      <div
        class="channel-vol flex items-center gap-1.5 text-muted"
        title={`${channel.name} volume`}
        aria-label={`${channel.name} volume`}
      >
        <SpeakerIcon size={16} />
        <input
          class="w-28 touch-pan-x"
          type="range"
          min={-60}
          max={12}
          step={0.1}
          value={channel.volume}
          title={`${channel.name} volume, ${channel.volume.toFixed(1)} dB`}
          aria-label={`${channel.name} volume`}
          onChange={(e) => {
            const el = e.target as HTMLInputElement;
            void setMasterVolume(channel.id, parseFloat(el.value));
          }}
        />
      </div>
      <button
        class={`channel-src flex-none w-[var(--ui-touch)] h-[var(--ui-touch)] rounded-md bg-[#1a1b21] border border-line grid place-items-center text-muted ${inputOn ? "bg-[#2a2318] border-amber-dim text-amber border-amber-dim" : ""}`}
        aria-pressed={inputOn}
        title={`${channel.name} input source ${inputOn ? "on" : "off"}`}
        aria-label={`${channel.name} input source`}
        onClick={() => setSources(channel.id, inputOn ? ["<in>"] : [])}
      >
        <SourceIcon size={16} />
      </button>
    </div>
  );
}

/** A channel holding at least one block: the drag-and-drop strip. */
function PopulatedChannel({ channel, chainRefEl }: { channel: import("../protocol").Channel; chainRefEl: { current: HTMLDivElement | null } }) {
  const s = state.value!;
  const [drag, setDrag] = useState<Drag | null>(null);
  const [drop, setDrop] = useState<Drop | null>(null);
  const strip = useRef<HTMLDivElement>(null);
  const preview = useRef<SVGPathElement>(null);
  const routeRef = useRef<RouteDrag | null>(null);

  /** Slot under the pointer: the first card whose midpoint is to the right. */
  function slotAt(clientX: number): number {
    return slotAtFor(strip.current, clientX, channel.blocks.length);
  }

  /** Content-space x of the insertion boundary before `slot`. */
  function boundaryX(slot: number): number {
    const el = strip.current;
    if (!el) return 0;
    const rect = el.getBoundingClientRect();
    const cards = Array.from(el.querySelectorAll(".block-card")) as HTMLElement[];
    if (cards.length === 0) {
      const io = el.querySelector(".port") as HTMLElement | null;
      const r = io?.getBoundingClientRect();
      return r ? r.right - rect.left + el.scrollLeft : 0;
    }
    if (slot >= cards.length) {
      const last = cards[cards.length - 1].getBoundingClientRect();
      return last.right - rect.left + el.scrollLeft;
    }
    return cards[slot].getBoundingClientRect().left - rect.left + el.scrollLeft;
  }

  /** Remember the slot under the pointer and paint the insertion line. */
  function trackDrop(e: DragEvent) {
    const slot = slotAt(e.clientX);
    setDrop((d) => (d && d.slot === slot ? d : { slot, x: boundaryX(slot) }));
  }

  const dragEnter = (e: DragEvent) => {
    const dt = e.dataTransfer;
    if (!dt || !isBlockDrag(dt)) return;
    e.preventDefault();
    dt.dropEffect = "copy";
    trackDrop(e);
  };

  const dragOver = (e: DragEvent) => {
    const dt = e.dataTransfer;
    if (!dt || !isBlockDrag(dt)) return;
    e.preventDefault();
    dt.dropEffect = "copy";
    trackDrop(e);
  };

  const dragLeave = (e: DragEvent) => {
    const next = e.relatedTarget as Node | null;
    if (next && (e.currentTarget as HTMLElement).contains(next)) return;
    setDrop(null);
  };

  const dropFromBrowser = (e: DragEvent) => {
    e.preventDefault();
    const slot = drop?.slot ?? slotAt(e.clientX);
    setDrop(null);
    const payload = payloadFromDrop(e.dataTransfer, s.registry);
    if (!payload) return;
    void insertBlock(channel.id, slot, payload);
  };

  /**
   * Cancel a routing drag: drop the target highlight, clear the drag state and
   * release pointer capture. Safe to call with a stale drag.
   */
  const cancelRouteDrag = (r: RouteDrag | null): void => {
    if (!r) return;
    if (r.over) r.over.classList.remove("routing-target");
    routeRef.current = null;
  };

  /**
   * Start a routing drag from a block's output handle.
   *
   * `stopPropagation()` is what keeps the card-reorder drag (the card's own
   * `onPointerDown`/`onPointerMove`/`onPointerUp` via `cardPress`) out of this
   * gesture: the handle is inside the card, and the card's handlers must stay
   * untouched for normal reorders to continue working.
   */
  const startRouteDrag = (e: PointerEvent, handle: HTMLElement): void => {
    e.stopPropagation();
    e.preventDefault();
    try {
      handle.setPointerCapture(e.pointerId);
    } catch {
      // not captureable — the gesture still ends on `pointerup`.
    }
    const card = handle.parentElement?.closest("[data-block-id]") as HTMLElement | null;
    const channelEl = card?.closest(".channel") as HTMLElement | null;
    const source = {
      channel: channelEl?.dataset.channelId ?? "",
      block: card?.dataset.blockId ?? "",
    };
    if (!source.channel || !source.block) return;
    routeRef.current = {
      source,
      pointer: { x: e.clientX, y: e.clientY },
      target: null,
      over: null,
    };
  };

  /**
   * Move while a routing drag is in flight: re-hit-test the target card, keep
   * the target highlight and update the live preview line.
   */
  const moveRouteDrag = (e: PointerEvent, handle: HTMLElement): void => {
    const r = routeRef.current;
    if (!r) return;
    const px = e.clientX;
    const py = e.clientY;
    const el = document.elementFromPoint(px, py) as HTMLElement | null;
    // `closest("[data-block-id]")` from a handle descendant returns the handle
    // span (which carries no `data-channel-id`); `closest(".block-card")` plus
    // `.channel` resolves the channel for every pointer position over the card,
    // including exactly on the output handle.
    const card = (el?.closest(".block-card") as HTMLElement | null);
    const channelId = (card?.closest(".channel") as HTMLElement | null)?.dataset
      .channelId ?? "";
    let target: { channel: string; block: string } | null = null;
    if (card && card.dataset.blockId) {
      const ok =
        channelId !== r.source.channel &&
        card.dataset.blockId !== r.source.block;
      if (ok) {
        target = { channel: channelId, block: card.dataset.blockId };
      }
    }
    if (r.over) r.over.classList.remove("routing-target");
    if (target) {
      card!.classList.add("routing-target");
      r.over = card;
      r.target = target;
    } else {
      r.over = null;
      r.target = null;
    }
    r.pointer = { x: px, y: py };
    redrawPreview(handle, r);
  };

  /** Draw the preview line from the source handle to the last known pointer. */
  const redrawPreview = (handle: HTMLElement, r: RouteDrag): void => {
    const chainRect = chainRefEl.current?.getBoundingClientRect();
    if (!chainRect) return;
    const hRect = handle.getBoundingClientRect();
    const x1 = ((hRect.left + hRect.right) / 2) - chainRect.left;
    const y1 = (hRect.top + hRect.height / 2) - chainRect.top;
    const path = preview.current;
    if (!path) return;
    path.setAttribute(
      "d",
      routeCurve(x1, y1, r.pointer.x - chainRect.left, r.pointer.y - chainRect.top),
    );
  };

  /**
   * End the routing drag: re-hit-test the release point, create the routing on
   * a valid target, then cancel the drag cleanly (highlight + state cleared).
   *
   * The engine's refusal surfaces through `mutate()` into `notice` — show that
   * and do nothing else (no block move, no state patch).
   */
  const endRouteDrag = (e: PointerEvent, handle: HTMLElement): void => {
    const r = routeRef.current;
    if (!r) return;
    // Re-hit-test at the actual release point, not the last move point.
    const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
    // `closest("[data-block-id]")` from a handle descendant returns the handle
    // span (which carries no `data-channel-id`); `closest(".block-card")` plus
    // `.channel` resolves the channel for every pointer position over the card,
    // including exactly on the output handle.
    const card = (el?.closest(".block-card") as HTMLElement | null);
    const channelId = (card?.closest(".channel") as HTMLElement | null)?.dataset
      .channelId ?? "";
    let target: { channel: string; block: string } | null = null;
    if (card && card.dataset.blockId) {
      const ok =
        channelId !== r.source.channel &&
        card.dataset.blockId !== r.source.block;
      if (ok) {
        target = { channel: channelId, block: card.dataset.blockId };
      }
    }
    if (r.over) r.over.classList.remove("routing-target");
    routeRef.current = null;
    try {
      handle.releasePointerCapture(e.pointerId);
    } catch {
      // already released / never captured
    }
    if (target) {
      // `createRouting` returns nothing; the engine reply lands as a full
      // snapshot which `fold` picks up. A refusal is surfaced through `notice`.
      void createRouting(r.source, target);
    }
  };

  // Cancel a routing drag with Escape, if one is in flight.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") cancelRouteDrag(routeRef.current);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // --- Pointer reorder -----------------------------------------------------
  // A card drag (mouse or touch) reorders. `dragCardId` is set by `cardDown`
  // before the gesture's `onStart`, so the start handler knows which card.
  // `useTouchDrag` owns the long-press/drag-threshold timing.

  const dragCardId = useRef<string | null>(null);
  const cardPress = useTouchDrag({
    onStart: (x, y) => {
      const id = dragCardId.current;
      if (!id) return;
      const from = channel.blocks.findIndex((b) => b.id === id);
      if (from < 0) return;
      setDrag({ id, from, insert: slotAt(x), x, y });
    },
    onMove: (x, y) => {
      setDrag((d) => {
        if (!d) return d;
        const insert = slotAt(x);
        if (insert === d.insert && x === d.x && y === d.y) return d;
        return { ...d, insert, x, y };
      });
    },
    onEnd: (dragged) => {
      const d = drag;
      setDrag(null);
      if (!dragged || !d) return;
      // `moveBlock` indexes the array *after* the removal, so shift the target
      // down one slot when the block leaves a position left of its landing slot.
      const to = Math.min(d.insert > d.from ? d.insert - 1 : d.insert, channel.blocks.length - 1);
      if (to !== d.from) void moveBlock(channel.id, d.id, to);
    },
  });

  const cardDown = (e: PointerEvent, blockId: string) => {
    // The bypass/remove buttons are controls, not drag handles.
    if ((e.target as Element).closest("button")) return;
    dragCardId.current = blockId;
    cardPress.onPointerDown(e);
  };

  // --- Touch drag from the rack -------------------------------------------
  // `Chain` routes a rack long-press to the channel under the finger; this
  // channel previews the insertion line and inserts on release.

  useEffect(
    () =>
      registerTouchTarget(channel.id, {
        preview: (slot) => {
          if (slot === null) {
            setDrop(null);
            return;
          }
          setDrop((d) => (d && d.slot === slot ? d : { slot, x: boundaryX(slot) }));
        },
        commit: (payload, slot) => {
          setDrop(null);
          void insertBlock(channel.id, slot, payload);
        },
      }),
    // `boundaryX` closes over the current strip and block count.
    [channel.id, channel.blocks.length],
  );

  // Keep the router's geometry current: it measures `.block-card` children.
  useEffect(() => registerTouchChannel(channel.id, strip.current), [channel.id]);

  // --- Cross-channel routing drag (the drag-to-route interaction) ---------
  // A `pointerdown` on `.block-out` starts a routing drag *without* touching
  // the card's own reorder drag (see `startRouteDrag` and `cardDown`). The
  // preview line, the target highlight and the source restore are all managed
  // by `routeRef` — a plain DOM ref, so `pointermove` never re-renders the
  // component and the line never jitters.

  const dragBlock = drag ? channel.blocks.find((b) => b.id === drag.id) : undefined;
  const dragSpec = dragBlock ? s.registry[dragBlock.type] : undefined;

  return (
    <div
      class={`channel min-w-0 flex flex-col rounded-card border ${drop ? "drop-target-active border-amber-dim bg-[rgba(232,163,61,0.05)] shadow-[0_0_0_1px_var(--color-amber-dim),0_0_22px_rgba(232,163,61,0.22)]" : "border-line-soft bg-[rgba(255,255,255,0.012)]"} p-2.5 transition-[border-color,box-shadow,background-color] duration-120`}
      data-channel-id={channel.id}
      onDragEnter={dragEnter}
      onDragOver={dragOver}
      onDragLeave={dragLeave}
      onDrop={dropFromBrowser}
    >
      <ChannelHead channel={channel} />

      <div
        class="strip relative flex min-h-[102px] min-w-0 items-center gap-2 overflow-x-auto px-2 py-[18px]"
        ref={strip}
      >
        <span class="port flex-none" aria-hidden="true">
          <PortIcon size={14} />
        </span>
        {channel.blocks.map((b, i) => {
          const spec = s.registry[b.type];
          const isSel = selected.value?.block === b.id;
          return (
            <Fragment key={b.id}>
              {/* Always-mounted slot so the cards themselves are never added or
                  removed mid-drag (that would break pointer capture). */}
              <div
                class={`block-placeholder flex-none h-[66px] rounded-lg border-[1px] border-dashed border-transparent overflow-hidden transition-[width,margin-right,border-color,background-color] duration-110 ${drag && drag.insert === i && drag.insert !== drag.from ? "w-[116px] mr-0 border-amber-dim bg-[rgba(232,163,61,0.08)]" : "w-0 mr-[-8px]"}`}
              />
              <div
                class={[
                  "block-card cat-scope flex-none w-[116px] h-[66px] rounded-lg border border-[#353843] border-l-3 bg-linear-to-r from-cat-soft to-[#1b1c22] shadow-card grid grid-rows-[32px_1fr] items-center justify-items-center gap-0.5 px-2 py-1 cursor-grab select-none relative touch-none transition-[border-color,box-shadow,transform] duration-120 hover:border-[#4a4d59]",
                  b.bypass ? "bypass opacity-45" : "",
                  isSel ? "selected border-cat shadow-[inset_0_0_0_1px_var(--cat)] -translate-y-0.5" : "",
                  drag?.id === b.id ? "dragging border-dashed border-[#4a4d59] opacity-35" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                data-slot={i}
                data-block-id={b.id}
                style={{
                  ...(spec ? catStyle(spec.category) : null),
                  ...(drag?.id === b.id ? { opacity: 0.35 } : null),
                }}
                // Keyboard selection: the card is a button in all but name, so
                // Tab reaches it and Enter/Space selects it.
                tabIndex={0}
                {...cardPress}
                onPointerDown={(e) => cardDown(e, b.id)}
                onKeyDown={(e) => {
                  // Enter/Space on the card itself selects it. If the key was
                  // pressed on an inner button (bypass/remove), let the button
                  // handle it natively instead of also selecting the card.
                  if (e.target !== e.currentTarget) return;
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    select(channel.id, b.id);
                  }
                }}
                onClick={() => select(channel.id, b.id)}
                title={`${b.name} — drag to reorder, click to edit`}
                aria-label={b.name}
              >
                <div
                  class="card-actions absolute top-1 right-1 flex gap-1"
                >
                  <button
                    class={`icon-btn bypass absolute top-1 left-1 size-6 after:absolute after:-inset-2.5 rounded-full grid place-items-center bg-[#26282f] text-muted border border-line-soft ${b.bypass ? "bg-amber text-amber-ink border-amber-edge" : ""}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      void setBypass(channel.id, b.id, !b.bypass);
                    }}
                    title={b.bypass ? "Unbypass" : "Bypass"}
                    aria-label={b.bypass ? "Unbypass" : "Bypass"}
                    aria-pressed={b.bypass}
                  >
                    <PowerIcon size={14} />
                  </button>
                  <button
                    class="icon-btn remove absolute top-1 right-1 size-6 after:absolute after:-inset-2.5 rounded-full grid place-items-center bg-[#26282f] text-muted border border-line-soft hover:text-[#ff8a7a] hover:border-[#5a3535]"
                    onClick={(e) => {
                      e.stopPropagation();
                      void removeBlock(channel.id, b.id);
                    }}
                    title="Remove block"
                    aria-label="Remove block"
                  >
                    <CloseIcon size={13} />
                  </button>
                </div>
                <span class="block-icon grid place-items-center text-cat">
                  {spec && <BlockIcon type={b.type} category={spec.category} size={32} />}
                </span>
                {b.type === "dynamics.multiband_compressor" && (
                  <span
                    class="band-chip absolute -top-2 left-1/2 -translate-x-1/2 px-1.5 rounded-full border border-line bg-[#1a1b21] text-[9px] font-semibold leading-[14px] whitespace-nowrap text-cat pointer-events-none"
                    title={`${Math.min(4, Math.max(2, Math.round(b.params.bands ?? 3)))} bands`}
                  >
                    Multiband ({Math.min(4, Math.max(2, Math.round(b.params.bands ?? 3)))}b)
                  </span>
                )}
                {b.type === "dynamics.smart_noise_gate" && (() => {
                  const gr = meters.value.gateReduction ?? 0;
                  const pct = Math.min(100, gr * 100);
                  return (
                    <span
                      class="gate-chip absolute bottom-0 left-1/2 -translate-x-1/2 h-[2px] w-[40px] rounded-sm pointer-events-none"
                      style={{
                        background: `linear-gradient(to right, #22c55e ${pct}%, #ef4444 ${pct}%)`,
                      }}
                      title={`Gate reduction: ${gr > 0 ? Math.round(gr) : 0}%`}
                    />
                  );
                })()}
                <span
                  class={`block-name max-w-full overflow-hidden text-ellipsis whitespace-nowrap text-[11px] font-semibold text-center leading-[1.2] ${b.bypass ? "line-through" : ""}`}
                >
                  {b.name}
                </span>
                {/* The real output handle for cross-channel routings. It is a
                    real element (not the `after:` pseudo-dot, which cannot take
                    pointer events), is the only drag source for a routing, and its
                    `pointerdown` stops propagation so the card-reorder drag is
                    untouched. */}
                <span
                  class="block-out"
                  data-block-id={b.id}
                  data-channel-id={channel.id}
                  aria-hidden="true"
                  onPointerDown={(e) => startRouteDrag(e, e.currentTarget)}
                  onPointerMove={(e) => moveRouteDrag(e, e.currentTarget)}
                  onPointerUp={(e) => endRouteDrag(e, e.currentTarget)}
                  onPointerCancel={(e) => endRouteDrag(e, e.currentTarget)}
                  onLostPointerCapture={(e) => endRouteDrag(e, e.currentTarget)}
                />
              </div>
            </Fragment>
          );
        })}
        <div
          class={`block-placeholder flex-none h-[66px] rounded-lg border-[1px] border-dashed border-transparent overflow-hidden transition-[width,margin-right,border-color,background-color] duration-110 ${drag && drag.insert === channel.blocks.length && drag.insert !== drag.from ? "w-[116px] mr-0 border-amber-dim bg-[rgba(232,163,61,0.08)]" : "w-0 mr-[-8px]"}`}
        />
        {routeRef.current ? (
          <path
            ref={preview}
            class="block-route-preview"
            fill="none"
            stroke={ROUTING_AMBER}
            stroke-width={2}
            stroke-dasharray="5 4"
            opacity={0.85}
            style={{ pointerEvents: "none" }}
          />
        ) : null}
        {drop && (
          <div
            class="drop-indicator absolute top-[14px] bottom-[14px] w-[2px] -ml-[1px] rounded rounded-[1px] bg-amber shadow-[0_0_8px_rgba(232,163,61,0.8)] z-20 pointer-events-none"
            style={{ left: `${drop.x}px` }}
          />
        )}
        <AddBlockButton channel={channel} />
        <span class="port flex-none" aria-hidden="true">
          <PortIcon size={14} />
        </span>
      </div>

      {/* Ghost of the dragged card, following the pointer. */}
      {drag && (
        <div
          class="drag-ghost fixed z-40 transform -translate-x-1/2 -translate-y-1/2 scale-[1.04] px-3.5 py-2 rounded-lg bg-linear-to-b from-[#3a3e49] to-[#23252c] border border-amber shadow-drag text-text text-[11px] font-semibold whitespace-nowrap pointer-events-none"
          style={{
            left: `${drag.x}px`,
            top: `${drag.y}px`,
            ...(dragSpec ? catStyle(dragSpec.category) : null),
          }}
        >
          {dragSpec && <BlockIcon type={dragBlock!.type} category={dragSpec.category} size={20} />}
          {dragBlock?.name}
        </div>
      )}
    </div>
  );
}

/** The centre panel: one row per channel, blocks running left to right. */
export function Chain() {
  const chain = useRef<HTMLDivElement>(null);
  const s = state.value;
  // One window-level router for the whole chain: it resolves the channel under
  // the finger and forwards preview/commit to that channel.
  useTouchDropRouter();
  if (!s) return null;
  return (
    <div
      class="chain-panel overflow-auto p-2.5 bg-[radial-gradient(120%_90%_at_50%_0%,#1a1c22_0%,#101116_70%)]"
      onClick={(e) => {
        const t = e.target as HTMLElement;
        // Clicking empty space (not a routing line, not a block) clears the
        // selected routing. The overlay is pointer-events:none so clicks on a
        // line or block bypass this handler and each element handles its own.
        if (t.closest("[data-routing-id]") || t.closest("[data-block-id]")) return;
        selectRouting(null);
      }}
    >

      <div class="chain-scroll relative grid gap-2.5" ref={chain}>
        {s.channels.map((c) => (
          <ChannelRow key={c.id} channel={c} chain={chain} />
        ))}
        {/* Cross-channel routing lines. A sibling of the channel rows, so the
            horizontal strip scrollers do not clip them; `.chain-scroll` is
            `relative` to give the overlay its positioning context. */}
        <RoutingLines />
      </div>

    </div>
  );
}
