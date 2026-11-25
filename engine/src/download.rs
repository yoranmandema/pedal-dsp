//! A download manager for NAM / IR model files.
//!
//! Contract: `docs/MODEL-BROWSE-AND-CACHE.md` §9.2 (layout, queue, checksum,
//! `.part` handling) and §9.3 (index record). Resume is a v1.1 extension of
//! §9.2 (HTTP `Range`).
//!
//! * A download is identified by the model's index id (`t3k:379978`) and lands
//!   at `$PEDAL_MODELS/<provider>/<tone_id>/<model_id>_<variant>.<ext>`.
//! * Bytes are staged in `<final>.part`. A dropped connection is retried with
//!   `Range: bytes=<len>-`; a server that ignores the range (200) restarts the
//!   file from zero. After the retries run out the `.part` is *kept* so a later
//!   download resumes from it.
//! * Unrecoverable failures (4xx, size/checksum mismatch, cancel) delete the
//!   `.part`.
//! * Already-downloaded (file present and sha256 matches the index) and
//!   already-queued/running ids are no-ops that report so.
//! * On success `.part` is published, the index record gets `local_path` and
//!   `local`, and `models.json` is rewritten atomically.
//! * Every state change is emitted as a [`DownloadEvent`] carrying a full
//!   [`DownloadItem`] snapshot, which the server broadcasts as `event: download`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Number, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, Semaphore};

use crate::model_index::{self, FullIndex, FullModelRecord, LocalState};

/// Max simultaneous downloads (§9.2).
const MAX_CONCURRENT: usize = 2;
/// Reconnect attempts after a dropped connection before giving up.
const MAX_RETRIES: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

/// One entry in the queue; also the wire shape of `listDownloads` items and of
/// the `download` event payload.
#[derive(Debug, Clone, Serialize)]
pub struct DownloadItem {
    pub id: String,
    pub status: DownloadStatus,
    pub received: u64,
    /// Expected total bytes; 0 while unknown.
    pub total: u64,
    /// 0..100.
    pub percent: f64,
    /// Path relative to `$PEDAL_MODELS` (POSIX).
    pub local_path: String,
    pub error: Option<String>,
    /// How many times the connection was re-established so far.
    pub retries: u32,
}

#[derive(Debug, Clone)]
pub struct DownloadEvent {
    pub item: DownloadItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnqueueOutcome {
    Queued,
    AlreadyDownloaded,
    AlreadyActive,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("no such model: {0}")]
    NoSuchModel(String),
    #[error("no usable download URL for {0}")]
    NoUrl(String),
    #[error("not running inside a tokio runtime")]
    NoRuntime,
    #[error("index: {0}")]
    Index(String),
}

/// Why one attempt ended.
enum Attempt {
    Done,
    /// Transient (network drop, 5xx): retry, resuming from the `.part`.
    Retry(String),
    /// Unrecoverable: delete the `.part`.
    Fatal(String),
    Cancelled,
}

struct Entry {
    item: DownloadItem,
    cancel: Arc<AtomicBool>,
}

struct Inner {
    entries: Mutex<Vec<Entry>>,
    tx: mpsc::UnboundedSender<DownloadEvent>,
    index: Mutex<FullIndex>,
    index_path: Option<PathBuf>,
    models_dir: PathBuf,
    sema: Arc<Semaphore>,
    retry_delay: Duration,
}

pub struct DownloadManager {
    inner: Arc<Inner>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `<provider>/<tone_id>/<model_id>_<variant>.<ext>` for a record (§9.2).
/// Prefers the stored `local_path` when present.
pub fn rel_path_for(rec: &FullModelRecord) -> String {
    if let Some(p) = &rec.local_path {
        return p.clone();
    }
    let model_id = rec.id.split_once(':').map(|(_, m)| m).unwrap_or(&rec.id);
    let variant = rec
        .metadata
        .get("variant")
        .and_then(|v| v.as_str())
        .unwrap_or("custom");
    let ext = match rec.metadata.get("format").and_then(|v| v.as_str()) {
        Some("ir") => "wav",
        _ => "nam",
    };
    format!(
        "{}/{}/{}_{}.{}",
        rec.provider, rec.tone_id, model_id, variant, ext
    )
}

fn rel_to_path(dir: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(dir.to_path_buf(), |p, c| p.join(c))
}

fn part_of(final_path: &Path) -> PathBuf {
    let mut name = final_path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    final_path.with_file_name(name)
}

fn bak_of(final_path: &Path) -> PathBuf {
    let mut name = final_path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    final_path.with_file_name(name)
}

/// sha256 of a file as `sha256:<hex>`.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

fn norm_sum(s: &str) -> &str {
    s.strip_prefix("sha256:").unwrap_or(s)
}

/// Total size from `Content-Range: bytes a-b/total` (or `bytes */total`).
fn content_range_total(h: &reqwest::header::HeaderMap) -> Option<u64> {
    let s = h.get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
    s.rsplit_once('/')?.1.trim().parse().ok()
}

fn http_client() -> reqwest::Client {
    // reqwest is built with `rustls-no-provider`; pick the provider once.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("reqwest client builds")
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

impl DownloadManager {
    pub fn new(
        tx: mpsc::UnboundedSender<DownloadEvent>,
        index: FullIndex,
        index_path: Option<PathBuf>,
    ) -> Self {
        Self::with_options(
            tx,
            index,
            index_path,
            model_index::models_dir(),
            Duration::from_secs(2),
        )
    }

    /// Explicit models directory and retry back-off (tests).
    pub fn with_options(
        tx: mpsc::UnboundedSender<DownloadEvent>,
        index: FullIndex,
        index_path: Option<PathBuf>,
        models_dir: PathBuf,
        retry_delay: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                entries: Mutex::new(Vec::new()),
                tx,
                index: Mutex::new(index),
                index_path,
                models_dir,
                sema: Arc::new(Semaphore::new(MAX_CONCURRENT)),
                retry_delay,
            }),
        }
    }

    /// Queue a download for index record `id`. `url` overrides the record's
    /// `download_url` (e.g. a signed URL the detail view just resolved). Only
    /// `http(s)` URLs can be fetched.
    pub fn enqueue(&self, id: &str, url: Option<&str>) -> Result<EnqueueOutcome, DownloadError> {
        let inner = &self.inner;
        let rec = self
            .get_model(id)
            .ok_or_else(|| DownloadError::NoSuchModel(id.to_string()))?;
        let rel = rel_path_for(&rec);
        let final_path = rel_to_path(&inner.models_dir, &rel);

        // Already queued or running → no-op.
        if inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.item.id == id && e.item.status.active())
        {
            return Ok(EnqueueOutcome::AlreadyActive);
        }
        // Already on disk and matching the index → no-op.
        if let (Some(local), true) = (&rec.local, final_path.is_file()) {
            if sha256_file(&final_path)
                .map(|s| norm_sum(&s) == norm_sum(&local.checksum))
                .unwrap_or(false)
            {
                return Ok(EnqueueOutcome::AlreadyDownloaded);
            }
        }

        let url = url
            .map(str::to_string)
            .or_else(|| rec.download_url.clone())
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            .ok_or_else(|| DownloadError::NoUrl(id.to_string()))?;
        let handle = tokio::runtime::Handle::try_current().map_err(|_| DownloadError::NoRuntime)?;

        let cancel = Arc::new(AtomicBool::new(false));
        let item = DownloadItem {
            id: id.to_string(),
            status: DownloadStatus::Queued,
            received: 0,
            total: 0,
            percent: 0.0,
            local_path: rel.clone(),
            error: None,
            retries: 0,
        };
        {
            let mut entries = inner.entries.lock().unwrap();
            entries.retain(|e| e.item.id != id); // drop an old finished entry
            entries.push(Entry {
                item: item.clone(),
                cancel: cancel.clone(),
            });
        }
        let _ = inner.tx.send(DownloadEvent { item });

        let inner = self.inner.clone();
        let id = id.to_string();
        handle.spawn(async move { run(inner, id, url, rel, final_path, cancel).await });
        Ok(EnqueueOutcome::Queued)
    }

    /// Cancel a queued/running download. Returns false if none is active.
    pub fn cancel(&self, id: &str) -> bool {
        let entries = self.inner.entries.lock().unwrap();
        match entries
            .iter()
            .find(|e| e.item.id == id && e.item.status.active())
        {
            Some(e) => {
                e.cancel.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn list(&self) -> Vec<DownloadItem> {
        self.inner
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.item.clone())
            .collect()
    }

    pub fn get_model(&self, id: &str) -> Option<FullModelRecord> {
        self.inner
            .index
            .lock()
            .unwrap()
            .models
            .iter()
            .find(|r| r.id == id)
            .cloned()
    }

    /// Run `f` over the in-memory index (listModels etc.).
    pub fn with_index<R>(&self, f: impl FnOnce(&FullIndex) -> R) -> R {
        f(&self.inner.index.lock().unwrap())
    }
}

impl Inner {
    fn update(&self, id: &str, f: impl FnOnce(&mut DownloadItem)) {
        let item = {
            let mut entries = self.entries.lock().unwrap();
            let Some(e) = entries.iter_mut().find(|e| e.item.id == id) else {
                return;
            };
            f(&mut e.item);
            e.item.clone()
        };
        let _ = self.tx.send(DownloadEvent { item });
    }

    fn fail(&self, id: &str, status: DownloadStatus, msg: Option<String>) {
        self.update(id, |it| {
            it.status = status;
            it.error = msg;
        });
    }
}

async fn run(
    inner: Arc<Inner>,
    id: String,
    url: String,
    rel: String,
    final_path: PathBuf,
    cancel: Arc<AtomicBool>,
) {
    let Ok(_permit) = inner.sema.clone().acquire_owned().await else {
        return;
    };
    let part = part_of(&final_path);
    if cancel.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(&part);
        inner.fail(&id, DownloadStatus::Cancelled, Some("cancelled".into()));
        return;
    }
    inner.update(&id, |it| it.status = DownloadStatus::Running);
    if let Some(dir) = final_path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            inner.fail(
                &id,
                DownloadStatus::Failed,
                Some(format!("create dir: {e}")),
            );
            return;
        }
    }

    let client = http_client();
    let (expected_size, expected_sum) = {
        let idx = inner.index.lock().unwrap();
        let rec = idx.models.iter().find(|r| r.id == id);
        let meta = rec.map(|r| &r.metadata);
        (
            meta.and_then(|m| m.get("size_bytes"))
                .and_then(|v| v.as_u64()),
            meta.and_then(|m| m.get("sha256"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
        )
    };

    let mut retries = 0u32;
    loop {
        match attempt(
            &inner,
            &client,
            &id,
            &url,
            &part,
            &cancel,
            expected_size,
            expected_sum.as_deref(),
        )
        .await
        {
            Attempt::Done => break,
            Attempt::Cancelled => {
                let _ = std::fs::remove_file(&part);
                inner.fail(&id, DownloadStatus::Cancelled, Some("cancelled".into()));
                return;
            }
            Attempt::Fatal(msg) => {
                let _ = std::fs::remove_file(&part);
                inner.fail(&id, DownloadStatus::Failed, Some(msg));
                return;
            }
            Attempt::Retry(msg) => {
                if retries >= MAX_RETRIES {
                    // Keep the `.part`: a later download resumes from it.
                    inner.fail(
                        &id,
                        DownloadStatus::Failed,
                        Some(format!(
                            "{msg} (gave up after {retries} retries; partial file kept for resume)"
                        )),
                    );
                    return;
                }
                retries += 1;
                inner.update(&id, |it| it.retries = retries);
                tokio::time::sleep(inner.retry_delay).await;
                if cancel.load(Ordering::SeqCst) {
                    let _ = std::fs::remove_file(&part);
                    inner.fail(&id, DownloadStatus::Cancelled, Some("cancelled".into()));
                    return;
                }
            }
        }
    }

    // Verified. Publish, then update + persist the index.
    let size = part.metadata().map(|m| m.len()).unwrap_or(0);
    let checksum = match sha256_file(&part) {
        Ok(c) => c,
        Err(e) => {
            inner.fail(&id, DownloadStatus::Failed, Some(format!("hash: {e}")));
            return;
        }
    };
    let bak = bak_of(&final_path);
    let had_old = final_path.exists();
    if had_old {
        let _ = std::fs::rename(&final_path, &bak);
    }
    if let Err(e) = std::fs::rename(&part, &final_path) {
        if had_old {
            let _ = std::fs::rename(&bak, &final_path);
        }
        inner.fail(&id, DownloadStatus::Failed, Some(format!("publish: {e}")));
        return;
    }
    let local = LocalState {
        file_name: final_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        size_bytes: size,
        checksum,
        downloaded_at: model_index::now_utc(),
        stale: false,
    };
    let saved = {
        let mut idx = inner.index.lock().unwrap();
        model_index::record_downloaded(&mut idx, &id, &rel, &local);
        match &inner.index_path {
            Some(p) => model_index::save_full_index(&idx, p),
            None => Ok(()),
        }
    };
    if let Err(e) = saved {
        // The file is in place but the index could not be written.
        inner.fail(
            &id,
            DownloadStatus::Failed,
            Some(format!("index write: {e}")),
        );
        return;
    }
    let _ = std::fs::remove_file(&bak);
    inner.update(&id, |it| {
        it.status = DownloadStatus::Done;
        it.received = size;
        it.total = size;
        it.percent = 100.0;
        it.error = None;
    });
}

#[allow(clippy::too_many_arguments)]
async fn attempt(
    inner: &Inner,
    client: &reqwest::Client,
    id: &str,
    url: &str,
    part: &Path,
    cancel: &AtomicBool,
    expected_size: Option<u64>,
    expected_sum: Option<&str>,
) -> Attempt {
    let have = part.metadata().map(|m| m.len()).unwrap_or(0);
    let mut req = client.get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return Attempt::Retry(format!("connect: {e}")),
    };
    let status = resp.status();
    let total;
    let mut append = false;
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        total = content_range_total(resp.headers());
        append = true;
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // Our `.part` is complete or stale.
        match content_range_total(resp.headers()) {
            Some(t) if t == have => return verify(part, expected_size, expected_sum),
            _ => {
                let _ = std::fs::remove_file(part);
                return Attempt::Retry("stale partial file discarded".into());
            }
        }
    } else if status.is_success() {
        // 200: the server ignored the range (or this is a fresh start).
        total = resp.content_length();
    } else if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Attempt::Retry(format!("server said {status}"));
    } else {
        return Attempt::Fatal(format!("server said {status}"));
    }

    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part)
    {
        Ok(f) => f,
        Err(e) => return Attempt::Fatal(format!("open {}: {e}", part.display())),
    };
    let mut received = if append { have } else { 0 };
    let total = total.or(expected_size).unwrap_or(0);
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Attempt::Cancelled;
        }
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => return Attempt::Retry(format!("connection dropped: {e}")),
        };
        if let Err(e) = file.write_all(&chunk) {
            return Attempt::Fatal(format!("write: {e}"));
        }
        received += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(100) {
            last_emit = Instant::now();
            inner.update(id, |it| {
                it.received = received;
                it.total = total;
                it.percent = if total > 0 {
                    (received as f64 / total as f64 * 1000.0).round() / 10.0
                } else {
                    0.0
                };
            });
        }
    }
    drop(file);
    if cancel.load(Ordering::SeqCst) {
        return Attempt::Cancelled;
    }
    if total > 0 && received < total {
        return Attempt::Retry(format!("connection dropped at {received} of {total} bytes"));
    }
    inner.update(id, |it| {
        it.received = received;
        it.total = total.max(received);
        it.percent = 100.0;
    });
    verify(part, expected_size, expected_sum)
}

fn verify(part: &Path, expected_size: Option<u64>, expected_sum: Option<&str>) -> Attempt {
    let len = part.metadata().map(|m| m.len()).unwrap_or(0);
    if let Some(sz) = expected_size {
        if sz != len {
            return Attempt::Fatal(format!("size mismatch: expected {sz} bytes, got {len}"));
        }
    }
    if let Some(sum) = expected_sum {
        match sha256_file(part) {
            Ok(actual) if norm_sum(&actual) == norm_sum(sum) => {}
            Ok(actual) => {
                return Attempt::Fatal(format!("checksum mismatch: expected {sum}, got {actual}"))
            }
            Err(e) => return Attempt::Fatal(format!("hash: {e}")),
        }
    }
    Attempt::Done
}

// ---------------------------------------------------------------------------
// listModels (index browsing)
// ---------------------------------------------------------------------------

/// One page of a paged `listModels` result.
#[derive(Debug, Clone, Serialize)]
pub struct ListModels {
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    pub models: Vec<FullModelRecord>,
    pub facets: Value,
}

/// `listModels` engine-side helper: filters the cached index by the request's
/// query/facets, applies stable sort + paging, and computes the facets object.
pub fn list_models(index: &FullIndex, args: &Value) -> ListModels {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let kinds: Vec<&str> = args
        .get("kinds")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let manufacturers: Vec<&str> = args
        .get("manufacturers")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let cab_ir: Vec<&str> = args
        .get("cabIr")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let formats: Vec<&str> = args
        .get("formats")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let states: Vec<&str> = args
        .get("states")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();

    let mut models: Vec<&FullModelRecord> = index.models.iter().collect();

    if !query.is_empty() {
        models.retain(|r| matches_query(r, &query));
    }
    if !kinds.is_empty() {
        models.retain(|r| {
            kinds.contains(
                &r.metadata
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            )
        });
    }
    if !manufacturers.is_empty() {
        models.retain(|r| {
            manufacturers.contains(
                &r.metadata
                    .get("manufacturer")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            )
        });
    }
    if !cab_ir.is_empty() {
        models.retain(|r| matches_cab_ir(r, &cab_ir));
    }
    if !formats.is_empty() {
        models.retain(|r| {
            formats.contains(
                &r.metadata
                    .get("format")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            )
        });
    }
    if !states.is_empty() {
        models.retain(|r| matches_state(r, &states));
    }

    let sort = args
        .get("sort")
        .and_then(|v| v.as_str())
        .unwrap_or("downloads");
    match sort {
        "name" => models.sort_by(|a, b| meta_str(a, "/name").cmp(&meta_str(b, "/name"))),
        "author" => {
            models.sort_by(|a, b| meta_str(a, "/author/name").cmp(&meta_str(b, "/author/name")))
        }
        "newest" => {
            models.sort_by(|a, b| meta_str(b, "/published_at").cmp(&meta_str(a, "/published_at")))
        }
        "downloads" => {
            models.sort_by(|a, b| {
                num(&b
                    .metadata
                    .get("stats")
                    .and_then(|s| s.get("downloads"))
                    .unwrap_or(&Value::Number(Number::from(0))))
                .cmp(&num(&a
                    .metadata
                    .get("stats")
                    .and_then(|s| s.get("downloads"))
                    .unwrap_or(&Value::Number(Number::from(0)))))
            });
        }
        "favorites" => {
            models.sort_by(|a, b| {
                num(&b
                    .metadata
                    .get("stats")
                    .and_then(|s| s.get("favorites"))
                    .unwrap_or(&Value::Number(Number::from(0))))
                .cmp(&num(&a
                    .metadata
                    .get("stats")
                    .and_then(|s| s.get("favorites"))
                    .unwrap_or(&Value::Number(Number::from(0)))))
            });
        }
        "updated" => {
            models.sort_by(|a, b| meta_str(b, "/updated_at").cmp(&meta_str(a, "/updated_at")));
        }
        "added" => {
            models.sort_by(|a, b| {
                b.local
                    .as_ref()
                    .map(|l| l.downloaded_at.as_str())
                    .cmp(&a.local.as_ref().map(|l| l.downloaded_at.as_str()))
            });
        }
        _ => {} // relevance: no sort
    }

    let page = args.get("page").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let page_size = args.get("pageSize").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
    let total = models.len();
    let start = (page.saturating_sub(1)).saturating_mul(page_size);
    let models: Vec<FullModelRecord> = models
        .drain(start..)
        .take(page_size)
        .map(|r| r.clone())
        .collect();

    let facets = compute_facets(&mut models.iter().collect::<Vec<_>>());

    ListModels {
        total,
        page,
        page_size,
        models,
        facets,
    }
}

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

fn meta_str(rec: &FullModelRecord, path: &str) -> Option<String> {
    rec.metadata
        .pointer(path)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn matches_query(rec: &FullModelRecord, query: &str) -> bool {
    let q = query.to_lowercase();
    if q.is_empty() {
        return true;
    }
    if let Some(name) = meta_str(rec, "/name") {
        if name.to_lowercase().contains(&q) {
            return true;
        }
    }
    if let Some(author) = meta_str(rec, "/author/name") {
        if author.to_lowercase().contains(&q) {
            return true;
        }
    }
    if let Some(tags) = rec.metadata.get("tags").and_then(|t| t.as_array()) {
        if tags
            .iter()
            .any(|t| t.as_str().map_or(false, |s| s.to_lowercase().contains(&q)))
        {
            return true;
        }
    }
    false
}

fn matches_cab_ir(rec: &FullModelRecord, cab_ir: &[&str]) -> bool {
    match cab_ir {
        ["with-cab"] => rec.metadata.get("with_cab").and_then(|v| v.as_bool()) == Some(true),
        ["without-cab"] => rec.metadata.get("with_cab").and_then(|v| v.as_bool()) == Some(false),
        ["ir-only"] => rec.metadata.get("format").and_then(|v| v.as_str()) == Some("ir"),
        _ => false,
    }
}

fn matches_state(rec: &FullModelRecord, states: &[&str]) -> bool {
    match states {
        ["downloaded"] => rec.local_path.is_some() && rec.local.is_some(),
        ["not-downloaded"] => rec.local_path.is_none() && rec.local.is_none(),
        ["stale"] => rec.local.as_ref().map(|l| l.stale).unwrap_or(false),
        ["all"] => true,
        _ => false,
    }
}

fn compute_facets(models: &mut [&FullModelRecord]) -> Value {
    let mut kind = BTreeMap::new();
    let mut manufacturer = BTreeMap::new();
    let mut cab_ir = BTreeMap::new();
    let mut tags = BTreeMap::new();
    let mut fmt = BTreeMap::new();
    let mut state = BTreeMap::new();
    for r in models.iter() {
        *kind
            .entry(
                r.metadata
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
            )
            .or_insert(0) += 1;
        *manufacturer
            .entry(
                r.metadata
                    .get("manufacturer")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
            )
            .or_insert(0) += 1;
        let cab = r
            .metadata
            .get("with_cab")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        *cab_ir
            .entry(if cab { "with-cab" } else { "without-cab" })
            .or_insert(0) += 1;
        *fmt.entry(
            r.metadata
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
        )
        .or_insert(0) += 1;
        let st = if r.local_path.is_some() && r.local.is_some() {
            "downloaded"
        } else {
            "not-downloaded"
        };
        *state.entry(st).or_insert(0) += 1;
        if let Some(tags_arr) = r.metadata.get("tags").and_then(|t| t.as_array()) {
            for t in tags_arr {
                if let Some(name) = t.as_str() {
                    *tags.entry(name).or_insert(0) += 1;
                }
            }
        }
    }
    json!({
        "kind": entries(kind),
        "manufacturer": entries(manufacturer),
        "cabIr": entries(cab_ir),
        "tags": entries(tags),
        "format": entries(fmt),
        "state": entries(state),
    })
}

fn entries<K: ToString, V: Ord + Copy + serde::Serialize>(m: BTreeMap<K, V>) -> Value {
    // count desc, then name asc
    let mut v: Vec<(K, V)> = m.into_iter().collect();
    v.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
    });
    json!(v
        .into_iter()
        .map(|(k, c)| json!({ "value": k.to_string(), "count": c }))
        .collect::<Vec<_>>())
}
