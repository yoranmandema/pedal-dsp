// Model browser: search, faceted filter, sort, paginate and download NAM/IR
// models straight from the local index (read-only; the engine answers over IPC).
//
// Two modes:
//   browse — primary action is Download (the browser overlay).
//   picker — opened from a channel; primary action is "Use in NAM block".
//
// Reads only; never touches the network. The engine answers every command.

import { useEffect, useRef, useState } from "preact/hooks";
import {
  addBlock,
  appendModelsPage,
  emptyFacets,
  modelsHasMore,
  modelsDownloading,
  modelsDownloadError,
  closeModels,
  downloadModel,
  modelsBusy,
  modelsDetail,
  modelsDetailId,
  modelsFacets,
  modelsIndexMeta,
  modelsMode,
  modelsOpen,
  modelsPickerChannel,
  modelsQuery,
  modelsRefreshError,
  modelsRefreshing,
  modelsResults,
  modelsStale,
  modelsSort,
  modelsSelectedFacets,
  modelsTotal,
  modelsView,
  refreshIndex,
  requestModel,
} from "../state";
import {
  authorName,
  authorText,
  formatBytes,
  isDownloaded,
  isStale,
  topTags,
} from "../model";
import { emptyNote, modalBackdrop, modalHead, modalTitle, navBtn, textField } from "../ui";
import type { ModelRecord, Sort } from "../model";
import type { ActiveFacets } from "../state";

// --- Small shared pieces ---------------------------------------------------

/** Escape arbitrary text for safe insertion into HTML (markup is added later). */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

/** Wrap each matched token in <mark> so the user sees why a row matched. */
function highlight(text: string, query: string): string {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!tokens.length) return escapeHtml(text);
  let html = escapeHtml(text);
  for (const token of tokens) {
    const re = new RegExp(`(${escapeHtml(token)})`, "gi");
    html = html.replace(re, "<mark>$1</mark>");
  }
  return html;
}

function kindGlyph(kind: string): string {
  const cls: Record<string, string> = {
    amp: "bg-amber",
    pedal: "bg-green",
    outboard: "bg-blue",
    ir: "bg-indigo",
    fullrig: "bg-violet",
    other: "bg-slate",
  };
  return cls[kind] ?? cls.other;
}

/** Thumbnail, lazy-loaded; a per-kind glyph is used when absent. */
function Thumbnail({ record, size }: { record: ModelRecord; size: "sm" | "lg" }) {
  const [broken, setBroken] = useState(false);
  const src = broken ? null : record.metadata.thumbnail;
  const cls = size === "sm" ? "w-10 h-10" : "w-40 h-40";
  return (
    <div class="flex shrink-0 justify-center">
      {src ? (
        <img
          class={`${cls} rounded-lg bg-inset object-cover`}
          src={src}
          alt=""
          loading="lazy"
          onError={() => setBroken(true)}
        />
      ) : (
        <div
          class={`${cls} rounded-lg ${kindGlyph(record.metadata.kind)} flex items-center justify-center text-white text-xs font-semibold`}
          title="No thumbnail"
        >
          {record.metadata.kind.toUpperCase().slice(0, 2)}
        </div>
      )}
    </div>
  );
}

// --- Detail pane ---------------------------------------------------------

/** The right-hand (or sheet) view of a single model: every metadata field. */
function ModelDetail({
  record,
  mode,
  onDownload,
  onUse,
}: {
  record: ModelRecord;
  mode: "browse" | "picker";
  onDownload: () => void;
  onUse: () => void;
}) {
  const local = record.local;
  const downloaded = isDownloaded(record);
  const stale = isStale(record);
  const [descOpen, setDescOpen] = useState(false);
  const description = record.metadata.description ?? "";
  const downloading = !!modelsDownloading.value[record.id];
  const dlError = modelsDownloadError.value[record.id] ?? null;

  return (
    <div class="detail flex flex-col gap-3 p-4">
      <div class="flex items-start gap-3">
        <Thumbnail record={record} size="lg" />
        <div class="flex-1 min-w-0">
          <h2 class="text-sm font-semibold text-text leading-snug">{record.metadata.name}</h2>
          <p class="text-xs text-muted mt-0.5">
            by {authorName(record)} · {record.metadata.kind} {record.metadata.format} · {record.metadata.variant}
          </p>
          <div class="flex flex-wrap gap-2 mt-2">
            {record.metadata.stats && (
              <span class="text-[10px] text-muted">
                ⬇ {record.metadata.stats.downloads.toLocaleString()} ⭐ {record.metadata.stats.favorites.toLocaleString()}
              </span>
            )}
            {downloaded ? (
              <span class={`text-[10px] font-semibold ${stale ? "text-amber" : "text-ok"}`}>
                ✓ {stale ? "update available" : "downloaded"}
              </span>
            ) : (
              <span class="text-[10px] text-muted">not downloaded</span>
            )}
            {stale && <span class="text-[10px] text-amber">↻ update</span>}
          </div>
        </div>
        {mode === "picker" && (
          <button
            class="mini px-3 py-2 rounded-md border text-sm font-semibold bg-[#1a1b21] text-muted hover:text-text border-line"
            onClick={onUse}
            disabled={!downloaded}
            title="Use in the NAM block"
          >
            {downloaded ? "Use in NAM block" : "Download first"}
          </button>
        )}
      </div>

      <dl class="grid grid-cols-2 gap-2 text-sm">
        {record.metadata.manufacturer && (
          <div>
            <dt class="text-[10px] uppercase tracking-[0.6px] text-muted">Manufacturer</dt>
            <dd class="text-text">{record.metadata.manufacturer}</dd>
          </div>
        )}
        {record.metadata.gear_model && (
          <div>
            <dt class="text-[10px] uppercase tracking-[0.6px] text-muted">Gear model</dt>
            <dd class="text-text">{record.metadata.gear_model}</dd>
          </div>
        )}
        {record.metadata.cab && (
          <div>
            <dt class="text-[10px] uppercase tracking-[0.6px] text-muted">Cabinet</dt>
            <dd class="text-text">{record.metadata.cab}</dd>
          </div>
        )}
        {record.metadata.mics && (
          <div>
            <dt class="text-[10px] uppercase tracking-[0.6px] text-muted">Mics</dt>
            <dd class="text-text">{record.metadata.mics}</dd>
          </div>
        )}
        {record.metadata.license && (
          <div>
            <dt class="text-[10px] uppercase tracking-[0.6px] text-muted">License</dt>
            <dd class="text-text">{record.metadata.license}</dd>
          </div>
        )}
        {record.metadata.sample_rate && (
          <div>
            <dt class="text-[10px] uppercase tracking-[0.6px] text-muted">Sample rate</dt>
            <dd class="text-text">{(record.metadata.sample_rate >= 1000 ? record.metadata.sample_rate / 1000 : record.metadata.sample_rate).toString()} kHz</dd>
          </div>
        )}
        {record.metadata.loudness_db != null && (
          <div>
            <dt class="text-[10px] uppercase tracking-[0.6px] text-muted">Loudness</dt>
            <dd class="text-text">{record.metadata.loudness_db.toFixed(2)} LUFS</dd>
          </div>
        )}
        {record.metadata.gain_db != null && (
          <div>
            <dt class="text-[10px] uppercase tracking-[0.6px] text-muted">Gain</dt>
            <dd class="text-text">{record.metadata.gain_db.toFixed(1)} dB</dd>
          </div>
        )}
      </dl>

      {record.metadata.tags.length > 0 && (
        <div class="text-sm">
          <dt class="text-[10px] uppercase tracking-[0.6px] text-muted mb-1">Tags</dt>
          <div class="flex flex-wrap gap-1.5">
            {record.metadata.tags.map((t) => (
              <span key={t} class="text-[10px] px-2 py-1 rounded-full bg-inset text-muted border border-line">
                #{t}
              </span>
            ))}
          </div>
        </div>
      )}

      {record.metadata.links.length > 0 && (
        <div class="text-sm">
          <dt class="text-[10px] uppercase tracking-[0.6px] text-muted mb-1">Links</dt>
          <div class="flex flex-wrap gap-2">
            {record.metadata.links.map((l) => (
              <a key={l} href={l} target="_blank" rel="noreferrer" class="text-xs text-amber hover:underline">
                {l}
              </a>
            ))}
          </div>
        </div>
      )}

      {downloaded && local && (
        <div class="text-sm bg-inset rounded-md p-3 border border-line">
          <dt class="text-[10px] uppercase tracking-[0.6px] text-muted mb-1">Local file</dt>
          <p class="text-xs text-text font-mono truncate">{local.file_name}</p>
          <p class="text-[10px] text-muted font-mono break-all">
            {formatBytes(local.size_bytes)} · {local.checksum} · {new Date(local.downloaded_at).toLocaleString()}
          </p>
          {stale && <p class="text-[10px] text-amber mt-1">Update available — re-download when ready.</p>}
        </div>
      )}

      {description && (
        <div class="text-sm">
          <dt class="text-[10px] uppercase tracking-[0.6px] text-muted mb-1">Description</dt>
          <p
            class="text-text leading-relaxed"
            dangerouslySetInnerHTML={{ __html: descOpen ? description : description.slice(0, 220) + (description.length > 220 ? "…" : "") }}
          />
          {description.length > 220 && (
            <button class="text-xs text-amber mt-1" onClick={() => setDescOpen(true)}>
              Show more
            </button>
          )}
        </div>
      )}

      <div class="sticky bottom-0 -mx-4 -mb-4 px-4 py-3 bg-panel border-t border-line-soft flex flex-col gap-2">
        {dlError && (
          <p class="text-xs text-danger-text bg-danger-bg border border-danger-line rounded-md px-2 py-1.5" role="alert">
            Download failed: {dlError}
          </p>
        )}
        <div class="flex gap-2">
        <button
          class={`flex-1 min-h-[var(--ui-touch)] px-3.5 py-2 rounded-md text-sm font-semibold border ${downloaded && !stale && !dlError ? "bg-surface text-muted border-line" : "bg-linear-to-b from-amber-hi to-amber-lo border-amber-edge text-amber-ink"}`}
          onClick={onDownload}
          disabled={downloading}
        >
          {downloading ? "Downloading…" : dlError ? "Retry download" : !downloaded ? "Download" : stale ? "Update — download again" : "Downloaded ✓ (re-download)"}
        </button>
        {mode === "picker" && (
          <button
            class="mini px-3 py-2 rounded-md border text-xs font-semibold bg-[#1a1b21] text-muted border-line hover:text-text"
            onClick={onUse}
            disabled={!downloaded}
          >
            Use in NAM block
          </button>
        )}
        </div>
      </div>
    </div>
  );
}

// --- Main component ------------------------------------------------------

const ROW_H = 64;
const OVERSCAN = 6;

export function ModelBrowser() {
  const listScroller = useRef<HTMLDivElement>(null);
  const gridScroller = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewH, setViewH] = useState(400);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [focusId, setFocusId] = useState<string | null>(null);

  const open = modelsOpen.value;
  const results = modelsResults.value;
  const total = modelsTotal.value;
  const selected = modelsSelectedFacets.value;
  const mode = modelsMode.value;
  const query = modelsQuery.value;
  const pickerChannel = modelsPickerChannel.value;
  const detailId = modelsDetailId.value;
  const detail = modelsDetail.value;
  const indexMeta = modelsIndexMeta.value;
  const refreshing = modelsRefreshing.value;
  const busy = modelsBusy.value;
  const view = modelsView.value;
  const refreshError = modelsRefreshError.value;
  const stale = modelsStale.value;
  const hasMore = modelsHasMore();
  const activeFilters = countActiveFilters(selected);

  const closeDetail = () => {
    modelsDetailId.value = null;
    modelsDetail.value = null;
  };
  const openDetail = (id: string) => {
    setFocusId(id);
    modelsDetailId.value = id;
    void requestModel(id);
  };

  // Measure the scroll viewport (list or grid) so virtualization has a real height.
  const scroller = view === "list" ? listScroller : gridScroller;
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    setViewH(el.clientHeight || 400);
    const ro = new ResizeObserver(() => setViewH(el.clientHeight || 400));
    ro.observe(el);
    return () => ro.disconnect();
  }, [open, view]);

  // A new query returns to the top.
  useEffect(() => {
    if (listScroller.current) listScroller.current.scrollTop = 0;
    if (gridScroller.current) gridScroller.current.scrollTop = 0;
    setScrollTop(0);
  }, [query, modelsSort.value, selected]);

  // Keep the first result focusable for keyboard users.
  useEffect(() => {
    if (focusId && !results.some((r) => r.id === focusId)) setFocusId(null);
  }, [results]);

  // Load the next page when the user nears the end of what's loaded.
  useEffect(() => {
    if (!open || busy || !hasMore) return;
    const el = scroller.current;
    if (!el) return;
    const bottom = view === "list" ? scrollTop + viewH : el.scrollTop + el.clientHeight;
    const content = view === "list" ? results.length * ROW_H : el.scrollHeight;
    if (bottom >= content - 400) void appendModelsPage();
  }, [open, scrollTop, viewH, results.length, hasMore, busy, view]);

  // Virtualized list: only the visible window is in the DOM.
  const listStart = Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN);
  const listEnd = Math.min(results.length, Math.ceil((scrollTop + viewH) / ROW_H) + OVERSCAN);
  const visible = results.slice(listStart, listEnd);

  const moveFocus = (delta: number) => {
    const idx = focusId ? results.findIndex((r) => r.id === focusId) : -1;
    const next = Math.min(results.length - 1, Math.max(0, idx + delta));
    if (next < 0 || !results.length) return;
    setFocusId(results[next].id);
    const el = listScroller.current;
    if (el && view === "list") {
      const top = next * ROW_H;
      if (top < el.scrollTop) el.scrollTop = top;
      else if (top + ROW_H > el.scrollTop + el.clientHeight) el.scrollTop = top + ROW_H - el.clientHeight;
    }
  };

  // Keyboard: `/` search, ↑/↓ move, Enter open, `d` download, `u` use, Esc back/close.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const typing = target.tagName === "INPUT" || target.tagName === "TEXTAREA";
      const k = e.key;
      if (k === "Escape") {
        e.preventDefault();
        if (typing && query) modelsQuery.value = "";
        else if (detailId) closeDetail();
        else closeModels();
      } else if (k === "ArrowDown" || k === "ArrowUp") {
        if (target.tagName === "SELECT") return;
        e.preventDefault();
        if (typing) (target as HTMLElement).blur();
        moveFocus(k === "ArrowDown" ? 1 : -1);
      } else if (typing || target.tagName === "SELECT") {
        return;
      } else if (k === "/") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (k === "Enter" && focusId && target.tagName !== "BUTTON") {
        e.preventDefault();
        openDetail(focusId);
      } else if (k === "d" && detailId) {
        e.preventDefault();
        void downloadModel(detailId);
      } else if (k === "u" && detailId && mode === "picker") {
        e.preventDefault();
        void useModel(detailId);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // Focus the search box when the browser opens.
  useEffect(() => {
    if (open) searchRef.current?.focus();
  }, [open]);

  const useModel = async (id: string) => {
    const channel = pickerChannel;
    if (!channel) return;
    const rec = detail ?? results.find((r) => r.id === id);
    if (!rec) return;
    await addBlock(channel, rec.metadata.kind, rec.metadata.name);
  };

  if (!open) return null;

  const empty = !busy && results.length === 0;
  const noIndex = empty && !indexMeta && !query && !activeFilters;

  return (
    <div
      class={`${modalBackdrop} fixed inset-0 z-50 grid place-items-center bg-[rgba(6,7,10,0.7)]`}
      onClick={() => closeModels()}
    >
      <div
        class="flex flex-col overflow-hidden rounded-xl border border-[#343743] bg-linear-to-b from-[#1e2027] to-well shadow-modal w-[min(980px,calc(100vw-24px))] h-[min(720px,calc(100vh-48px))]"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="Model browser"
      >
        <header class={`${modalHead} flex-shrink-0`}>
          <h2 class={`${modalTitle} flex-shrink-0`}>{mode === "picker" ? "Choose a model" : "Models"}</h2>
          <button class={navBtn} onClick={() => closeModels()} title="Close (Esc)" aria-label="Close">
            ×
          </button>
        </header>

        <div class="flex items-center gap-2 border-b border-line-soft px-3.5 py-2.5 flex-shrink-0">
          <input
            ref={searchRef}
            class={`${textField} px-2.5 py-1.5 flex-1 min-w-0`}
            type="search"
            value={query}
            placeholder="Search name, author, tag…  ( / )"
            onInput={(e) => (modelsQuery.value = (e.target as HTMLInputElement).value)}
            aria-label="Search models"
          />
          <span class="text-xs text-muted shrink-0 tabular-nums" aria-live="polite">
            {refreshing ? "Refreshing…" : `${total}${indexMeta ? ` of ${indexMeta.count}` : ""}`}
          </span>
        </div>

        {stale && indexMeta && !refreshing && (
          <div class="flex items-center gap-2 px-3.5 py-2 bg-amber-bg border-b border-amber-line text-amber-text text-xs flex-shrink-0" role="status">
            <span>Index is {Math.floor((Date.now() - Date.parse(indexMeta.generated_at)) / 86400000)} days old.</span>
            <button class="font-semibold underline" onClick={() => void refreshIndex()}>Refresh</button>
          </div>
        )}
        {refreshError && (
          <div class="flex items-center gap-2 px-3.5 py-2 bg-danger-bg border-b border-danger-line text-danger-text text-xs flex-shrink-0" role="alert">
            <span class="min-w-0 flex-1 truncate">Refresh failed: {refreshError}. Showing the cached index.</span>
            <button class="font-semibold underline" onClick={() => void refreshIndex()}>Retry</button>
          </div>
        )}

        <div class="flex flex-wrap items-center gap-2 px-3.5 py-2 border-b border-line-soft flex-shrink-0">
          <button
            class={`px-2.5 py-1.5 rounded-md text-xs font-semibold border ${filtersOpen || activeFilters ? "bg-raised text-text border-line-hi" : "bg-surface text-muted border-line"}`}
            onClick={() => setFiltersOpen((v) => !v)}
            aria-expanded={filtersOpen}
            aria-controls="filters-panel"
          >
            Filters{activeFilters ? ` (${activeFilters})` : ""}
          </button>
          <label class="flex items-center gap-1.5 text-xs text-muted">
            <span class="uppercase tracking-[0.6px]">Sort</span>
            <select
              class={`${textField} px-1.5 py-1 text-xs`}
              value={modelsSort.value}
              onChange={(e) => (modelsSort.value = (e.target as HTMLSelectElement).value as Sort)}
            >
              {query.trim() && <option value="relevance">Best match</option>}
              <option value="downloads">Most downloaded</option>
              <option value="favorites">Most favorited</option>
              <option value="name">Name A→Z</option>
              <option value="author">Author A→Z</option>
              <option value="newest">Newest</option>
              <option value="updated">Recently updated</option>
              <option value="added">Recently added</option>
            </select>
          </label>
          <div class="flex" role="group" aria-label="View">
            {(["list", "grid"] as const).map((v) => (
              <button
                key={v}
                class={`px-2.5 py-1.5 text-xs font-semibold border first:rounded-l-md last:rounded-r-md ${view === v ? "bg-raised text-text border-line-hi" : "bg-surface text-muted border-line"}`}
                aria-pressed={view === v}
                onClick={() => (modelsView.value = v)}
              >
                {v === "list" ? "List" : "Grid"}
              </button>
            ))}
          </div>
          <button
            class={`ml-auto px-2.5 py-1.5 rounded-md border border-line bg-surface text-xs font-semibold ${refreshing ? "text-amber" : "text-muted hover:text-text"}`}
            onClick={() => void refreshIndex()}
            disabled={refreshing}
            title="Re-read the local model index"
            aria-label="Refresh index"
          >
            {refreshing ? "Refreshing…" : "⟳ Refresh"}
          </button>
        </div>

        {activeFilters > 0 && (
          <div class="flex flex-wrap items-center gap-1.5 px-3.5 py-2 border-b border-line-soft flex-shrink-0">
            {(["kinds", "manufacturers", "cabIr", "tags", "formats", "states"] as FacetField[]).flatMap((f) =>
              (selected[f] as string[]).map((v) => <Chip key={f + v} label={v} onOff={() => toggleFacet(f, v)} />),
            )}
            <button class="text-muted hover:text-text text-xs" onClick={clearFilters}>Clear all</button>
          </div>
        )}

        {filtersOpen && (
          <div id="filters-panel" class="max-h-[40%] overflow-y-auto border-b border-line-soft px-3.5 py-3 flex flex-col gap-3 flex-shrink-0">
            {facetGroups().filter((g) => g.values.length > 0).map((g) => (
              <div key={g.label}>
                <div class="text-[10px] uppercase tracking-[0.6px] text-muted mb-1">{g.label}</div>
                <div class="flex flex-wrap gap-1.5">
                  {g.values.map((v) => {
                    const on = (selected[g.field] as string[]).includes(v.value);
                    return (
                      <button
                        key={v.value}
                        class={`px-2 py-1 rounded-full border text-xs ${on ? "bg-amber-bg border-amber-line text-amber-text" : "bg-surface border-line text-muted hover:text-text"}`}
                        aria-pressed={on}
                        onClick={() => toggleFacet(g.field, v.value)}
                      >
                        {v.value} <span class="text-[10px] opacity-70">{v.count}</span>
                      </button>
                    );
                  })}
                </div>
              </div>
            ))}
            {activeFilters > 0 && (
              <button class="self-start text-xs text-amber" onClick={clearFilters}>Clear all filters</button>
            )}
          </div>
        )}

        <div class="relative flex flex-1 min-h-0">
          <div class="relative flex-1 min-w-0 min-h-0">
            {empty && (
              <div class="absolute inset-0 grid place-items-center p-6 text-center" role="status">
                {refreshing ? (
                  <p class="text-sm text-muted">Refreshing index…</p>
                ) : noIndex ? (
                  <div>
                    <p class={emptyNote}>No model index yet.</p>
                    <button class="mt-2 text-sm font-semibold text-amber" onClick={() => void refreshIndex()}>Load the model index</button>
                  </div>
                ) : (
                  <div>
                    <p class={emptyNote}>No models match{query ? ` “${query}”` : ""}.</p>
                    {(query || activeFilters > 0) && (
                      <button
                        class="mt-2 text-sm font-semibold text-amber"
                        onClick={() => { modelsQuery.value = ""; clearFilters(); }}
                      >
                        Clear search and filters
                      </button>
                    )}
                  </div>
                )}
              </div>
            )}

            {view === "list" ? (
              <div
                ref={listScroller}
                class="absolute inset-0 overflow-y-auto"
                onScroll={(e) => setScrollTop((e.target as HTMLElement).scrollTop)}
                role="listbox"
                aria-label="Models"
              >
                <div class="relative" style={{ height: results.length * ROW_H }}>
                  {visible.map((r, i) => (
                    <div key={r.id} class="absolute left-0 right-0" style={{ top: (listStart + i) * ROW_H, height: ROW_H }}>
                      <Row
                        record={r}
                        highlightQuery={query}
                        isFocused={focusId === r.id}
                        isOpen={detailId === r.id}
                        onSelect={() => openDetail(r.id)}
                      />
                    </div>
                  ))}
                </div>
              </div>
            ) : (
              <div
                ref={gridScroller}
                class="absolute inset-0 overflow-y-auto p-3 grid gap-2 content-start grid-cols-[repeat(auto-fill,minmax(180px,1fr))]"
                onScroll={(e) => setScrollTop((e.target as HTMLElement).scrollTop)}
              >
                {results.map((r) => (
                  <ModelCard key={r.id} record={r} isFocused={focusId === r.id} onSelect={() => openDetail(r.id)} />
                ))}
              </div>
            )}
          </div>

          {/* Detail: a side pane on wide screens, a full sheet on narrow ones. */}
          <div
            class={`${detailId ? "block" : "hidden desk:block"} absolute inset-0 z-10 bg-panel desk:static desk:inset-auto desk:z-auto desk:w-[400px] desk:shrink-0 desk:border-l border-line overflow-y-auto`}
            aria-label="Model detail"
          >
            {detail && detailId ? (
              <>
                <div class="flex items-center justify-between px-3 py-2 border-b border-line-soft desk:hidden">
                  <button class="text-sm text-text" onClick={closeDetail}>‹ Back</button>
                </div>
                <ModelDetail
                  record={detail}
                  mode={mode}
                  onDownload={() => void downloadModel(detail.id)}
                  onUse={() => void useModel(detail.id)}
                />
              </>
            ) : (
              <div class="p-6 text-center text-sm text-muted">Select a model to inspect it.</div>
            )}
          </div>
        </div>

        <div class="flex items-center justify-between px-3.5 py-2 border-t border-line-soft flex-shrink-0 text-[10px] text-muted">
          <span>↑↓ move · Enter open · d download · / search · Esc back</span>
          {busy && <span>Loading…</span>}
        </div>
      </div>
    </div>
  );
}

// --- Supporting components + helpers ---------------------------------------

type FacetField = "kinds" | "manufacturers" | "cabIr" | "tags" | "formats" | "states";

interface FacetGroup {
  label: string;
  field: FacetField;
  values: Array<{ value: string; count: number }>;
}

function facetGroups(): FacetGroup[] {
  const f = modelsFacets.value;
  return [
    { label: "Kind", field: "kinds", values: f.kind ?? [] },
    { label: "Manufacturer", field: "manufacturers", values: f.manufacturer ?? [] },
    { label: "Cab / IR", field: "cabIr", values: f.cabIr ?? [] },
    { label: "Tags", field: "tags", values: f.tags ?? [] },
    { label: "Format", field: "formats", values: f.format ?? [] },
    { label: "Download state", field: "states", values: (f.state ?? []).filter((v) => v.value !== "all") },
  ];
}

function countActiveFilters(s: ActiveFacets): number {
  return s.kinds.length + s.manufacturers.length + s.cabIr.length + s.tags.length + s.formats.length + s.states.length;
}

function toggleFacet(field: FacetField, value: string) {
  const selected = modelsSelectedFacets.value;
  const current = selected[field] as string[];
  const next = current.includes(value) ? current.filter((v) => v !== value) : [...current, value];
  modelsSelectedFacets.value = { ...selected, [field]: next };
}

function clearFilters() {
  modelsSelectedFacets.value = emptyFacets();
}

function Chip({ label, onOff }: { label: string; onOff: () => void }) {
  return (
    <button
      class="px-2.5 py-1 rounded-full border text-xs font-semibold bg-amber-bg border-amber-line text-amber-text hover:brightness-125"
      onClick={onOff}
      aria-label={`Remove filter ${label}`}
    >
      {label} ✕
    </button>
  );
}

function StateLabel({ record }: { record: ModelRecord }) {
  if (!isDownloaded(record)) return <span class="text-muted">—</span>;
  return isStale(record) ? (
    <span class="font-semibold text-amber">update</span>
  ) : (
    <span class="font-semibold text-ok">✓ downloaded</span>
  );
}

function Row({
  record,
  isFocused,
  isOpen,
  onSelect,
  highlightQuery,
}: {
  record: ModelRecord;
  isFocused: boolean;
  isOpen: boolean;
  onSelect: () => void;
  highlightQuery: string;
}) {
  return (
    <button
      class={`w-full h-full text-left border-b border-line-soft px-3 flex items-center gap-3 hover:bg-panel-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-amber ${isOpen ? "bg-raised" : isFocused ? "bg-panel-2 ring-1 ring-inset ring-amber-dim" : ""}`}
      onClick={onSelect}
      role="option"
      aria-selected={isOpen}
    >
      <Thumbnail record={record} size="sm" />
      <div class="min-w-0 flex-1">
        <div class="text-sm font-medium truncate">
          <span dangerouslySetInnerHTML={{ __html: highlight(record.metadata.name, highlightQuery) }} />
        </div>
        <div class="text-xs text-muted truncate">{authorText(record)}</div>
        <div class="text-[10px] text-muted truncate">
          {topTags(record).map((t) => (
            <span key={t} class="inline-block mr-1 rounded bg-inset px-1">#{t}</span>
          ))}
        </div>
      </div>
      <div class="shrink-0 text-right text-xs min-w-[84px]">
        <StateLabel record={record} />
      </div>
    </button>
  );
}

function ModelCard({ record, isFocused, onSelect }: { record: ModelRecord; isFocused: boolean; onSelect: () => void }) {
  return (
    <button
      class={`grid gap-2 p-2 rounded-lg border text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-amber ${isFocused ? "bg-panel-2 border-amber-dim" : "bg-inset border-transparent hover:bg-panel-2 hover:border-line"}`}
      onClick={onSelect}
    >
      <Thumbnail record={record} size="sm" />
      <div class="min-w-0">
        <div class="text-sm font-medium truncate">{record.metadata.name}</div>
        <div class="text-xs text-muted truncate">{authorName(record)}</div>
        <div class="text-xs mt-1"><StateLabel record={record} /></div>
      </div>
    </button>
  );
}
