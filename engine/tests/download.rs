//! Download manager against a loopback HTTP mock: mid-download drop + Range
//! resume, servers that ignore Range, duplicate avoidance, failure cleanup,
//! cancel, and the index update. No audio, no external network.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pedal_engine::download::{DownloadEvent, DownloadManager, DownloadStatus, EnqueueOutcome};
use pedal_engine::model_index::{FullIndex, FullModelRecord, LocalState};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Drop the first (non-range) response after half the body; honour Range.
    DropOnceThenRange,
    /// Drop the first response; ignore Range afterwards (always 200 + full body).
    DropOnceIgnoreRange,
    /// Always 404.
    NotFound,
    /// Send headers + a trickle forever (for cancel).
    Slow,
}

struct Mock {
    url: String,
    requests: Arc<Mutex<Vec<Option<String>>>>,
}

fn body() -> Vec<u8> {
    (0..200_000u32).map(|i| (i % 251) as u8).collect()
}

async fn mock(mode: Mode) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/m.nam", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let hits = Arc::new(AtomicUsize::new(0));
    let reqs = requests.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let reqs = reqs.clone();
            let hits = hits.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let range = head
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .and_then(|l| l.split("bytes=").nth(1))
                    .and_then(|r| r.trim().trim_end_matches('-').parse::<usize>().ok());
                reqs.lock()
                    .unwrap()
                    .push(range.map(|r| format!("bytes={r}-")));
                let nth = hits.fetch_add(1, Ordering::SeqCst);
                let data = body();
                match mode {
                    Mode::NotFound => {
                        let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    }
                    Mode::Slow => {
                        let h = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            data.len()
                        );
                        let _ = sock.write_all(h.as_bytes()).await;
                        for _ in 0..600 {
                            if sock.write_all(&data[..100]).await.is_err() {
                                return;
                            }
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                    Mode::DropOnceThenRange | Mode::DropOnceIgnoreRange => {
                        if nth == 0 {
                            let h = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
                            let _ = sock.write_all(h.as_bytes()).await;
                            let _ = sock.write_all(&data[..data.len() / 2]).await;
                            let _ = sock.flush().await;
                            return; // connection closes mid-body
                        }
                        match (range, mode == Mode::DropOnceThenRange) {
                            (Some(start), true) => {
                                let h = format!(
                                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                                    data.len() - start, start, data.len() - 1, data.len()
                                );
                                let _ = sock.write_all(h.as_bytes()).await;
                                let _ = sock.write_all(&data[start..]).await;
                            }
                            _ => {
                                let h = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
                                let _ = sock.write_all(h.as_bytes()).await;
                                let _ = sock.write_all(&data).await;
                            }
                        }
                    }
                }
                let _ = sock.shutdown().await;
            });
        }
    });
    Mock { url, requests }
}

fn record(url: &str, local: Option<LocalState>) -> FullModelRecord {
    FullModelRecord {
        id: "t3k:42".into(),
        provider: "t3k".into(),
        tone_id: "7".into(),
        source_url: "https://example.invalid/tones/7".into(),
        download_url: Some(url.into()),
        content_hash: None,
        fetched_at: "2026-11-18T00:00:00Z".into(),
        local_path: local.as_ref().map(|_| "t3k/7/42_a2.nam".into()),
        local,
        metadata: serde_json::json!({
            "name": "Test", "kind": "amp", "format": "nam", "variant": "a2",
            "author": { "name": "x" },
        }),
    }
}

struct Fixture {
    dir: PathBuf,
    index_path: PathBuf,
    mgr: DownloadManager,
    rx: mpsc::UnboundedReceiver<DownloadEvent>,
}

fn fixture(name: &str, rec: FullModelRecord) -> Fixture {
    let dir = std::env::temp_dir().join(format!("pedal-dl-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let index_path = dir.join("index").join("models.json");
    let index = FullIndex {
        schema: 1,
        provider: "t3k".into(),
        generated_at: "2026-11-18T00:00:00Z".into(),
        count: 1,
        models: vec![rec],
    };
    let (tx, rx) = mpsc::unbounded_channel();
    let mgr = DownloadManager::with_options(
        tx,
        index,
        Some(index_path.clone()),
        dir.join("models"),
        Duration::from_millis(20),
    );
    Fixture {
        dir,
        index_path,
        mgr,
        rx,
    }
}

async fn wait_terminal(f: &mut Fixture) -> Vec<DownloadEvent> {
    let mut seen = Vec::new();
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(20), f.rx.recv())
            .await
            .expect("timed out")
            .expect("channel");
        let done = matches!(
            ev.item.status,
            DownloadStatus::Done | DownloadStatus::Failed | DownloadStatus::Cancelled
        );
        seen.push(ev);
        if done {
            return seen;
        }
    }
}

fn sum(data: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(data)))
}

#[tokio::test]
async fn drop_mid_download_resumes_with_range_to_a_valid_file() {
    let m = mock(Mode::DropOnceThenRange).await;
    let mut f = fixture("resume", record(&m.url, None));
    assert_eq!(
        f.mgr.enqueue("t3k:42", None).unwrap(),
        EnqueueOutcome::Queued
    );
    let events = wait_terminal(&mut f).await;
    let last = &events.last().unwrap().item;
    assert_eq!(last.status, DownloadStatus::Done, "{:?}", last.error);
    assert!(last.retries >= 1);
    // statuses reported queued -> running -> done
    assert_eq!(events[0].item.status, DownloadStatus::Queued);
    assert!(events
        .iter()
        .any(|e| e.item.status == DownloadStatus::Running));

    let path = f.dir.join("models/t3k/7/42_a2.nam");
    assert_eq!(std::fs::read(&path).unwrap(), body());
    assert!(!path.with_file_name("42_a2.nam.part").exists());
    // the second request asked for the remainder
    let reqs = m.requests.lock().unwrap().clone();
    assert_eq!(reqs[0], None);
    assert_eq!(reqs[1].as_deref(), Some("bytes=100000-"));

    // index updated in memory and on disk
    let rec = f.mgr.get_model("t3k:42").unwrap();
    assert_eq!(rec.local_path.as_deref(), Some("t3k/7/42_a2.nam"));
    let local = rec.local.unwrap();
    assert_eq!(local.checksum, sum(&body()));
    assert_eq!(local.size_bytes, body().len() as u64);
    let disk = FullIndex::load(&f.index_path).unwrap().unwrap();
    assert_eq!(
        disk.models[0].local_path.as_deref(),
        Some("t3k/7/42_a2.nam")
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn server_that_ignores_range_restarts_cleanly() {
    let m = mock(Mode::DropOnceIgnoreRange).await;
    let mut f = fixture("norange", record(&m.url, None));
    f.mgr.enqueue("t3k:42", None).unwrap();
    let events = wait_terminal(&mut f).await;
    assert_eq!(events.last().unwrap().item.status, DownloadStatus::Done);
    assert_eq!(
        std::fs::read(f.dir.join("models/t3k/7/42_a2.nam")).unwrap(),
        body()
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn redownload_is_a_reported_no_op_and_active_duplicates_are_rejected() {
    let m = mock(Mode::DropOnceThenRange).await;
    let mut f = fixture("dup", record(&m.url, None));
    f.mgr.enqueue("t3k:42", None).unwrap();
    // second enqueue while the first is active
    assert_eq!(
        f.mgr.enqueue("t3k:42", None).unwrap(),
        EnqueueOutcome::AlreadyActive
    );
    wait_terminal(&mut f).await;
    let before = m.requests.lock().unwrap().len();
    assert_eq!(
        f.mgr.enqueue("t3k:42", None).unwrap(),
        EnqueueOutcome::AlreadyDownloaded
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        m.requests.lock().unwrap().len(),
        before,
        "no new HTTP request"
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn http_error_fails_and_leaves_no_partial_file() {
    let m = mock(Mode::NotFound).await;
    let mut f = fixture("404", record(&m.url, None));
    f.mgr.enqueue("t3k:42", None).unwrap();
    let events = wait_terminal(&mut f).await;
    let last = &events.last().unwrap().item;
    assert_eq!(last.status, DownloadStatus::Failed);
    assert!(last.error.as_deref().unwrap().contains("404"));
    let dir = f.dir.join("models/t3k/7");
    assert!(std::fs::read_dir(&dir)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true));
    assert!(f.mgr.get_model("t3k:42").unwrap().local.is_none());
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn checksum_mismatch_is_fatal_and_cleans_up() {
    let m = mock(Mode::DropOnceThenRange).await;
    let mut rec = record(&m.url, None);
    rec.metadata["sha256"] = serde_json::json!(format!("sha256:{}", "0".repeat(64)));
    let mut f = fixture("badsum", rec);
    f.mgr.enqueue("t3k:42", None).unwrap();
    let events = wait_terminal(&mut f).await;
    let last = &events.last().unwrap().item;
    assert_eq!(last.status, DownloadStatus::Failed);
    assert!(last.error.as_deref().unwrap().contains("checksum mismatch"));
    assert!(!f.dir.join("models/t3k/7/42_a2.nam").exists());
    assert!(!f.dir.join("models/t3k/7/42_a2.nam.part").exists());
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn matching_checksum_and_size_pass() {
    let m = mock(Mode::DropOnceThenRange).await;
    let mut rec = record(&m.url, None);
    rec.metadata["sha256"] = serde_json::json!(sum(&body()));
    rec.metadata["size_bytes"] = serde_json::json!(body().len());
    let mut f = fixture("goodsum", rec);
    f.mgr.enqueue("t3k:42", None).unwrap();
    assert_eq!(
        wait_terminal(&mut f).await.last().unwrap().item.status,
        DownloadStatus::Done
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn cancel_removes_the_partial_file() {
    let m = mock(Mode::Slow).await;
    let mut f = fixture("cancel", record(&m.url, None));
    f.mgr.enqueue("t3k:42", None).unwrap();
    // wait for progress, then cancel
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(10), f.rx.recv())
            .await
            .unwrap()
            .unwrap();
        if ev.item.received > 0 {
            break;
        }
    }
    assert!(f.mgr.cancel("t3k:42"));
    let events = wait_terminal(&mut f).await;
    assert_eq!(
        events.last().unwrap().item.status,
        DownloadStatus::Cancelled
    );
    assert!(!f.dir.join("models/t3k/7/42_a2.nam.part").exists());
    assert!(!f.mgr.cancel("t3k:42"), "nothing active any more");
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn unknown_model_and_missing_url_are_errors() {
    let m = mock(Mode::NotFound).await;
    let mut rec = record(&m.url, None);
    rec.download_url = Some("r2://models/x.nam".into());
    let f = fixture("nourl", rec);
    assert!(f.mgr.enqueue("t3k:nope", None).is_err());
    assert!(
        f.mgr.enqueue("t3k:42", None).is_err(),
        "r2:// is not fetchable"
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}
