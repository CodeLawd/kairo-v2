//! `settings:set`, `settings:get` and `settings:getAll` from
//! `src/main/ipc/index.ts`, the post-open KJV repair, the other store files,
//! crash safety and timing.

use kairo_domain::cloud::SecretsConfigured;
use kairo_oracle::oracle_value;
use kairo_store::json_store::JsonStore;
use kairo_store::settings::{
    LaunchContext, PostOpenServices, STORES, SettingsEffect, SettingsSetError, SettingsStore,
    open_store, open_ui_state, store_spec,
};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kairo_oracle::pinned_uuids as ids;

fn open(dir: &Path) -> SettingsStore {
    let mut ids = ids();
    SettingsStore::open(
        dir,
        &mut LaunchContext {
            now_ms: 1_700_000_000_000.0,
            new_id: &mut ids,
        },
    )
}

fn set(
    store: &mut SettingsStore,
    key: &str,
    value: Value,
) -> Result<Vec<SettingsEffect>, SettingsSetError> {
    let mut ids = ids();
    store.apply_set(
        key,
        value,
        &mut LaunchContext {
            now_ms: 1.0,
            new_id: &mut ids,
        },
    )
}

fn no_secrets() -> SecretsConfigured {
    serde_json::from_value(json!({
        "deepgram": true, "anthropic": false, "deepseek": false, "bible": false, "brave": false, "googleTranslate": false
    }))
    .unwrap()
}

fn on_disk(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("kairo-settings.json")).unwrap())
        .unwrap()
}

#[test]
fn secret_sections_merge_and_never_store_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let effects = set(
        &mut store,
        "stt",
        json!({ "provider": "deepgram", "apiKey": "  dg-secret  ", "language": "fr-FR" }),
    )
    .unwrap();
    assert_eq!(effects, vec![SettingsEffect::BroadcastSettingsChanged]);
    store.flush().unwrap();
    let stt = &on_disk(dir.path())["stt"];
    assert_eq!(
        stt["apiKey"],
        json!(""),
        "provider keys live in the church account"
    );
    assert_eq!(stt["language"], json!("fr-FR"));
    assert_eq!(
        stt["llmProvider"],
        json!("deepseek"),
        "keys not sent are kept"
    );
    assert!(stt.get("clearKeys").is_none());

    set(
        &mut store,
        "lyrics",
        json!({ "glossColor": "#ffffff", "clearKeys": ["braveApiKey"] }),
    )
    .unwrap();
    store.flush().unwrap();
    let lyrics = &on_disk(dir.path())["lyrics"];
    assert_eq!(lyrics["glossColor"], json!("#ffffff"));
    assert_eq!(lyrics["braveApiKey"], json!(""));
    assert!(lyrics.get("clearKeys").is_none());
}

#[test]
fn overlay_sets_merge_with_what_is_stored_and_normalise() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let before = store.data()["overlay"]["outputs"].clone();
    let effects = set(
        &mut store,
        "overlay",
        json!({ "template": "{Text}", "maxVerses": -3 }),
    )
    .unwrap();
    let overlay = &store.data()["overlay"];
    assert_eq!(overlay["template"], json!("{Text}"));
    assert_eq!(overlay["maxVerses"], json!(0), "clamped by the normaliser");
    assert_eq!(overlay["outputs"], before, "slices not sent survive");
    assert!(matches!(
        &effects[..],
        [SettingsEffect::ReconcileOutputs(Some(_))]
    ));
}

#[test]
fn other_sections_follow_the_ipc_handler() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let church =
        json!({ "name": "Grace", "timezone": "Europe/London", "role": "tech", "serviceTimes": [] });
    assert_eq!(
        set(&mut store, "church", church.clone()).unwrap(),
        vec![SettingsEffect::SyncChurchProfile(
            json!({ "name": "Grace", "timezone": "Europe/London", "serviceTimes": [] })
        )]
    );
    assert_eq!(store.data()["church"], church);

    let scripture =
        json!({ "defaultTranslation": "ESV", "autoMode": false, "confidenceThreshold": 0.9 });
    assert_eq!(
        set(&mut store, "scripture", scripture.clone()).unwrap(),
        vec![SettingsEffect::ApplyScripture(scripture.clone())]
    );
    assert_eq!(store.data()["scripture"], scripture, "stored as sent");

    assert_eq!(
        set(&mut store, "presentation", json!({})).unwrap(),
        vec![
            SettingsEffect::PruneProps,
            SettingsEffect::ReconcileOutputs(None)
        ]
    );
    assert!(
        set(&mut store, "capture", json!({ "bitrateMbps": 500 }))
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.data()["capture"]["bitrateMbps"], json!(100));
    set(&mut store, "web", json!(null)).unwrap();
    assert!(store.data()["web"].is_object());
    set(&mut store, "documents", json!("junk")).unwrap();
    assert!(store.data()["documents"].is_object());
    set(
        &mut store,
        "themeLibrary",
        json!([{ "id": "a", "name": " Mine ", "theme": {} }]),
    )
    .unwrap();
    assert_eq!(store.data()["themeLibrary"][0]["name"], json!("Mine"));

    for retired in ["propresenter", "propresenterResources"] {
        assert_eq!(
            set(&mut store, retired, json!({ "host": "x" })),
            Err(SettingsSetError::Retired)
        );
    }
}

#[test]
fn the_renderer_never_sees_secrets() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kairo-settings.json"),
        r##"{"stt":{"apiKey":"dg","bibleApiKey":"bb","llmProvider":"deepseek"},"lyrics":{"braveApiKey":"br","glossColor":"#fff"}}"##,
    )
    .unwrap();
    let store = open(dir.path());
    let mut ids = ids();
    let mut ctx = LaunchContext {
        now_ms: 1.0,
        new_id: &mut ids,
    };
    let all = store.for_renderer(&no_secrets(), &mut ctx);
    assert_eq!(all["stt"]["apiKey"], json!(""));
    assert_eq!(all["stt"]["bibleApiKey"], json!(""));
    assert_eq!(all["lyrics"]["braveApiKey"], json!(""));
    assert_eq!(all["secretsConfigured"]["deepgram"], json!(true));
    assert_eq!(store.get_for_renderer("stt", &mut ctx)["apiKey"], json!(""));
    assert_eq!(store.get_for_renderer("missing", &mut ctx), Value::Null);
    assert!(store.get_for_renderer("overlay", &mut ctx)["outputs"].is_array());
    // The stored value itself still holds the key until the cloud owns it.
    assert_eq!(store.data()["stt"]["apiKey"], json!("dg"));
    // A complete store's view parses as the typed renderer settings.
    let fresh = tempfile::tempdir().unwrap();
    let all = open(fresh.path()).for_renderer(&no_secrets(), &mut ctx);
    let typed: kairo_domain::settings::SettingsWithSecretsStatus =
        serde_json::from_value(all).unwrap();
    assert!(typed.secrets_configured.deepgram);
}

struct Packs(Result<bool, String>);
impl PostOpenServices for Packs {
    fn bible_pack_installed(&self, _: &str) -> Result<bool, String> {
        self.0.clone()
    }
}

#[test]
fn an_unreadable_nkjv_default_moves_to_kjv_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kairo-settings.json"),
        r#"{"scripture":{"defaultTranslation":"nkjv"}}"#,
    )
    .unwrap();
    // The launch's own one-time move to KJV must not have run yet for this case.
    std::fs::write(
        dir.path().join("kairo-migrations.json"),
        r#"{"nkjvDefaultV1":true}"#,
    )
    .unwrap();
    let mut store = open(dir.path());
    store.migrate_unreadable_nkjv_default(&Packs(Ok(false)));
    assert_eq!(
        store.data()["scripture"]["defaultTranslation"],
        json!("KJV")
    );
    assert_eq!(store.migrations()["kjvDefaultV1"], json!(true));
    // Once only: a later NKJV choice stays.
    let mut next = store.data()["scripture"].clone();
    next["defaultTranslation"] = json!("NKJV");
    set(&mut store, "scripture", next).unwrap();
    store.migrate_unreadable_nkjv_default(&Packs(Ok(false)));
    assert_eq!(
        store.data()["scripture"]["defaultTranslation"],
        json!("NKJV")
    );
}

#[test]
fn a_readable_nkjv_stays_and_a_failed_check_still_sets_the_flag() {
    for (packs, key) in [
        (Packs(Ok(true)), ""),
        (Packs(Ok(false)), "key"),
        (Packs(Err("db locked".into())), ""),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            json!({ "scripture": { "defaultTranslation": "NKJV" }, "stt": { "bibleApiKey": key } });
        std::fs::write(dir.path().join("kairo-settings.json"), settings.to_string()).unwrap();
        std::fs::write(
            dir.path().join("kairo-migrations.json"),
            r#"{"nkjvDefaultV1":true}"#,
        )
        .unwrap();
        let mut store = open(dir.path());
        store.migrate_unreadable_nkjv_default(&packs);
        assert_eq!(
            store.data()["scripture"]["defaultTranslation"],
            json!("NKJV")
        );
        assert_eq!(store.migrations()["kjvDefaultV1"], json!(true));
    }
}

#[test]
fn whats_new_marks_only_upgrades() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        !open(dir.path()).upgraded_from_before_whats_new(),
        "a fresh install is new"
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("kairo-settings.json"), r#"{"display":{}}"#).unwrap();
    let store = open(dir.path());
    assert!(store.upgraded_from_before_whats_new());
    assert_eq!(
        store.data()["whatsNew"],
        json!({ "lastSeenVersion": "1.0.28" })
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kairo-settings.json"),
        r#"{"whatsNew":{"lastSeenVersion":"1.0.35"}}"#,
    )
    .unwrap();
    assert_eq!(
        open(dir.path()).data()["whatsNew"]["lastSeenVersion"],
        json!("1.0.35")
    );
}

#[test]
fn section_changes_notify_listeners() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    store.on_did_change("display", move |new, _| {
        log.lock().unwrap().push(new["theme"].clone())
    });
    set(
        &mut store,
        "display",
        json!({ "theme": "light", "fontSize": 16, "transcriptionFontSize": 18 }),
    )
    .unwrap();
    set(
        &mut store,
        "display",
        json!({ "theme": "light", "fontSize": 16, "transcriptionFontSize": 18 }),
    )
    .unwrap();
    set(&mut store, "audio", json!({ "deviceId": "x" })).unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![json!("light")]);
    let typed: kairo_domain::settings::DisplaySettings = store.section("display").unwrap();
    assert_eq!(
        serde_json::to_value(&typed).unwrap()["theme"],
        json!("light")
    );
}

#[test]
fn every_other_store_opens_with_electrons_defaults() {
    let expected = [
        (
            "kairo-libraries",
            oracle_value("libraries.emptyLibrariesState", json!([])),
        ),
        (
            "kairo-setlist",
            oracle_value("setlist.emptySetlistState", json!([])),
        ),
        (
            "kairo-onboarding",
            json!({ "onboarding": oracle_value("cloud/onboarding.DEFAULT_ONBOARDING_STATE", json!([])) }),
        ),
    ];
    for (name, value) in expected {
        let spec = store_spec(name).unwrap();
        assert_eq!(Value::Object((spec.defaults)()), value, "{name}");
    }
    let dir = tempfile::tempdir().unwrap();
    for spec in STORES {
        let store = open_store(dir.path(), spec);
        store.flush().unwrap();
        let text = std::fs::read_to_string(dir.path().join(format!("{}.json", spec.name))).unwrap();
        assert_eq!(
            text,
            kairo_store::json_text::stringify_tabs(&Value::Object((spec.defaults)())),
            "{}",
            spec.name
        );
    }
}

#[test]
fn renamed_stores_adopt_their_legacy_files_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("proautomate-onboarding.json"),
        r#"{"onboarding":{"currentStep":"done"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("proautomate-sermon-plans.json"),
        r#"{"plans":[{"id":"p"}],"livePlanId":"p"}"#,
    )
    .unwrap();
    let onboarding = open_store(dir.path(), store_spec("kairo-onboarding").unwrap());
    let plans = open_store(dir.path(), store_spec("kairo-sermon-plans").unwrap());
    assert_eq!(
        onboarding.get("onboarding").unwrap()["currentStep"],
        json!("done")
    );
    assert_eq!(plans.get("livePlanId"), Some(&json!("p")));
    assert!(
        dir.path().join("proautomate-onboarding.json").exists(),
        "legacy files stay for older builds"
    );
}

#[test]
fn ui_state_is_a_plain_store() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut ui = open_ui_state(dir.path());
        ui.set("kairo.sidebarWidth", json!(280));
    }
    let ui = open_ui_state(dir.path());
    assert_eq!(ui.get("kairo.sidebarWidth"), Some(&json!(280)));
}

#[test]
fn a_set_returns_quickly_and_writes_later() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kairo-settings.json"),
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../kairo-domain/tests/fixtures/owner/current-kairo-settings.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut store = open(dir.path());
    store.flush().unwrap();
    let mut samples = Vec::new();
    for i in 0..200 {
        let theme = if i % 2 == 0 { "light" } else { "dark" };
        let start = Instant::now();
        set(
            &mut store,
            "display",
            json!({ "theme": theme, "fontSize": 16, "transcriptionFontSize": 18 }),
        )
        .unwrap();
        samples.push(start.elapsed());
    }
    store.flush().unwrap();
    samples.sort();
    let p99 = samples[samples.len() * 99 / 100];
    println!(
        "display set on the owner's settings: p50 {:?}, p99 {:?}, max {:?}",
        samples[100], p99, samples[199]
    );
    assert!(p99 < Duration::from_millis(5), "p99 {p99:?}");
}

const KILL_ENV: &str = "KAIRO_KILL_WRITER_DIR";

/// In a child process: write the settings store as fast as possible until killed.
#[test]
fn kill_writer_child() {
    let Ok(dir) = std::env::var(KILL_ENV) else {
        return;
    };
    let dir = Path::new(&dir);
    let mut store = JsonStore::open(dir, "kairo-settings", serde_json::Map::new());
    println!("\n@@WRITING");
    let big: Vec<Value> = (0..2_000)
        .map(|i| json!({ "id": i, "text": "x".repeat(64) }))
        .collect();
    let mut i = 0u64;
    loop {
        i += 1;
        store.set("counter", json!(i));
        store.set("payload", Value::Array(big.clone()));
        let _ = store.flush();
    }
}

#[test]
fn killing_the_process_mid_write_never_leaves_a_broken_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kairo-settings.json");
    std::fs::write(&path, "{}").unwrap();
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut parsed_after_writes = 0;
    for round in 0..500 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "kill_writer_child",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env(KILL_ENV, dir.path())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        assert!(
            lines.any(|l| l.is_ok_and(|l| l.contains("@@WRITING"))),
            "round {round}: the writer did not start"
        );
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        std::thread::sleep(Duration::from_micros(500 + seed % 15_000));
        child.kill().unwrap(); // SIGKILL on Unix, TerminateProcess on Windows.
        child.wait().unwrap();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("round {round}: {e}"));
        let value: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("round {round}: the file does not parse: {e}"));
        if value.get("counter").is_some() {
            parsed_after_writes += 1;
        }
    }
    println!(
        "500 kills; the file parsed every time; {parsed_after_writes} rounds had a completed write"
    );
    assert!(
        parsed_after_writes > 400,
        "kills mostly landed after a write: {parsed_after_writes}"
    );
    // Opening again clears temporary files the killed writes left.
    drop(JsonStore::open(
        dir.path(),
        "kairo-settings",
        serde_json::Map::new(),
    ));
    let leftovers = std::fs::read_dir(dir.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn display_rebinding_is_persisted_without_overwriting_a_new_assignment_or_other_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open(dir.path());
    store.set_raw("overlay", json!({"other":"preserved","outputs":[
        {"id":"a","displayId":7,"displayLabel":"Projector","displaySize":{"width":1920,"height":1080},"keep":true},
        {"id":"b","displayId":99}]}));
    assert!(store.persist_display_rebinding("a", 7, 8, false));
    assert!(
        !store.persist_display_rebinding("a", 7, 9, false),
        "stale replug cannot revert a later assignment"
    );
    store.flush().unwrap();
    let stored = &on_disk(dir.path())["overlay"];
    assert_eq!(stored["outputs"][0]["displayId"], 8);
    assert_eq!(stored["outputs"][0]["displayLabel"], "Projector");
    assert_eq!(stored["outputs"][0]["keep"], true);
    assert_eq!(stored["outputs"][1]["displayId"], 99);
    assert_eq!(stored["other"], "preserved");
    store.set_raw(
        "presentation",
        json!({"keep":true,"stageDisplays":[{"id":"s","displayId":1}]}),
    );
    assert!(store.persist_display_rebinding("s", 1, 2, true));
    store.flush().unwrap();
    assert_eq!(
        on_disk(dir.path())["presentation"]["stageDisplays"][0]["displayId"],
        2
    );
    assert!(!store.persist_display_rebinding("missing", 1, 2, false));
}
