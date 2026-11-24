import { cancelDownload, dismissDownload, downloads } from "../state";

function mb(n: number): string {
  return (n / 1048576).toFixed(1);
}

/** Floating list of model downloads with a progress bar per file. Hidden when
 *  the queue is empty. */
export function DownloadsPanel() {
  const list = downloads.value;
  if (list.length === 0) return null;
  return (
    <div
      class="downloads fixed left-4 bottom-4 z-40 grid gap-2 w-[min(320px,calc(100vw-32px))] rounded-lg border border-line bg-[#1a1b21] p-3 text-xs shadow-toast"
      role="region"
      aria-label="Downloads"
    >
      {list.map((d) => {
        const active = d.status === "queued" || d.status === "running";
        return (
          <div key={d.id} class="grid gap-1">
            <div class="flex items-center justify-between gap-2">
              <span class="truncate text-text" title={d.local_path}>
                {d.local_path.split("/").pop() ?? d.id}
              </span>
              <button
                class="text-muted hover:text-text"
                aria-label={active ? "Cancel download" : "Dismiss"}
                onClick={() => (active ? cancelDownload(d.id) : dismissDownload(d.id))}
              >
                {active ? "Cancel" : "✕"}
              </button>
            </div>
            <div class="h-1.5 rounded-full bg-[#2f323b] overflow-hidden">
              <div
                class={`h-full ${d.status === "failed" ? "bg-danger-text" : "bg-amber"}`}
                style={{ width: `${d.status === "done" ? 100 : d.percent}%` }}
              />
            </div>
            <div class="text-muted">
              {d.status === "queued" && "Queued"}
              {d.status === "running" &&
                `${d.percent.toFixed(0)}%${d.total ? ` · ${mb(d.received)} / ${mb(d.total)} MB` : ""}${d.retries ? ` · resumed ×${d.retries}` : ""}`}
              {d.status === "done" && "Downloaded"}
              {d.status === "cancelled" && "Cancelled"}
              {d.status === "failed" && `Failed: ${d.error ?? "unknown error"}`}
            </div>
          </div>
        );
      })}
    </div>
  );
}
