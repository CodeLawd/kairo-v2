//! The settings launch against Electron's own: each fixture's files go into a
//! fresh userData folder, the Electron app's real `src/main/db/index.ts` runs
//! there (`oracle:settings-launch.launch`, with `electron` stubbed and time and
//! ids pinned), and so does `SettingsStore::open`. Every file the folder then
//! holds must be byte-identical.
//!
//! Runs from `tests/oracle-cache` unless `KAIRO_ORACLE_MODE` says otherwise.

use kairo_oracle::oracle_value;
use kairo_store::json_store::OpenState;
use kairo_store::settings::{LaunchContext, SettingsStore};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

const NOW: f64 = 1_700_000_000_000.0;

fn owner(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kairo-domain/tests/fixtures/owner")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn files(pairs: &[(&str, String)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, text)| (name.to_string(), text.clone()))
        .collect()
}

use kairo_oracle::pinned_uuids as pinned_ids;

struct Launch {
    ok: bool,
    files: BTreeMap<String, String>,
}

fn electron(input: &BTreeMap<String, String>, read_only: &[&str]) -> Launch {
    let result = oracle_value("oracle:settings-launch.launch", json!([input, read_only]));
    let files = result["files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    Launch {
        ok: result["ok"] == json!(true),
        files,
    }
}

fn native(input: &BTreeMap<String, String>, read_only: &[&str]) -> (Launch, OpenState) {
    let dir = tempfile::tempdir().unwrap();
    let user_data = dir.path().join("Kairo");
    std::fs::create_dir_all(&user_data).unwrap();
    for (name, text) in input {
        std::fs::write(user_data.join(name), text).unwrap();
    }
    #[cfg(unix)]
    for name in read_only {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(user_data.join(name), std::fs::Permissions::from_mode(0o444))
            .unwrap();
    }
    let mut ids = pinned_ids();
    let mut ctx = LaunchContext {
        now_ms: NOW,
        new_id: &mut ids,
    };
    let store = SettingsStore::open(&user_data, &mut ctx);
    let flushed = store.flush();
    let state = store.open_state().clone();
    drop(store);
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(&user_data).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        files.insert(name, std::fs::read_to_string(entry.path()).unwrap());
    }
    (
        Launch {
            ok: flushed.is_ok(),
            files,
        },
        state,
    )
}

fn assert_same(label: &str, input: BTreeMap<String, String>, read_only: &[&str]) {
    let expected = electron(&input, read_only);
    assert!(expected.ok, "{label}: the Electron launch failed");
    let (actual, _) = native(&input, read_only);
    assert!(actual.ok, "{label}: the native launch could not write");
    assert_eq!(
        expected.files.keys().collect::<Vec<_>>(),
        actual.files.keys().collect::<Vec<_>>(),
        "{label}: files"
    );
    for (name, text) in &expected.files {
        let ours = &actual.files[name];
        // Key order is not compared (DECISIONS.md, 011 and 021): Electron keeps
        // the order its input happened to have, typed values cannot.
        let (a, b): (Value, Value) = (
            serde_json::from_str(text).unwrap(),
            serde_json::from_str(ours).unwrap(),
        );
        let differences = kairo_oracle::compare(&a, &b);
        assert!(
            differences.is_empty(),
            "{label}: {name} differs:\n  {}",
            differences.join("\n  ")
        );
        // Numbers must also be the same JSON text, which `compare` reads as f64.
        assert_eq!(
            number_texts(text),
            number_texts(ours),
            "{label}: {name} number text"
        );
        // A file either app rewrote is exactly what electron-store writes for its value.
        if input.get(name) != Some(ours) {
            assert_eq!(
                ours,
                &kairo_store::json_text::stringify_tabs(&b),
                "{label}: {name} layout"
            );
        }
    }
}

/// Every number literal in a JSON text, sorted, so whole floats written as
/// `1.0` or integers written as floats would show.
fn number_texts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut current = String::new();
    for c in text.chars() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if c.is_ascii_digit() || "+-.eE".contains(c) {
            current.push(c);
            continue;
        }
        if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    out.sort();
    out
}

#[test]
fn fresh_install() {
    assert_same("fresh install", BTreeMap::new(), &[]);
}

#[test]
fn current_profile() {
    assert_same(
        "current profile",
        files(&[
            ("kairo-settings.json", owner("current-kairo-settings.json")),
            (
                "kairo-migrations.json",
                owner("current-kairo-migrations.json"),
            ),
        ]),
        &[],
    );
}

#[test]
fn older_kairo_profile_from_before_whats_new() {
    // Has ProPresenter settings, no `whatsNew`, and only some migration flags.
    assert_same(
        "1.0.28 upgrade",
        files(&[
            ("kairo-settings.json", owner("backup-kairo-settings.json")),
            (
                "kairo-migrations.json",
                owner("backup-kairo-migrations.json"),
            ),
        ]),
        &[],
    );
}

#[test]
fn proautomate_era_profile() {
    // Only the old names exist: both are adopted, then repaired.
    assert_same(
        "proautomate era",
        files(&[
            (
                "proautomate-settings.json",
                owner("backup-proautomate-settings.json"),
            ),
            (
                "proautomate-migrations.json",
                owner("backup-proautomate-migrations.json"),
            ),
        ]),
        &[],
    );
}

#[test]
fn both_names_present_keeps_the_new_file() {
    assert_same(
        "both names",
        files(&[
            (
                "proautomate-settings.json",
                owner("backup-proautomate-settings.json"),
            ),
            ("kairo-settings.json", owner("current-kairo-settings.json")),
        ]),
        &[],
    );
}

#[test]
fn partial_file() {
    let partial = json!({
        "display": { "theme": "light" },
        "stt": { "provider": "deepgram", "llmProvider": "anthropic", "llmModel": "claude-3" },
        "lyrics": { "glossColor": "#d4a017" },
        "scripture": { "defaultTranslation": "NKJV" },
        "overlay": { "template": "{Text}" },
    });
    assert_same(
        "partial",
        files(&[(
            "kairo-settings.json",
            serde_json::to_string(&partial).unwrap(),
        )]),
        &[],
    );
}

#[test]
fn hostile_sections_that_electron_survives() {
    let hostile = json!({
        "whatsNew": { "lastSeenVersion": "1.0.30" },
        "lyrics": { "googleTranslateApiKey": 5, "glossColor": "gold" },
        "workspace": null,
        "church": "First Church",
        "scripture": { "offlineDownloadBibleIds": "KJV" },
        "themeLibrary": "nope",
        "presentation": [],
        "overlay": 7,
        "propresenter": { "host": "x" },
        "unknownSection": { "kept": true },
    });
    let migrations = json!({ "customThemeLibraryV1": true, "builtInThemesV1": "yes" });
    assert_same(
        "hostile",
        files(&[
            (
                "kairo-settings.json",
                serde_json::to_string(&hostile).unwrap(),
            ),
            (
                "kairo-migrations.json",
                serde_json::to_string(&migrations).unwrap(),
            ),
        ]),
        &[],
    );
}

#[cfg(unix)]
#[test]
fn read_only_file_in_a_writable_folder() {
    // A rename replaces a read-only file, so both apps save.
    assert_same(
        "read-only file",
        files(&[("kairo-settings.json", owner("current-kairo-settings.json"))]),
        &["kairo-settings.json"],
    );
}

#[test]
fn corrupt_file_starts_from_defaults_where_electron_cannot_start() {
    let input = files(&[("kairo-settings.json", "{\"display\": {".to_string())]);
    let expected = electron(&input, &[]);
    assert!(!expected.ok, "Electron throws on a corrupt settings file");
    let (actual, state) = native(&input, &[]);
    let OpenState::Corrupt(aside) = state else {
        panic!("{state:?}")
    };
    let aside = aside.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        actual.files[&aside], "{\"display\": {",
        "the bad file is kept for support"
    );
    // Otherwise the result is a fresh install.
    let (fresh, _) = native(&BTreeMap::new(), &[]);
    assert_eq!(
        actual.files["kairo-settings.json"],
        fresh.files["kairo-settings.json"]
    );
    assert_eq!(
        actual.files["kairo-migrations.json"],
        fresh.files["kairo-migrations.json"]
    );
}

#[test]
fn a_hostile_stt_section_that_crashes_electron_is_healed() {
    let input = files(&[("kairo-settings.json", r#"{"stt":null}"#.to_string())]);
    assert!(
        !electron(&input, &[]).ok,
        "Electron reads null.llmProvider and throws"
    );
    let (actual, _) = native(&input, &[]);
    let settings: Value = serde_json::from_str(&actual.files["kairo-settings.json"]).unwrap();
    assert_eq!(
        settings["stt"],
        json!({ "llmProvider": "deepseek", "llmModel": "" })
    );
}

#[test]
fn a_second_launch_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kairo-settings.json"),
        owner("backup-kairo-settings.json"),
    )
    .unwrap();
    let mut ids = pinned_ids();
    let mut ctx = LaunchContext {
        now_ms: NOW,
        new_id: &mut ids,
    };
    SettingsStore::open(dir.path(), &mut ctx).flush().unwrap();
    let first = std::fs::read_to_string(dir.path().join("kairo-settings.json")).unwrap();
    SettingsStore::open(dir.path(), &mut ctx).flush().unwrap();
    let second = std::fs::read_to_string(dir.path().join("kairo-settings.json")).unwrap();
    assert_eq!(first, second);
    let flags: Map<String, Value> = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("kairo-migrations.json")).unwrap(),
    )
    .unwrap();
    // The flags the launch itself sets; the others belong to later steps.
    for flag in [
        "deepseekDefaultV1",
        "nkjvDefaultV1",
        "customThemeLibraryV1",
        "builtInThemesV1",
        "glossPaletteV1",
    ] {
        assert_eq!(flags[flag], json!(true), "{flag}");
    }
    for flag in ["kjvDefaultV1", "cloudOnboardingV1", "songsFolderExportV1"] {
        assert_eq!(flags[flag], json!(false), "{flag}");
    }
}
