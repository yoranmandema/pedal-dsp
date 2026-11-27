# Model Download Flow — User Documentation

## What this flow does

The model download flow lets you browse, search, and download NAM / IR amp models from the TONE3000 API. A local *index* tracks every browsable model; downloaded files live on disk under the library root.

Three rules:
1. **Browsing is offline.** Search, filter, sort, and detail views read only the local index.
2. **The index is the only source of truth for the UI.** The app never fetches a model record on demand.
3. **Downloads are never implicit and never destructive.** A download is a confirmed user action. Refreshing the index never deletes or overwrites a cached file.

---

## Prerequisites

- The app is running (see `RUN-THE-APP.md`).
- You have a TONE3000 API account and your access token. This is configured once.

## 1. Configure credentials

1. Create or open your TONE3000 API token in the TONE3000 dashboard (or via the CLI: `tone3000 token create`).
2. Copy the token string.
3. Back up `ui/.env.example` to `ui/.env.local` and set:

   ```bash
   VITE_TONE3000_TOKEN=<your token here>
   VITE_TONE3000_API_URL=<https://api.tone3000.ai (default)>
   ```

   The app reads `ui/.env.local` at startup. Restart after editing.

## 2. Refresh the index

1. Open the **Model Browser** from the TopBar (`Models` button).
2. Click **Refresh index**.
3. The app fetches the public catalog from the API, downloads the index snapshot, and stores it locally. This usually takes a few seconds.

The index is saved at `<project>/ui/public/model-index.json`. It is a full catalog snapshot; the *local library* is a separate, smaller on-disk index of downloaded files.

## 3. Search and filter

- **Search** — one free-text field, placeholder `Search name, author, tag…`. Case-insensitive, diacritic-folded, whitespace-split. A record matches only when **every** token matches at least one field (AND across tokens). Substring matching (no word boundaries).
- **Filters** — combine as many as you like:
  - `Category` (Amp, Cab, Speaker, Noise Gate)
  - `Tone` (select a tone from the list)
  - `Format` (NAM / IR)
  - `Manufacturers` (multiselect)
  - `Tags` (multiselect)
- **Sort** — choose: `Newest`, `Oldest`, `Name` (A–Z), `Name (Z–A)`.
- Result count is always visible: **“312 of 1,204”**.
- Keys: `/` focuses search (while the browser is open), `Esc` clears, `Esc` again closes the browser.

## 4. Browse and view a model

1. Click any result to open the detail panel.
2. The detail view shows: name, author, tone, category, format, description, and a parameter preview.
3. Click **Download** to start downloading.

The **Use in NAM block** button works in picker mode — it downloads the model and uses it in the NAM block that opened the picker.

## 5. Where downloads land

- **Library root:** `<project>/models` (configurable via `VITE_MODEL_LIB_DIR` in `.env.local`).
- Each download lands at `<library root>/<tone id>/<model id>/<file name>`.
- The local library index is saved at `<project>/models/index.json`.

**Cache location and how to clear it:**

- `models/` — downloaded model files (largest)
- `ui/public/model-index.json` — the full catalog snapshot from Refresh index
- `ui/src/model-index.schema.json` — JSON schema for the index

To clear the entire cache:

```bash
rm -rf models/                                   # delete downloaded files
rm -f ui/public/model-index.json                 # delete the catalog snapshot
```

To clear only the downloaded library:

```bash
rm -rf models/
```

## 6. Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| “Invalid credentials” | Token missing or wrong | Check `VITE_TONE3000_TOKEN` in `.env.local`, restart the app |
| “Network unreachable” | No internet, or API URL is wrong | Check `VITE_TONE3000_API_URL`, check your network |
| “Rate limit exceeded” | Too many requests | Wait 60 s and retry; reduce refresh frequency |
| “Download failed” | Partial file from a previous interrupted download | Delete the partial file at `models/<tone id>/<model id>/`, or clear the library with `rm -rf models/` |
| Refresh index fails | API is down or token is expired | Check the TONE3000 status page; refresh credentials |

---

## How the flow is implemented

- **Index builder** (`engine/src/model_index.rs`) — fetches and stores the catalog index.
- **Download manager** (`engine/src/download.rs`) — queue, HTTP range/resume, sha256 verify, duplicate-avoidance, partial cleanup, progress events, index update.
- **Browsing UI** — Preact components in `ui/src/components/ModelBrowser.tsx` and `ui/src/state.ts`.
- **Server wire** — `engine/src/server.rs` exposes `downloadModel`, `cancelDownload`, `listDownloads`, plus progress events and full model records.

