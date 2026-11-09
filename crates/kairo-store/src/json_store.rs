//! One JSON store file, compatible with electron-store 8 (conf 10).
//!
//! The same file both apps read: `<userData>/<name>.json`, written as
//! `JSON.stringify(data, undefined, '\t')`. Opening merges `defaults` into the
//! file's top level the way conf does (`Object.assign({}, defaults, file)`, a
//! shallow merge) and writes the result back only if that added keys.
//! [`JsonStore::on_did_change`] calls back when a key's value changes, as
//! `store.onDidChange` does.
//!
//! Where this differs from electron-store, on purpose (DECISIONS.md, 021):
//!
//! - Values live in memory after opening; electron-store re-reads the file on
//!   every `get`. Only this process writes the file while it runs.
//! - Writes happen on a background thread. A burst of sets becomes one write
//!   of the newest value; [`JsonStore::flush`] waits for it. Each write is a
//!   temporary file, `fsync` and rename, so the file always parses.
//! - A file that is not valid JSON is kept as `<name>.corrupt-<time>.json` and
//!   the store starts from its defaults; electron-store throws and the app
//!   does not start.
//! - A failed write (read-only file, full disk) is logged and remembered in
//!   [`JsonStore::last_write_error`]; values stay in memory. electron-store throws.

use crate::crash::write_atomic;
use crate::json_text::stringify_tabs;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

type Data = Map<String, Value>;
type Listener = Box<dyn FnMut(&Value, &Value) + Send>;

/// What opening found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenState {
    /// No file yet: a fresh store.
    Missing,
    /// The file parsed.
    Read,
    /// The file did not parse; it was moved to this path.
    Corrupt(PathBuf),
    /// The file could not be read (permissions, not a file).
    Unreadable(String),
}

/// Identifies a listener, for [`JsonStore::remove_listener`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ListenerId(u64);

/// `Object.assign({}, value)` for a parsed file: an object as is, an array or
/// string by index, anything else empty.
pub fn js_assign_source(value: Value) -> Data {
    match value {
        Value::Object(map) => map,
        Value::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        Value::String(s) => s
            .encode_utf16()
            .enumerate()
            .map(|(i, unit)| {
                (
                    i.to_string(),
                    Value::String(String::from_utf16_lossy(&[unit])),
                )
            })
            .collect(),
        _ => Data::new(),
    }
}

#[derive(Default)]
struct WriterState {
    pending: Option<Arc<Data>>,
    queued: u64,
    written: u64,
    writing: bool,
    last_error: Option<String>,
    stop: bool,
}

struct Writer {
    shared: Arc<(Mutex<WriterState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    fn start(path: PathBuf) -> Writer {
        let shared = Arc::new((Mutex::new(WriterState::default()), Condvar::new()));
        let worker = shared.clone();
        let name = format!(
            "store-{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("json")
        );
        let thread = std::thread::Builder::new()
            .name(name)
            .spawn(move || {
                let (lock, signal) = &*worker;
                loop {
                    let (data, generation) = {
                        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                        while state.pending.is_none() && !state.stop {
                            state = signal.wait(state).unwrap_or_else(|e| e.into_inner());
                        }
                        let Some(data) = state.pending.take() else {
                            return;
                        };
                        state.writing = true;
                        (data, state.queued)
                    };
                    let result = write_atomic(
                        &path,
                        stringify_tabs(&Value::Object((*data).clone())).as_bytes(),
                    );
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    state.writing = false;
                    state.written = generation;
                    match result {
                        Ok(()) => state.last_error = None,
                        Err(error) => {
                            let text = format!("{}: {error}", path.display());
                            if state.last_error.as_deref() != Some(text.as_str()) {
                                tracing::error!(error = %text, "[Store] Could not save");
                            }
                            state.last_error = Some(text);
                        }
                    }
                    signal.notify_all();
                }
            })
            .expect("spawn store writer");
        Writer {
            shared,
            thread: Some(thread),
        }
    }

    fn submit(&self, data: Arc<Data>) {
        let (lock, signal) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        state.pending = Some(data);
        state.queued += 1;
        signal.notify_all();
    }

    fn flush(&self) -> Result<(), String> {
        let (lock, signal) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        let target = state.queued;
        while state.written < target {
            state = signal.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        state.last_error.clone().map_or(Ok(()), Err)
    }

    fn last_error(&self) -> Option<String> {
        self.shared
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_error
            .clone()
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.flush();
        {
            let (lock, signal) = &*self.shared;
            lock.lock().unwrap_or_else(|e| e.into_inner()).stop = true;
            signal.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// An open store file.
pub struct JsonStore {
    path: PathBuf,
    data: Arc<Data>,
    opened: OpenState,
    writer: Writer,
    listeners: Vec<(ListenerId, Option<String>, Listener)>,
    next_listener: u64,
}

/// The file for store `name` in `dir`.
pub fn store_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

/// Removes temporary files a killed write left for store `name` (named by
/// [`write_atomic`]). Only one Kairo runs per profile, so none is in use.
fn remove_stale_temp_files(dir: &Path, name: &str) {
    let prefix = format!(".{name}.json.tmp-");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Reads a store file as conf does, before defaults. `None` data means "start empty".
pub fn read_file(path: &Path) -> (Data, OpenState) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Data::new(), OpenState::Missing);
        }
        Err(error) => return (Data::new(), OpenState::Unreadable(error.to_string())),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => (js_assign_source(value), OpenState::Read),
        Err(error) => {
            let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("store");
            let aside = path.with_file_name(format!("{stem}.corrupt-{stamp}.json"));
            match std::fs::rename(path, &aside) {
                Ok(()) => {
                    tracing::warn!(%error, kept = %aside.display(), "[Store] Settings file was not valid JSON; started from defaults")
                }
                Err(rename) => {
                    tracing::error!(%error, %rename, "[Store] Settings file was not valid JSON and could not be moved aside")
                }
            }
            (Data::new(), OpenState::Corrupt(aside))
        }
    }
}

impl JsonStore {
    /// Opens `<dir>/<name>.json` with `defaults` merged into its top level.
    pub fn open(dir: &Path, name: &str, defaults: Data) -> JsonStore {
        let path = store_path(dir, name);
        remove_stale_temp_files(dir, name);
        let (file, opened) = read_file(&path);
        let mut merged = defaults;
        for (key, value) in file.iter() {
            merged.insert(key.clone(), value.clone());
        }
        let writer = Writer::start(path.clone());
        let store = JsonStore {
            path,
            data: Arc::new(merged),
            opened,
            writer,
            listeners: Vec::new(),
            next_listener: 0,
        };
        // conf writes back only when the merge changed something. An unreadable
        // file is never overwritten.
        if *store.data != file && !matches!(store.opened, OpenState::Unreadable(_)) {
            store.writer.submit(store.data.clone());
        }
        store
    }

    /// The file this store writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What opening found.
    pub fn open_state(&self) -> &OpenState {
        &self.opened
    }

    /// `store.store`: every key.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// `store.get(key)`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    /// `key` read as `T`.
    pub fn get_as<T: DeserializeOwned>(&self, key: &str) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.get(key).cloned().unwrap_or(Value::Null))
    }

    /// `store.set(key, value)`. A key keeps its place; a new key goes last.
    pub fn set(&mut self, key: &str, value: Value) {
        let old = self.data.get(key).cloned().unwrap_or(Value::Null);
        Arc::make_mut(&mut self.data).insert(key.to_string(), value);
        self.changed(&[(key.to_string(), old)]);
    }

    /// `key` written from `value`, with whole floats as integers.
    pub fn set_as<T: Serialize>(&mut self, key: &str, value: &T) -> Result<(), serde_json::Error> {
        self.set(key, kairo_domain::json::to_js_value(value)?);
        Ok(())
    }

    /// `store.store = data`: replaces every key.
    pub fn replace(&mut self, data: Data) {
        let before = std::mem::replace(&mut self.data, Arc::new(data));
        let keys: Vec<(String, Value)> = self
            .listeners
            .iter()
            .filter_map(|(_, key, _)| key.clone())
            .map(|key| {
                let old = before.get(&key).cloned().unwrap_or(Value::Null);
                (key, old)
            })
            .collect();
        self.changed(&keys);
        // Whole-store listeners see every replace.
        if self.listeners.iter().any(|(_, key, _)| key.is_none()) {
            let old = Value::Object((*before).clone());
            let new = Value::Object((*self.data).clone());
            for (_, key, listener) in &mut self.listeners {
                if key.is_none() && old != new {
                    listener(&new, &old);
                }
            }
        }
    }

    fn changed(&mut self, keys: &[(String, Value)]) {
        self.writer.submit(self.data.clone());
        for (changed, old) in keys {
            let new = self.data.get(changed).cloned().unwrap_or(Value::Null);
            if &new == old {
                continue;
            }
            for (_, key, listener) in &mut self.listeners {
                if key.as_deref() == Some(changed.as_str()) {
                    listener(&new, old);
                }
            }
        }
    }

    /// `store.onDidChange(key, callback)`: called with `(new, old)` when the
    /// value at `key` changes. Absent values are `null`.
    pub fn on_did_change(
        &mut self,
        key: &str,
        callback: impl FnMut(&Value, &Value) + Send + 'static,
    ) -> ListenerId {
        self.add_listener(Some(key.to_string()), Box::new(callback))
    }

    /// `store.onDidAnyChange(callback)`, for whole-store replaces.
    pub fn on_did_any_change(
        &mut self,
        callback: impl FnMut(&Value, &Value) + Send + 'static,
    ) -> ListenerId {
        self.add_listener(None, Box::new(callback))
    }

    fn add_listener(&mut self, key: Option<String>, callback: Listener) -> ListenerId {
        self.next_listener += 1;
        let id = ListenerId(self.next_listener);
        self.listeners.push((id, key, callback));
        id
    }

    /// Stops a listener.
    pub fn remove_listener(&mut self, id: ListenerId) {
        self.listeners.retain(|(listener, _, _)| *listener != id);
    }

    /// Waits until everything set so far is on disk. Returns the latest write error.
    pub fn flush(&self) -> Result<(), String> {
        self.writer.flush()
    }

    /// The latest write failure, cleared by the next successful write.
    pub fn last_write_error(&self) -> Option<String> {
        self.writer.last_error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn defaults() -> Data {
        json!({ "a": 1, "list": [] }).as_object().unwrap().clone()
    }

    fn text(dir: &Path, name: &str) -> String {
        std::fs::read_to_string(store_path(dir, name)).unwrap()
    }

    #[test]
    fn a_fresh_store_writes_its_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonStore::open(dir.path(), "s", defaults());
        store.flush().unwrap();
        assert_eq!(store.open_state(), &OpenState::Missing);
        assert_eq!(text(dir.path(), "s"), "{\n\t\"a\": 1,\n\t\"list\": []\n}");
    }

    #[test]
    fn defaults_merge_shallowly_and_file_order_follows_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(store_path(dir.path(), "s"), r#"{"extra":true,"a":{"x":1}}"#).unwrap();
        let store = JsonStore::open(dir.path(), "s", defaults());
        store.flush().unwrap();
        // Object.assign({}, defaults, file): default keys first, file values win, no deep merge.
        assert_eq!(
            Value::Object(store.data().clone()),
            json!({ "a": { "x": 1 }, "list": [], "extra": true })
        );
        assert_eq!(
            text(dir.path(), "s"),
            "{\n\t\"a\": {\n\t\t\"x\": 1\n\t},\n\t\"list\": [],\n\t\"extra\": true\n}"
        );
    }

    #[test]
    fn a_complete_file_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let original = r#"{"a":2,"list":[1]}"#;
        std::fs::write(store_path(dir.path(), "s"), original).unwrap();
        let store = JsonStore::open(dir.path(), "s", defaults());
        store.flush().unwrap();
        assert_eq!(text(dir.path(), "s"), original);
    }

    #[test]
    fn non_object_files_assign_like_javascript() {
        assert_eq!(
            Value::Object(js_assign_source(json!([5, 6]))),
            json!({ "0": 5, "1": 6 })
        );
        assert_eq!(
            Value::Object(js_assign_source(json!("ab"))),
            json!({ "0": "a", "1": "b" })
        );
        assert!(js_assign_source(json!(3)).is_empty() && js_assign_source(json!(null)).is_empty());
    }

    #[test]
    fn a_corrupt_file_is_kept_aside_and_defaults_used() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(store_path(dir.path(), "s"), "{\"a\": 1,").unwrap();
        let store = JsonStore::open(dir.path(), "s", defaults());
        store.flush().unwrap();
        let OpenState::Corrupt(aside) = store.open_state() else {
            panic!("{:?}", store.open_state())
        };
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "{\"a\": 1,");
        assert_eq!(store.get("a"), Some(&json!(1)));
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_file_keeps_values_in_memory_and_reports() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = store_path(dir.path(), "s");
        std::fs::write(&path, r#"{"a":2}"#).unwrap();
        // The folder is read-only too, as a rename can replace a read-only file.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut store = JsonStore::open(dir.path(), "s", defaults());
        store.set("a", json!(3));
        let result = store.flush();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert!(store.last_write_error().is_some());
        assert_eq!(store.get("a"), Some(&json!(3)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"a":2}"#);
    }

    #[test]
    fn listeners_fire_only_on_real_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = JsonStore::open(dir.path(), "s", defaults());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let id = store.on_did_change("a", move |new, old| {
            seen.lock().unwrap().push((new.clone(), old.clone()))
        });
        store.set("a", json!(1));
        store.set("a", json!(2));
        store.set("list", json!([1]));
        store.replace(json!({ "a": 5 }).as_object().unwrap().clone());
        store.remove_listener(id);
        store.set("a", json!(9));
        assert_eq!(
            *calls.lock().unwrap(),
            vec![(json!(2), json!(1)), (json!(5), json!(2))]
        );
        let any = Arc::new(AtomicUsize::new(0));
        let count = any.clone();
        store.on_did_any_change(move |_, _| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        store.replace(store.data().clone());
        store.replace(json!({}).as_object().unwrap().clone());
        assert_eq!(any.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_burst_of_sets_ends_with_the_last_value_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = JsonStore::open(dir.path(), "s", defaults());
        for i in 0..2_000 {
            store.set("a", json!(i));
        }
        store.flush().unwrap();
        let on_disk: Value = serde_json::from_str(&text(dir.path(), "s")).unwrap();
        assert_eq!(on_disk["a"], json!(1999));
        // Only the store file remains: no temporary files left behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn dropping_the_store_saves_pending_values() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = JsonStore::open(dir.path(), "s", defaults());
            store.set("a", json!("last"));
        }
        let on_disk: Value = serde_json::from_str(&text(dir.path(), "s")).unwrap();
        assert_eq!(on_disk["a"], json!("last"));
    }
}

#[cfg(test)]
mod temp_tests {
    use super::*;

    #[test]
    fn opening_removes_temp_files_from_killed_writes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".s.json.tmp-123-4"), "partial").unwrap();
        std::fs::write(dir.path().join(".other.json.tmp-123-4"), "not ours").unwrap();
        let store = JsonStore::open(dir.path(), "s", Data::new());
        store.flush().unwrap();
        assert!(!dir.path().join(".s.json.tmp-123-4").exists());
        assert!(dir.path().join(".other.json.tmp-123-4").exists());
    }
}
