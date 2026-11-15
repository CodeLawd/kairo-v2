//! A staged release directory, built from small stand-in files, through the
//! same checks the publish job runs.

use kairo_release::*;
use semver::Version;
use std::fs;
use std::path::Path;

fn write(dir: &Path, name: &str, body: &[u8]) {
    fs::write(dir.join(name), body).unwrap();
}

fn feed(dir: &Path, name: &str, version: &str, file: &str) {
    let p = payload(&dir.join(file)).unwrap();
    write(
        dir,
        name,
        format!(
            "version: {version}\nfiles:\n  - url: {file}\n    sha512: {}\n    size: {}\npath: {file}\nsha512: {}\nreleaseDate: '2026-11-15T12:00:00.000Z'\n",
            p.sha512, p.size, p.sha512
        )
        .as_bytes(),
    );
}

/// Every asset the matrix uploads, plus the feeds the publish job checks.
fn complete(dir: &Path) -> Version {
    let v = "2.0.0";
    for (name, body) in [
        ("Kairo-2.0.0-arm64.dmg", "arm dmg"),
        ("Kairo-2.0.0.dmg", "x64 dmg"),
        ("Kairo-2.0.0-arm64-mac.zip", "arm zip"),
        ("Kairo-2.0.0-mac.zip", "x64 zip"),
        ("Kairo-Setup-2.0.0.exe", "exe"),
        ("Kairo-Setup-2.0.0.exe.blockmap", "map"),
        ("Kairo-2.0.0.AppImage", "appimage"),
        ("kairo-presenter_2.0.0_amd64.deb", "deb"),
    ] {
        write(dir, name, body.as_bytes());
    }
    feed(dir, "latest.yml", v, "Kairo-Setup-2.0.0.exe");
    feed(dir, "latest-linux.yml", v, "Kairo-2.0.0.AppImage");
    let version = Version::new(2, 0, 0);
    write_mac_feed(dir, &version, "2026-11-15T12:00:00.000Z").unwrap();
    version
}

fn errors(dir: &Path, version: &Version) -> String {
    verify_stage(dir, version).unwrap_err().join("\n")
}

#[test]
fn a_complete_release_passes_and_gets_a_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    verify_stage(dir.path(), &version).unwrap();
    let manifest = sha256_manifest(dir.path()).unwrap();
    assert_eq!(manifest.lines().count(), 11);
    write(dir.path(), "sha256sums.txt", manifest.as_bytes());
    verify_stage(dir.path(), &version).unwrap();
    assert_eq!(sha256_manifest(dir.path()).unwrap(), manifest);
}

#[test]
fn a_missing_platform_blocks_the_release() {
    for missing in [
        "Kairo-2.0.0-arm64.dmg",
        "Kairo-2.0.0-mac.zip",
        "Kairo-Setup-2.0.0.exe",
        "Kairo-2.0.0.AppImage",
        "kairo-presenter_2.0.0_amd64.deb",
        "latest-linux.yml",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let version = complete(dir.path());
        fs::remove_file(dir.path().join(missing)).unwrap();
        assert!(
            errors(dir.path(), &version).contains(&format!("missing {missing}")),
            "{missing}"
        );
    }
}

#[test]
fn a_feed_that_does_not_match_its_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    // Re-signed or rebuilt after the feed was written.
    write(dir.path(), "Kairo-Setup-2.0.0.exe", b"different");
    let e = errors(dir.path(), &version);
    assert!(
        e.contains("latest.yml: the sha512 of Kairo-Setup-2.0.0.exe"),
        "{e}"
    );
    assert!(
        e.contains("latest.yml: the size of Kairo-Setup-2.0.0.exe"),
        "{e}"
    );
}

#[test]
fn a_feed_for_another_version_or_a_missing_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    feed(
        dir.path(),
        "latest-linux.yml",
        "1.0.33",
        "Kairo-2.0.0.AppImage",
    );
    assert!(errors(dir.path(), &version).contains("latest-linux.yml is for 1.0.33, not 2.0.0"));

    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    write(
        dir.path(),
        "latest.yml",
        b"version: 2.0.0\nfiles:\n  - url: Kairo-Setup-2.0.1.exe\n    sha512: x\n",
    );
    let e = errors(dir.path(), &version);
    assert!(
        e.contains("lists Kairo-Setup-2.0.1.exe, which is not in the release"),
        "{e}"
    );
    assert!(e.contains("latest.yml: Windows would take"), "{e}");
}

#[test]
fn stray_or_renamed_assets_are_refused() {
    // An old version left behind would be the website's first `.exe` match.
    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    write(dir.path(), "Kairo-Setup-1.0.33.exe", b"old");
    let e = errors(dir.path(), &version);
    assert!(e.contains("unexpected file Kairo-Setup-1.0.33.exe"), "{e}");

    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    write(dir.path(), "Kairo-2.0.0-universal.dmg", b"x");
    assert!(errors(dir.path(), &version).contains("unexpected file Kairo-2.0.0-universal.dmg"));
}

#[test]
fn an_empty_asset_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    write(dir.path(), "kairo-presenter_2.0.0_amd64.deb", b"");
    assert!(errors(dir.path(), &version).contains("kairo-presenter_2.0.0_amd64.deb is empty"));
}

#[test]
fn the_mac_feed_needs_both_architectures_and_the_minimum() {
    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    let text = fs::read_to_string(dir.path().join("latest-mac.yml")).unwrap();
    // Only the arm64 payloads, as the dropped merge in the Electron release would leave it.
    let arm_only: String = text
        .split("  - url: Kairo-2.0.0-mac.zip")
        .next()
        .unwrap()
        .to_owned()
        + &text[text.find("path:").unwrap()..];
    write(
        dir.path(),
        "latest-mac.yml",
        arm_only
            .replace("minimumSystemVersion: 22.0.0\n", "")
            .as_bytes(),
    );
    let e = errors(dir.path(), &version);
    assert!(e.contains("a X64 Mac would take"), "{e}");
    assert!(e.contains("minimumSystemVersion: 22.0.0"), "{e}");
}

#[test]
fn the_mac_feed_is_written_once_and_needs_a_real_date() {
    let dir = tempfile::tempdir().unwrap();
    let version = complete(dir.path());
    assert!(
        write_mac_feed(dir.path(), &version, "2026-11-15T12:00:00.000Z")
            .unwrap_err()
            .contains("already exists")
    );
    fs::remove_file(dir.path().join("latest-mac.yml")).unwrap();
    assert!(write_mac_feed(dir.path(), &version, "yesterday").is_err());
    fs::remove_file(dir.path().join("Kairo-2.0.0.dmg")).unwrap();
    assert!(
        write_mac_feed(dir.path(), &version, "2026-11-15T12:00:00Z")
            .unwrap_err()
            .contains("Kairo-2.0.0.dmg")
    );
}

#[test]
fn the_decklink_helper_must_match_its_sources() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        check_decklink(dir.path())
            .unwrap_err()
            .contains("no prebuilt DeckLink helper")
    );
    for file in DECKLINK_SOURCES {
        let path = dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("// {file}\r\n")).unwrap();
    }
    fs::create_dir_all(dir.path().join("native/decklink/prebuilt")).unwrap();
    fs::write(dir.path().join(DECKLINK_PREBUILT), b"helper").unwrap();
    let hash = decklink_source_hash(dir.path()).unwrap();
    fs::write(
        dir.path().join(DECKLINK_PREBUILT_SOURCE),
        format!("{hash}\n"),
    )
    .unwrap();
    check_decklink(dir.path()).unwrap();
    // CRLF and LF checkouts hash the same.
    fs::write(
        dir.path().join(DECKLINK_SOURCES[0]),
        format!("// {}\n", DECKLINK_SOURCES[0]),
    )
    .unwrap();
    check_decklink(dir.path()).unwrap();
    fs::write(dir.path().join(DECKLINK_SOURCES[0]), "// changed\n").unwrap();
    assert!(
        check_decklink(dir.path())
            .unwrap_err()
            .contains("source changed")
    );
}

#[test]
fn the_repository_helper_matches() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    check_decklink(&root).unwrap();
}
