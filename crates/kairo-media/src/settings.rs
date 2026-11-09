//! The two settings sections these libraries read and write: `media` and `tracks`.

use kairo_store::settings::SettingsStore;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Read and write one top-level settings key, as `store.get`/`store.set` do.
pub trait SettingsPort: Send {
    fn get(&self, key: &str) -> Option<Value>;
    fn set(&mut self, key: &str, value: Value);
}

impl SettingsPort for SettingsStore {
    fn get(&self, key: &str) -> Option<Value> {
        self.data().get(key).cloned()
    }

    fn set(&mut self, key: &str, value: Value) {
        self.set_raw(key, value);
    }
}

impl<T: SettingsPort> SettingsPort for Arc<Mutex<T>> {
    fn get(&self, key: &str) -> Option<Value> {
        self.lock().unwrap_or_else(|e| e.into_inner()).get(key)
    }

    fn set(&mut self, key: &str, value: Value) {
        self.lock()
            .unwrap_or_else(|e| e.into_inner())
            .set(key, value);
    }
}

/// `media.folder` or `tracks.folder` when it is a string, else empty.
pub(crate) fn folder_of(settings: &dyn SettingsPort, key: &str) -> String {
    settings
        .get(key)
        .and_then(|v| v.get("folder").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}
