//! What happens when Kairo fails: the native counterpart of the
//! `uncaughtException` and `unhandledRejection` handlers in `src/main/index.ts`.
//!
//! The panic hook logs the panic, saves the recovery snapshot the way
//! `resilienceManager.serializeState` does (`session-recovery.json`,
//! two-space JSON), adds the crash to a local ledger, and then runs the
//! previous hook. Every step catches its own failure: a hook that fails must
//! not hide the original panic.
//!
//! Crashes are recorded locally only, as Electron does. Sending them is the
//! crash-reports service (prompt 032), which can read the ledger.

use crate::fs_atomic::write_atomic;
use kairo_domain::cloud::scrub_home;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::atomic::{AtomicU8, Ordering},
};

/// Crashes kept in the ledger, newest last (`CRASH_MAX_REPORTS`).
pub const MAX_LEDGER_ENTRIES: usize = kairo_domain::cloud::CRASH_MAX_REPORTS;
/// Longest message kept per crash (`CRASH_MAX_ERROR_CHARS`).
pub const MAX_MESSAGE_CHARS: usize = kairo_domain::cloud::CRASH_MAX_ERROR_CHARS;

/// One recorded panic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashEntry {
    /// ISO time.
    pub at: String,
    pub thread: String,
    pub message: String,
    /// `file:line:column`, when known.
    pub location: Option<String>,
    pub app_version: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashConsent {
    pub share_stats: bool,
    pub send_crash_reports: bool,
}

const CONSENT_UNKNOWN: u8 = 4;
static CONSENT: AtomicU8 = AtomicU8::new(CONSENT_UNKNOWN);

/// Snapshot only the two privacy flags for the panic hook. The hook does not
/// acquire a settings lock and always keeps its local diagnostic entry.
pub fn set_crash_consent(consent: CrashConsent) {
    CONSENT.store(
        u8::from(consent.share_stats) | (u8::from(consent.send_crash_reports) << 1),
        Ordering::Release,
    );
}

/// The ledger file: a running count and the latest crashes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashLedger {
    /// Every crash recorded since the ledger was created.
    pub count: u64,
    pub crashes: Vec<CrashEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub consent: BTreeMap<u64, CrashConsent>,
}

impl CrashLedger {
    /// Reads the ledger; a missing or unreadable file is an empty ledger.
    pub fn load(path: &Path) -> CrashLedger {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Adds `entry`, keeping the newest [`MAX_LEDGER_ENTRIES`].
    pub fn push(&mut self, entry: CrashEntry) {
        self.count += 1;
        self.crashes.push(entry);
        let excess = self.crashes.len().saturating_sub(MAX_LEDGER_ENTRIES);
        self.crashes.drain(..excess);
        let first = self.count.saturating_sub(self.crashes.len() as u64);
        self.consent.retain(|sequence, _| *sequence > first);
    }
}

/// Adds a crash to the ledger at `path`.
pub fn record_crash(path: &Path, entry: CrashEntry) -> std::io::Result<()> {
    // Panics on two threads at once must not each read the old ledger and
    // overwrite the other's crash.
    static LEDGER: Mutex<()> = Mutex::new(());
    let _ledger = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
    let mut ledger = CrashLedger::load(path);
    ledger.push(entry);
    let consent = CONSENT.load(Ordering::Acquire);
    if consent != CONSENT_UNKNOWN {
        ledger.consent.insert(
            ledger.count,
            CrashConsent {
                share_stats: consent & 1 != 0,
                send_crash_reports: consent & 2 != 0,
            },
        );
    }
    write_pretty_json(path, &ledger)
}

/// `saveSessionState`: the snapshot as two-space JSON, as `JSON.stringify(state, null, 2)` writes it.
pub fn save_recovery_state(path: &Path, state: &Value) -> std::io::Result<()> {
    write_pretty_json(path, state)
}

fn write_pretty_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, text.as_bytes())
}

type StateProvider = Box<dyn Fn() -> Option<Value> + Send + Sync>;

/// The current recovery snapshot, set by the core runtime
/// (`registerStateCallback`). `None` until something registers.
static STATE_PROVIDER: Mutex<Option<StateProvider>> = Mutex::new(None);

/// Registers (or with `None`, clears) the function that returns the
/// recovery snapshot. It runs inside the panic hook, so it must not panic
/// (a second panic aborts the process) and must not wait on locks the
/// panicking thread may hold; it should read a snapshot kept up to date
/// elsewhere.
pub fn set_recovery_state_provider(provider: Option<StateProvider>) {
    *STATE_PROVIDER.lock().unwrap_or_else(|e| e.into_inner()) = provider;
}

/// Where the hook writes.
#[derive(Clone, Debug)]
pub struct CrashConfig {
    pub recovery_file: PathBuf,
    pub ledger_file: PathBuf,
    /// For scrubbing the user's home folder out of messages.
    pub home: String,
    pub windows: bool,
    pub app_version: String,
}

static CONFIG: OnceLock<CrashConfig> = OnceLock::new();

thread_local! {
    static IN_HOOK: Cell<bool> = const { Cell::new(false) };
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Box<dyn Any>".to_string()
    }
}

/// Builds the ledger entry for a panic, with the home folder scrubbed.
pub fn crash_entry(config: &CrashConfig, info: &PanicHookInfo<'_>) -> CrashEntry {
    let scrub = |text: &str| scrub_home(text, &config.home, config.windows);
    let message: String = scrub(&panic_message(info))
        .chars()
        .take(MAX_MESSAGE_CHARS)
        .collect();
    CrashEntry {
        at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        thread: std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string(),
        message,
        location: info
            .location()
            .map(|l| scrub(&format!("{}:{}:{}", l.file(), l.line(), l.column()))),
        app_version: config.app_version.clone(),
    }
}

/// Runs the hook's steps for one panic. Public for tests.
pub fn handle_panic(config: &CrashConfig, info: &PanicHookInfo<'_>) {
    let entry = crash_entry(config, info);
    tracing::error!(
        thread = %entry.thread,
        location = entry.location.as_deref().unwrap_or("unknown"),
        "Uncaught panic: {}",
        entry.message
    );
    // `try_lock`: the panicking thread may be the one registering a provider.
    let state = match STATE_PROVIDER.try_lock() {
        Ok(provider) => provider.as_ref().and_then(|f| f()),
        Err(_) => {
            tracing::error!(
                "Failed to serialize state during panic: the snapshot provider is busy"
            );
            None
        }
    };
    if let Some(state) = state
        && let Err(error) = save_recovery_state(&config.recovery_file, &state)
    {
        tracing::error!(%error, "Failed to serialize state during panic");
    }
    if let Err(error) = record_crash(&config.ledger_file, entry) {
        tracing::error!(%error, "[Crashes] could not record");
    }
}

/// Installs the panic hook once. Later calls keep the first configuration.
pub fn install_panic_hook(config: CrashConfig) {
    if CONFIG.set(config).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let reentered = IN_HOOK.with(|flag| flag.replace(true));
        if !reentered && let Some(config) = CONFIG.get() {
            handle_panic(config, info);
        }
        IN_HOOK.with(|flag| flag.set(false));
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(dir: &Path) -> CrashConfig {
        CrashConfig {
            recovery_file: dir.join("session-recovery.json"),
            ledger_file: dir.join("kairo-native-crashes.json"),
            home: "/Users/ada".into(),
            windows: false,
            app_version: "0.1.0".into(),
        }
    }

    static HOOK: Mutex<()> = Mutex::new(());

    /// Runs `body` under a temporary hook that calls [`handle_panic`]. The
    /// hook is process-wide, so tests that swap it take turns.
    fn with_panic(config: &CrashConfig, body: impl FnOnce() + std::panic::UnwindSafe) {
        let _turn = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        let config = config.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| handle_panic(&config, info)));
        let result = std::panic::catch_unwind(body);
        std::panic::set_hook(previous);
        assert!(result.is_err());
    }

    #[test]
    fn the_ledger_counts_and_keeps_the_newest() {
        let mut ledger = CrashLedger::default();
        for i in 0..15 {
            ledger.push(CrashEntry {
                at: i.to_string(),
                thread: "main".into(),
                message: "m".into(),
                location: None,
                app_version: "1".into(),
            });
        }
        assert_eq!(ledger.count, 15);
        assert_eq!(ledger.crashes.len(), MAX_LEDGER_ENTRIES);
        assert_eq!(ledger.crashes[0].at, "5");
    }

    #[test]
    fn crashes_recorded_at_once_are_all_counted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.json");
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    record_crash(
                        &path,
                        CrashEntry {
                            at: i.to_string(),
                            thread: "t".into(),
                            message: "m".into(),
                            location: None,
                            app_version: "1".into(),
                        },
                    )
                    .unwrap()
                })
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        assert_eq!(CrashLedger::load(&path).count, 8);
    }

    #[test]
    fn an_unreadable_ledger_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.json");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(CrashLedger::load(&path), CrashLedger::default());
        assert_eq!(
            CrashLedger::load(&dir.path().join("missing.json")),
            CrashLedger::default()
        );
    }

    #[test]
    fn a_panic_is_logged_saved_and_recorded() {
        // One test owns the global provider, so tests cannot race on it.
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        set_recovery_state_provider(Some(Box::new(|| Some(json!({ "slide": 3 })))));
        with_panic(&config, || panic!("boom in /Users/ada/Songs/x.json"));
        let recovery = std::fs::read_to_string(&config.recovery_file).unwrap();
        assert_eq!(recovery, "{\n  \"slide\": 3\n}");
        let ledger = CrashLedger::load(&config.ledger_file);
        assert_eq!(ledger.count, 1);
        assert_eq!(ledger.crashes[0].message, "boom in ~/Songs/x.json");
        assert!(
            ledger.crashes[0]
                .location
                .as_deref()
                .unwrap()
                .contains("crash.rs")
        );

        // No snapshot: nothing is written, the crash still counts.
        std::fs::remove_file(&config.recovery_file).unwrap();
        set_recovery_state_provider(Some(Box::new(|| None)));
        with_panic(&config, || panic!("{}", String::from("second")));
        assert!(!config.recovery_file.exists());
        assert_eq!(CrashLedger::load(&config.ledger_file).count, 2);

        set_recovery_state_provider(None);

        // An unwritable folder fails quietly.
        let bad = CrashConfig {
            ledger_file: PathBuf::from("/nonexistent/\0/x.json"),
            ..config.clone()
        };
        with_panic(&bad, || panic!("third"));
    }

    #[test]
    fn long_messages_are_cut() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        with_panic(&config, || panic!("{}", "x".repeat(5_000)));
        let ledger = CrashLedger::load(&config.ledger_file);
        assert_eq!(ledger.crashes[0].message.chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn atomic_writes_replace_whole_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/file.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
}
