use kairo_core::settings::{SettingsServices, SettingsWiring};
use kairo_domain::cloud::SecretsConfigured;
use kairo_store::settings::{LaunchContext, SettingsStore};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Services {
    calls: Mutex<Vec<(String, Value)>>,
    store: Option<Arc<Mutex<SettingsStore>>>,
}
impl Services {
    fn push(&self, name: &str, value: Value) {
        let _unlocked = self
            .store
            .as_ref()
            .unwrap()
            .try_lock()
            .expect("effects must run after releasing the store lock");
        self.calls.lock().unwrap().push((name.into(), value));
    }
    fn take(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}
impl SettingsServices for Services {
    fn secrets_configured(&self) -> SecretsConfigured {
        SecretsConfigured {
            bible: true,
            ..Default::default()
        }
    }
    fn reconcile_outputs(&self, value: Option<Value>) {
        self.push("reconcile", value.unwrap_or(Value::Null));
    }
    fn prune_props(&self) {
        self.push("prune", Value::Null);
    }
    fn sync_church_profile(&self, value: Value) {
        self.push("church", value);
    }
    fn broadcast_settings_changed(&self, value: Value) {
        self.push("broadcast", value);
    }
    fn scripture_default_translation(&self, value: Value) {
        self.push("scripture.translation", value);
    }
    fn scripture_auto_mode(&self, value: Value) {
        self.push("scripture.auto", value);
    }
    fn scripture_confidence(&self, value: Value) {
        self.push("scripture.confidence", value);
    }
    fn orchestrator_translation(&self, value: Value) {
        self.push("orchestrator.translation", value);
    }
    fn orchestrator_auto_mode(&self, value: Value) {
        self.push("orchestrator.auto", value);
    }
    fn orchestrator_confidence(&self, value: Value) {
        self.push("orchestrator.confidence", value);
    }
}
fn app() -> (tempfile::TempDir, SettingsWiring, Arc<Services>) {
    let dir = tempfile::tempdir().unwrap();
    let mut id = || "test-id".into();
    let store = SettingsStore::open(
        dir.path(),
        &mut LaunchContext {
            now_ms: 1.,
            new_id: &mut id,
        },
    );
    let store = Arc::new(Mutex::new(store));
    let services = Arc::new(Services {
        store: Some(store.clone()),
        ..Default::default()
    });
    let app = SettingsWiring::new(store, services.clone());
    (dir, app, services)
}
#[test]
fn overlay_merges_normalizes_and_reconciles_stored_outputs() {
    let (_dir, mut app, services) = app();
    let before = app.get("overlay");
    app.set("overlay", json!({"mode":"full"})).unwrap();
    let after = app.get("overlay");
    assert_eq!(after["theme"], before["theme"]);
    assert_eq!(after["outputs"], before["outputs"]);
    assert_eq!(
        services.take(),
        vec![("reconcile".into(), after["outputs"].clone())]
    );
}
#[test]
fn presentation_prunes_before_reconcile_and_does_not_broadcast() {
    let (_dir, mut app, services) = app();
    app.set("presentation", json!({"props":null})).unwrap();
    assert!(app.get("presentation")["props"].is_array());
    assert_eq!(
        services.take(),
        vec![
            ("prune".into(), Value::Null),
            ("reconcile".into(), Value::Null)
        ]
    );
}
#[test]
fn stt_always_blanks_keys_merges_fields_and_broadcasts_licence_providers() {
    let (_dir, mut app, services) = app();
    let before = app.get("stt");
    app.set("stt", json!({"apiKey":"secret", "anthropicApiKey":"secret", "deepseekApiKey":"secret", "bibleApiKey":"secret", "clearKeys":["apiKey"]})).unwrap();
    let stored = app.get("stt");
    for key in ["apiKey", "anthropicApiKey", "deepseekApiKey", "bibleApiKey"] {
        assert_eq!(stored[key], "");
        assert_eq!(app.store().lock().unwrap().data()["stt"][key], "");
    }
    assert_eq!(stored["llmProvider"], before["llmProvider"]);
    assert!(stored.get("clearKeys").is_none());
    let calls = services.take();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "broadcast");
    assert_eq!(calls[0].1["secretsConfigured"]["bible"], true);
    assert_eq!(calls[0].1["secretsConfigured"]["deepgram"], false);
    assert_eq!(calls[0].1["stt"], stored);
}
#[test]
fn lyrics_always_blanks_keys_and_broadcasts() {
    let (_dir, mut app, services) = app();
    app.set(
        "lyrics",
        json!({"braveApiKey":"secret", "googleTranslateApiKey":"secret"}),
    )
    .unwrap();
    assert_eq!(app.get("lyrics")["braveApiKey"], "");
    assert_eq!(app.get("lyrics")["googleTranslateApiKey"], "");
    assert_eq!(
        app.store().lock().unwrap().data()["lyrics"]["braveApiKey"],
        ""
    );
    assert_eq!(
        app.store().lock().unwrap().data()["lyrics"]["googleTranslateApiKey"],
        ""
    );
    assert_eq!(services.take()[0].0, "broadcast");
}
#[test]
fn capture_merges_before_normalizing() {
    let (_dir, mut app, services) = app();
    app.set("capture", json!({"folder":"/tmp/recordings"}))
        .unwrap();
    app.set("capture", json!({"bitrateMbps":-1})).unwrap();
    assert_eq!(app.get("capture")["folder"], "/tmp/recordings");
    assert_eq!(app.get("capture")["bitrateMbps"], 2);
    assert!(services.take().is_empty());
}
#[test]
fn web_normalizes_without_broadcast() {
    let (_dir, mut app, services) = app();
    app.set("web", json!({"bookmarks":null})).unwrap();
    assert!(app.get("web")["bookmarks"].is_array());
    assert!(services.take().is_empty());
}
#[test]
fn documents_normalizes_without_broadcast() {
    let (_dir, mut app, services) = app();
    app.set("documents", json!({"slideshowSec":-100})).unwrap();
    assert_eq!(app.get("documents")["slideshowSec"], 2);
    assert!(services.take().is_empty());
}
#[test]
fn theme_library_normalizes_without_broadcast() {
    let (_dir, mut app, services) = app();
    app.set("themeLibrary", Value::Null).unwrap();
    assert!(app.get("themeLibrary").is_array());
    assert!(services.take().is_empty());
}
#[test]
fn church_syncs_only_profile_fields_after_storing() {
    let (_dir, mut app, services) = app();
    let value = json!({"name":"Church", "timezone":"Africa/Lagos", "serviceTimes":[], "other":123});
    app.set("church", value.clone()).unwrap();
    assert_eq!(app.get("church"), value);
    assert_eq!(
        services.take(),
        vec![(
            "church".into(),
            json!({"name":"Church", "timezone":"Africa/Lagos", "serviceTimes":[]})
        )]
    );
}
#[test]
fn scripture_updates_services_in_source_order() {
    let (_dir, mut app, services) = app();
    let value = json!({"defaultTranslation":"KJV", "autoMode":true, "confidenceThreshold":0.8});
    app.set("scripture", value.clone()).unwrap();
    assert_eq!(app.get("scripture"), value);
    assert_eq!(
        services.take(),
        vec![
            ("scripture.translation".into(), json!("KJV")),
            ("scripture.auto".into(), json!(true)),
            ("scripture.confidence".into(), json!(0.8)),
            ("orchestrator.translation".into(), json!("KJV")),
            ("orchestrator.auto".into(), json!(true)),
            ("orchestrator.confidence".into(), json!(0.8)),
        ]
    );
}
#[test]
fn retired_settings_are_rejected_without_effects() {
    let (_dir, mut app, services) = app();
    for key in ["propresenter", "propresenterResources"] {
        assert_eq!(
            app.set(key, json!({})).unwrap_err().to_string(),
            "ProPresenter settings are no longer supported"
        );
        assert!(app.get(key).is_null());
    }
    assert!(services.take().is_empty());
}
#[test]
fn all_reads_redact_even_legacy_raw_keys_and_use_licence_status() {
    let (_dir, app, _services) = app();
    app.store()
        .lock()
        .unwrap()
        .merge_fields("stt", [("apiKey", json!("secret"))]);
    assert_eq!(app.get("stt")["apiKey"], "");
    assert_eq!(app.get_all()["stt"]["apiKey"], "");
    assert_eq!(app.get_all()["secretsConfigured"]["bible"], true);
    assert_eq!(app.get_all()["secretsConfigured"]["deepgram"], false);
    assert_eq!(
        SettingsWiring::test_api_key(),
        json!({"ok":false,"message":"Provider credentials are managed in your church account."})
    );
}
