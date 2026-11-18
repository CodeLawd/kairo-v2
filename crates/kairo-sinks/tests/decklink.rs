//! The DeckLink provider and helper client, ported from Electron's
//! `decklink-inventory.test.ts`, `decklink-paths.test.ts`,
//! `native/decklink/tests/client.test.ts` (fake helpers are shell scripts
//! here) and `probe.test.mjs` / `frame-input.test.mjs` (the real helper).
#![cfg(unix)]

use kairo_sinks::decklink::*;
#[cfg(target_os = "macos")]
use kairo_sinks::video_devices::Provider;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[cfg(target_os = "macos")]
fn prebuilt() -> PathBuf {
    repo().join("native/decklink/prebuilt/decklink-helper")
}

fn script(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("helper");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(target_os = "macos")]
fn provider(dir: &Path, body: &str) -> DeckLinkProvider {
    // The temp folder stands in for the installed runtime.
    DeckLinkProvider::new(script(dir, body), dir.to_path_buf())
}

// ─── paths ──────────────────────────────────────────────────────────────────

#[test]
fn card_outputs_are_on_in_every_macos_build_unless_explicitly_turned_off() {
    assert!(enabled("macos", None));
    assert!(enabled("macos", Some("1")));
    assert!(!enabled("macos", Some("0")));
    assert!(!enabled("windows", None));
    assert!(!enabled("linux", None));
}

#[test]
fn uses_the_bundled_helper_when_packaged_and_the_local_build_in_development() {
    let exe = Path::new("/App.app/Contents/MacOS");
    let repo = Path::new("/repo");
    let bundled = exe.join("../Resources/decklink/decklink-helper");
    assert_eq!(helper_path(exe, Some(repo), |p| p == bundled), bundled);
    let built = repo.join("native/decklink/build/decklink-helper");
    assert_eq!(helper_path(exe, Some(repo), |p| p == built), built);
    let committed = repo.join("native/decklink/prebuilt/decklink-helper");
    assert_eq!(helper_path(exe, Some(repo), |p| p == committed), committed);
    // None exists: the last candidate, reported as "not included".
    assert_eq!(helper_path(exe, Some(repo), |_| false), committed);
    assert_eq!(helper_path(exe, None, |_| false), bundled);
}

// ─── inventory ──────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn compares_desktop_video_versions_against_the_15_3_1_floor() {
    for old in ["12.9", "14.4.1", "15.3", "15.2.9"] {
        assert!(driver_too_old(Some(old)), "{old}");
    }
    for ok in ["15.3.1", "15.4", "16.4", "", "unknown"] {
        assert!(!driver_too_old(Some(ok)), "{ok}");
    }
    assert!(!driver_too_old(None));
}

#[cfg(target_os = "macos")]
#[test]
fn keeps_all_four_reported_channels_and_preserves_inactive_profile_explanations() {
    let dir = tempfile::tempdir().unwrap();
    let reason =
        "Inactive in the current card profile. Check connector mapping in Desktop Video Setup.";
    let ports: Vec<serde_json::Value> = (0..4)
        .map(|i| {
            serde_json::json!({
                "id": format!("decklink:100:{i}"), "provider": "decklink",
                "deviceName": "DeckLink Duo 2", "portName": format!("DeckLink Duo 2 ({})", i + 1),
                "supportsKeyFill": false,
                "modes": if i < 2 { serde_json::json!([{"id":"decklink:1","label":"1080p25","width":1920,"height":1080,"frameRateN":25,"frameRateD":1,"interlaced":false}]) } else { serde_json::json!([]) },
                "unavailableReason": if i < 2 { serde_json::Value::Null } else { reason.into() },
            })
        })
        .collect();
    let inventory = serde_json::json!({"version": "15.3.1", "ports": ports});
    let mut provider = provider(
        dir.path(),
        &format!("cat <<'INVENTORY'\n{inventory}\nINVENTORY"),
    );
    let listed = provider.list_ports().unwrap();
    assert_eq!(listed.len(), 4);
    assert_eq!(
        listed
            .iter()
            .filter(|p| p.unavailable_reason.is_none())
            .count(),
        2
    );
    assert_eq!(listed[2].unavailable_reason.as_deref(), Some(reason));
    assert_eq!(listed[3].unavailable_reason.as_deref(), Some(reason));
    assert_eq!(provider.status().version.as_deref(), Some("15.3.1"));
}

#[cfg(target_os = "macos")]
#[test]
fn names_the_driver_version_when_desktop_video_is_too_old() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(dir.path(), r#"echo '{"version":"12.9","ports":[]}'"#);
    let error = provider.list_ports().unwrap_err();
    assert!(
        error.contains("Desktop Video 12.9 is too old") && error.contains("15.3.1"),
        "{error}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn reports_the_helper_message_rather_than_the_command_line_when_listing_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(
        dir.path(),
        "echo 'Read card profile (requires Desktop Video 15.3.1 or newer): SDK error -1' >&2; exit 1",
    );
    assert_eq!(
        provider.list_ports().unwrap_err(),
        "Read card profile (requires Desktop Video 15.3.1 or newer): SDK error -1"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn reports_compatibility_mode_for_a_supported_older_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(
        dir.path(),
        r#"echo '{"version":"15.3.1","compatibilityMode":true,"ports":[]}'"#,
    );
    provider.list_ports().unwrap();
    let driver = provider.status().driver.unwrap();
    assert_eq!(
        driver.state,
        kairo_domain::video_devices::DriverState::Compatibility
    );
    assert!(driver.message.contains("15.3.1") && driver.message.contains("compatibility mode"));
}

#[cfg(target_os = "macos")]
#[test]
fn retains_the_unsupported_version_and_update_advice_when_enumeration_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(
        dir.path(),
        r#"if [ "$1" = '--version' ]; then echo '{"version":"14.4"}'; else echo 'Read card profile: SDK error -1' >&2; exit 1; fi"#,
    );
    let error = provider.list_ports().unwrap_err();
    assert!(
        error.contains("Desktop Video 14.4") && error.contains("Update"),
        "{error}"
    );
    let status = provider.status();
    assert_eq!(status.version.as_deref(), Some("14.4"));
    assert_eq!(
        status.driver.unwrap().state,
        kairo_domain::video_devices::DriverState::Unsupported
    );
}

#[cfg(target_os = "macos")]
#[test]
fn does_not_recommend_updating_a_current_driver_because_no_cards_were_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(
        dir.path(),
        r#"echo '{"version":"16.4","compatibilityMode":false,"ports":[]}'"#,
    );
    provider.list_ports().unwrap();
    let driver = provider.status().driver.unwrap();
    assert_eq!(
        driver.state,
        kairo_domain::video_devices::DriverState::Current
    );
    assert!(!driver.message.to_lowercase().contains("update"));
}

#[cfg(target_os = "macos")]
#[test]
fn preserves_a_supported_driver_scan_failure_without_recommending_an_update() {
    for version in ["15.3.1", "16.4"] {
        let dir = tempfile::tempdir().unwrap();
        let mut provider = provider(
            dir.path(),
            &format!(
                r#"if [ "$1" = '--version' ]; then echo '{{"version":"{version}"}}'; else echo 'Read modes: SDK error -1' >&2; exit 1; fi"#
            ),
        );
        assert_eq!(
            provider.list_ports().unwrap_err(),
            "Read modes: SDK error -1"
        );
        let status = provider.status();
        assert_eq!(status.version.as_deref(), Some(version));
        let driver = status.driver.unwrap();
        assert_eq!(
            driver.state,
            kairo_domain::video_devices::DriverState::Current
        );
        assert!(
            !driver.message.contains("pdate") && !driver.message.contains("compatibility mode")
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn invalid_inventory_and_missing_pieces_are_reasons_not_panics() {
    let dir = tempfile::tempdir().unwrap();
    let mut garbled = provider(dir.path(), "echo 'not json'");
    assert_eq!(
        garbled.list_ports().unwrap_err(),
        "Invalid DeckLink inventory"
    );
    let mut no_ports = provider(dir.path(), r#"echo '{"version":"16.4"}'"#);
    assert_eq!(
        no_ports.list_ports().unwrap_err(),
        "Invalid DeckLink inventory"
    );
    let missing_helper = DeckLinkProvider::new(dir.path().join("absent"), dir.path().to_path_buf());
    assert_eq!(
        missing_helper.status().reason.as_deref(),
        Some("DeckLink native helper is not included in this build")
    );
    let missing_runtime = DeckLinkProvider::new(prebuilt(), dir.path().join("absent"));
    let status = missing_runtime.status();
    assert!(!status.available);
    assert_eq!(
        status.driver.unwrap().state,
        kairo_domain::video_devices::DriverState::Missing
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ports_without_modes_explain_why_and_key_fill_and_unknown_ports_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(
        dir.path(),
        r#"echo '{"version":"16.4","ports":[{"id":"decklink:1:0","provider":"decklink","deviceName":"Card","portName":"SDI","supportsKeyFill":true,"modes":[],"unavailableReason":null}]}'"#,
    );
    let ports = provider.list_ports().unwrap();
    assert_eq!(
        ports[0].unavailable_reason.as_deref(),
        Some("No output modes available in the current device profile")
    );
    let error =
        |r: Result<Box<dyn kairo_sinks::video_devices::OutputSink>, String>| r.err().unwrap();
    assert_eq!(
        error(provider.open_output("decklink:1:0", "decklink:1", true)),
        "DeckLink key/fill is not qualified in this build"
    );
    assert_eq!(
        error(provider.open_output("decklink:9:9", "decklink:1", false)),
        "Device disconnected"
    );
    assert_eq!(
        error(provider.open_output("decklink:1:0", "decklink:1", false)),
        "Unsupported video mode"
    );
}

/// A helper that lists one 1080p port and plays it, attaching the ring
/// (`ring_ok`) or not, and copying stdin to `out`.
#[cfg(target_os = "macos")]
fn playing(dir: &Path, ring_ok: bool) -> DeckLinkProvider {
    let ring = if ring_ok {
        r#"{"event":"ring","ok":true}"#
    } else {
        r#"{"event":"ring","ok":false,"reason":"Frame ring raster does not match the output mode"}"#
    };
    let out = dir.join("out");
    provider(
        dir,
        &format!(
            r#"case "$1" in
--list) echo '{{"version":"16.4","ports":[{{"id":"decklink:1:0","provider":"decklink","deviceName":"Card","portName":"SDI","supportsKeyFill":false,"modes":[{{"id":"decklink:1","label":"1080p25","width":1920,"height":1080,"frameRateN":25,"frameRateD":1,"interlaced":false}}],"unavailableReason":null}}]}}';;
--play-shm) echo "$4" > '{dir}/ring-name'; echo '{ring}'; echo '{{"event":"started","port":"'"$2"'"}}'; cat > '{out}';;
--play) echo '{{"event":"started","port":"'"$2"'"}}'; cat > '{out}';;
esac"#,
            dir = dir.display(),
            out = out.display()
        ),
    )
}

#[cfg(target_os = "macos")]
#[test]
fn an_open_card_gets_a_frame_ring_at_the_mode_raster_when_the_helper_attaches_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = playing(dir.path(), true);
    provider.list_ports().unwrap();
    let card = provider
        .open_output("decklink:1:0", "decklink:1", false)
        .unwrap();
    let ring = card.frame_ring().expect("ring attached");
    assert_eq!(ring.raster(), (1920, 1080));
    let name = std::fs::read_to_string(dir.path().join("ring-name")).unwrap();
    assert!(
        name.trim().starts_with("/kairo-dl-") && name.trim().len() <= 30,
        "{name}"
    );
    card.close();
    card.close();
}

#[cfg(target_os = "macos")]
#[test]
fn a_ring_the_helper_refuses_falls_back_to_cpu_frames() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = playing(dir.path(), false);
    provider.list_ports().unwrap();
    let card = provider
        .open_output("decklink:1:0", "decklink:1", false)
        .unwrap();
    assert!(card.frame_ring().is_none());
    card.set_frame(&[1, 2, 3, 4], 1, 1).unwrap();
    card.close();
    let bytes = std::fs::read(dir.path().join("out")).unwrap();
    assert_eq!(bytes, [1, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 4]);
}

// ─── client.test.ts ─────────────────────────────────────────────────────────

fn spawn(dir: &Path, body: &str, timeout: Duration) -> Result<Playback, String> {
    // Pass the fixture as input to the shell so parallel forks cannot
    // block exec with a briefly inherited writable script descriptor.
    Playback::spawn(Command::new("/bin/sh").arg(script(dir, body)), timeout)
}

const STARTED: &str = r#"echo '{"event":"started","port":"decklink:1:0"}'"#;

#[test]
fn copies_frame_ownership_and_writes_a_bounded_binary_packet() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let playback = spawn(
        dir.path(),
        &format!("{STARTED}; cat > '{}'", out.display()),
        Duration::from_secs(5),
    )
    .unwrap();
    let mut frame = vec![9u8; 2 * 4];
    playback.set_frame(&frame, 2, 1).unwrap();
    frame.fill(0); // The caller may reuse its buffer.
    assert!(playback.set_frame(&[0; 3], 1, 1).is_err());
    assert!(playback.set_frame(&[0; 4], 4097, 1).is_err());
    playback.close();
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..12], &[2, 0, 0, 0, 1, 0, 0, 0, 8, 0, 0, 0]);
    assert_eq!(&bytes[12..], &[9u8; 8]);
    assert_eq!(
        playback.set_frame(&[0; 4], 1, 1).unwrap_err(),
        "DeckLink output closed"
    );
}

#[test]
fn a_native_crash_is_reported_asynchronously_and_blocks_later_frames() {
    let dir = tempfile::tempdir().unwrap();
    let playback = spawn(
        dir.path(),
        &format!("{STARTED}; sleep 0.2; kill -SEGV $$"),
        Duration::from_secs(5),
    )
    .unwrap();
    let seen = Arc::new(Mutex::new(None));
    let s = seen.clone();
    playback.on_error(Box::new(move |e| *s.lock().unwrap() = Some(e)));
    let deadline = Instant::now() + Duration::from_secs(5);
    while seen.lock().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let error = seen.lock().unwrap().clone().expect("crash reported");
    assert!(error.contains("signal 11"), "{error}");
    assert_eq!(playback.set_frame(&[0; 4], 1, 1).unwrap_err(), error);
    // A listener added after the failure hears it at once.
    let late = Arc::new(Mutex::new(None));
    let l = late.clone();
    playback.on_error(Box::new(move |e| *l.lock().unwrap() = Some(e)));
    assert_eq!(late.lock().unwrap().as_ref(), Some(&error));
}

#[test]
fn busy_errors_preserve_the_native_explanation_before_ready() {
    let dir = tempfile::tempdir().unwrap();
    let error = spawn(
        dir.path(),
        "echo 'Output busy: another application is using this port' >&2; exit 1",
        Duration::from_secs(5),
    )
    .err()
    .unwrap();
    assert_eq!(error, "Output busy: another application is using this port");
}

#[test]
fn playback_callback_failures_reach_subscribers() {
    let dir = tempfile::tempdir().unwrap();
    let playback = spawn(
        dir.path(),
        &format!(r#"{STARTED}; sleep 0.2; echo '{{"event":"error","reason":"Schedule frame failed"}}'; cat > /dev/null"#),
        Duration::from_secs(5),
    )
    .unwrap();
    let seen = Arc::new(Mutex::new(None));
    let s = seen.clone();
    playback.on_error(Box::new(move |e| *s.lock().unwrap() = Some(e)));
    let deadline = Instant::now() + Duration::from_secs(5);
    while seen.lock().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some("Schedule frame failed")
    );
}

#[test]
fn backpressure_retains_only_the_newest_unsent_frame() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    // Frames larger than the pipe buffer, and a helper slow to read.
    let playback = spawn(
        dir.path(),
        &format!("{STARTED}; sleep 0.5; cat > '{}'", out.display()),
        Duration::from_secs(5),
    )
    .unwrap();
    let (w, h) = (512u32, 512u32);
    for value in [1u8, 2, 3] {
        playback
            .set_frame(&vec![value; (w * h * 4) as usize], w, h)
            .unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }
    std::thread::sleep(Duration::from_millis(1000));
    playback.close();
    let bytes = std::fs::read(&out).unwrap();
    let packet = 12 + (w * h * 4) as usize;
    assert_eq!(bytes.len(), 2 * packet, "the second frame was replaced");
    assert_eq!(bytes[12], 1);
    assert_eq!(bytes[packet + 12], 3);
}

#[test]
fn hung_startup_is_terminated_and_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let error = spawn(dir.path(), "sleep 30", Duration::from_millis(200))
        .err()
        .unwrap();
    assert_eq!(error, "DeckLink helper startup timed out");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn startup_timeout_force_kills_a_helper_that_ignores_sigterm() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let started = Instant::now();
    let error = spawn(
        dir.path(),
        &format!(
            "trap '' TERM; echo $$ > '{}'; while true; do sleep 0.1; done",
            pid_file.display()
        ),
        Duration::from_millis(1000),
    )
    .err()
    .unwrap();
    assert_eq!(error, "DeckLink helper startup timed out");
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_secs(5) && elapsed < Duration::from_secs(9),
        "{elapsed:?}"
    );
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    // SAFETY: probing whether the process exists.
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the helper is gone");
}

#[test]
fn reports_hardware_playback_samples_without_treating_them_as_failures() {
    let dir = tempfile::tempdir().unwrap();
    let playback = spawn(
        dir.path(),
        &format!(r#"{STARTED}; echo '{{"event":"stats","completed":250,"late":0,"dropped":0,"repeated":3,"received":247,"outputFps":25}}'; echo '{{"event":"stats","completed":-1,"late":0,"dropped":0,"repeated":0,"received":0,"outputFps":25}}'; cat > /dev/null"#),
        Duration::from_secs(5),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while playback.diagnostics().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100));
    let d = playback.diagnostics().expect("stats");
    assert_eq!(
        (d.completed, d.repeated, d.received, d.output_fps),
        (250.0, 3.0, 247.0, 25.0)
    );
    assert!(
        playback.set_frame(&[0; 4], 1, 1).is_ok(),
        "stats are not failures"
    );
    playback.close();
}

#[test]
fn an_invalid_helper_response_fails_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let error = spawn(
        dir.path(),
        "echo 'garbage'; sleep 5",
        Duration::from_secs(5),
    )
    .err()
    .unwrap();
    assert_eq!(error, "Invalid DeckLink helper response");
}

#[test]
fn close_escalates_to_sigkill_when_the_helper_does_not_release() {
    let dir = tempfile::tempdir().unwrap();
    let playback = spawn(
        dir.path(),
        &format!("{STARTED}; trap '' TERM; while true; do sleep 0.1; done"),
        Duration::from_secs(5),
    )
    .unwrap();
    let started = Instant::now();
    playback.close();
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_secs(4) && elapsed < Duration::from_secs(7),
        "{elapsed:?}"
    );
    // Quick when the helper releases on EOF.
    let quick = spawn(
        dir.path(),
        &format!("{STARTED}; cat > /dev/null"),
        Duration::from_secs(5),
    )
    .unwrap();
    let started = Instant::now();
    quick.close();
    assert!(started.elapsed() < Duration::from_secs(1));
}

// ─── the real helper ────────────────────────────────────────────────────────

#[test]
fn the_prebuilt_helper_matches_its_recorded_sources() {
    // `scripts/prepare-mac-natives.mjs`: path then text, CRLF folded.
    use sha2_lite::Sha256;
    let files = [
        "native/decklink/helper.cpp",
        "native/decklink/compatibility.h",
        "native/decklink/sdk-compatibility.h",
        "native/decklink/frame-input.h",
        "native/decklink/uyvy.h",
        "native/gpu-frames/ring.h",
        "native/decklink/build.mjs",
    ];
    let mut hash = Sha256::new();
    for file in files {
        hash.update(file.as_bytes());
        let text = std::fs::read_to_string(repo().join(file)).unwrap();
        hash.update(text.replace("\r\n", "\n").as_bytes());
    }
    let recorded =
        std::fs::read_to_string(repo().join("native/decklink/prebuilt/SOURCE_SHA256")).unwrap();
    assert_eq!(hash.hex(), recorded.trim());
}

#[cfg(target_os = "macos")]
#[test]
fn native_self_test_validates_clock_recovery_and_pixel_conversion() {
    let out = Command::new(prebuilt())
        .arg("--self-test")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        r#"{"ok":true}"#
    );
}

#[cfg(target_os = "macos")]
#[test]
fn invalid_arguments_never_acquire_hardware() {
    for args in [
        &["--probe", "invalid", "invalid", "-1"][..],
        &["--probe", "decklink:1:0", "decklink:1", "2oops"],
        &["--play-shm", "decklink:1:0", "decklink:1", "no-slash"],
        &[
            "--play-shm",
            "decklink:1:0",
            "decklink:1",
            "/a-ring-name-much-longer-than-thirty",
        ],
    ] {
        let out = Command::new(prebuilt()).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("Invalid"));
    }
}

/// With Desktop Video installed (no card needed): the real runtime answers.
#[cfg(target_os = "macos")]
#[test]
fn the_installed_runtime_lists_and_refuses_a_missing_port() {
    if !Path::new(RUNTIME).exists() {
        eprintln!("skipped: Blackmagic Desktop Video is not installed");
        return;
    }
    let mut provider = DeckLinkProvider::new(prebuilt(), PathBuf::from(RUNTIME));
    let ports = provider.list_ports().expect("inventory");
    let status = provider.status();
    assert!(status.available);
    assert!(status.version.is_some(), "{status:?}");
    eprintln!("Desktop Video {:?}, {} ports", status.version, ports.len());
    let error = Playback::spawn(
        Command::new(prebuilt()).args(["--play", "decklink:no-such-card", "decklink:1"]),
        Duration::from_secs(10),
    )
    .err()
    .unwrap();
    assert_eq!(error, "Device disconnected");
}

#[cfg(target_os = "macos")]
#[test]
fn frame_input_reads_full_and_partial_packets_without_starving_hd_playback() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("reader");
    let build = Command::new("xcrun")
        .args(["clang++", "-std=c++17", "-O2"])
        .arg(repo().join("native/decklink/tests/frame-input.cpp"))
        .arg("-o")
        .arg(&binary)
        .output();
    let Ok(build) = build else {
        eprintln!("skipped: no xcrun");
        return;
    };
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    // Four HD frames plus a partial component: bulk reads and EOF.
    let input = vec![37u8; 1920 * 1080 * 4 * 4 + 13];
    let path = dir.path().join("frames.bgra");
    std::fs::write(&path, &input).unwrap();
    let out = Command::new(&binary)
        .stdin(std::fs::File::open(&path).unwrap())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("{} {}", input.len(), input.len() * 37)
    );
    // CPU time: four frames inside their 160 ms budget at 25 fps.
    let cpu_ms: f64 = String::from_utf8_lossy(&out.stderr).trim().parse().unwrap();
    assert!(cpu_ms < 160.0, "four HD frames took {cpu_ms} ms of CPU");
    for (bytes, want) in [(17usize, "17 629"), (0, "0 0")] {
        let path = dir.path().join(format!("in-{bytes}"));
        std::fs::write(&path, vec![37u8; bytes]).unwrap();
        let out = Command::new(&binary)
            .stdin(std::fs::File::open(&path).unwrap())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), want);
    }
}

/// SHA-256, small and dependency-free, for the source-hash check.
mod sha2_lite {
    pub struct Sha256 {
        state: [u32; 8],
        buffer: Vec<u8>,
        length: u64,
    }
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    impl Sha256 {
        pub fn new() -> Self {
            Self {
                state: [
                    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
                    0x1f83d9ab, 0x5be0cd19,
                ],
                buffer: Vec::new(),
                length: 0,
            }
        }
        pub fn update(&mut self, data: &[u8]) {
            self.length += data.len() as u64;
            self.buffer.extend_from_slice(data);
            let whole = self.buffer.len() / 64 * 64;
            let blocks: Vec<u8> = self.buffer.drain(..whole).collect();
            for block in blocks.chunks(64) {
                self.compress(block);
            }
        }
        fn compress(&mut self, block: &[u8]) {
            let mut w = [0u32; 64];
            for (i, word) in block.chunks(4).enumerate() {
                w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let mut v = self.state;
            for i in 0..64 {
                let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
                let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
                let t1 = v[7]
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
                let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
                let t2 = s0.wrapping_add(maj);
                v = [
                    t1.wrapping_add(t2),
                    v[0],
                    v[1],
                    v[2],
                    v[3].wrapping_add(t1),
                    v[4],
                    v[5],
                    v[6],
                ];
            }
            for (s, x) in self.state.iter_mut().zip(v) {
                *s = s.wrapping_add(x);
            }
        }
        pub fn hex(mut self) -> String {
            let bits = self.length * 8;
            let mut pad = vec![0x80u8];
            while (self.buffer.len() + pad.len()) % 64 != 56 {
                pad.push(0);
            }
            pad.extend_from_slice(&bits.to_be_bytes());
            let length = self.length;
            self.update(&pad);
            self.length = length;
            self.state.iter().map(|w| format!("{w:08x}")).collect()
        }
    }
}
