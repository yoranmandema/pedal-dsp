//! Download manager end-to-end against a local fake HTTP server (no real
//! network, no audio). The server drops the connection mid-body on the first
//! request and honours `Range` afterwards.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use pedal_engine::download::{DownloadManager, DownloadStatus, EnqueueOutcome};
use pedal_engine::model_index::{FullIndex, FullModelRecord};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn body() -> Vec<u8> {
    (0..200_000u32).map(|i| (i % 251) as u8).collect()
}

/// `drop_first`: first request sends half the body then closes.
async fn serve(drop_first: bool, honour_range: bool) -> (String, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/m.nam", l.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    tokio::spawn(async move {
        let data = body();
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let n = h.fetch_add(1, Ordering::SeqCst);
            let data = data.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let k = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..k]).to_lowercase();
                let start = req
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
                let total = data.len();
                let (head, slice) = match start {
                    Some(st) if honour_range => (
                        format!(
                            "HTTP/1.1 206 Partial Content\r\ncontent-length: {}\r\ncontent-range: bytes {}-{}/{}\r\nconnection: close\r\n\r\n",
                            total - st, st, total - 1, total
                        ),
                        &data[st..],
                    ),
                    _ => (
                        format!("HTTP/1.1 200 OK\r\ncontent-length: {total}\r\nconnection: close\r\n\r\n"),
                        &data[..],
                    ),
                };
                let _ = s.write_all(head.as_bytes()).await;
                if drop_first && n == 0 {
                    let _ = s.write_all(&slice[..slice.len() / 2]).await;
                    let _ = s.flush().await;
                    return; // connection closed short of content-length
                }
                let _ = s.write_all(slice).await;
            });
        }
    });
    (url, hits)
}

fn index(url: &str, sha: &str, size: u64) -> FullIndex {
    FullIndex {
        schema: 1,
        provider: "tone3000".into(),
        generated_at: "now".into(),
        count: 1,
        models: vec![FullModelRecord {
            id: "t3k:7".into(),
            provider: "tone3000".into(),
            tone_id: "42".into(),
            source_url: "x".into(),
            download_url: Some(url.into()),
            content_hash: None,
            fetched_at: "now".into(),
            local_path: None,
            local: None,
            metadata: json!({"variant": "std", "size_bytes": size, "sha256": sha}),
        }],
    }
}

async fn wait_done(m: &DownloadManager) -> pedal_engine::download::DownloadItem {
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Some(it) = m.list().into_iter().next() {
            if !matches!(it.status, DownloadStatus::Queued | DownloadStatus::Running) {
                return it;
            }
        }
    }
    panic!("download did not finish");
}

fn sha(d: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(d)))
}

#[tokio::test]
async fn resumes_after_drop_and_updates_index() {
    let data = body();
    let (url, hits) = serve(true, true).await;
    let dir = std::env::temp_dir().join(format!("dl-resume-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let idx_path = dir.join("models.json");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let m = DownloadManager::with_options(
        tx,
        index(&url, &sha(&data), data.len() as u64),
        Some(idx_path.clone()),
        dir.join("models"),
        Duration::from_millis(10),
    );
    assert_eq!(m.enqueue("t3k:7", None).unwrap(), EnqueueOutcome::Queued);
    let it = wait_done(&m).await;
    assert_eq!(it.status, DownloadStatus::Done, "{:?}", it.error);
    assert!(it.retries >= 1);
    assert!(hits.load(Ordering::SeqCst) >= 2);

    let final_path = dir.join("models").join(&it.local_path);
    assert_eq!(std::fs::read(&final_path).unwrap(), data);
    assert!(!final_path.with_file_name("7_std.nam.part").exists());
    // Progress events were emitted.
    let mut saw_done = false;
    while let Ok(ev) = rx.try_recv() {
        saw_done |= ev.item.status == DownloadStatus::Done;
    }
    assert!(saw_done);
    // Index updated in memory and on disk.
    let rec = m.get_model("t3k:7").unwrap();
    assert_eq!(rec.local_path.as_deref(), Some(it.local_path.as_str()));
    let disk = FullIndex::load(&idx_path).unwrap().unwrap();
    assert_eq!(disk.models[0].local_path.as_deref(), Some(it.local_path.as_str()));

    // Re-download is a reported no-op.
    assert_eq!(
        m.enqueue("t3k:7", None).unwrap(),
        EnqueueOutcome::AlreadyDownloaded
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn bad_checksum_deletes_partial() {
    let data = body();
    let (url, _) = serve(false, true).await;
    let dir = std::env::temp_dir().join(format!("dl-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let m = DownloadManager::with_options(
        tx,
        index(&url, &format!("sha256:{}", "0".repeat(64)), data.len() as u64),
        None,
        dir.join("models"),
        Duration::from_millis(10),
    );
    m.enqueue("t3k:7", None).unwrap();
    let it = wait_done(&m).await;
    assert_eq!(it.status, DownloadStatus::Failed);
    assert!(it.error.unwrap().contains("checksum"));
    let p = dir.join("models").join(&it.local_path);
    assert!(!p.exists());
    assert!(!p.with_file_name("7_std.nam.part").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn server_without_range_restarts_from_zero() {
    let data = body();
    let (url, _) = serve(true, false).await;
    let dir = std::env::temp_dir().join(format!("dl-norange-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let m = DownloadManager::with_options(
        tx,
        index(&url, &sha(&data), data.len() as u64),
        None,
        dir.join("models"),
        Duration::from_millis(10),
    );
    m.enqueue("t3k:7", None).unwrap();
    let it = wait_done(&m).await;
    assert_eq!(it.status, DownloadStatus::Done, "{:?}", it.error);
    assert_eq!(std::fs::read(dir.join("models").join(&it.local_path)).unwrap(), data);
    let _ = std::fs::remove_dir_all(&dir);
}
