import { state, addBlock } from "../state";
import { BlockList, groupBlocks, type BlockEntry } from "./BlockList";
import { GainStagingBlock } from "./GainStagingBlock";

/** The left rack: every block type the engine offers, grouped by category. */
export function Rack() {
  const s = state.value;
  if (!s) return null;

  const pick = ({ type, name }: BlockEntry) => {
    const ch = s.channels[0];
    if (ch) void addBlock(ch.id, type, name);
  };

  const ch = s.channels[0];
  const gsBlock = ch?.blocks.find((b: { type: string }) => b.type === "gain_staging");

  return (
    <aside class="rack bg-panel border-r border-line overflow-y-auto p-2.5 phone:max-h-[200px] phone:overflow-auto phone:border-r-0 phone:border-b phone:p-2">
      {gsBlock && <GainStagingBlock channel={ch?.id ?? ""} block={gsBlock} />}
      <BlockList groups={groupBlocks(s.registry)} onSelect={pick} />
    </aside>
  );
}