//! Native release checks and update metadata (prompt 104).
//!
//! Ports the safety rules of the Electron release (`scripts/release.mjs`,
//! `.github/workflows/release.yml`) to the native packages:
//!
//! - one version sequence: native releases are `2.x.y` and above every
//!   Electron release (DECISIONS 104);
//! - the tag matches the workspace version;
//! - every release has a What's New entry unless it is internal-only;
//! - the committed DeckLink helper matches its sources;
//! - `latest-mac.yml` lists both architectures, as the Electron merge did,
//!   with `minimumSystemVersion` for macOS 13 (DECISIONS 101);
//! - a release is published only when every asset is there, under the names
//!   the website, the admin console and both updaters match.

use base64::Engine as _;
use regex::Regex;
use semver::Version;
use sha2::{Digest, Sha512};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::Path;

use kairo_updater::feed::{
    Arch, UpdateFeed, parse_feed, pick_linux_appimage, pick_mac_zip, pick_windows_installer,
};

/// The first major version of the native line. Electron stays below it.
pub const NATIVE_MAJOR: u64 = 2;

/// `minimumSystemVersion` in `latest-mac.yml`. electron-updater compares it
/// with `os.release()`, the Darwin version: Darwin 22 is macOS 13.
pub const MAC_MINIMUM_SYSTEM_VERSION: &str = "22.0.0";

/// The release repository the website, the admin console and both updaters read.
pub const RELEASE_REPO: &str = kairo_updater::feed::RELEASE_REPO;

/// The What's New data, generated from the website changelog.
pub const CHANGELOG_JSON: &str = "crates/ui-shell/data/changelog.json";

/// Everything compiled into the DeckLink helper that lives in this repository,
/// in the order `scripts/prepare-mac-natives.mjs` hashes them.
pub const DECKLINK_SOURCES: [&str; 7] = [
    "native/decklink/helper.cpp",
    "native/decklink/compatibility.h",
    "native/decklink/sdk-compatibility.h",
    "native/decklink/frame-input.h",
    "native/decklink/uyvy.h",
    "native/gpu-frames/ring.h",
    "native/decklink/build.mjs",
];
pub const DECKLINK_PREBUILT: &str = "native/decklink/prebuilt/decklink-helper";
pub const DECKLINK_PREBUILT_SOURCE: &str = "native/decklink/prebuilt/SOURCE_SHA256";

// ---------------------------------------------------------------- versions

/// A release version: plain `MAJOR.MINOR.PATCH`. The Windows file version,
/// the deb version and the packaging scripts all need it numeric.
pub fn parse_release_version(text: &str) -> Result<Version, String> {
    let version = Version::parse(text.trim())
        .map_err(|e| format!("\"{text}\" is not a version like 2.0.0 ({e})"))?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(format!(
            "\"{text}\" has a pre-release or build part; release versions are plain MAJOR.MINOR.PATCH"
        ));
    }
    Ok(version)
}

/// The version a tag names. `v2.0.0` always; `v2.0.0-test.3` only for a dry
/// run into this repository's own releases. Returns the version and whether
/// the tag is a test tag.
pub fn version_from_tag(tag: &str, allow_test: bool) -> Result<(Version, bool), String> {
    let pattern = Regex::new(r"^v(\d+\.\d+\.\d+)(-test\.\d+)?$").expect("valid regex");
    let captures = pattern.captures(tag).ok_or_else(|| {
        format!("\"{tag}\" is not a release tag like v2.0.0 (or v2.0.0-test.1 for a dry run)")
    })?;
    let test = captures.get(2).is_some();
    if test && !allow_test {
        return Err(format!(
            "\"{tag}\" is a test tag; test tags may only publish a dry run to this repository"
        ));
    }
    Ok((parse_release_version(&captures[1])?, test))
}

/// Native versions start at `NATIVE_MAJOR` and sort above every Electron
/// release, so an Electron install offered a native release sees it as newer.
pub fn check_native_sequence(native: &Version, electron_latest: &str) -> Result<(), String> {
    if native.major < NATIVE_MAJOR {
        return Err(format!(
            "native versions start at {NATIVE_MAJOR}.0.0 (got {native}); Electron keeps 1.x"
        ));
    }
    let electron = Version::parse(electron_latest.trim().trim_start_matches('v')).map_err(|e| {
        format!("cannot read the latest Electron release \"{electron_latest}\": {e}")
    })?;
    if *native <= electron {
        return Err(format!(
            "{native} does not sort above the latest {RELEASE_REPO} release ({electron})"
        ));
    }
    Ok(())
}

/// `[workspace.package] version` from the root `Cargo.toml`.
pub fn workspace_version(cargo_toml: &str) -> Result<String, String> {
    let value: toml::Value = cargo_toml
        .parse()
        .map_err(|e| format!("Cargo.toml does not parse: {e}"))?;
    value
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml has no [workspace.package] version".to_owned())
}

/// The root `Cargo.toml` with `[workspace.package] version` set to `new`,
/// changing nothing else.
pub fn set_workspace_version(cargo_toml: &str, new: &str) -> Result<String, String> {
    let mut out = String::with_capacity(cargo_toml.len());
    let mut section = String::new();
    let mut replaced = false;
    for line in cargo_toml.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed.to_owned();
        }
        if section == "[workspace.package]"
            && !replaced
            && trimmed.starts_with("version")
            && trimmed["version".len()..].trim_start().starts_with('=')
        {
            let ending = if line.ends_with('\n') { "\n" } else { "" };
            out.push_str(&format!("version = \"{new}\"{ending}"));
            replaced = true;
        } else {
            out.push_str(line);
        }
    }
    if !replaced {
        return Err("Cargo.toml has no [workspace.package] version line".into());
    }
    if workspace_version(&out)? != new {
        return Err("rewriting Cargo.toml did not set the version".into());
    }
    Ok(out)
}

/// `patch`, `minor`, `major` or an exact version, as `release.mjs` takes them.
/// The result must be newer than `current`.
pub fn next_version(current: &str, bump: &str) -> Result<Version, String> {
    let current = Version::parse(current.trim())
        .map_err(|e| format!("the workspace version \"{current}\" is not semver: {e}"))?;
    let next = match bump {
        "patch" => Version::new(current.major, current.minor, current.patch + 1),
        "minor" => Version::new(current.major, current.minor + 1, 0),
        "major" => Version::new(current.major + 1, 0, 0),
        exact => parse_release_version(exact).map_err(|_| {
            format!("unknown version \"{exact}\"; use patch, minor, major or an exact version like 2.0.0")
        })?,
    };
    if next <= current {
        return Err(format!(
            "{next} is not newer than the current version {current}"
        ));
    }
    Ok(next)
}

// --------------------------------------------------------------- changelog

/// The date of `version`'s entry in the What's New data, if it has one.
pub fn changelog_entry_date(json: &str, version: &str) -> Result<Option<String>, String> {
    #[derive(serde::Deserialize)]
    struct Entry {
        version: String,
        date: String,
    }
    let entries: Vec<Entry> =
        serde_json::from_str(json).map_err(|e| format!("{CHANGELOG_JSON} does not parse: {e}"))?;
    Ok(entries
        .into_iter()
        .find(|e| e.version == version)
        .map(|e| e.date))
}

/// The changelog entry as release-note markdown, if there is one.
pub fn changelog_notes(json: &str, version: &str) -> Result<Option<String>, String> {
    #[derive(serde::Deserialize)]
    struct Entry {
        version: String,
        title: String,
        notes: Vec<String>,
    }
    let entries: Vec<Entry> =
        serde_json::from_str(json).map_err(|e| format!("{CHANGELOG_JSON} does not parse: {e}"))?;
    Ok(entries.into_iter().find(|e| e.version == version).map(|e| {
        let mut text = format!("## {}\n\n", e.title);
        for note in e.notes {
            text.push_str(&format!("- {note}\n"));
        }
        text
    }))
}

// ---------------------------------------------------------------- DeckLink

/// The SHA-256 `prepare-mac-natives.mjs` records: each path, then its text
/// with CRLF made LF, in order.
pub fn decklink_source_hash(root: &Path) -> Result<String, String> {
    let mut hash = sha2::Sha256::new();
    for file in DECKLINK_SOURCES {
        let text =
            fs::read_to_string(root.join(file)).map_err(|e| format!("cannot read {file}: {e}"))?;
        hash.update(file.as_bytes());
        hash.update(text.replace("\r\n", "\n").as_bytes());
    }
    Ok(hex(&hash.finalize()))
}

/// A release never ships without the helper, or with one older than its code.
pub fn check_decklink(root: &Path) -> Result<(), String> {
    let rebuild = "Rebuild it on a Mac with the Blackmagic DeckLink SDK and commit native/decklink/prebuilt/:\n\n  DECKLINK_SDK_PATH='/path/to/Blackmagic DeckLink SDK 16.0' npm run prepare:mac-natives\n\n(in kairo-presenter, then copy native/decklink/prebuilt/ here)";
    if !root.join(DECKLINK_PREBUILT).is_file() || !root.join(DECKLINK_PREBUILT_SOURCE).is_file() {
        return Err(format!(
            "no prebuilt DeckLink helper, so this build would ship without video card outputs.\n{rebuild}"
        ));
    }
    let recorded = fs::read_to_string(root.join(DECKLINK_PREBUILT_SOURCE))
        .map_err(|e| format!("cannot read {DECKLINK_PREBUILT_SOURCE}: {e}"))?;
    if recorded.trim() != decklink_source_hash(root)? {
        return Err(format!(
            "the DeckLink helper source changed since {DECKLINK_PREBUILT} was built.\n{rebuild}"
        ));
    }
    Ok(())
}

/// `lipo -archs` output: the helper must run on Apple Silicon and Intel.
pub fn check_universal(lipo_archs: &str) -> Result<(), String> {
    let archs: Vec<&str> = lipo_archs.split_whitespace().collect();
    if archs.contains(&"arm64") && archs.contains(&"x86_64") {
        Ok(())
    } else {
        Err(format!(
            "{DECKLINK_PREBUILT} is {}; rebuild it universal",
            if archs.is_empty() {
                "unreadable".to_owned()
            } else {
                archs.join(", ")
            }
        ))
    }
}

// ------------------------------------------------------------------ assets

/// Every file a complete release carries, by the names electron-builder gave
/// the Electron release. Optional files may be missing; nothing else may be there.
pub fn expected_assets(version: &Version) -> Vec<(String, bool)> {
    let v = version.to_string();
    vec![
        (format!("Kairo-{v}-arm64.dmg"), true),
        (format!("Kairo-{v}.dmg"), true),
        (format!("Kairo-{v}-arm64-mac.zip"), true),
        (format!("Kairo-{v}-mac.zip"), true),
        ("latest-mac.yml".into(), true),
        (format!("Kairo-Setup-{v}.exe"), true),
        (format!("Kairo-Setup-{v}.exe.blockmap"), false),
        ("latest.yml".into(), true),
        (format!("Kairo-{v}.AppImage"), true),
        (format!("kairo-presenter_{v}_amd64.deb"), true),
        ("latest-linux.yml".into(), true),
        ("sha256sums.txt".into(), false),
    ]
}

/// A file's name, base64 SHA-512 (as electron-builder writes it) and size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payload {
    pub name: String,
    pub sha512: String,
    pub size: u64,
}

pub fn payload(path: &Path) -> Result<Payload, String> {
    let mut file =
        fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut hash = Sha512::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hash.update(&buf[..n]);
    }
    Ok(Payload {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        sha512: base64::engine::general_purpose::STANDARD.encode(hash.finalize()),
        size,
    })
}

/// `latest-mac.yml` for both architectures, laid out as electron-builder
/// writes it (js-yaml): what a single dual-arch run would emit, which the
/// Electron release reproduced by merging. Scalars follow the arm64 zip.
pub fn mac_feed_yaml(
    version: &Version,
    arm64_zip: &Payload,
    arm64_dmg: &Payload,
    x64_zip: &Payload,
    x64_dmg: &Payload,
    release_date: &str,
) -> String {
    let mut out = format!("version: {version}\nfiles:\n");
    for file in [arm64_zip, arm64_dmg, x64_zip, x64_dmg] {
        out.push_str(&format!(
            "  - url: {}\n    sha512: {}\n    size: {}\n",
            file.name, file.sha512, file.size
        ));
    }
    out.push_str(&format!(
        "path: {}\nsha512: {}\nreleaseDate: '{}'\nminimumSystemVersion: {MAC_MINIMUM_SYSTEM_VERSION}\n",
        arm64_zip.name,
        arm64_zip.sha512,
        release_date.replace('\'', "''")
    ));
    out
}

/// Writes `latest-mac.yml` into `dir` from the four macOS payloads there.
pub fn write_mac_feed(dir: &Path, version: &Version, release_date: &str) -> Result<String, String> {
    if !Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{3})?Z$")
        .expect("valid regex")
        .is_match(release_date)
    {
        return Err(format!(
            "release date \"{release_date}\" is not an ISO 8601 UTC time like 2026-11-15T12:00:00.000Z"
        ));
    }
    let v = version.to_string();
    let read = |name: String| payload(&dir.join(&name));
    let yaml = mac_feed_yaml(
        version,
        &read(format!("Kairo-{v}-arm64-mac.zip"))?,
        &read(format!("Kairo-{v}-arm64.dmg"))?,
        &read(format!("Kairo-{v}-mac.zip"))?,
        &read(format!("Kairo-{v}.dmg"))?,
        release_date,
    );
    let path = dir.join("latest-mac.yml");
    if path.exists() {
        return Err(format!(
            "{} already exists; the macOS jobs must not upload a feed of their own",
            path.display()
        ));
    }
    fs::write(&path, &yaml).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(yaml)
}

/// What the website's download route picks for each platform
/// (`website/src/app/api/download/[platform]/route.ts`), and the file it
/// falls back to by name.
pub fn website_matchers(version: &Version) -> Vec<(&'static str, Regex, Option<Regex>, String)> {
    let v = version.to_string();
    let re = |p: &str| Regex::new(p).expect("valid regex");
    vec![
        (
            "mac-arm64",
            re(r"(?i)arm64\.dmg$"),
            None,
            format!("Kairo-{v}-arm64.dmg"),
        ),
        (
            "mac-x64",
            re(r"(?i)\.dmg$"),
            Some(re(r"(?i)(arm64|aarch64)")),
            format!("Kairo-{v}.dmg"),
        ),
        (
            "windows-x64",
            re(r"(?i)\.exe$"),
            None,
            format!("Kairo-Setup-{v}.exe"),
        ),
        (
            "linux-x64",
            re(r"(?i)\.AppImage$"),
            None,
            format!("Kairo-{v}.AppImage"),
        ),
    ]
}

/// The admin console counts these as downloads (`admin.service.ts` INSTALLER).
pub fn admin_installer() -> Regex {
    Regex::new(r"(?i)\.(dmg|exe|AppImage|deb|zip)$").expect("valid regex")
}

fn read_feed(dir: &Path, name: &str) -> Result<UpdateFeed, String> {
    let text =
        fs::read_to_string(dir.join(name)).map_err(|e| format!("cannot read {name}: {e}"))?;
    parse_feed(&text).map_err(|e| format!("{name}: {e}"))
}

/// One feed: the right version, and every file it lists is in the release
/// with that hash and size.
fn check_feed(
    dir: &Path,
    name: &str,
    version: &str,
    present: &BTreeSet<String>,
    errors: &mut Vec<String>,
) -> Option<UpdateFeed> {
    let feed = match read_feed(dir, name) {
        Ok(feed) => feed,
        Err(e) => {
            errors.push(e);
            return None;
        }
    };
    if feed.version != version {
        errors.push(format!("{name} is for {}, not {version}", feed.version));
    }
    if feed.files.is_empty() {
        errors.push(format!("{name} lists no files"));
    }
    for file in &feed.files {
        if !present.contains(&file.url) {
            errors.push(format!(
                "{name} lists {}, which is not in the release",
                file.url
            ));
            continue;
        }
        match payload(&dir.join(&file.url)) {
            Ok(p) => {
                if p.sha512 != file.sha512 {
                    errors.push(format!(
                        "{name}: the sha512 of {} does not match the file",
                        file.url
                    ));
                }
                if file.size.is_some_and(|s| s != p.size) {
                    errors.push(format!(
                        "{name}: the size of {} does not match the file",
                        file.url
                    ));
                }
            }
            Err(e) => errors.push(e),
        }
    }
    if let Some(path) = &feed.path
        && !feed.files.iter().any(|f| &f.url == path)
    {
        errors.push(format!("{name}: path {path} is not one of its files"));
    }
    Some(feed)
}

/// Checks a staged release directory. Returns every problem, not just the first.
pub fn verify_stage(dir: &Path, version: &Version) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let v = version.to_string();
    let expected = expected_assets(version);
    let present: BTreeSet<String> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(e) => return Err(vec![format!("cannot read {}: {e}", dir.display())]),
    };
    for (name, required) in &expected {
        if *required && !present.contains(name) {
            errors.push(format!("missing {name}"));
        }
    }
    for name in &present {
        if !expected.iter().any(|(n, _)| n == name) {
            errors.push(format!(
                "unexpected file {name} (a stale version, or an asset name the website and updaters do not match)"
            ));
        }
    }
    for name in &present {
        match fs::metadata(dir.join(name)) {
            Ok(m) if m.len() == 0 => errors.push(format!("{name} is empty")),
            Err(e) => errors.push(format!("cannot stat {name}: {e}")),
            _ => {}
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    if let Some(feed) = check_feed(dir, "latest-mac.yml", &v, &present, &mut errors) {
        for (arch, want) in [
            (Arch::Arm64, format!("Kairo-{v}-arm64-mac.zip")),
            (Arch::X64, format!("Kairo-{v}-mac.zip")),
        ] {
            match pick_mac_zip(&feed.files, arch) {
                Some(f) if f.url == want => {}
                other => errors.push(format!(
                    "latest-mac.yml: a {arch:?} Mac would take {:?}, not {want}",
                    other.map(|f| &f.url)
                )),
            }
        }
        let text = fs::read_to_string(dir.join("latest-mac.yml")).unwrap_or_default();
        let minimum = text
            .lines()
            .find_map(|l| l.strip_prefix("minimumSystemVersion:"));
        if minimum.map(|m| m.trim().trim_matches('\'').trim_matches('"'))
            != Some(MAC_MINIMUM_SYSTEM_VERSION)
        {
            errors.push(format!(
                "latest-mac.yml must carry minimumSystemVersion: {MAC_MINIMUM_SYSTEM_VERSION}, so Electron on macOS 11 and 12 is not offered it"
            ));
        }
    }
    if let Some(feed) = check_feed(dir, "latest.yml", &v, &present, &mut errors) {
        let want = format!("Kairo-Setup-{v}.exe");
        match pick_windows_installer(&feed.files, Arch::X64) {
            Some(f) if f.url == want => {}
            other => errors.push(format!(
                "latest.yml: Windows would take {:?}, not {want}",
                other.map(|f| &f.url)
            )),
        }
    }
    if let Some(feed) = check_feed(dir, "latest-linux.yml", &v, &present, &mut errors) {
        let want = format!("Kairo-{v}.AppImage");
        match pick_linux_appimage(&feed.files, Arch::X64) {
            Some(f) if f.url == want => {}
            other => errors.push(format!(
                "latest-linux.yml: Linux would take {:?}, not {want}",
                other.map(|f| &f.url)
            )),
        }
    }

    // The website picks the first asset that matches; exactly one may.
    for (platform, matcher, exclude, fallback) in website_matchers(version) {
        let hits: Vec<&String> = present
            .iter()
            .filter(|n| matcher.is_match(n) && !exclude.as_ref().is_some_and(|x| x.is_match(n)))
            .collect();
        if hits.len() != 1 || *hits[0] != fallback {
            errors.push(format!(
                "the website's {platform} download would match {hits:?}, expected exactly {fallback}"
            ));
        }
    }
    let installers: Vec<&String> = present
        .iter()
        .filter(|n| admin_installer().is_match(n))
        .collect();
    if installers.len() != 7 {
        errors.push(format!(
            "the admin console would count {installers:?} as installers; expected the 7 installers"
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// `sha256sum`-style manifest of every file in `dir` except itself.
pub fn sha256_manifest(dir: &Path) -> Result<String, String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "sha256sums.txt")
        .collect();
    names.sort();
    let mut out = String::new();
    for name in names {
        let bytes = fs::read(dir.join(&name)).map_err(|e| format!("cannot read {name}: {e}"))?;
        out.push_str(&format!("{}  {name}\n", hex(&sha2::Sha256::digest(&bytes))));
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_versions_are_plain() {
        assert_eq!(
            parse_release_version("2.0.0").unwrap(),
            Version::new(2, 0, 0)
        );
        assert!(parse_release_version("2.0.0-beta.1").is_err());
        assert!(parse_release_version("2.0").is_err());
        assert!(parse_release_version("").is_err());
    }

    #[test]
    fn tags() {
        assert_eq!(
            version_from_tag("v2.1.3", false).unwrap(),
            (Version::new(2, 1, 3), false)
        );
        assert_eq!(
            version_from_tag("v2.0.0-test.4", true).unwrap(),
            (Version::new(2, 0, 0), true)
        );
        assert!(version_from_tag("v2.0.0-test.4", false).is_err());
        assert!(version_from_tag("2.0.0", true).is_err());
        assert!(version_from_tag("v2.0.0-beta.1", true).is_err());
        assert!(version_from_tag("v2.0.0 ", true).is_err());
    }

    #[test]
    fn the_native_line_sorts_above_electron() {
        let v = |s| Version::parse(s).unwrap();
        assert!(check_native_sequence(&v("2.0.0"), "v1.0.33").is_ok());
        assert!(check_native_sequence(&v("2.0.0"), "1.9.99").is_ok());
        assert!(check_native_sequence(&v("1.0.34"), "v1.0.33").is_err());
        // An Electron release that strayed onto 2.x blocks the native one.
        assert!(check_native_sequence(&v("2.0.0"), "v2.0.0").is_err());
        assert!(check_native_sequence(&v("2.0.1"), "v2.0.0").is_ok());
        assert!(check_native_sequence(&v("2.0.0"), "garbage").is_err());
    }

    #[test]
    fn bumps() {
        assert_eq!(
            next_version("2.0.0", "patch").unwrap(),
            Version::new(2, 0, 1)
        );
        assert_eq!(
            next_version("2.0.4", "minor").unwrap(),
            Version::new(2, 1, 0)
        );
        assert_eq!(
            next_version("0.1.0", "2.0.0").unwrap(),
            Version::new(2, 0, 0)
        );
        assert!(next_version("2.0.0", "2.0.0").is_err());
        assert!(next_version("2.0.0", "1.9.0").is_err());
        assert!(next_version("2.0.0", "nope").is_err());
        assert!(next_version("2.0.0", "2.1.0-rc.1").is_err());
    }

    #[test]
    fn workspace_version_is_rewritten_in_place() {
        let toml = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace.dependencies]\nversion = \"9\"\n";
        assert_eq!(workspace_version(toml).unwrap(), "0.1.0");
        let out = set_workspace_version(toml, "2.0.0").unwrap();
        assert_eq!(out, toml.replacen("\"0.1.0\"", "\"2.0.0\"", 1));
        assert!(set_workspace_version("[workspace]\n", "2.0.0").is_err());
    }

    #[test]
    fn changelog_lookup() {
        let json = r#"[{"version":"2.0.0","date":"2026-12-01","title":"Native","notes":["Faster."],"highlights":[]}]"#;
        assert_eq!(
            changelog_entry_date(json, "2.0.0").unwrap().as_deref(),
            Some("2026-12-01")
        );
        assert_eq!(changelog_entry_date(json, "2.0.1").unwrap(), None);
        assert_eq!(
            changelog_notes(json, "2.0.0").unwrap().unwrap(),
            "## Native\n\n- Faster.\n"
        );
        assert!(changelog_entry_date("{", "2.0.0").is_err());
    }

    #[test]
    fn universal_helper() {
        assert!(check_universal("x86_64 arm64\n").is_ok());
        assert!(check_universal("arm64").is_err());
        assert!(check_universal("").is_err());
    }

    #[test]
    fn the_mac_feed_reads_back_through_the_updater() {
        let p = |name: &str, n: u64| Payload {
            name: name.into(),
            sha512: format!("h{n}+/=="),
            size: n,
        };
        let yaml = mac_feed_yaml(
            &Version::new(2, 0, 0),
            &p("Kairo-2.0.0-arm64-mac.zip", 1),
            &p("Kairo-2.0.0-arm64.dmg", 2),
            &p("Kairo-2.0.0-mac.zip", 3),
            &p("Kairo-2.0.0.dmg", 4),
            "2026-11-15T12:00:00.000Z",
        );
        let feed = parse_feed(&yaml).unwrap();
        assert_eq!(feed.version, "2.0.0");
        assert_eq!(feed.files.len(), 4);
        assert_eq!(
            pick_mac_zip(&feed.files, Arch::Arm64).unwrap().url,
            "Kairo-2.0.0-arm64-mac.zip"
        );
        assert_eq!(
            pick_mac_zip(&feed.files, Arch::X64).unwrap().url,
            "Kairo-2.0.0-mac.zip"
        );
        assert_eq!(feed.path.as_deref(), Some("Kairo-2.0.0-arm64-mac.zip"));
        assert_eq!(
            feed.release_date.as_deref(),
            Some("2026-11-15T12:00:00.000Z")
        );
        assert!(yaml.contains("\nminimumSystemVersion: 22.0.0\n"));
    }
}
