//! Consent-gated daily counters and content-free crash summaries.

use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex as SyncMutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, Local, SecondsFormat, Utc};
use kairo_domain::{
    cloud::{
        CRASH_MAX_ERRORS, CRASH_MAX_REPORTS, CrashKind, CrashReport, CrashSystem, CrashUpload,
    },
    usage::{
        LedgerState, LedgerStorage, UsageDayReport, UsageErrorKind, UsageFeature, UsageLedger,
        UsageReport, UsageSystem, is_usage_day, local_day,
    },
};
use kairo_store::{
    crash::{CrashConsent, CrashLedger, set_crash_consent},
    json_store::{JsonStore, ListenerId},
    paths::CRASH_LEDGER_FILE_NAME,
    settings::SettingsStore,
};
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, Notify, watch},
    time::{Instant, MissedTickBehavior, interval_at, sleep_until},
};

use crate::{CloudSession, service_records::ServiceUsage};

const USAGE: u8 = 1;
const CRASH: u8 = 2;
const ALL: u8 = USAGE | CRASH;
const CRASH_DELAY: Duration = Duration::from_secs(5);

fn allowed(value: &Value) -> u8 {
    let mut flags = ALL;
    if value.get("shareStats").and_then(Value::as_bool) == Some(false) {
        flags &= !USAGE;
    }
    if value.get("sendCrashReports").and_then(Value::as_bool) == Some(false) {
        flags &= !CRASH;
    }
    flags
}

#[derive(Default)]
struct Privacy {
    enabled: AtomicU8,
    withdrawn: AtomicU8,
    changed: Notify,
    revocations: watch::Sender<[u64; 2]>,
}

impl Privacy {
    fn set(&self, enabled: u8) {
        set_crash_consent(CrashConsent {
            share_stats: enabled & USAGE != 0,
            send_crash_reports: enabled & CRASH != 0,
        });
        let previous = self.enabled.swap(enabled, Ordering::AcqRel);
        let revoked = previous & !enabled;
        self.withdrawn.fetch_or(revoked, Ordering::Release);
        if revoked != 0 {
            self.revocations.send_modify(|revisions| {
                for (index, bit) in [USAGE, CRASH].into_iter().enumerate() {
                    if revoked & bit != 0 {
                        revisions[index] = revisions[index].wrapping_add(1);
                    }
                }
            });
        }
        self.changed.notify_one();
    }

    async fn until_revoked<T>(
        &self,
        index: usize,
        mut revisions: watch::Receiver<[u64; 2]>,
        work: impl Future<Output = T>,
    ) -> Option<T> {
        let current = revisions.borrow()[index];
        if revisions.has_changed().unwrap_or(true)
            || self.enabled.load(Ordering::Acquire) & (1 << index) == 0
        {
            return None;
        }
        tokio::select! {
            biased;
            _ = revisions.wait_for(|next| next[index] != current) => None,
            result = work => Some(result),
        }
    }
}

struct UsageStore(JsonStore);

impl LedgerStorage for UsageStore {
    fn read(&self) -> LedgerState {
        LedgerState {
            days: self.0.get_as("days").unwrap_or_default(),
            extra: Default::default(),
        }
    }

    fn write(&mut self, state: LedgerState) {
        let _ = self.0.set_as("days", &state.days);
    }
}

fn today() -> String {
    local_day(Local::now())
}

struct LocalReports {
    usage: UsageLedger<UsageStore, fn() -> String>,
    crashes: JsonStore,
    context: VecDeque<CrashContext>,
    last_wake: Option<Instant>,
    usage_revision: u64,
    crash_revision: u64,
}

impl LocalReports {
    fn reports(&self) -> Vec<CrashReport> {
        self.crashes.get_as("reports").unwrap_or_default()
    }

    fn set_reports(&mut self, reports: &[CrashReport]) {
        let _ = self.crashes.set_as("reports", &reports);
    }

    fn enforce(&mut self, privacy: &Privacy) -> u8 {
        let current = privacy.enabled.load(Ordering::Acquire);
        let purge = privacy.withdrawn.swap(0, Ordering::AcqRel) | (ALL & !current);
        if purge & USAGE != 0 {
            self.usage_revision = self.usage_revision.wrapping_add(1);
            if !self.usage.pending().is_empty() {
                self.usage.clear();
            }
        }
        if purge & CRASH != 0 {
            self.crash_revision = self.crash_revision.wrapping_add(1);
            self.context.clear();
            if !self.reports().is_empty() {
                self.set_reports(&[]);
            }
        }
        current
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeCrash {
    Engine,
    WebHost,
    UiPanic,
    Window,
}

impl NativeCrash {
    fn wire(self) -> (CrashKind, &'static str, UsageErrorKind) {
        match self {
            Self::Engine => (
                CrashKind::ChildProcess,
                "engine",
                UsageErrorKind::ChildProcessGone,
            ),
            Self::WebHost => (
                CrashKind::ChildProcess,
                "web-host",
                UsageErrorKind::ChildProcessGone,
            ),
            Self::UiPanic => (CrashKind::Renderer, "ui-panic", UsageErrorKind::Uncaught),
            Self::Window => (CrashKind::Renderer, "window", UsageErrorKind::RendererGone),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CrashReason {
    Panic,
    AbnormalExit,
    Killed,
    OutOfMemory,
    LaunchFailed,
    IntegrityFailure,
}

impl CrashReason {
    fn label(self) -> &'static str {
        match self {
            Self::Panic => "panic",
            Self::AbnormalExit => "abnormal-exit",
            Self::Killed => "killed",
            Self::OutOfMemory => "oom",
            Self::LaunchFailed => "launch-failed",
            Self::IntegrityFailure => "integrity-failure",
        }
    }
}

/// Diagnostic breadcrumbs deliberately accept categories only. A log line,
/// transcript, path, slide title or provider response cannot enter a report.
#[derive(Clone, Copy, Debug)]
pub enum CrashContext {
    Audio,
    Capture,
    Engine,
    Graphics,
    Network,
    Output,
    WebHost,
    Window,
}

impl CrashContext {
    fn label(self) -> &'static str {
        match self {
            Self::Audio => "audio error",
            Self::Capture => "capture error",
            Self::Engine => "engine error",
            Self::Graphics => "graphics error",
            Self::Network => "network error",
            Self::Output => "output error",
            Self::WebHost => "web host error",
            Self::Window => "window error",
        }
    }
}

struct Flight<'a>(&'a AtomicBool);

impl<'a> Flight<'a> {
    fn begin(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(Self(flag))
    }
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The reporting owner. Open and run on AppCore's background runtime.
pub struct Telemetry {
    cloud: CloudSession,
    settings: Arc<SyncMutex<SettingsStore>>,
    listener: ListenerId,
    privacy: Arc<Privacy>,
    local: Mutex<LocalReports>,
    usage_flight: AtomicBool,
    crash_flight: AtomicBool,
    running: AtomicBool,
    crash_ready: Notify,
    memory_gb: f64,
}

impl Telemetry {
    pub async fn open(
        cloud: CloudSession,
        user_data: PathBuf,
        settings: Arc<SyncMutex<SettingsStore>>,
    ) -> io::Result<Self> {
        let privacy = Arc::new(Privacy::default());
        let (listener, mut local, ledger, memory_gb) = tokio::task::spawn_blocking({
            let settings = settings.clone();
            let privacy = privacy.clone();
            move || {
                let listener = {
                    let mut settings = settings
                        .lock()
                        .map_err(|_| io::Error::other("settings unavailable"))?;
                    privacy.set(allowed(
                        settings.data().get("usage").unwrap_or(&Value::Null),
                    ));
                    let privacy = Arc::downgrade(&privacy);
                    settings.on_did_change("usage", move |next, _| {
                        if let Some(privacy) = privacy.upgrade() {
                            privacy.set(allowed(next));
                        }
                    })
                };
                let usage = JsonStore::open(
                    &user_data,
                    "kairo-usage",
                    serde_json::from_value(json!({"days":{}})).unwrap(),
                );
                let crashes = JsonStore::open(
                    &user_data,
                    "kairo-crash-reports",
                    serde_json::from_value(json!({"reports":[]})).unwrap(),
                );
                let ledger = CrashLedger::load(&user_data.join(CRASH_LEDGER_FILE_NAME));
                let mut system = sysinfo::System::new();
                system.refresh_memory();
                let memory_gb = (system.total_memory() as f64 / 1024_f64.powi(3)).round();
                Ok::<_, io::Error>((
                    listener,
                    LocalReports {
                        usage: UsageLedger::new(UsageStore(usage), today as fn() -> String),
                        crashes,
                        context: VecDeque::new(),
                        last_wake: None,
                        usage_revision: 0,
                        crash_revision: 0,
                    },
                    ledger,
                    memory_gb,
                ))
            }
        })
        .await
        .map_err(io::Error::other)??;
        let flags = local.enforce(&privacy);
        import_panics(&mut local, ledger, flags, crash_system(&cloud, memory_gb));
        Ok(Self {
            cloud,
            settings,
            listener,
            privacy,
            local: Mutex::new(local),
            usage_flight: AtomicBool::new(false),
            crash_flight: AtomicBool::new(false),
            running: AtomicBool::new(false),
            crash_ready: Notify::new(),
            memory_gb,
        })
    }

    pub async fn track(&self, feature: UsageFeature, count: f64) {
        let mut local = self.local.lock().await;
        if local.enforce(&self.privacy) & USAGE != 0 && count.is_finite() {
            local.usage.track(feature, count);
        }
    }

    pub async fn track_error(&self, error: UsageErrorKind) {
        let mut local = self.local.lock().await;
        if local.enforce(&self.privacy) & USAGE != 0 {
            local.usage.error(error);
        }
    }

    pub async fn note_error(&self, context: CrashContext) {
        let mut local = self.local.lock().await;
        if local.enforce(&self.privacy) & CRASH == 0 {
            return;
        }
        if local.context.len() == CRASH_MAX_ERRORS {
            local.context.pop_front();
        }
        local.context.push_back(context);
    }

    pub async fn record_crash(&self, event: NativeCrash, reason: CrashReason, exit_code: i32) {
        let mut local = self.local.lock().await;
        let flags = local.enforce(&self.privacy);
        let (kind, process, usage) = event.wire();
        if flags & USAGE != 0 {
            local.usage.error(usage);
        }
        if flags & CRASH == 0 {
            return;
        }
        let now = Utc::now();
        let at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let errors = local
            .context
            .iter()
            .map(|entry| entry.label().into())
            .collect();
        let mut reports = local.reports();
        if let Some(last) = reports.last_mut()
            && last.kind == kind
            && last.process == process
            && last.reason == reason.label()
            && last.exit_code == f64::from(exit_code)
            && parse_time(&last.last_at).is_some_and(|prior| {
                let age = (now - prior).num_milliseconds();
                (0..60_000).contains(&age)
            })
        {
            last.last_at = at;
            last.count = (last.count + 1.0).min(1000.0);
            last.errors = errors;
        } else {
            reports.push(CrashReport {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                process: process.into(),
                reason: reason.label().into(),
                exit_code: f64::from(exit_code),
                first_at: at.clone(),
                last_at: at,
                count: 1.0,
                system: crash_system(&self.cloud, self.memory_gb),
                errors,
                log: String::new(),
                extra: Default::default(),
            });
        }
        limit_reports(&mut reports);
        local.set_reports(&reports);
        self.crash_ready.notify_one();
    }

    /// Window focus is a flush opportunity, at most once per ten minutes.
    pub async fn wake(&self) {
        let mut local = self.local.lock().await;
        if local
            .last_wake
            .is_some_and(|last| last.elapsed() < Duration::from_secs(600))
        {
            return;
        }
        local.last_wake = Some(Instant::now());
        drop(local);
        self.flush_usage().await;
    }

    pub async fn flush_usage(&self) {
        let Some(_flight) = Flight::begin(&self.usage_flight) else {
            return;
        };
        let consent = self.privacy.revocations.subscribe();
        let (days, revision) = {
            let mut local = self.local.lock().await;
            if local.enforce(&self.privacy) & USAGE == 0 {
                return;
            }
            let days: Vec<_> = local
                .usage
                .pending()
                .into_iter()
                .filter_map(safe_day)
                .collect();
            (days, local.usage_revision)
        };
        if days.is_empty() || !self.cloud.can_upload().await {
            return;
        }
        let Some(system) = self.usage_system() else {
            return;
        };
        {
            let mut local = self.local.lock().await;
            if local.enforce(&self.privacy) & USAGE == 0 || local.usage_revision != revision {
                return;
            }
        }
        let result = self
            .privacy
            .until_revoked(
                0,
                consent,
                self.cloud.report_usage(UsageReport {
                    install_id: self.cloud.device_id().into(),
                    days: days.clone(),
                    system,
                }),
            )
            .await;
        if let Some(Ok(result)) = result {
            let mut local = self.local.lock().await;
            if local.enforce(&self.privacy) & USAGE == 0 || local.usage_revision != revision {
                return;
            }
            let pending = local.usage.pending();
            let accepted: Vec<_> = result
                .accepted
                .into_iter()
                .filter(|id| {
                    days.iter().find(|day| &day.day == id).is_some_and(|sent| {
                        pending
                            .iter()
                            .find(|day| &day.day == id)
                            .cloned()
                            .and_then(safe_day)
                            .as_ref()
                            == Some(sent)
                    })
                })
                .collect();
            local.usage.mark_accepted(&accepted);
        }
    }

    pub async fn flush_crashes(&self) {
        let Some(_flight) = Flight::begin(&self.crash_flight) else {
            return;
        };
        let consent = self.privacy.revocations.subscribe();
        let (reports, revision) = {
            let mut local = self.local.lock().await;
            if local.enforce(&self.privacy) & CRASH == 0 {
                return;
            }
            (local.reports(), local.crash_revision)
        };
        if reports.is_empty() || !self.cloud.can_upload().await {
            return;
        }
        {
            let mut local = self.local.lock().await;
            if local.enforce(&self.privacy) & CRASH == 0 || local.crash_revision != revision {
                return;
            }
        }
        let result = self
            .privacy
            .until_revoked(
                1,
                consent,
                self.cloud.report_crashes(CrashUpload {
                    install_id: self.cloud.device_id().into(),
                    reports: reports.clone(),
                }),
            )
            .await;
        if let Some(Ok(result)) = result {
            let mut local = self.local.lock().await;
            if local.enforce(&self.privacy) & CRASH == 0 || local.crash_revision != revision {
                return;
            }
            let mut pending = local.reports();
            pending.retain(|report| {
                !result.accepted.contains(&report.id) || !reports.contains(report)
            });
            local.set_reports(&pending);
        }
    }

    /// Retain this future while the application runs. Dropping it cancels
    /// timers and HTTP waits. The local writer keeps unsent totals.
    pub async fn run(&self) {
        let Some(_running) = Flight::begin(&self.running) else {
            return;
        };
        tokio::join!(
            async {
                let mut ticks = interval_at(
                    Instant::now() + Duration::from_secs(30),
                    Duration::from_secs(6 * 3600),
                );
                ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
                loop {
                    ticks.tick().await;
                    self.flush_usage().await;
                }
            },
            async {
                let mut ticks = interval_at(
                    Instant::now() + Duration::from_secs(20),
                    Duration::from_secs(3600),
                );
                ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
                let mut due = Instant::now();
                let mut scheduled = false;
                loop {
                    tokio::select! {
                        _ = ticks.tick() => self.flush_crashes().await,
                        _ = self.crash_ready.notified() => {
                            due = Instant::now() + CRASH_DELAY;
                            scheduled = true;
                        }
                        _ = sleep_until(due), if scheduled => {
                            scheduled = false;
                            self.flush_crashes().await;
                        }
                    }
                }
            },
            async {
                let mut usage = self.cloud.topics().stream::<ServiceUsage>();
                while let Some(event) = usage.next().await {
                    let Ok(event) = event else { continue };
                    let (feature, count) = match *event {
                        ServiceUsage::Started => (UsageFeature::ServiceStarted, 1.0),
                        ServiceUsage::Ended => (UsageFeature::ServiceEnded, 1.0),
                        ServiceUsage::ListeningMinutes(minutes) => {
                            (UsageFeature::ListeningMinutes, minutes as f64)
                        }
                        ServiceUsage::RecapUploaded => (UsageFeature::RecapUploaded, 1.0),
                    };
                    self.track(feature, count).await;
                }
            },
            async {
                // Window focus is a flush opportunity.
                let mut power = self.cloud.topics().stream::<kairo_core::PowerEvent>();
                while let Some(event) = power.next().await {
                    if matches!(event.as_deref(), Ok(kairo_core::PowerEvent::Focus)) {
                        self.wake().await;
                    }
                }
            },
            async {
                loop {
                    self.privacy.changed.notified().await;
                    self.local.lock().await.enforce(&self.privacy);
                }
            },
        );
    }

    fn usage_system(&self) -> Option<UsageSystem> {
        let settings = self.settings.lock().ok()?;
        let settings = settings.data();
        let device = self.cloud.device();
        let outputs = settings
            .get("overlay")
            .and_then(|overlay| overlay.get("outputs"))
            .and_then(Value::as_array);
        let count = |kind: &str| {
            outputs
                .into_iter()
                .flatten()
                .filter(|output| {
                    output.get("enabled").and_then(Value::as_bool) == Some(true)
                        && output.get("kind").and_then(Value::as_str) == Some(kind)
                })
                .count() as f64
        };
        let scripture = settings.get("scripture");
        let translation = scripture
            .and_then(|s| s.get("defaultTranslation"))
            .and_then(Value::as_str)
            .filter(|value| {
                value.len() <= 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
            })
            .unwrap_or("");
        let theme = settings
            .get("display")
            .and_then(|s| s.get("theme"))
            .and_then(Value::as_str)
            .filter(|theme| matches!(*theme, "light" | "dark" | "system"))
            .unwrap_or("dark");
        Some(UsageSystem {
            os: device.os.into(),
            os_version: device.os_version.clone(),
            arch: device.arch.into(),
            app_version: device.app_version.into(),
            electron_version: device.electron_version.into(),
            locale: kairo_store::os_locale(),
            cpu_count: std::thread::available_parallelism().map_or(0.0, |count| count.get() as f64),
            memory_gb: self.memory_gb,
            screens: count("screen"),
            ndi_outputs: count("ndi"),
            propresenter: false,
            transcription: settings
                .get("stt")
                .and_then(|s| s.get("provider"))
                .and_then(Value::as_str)
                .is_some_and(|provider| matches!(provider, "deepgram" | "whisper")),
            automation: scripture
                .and_then(|s| s.get("autoMode"))
                .and_then(Value::as_bool)
                == Some(true),
            default_translation: translation.into(),
            theme: theme.into(),
        })
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        if let Ok(mut settings) = self.settings.try_lock() {
            settings.remove_listener(self.listener);
        }
    }
}

fn safe_day(mut day: UsageDayReport) -> Option<UsageDayReport> {
    if !is_usage_day(&day.day) {
        return None;
    }
    day.counts.retain(|key, count| {
        count.is_finite()
            && *count > 0.0
            && UsageFeature::ALL.iter().any(|item| item.as_str() == key)
    });
    day.errors.retain(|key, count| {
        count.is_finite()
            && *count > 0.0
            && UsageErrorKind::ALL.iter().any(|item| item.as_str() == key)
    });
    Some(day)
}

fn crash_system(cloud: &CloudSession, memory_gb: f64) -> CrashSystem {
    let device = cloud.device();
    CrashSystem {
        os: device.os.into(),
        os_version: device.os_version.clone(),
        arch: device.arch.into(),
        app_version: device.app_version.into(),
        electron_version: device.electron_version.into(),
        memory_gb,
        gpu: BTreeMap::new(),
        extra: Default::default(),
    }
}

fn parse_time(at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

fn limit_reports(reports: &mut Vec<CrashReport>) {
    let now = Utc::now();
    reports.retain(|report| {
        [&report.first_at, &report.last_at].into_iter().all(|time| {
            parse_time(time).is_some_and(|at| {
                at >= now - chrono::Days::new(90) && at <= now + chrono::Days::new(1)
            })
        })
    });
    reports.drain(..reports.len().saturating_sub(CRASH_MAX_REPORTS));
}

fn import_panics(local: &mut LocalReports, ledger: CrashLedger, flags: u8, system: CrashSystem) {
    let cursor = local
        .crashes
        .get("nativePanicCursor")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let missing = if cursor <= ledger.count {
        ledger.count - cursor
    } else {
        ledger.count
    };
    let entries = ledger
        .crashes
        .len()
        .min(usize::try_from(missing).unwrap_or(usize::MAX));
    let mut reports = local.reports();
    // Older Electron queues can contain free-form console errors and log tails.
    // Keep counts and timestamps, but never send these strings from native.
    for report in &mut reports {
        let native = matches!(
            report.process.as_str(),
            "engine" | "web-host" | "ui-panic" | "window"
        );
        if !native {
            report.process = match report.kind {
                CrashKind::Renderer => "window",
                CrashKind::ChildProcess => "helper",
            }
            .into();
            report.reason = "abnormal-exit".into();
            report.errors.clear();
        }
        report.errors.retain(|error| {
            matches!(
                error.as_str(),
                "audio error"
                    | "capture error"
                    | "engine error"
                    | "graphics error"
                    | "network error"
                    | "output error"
                    | "web host error"
                    | "window error"
            )
        });
        if !matches!(
            report.reason.as_str(),
            "panic" | "abnormal-exit" | "killed" | "oom" | "launch-failed" | "integrity-failure"
        ) {
            report.reason = "abnormal-exit".into();
        }
        if uuid::Uuid::parse_str(&report.id).is_err() {
            report.id = uuid::Uuid::new_v4().to_string();
        }
        report.log.clear();
        report.extra.clear();
        report.system = system.clone();
    }
    let first = ledger.count.saturating_sub(ledger.crashes.len() as u64);
    for (index, entry) in ledger
        .crashes
        .into_iter()
        .enumerate()
        .rev()
        .take(entries)
        .rev()
    {
        let consent = ledger.consent.get(&(first + index as u64 + 1));
        if flags & USAGE != 0 && consent.is_none_or(|consent| consent.share_stats) {
            local.usage.error(UsageErrorKind::Uncaught);
        }
        if flags & CRASH != 0 && consent.is_none_or(|consent| consent.send_crash_reports) {
            reports.push(CrashReport {
                id: uuid::Uuid::new_v4().to_string(),
                kind: CrashKind::Renderer,
                process: "ui-panic".into(),
                reason: "panic".into(),
                exit_code: 0.0,
                first_at: entry.at.clone(),
                last_at: entry.at,
                count: 1.0,
                system: system.clone(),
                errors: Vec::new(),
                log: String::new(),
                extra: Default::default(),
            });
        }
    }
    limit_reports(&mut reports);
    local.crashes.batch(|store| {
        let _ = store.set_as("reports", &reports);
        store.set("nativePanicCursor", json!(ledger.count));
    });
}

#[cfg(test)]
mod native_crash_tests {
    use super::*;

    /// A contained shell panic is reported as Electron's
    /// `render-process-gone` was: a renderer crash of the main window that
    /// also counts `renderer_gone`.
    #[test]
    fn a_contained_window_panic_reads_as_a_renderer_crash() {
        let (kind, process, usage) = NativeCrash::Window.wire();
        assert_eq!(kind, CrashKind::Renderer);
        assert_eq!(process, "window");
        assert_eq!(usage, UsageErrorKind::RendererGone);
        assert_eq!(CrashReason::Panic.label(), "panic");
    }
}
