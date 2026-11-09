//! Byte-semantic round trip: every fixture parses into its type and writes
//! back as the same JSON (same keys, values and number text; key order aside).
//!
//! `fixtures/owner` holds the owner's real store files with every string that
//! is not a TypeScript literal or a colour replaced
//! (`tools/ts-oracle/sanitize-fixture.mjs`). `fixtures/handmade` holds old and
//! hostile shapes written by hand.

mod common;

use common::{fixture, fixtures, read, round_trip};
use kairo_domain::cloud::CrashReportQueue;
use kairo_domain::collections::{LibrariesState, SetlistState};
use kairo_domain::json::to_js_value;
use kairo_domain::scripture::{PassageStore, SermonPlanStore};
use kairo_domain::services::ServiceSnapshot;
use kairo_domain::settings::{
    MigrationFlags, OnboardingStore, SECTION_COUNT, Stored, StoredSettings,
};
use kairo_domain::usage::{LedgerState, SongUsageStore};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Round trips an Electron store file as `Stored<T>` for its store type, and
/// returns whether it parsed as the canonical `T`. A file that does not (an
/// older shape) is kept as raw JSON for the store's normaliser.
fn round_trip_store(label: &str, file: &str, json: &Value) -> bool {
    fn store<T: Serialize + DeserializeOwned>(label: &str, json: &Value) -> bool {
        round_trip::<Stored<T>>(label, json).typed().is_some()
    }
    let name = file
        .trim_start_matches("current-")
        .trim_start_matches("backup-")
        .trim_end_matches(".json")
        .replace("proautomate-", "kairo-");
    match name.as_str() {
        "kairo-settings" => {
            // Settings are kept per section instead; see `StoredSettings`.
            round_trip::<StoredSettings>(label, json)
                .non_canonical_sections()
                .is_empty()
        }
        "kairo-migrations" => store::<MigrationFlags>(label, json),
        "kairo-onboarding" => store::<OnboardingStore>(label, json),
        "kairo-libraries" => store::<LibrariesState>(label, json),
        "kairo-passages" => store::<PassageStore>(label, json),
        "kairo-sermon-plans" => store::<SermonPlanStore>(label, json),
        "kairo-services" => store::<ServiceSnapshot>(label, json),
        "kairo-setlist" => store::<SetlistState>(label, json),
        "kairo-usage" => store::<LedgerState>(label, json),
        "song-usage" => store::<SongUsageStore>(label, json),
        "kairo-crash-reports" => store::<CrashReportQueue>(label, json),
        other => panic!("{label}: no store type for {other}"),
    }
}

#[test]
fn every_owner_store_file_round_trips() {
    let files = fixtures("owner");
    assert!(
        files.len() >= 20,
        "expected the owner fixtures, found {}",
        files.len()
    );
    let mut not_canonical = Vec::new();
    for path in files {
        let file = path.file_name().unwrap().to_str().unwrap().to_owned();
        if !round_trip_store(&file, &file, &read(&path)) {
            not_canonical.push(file);
        }
    }
    // Every file the current build wrote is canonical. Older ones may not be.
    let current: Vec<_> = not_canonical
        .iter()
        .filter(|f| f.starts_with("current-"))
        .collect();
    assert!(
        current.is_empty(),
        "current files that are not canonical: {current:?}"
    );
    eprintln!("older files kept raw for normalisers: {not_canonical:?}");
}

#[test]
fn every_handmade_file_round_trips() {
    for path in fixtures("handmade") {
        let file = path.file_name().unwrap().to_str().unwrap();
        round_trip::<StoredSettings>(file, &read(&path));
    }
}

/// Today's settings file, written by the current Electron build, is canonical
/// in every section, so the rest of the app can use `AppSettings` directly.
#[test]
fn current_settings_are_fully_canonical() {
    let json = read(&fixture("owner/current-kairo-settings.json"));
    let stored: StoredSettings = round_trip("current settings", &json);
    assert_eq!(stored.non_canonical_sections(), Vec::<&str>::new());
    let settings = stored.into_canonical().unwrap();
    // A key no TypeScript type names survives at the top level.
    assert!(settings.extra.contains_key("audience"));
    assert_eq!(to_js_value(&settings).unwrap(), json);
}

/// Older files are kept, not repaired: the sections that do not match yet stay
/// raw for the normalisers, and retired keys stay in `extra`.
#[test]
fn old_files_keep_non_canonical_sections_raw() {
    let path = fixtures("owner")
        .into_iter()
        .find(|p| p.ends_with("backup-proautomate-settings.json"))
        .unwrap();
    let stored: StoredSettings = round_trip("proautomate settings", &read(&path));
    let missing = stored.non_canonical_sections();
    for absent in ["whatsNew", "capture", "web", "usage", "themeDefaults"] {
        assert!(
            missing.contains(&absent),
            "{absent} should be absent: {missing:?}"
        );
    }
    assert!(matches!(stored.overlay, Some(Stored::Raw(_))));
    assert!(stored.extra.contains_key("propresenter"));
    assert!(stored.extra.contains_key("propresenterResources"));
    assert!(stored.into_canonical().is_err());

    let v0 = read(&fixture("handmade/proautomate-settings-v0.json"));
    let stored: StoredSettings = round_trip("v0", &v0);
    assert_eq!(
        stored.non_canonical_sections().len(),
        SECTION_COUNT,
        "every section is old or absent"
    );
}

/// Wrong types and unknown enum values never fail a load. The section stays
/// raw; a canonical section keeps unknown nested keys in its `extra`.
#[test]
fn hostile_values_stay_raw_and_unknown_keys_survive() {
    let json = read(&fixture("handmade/settings-hostile.json"));
    let stored: StoredSettings = round_trip("hostile", &json);
    for raw in [
        "lyrics",
        "scripture",
        "stt",
        "church",
        "workspace",
        "themeLibrary",
        "themeDefaults",
    ] {
        assert!(
            stored.non_canonical_sections().contains(&raw),
            "{raw} should be raw"
        );
    }
    let capture = stored
        .capture
        .as_ref()
        .and_then(Stored::typed)
        .expect("capture is canonical");
    assert!(capture.extra.contains_key("addedLater"));
    let display = stored
        .display
        .as_ref()
        .and_then(Stored::typed)
        .expect("display is canonical");
    let shortcuts = display.shortcuts.as_ref().unwrap();
    assert_eq!(shortcuts.clear.as_deref(), Some(&["Escape".to_owned()][..]));
    assert!(shortcuts.extra.contains_key("futureCommand"));
    assert!(stored.extra.contains_key("futureSection"));
    assert!(stored.documents.as_ref().and_then(Stored::typed).is_some());
}

/// A file from a newer version: unknown keys added to every object of a
/// canonical file all survive, and every section is still canonical.
#[test]
fn newer_version_keys_survive_at_every_level() {
    fn add_keys(value: &mut Value, depth: usize) {
        match value {
            Value::Object(map) => {
                for child in map.values_mut() {
                    add_keys(child, depth + 1);
                }
                map.insert(format!("futureKey{depth}"), serde_json::json!({"n": depth}));
            }
            Value::Array(items) => items.iter_mut().for_each(|item| add_keys(item, depth + 1)),
            _ => {}
        }
    }
    let mut json = read(&fixture("owner/current-kairo-settings.json"));
    // Records keyed by data (shortcut bindings aside) gain a key like any object;
    // that is fine, as they are maps or have `extra`.
    add_keys(&mut json, 0);
    let stored: StoredSettings = round_trip("future settings", &json);
    assert_eq!(stored.non_canonical_sections(), Vec::<&str>::new());
}

#[test]
fn absent_optional_fields_are_not_written_as_null() {
    let json = serde_json::json!({
        "id": "s", "title": "T", "artist": "", "sections": [{"type": "verse", "label": "V", "lines": []}],
        "createdAt": 1, "updatedAt": 2
    });
    let song: kairo_domain::lyrics::LyricsSong = round_trip("song", &json);
    assert!(song.copyright.is_none());
    let mut missing_required_null = json.clone();
    missing_required_null
        .as_object_mut()
        .unwrap()
        .remove("title");
    assert!(
        serde_json::from_value::<kairo_domain::lyrics::LyricsSong>(missing_required_null).is_err()
    );
}

/// An empty settings file loads with every section absent and writes back as
/// `{}`; a file that is not an object is an error, not a panic, and the store
/// (prompt 021) decides what to do with it.
#[test]
fn empty_and_non_object_settings_files() {
    let empty: StoredSettings = round_trip("empty", &serde_json::json!({}));
    assert_eq!(empty.non_canonical_sections().len(), SECTION_COUNT);
    for not_an_object in ["[]", "null", "\"text\"", "3", "{", ""] {
        assert!(
            serde_json::from_str::<StoredSettings>(not_an_object).is_err(),
            "{not_an_object:?} should not load"
        );
    }
    // As with `JSON.parse`, the last of two equal keys wins.
    let twice: StoredSettings =
        serde_json::from_str(r#"{"audio":{"deviceId":"a"},"audio":{"deviceId":"b"}}"#).unwrap();
    assert_eq!(twice.audio.unwrap().typed().unwrap().device_id, "b");
}
