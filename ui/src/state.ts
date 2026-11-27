// The UI's single source of truth. Everything flows through the bridge; no
// component talks to the engine directly.

import { effect, signal } from "@preact/signals";
import { createBridge, type Bridge } from "./bridge";
import { type BlockPayload } from "./dnd";
import type { Config } from "./protocol";
import {
  asState,
  isDownload,
  isGainStaging,
  isLooper,
  isMeters,
  isState,
  isTuner,
  type DownloadItem,
  type GainStagingLevelEvent,
  type GainStagingState,
  type Meters,
  type Routing,
  type RoutingEndpoint,
  type State,
  type Tuner,
} from "./protocol";

// Re-export types used by components from state.ts
export { type DownloadItem } from "./protocol";

export const bridge: Bridge = createBridge();

export const state = signal<State | null>(null);
export const meters = signal<Meters>({ in: 0, out: 0, cpu: 0, xruns: 0 });
export const ready = signal(false);
/** Looper playback position, 0..1. Updated on `event: looper` at ~10 Hz while
 * the looper is Playing or Overdubbing; a plain signal so the panel re-renders
 * to a new number without animating. */
export const looperFrac = signal(0);
/** Supported I/O buffer sizes, in frames, from the engine's `getConfig`. */
export const bufferSizeOptions = signal<number[]>([]);
/** Fatal: the UI could not reach the engine at all. */
export const error = signal<string | null>(null);
/** Transient: the last command was rejected. Shown as a toast, never fatal. */
export const notice = signal<string | null>(null);
export const selected = signal<{ channel: string; block: string } | null>(null);
/** The selected cross-channel routing line, if any. UI-only: the engine has no
 *  notion of selection and is never told about it. */
export const selectedRouting = signal<string | null>(null);
/** Whether the preset browser modal is open. */
export const presetsOpen = signal(false);
export const footswitchOpen = signal(false);
/** Channel the add-block picker targets; null when the picker is closed. */
export const addBlockFor = signal<string | null>(null);
/** Current filter text of the add-block picker. */
export const blockQuery = signal("");
/** The tuner reading, updated on `event: tuner` at 20 Hz. Always-mounted: the
 *  modal's keyboard effect must persist even when the panel is closed. */
export const tuner = signal<Tuner>({
  active: false,
  freq: 0,
  note: "",
  octave: 0,
  cents: 0,
  clarity: 0,
  reference: 440,
});
/** The download queue, newest state per model id (from `event: download`). */
export const downloads = signal<DownloadItem[]>([]);

function upsertDownload(item: DownloadItem): void {
  const cur = downloads.value;
  const i = cur.findIndex((d) => d.id === item.id);
  downloads.value = i < 0 ? [...cur, item] : cur.map((d, j) => (j === i ? item : d));
}

export function dismissDownload(id: string): void {
  downloads.value = downloads.value.filter((d) => d.id !== id);
}

/** Whether the tuner modal is open. */
export const tunerOpen = signal(false);
/** Whether the gain-staging modal is open. */
export const gainStagingOpen = signal(false);
/** Live gain-staging snapshot from the engine. */
export const gainStaging = signal<GainStagingState>({});
/**
 * The block type selected in the browser (rack or picker), for the keyboard
 * path: Enter/Space on a row selects it here, then the channel view's
 * "Add selected block" button inserts it at the end of that channel.
 *
 * Null when nothing is selected. Survives across channel rows, so Tab can
 * move from the rack to the chain without losing the choice.
 */
export const selectedBlockType = signal<BlockPayload | null>(null);
/**
 * The block type a touch long-press picked up from the rack, for the duration
 * of the gesture. The channel view reads it on `pointerup` to insert the block
 * at the slot under the finger. Null when no touch drag is in flight.
 *
 * A module-level signal (rather than a prop) because the rack row that starts
 * the gesture and the channel that finishes it are not in a parent/child
 * relationship: `Rack` and `Chain` are siblings.
 */
export const touchDragBlock = signal<BlockPayload | null>(null);

/** The channel a touch drag is currently over, by id. */
export const touchDragOver = signal<string | null>(null);

/** Open the picker over `channel`, resetting any previous search. */
export function openBlockPicker(channel: string): void {
  blockQuery.value = "";
  addBlockFor.value = channel;
}

/** Highest revision applied. See `fold`. */
let lastRev = -1;

/**
 * Apply a snapshot, unless it is a duplicate or an out-of-order delivery.
 *
 * The engine sends the same state twice for a mutation (once in the reply, once
 * as a broadcast) and bumps `rev` on every rebuild, so `rev` is the ordering
 * key. A *lower* rev means the engine restarted and its counter began again —
 * accept that one, because the alternative is a UI that never recovers.
 */
function fold(next: State): boolean {
  if (next.rev === lastRev) return false;
  if (next.rev < lastRev) lastRev = -1;
  lastRev = next.rev;
  state.value = next;
  console.log("[state] fold rev=", next.rev, "state.value===next?", state.value === next);
  ready.value = true;
  // A snapshot means the engine is reachable; drop any boot-time error so a
  // slow-starting engine doesn't leave the UI stuck on the error screen.
  error.value = null;
  return true;
}

/** Connect and take the first snapshot. Safe to call once. */
export async function init(): Promise<void> {
  bridge.subscribe((msg) => {
    if (isState(msg)) fold(asState(msg.payload));
    else if (isMeters(msg)) meters.value = msg.payload;
    else if (isTuner(msg)) tuner.value = msg.payload;
    else if (isLooper(msg)) looperFrac.value = msg.payload.frac;
    else if (isDownload(msg)) upsertDownload(msg.payload);
    else if (isGainStaging(msg)) {
      const ev = msg as GainStagingLevelEvent;
      if (ev.event === "gain_staging_level") {
        gainStaging.value = {
          ...gainStaging.value,
          currentDb: ev.payload.level,
          peakDb: ev.payload.peak_level,
          active: ev.payload.active,
        };
      } else if (ev.event === "measure_complete") {
        gainStaging.value = {
          ...gainStaging.value,
          profileName: ev.payload.profile_name,
          recommendedGain: ev.payload.recommended_input,
          measuredAvg: ev.payload.recommended_input,
          measuredPeak: ev.payload.recommended_output,
          sdr: ev.payload.sdr,
          active: false,
          progress: 100,
        };
      } else if (ev.event === "gain_staging_profiles") {
        // Don't overwrite live snapshot fields; just store the names list.
        const prev = gainStaging.value;
        if (prev) {
          gainStaging.value = { ...prev, profileNames: ev.payload.profiles };
        }
      }
    }
  });
  try {
    fold(asState(await bridge.request("getState")));
    // Populate the buffer-size dropdown once; the engine greets every new
    // connection with a snapshot, which covers the latency readout. If the
    // second request fails (engine restarting), fall back to the engine's
    // built-in ladder so the control still renders and works.
    try {
      const cfg = await bridge.request("getConfig");
      if (cfg && typeof cfg === "object" && "supported" in cfg) {
        bufferSizeOptions.value = (cfg as Config).supported ?? [];
      }
    } catch {
      // ignore: the snapshot already has the current buffer size; the user can
      // still change it with the fallback list. The engine greets again with
      // fresh state after a buffer change, so the UI stays in sync.
      bufferSizeOptions.value = [128, 256, 512];
    }
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    // The engine may still be starting (the shell spawns it alongside the
    // window). The bridge keeps reconnecting and the engine greets every new
    // connection with a snapshot, which `fold` picks up, so keep showing
    // "Connecting…" instead of the error screen.
    if (msg !== "engine disconnected") error.value = msg;
  }
}

// --- Actions ----------------------------------------------------------------
// Mutating commands return the full new state, so there is nothing to merge.

/** Fold a command response into the snapshot. */
function apply(payload: unknown): void {
  if (!payload || typeof payload !== "object") return;
  const p = payload as Partial<State>;
  if (typeof p.rev === "number" && p.registry && Array.isArray(p.channels)) {
    fold(p as State);
  } else if (Array.isArray(p.presets) && state.value) {
    // A preset listing is a partial: patch just the names.
    state.value = { ...state.value, presets: p.presets };
  } else if (Array.isArray(p.routings) && state.value) {
    // `listRoutings` answers with the list alone; the snapshot stays the
    // authority for everything else.
    state.value = { ...state.value, routings: p.routings as Routing[] };
  }
}

/**
 * Send a mutation. Failures are surfaced rather than swallowed: an unhandled
 * rejection here used to leave the UI silently showing stale state.
 */
const mutate = (cmd: string, args?: Record<string, unknown>) =>
  bridge
    .request(cmd, args)
    .then(apply)
    .catch((e: unknown) => {
      const msg = e instanceof Error ? e.message : String(e);
      // A dropped socket is not a command failure: the reconnect greeting
      // brings the authoritative state. Say nothing.
      if (msg !== "engine disconnected") {
        notice.value = msg;
        // eslint-disable-next-line no-console
        console.error("[mutate] notice=", msg);
      }
    });

export const setParam = (channel: string, block: string, param: string, value: unknown) =>
  mutate("setParam", { channel, block, param, value });

/** Graphic EQ band change — rides the existing `setParam` command (no new protocol). */
export const setGraphicEqBand = (channel: string, band: number, db: number) =>
  setParam(channel, "tone.graphic_eq", `gain${band}`, db);

export const setConfig = (bufferFrames: number) =>
  mutate("setConfig", { buffer_frames: bufferFrames });

/** Fetch engine config: supported buffer sizes, current values, and latency. */
export const getConfig = () =>
  bridge.request("getConfig").then((payload) => payload as Config);

export const setBypass = (channel: string, block: string, bypass: boolean) =>
  mutate("setBypass", { channel, block, bypass });

export const setInputGain = (value: number) => mutate("setInputGain", { value });

/** Per-channel volume, dB. */
export const setMasterVolume = (channel: string, value: number) =>
  mutate("setMasterVolume", { channel, value });

export const setMasterOutVolume = (value: number) =>
  mutate("setMasterOutVolume", { value });

export const addBlock = (channel: string, type: string, name: string) =>
  mutate("addBlock", { channel, type, name });

/**
 * Insert a block at the end of `channel` — the keyboard path.
 *
 * Same as `addBlock`, but it also clears the browser selection, because the
 * choice has been consumed. `addBlock` itself stays a bare append so the rack
 * click and the picker keep their existing behaviour.
 */
export function appendSelectedBlock(channel: string, entry: BlockPayload): void {
  selectedBlockType.value = null;
  void addBlock(channel, entry.type, entry.name);
}

/**
 * Insert a block dragged from the browser at `index` in `channel`.
 *
 * The engine's `addBlock` appends, so this adds and then moves the new node
 * into place. The new id is found by diffing the channel's block ids across the
 * add, which is robust to the engine's name-derived id suffixing (`amp`, `amp2`,
 * …). A drop on an empty channel is just the add.
 */
export async function insertBlock(
  channel: string,
  index: number,
  entry: BlockPayload,
): Promise<void> {
  const before = new Set(
    (state.value?.channels.find((c) => c.id === channel)?.blocks ?? []).map((b) => b.id),
  );
  await mutate("addBlock", { channel, type: entry.type, name: entry.name });
  const blocks = state.value?.channels.find((c) => c.id === channel)?.blocks ?? [];
  const added = blocks.find((b) => !before.has(b.id));
  if (!added) return;
  const to = Math.max(0, Math.min(index, blocks.length - 1));
  if (to !== blocks.indexOf(added)) {
    await mutate("moveBlock", { channel, block: added.id, to });
  }
}

export const removeBlock = (channel: string, block: string) =>
  mutate("removeBlock", { channel, block });

export const moveBlock = (channel: string, block: string, to: number) =>
  mutate("moveBlock", { channel, block, to });

export const setSources = (channel: string, sources: string[]) =>
  mutate("setSources", { channel, sources });

// --- Cross-channel routings -------------------------------------------------
// A routing is a tap from one block to a block in another channel. Every
// command returns the full snapshot, so the list on screen is always the
// engine's, never a local guess.

export const createRouting = (source: RoutingEndpoint, dest: RoutingEndpoint) =>
  mutate("createRouting", { source, dest });

export const removeRouting = (id: string) => mutate("removeRouting", { id });

export const setEnabled = (id: string, enabled: boolean) =>
  mutate("setEnabled", { id, enabled });

export const setGain = (id: string, gain: number) => mutate("setGain", { id, gain });

export const listRoutings = () => mutate("listRoutings");

/** Select a routing line (or `null` to clear). Never sent to the engine. */
export const selectRouting = (id: string | null) => {
  selectedRouting.value = id;
};

export const setSource = (source: "riff" | "live" | "silence") =>
  mutate("setSource", { source });

export const listPresets = () => mutate("listPresets");

export const loadPreset = (name: string) => mutate("loadPreset", { name });

/** Step to the neighbouring preset (no-op at either end). Reuses `loadPreset`. */
export const nextPreset = () => stepPreset(1);
export const prevPreset = () => stepPreset(-1);
function stepPreset(d: number) {
  const s = state.value;
  if (!s) return;
  const next = s.presets.indexOf(s.preset) + d;
  if (next >= 0 && next < s.presets.length) void loadPreset(s.presets[next]);
}

// --- Footswitches and expression pedal (US-6.3) ---------------------------
export type FsAction = "preset-up" | "preset-down" | null;
/** Debounced switch / pedal state. Hardware (later) and the Test panel both write it. */
export const footswitch = signal({ up: false, down: false, expression: 0 });
/** What each switch does; `null` = unassigned. */
export const footswitchAction = signal<{ up: FsAction; down: FsAction }>({ up: null, down: null });
/** The block param the expression pedal drives: "channel|block|param". */
export const expressionTarget = signal<string>("");

const runAction = (a: FsAction) => {
  if (a === "preset-up") nextPreset();
  else if (a === "preset-down") prevPreset();
};

/** A switch press (edge). Updates the read-out and runs its bound action. */
export function pressFootswitch(which: "up" | "down") {
  footswitch.value = { ...footswitch.value, [which]: true };
  runAction(footswitchAction.value[which]);
}
export function releaseFootswitch(which: "up" | "down") {
  footswitch.value = { ...footswitch.value, [which]: false };
}

/** Find the bound param's spec in the current rig, or null. */
export function resolveExpressionTarget(): { channel: string; block: string; param: string; min: number; max: number } | null {
  const s = state.value;
  const [channel, block, param] = expressionTarget.value.split("|");
  if (!s || !param) return null;
  const ch = s.channels.find((c) => c.id === channel);
  const b = ch?.blocks.find((x) => x.id === block);
  const spec = b && s.registry[b.type]?.params.find((p) => p.id === param);
  return spec && spec.kind === "number" ? { channel, block, param, min: spec.min, max: spec.max } : null;
}

/** Expression 0..1 (heel..toe) -> the bound param, linearly across its range. */
export function setExpression(v: number) {
  const e = Math.min(1, Math.max(0, v));
  footswitch.value = { ...footswitch.value, expression: e };
  const t = resolveExpressionTarget();
  if (t) void setParam(t.channel, t.block, t.param, t.min + (t.max - t.min) * e);
}

export const savePreset = (name: string) => mutate("savePreset", { name });

export const deletePreset = (name: string) => mutate("deletePreset", { name });

export const renamePreset = (from: string, to: string) =>
  mutate("renamePreset", { from, to });

export const select = (channel: string, block: string) => {
  selected.value = { channel, block };
};

/**
 * Choose a block type in the browser for the keyboard path. `Entry` is the
 * rack/picker row; the payload is what a drop would have carried.
 */
export function selectBlockType(entry: BlockPayload | null): void {
  selectedBlockType.value = entry;
}
// --- Model browsing -----------------------------------------------------
// Search, facet filtering, sorting and paging all run **in the engine**
// over the cached index (docs/MODEL-BROWSE-AND-CACHE.md §3, §4, §6, §10);
// the UI holds only the current page, the facet choices and the transient
// download state. Commands are the frozen wire names; payload types live in
// `./model`.

import { type ModelPage, type ModelRecord, type Sort, type Kind, type CabIr, type Format, type LocalState, type ModelState, type Facets, PAGE_SIZE } from "./model";

/** Stale threshold from spec §8 (`PEDAL_INDEX_STALE_DAYS`). */
const STALE_MS = 7 * 86400000;

/** Whether the model browser is open. */
export const modelsOpen = signal(false);
/** Browse (primary action: Download) or picker (primary action: Use in NAM block). */
export const modelsMode = signal<"browse" | "picker">("browse");
/** The NAM channel the picker targets; null when not in picker mode. */
export const modelsPickerChannel = signal<string | null>(null);
/** Current search text. */
export const modelsQuery = signal("");
/** Active facet selections. `[]` in every list means "all" (engine drops it). */
export interface ActiveFacets {
  kinds: Kind[];
  manufacturers: string[];
  cabIr: CabIr[];
  tags: string[];
  formats: Format[];
  states: ModelState[];
}
export const emptyFacets = (): ActiveFacets => ({
  kinds: [],
  manufacturers: [],
  cabIr: [],
  tags: [],
  formats: [],
  states: [],
});
export const modelsSelectedFacets = signal<ActiveFacets>(emptyFacets());
/** Sort order (wire values frozen by spec §6). */
export const modelsSort = signal<Sort>("downloads");
/** List vs grid. */
export const modelsView = signal<"list" | "grid">("list");
/** Current highest loaded page (1-based) for the *active* query; grid appends. */
export const modelsPage = signal(1);
/** Server page size. */
export const modelsPageSize = signal<number>(PAGE_SIZE);
/** Loaded records for the current query (appended across pages). */
export const modelsResults = signal<ModelRecord[]>([]);
/** Total match count for the current query (before paging). */
export const modelsTotal = signal(0);
/** Facets for the current result set, computed by the engine. */
export const modelsFacets = signal<Facets>({});
/** In-flight listModels request. */
export const modelsBusy = signal(false);
/** Refresh in flight. */
export const modelsRefreshing = signal(false);
/** Last refresh error (shown inline; a failed refresh never clears the index). */
export const modelsRefreshError = signal<string | null>(null);
/** Metadata from the index the engine is serving. */
export const modelsIndexMeta = signal<{ generated_at: string; count: number } | null>(null);
/** Whether the cached index is older than the stale threshold. */
export const modelsStale = signal(false);
/** Detail record being inspected. */
export const modelsDetailId = signal<string | null>(null);
export const modelsDetail = signal<ModelRecord | null>(null);
/** Successful download results by id, for instant row/detail updates. */
export const modelLocal = signal<Record<string, LocalState | null>>({});
/** In-flight download ids. */
export const modelsDownloading = signal<Record<string, boolean>>({});
/** Per-id download errors (spec §8: per-row error + Retry). */
export const modelsDownloadError = signal<Record<string, string | null>>({});

/** True when more matches exist beyond the loaded pages. */
export function modelsHasMore(): boolean {
  return modelsTotal.value > modelsResults.value.length;
}

function listArgs(page: number): Record<string, unknown> {
  const f = modelsSelectedFacets.value;
  return {
    query: modelsQuery.value.trim() || undefined,
    kinds: f.kinds,
    manufacturers: f.manufacturers,
    cabIr: f.cabIr,
    tags: f.tags,
    formats: f.formats,
    states: f.states.filter((s) => s !== "all"),
    sort: modelsSort.value,
    page,
    pageSize: modelsPageSize.value,
  };
}

/** Fetch one page; replaces the loaded list. Idempotent while busy. */
export async function requestModels(): Promise<void> {
  if (modelsBusy.value) return;
  modelsBusy.value = true;
  try {
    const res = (await bridge.request("listModels", listArgs(modelsPage.value))) as ModelPage;
    modelsResults.value = res.models ?? [];
    modelsTotal.value = res.total ?? 0;
    modelsFacets.value = res.facets ?? {};
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg !== "engine disconnected") notice.value = msg;
  } finally {
    modelsBusy.value = false;
  }
}

/** Fetch the *next* page and append it — grid scrolling, not a new query. */
export async function appendModelsPage(): Promise<void> {
  if (modelsBusy.value || !modelsHasMore()) return;
  modelsBusy.value = true;
  const next = modelsPage.value + 1;
  try {
    const res = (await bridge.request("listModels", listArgs(next))) as ModelPage;
    if (next !== modelsPage.value + 1) return; // query changed mid-flight: drop
    modelsPage.value = next;
    modelsResults.value = [...modelsResults.value, ...(res.models ?? [])];
    modelsTotal.value = res.total ?? modelsTotal.value;
    modelsFacets.value = res.facets ?? modelsFacets.value;
  } catch {
    // keep what is loaded; scrolling can retry
  } finally {
    modelsBusy.value = false;
  }
}

/** Pull one record by index id (detail pane). */
export async function requestModel(id: string): Promise<void> {
  try {
    const rec = (await bridge.request("getModel", { id })) as ModelRecord | null;
    modelsDetail.value = rec ?? null;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg !== "engine disconnected") notice.value = msg;
  }
}

function indexIsStale(generatedAt: string): boolean {
  const t = Date.parse(generatedAt);
  return Number.isFinite(t) && Date.now() - t > STALE_MS;
}

/**
 * Seed the index metadata by re-reading the cached file. Spec §10 keeps
 * `refreshIndex` disk-backed in the engine, so this doubles as the staleness
 * probe: the UI cannot derive `generated_at` from a `listModels` page, and the
 * stale banner (§8) needs it. Errors are swallowed here — the empty-state and
 * manual-refresh paths surface them properly.
 */
export async function seedIndexMeta(): Promise<void> {
  try {
    const meta = (await bridge.request("refreshIndex", {})) as { generated_at: string; count: number };
    modelsIndexMeta.value = meta;
    modelsStale.value = indexIsStale(meta.generated_at);
  } catch {
    /* no index yet, or engine restarting: the results area says so */
  }
}

/** Manual refresh (§8): re-read the cached index; never blocks the list. */
export async function refreshIndex(): Promise<{ generated_at: string; count: number } | null> {
  if (modelsRefreshing.value) return modelsIndexMeta.value;
  modelsRefreshing.value = true;
  modelsRefreshError.value = null;
  try {
    const meta = (await bridge.request("refreshIndex", {})) as { generated_at: string; count: number };
    modelsIndexMeta.value = meta;
    modelsStale.value = false;
    modelsPage.value = 1;
    void requestModels();
    return meta;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    modelsRefreshError.value = msg;
    notice.value = `Couldn't refresh — still showing the cached index (${msg}).`;
    return null;
  } finally {
    modelsRefreshing.value = false;
  }
}

/** Hand a model to the engine's download queue. Resolves with the LocalState. */
export async function downloadModel(id: string): Promise<LocalState | null> {
  if (modelsDownloading.value[id]) return modelLocal.value[id] ?? null;
  modelsDownloading.value = { ...modelsDownloading.value, [id]: true };
  modelsDownloadError.value = { ...modelsDownloadError.value, [id]: null };
  try {
    const res = (await bridge.request("downloadModel", { id })) as {
      id: string;
      local_path: string | null;
      local: LocalState | null;
    };
    const local = res.local;
    modelLocal.value = { ...modelLocal.value, [id]: local };
    // Patch the loaded rows/detail in place: a download changes only its own
    // record's local block (§9.4), so no re-paging round-trip is needed.
    const patch = (r: ModelRecord): ModelRecord =>
      r.id === id ? { ...r, local, local_path: res.local_path ?? r.local_path } : r;
    modelsResults.value = modelsResults.value.map(patch);
    if (modelsDetail.value) modelsDetail.value = patch(modelsDetail.value);
    return local;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    modelsDownloadError.value = { ...modelsDownloadError.value, [id]: msg };
    return null;
  } finally {
    modelsDownloading.value = { ...modelsDownloading.value, [id]: false };
  }
}

/** v1 engine: downloads finish or fail; the full cancel/queue is a later card. */
export function cancelDownload(id: string): void {
  void bridge.request("cancelDownload", { id }).catch(() => undefined);
}

// --- Reactivity: search / facets / sort re-query the engine (debounced 150 ms,
// spec §3) for as long as the browser is open.

let modelsWatch: (() => void) | null = null;
let modelsDebounce: ReturnType<typeof setTimeout> | null = null;

function startModelsWatch(): void {
  if (modelsWatch) return;
  let first = true;
  modelsWatch = effect(() => {
    if (!modelsOpen.value) return;
    void modelsQuery.value;
    void modelsSort.value;
    void modelsSelectedFacets.value;
    if (first) {
      first = false;
      return;
    }
    modelsPage.value = 1;
    if (modelsDebounce) clearTimeout(modelsDebounce);
    modelsDebounce = setTimeout(() => {
      modelsDebounce = null;
      void requestModels();
    }, 150);
  });
}

function stopModelsWatch(): void {
  if (modelsDebounce) {
    clearTimeout(modelsDebounce);
    modelsDebounce = null;
  }
  modelsWatch?.();
  modelsWatch = null;
}

/** Default view: grid on a coarse pointer (touch), list otherwise (spec §5). */
function defaultView(): "list" | "grid" {
  return typeof window !== "undefined" && window.matchMedia?.("(pointer: coarse)").matches
    ? "grid"
    : "list";
}

/** Open the model browser (browse mode). Resets query, facets and paging. */
export function openModels(): void {
  modelsOpen.value = true;
  modelsMode.value = "browse";
  modelsPickerChannel.value = null;
  modelsQuery.value = "";
  modelsSelectedFacets.value = emptyFacets();
  modelsSort.value = "downloads";
  modelsView.value = defaultView();
  modelsPage.value = 1;
  modelsDetailId.value = null;
  modelsDetail.value = null;
  modelsRefreshError.value = null;
  modelsBusy.value = false;
  void requestModels();
  void seedIndexMeta();
  startModelsWatch();
}

/** Open the browser in picker mode, targeting `channel`. */
export function openModelsPicker(channel: string): void {
  modelsOpen.value = true;
  modelsMode.value = "picker";
  modelsPickerChannel.value = channel;
  modelsQuery.value = "";
  modelsSelectedFacets.value = emptyFacets();
  modelsSort.value = "downloads";
  modelsView.value = "list";
  modelsPage.value = 1;
  modelsDetailId.value = null;
  modelsDetail.value = null;
  modelsRefreshError.value = null;
  modelsBusy.value = false;
  void requestModels();
  void seedIndexMeta();
  startModelsWatch();
}

/** Close the gain-staging modal. */
export function closeGainStaging(): void {
  gainStagingOpen.value = false;
}

/** Open the gain-staging modal. */
export function openGainStaging(): void {
  gainStagingOpen.value = true;
}

/** Close the model browser. */
export function closeModels(): void {
  modelsOpen.value = false;
  modelsPickerChannel.value = null;
  modelsRefreshError.value = null;
  stopModelsWatch();
}
