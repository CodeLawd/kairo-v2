//! macOS N to N+1 with locally built, ad-hoc signed bundles: the installed
//! N runs the real updater against a local feed, downloads and verifies the
//! N+1 zip, quits, and the detached swap replaces the bundle and reopens it.
//! Also: a zip holding a different app, and one with a broken signature,
//! are never swapped in.
#![cfg(target_os = "macos")]

use base64::Engine;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const RELEASES: &str = "/CodeLawd/kairo-presenter/releases";

async fn serve(routes: HashMap<String, Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes = Arc::new(routes);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let routes = routes.clone();
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
                let path = head.split_whitespace().nth(1).unwrap_or("/");
                let (status, body) = routes
                    .get(path)
                    .map_or((404, b"Not Found".to_vec()), |b| (200, b.clone()));
                let header = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(&body).await;
            });
        }
    });
    base
}

fn probe() -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args(["build", "-p", "kairo-updater", "--example", "update_probe"])
        .status()
        .unwrap();
    assert!(status.success());
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    target.join("debug/examples/update_probe")
}

/// A signed `Kairo.app` at `dir` with `version` and `bundle_id`.
fn bundle(dir: &Path, version: &str, bundle_id: &str, probe: &Path) -> PathBuf {
    let app = dir.join("Kairo.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleExecutable</key><string>Kairo</string>
<key>CFBundleName</key><string>Kairo</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>{version}</string>
<key>LSBackgroundOnly</key><true/>
</dict></plist>"#
        ),
    )
    .unwrap();
    std::fs::copy(probe, app.join("Contents/MacOS/Kairo")).unwrap();
    let signed = Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app)
        .output()
        .unwrap();
    assert!(
        signed.status.success(),
        "{}",
        String::from_utf8_lossy(&signed.stderr)
    );
    app
}

fn zip(app: &Path, out: &Path) -> Vec<u8> {
    let ok = Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(app)
        .arg(out)
        .status()
        .unwrap();
    assert!(ok.success());
    std::fs::read(out).unwrap()
}

fn feed(version: &str, file: &str, bytes: &[u8]) -> Vec<u8> {
    let sha = base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes));
    format!("version: {version}\nfiles:\n  - url: {file}\n    sha512: {sha}\n    size: {}\npath: {file}\nsha512: {sha}\nreleaseDate: '2026-11-10T00:00:00.000Z'\n", bytes.len()).into_bytes()
}

fn plist_version(app: &Path) -> String {
    let out = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleShortVersionString"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn wait_for(what: &str, limit: Duration, done: impl Fn() -> bool) -> Duration {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
    start.elapsed()
}

fn arch_zip(version: &str) -> String {
    if cfg!(target_arch = "aarch64") {
        format!("Kairo-{version}-arm64-mac.zip")
    } else {
        format!("Kairo-{version}-mac.zip")
    }
}

struct Run {
    log: String,
    exit: std::process::ExitStatus,
}

fn run_installed(app: &Path, base: &str, root: &Path) -> Run {
    let log = root.join("probe.log");
    let exit = Command::new(app.join("Contents/MacOS/Kairo"))
        .arg(format!("{base}{RELEASES}"))
        .arg(&log)
        .status()
        .unwrap();
    Run {
        log: std::fs::read_to_string(&log).unwrap_or_default(),
        exit,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn n_to_n_plus_one_with_local_builds() {
    let probe = probe();
    let root = tempfile::tempdir().unwrap();
    let apps = root.path().join("Applications");
    let installed = bundle(&apps, "1.0.33", "com.kairo.app", &probe);
    let build = root.path().join("build");
    let next = bundle(&build, "1.0.34", "com.kairo.app", &probe);
    let file = arch_zip("1.0.34");
    let bytes = zip(&next, &build.join(&file));
    let base = serve(HashMap::from([
        (
            format!("{RELEASES}/latest/download/latest-mac.yml"),
            feed("1.0.34", &file, &bytes),
        ),
        (format!("{RELEASES}/download/v1.0.34/{file}"), bytes),
    ]))
    .await;

    let started = Instant::now();
    let run = tokio::task::spawn_blocking({
        let (installed, base, root) = (installed.clone(), base.clone(), root.path().to_owned());
        move || run_installed(&installed, &base, &root)
    })
    .await
    .unwrap();
    assert!(run.exit.success(), "{}", run.log);
    assert!(run.log.contains("Downloaded"), "{}", run.log);
    assert!(
        !run.log.contains("install "),
        "the app quit to install: {}",
        run.log
    );
    // The detached installer may wait up to 60 seconds for the old PID to
    // disappear. Poll long enough to cover that wait and signature checks.
    let swapped = wait_for("the swap", Duration::from_secs(90), || {
        plist_version(&installed) == "1.0.34"
    });
    let verify = Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&installed)
        .output()
        .unwrap();
    assert!(
        verify.status.success(),
        "{}",
        String::from_utf8_lossy(&verify.stderr)
    );
    wait_for("the relaunch", Duration::from_secs(30), || {
        apps.join("relaunched-1.0.34").exists()
    });
    let left: Vec<String> = std::fs::read_dir(&apps)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        left.iter().all(|n| !n.contains("kairo-old")),
        "no backup left: {left:?}"
    );
    let quarantine = Command::new("xattr")
        .arg("-p")
        .arg("com.apple.quarantine")
        .arg(&installed)
        .output()
        .unwrap();
    assert!(!quarantine.status.success(), "no quarantine flag");
    eprintln!(
        "N to N+1: check, download, verify, quit {:?}; swap seen {swapped:?} after",
        started.elapsed() - swapped
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn another_app_or_a_broken_signature_is_never_swapped_in() {
    let probe = probe();
    for case in ["other-app", "broken-signature"] {
        let root = tempfile::tempdir().unwrap();
        let installed = bundle(
            &root.path().join("Applications"),
            "1.0.33",
            "com.kairo.app",
            &probe,
        );
        let build = root.path().join("build");
        let id = if case == "other-app" {
            "com.other.app"
        } else {
            "com.kairo.app"
        };
        let next = bundle(&build, "1.0.34", id, &probe);
        if case == "broken-signature" {
            // Changed after signing: the seal no longer matches.
            std::fs::write(next.join("Contents/MacOS/Kairo"), b"#!/bin/sh\n").unwrap();
        }
        let file = arch_zip("1.0.34");
        let bytes = zip(&next, &build.join(&file));
        let base = serve(HashMap::from([
            (
                format!("{RELEASES}/latest/download/latest-mac.yml"),
                feed("1.0.34", &file, &bytes),
            ),
            (format!("{RELEASES}/download/v1.0.34/{file}"), bytes),
        ]))
        .await;
        let run = tokio::task::spawn_blocking({
            let (installed, base, root) = (installed.clone(), base.clone(), root.path().to_owned());
            move || run_installed(&installed, &base, &root)
        })
        .await
        .unwrap();
        assert!(
            run.log
                .contains("Could not download the update. Please try again."),
            "{case}: {}",
            run.log
        );
        assert!(run.log.contains("install Ignored"), "{case}: {}", run.log);
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            plist_version(&installed),
            "1.0.33",
            "{case}: N is untouched"
        );
    }
}
