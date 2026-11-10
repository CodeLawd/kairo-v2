//! Ported from `local-bible-installer.test.ts` and `local-pack-download.test.ts`,
//! plus the production HTTP fetcher against a local server and the service's
//! download path end to end.

mod common;

use common::*;
use flate2::{Compression, write::GzEncoder};
use kairo_domain::scripture::translations::get_downloadable_pack;
use kairo_domain::scripture::{LocalBibleInstallPhase, LocalBiblePackInstallResult};
use kairo_domain::serde_util::True;
use kairo_scripture::api::BoxFuture;
use kairo_scripture::installer::{InstallFailure, InstallerBackend, LocalBibleInstaller};
use kairo_scripture::pack::{
    DownloadError, HttpPackFetcher, MAX_COMPRESSED_PACK_BYTES, PackFetcher, TransferProgress,
    download_bible_pack,
};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn download(
    id: &str,
    fetcher: &dyn PackFetcher,
    hash: Option<&str>,
) -> (Result<Vec<u8>, DownloadError>, Vec<TransferProgress>) {
    let mut events = Vec::new();
    let result = block_on(download_bible_pack(id, fetcher, hash, &mut |p| {
        events.push(p)
    }));
    (result, events)
}

// ─── Contract ───────────────────────────────────────────────────────────────

#[test]
fn downloads_target_the_bible_packs_release_with_pinned_hashes() {
    let nkjv = get_downloadable_pack("NKJV").unwrap();
    assert_eq!(
        nkjv.url,
        "https://github.com/CodeLawd/kairo-bible-packs/releases/download/bible-packs-v1/nkjv-pack.db.gz"
    );
    for id in ["NKJV", "NLT", "AMPC"] {
        let pack = get_downloadable_pack(id).unwrap();
        assert!(pack.url.starts_with("https://"));
        assert_eq!(pack.sha256.len(), 64);
        assert!(
            pack.sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
}

#[test]
fn translations_without_a_registry_pack_are_rejected_before_any_fetch() {
    let fetcher = Canned::ok(b"whatever".to_vec());
    let (result, _) = download("KJV", &fetcher, None);
    assert_eq!(
        result.unwrap_err(),
        DownloadError::NotDownloadable("KJV".into())
    );
    assert!(fetcher.requested.lock().unwrap().is_empty());
}

// ─── Failure paths ──────────────────────────────────────────────────────────

#[test]
fn a_missing_release_fails_with_the_http_status() {
    let fetcher = Canned {
        status: 404,
        ..Canned::ok(Vec::new())
    };
    let (result, _) = download("nkjv", &fetcher, None);
    assert_eq!(
        result.unwrap_err().to_string(),
        "NKJV download failed (HTTP 404)."
    );
}

#[test]
fn an_oversized_payload_is_rejected_before_hashing() {
    let big = vec![0; MAX_COMPRESSED_PACK_BYTES as usize + 1];
    let (result, _) = download("NKJV", &Canned::ok(big), None);
    assert!(result.unwrap_err().to_string().contains("invalid size"));
    // A declared length over the limit stops before reading.
    let fetcher = Canned {
        content_length: Some(MAX_COMPRESSED_PACK_BYTES + 1),
        ..Canned::ok(b"x".to_vec())
    };
    let (result, events) = download("NKJV", &fetcher, None);
    assert_eq!(
        result.unwrap_err(),
        DownloadError::InvalidSize("NKJV".into())
    );
    assert!(events.is_empty());
    // So does an empty body.
    let (result, _) = download(
        "NKJV",
        &Canned {
            chunks: vec![],
            ..Canned::ok(vec![])
        },
        None,
    );
    assert_eq!(
        result.unwrap_err(),
        DownloadError::InvalidSize("NKJV".into())
    );
}

#[test]
fn bytes_that_miss_the_pinned_hash_fail_the_security_check() {
    let (result, _) = download("NKJV", &Canned::ok(b"tampered-bytes".to_vec()), None);
    assert!(result.unwrap_err().to_string().contains("security check"));
}

#[test]
fn a_hash_matching_but_corrupt_gzip_fails_decompression() {
    let payload = b"not-gzip-but-hash-matched".to_vec();
    let hash = sha256_hex(&payload);
    let (result, _) = download("NKJV", &Canned::ok(payload), Some(&hash));
    assert!(result.unwrap_err().to_string().contains("decompress"));
}

// ─── Success path ───────────────────────────────────────────────────────────

#[test]
fn matching_bytes_gunzip_to_the_sqlite_payload() {
    let sqlite = b"SQLite format 3\0synthetic-pack-bytes".to_vec();
    let compressed = gzip(&sqlite);
    let hash = sha256_hex(&compressed);
    let (result, _) = download("NKJV", &Canned::ok(compressed), Some(&hash));
    assert_eq!(result.unwrap(), sqlite);
}

#[test]
fn streamed_downloads_report_measured_bytes_then_verification() {
    let sqlite = b"Synthetic SQLite pack".to_vec();
    let compressed = gzip(&sqlite);
    let fetcher = Canned {
        content_length: Some(compressed.len() as u64),
        chunks: vec![compressed[..8].to_vec(), compressed[8..].to_vec()],
        ..Canned::ok(Vec::new())
    };
    let hash = sha256_hex(&compressed);
    let (result, events) = download("NKJV", &fetcher, Some(&hash));
    assert_eq!(result.unwrap(), sqlite);
    assert!(
        events
            .iter()
            .any(|e| e.phase == LocalBibleInstallPhase::Downloading && e.received_bytes == 8)
    );
    let last = events.last().unwrap();
    assert_eq!(last.phase, LocalBibleInstallPhase::Verifying);
    assert_eq!(last.received_bytes, compressed.len() as u64);
    assert_eq!(last.total_bytes, Some(compressed.len() as u64));
}

#[test]
fn unknown_length_stays_indeterminate_and_oversized_streams_stop_early() {
    let compressed = gzip(b"Synthetic payload");
    let hash = sha256_hex(&compressed);
    let (result, events) = download("NKJV", &Canned::ok(compressed), Some(&hash));
    result.unwrap();
    assert!(!events.is_empty());
    assert!(events.iter().all(|e| e.total_bytes.is_none()));

    let endless = Canned {
        chunks: vec![vec![0; 6 * 1024 * 1024]],
        endless: true,
        ..Canned::ok(Vec::new())
    };
    let (result, _) = download("NKJV", &endless, None);
    assert!(result.unwrap_err().to_string().contains("invalid size"));
    assert!(
        endless.dropped.load(Ordering::SeqCst),
        "the transfer must be cancelled"
    );
}

// ─── local-bible-installer.test.ts ──────────────────────────────────────────

type Gate = async_channel_gate::Gate;

/// A tiny one-shot gate built on a mutex and waker list.
mod async_channel_gate {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    #[derive(Clone, Default)]
    pub struct Gate(Arc<Mutex<(bool, Vec<Waker>)>>);

    impl Gate {
        pub fn open(&self) {
            let wakers = {
                let mut state = self.0.lock().unwrap();
                state.0 = true;
                std::mem::take(&mut state.1)
            };
            wakers.into_iter().for_each(Waker::wake);
        }
        pub fn wait(&self) -> impl Future<Output = ()> + Send + '_ {
            Wait(self)
        }
    }

    struct Wait<'a>(&'a Gate);

    impl Future for Wait<'_> {
        type Output = ();
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            let mut state = (self.0).0.lock().unwrap();
            if state.0 {
                Poll::Ready(())
            } else {
                state.1.push(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

#[derive(Default)]
struct FakeBackend {
    gate: Option<Gate>,
    fail: Mutex<bool>,
    downloads: AtomicUsize,
    installs: AtomicUsize,
}

impl InstallerBackend for FakeBackend {
    fn download<'a>(
        &'a self,
        _: &'a str,
        update: &'a mut (dyn FnMut(TransferProgress) + Send),
    ) -> BoxFuture<'a, Result<Vec<u8>, InstallFailure>> {
        Box::pin(async move {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            if *self.fail.lock().unwrap() {
                return Err(DownloadError::Network("fetch failed".into()).into());
            }
            update(TransferProgress {
                phase: LocalBibleInstallPhase::Downloading,
                received_bytes: 5,
                total_bytes: Some(10),
            });
            if let Some(gate) = &self.gate {
                gate.wait().await;
            }
            update(TransferProgress {
                phase: LocalBibleInstallPhase::Verifying,
                received_bytes: 10,
                total_bytes: Some(10),
            });
            Ok(b"fixture".to_vec())
        })
    }

    fn install<'a>(
        &'a self,
        translation: &'a str,
        _: Vec<u8>,
    ) -> BoxFuture<'a, Result<LocalBiblePackInstallResult, InstallFailure>> {
        Box::pin(async move {
            self.installs.fetch_add(1, Ordering::SeqCst);
            Ok(LocalBiblePackInstallResult {
                translation: translation.into(),
                verse_count: 31_101.0,
                chapter_count: 1189.0,
                installed: True,
            })
        })
    }
}

#[test]
fn installation_shares_one_request_and_retains_progress_after_completion() {
    use futures_util::FutureExt;
    let gate = Gate::default();
    let backend = Arc::new(FakeBackend {
        gate: Some(gate.clone()),
        ..Default::default()
    });
    let installer = LocalBibleInstaller::new(backend.clone());
    let phases = Arc::new(Mutex::new(Vec::new()));
    let seen = phases.clone();
    let subscription = installer.subscribe(move |p| seen.lock().unwrap().push(p.phase));
    let first = installer.start("ampc");
    let second = installer.start("AMPC");
    assert!(first.ptr_eq(&second));
    // Run until the download waits on the gate.
    assert!(first.clone().now_or_never().is_none());
    assert_eq!(installer.get_progress("AMPC").unwrap().received_bytes, 5.0);
    gate.open();
    let result = block_on(first).unwrap();
    assert_eq!(result.translation, "AMPC");
    block_on(second).unwrap();
    assert_eq!(backend.downloads.load(Ordering::SeqCst), 1);
    assert_eq!(backend.installs.load(Ordering::SeqCst), 1);
    let progress = installer.get_progress("AMPC").unwrap();
    assert_eq!(progress.phase, LocalBibleInstallPhase::Ready);
    assert!(progress.revision > 1.0);
    let phases = phases.lock().unwrap().clone();
    assert!(phases.contains(&LocalBibleInstallPhase::Verifying));
    assert!(phases.contains(&LocalBibleInstallPhase::Installing));
    subscription.unsubscribe();
    installer.clear("ampc");
    assert!(installer.get_progress("AMPC").is_none());
}

#[test]
fn failure_never_installs_provides_recovery_text_and_can_be_retried() {
    let backend = Arc::new(FakeBackend {
        fail: Mutex::new(true),
        ..Default::default()
    });
    let installer = LocalBibleInstaller::new(backend.clone());
    let error = block_on(installer.start("AMPC")).unwrap_err();
    assert!(
        error.message.to_lowercase().contains("internet connection"),
        "{error}"
    );
    assert_eq!(backend.installs.load(Ordering::SeqCst), 0);
    let progress = installer.get_progress("AMPC").unwrap();
    assert_eq!(progress.phase, LocalBibleInstallPhase::Failed);
    assert!(progress.error.unwrap().to_lowercase().contains("try again"));
    *backend.fail.lock().unwrap() = false;
    block_on(installer.start("AMPC")).unwrap();
    assert_eq!(backend.installs.load(Ordering::SeqCst), 1);
    let progress = installer.get_progress("AMPC").unwrap();
    assert_eq!(progress.phase, LocalBibleInstallPhase::Ready);
    // A retry clears the earlier error.
    assert_eq!(progress.error, None);
}

#[test]
fn rapid_download_updates_are_throttled_but_phase_changes_are_not() {
    struct Chatty;
    impl InstallerBackend for Chatty {
        fn download<'a>(
            &'a self,
            _: &'a str,
            update: &'a mut (dyn FnMut(TransferProgress) + Send),
        ) -> BoxFuture<'a, Result<Vec<u8>, InstallFailure>> {
            Box::pin(async move {
                for received_bytes in 1..=50 {
                    update(TransferProgress {
                        phase: LocalBibleInstallPhase::Downloading,
                        received_bytes,
                        total_bytes: None,
                    });
                }
                Err(InstallFailure::Other("Missing verse 1:1:1.".into()))
            })
        }
        fn install<'a>(
            &'a self,
            _: &'a str,
            _: Vec<u8>,
        ) -> BoxFuture<'a, Result<LocalBiblePackInstallResult, InstallFailure>> {
            unreachable!("download fails first")
        }
    }
    let installer = LocalBibleInstaller::new(Arc::new(Chatty));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let _subscription =
        installer.subscribe(move |p| sink.lock().unwrap().push((p.phase, p.received_bytes)));
    let error = block_on(installer.start("NKJV")).unwrap_err();
    assert!(error.message.contains("restart Kairo"));
    let seen = seen.lock().unwrap().clone();
    // The start is announced, the 50 rapid updates are not, the failure is.
    assert_eq!(
        seen.first(),
        Some(&(LocalBibleInstallPhase::Downloading, 0.0))
    );
    assert_eq!(seen.last().unwrap().0, LocalBibleInstallPhase::Failed);
    assert!(seen.len() < 10, "{seen:?}");
    assert_eq!(installer.get_progress("NKJV").unwrap().received_bytes, 50.0);
}

// ─── Service download path ──────────────────────────────────────────────────

#[test]
fn service_downloads_refuse_unknown_packs_and_never_install_unverified_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let db = open_dest_with_kjv(dir.path());
    // A structurally valid NKJV pack, but not the pinned bytes.
    let pack = write_pack(
        &dir.path().join("nkjv.db"),
        "NKJV",
        "Fixture",
        &fixture_verses(),
    );
    let fetcher = Arc::new(Canned::ok(gzip(&std::fs::read(&pack).unwrap())));
    let service = service_with(db.clone(), fetcher.clone());
    let error = block_on(service.download_local_bible_translation("KJV")).unwrap_err();
    assert_eq!(
        error.to_string(),
        "No downloadable local pack is available for KJV."
    );

    let progress = Arc::new(Mutex::new(Vec::new()));
    let sink = progress.clone();
    let _subscription =
        service.on_local_bible_install_progress(move |p| sink.lock().unwrap().push(p.clone()));
    let error = block_on(service.download_local_bible_translation("nkjv")).unwrap_err();
    assert_eq!(
        error.to_string(),
        "The download could not be verified. Please try downloading it again."
    );
    assert_eq!(
        fetcher.requested.lock().unwrap().as_slice(),
        [get_downloadable_pack("NKJV").unwrap().url]
    );
    assert!(!block_on(db.has_translation("NKJV")).unwrap());
    let status = block_on(service.get_local_bible_pack_status("NKJV")).unwrap();
    assert!(!status.installed);
    assert_eq!(
        status.installation.unwrap().phase,
        LocalBibleInstallPhase::Failed
    );
    assert_eq!(
        progress.lock().unwrap().last().unwrap().phase,
        LocalBibleInstallPhase::Failed
    );
    // A failed install may be cleared by removal.
    let status = block_on(service.remove_local_bible_translation("NKJV")).unwrap();
    assert!(status.installation.is_none());
}

#[test]
fn removal_waits_for_an_install_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Gate::default();
    struct Held(Gate);
    impl PackFetcher for Held {
        fn get<'a>(
            &'a self,
            _: &'a str,
        ) -> BoxFuture<'a, Result<Box<dyn kairo_scripture::pack::PackBody>, DownloadError>>
        {
            Box::pin(async move {
                self.0.wait().await;
                Err(DownloadError::Network("closed".into()))
            })
        }
    }
    let service = service_with(open_dest_with_kjv(dir.path()), Arc::new(Held(gate.clone())));
    let mut install = Box::pin(service.download_local_bible_translation("NLT"));
    assert!(futures_util::FutureExt::now_or_never(&mut install).is_none());
    let error = block_on(service.remove_local_bible_translation("NLT")).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Wait for this Bible to finish installing before removing it."
    );
    gate.open();
    assert!(
        block_on(install)
            .unwrap_err()
            .to_string()
            .contains("internet connection")
    );
    assert!(block_on(service.remove_local_bible_translation("NLT")).is_ok());
}

/// The real AMPC asset through the service, with its pinned hash.
#[test]
#[ignore = "Requires the Electron checkout's bible-packs/ampc-pack.db.gz"]
fn service_installs_the_real_ampc_asset_end_to_end() {
    let asset = kairo_oracle::electron_dir().join("bible-packs/ampc-pack.db.gz");
    let bytes = std::fs::read(asset).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = open_dest_with_kjv(dir.path());
    let service = service_with(db.clone(), Arc::new(Canned::ok(bytes)));
    let leftovers = || {
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter(|entry| {
                let name = entry.as_ref().unwrap().file_name();
                name.to_string_lossy().starts_with("kairo-ampc-")
            })
            .count()
    };
    let before = leftovers();
    let result = block_on(service.download_local_bible_translation("AMPC")).unwrap();
    assert_eq!(
        (result.verse_count, result.chapter_count),
        (31_101.0, 1189.0)
    );
    let status = block_on(service.get_local_bible_pack_status("AMPC")).unwrap();
    assert!(status.installed);
    assert_eq!(
        status.installation.unwrap().phase,
        LocalBibleInstallPhase::Ready
    );
    let verses =
        block_on(service.lookup_verses("Mark", 9.0, 43.0, Some(48.0), Some("AMPC"))).unwrap();
    assert_eq!(
        verses.iter().map(|v| v.verse).collect::<Vec<_>>(),
        [43.0, 45.0, 47.0, 48.0]
    );
    // The temporary pack file is removed after install.
    assert_eq!(leftovers(), before);
}

/// Real downloads from the registry release.
#[test]
#[ignore = "Downloads about 16 MB from GitHub"]
fn real_registry_packs_download_and_verify() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let fetcher = HttpPackFetcher::new().unwrap();
    for id in ["NKJV", "NLT", "AMPC"] {
        let start = std::time::Instant::now();
        let mut events = 0;
        let sqlite = runtime
            .block_on(download_bible_pack(id, &fetcher, None, &mut |_| {
                events += 1
            }))
            .unwrap();
        assert!(sqlite.starts_with(b"SQLite format 3\0"));
        eprintln!(
            "{id}: {} bytes after gunzip, {events} progress events, {:?}",
            sqlite.len(),
            start.elapsed()
        );
    }
}

// ─── The production fetcher against a local server ──────────────────────────

/// Serves `responses` in order, one connection each, and returns the port.
async fn serve(responses: Vec<Vec<u8>>) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let _ = socket.read(&mut request).await;
            socket.write_all(&response).await.unwrap();
            let _ = socket.shutdown().await;
        }
    });
    port
}

struct Get(HttpPackFetcher, String);

impl Get {
    async fn run(&self) -> Result<(u16, Option<u64>, Vec<u8>), DownloadError> {
        let mut body = self.0.get(&self.1).await?;
        let (status, length) = (body.status(), body.content_length());
        let mut bytes = Vec::new();
        while let Some(chunk) = body.chunk().await? {
            bytes.extend(chunk);
        }
        Ok((status, length, bytes))
    }
}

#[tokio::test]
async fn http_fetcher_follows_redirects_and_reports_status_and_length() {
    let port = serve(vec![
        b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_vec(),
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    ])
    .await;
    let url = format!("http://127.0.0.1:{port}/pack.db.gz");
    let get = Get(HttpPackFetcher::new().unwrap(), url);
    assert_eq!(get.run().await.unwrap(), (200, Some(5), b"hello".to_vec()));
    assert_eq!(get.run().await.unwrap().0, 404);
}

#[tokio::test]
async fn http_fetcher_reports_an_unreachable_host_as_a_network_failure() {
    // Bind then drop, so nothing listens on the port.
    let port = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let get = Get(
        HttpPackFetcher::new().unwrap(),
        format!("http://127.0.0.1:{port}/"),
    );
    assert!(matches!(get.run().await, Err(DownloadError::Network(_))));
}
