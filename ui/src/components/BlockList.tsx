// The block catalogue, shared by the left rack and the add-block picker. One
// renderer means both surfaces stay identical: category grouping, ordering,
// colour and row layout all come from here, so the picker really is the rack's
// browser reused as a modal.
//
// Rows are drag sources three ways:
//   - HTML5 drag on a mouse (`dragstart` publishes a `BlockPayload` on the
//     shared `BLOCK_MIME` type, see `dnd.ts`);
//   - a long press on touch, which publishes the same payload on
//     `touchDragBlock` for the channel view to pick up (`touch.ts`);
//   - Enter/Space, which selects the type for the channel's keyboard-insert
//     button (`state.ts` `selectedBlockType`).
//
// `dragend` (and the pointer gesture's `up`/`cancel`) clear the transient
// `.dragging` marker and the stored payload, whichever way the gesture ends.

import { BlockIcon, Icon } from "./Icon";
import { catStyle } from "../theme";
import type { BlockSpec, Category } from "../protocol";
import { endDrag, startDrag, type BlockPayload } from "../dnd";
import { selectedBlockType, selectBlockType, touchDragBlock } from "../state";
import { useRackLongPress } from "../touch";

/**
 * Categories in display order. I/O and Split/Merge are deliberately absent:
 * they are graph wiring primitives (input/output nodes, the recombine summer),
 * not pedals a player drops into a chain.
 */
export const CATEGORY_ORDER: Category[] = [
  "Modelling",
  "Distortion",
  "Tone",
  "Dynamics",
  "Time",
  "Modulation",
  "Pitch",
  "Utility",
];

export interface BlockEntry {
  type: string;
  name: string;
  category: Category;
  /** Default configuration, taken straight from the registry spec. */
  config: Record<string, number>;
}

export interface BlockGroup {
  category: Category;
  blocks: BlockEntry[];
}

/** Default configuration for a block: every parameter at its spec default. */
function defaultConfig(spec: BlockSpec): Record<string, number> {
  return Object.fromEntries(spec.params.map((p) => [p.id, p.default]));
}

/**
 * Group the registry by category in display order, dropping entries that do not
 * match `query` (case-insensitive substring over name and type). An empty
 * query lists everything. Insertion order within a category is preserved.
 */
export function groupBlocks(
  registry: Record<string, BlockSpec>,
  query = "",
): BlockGroup[] {
  const q = query.trim().toLowerCase();
  const byCategory = new Map<Category, BlockEntry[]>();
  for (const [type, spec] of Object.entries(registry)) {
    if (q && !`${spec.name} ${type}`.toLowerCase().includes(q)) continue;
    const list = byCategory.get(spec.category) ?? [];
    list.push({
      type,
      name: spec.name,
      category: spec.category,
      config: defaultConfig(spec),
    });
    byCategory.set(spec.category, list);
  }
  return CATEGORY_ORDER.filter((c) => byCategory.has(c)).map((c) => ({
    category: c,
    blocks: byCategory.get(c)!,
  }));
}

/** Outcome of the most recent browser drag. `dragend` writes it; the
 *  drop-target side and the edge-case tests read it. */
export const lastBlockDrag: { dropped: boolean; payload: BlockPayload | null } = {
  dropped: false,
  payload: null,
};

/**
 * The grouped catalogue itself: one section per category, one button per block.
 * It reads `groups` and reports a click through `onSelect(entry)`, so the same
 * list serves the always-visible rack and the modal picker. Grouping and
 * filtering live in `groupBlocks`, not here.
 */
export function BlockList({
  groups,
  onSelect,
}: {
  groups: BlockGroup[];
  onSelect: (entry: BlockEntry) => void;
}) {
  const chosen = selectedBlockType.value;
  const drag = touchDragBlock.value;
  const longPress = useRackLongPress();

  return (
    <div class="block-list phone:flex phone:items-start phone:gap-4">
      {groups.map((g) => (
        <section key={g.category} class="flex flex-col gap-1 phone:flex-none">
          <h2 class="cat-scope flex items-center gap-1.5 text-[10px] tracking-[1.4px] font-semibold mb-1.5 text-cat phone:mb-1 phone:text-[9px] phone:tracking-[1px]" style={catStyle(g.category)}>
            <Icon category={g.category} size={12} />
            <span>{g.category}</span>
          </h2>
          {g.blocks.map((b) => (
            <button
              key={b.type}
              class={[
                "rack-item cat-scope w-full flex items-center gap-2 px-2.5 min-h-[var(--ui-touch)] py-1.75 rounded-md text-left border border-transparent border-l-3 bg-linear-to-r from-cat-soft to-transparent cursor-grab transition-[background-color,border-color,border-left-color] duration-120 hover:bg-panel-2 hover:border-[#3b3e49] hover:border-l-cat phone:flex-none phone:w-[172px] phone:px-2 phone:py-1 touch-none portrait:min-h-[var(--ui-touch)]",
                chosen?.type === b.type ? "border-amber-dim bg-[rgba(232,163,61,0.08)]" : "",
                drag?.type === b.type ? "dragging border-dashed border-[#4a4d59] opacity-35" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              style={catStyle(b.category)}
              draggable
              aria-pressed={chosen?.type === b.type}
              data-block-type={b.type}
              data-block-name={b.name}
              onDragStart={(e) => startDrag(e, e.currentTarget as HTMLElement, { type: b.type, name: b.name, config: b.config })}
              onDragEnd={endDrag}
              {...longPress}
              onClick={() => onSelect(b)}
              onKeyDown={(e) => {
                // Enter/Space selects the type for the keyboard-insert path.
                // The button's own click would also fire, so prevent it.
                if (e.key !== "Enter" && e.key !== " ") return;
                e.preventDefault();
                selectBlockType({ type: b.type, name: b.name, config: b.config });
              }}
              title={`${b.name} — drag to a channel`}
            >
              <span class="rack-icon grid place-items-center text-cat">
                <BlockIcon type={b.type} category={b.category} size={18} />
              </span>
              <span class="rack-text grid gap-[1px] min-w-0">
                <span class="rack-name text-[12px] font-medium">{b.name}</span>
              </span>
            </button>
          ))}
        </section>
      ))}
    </div>
  );
}
