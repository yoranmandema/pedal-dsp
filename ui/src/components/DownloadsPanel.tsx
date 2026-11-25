import { downloads, dismissDownload, cancelDownload, type DownloadItem } from "../state";


/** Format byte count to human readable. */
function fmtBytes(n: number | null): string {
  if (n === null || !Number.isFinite(n)) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** Progress bar and metadata for one download item. */
function DownloadRow({ item }: { item: DownloadItem }) {
  const pct = item.progress !== null ? Math.round(item.progress * 100) : null;
  const isDone = item.status === "completed";
  const isError = item.status === "error";
  const isCancelled = item.status === "cancelled";
  const isActive = item.status === "progress";

  return (
    <div class={`download-row flex items-center gap-3 px-3 py-2 rounded-md ${isDone ? "bg-[#1a2a1a] border border-ok-line/30" : isError ? "bg-[#2a1a1a] border border-danger-line/30" : isCancelled ? "bg-[#1a1a2a] border border-line-soft/50" : "bg-[#1a1b21] border border-line/30"}`}>
      {/* Icon / status dot */}
      <div class={`icon size-5 rounded-full grid place-items-center flex-shrink-0 ${isDone ? "text-ok bg-ok/20" : isError ? "text-danger bg-danger/20" : isActive ? "text-amber bg-amber/20" : "text-muted bg-line/20"}`}>
        {isDone ? (
          <svg class="w-3 h-3" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M4 8l3 3 5-6" /></svg>
        ) : isActive ? (
          <svg class="w-3 h-3 animate-spin" viewBox="0 0 16 16" fill="currentColor"><circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="2" stroke-dasharray="28" stroke-dashoffset="8" /></svg>
        ) : isError ? (
          <span class="text-xs">✕</span>
        ) : isCancelled ? (
          <span class="text-xs">⊘</span>
        ) : (
          <span class="text-[8px]">•</span>
        )}
      </div>

      {/* Name */}
      <div class="min-w-0 flex-1">
        <div class="download-name text-sm text-text truncate font-medium">{item.name}</div>
        {pct !== null && (
          <div class="text-[10px] text-muted tabular-nums mt-0.5">
            {pct}% · {fmtBytes(item.receivedBytes)}{item.totalBytes ? ` / ${fmtBytes(item.totalBytes)}` : ""}
          </div>
        )}
        {isError && item.error && (
          <div class="text-[10px] text-danger mt-0.5 truncate">{item.error}</div>
        )}
      </div>

      {/* Actions */}
      <div class="flex items-center gap-1.5 flex-shrink-0">
        {/* Cancel (in-progress or queued) */}
        {isActive || item.status === "queued" ? (
          <button
            class="dl-btn text-[10px] px-2 py-1 rounded-sm bg-[#1a1b21] border border-line text-muted hover:text-danger hover:border-danger/50 hover:bg-danger/10"
            onClick={() => cancelDownload(item.id)}
            title="Cancel download"
          >
            Cancel
          </button>
        ) : null}

        {/* Retry (error) */}
        {isError && (
          <button
            class="dl-btn text-[10px] px-2 py-1 rounded-sm bg-[#1a1b21] border border-line text-muted hover:text-text hover:border-line-hi"
            onClick={() => void cancelDownload(item.id)}
            title="Retry download"
          >
            Retry
          </button>
        )}

        {/* Dismiss (done, error, cancelled, paused) */}
        {(isDone || isError || isCancelled || item.status === "paused") && (
          <button
            class="dl-btn text-[10px] px-2 py-1 rounded-sm bg-[#1a1b21] border border-line text-muted hover:text-text hover:border-line-hi"
            onClick={() => dismissDownload(item.id)}
            title="Dismiss"
            aria-label="Dismiss download"
          >
            Dismiss
          </button>
        )}
      </div>
    </div>
  );
}

/** Progress bar overlay for active downloads. */
function ProgressBar({ pct }: { pct: number }) {
  return (
    <div class="dl-progress w-full h-0.5 bg-line-soft rounded-full mt-2 ml-13 overflow-hidden">
      <div
        class="h-full bg-amber rounded-full transition-[width] duration-300 ease-linear"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

/** The downloads panel — shown in the model browser when downloads are active. */
export function DownloadsPanel() {
  const activeItems = downloads.value.filter(
    (d: DownloadItem) => d.status === "progress" || d.status === "queued" || d.status === "paused"
  );
  const doneItems = downloads.value.filter(
    (d: DownloadItem) => d.status === "completed" || d.status === "error" || d.status === "cancelled"
  );
  const items = [...activeItems, ...doneItems];

  if (items.length === 0) return null;

  return (
    <div class="downloads-panel grid gap-2">
      <div class="downloads-header flex items-center justify-between px-1">
        <h3 class="text-sm font-semibold tracking-[0.5px] text-text">Downloads ({items.length})</h3>
        {doneItems.length > 0 && (
          <button
            class="text-[10px] text-muted hover:text-text underline"
            onClick={() => doneItems.forEach(d => dismissDownload(d.id))}
          >
            Dismiss all done
          </button>
        )}
      </div>

      <div class="dl-list grid gap-1.5">
        {items.map((item) => (
          <div key={item.id} class="relative">
            <DownloadRow item={item} />
            {item.status === "progress" && item.progress !== null && (
              <ProgressBar pct={Math.round(item.progress * 100)} />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
