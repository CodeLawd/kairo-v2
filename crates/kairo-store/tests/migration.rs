//! First-run migration over the prompt 100 profiles: fresh, 1.0.28, recent,
//! 5,000 songs, moved workspace and missing external media. Each runs
//! forward (the migration, then the native settings launch) and back
//! (Electron's own `src/main/db/index.ts` on the result, through the
//! oracle), and checks that no Electron file was deleted.

use kairo_oracle::oracle_value;
use kairo_store::migration::devices::{InputDevice, chromium_device_id};
use kairo_store::migration::local_storage::{FILE_ORIGIN, write_fixture};
use kairo_store::migration::{BACKUP_DIR, MARKER_FILE, MigrationInput, MigrationReport, run};
use kairo_store::secrets::{KeyProtector, SecretError};
use kairo_store::settings::{LaunchContext, SettingsStore};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SALT: &str = "0123456789abcdef0123456789abcdef";
const UID: &str = "AppleUSBAudioEngine:Focusrite:Scarlett 2i4 USB:1235:8200";

#[derive(Default)]
struct Keys(Mutex<BTreeMap<String, [u8; 32]>>);
impl KeyProtector for Keys {
    fn load(&self, slot: &str) -> Result<Option<[u8; 32]>, SecretError> {
        Ok(self.0.lock().unwrap().get(slot).copied())
    }
    fn save(&self, slot: &str, key: &[u8; 32]) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(slot.into(), *key);
        Ok(())
    }
}

fn mic() -> Vec<InputDevice> {
    vec![
        InputDevice {
            id: "default".into(),
            label: "System Default".into(),
        },
        InputDevice {
            id: format!("coreaudio:{UID}"),
            label: "Scarlett 2i4 USB".into(),
        },
    ]
}

struct Profile {
    _dir: tempfile::TempDir,
    user_data: PathBuf,
    workspace: PathBuf,
}

fn profile() -> Profile {
    let dir = tempfile::tempdir().unwrap();
    let user_data = dir.path().join("Kairo");
    std::fs::create_dir_all(&user_data).unwrap();
    std::fs::create_dir_all(dir.path().join("Documents")).unwrap();
    let workspace = dir.path().join("Documents/Kairo");
    Profile {
        user_data,
        workspace,
        _dir: dir,
    }
}

fn write(p: &Profile, name: &str, value: &Value) {
    std::fs::write(
        p.user_data.join(name),
        serde_json::to_string_pretty(value).unwrap(),
    )
    .unwrap();
}

fn migrate(p: &Profile, devices: fn() -> Vec<InputDevice>) -> MigrationReport {
    let keys = Keys::default();
    run(&MigrationInput {
        user_data: &p.user_data,
        default_workspace: p.workspace.clone(),
        protector: &keys,
        electron_app: "Kairo",
        devices: &devices,
        now: chrono::Local::now(),
    })
}

/// The native settings launch, as the app runs it after the migration.
fn native_launch(p: &Profile) -> Value {
    let mut ids = kairo_oracle::pinned_uuids();
    let mut ctx = LaunchContext {
        now_ms: 1_700_000_000_000.0,
        new_id: &mut ids,
    };
    let store = SettingsStore::open(&p.user_data, &mut ctx);
    store.flush().unwrap();
    Value::Object(store.data().clone())
}

/// Every top-level JSON store, by name, for the Electron launch.
fn stores(p: &Profile) -> BTreeMap<String, String> {
    std::fs::read_dir(&p.user_data)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().unwrap().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.ends_with(".json") && (n.starts_with("kairo-") || n.starts_with("proautomate-"))
        })
        .filter(|n| n != MARKER_FILE)
        .map(|n| {
            let text = std::fs::read_to_string(p.user_data.join(&n)).unwrap();
            (n, text)
        })
        .collect()
}

/// Rollback: Electron's real launch on the migrated profile starts and
/// keeps `keep` (a JSON pointer into `kairo-settings.json`) as it was.
fn electron_loads(p: &Profile, keep: &[(&str, Value)]) {
    let result = oracle_value("oracle:settings-launch.launch", json!([stores(p), []]));
    assert_eq!(
        result["ok"],
        json!(true),
        "Electron starts on the migrated profile"
    );
    let settings: Value =
        serde_json::from_str(result["files"]["kairo-settings.json"].as_str().unwrap()).unwrap();
    for (pointer, value) in keep {
        assert_eq!(
            settings.pointer(pointer),
            Some(value),
            "{pointer} after rollback"
        );
    }
}

fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in walk(dir) {
        out.push(
            entry
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
    }
    out.sort();
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn assert_nothing_deleted(before: &[String], p: &Profile) {
    let after = files(&p.user_data);
    for file in before {
        assert!(after.contains(file), "{file} was deleted");
    }
}

#[test]
fn fresh_install_needs_nothing_and_runs_once() {
    let p = profile();
    let report = migrate(&p, mic);
    assert!(report.ran && !report.upgraded);
    assert!(report.backup.is_none());
    assert!(report.attention().is_empty());
    assert!(p.user_data.join(MARKER_FILE).is_file());
    let again = migrate(&p, mic);
    assert!(!again.ran, "the marker stops a second run");
    native_launch(&p);
}

#[test]
fn version_1_0_28_upgrades_and_rolls_back() {
    let p = profile();
    // 1.0.28: the old store names, no `whatsNew`, a Chromium microphone id.
    let hashed = chromium_device_id(SALT, FILE_ORIGIN, UID);
    write(
        &p,
        "proautomate-settings.json",
        &json!({
            "audio": {"deviceId": hashed},
            "church": {"name": "Grace Church", "timezone": "", "role": "", "serviceTimes": []},
            "stt": {"provider": "deepgram", "apiKey": "", "anthropicApiKey": "", "llmProvider": "anthropic", "llmModel": "", "bibleApiKey": "", "language": "en-US"}
        }),
    );
    write(
        &p,
        "proautomate-migrations.json",
        &json!({"nkjvDefaultV1": true}),
    );
    write(
        &p,
        "Preferences",
        &json!({"electron": {"media": {"device_id_salt": SALT}}}),
    );
    write_fixture(
        &p.user_data,
        FILE_ORIGIN,
        &[
            ("operator-transcript-width", "360"),
            (
                "kairo-message-nuggets",
                r#"[{"id":"n1","text":"Be still"}]"#,
            ),
            ("kairo.lyrics-library-width", "300"),
        ],
    )
    .unwrap();
    let before = files(&p.user_data);
    let original = std::fs::read(p.user_data.join("proautomate-settings.json")).unwrap();

    let report = migrate(&p, mic);
    assert!(report.ran && report.upgraded);
    assert!(report.attention().is_empty(), "{:?}", report.notes);
    let backup = report.backup.clone().unwrap();
    assert!(backup.starts_with(p.user_data.join(BACKUP_DIR)));
    assert_eq!(
        std::fs::read(backup.join("proautomate-settings.json")).unwrap(),
        original,
        "the backup holds the file as it was"
    );
    assert!(backup.join("Preferences").is_file());

    // localStorage → ui-state, as Electron's strings.
    let ui: Value =
        serde_json::from_str(&std::fs::read_to_string(p.user_data.join("ui-state.json")).unwrap())
            .unwrap();
    assert_eq!(ui["operator-transcript-width"], json!("360"));
    assert_eq!(ui["kairo.lyrics-library-width"], json!("300"));
    let nuggets = ui_operator_nuggets(&ui["kairo-message-nuggets"]);
    assert_eq!(nuggets, vec![("n1".to_owned(), "Be still".to_owned())]);
    assert_eq!(report.imported_keys, 3);

    // The microphone hash became its label.
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(p.user_data.join("proautomate-settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["audio"]["deviceId"], json!("Scarlett 2i4 USB"));

    let data = native_launch(&p);
    assert_eq!(data["whatsNew"]["lastSeenVersion"], json!("1.0.28"));
    assert_eq!(data["church"]["name"], json!("Grace Church"));
    assert_nothing_deleted(&before, &p);
    assert!(
        p.user_data.join("proautomate-settings.json").is_file(),
        "legacy kept"
    );
    electron_loads(
        &p,
        &[
            ("/church/name", json!("Grace Church")),
            ("/audio/deviceId", json!("Scarlett 2i4 USB")),
            ("/whatsNew/lastSeenVersion", json!("1.0.28")),
        ],
    );
}

/// The saved form of `kairo-message-nuggets` the Operator reads (a JSON
/// string in localStorage), as `(id, text)`.
fn ui_operator_nuggets(value: &Value) -> Vec<(String, String)> {
    let text = value.as_str().unwrap();
    serde_json::from_str::<Vec<Value>>(text)
        .unwrap()
        .iter()
        .map(|n| {
            (
                n["id"].as_str().unwrap().into(),
                n["text"].as_str().unwrap().into(),
            )
        })
        .collect()
}

#[test]
fn recent_profile_keeps_native_values_and_flags_an_unplugged_microphone() {
    let p = profile();
    std::fs::create_dir_all(&p.workspace).unwrap();
    let other = chromium_device_id(SALT, FILE_ORIGIN, "some-other-device");
    write(
        &p,
        "kairo-settings.json",
        &json!({"whatsNew": {"lastSeenVersion": "1.0.40"}, "audio": {"deviceId": other}, "workspace": {"folder": ""}}),
    );
    write(
        &p,
        "kairo-migrations.json",
        &json!({"nkjvDefaultV1": true, "builtInThemesV1": true}),
    );
    write(
        &p,
        "Preferences",
        &json!({"electron": {"media": {"device_id_salt": SALT}}}),
    );
    write(
        &p,
        "ui-state.json",
        &json!({"operator-transcript-width": 400}),
    );
    write_fixture(
        &p.user_data,
        FILE_ORIGIN,
        &[
            ("operator-transcript-width", "360"),
            ("media-dock-height", "250"),
        ],
    )
    .unwrap();
    let report = migrate(&p, mic);
    let attention = report.attention();
    assert_eq!(attention.len(), 1, "{attention:?}");
    assert!(attention[0].contains("microphone"));
    let ui: Value =
        serde_json::from_str(&std::fs::read_to_string(p.user_data.join("ui-state.json")).unwrap())
            .unwrap();
    assert_eq!(
        ui["operator-transcript-width"],
        json!(400),
        "native value wins"
    );
    assert_eq!(ui["media-dock-height"], json!("250"));
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(p.user_data.join("kairo-settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        settings["audio"]["deviceId"],
        json!(other),
        "left for rollback"
    );
    native_launch(&p);
    electron_loads(&p, &[("/whatsNew/lastSeenVersion", json!("1.0.40"))]);
}

#[test]
fn five_thousand_songs_under_three_seconds() {
    let p = profile();
    write(
        &p,
        "kairo-settings.json",
        &json!({"whatsNew": {"lastSeenVersion": "1.0.40"}}),
    );
    let db = rusqlite::Connection::open(p.user_data.join("lyrics.db")).unwrap();
    db.execute_batch(include_str!("../src/db/lyrics.sql"))
        .unwrap();
    let tx = db.unchecked_transaction().unwrap();
    for i in 0..5000 {
        tx.execute(
            "INSERT INTO songs (id, title, created_at, updated_at) VALUES (?1, ?2, 0, 0)",
            (format!("song-{i}"), format!("Song {i}")),
        )
        .unwrap();
        for s in 0..4 {
            tx.execute(
                "INSERT INTO song_sections (song_id, type, label, lines) VALUES (?1, 'verse', ?2, '[\"line\"]')",
                (format!("song-{i}"), format!("Verse {s}")),
            )
            .unwrap();
        }
    }
    tx.commit().unwrap();
    drop(db);
    let report = migrate(&p, mic);
    eprintln!("5,000-song profile: {:.1} ms", report.elapsed_ms);
    assert!(report.attention().is_empty(), "{:?}", report.notes);
    assert!(report.elapsed_ms < 3000.0, "{} ms", report.elapsed_ms);
    let count: i64 = rusqlite::Connection::open(p.user_data.join("lyrics.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM songs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 5000);
    native_launch(&p);
    electron_loads(&p, &[("/whatsNew/lastSeenVersion", json!("1.0.40"))]);
}

#[test]
fn moved_workspace_is_checked() {
    let p = profile();
    let moved = p.user_data.parent().unwrap().join("Shared/Kairo");
    std::fs::create_dir_all(&moved).unwrap();
    write(
        &p,
        "kairo-settings.json",
        &json!({"workspace": {"folder": moved.to_string_lossy()}}),
    );
    let report = migrate(&p, mic);
    assert!(report.attention().is_empty(), "{:?}", report.notes);
    native_launch(&p);

    let gone = profile();
    let missing = gone
        .user_data
        .parent()
        .unwrap()
        .join("Unplugged")
        .join("Kairo");
    let missing = missing.to_string_lossy();
    write(
        &gone,
        "kairo-settings.json",
        &json!({"workspace": {"folder": missing}}),
    );
    let report = migrate(&gone, mic);
    let attention = report.attention();
    assert_eq!(attention.len(), 1);
    assert!(attention[0].contains(missing.as_ref()));
    native_launch(&gone);
    electron_loads(&gone, &[("/workspace/folder", json!(missing))]);
}

#[test]
fn missing_external_media_is_flagged_and_kept() {
    let p = profile();
    let media = p.user_data.parent().unwrap().join("Media Drive");
    let worship = media.join("Worship");
    let tracks = media.join("Tracks");
    write(
        &p,
        "kairo-settings.json",
        &json!({"media": {"folder": worship, "playlists": []}, "tracks": {"folder": tracks}}),
    );
    let report = migrate(&p, mic);
    let attention = report.attention();
    assert_eq!(attention.len(), 2, "{attention:?}");
    assert!(
        attention
            .iter()
            .all(|a| a.contains(media.to_string_lossy().as_ref()))
    );
    native_launch(&p);
    electron_loads(&p, &[("/media/folder", json!(worship))]);
}

#[test]
fn a_damaged_database_is_reported_and_kept() {
    let p = profile();
    write(&p, "kairo-settings.json", &json!({}));
    std::fs::write(
        p.user_data.join("bible.db"),
        b"not a database at all, just text",
    )
    .unwrap();
    let report = migrate(&p, mic);
    let attention = report.attention();
    assert_eq!(attention.len(), 1, "{attention:?}");
    assert!(attention[0].starts_with("bible.db could not be opened"));
    assert!(p.user_data.join("bible.db").is_file());
}

#[test]
fn a_failed_backup_changes_nothing_and_retries() {
    let p = profile();
    write(
        &p,
        "kairo-settings.json",
        &json!({"audio": {"deviceId": "x"}}),
    );
    // A file where the backup folder must go.
    std::fs::write(p.user_data.join(BACKUP_DIR), b"").unwrap();
    let report = migrate(&p, mic);
    assert_eq!(report.attention().len(), 1);
    assert!(report.attention()[0].contains("could not back up"));
    assert!(
        !p.user_data.join(MARKER_FILE).exists(),
        "tries again next launch"
    );
}

#[test]
fn unreadable_local_storage_resets_layout_only() {
    let p = profile();
    write(&p, "kairo-settings.json", &json!({}));
    let dir = p.user_data.join("Local Storage/leveldb");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("CURRENT"), b"MANIFEST-000999\n").unwrap();
    let report = migrate(&p, mic);
    assert!(report.attention().is_empty(), "layout loss is not shown");
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.text.contains("window layout not imported"))
    );
    assert!(!p.user_data.join("ui-state.json").exists());
}
