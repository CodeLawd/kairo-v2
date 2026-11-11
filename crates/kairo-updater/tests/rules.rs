//! `tests/mac-self-update.test.ts`, `tests/updater-errors.test.ts`, and the
//! feed reader.

use kairo_updater::feed::{
    Arch, Platform, ReleaseSource, UpdateFile, feed_file, parse_feed, pick_linux_appimage,
    pick_mac_zip, pick_windows_installer, release_notes_from_atom,
};
use kairo_updater::install::{
    bundle_from_executable, designated_requirement, is_adhoc_signature, windows_installer_args,
};
use kairo_updater::{Operation, update_error_message};
use std::path::{Path, PathBuf};

fn file(url: &str) -> UpdateFile {
    UpdateFile {
        url: url.into(),
        sha512: "x".into(),
        size: Some(1),
    }
}

#[test]
fn updater_failures_show_a_helpful_message() {
    let error = r#"504 "method: GET url: https://github.com/example/latest-mac.yml\n Data: <html>Gateway Time-out</html> Headers: secret""#;
    assert_eq!(
        update_error_message(error, Operation::Check),
        "Could not check for updates. Please check your connection and try again."
    );
    assert_eq!(
        update_error_message(error, Operation::Download),
        "Could not download the update. Please check your connection and try again."
    );
    assert_eq!(
        update_error_message("unexpected failure", Operation::Check),
        "Could not check for updates. Please try again."
    );
    for network in [
        "HTTP 429",
        "ECONNRESET",
        "getaddrinfo ENOTFOUND",
        "net::ERR_FAILED",
        "timed out",
        "Network down",
        "408",
        "you are offline",
    ] {
        assert!(
            update_error_message(network, Operation::Check).ends_with("connection and try again."),
            "{network}"
        );
    }
    // `\b` is ASCII: digits glued to letters are not a status code.
    assert!(update_error_message("code5000", Operation::Check).ends_with("Please try again."));
    assert!(
        update_error_message("version 1.504.2", Operation::Check)
            .ends_with("connection and try again.")
    );
}

#[test]
fn picks_the_zip_for_this_architecture_never_a_dmg() {
    // The real latest-mac.yml from v1.0.2.
    let files = [
        file("Kairo-1.0.2-arm64-mac.zip"),
        file("Kairo-1.0.2-arm64.dmg"),
        file("Kairo-1.0.2-mac.zip"),
        file("Kairo-1.0.2.dmg"),
    ];
    assert_eq!(
        pick_mac_zip(&files, Arch::Arm64).unwrap().url,
        "Kairo-1.0.2-arm64-mac.zip"
    );
    assert_eq!(
        pick_mac_zip(&files, Arch::X64).unwrap().url,
        "Kairo-1.0.2-mac.zip"
    );
    let dmgs: Vec<_> = files
        .iter()
        .filter(|f| !f.url.ends_with(".zip"))
        .cloned()
        .collect();
    assert!(pick_mac_zip(&dmgs, Arch::Arm64).is_none());
    // An arm64-only release offers nothing to an Intel Mac.
    assert!(pick_mac_zip(&files[..2], Arch::X64).is_none());
}

#[test]
fn picks_windows_and_linux_files_by_architecture() {
    let win = [
        file("Kairo-Setup-1.0.3.exe"),
        file("Kairo-Setup-1.0.3.exe.blockmap"),
    ];
    assert_eq!(
        pick_windows_installer(&win, Arch::X64).unwrap().url,
        "Kairo-Setup-1.0.3.exe"
    );
    assert_eq!(
        pick_windows_installer(&win, Arch::Arm64).unwrap().url,
        "Kairo-Setup-1.0.3.exe",
        "x64 runs emulated"
    );
    let both = [
        file("Kairo-Setup-1.0.3-arm64.exe"),
        file("Kairo-Setup-1.0.3.exe"),
    ];
    assert_eq!(
        pick_windows_installer(&both, Arch::Arm64).unwrap().url,
        "Kairo-Setup-1.0.3-arm64.exe"
    );
    assert_eq!(
        pick_windows_installer(&both, Arch::X64).unwrap().url,
        "Kairo-Setup-1.0.3.exe"
    );
    let linux = [file("Kairo-1.0.3.AppImage"), file("kairo_1.0.3_amd64.deb")];
    assert_eq!(
        pick_linux_appimage(&linux, Arch::X64).unwrap().url,
        "Kairo-1.0.3.AppImage"
    );
    assert!(pick_linux_appimage(&linux, Arch::Arm64).is_none());
    assert!(pick_linux_appimage(&[file("Kairo-1.0.3-arm64.AppImage")], Arch::X64).is_none());
    assert_eq!(feed_file(Platform::Mac, Arch::Arm64), "latest-mac.yml");
    assert_eq!(feed_file(Platform::Windows, Arch::X64), "latest.yml");
    assert_eq!(feed_file(Platform::Linux, Arch::X64), "latest-linux.yml");
    assert_eq!(
        feed_file(Platform::Linux, Arch::Arm64),
        "latest-linux-arm64.yml"
    );
}

#[test]
fn builds_release_urls_and_refuses_anything_else() {
    let source = ReleaseSource::github("CodeLawd/kairo-presenter");
    assert_eq!(
        source
            .asset_url("1.0.3", "Kairo-1.0.3-arm64-mac.zip")
            .unwrap(),
        "https://github.com/CodeLawd/kairo-presenter/releases/download/v1.0.3/Kairo-1.0.3-arm64-mac.zip"
    );
    let absolute = "https://github.com/CodeLawd/kairo-presenter/releases/download/v1.0.3/Kairo.zip";
    assert_eq!(source.asset_url("1.0.3", absolute).unwrap(), absolute);
    assert!(
        source
            .asset_url("1.0.3", "https://evil.example/Kairo.zip")
            .unwrap_err()
            .contains("unexpected address")
    );
    assert!(
        source
            .asset_url("1.0.3", "HTTP://evil.example/Kairo.zip")
            .unwrap_err()
            .contains("unexpected address")
    );
    assert!(
        source
            .asset_url("1.0.3", "../secrets.zip")
            .unwrap_err()
            .contains("Unexpected update file")
    );
    assert!(
        source
            .asset_url("1.0.3", "a b.zip")
            .unwrap_err()
            .contains("Unexpected update file")
    );
    assert!(
        source
            .asset_url("1.0.3/../x", "a.zip")
            .unwrap_err()
            .contains("Unexpected version")
    );
    assert_eq!(
        source.feed_url("latest-mac.yml"),
        "https://github.com/CodeLawd/kairo-presenter/releases/latest/download/latest-mac.yml"
    );
}

#[test]
fn reads_an_adhoc_signature_and_a_designated_requirement() {
    assert!(is_adhoc_signature(
        "Executable=/Applications/Kairo.app\nSignature=adhoc\nTeamIdentifier=not set"
    ));
    assert!(!is_adhoc_signature(
        "Authority=Developer ID Application: Kairo (ABCDE12345)\nTeamIdentifier=ABCDE12345"
    ));
    assert_eq!(
        designated_requirement("Executable=/x\ndesignated => identifier \"com.kairo.app\" and anchor apple generic and certificate leaf[subject.OU] = ABCDE12345\n").as_deref(),
        Some("identifier \"com.kairo.app\" and anchor apple generic and certificate leaf[subject.OU] = ABCDE12345")
    );
    assert_eq!(designated_requirement("nothing"), None);
}

#[test]
fn finds_the_bundle_from_the_executable_path() {
    assert_eq!(
        bundle_from_executable(Path::new("/Applications/Kairo.app/Contents/MacOS/Kairo")),
        Some(PathBuf::from("/Applications/Kairo.app"))
    );
    assert_eq!(
        bundle_from_executable(Path::new("/usr/local/bin/node")),
        None
    );
    assert_eq!(
        bundle_from_executable(Path::new("/A/Kairo.app/Contents/MacOS/sub/Kairo")),
        None
    );
}

#[test]
fn windows_installer_arguments_match_electron_updater() {
    assert_eq!(windows_installer_args(true), ["--updated", "--force-run"]);
    assert_eq!(windows_installer_args(false), ["--updated", "/S"]);
}

#[cfg(unix)]
#[test]
fn the_swap_script_replaces_the_bundle_and_puts_the_old_one_back_on_failure() {
    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("swap.sh");
    std::fs::write(&script, kairo_updater::install::MAC_SWAP_SCRIPT).unwrap();
    let installed = root.path().join("Applications").join("Kairo.app");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(installed.join("version"), "old").unwrap();
    let staging = root.path().join("staging");
    let fresh = staging.join("Kairo.app");
    std::fs::create_dir_all(&fresh).unwrap();
    std::fs::write(fresh.join("version"), "new").unwrap();
    // PID 999999 is not running, so it swaps straight away; "0" = no relaunch.
    let ok = std::process::Command::new("/bin/bash")
        .arg(&script)
        .args(["999999"])
        .arg(&installed)
        .arg(&fresh)
        .arg("0")
        .status()
        .unwrap();
    assert!(ok.success());
    assert_eq!(
        std::fs::read_to_string(installed.join("version")).unwrap(),
        "new"
    );
    assert!(!staging.exists(), "staging folder is cleaned up");
    let left: Vec<_> = std::fs::read_dir(root.path().join("Applications"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["Kairo.app"], "no backup left behind");
    // A new bundle that does not exist: the old one must survive.
    std::process::Command::new("/bin/bash")
        .arg(&script)
        .arg("999999")
        .arg(&installed)
        .arg(root.path().join("missing").join("Kairo.app"))
        .arg("0")
        .status()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(installed.join("version")).unwrap(),
        "new"
    );
}

#[test]
fn parses_a_real_mac_feed() {
    let feed = parse_feed(include_str!("fixtures/latest-mac-1.0.27.yml")).unwrap();
    assert_eq!(feed.version, "1.0.27");
    assert_eq!(feed.files.len(), 2);
    assert_eq!(feed.files[0].url, "Kairo-1.0.27-arm64-mac.zip");
    assert_eq!(feed.files[0].size, Some(116_433_567));
    assert!(feed.files[0].sha512.ends_with("=="));
    assert_eq!(
        feed.release_date.as_deref(),
        Some("2026-11-03T22:53:51.346Z")
    );
    assert_eq!(
        pick_mac_zip(&feed.files, Arch::Arm64).unwrap().url,
        "Kairo-1.0.27-arm64-mac.zip"
    );
}

#[test]
fn parses_windows_and_linux_feeds_and_refuses_broken_ones() {
    let win = "version: 1.0.34\nfiles:\n  - url: Kairo-Setup-1.0.34.exe\n    sha512: \"abc+/==\"\n    size: 99\n    isAdminRightsRequired: false\npath: Kairo-Setup-1.0.34.exe\nsha512: abc+/==\nreleaseDate: '2026-11-10T00:00:00.000Z'\n";
    let feed = parse_feed(win).unwrap();
    assert_eq!(
        (feed.files[0].url.as_str(), feed.files[0].sha512.as_str()),
        ("Kairo-Setup-1.0.34.exe", "abc+/==")
    );
    let linux = "version: '1.0.34'\nfiles:\n  - url: Kairo-1.0.34.AppImage\n    sha512: def\n    size: 5\n    blockMapSize: 1\nreleaseNotes: |\n  Fixes.\n\n  More fixes.\nstagingPercentage: 50\n";
    let feed = parse_feed(linux).unwrap();
    assert_eq!(feed.version, "1.0.34");
    assert_eq!(feed.release_notes.as_deref(), Some("Fixes.\n\nMore fixes."));
    // Old feeds with only `path` and `sha512`.
    let old = parse_feed("version: 1.0.1\npath: Kairo-1.0.1-mac.zip\nsha512: zz\n").unwrap();
    assert_eq!(
        old.files,
        [UpdateFile {
            url: "Kairo-1.0.1-mac.zip".into(),
            sha512: "zz".into(),
            size: None
        }]
    );
    for broken in [
        "",
        "files:\n  - url: a.zip\n",
        "version: 1\nfiles:\n  - url: a.zip\n",
        "version: 1\nfiles:\n  - url: a\n    sha512: b\n    size: big\n",
        "<html>Not Found</html>",
        "version: 'oops\n",
    ] {
        assert!(parse_feed(broken).is_err(), "{broken:?}");
    }
    assert_eq!(
        parse_feed("version: 'it''s'\nreleaseNotes: \"a\\nb\"\n")
            .unwrap()
            .release_notes
            .as_deref(),
        Some("a\nb")
    );
}

#[test]
fn release_notes_come_from_the_atom_entry_for_the_tag() {
    let atom = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry><link rel="alternate" type="text/html" href="https://github.com/CodeLawd/kairo-presenter/releases/tag/v1.0.35"/><content type="html">&lt;p&gt;Newer&lt;/p&gt;</content></entry>
  <entry><link rel="alternate" type="text/html" href="https://github.com/CodeLawd/kairo-presenter/releases/tag/v1.0.34"/><content type="html">&lt;p&gt;Fixes&lt;/p&gt;</content></entry>
</feed>"#;
    assert_eq!(
        release_notes_from_atom(atom, "1.0.34").as_deref(),
        Some("<p>Fixes</p>")
    );
    assert_eq!(
        release_notes_from_atom(atom, "1.0.3"),
        None,
        "tag must match exactly"
    );
    assert_eq!(release_notes_from_atom("not xml", "1.0.34"), None);
}
