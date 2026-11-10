//! The updater against simulated release feeds on a local server: valid,
//! tampered checksum, wrong architecture, network loss, interrupted
//! download, plus the guards around install.
//!
//! The Linux AppImage path runs end to end here on any Unix (its swap is a
//! shell script). The macOS bundle path is proven in `mac_bundle.rs`.

use base64::Engine;
use kairo_domain::status::UpdateStatus;
use kairo_updater::feed::{Arch, Platform, ReleaseSource};
use kairo_updater::{InstallOutcome, UpdaterConfig, UpdaterService};
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone)]
struct Route {
    status: u16,
    body: Vec<u8>,
    /// Send only this many body bytes, then drop the connection.
    cut_after: Option<usize>,
}

#[derive(Clone, Default)]
struct Server {
    routes: Arc<Mutex<HashMap<String, Route>>>,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Server {
    async fn start() -> (Self, String) {
        let server = Self::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let s = server.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let s = s.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(n) = stream.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let head = String::from_utf8_lossy(&buf).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    s.hits.lock().unwrap().push(path.clone());
                    let route = s
                        .routes
                        .lock()
                        .unwrap()
                        .get(&path)
                        .cloned()
                        .unwrap_or(Route {
                            status: 404,
                            body: b"Not Found".to_vec(),
                            cut_after: None,
                        });
                    let header = format!(
                        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        route.status,
                        route.body.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let body = match route.cut_after {
                        Some(n) => &route.body[..n],
                        None => &route.body[..],
                    };
                    let _ = stream.write_all(body).await;
                    let _ = stream.flush().await;
                });
            }
        });
        (server, base)
    }

    fn route(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(
            path.into(),
            Route {
                status: 200,
                body: body.into(),
                cut_after: None,
            },
        );
    }

    fn cut(&self, path: &str, after: usize) {
        self.routes.lock().unwrap().get_mut(path).unwrap().cut_after = Some(after);
    }
}

fn sha512(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
}

fn feed(version: &str, file: &str, payload: &[u8]) -> String {
    format!(
        "version: {version}\nfiles:\n  - url: {file}\n    sha512: {}\n    size: {}\npath: {file}\nsha512: {}\nreleaseDate: '2026-11-10T00:00:00.000Z'\n",
        sha512(payload),
        payload.len(),
        sha512(payload)
    )
}

struct Rig {
    server: Server,
    service: UpdaterService,
    statuses: Arc<Mutex<Vec<UpdateStatus>>>,
    root: tempfile::TempDir,
    appimage: PathBuf,
    temp: PathBuf,
    releases: String,
}

async fn rig(platform: Platform, arch: Arch, appimage: bool) -> Rig {
    let (server, base) = Server::start().await;
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("apps").join("Kairo.AppImage");
    std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
    std::fs::write(&installed, "#!/bin/sh\necho 1.0.33\n").unwrap();
    let temp = root.path().join("tmp");
    std::fs::create_dir_all(&temp).unwrap();
    let config = UpdaterConfig {
        current_version: "1.0.33".into(),
        packaged: true,
        simulate: false,
        platform,
        arch,
        exe: installed.clone(),
        // A PID that is not running, so the swap does not wait.
        pid: 999_999,
        temp_dir: temp.clone(),
        appimage: appimage.then(|| installed.clone()),
        web_url: Some("https://kairopresenter.com".into()),
        source: ReleaseSource {
            releases: format!("{base}{RELEASES}"),
        },
    };
    let service = UpdaterService::new(config);
    let statuses: Arc<Mutex<Vec<UpdateStatus>>> = Arc::default();
    let s = statuses.clone();
    service.on_change(move |status| s.lock().unwrap().push(status.clone()));
    Rig {
        server,
        service,
        statuses,
        root,
        appimage: installed,
        temp,
        releases: format!("{base}{RELEASES}"),
    }
}

const RELEASES: &str = "/CodeLawd/kairo-presenter/releases";

fn payload() -> Vec<u8> {
    let mut bytes = b"#!/bin/sh\necho 1.0.34\n".to_vec();
    bytes.extend((0..200_000u32).map(|i| (i % 251) as u8));
    bytes
}

fn message(status: &UpdateStatus) -> &str {
    match status {
        UpdateStatus::Error { message } => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn leftovers(temp: &Path) -> usize {
    std::fs::read_dir(temp).unwrap().count()
}

async fn wait_until(what: &str, done: impl Fn() -> bool) {
    for _ in 0..200 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn valid_feed_checks_downloads_on_request_and_replaces_the_appimage() {
    let rig = rig(Platform::Linux, Arch::X64, true).await;
    let bytes = payload();
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        feed("1.0.34", "Kairo-1.0.34.AppImage", &bytes),
    );
    rig.server.route(
        &format!("{RELEASES}/download/v1.0.34/Kairo-1.0.34.AppImage"),
        bytes.clone(),
    );
    rig.server.route(
        &format!("{RELEASES}.atom"),
        format!(r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><link href="https://github.com{RELEASES}/tag/v1.0.34"/><content type="html">&lt;p&gt;Fixes&lt;/p&gt;</content></entry></feed>"#),
    );

    // Not packaged: everything is a no-op.
    assert_eq!(
        rig.service.status(),
        UpdateStatus::Idle {
            current_version: Some("1.0.33".into())
        }
    );
    let checked = rig.service.check().await;
    assert_eq!(
        checked,
        UpdateStatus::Available {
            version: "1.0.34".into(),
            notes: Some("<p>Fixes</p>".into())
        }
    );
    assert!(
        !rig.server
            .hits
            .lock()
            .unwrap()
            .iter()
            .any(|h| h.ends_with(".AppImage")),
        "checking never downloads"
    );
    // A second check finds the same release.
    assert!(matches!(
        rig.service.check().await,
        UpdateStatus::Available { .. }
    ));

    let downloaded = rig.service.download().await;
    assert_eq!(
        downloaded,
        UpdateStatus::Downloaded {
            version: "1.0.34".into(),
            notes: Some("<p>Fixes</p>".into())
        }
    );
    let percents: Vec<f64> = rig
        .statuses
        .lock()
        .unwrap()
        .iter()
        .filter_map(|s| match s {
            UpdateStatus::Downloading { percent, .. } => *percent,
            _ => None,
        })
        .collect();
    assert_eq!(percents.first(), Some(&0.0));
    assert!(
        percents.windows(2).all(|w| w[0] < w[1]),
        "each percentage once, rising: {percents:?}"
    );
    assert!(
        percents.iter().all(|p| *p <= 99.0),
        "never 100 before the checksum"
    );
    assert!(percents.len() <= 101);

    // Checking while one is staged keeps it.
    assert!(matches!(
        rig.service.check().await,
        UpdateStatus::Downloaded { .. }
    ));
    // Downloading again is a no-op.
    assert!(matches!(
        rig.service.download().await,
        UpdateStatus::Downloaded { .. }
    ));

    // Install: live output asks first; then hardware is released, the swap starts and the app quits.
    let released = Arc::new(AtomicBool::new(false));
    let quits = Arc::new(AtomicUsize::new(0));
    let r = released.clone();
    rig.service.set_before_install(move || {
        let r = r.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            r.store(true, Ordering::SeqCst);
        }
    });
    let q = quits.clone();
    let r = released.clone();
    rig.service.set_quit(move || {
        assert!(
            r.load(Ordering::SeqCst),
            "hardware is released before the quit"
        );
        q.fetch_add(1, Ordering::SeqCst);
    });
    rig.service.set_live_output(|| true);
    assert_eq!(
        rig.service.install(false).await,
        InstallOutcome::NeedsConfirmation
    );
    assert!(!released.load(Ordering::SeqCst));
    assert_eq!(rig.service.install(true).await, InstallOutcome::Started);
    assert_eq!(quits.load(Ordering::SeqCst), 1);
    let installed = rig.appimage.clone();
    wait_until("the AppImage swap", || {
        std::fs::read(&installed).ok().as_deref() == Some(&bytes[..])
    })
    .await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&installed).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    wait_until("staging cleaned", || leftovers(&rig.temp) == 0).await;
    // A second install does nothing.
    assert_eq!(rig.service.install(true).await, InstallOutcome::Ignored);
    drop(rig.root);
}

#[tokio::test]
async fn a_tampered_checksum_is_never_staged() {
    let rig = rig(Platform::Linux, Arch::X64, true).await;
    let bytes = payload();
    let mut feed_text = feed("1.0.34", "Kairo-1.0.34.AppImage", &bytes);
    feed_text = feed_text.replace(&sha512(&bytes), &sha512(b"something else"));
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        feed_text,
    );
    rig.server.route(
        &format!("{RELEASES}/download/v1.0.34/Kairo-1.0.34.AppImage"),
        bytes,
    );
    rig.service.check().await;
    let status = rig.service.download().await;
    assert_eq!(
        message(&status),
        "Could not download the update. Please try again."
    );
    assert_eq!(leftovers(&rig.temp), 0, "the download is deleted");
    assert_eq!(rig.service.install(true).await, InstallOutcome::Ignored);
    rig.service.on_quit();
    assert_eq!(
        std::fs::read_to_string(&rig.appimage).unwrap(),
        "#!/bin/sh\necho 1.0.33\n"
    );
}

#[tokio::test]
async fn the_wrong_architecture_is_never_offered() {
    let rig = rig(Platform::Linux, Arch::X64, true).await;
    let bytes = payload();
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        feed("1.0.34", "Kairo-1.0.34-arm64.AppImage", &bytes),
    );
    rig.service.check().await;
    let status = rig.service.download().await;
    assert_eq!(
        message(&status),
        "This release has no update file for this computer."
    );
    assert!(
        !rig.server
            .hits
            .lock()
            .unwrap()
            .iter()
            .any(|h| h.contains("arm64"))
    );

    // An Intel Mac given only the Apple silicon zip.
    let rig = self::rig_mac_x64_with_arm_zip().await;
    let status = rig.service.download().await;
    assert_eq!(
        message(&status),
        "This release has no update file for this Mac."
    );
}

async fn rig_mac_x64_with_arm_zip() -> Rig {
    let rig = rig(Platform::Mac, Arch::X64, false).await;
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-mac.yml"),
        feed("1.0.34", "Kairo-1.0.34-arm64-mac.zip", b"zip"),
    );
    rig.service.check().await;
    rig
}

#[tokio::test]
async fn network_loss_reports_a_connection_problem() {
    let rig = rig(Platform::Linux, Arch::X64, true).await;
    // Nothing is listening on this port.
    let offline = UpdaterService::new(UpdaterConfig {
        source: ReleaseSource {
            releases: "http://127.0.0.1:9/releases".into(),
        },
        ..base_config(&rig)
    });
    assert_eq!(
        message(&offline.check().await),
        "Could not check for updates. Please check your connection and try again."
    );
    // A server error is a connection problem too; a missing feed (404) is not.
    rig.server.routes.lock().unwrap().insert(
        format!("{RELEASES}/latest/download/latest-linux.yml"),
        Route {
            status: 503,
            body: b"busy".to_vec(),
            cut_after: None,
        },
    );
    assert_eq!(
        message(&rig.service.check().await),
        "Could not check for updates. Please check your connection and try again."
    );
    rig.server.routes.lock().unwrap().clear();
    assert_eq!(
        message(&rig.service.check().await),
        "Could not check for updates. Please try again."
    );
    // A broken feed.
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        "<html>oops</html>",
    );
    assert_eq!(
        message(&rig.service.check().await),
        "Could not check for updates. Please try again."
    );
    // The same or an older version is no update.
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        feed("1.0.33", "Kairo-1.0.33.AppImage", b"x"),
    );
    assert_eq!(
        rig.service.check().await,
        UpdateStatus::Idle {
            current_version: Some("1.0.33".into())
        }
    );
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        feed("1.0.2", "Kairo-1.0.2.AppImage", b"x"),
    );
    assert_eq!(
        rig.service.check().await,
        UpdateStatus::Idle {
            current_version: Some("1.0.33".into())
        }
    );
}

fn base_config(rig: &Rig) -> UpdaterConfig {
    UpdaterConfig {
        current_version: "1.0.33".into(),
        packaged: true,
        simulate: false,
        platform: Platform::Linux,
        arch: Arch::X64,
        exe: rig.appimage.clone(),
        pid: 999_999,
        temp_dir: rig.temp.clone(),
        appimage: Some(rig.appimage.clone()),
        web_url: None,
        source: ReleaseSource {
            releases: rig.releases.clone(),
        },
    }
}

#[tokio::test]
async fn an_interrupted_download_is_discarded_and_offered_again() {
    let rig = rig(Platform::Linux, Arch::X64, true).await;
    let bytes = payload();
    let path = format!("{RELEASES}/download/v1.0.34/Kairo-1.0.34.AppImage");
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        feed("1.0.34", "Kairo-1.0.34.AppImage", &bytes),
    );
    rig.server.route(&path, bytes.clone());
    rig.server.cut(&path, bytes.len() / 2);
    rig.service.check().await;
    let status = rig.service.download().await;
    assert_eq!(
        message(&status),
        "Could not download the update. Please check your connection and try again."
    );
    assert_eq!(leftovers(&rig.temp), 0);
    assert_eq!(rig.service.install(true).await, InstallOutcome::Ignored);
    // As after a relaunch: a fresh check offers it again and the full file installs.
    std::fs::create_dir_all(rig.temp.join("kairo-update-1-2")).unwrap();
    let relaunched = UpdaterService::new(UpdaterConfig {
        ..base_config(&rig)
    });
    rig.server
        .routes
        .lock()
        .unwrap()
        .get_mut(&path)
        .unwrap()
        .cut_after = None;
    relaunched.init().await;
    assert_eq!(
        leftovers(&rig.temp),
        0,
        "a stale download folder is removed at start"
    );
    assert!(matches!(
        relaunched.status(),
        UpdateStatus::Available { .. }
    ));
    assert!(matches!(
        relaunched.download().await,
        UpdateStatus::Downloaded { .. }
    ));
    // An ordinary quit installs it without reopening.
    relaunched.on_quit();
    let installed = rig.appimage.clone();
    wait_until("swap on quit", || {
        std::fs::read(&installed).ok().as_deref() == Some(&bytes[..])
    })
    .await;
}

#[tokio::test]
async fn a_package_install_is_sent_to_the_website_and_a_read_only_install_says_so() {
    let rig = rig(Platform::Linux, Arch::X64, false).await;
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        feed("1.0.34", "Kairo-1.0.34.AppImage", b"x"),
    );
    assert!(
        matches!(rig.service.check().await, UpdateStatus::Available { .. }),
        "a deb install still hears about updates"
    );
    assert_eq!(
        message(&rig.service.download().await),
        "This Kairo was installed from a package. Download the new version from https://kairopresenter.com and install it."
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let rig = self::rig(Platform::Linux, Arch::X64, true).await;
        let bytes = payload();
        rig.server.route(
            &format!("{RELEASES}/latest/download/latest-linux.yml"),
            feed("1.0.34", "Kairo-1.0.34.AppImage", &bytes),
        );
        rig.server.route(
            &format!("{RELEASES}/download/v1.0.34/Kairo-1.0.34.AppImage"),
            bytes,
        );
        rig.service.check().await;
        rig.service.download().await;
        let folder = rig.appimage.parent().unwrap().to_owned();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        let released = Arc::new(AtomicBool::new(false));
        let r = released.clone();
        rig.service.set_before_install(move || {
            let r = r.clone();
            async move { r.store(true, Ordering::SeqCst) }
        });
        assert_eq!(rig.service.install(true).await, InstallOutcome::Ignored);
        assert!(
            !released.load(Ordering::SeqCst),
            "hardware is kept when the install cannot happen"
        );
        assert_eq!(
            message(&rig.service.status()),
            format!(
                "Kairo can't update itself in {}. Download the new version from https://kairopresenter.com.",
                folder.display()
            )
        );
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[tokio::test]
async fn a_feed_pointing_elsewhere_is_refused() {
    let rig = rig(Platform::Linux, Arch::X64, true).await;
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest-linux.yml"),
        feed("1.0.34", "https://evil.example/Kairo-1.0.34.AppImage", b"x"),
    );
    rig.service.check().await;
    assert_eq!(
        message(&rig.service.download().await),
        "Could not download the update. Please try again."
    );
    assert!(
        rig.server
            .hits
            .lock()
            .unwrap()
            .iter()
            .all(|h| !h.contains("evil"))
    );
}

#[tokio::test]
async fn windows_stages_the_verified_installer() {
    let rig = rig(Platform::Windows, Arch::X64, false).await;
    let bytes = payload();
    rig.server.route(
        &format!("{RELEASES}/latest/download/latest.yml"),
        feed("1.0.34", "Kairo-Setup-1.0.34.exe", &bytes),
    );
    rig.server.route(
        &format!("{RELEASES}/download/v1.0.34/Kairo-Setup-1.0.34.exe"),
        bytes.clone(),
    );
    rig.service.check().await;
    assert!(matches!(
        rig.service.download().await,
        UpdateStatus::Downloaded { .. }
    ));
    let staged: Vec<PathBuf> = std::fs::read_dir(&rig.temp)
        .unwrap()
        .flat_map(|d| std::fs::read_dir(d.unwrap().path()).unwrap())
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].file_name().unwrap(), "Kairo-Setup-1.0.34.exe");
    assert_eq!(std::fs::read(&staged[0]).unwrap(), bytes);
}

#[tokio::test]
async fn unpackaged_builds_do_nothing_and_simulation_walks_every_state() {
    let rig = rig(Platform::Linux, Arch::X64, true).await;
    let off = UpdaterService::new(UpdaterConfig {
        packaged: false,
        ..base_config(&rig)
    });
    off.init().await;
    assert_eq!(
        off.check().await,
        UpdateStatus::Idle {
            current_version: Some("1.0.33".into())
        }
    );
    assert!(matches!(off.download().await, UpdateStatus::Idle { .. }));
    assert_eq!(off.install(true).await, InstallOutcome::Ignored);
    assert!(rig.server.hits.lock().unwrap().is_empty());

    let sim = UpdaterService::new(UpdaterConfig {
        packaged: false,
        simulate: true,
        ..base_config(&rig)
    });
    let seen: Arc<Mutex<Vec<UpdateStatus>>> = Arc::default();
    let s = seen.clone();
    sim.on_change(move |status| s.lock().unwrap().push(status.clone()));
    // Download before a check does nothing.
    assert!(matches!(sim.download().await, UpdateStatus::Idle { .. }));
    sim.init().await;
    assert_eq!(
        sim.status(),
        UpdateStatus::Available {
            version: "1.0.34".into(),
            notes: Some("Simulated release notes.".into())
        }
    );
    assert!(matches!(
        sim.download().await,
        UpdateStatus::Downloaded { .. }
    ));
    let downloading = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|s| matches!(s, UpdateStatus::Downloading { .. }))
        .count();
    assert_eq!(downloading, 21, "0 to 100 in steps of 5");
    assert_eq!(sim.install(true).await, InstallOutcome::Ignored);
    assert_eq!(
        sim.status(),
        UpdateStatus::Idle {
            current_version: Some("1.0.33".into())
        }
    );
    assert_eq!(
        seen.lock().unwrap().last(),
        Some(&UpdateStatus::Idle {
            current_version: Some("1.0.34".into())
        })
    );
    assert!(
        rig.server.hits.lock().unwrap().is_empty(),
        "the simulator never touches the network"
    );
    assert!(
        !kairo_updater::simulation_requested(true),
        "never in a packaged build"
    );
}
