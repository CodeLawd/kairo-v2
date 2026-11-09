//! Health transitions, connectivity polling and Electron-compatible recovery.

use crate::{AppCore, CoreError, Handle, Message, Publisher};
use kairo_domain::{
    scripture::small::{
        DetectorRecoveryEvent, DetectorRecoveryState, transition_detector_recovery,
    },
    status::{
        DetectorFallbackKind, DetectorFallbackReason, HealthStatus, OverallHealth,
        ResilienceStatus, ServiceHealth, ServiceName,
    },
};
use serde_json::Value;
use std::{
    future::Future,
    io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::{Instant, MissedTickBehavior, interval_at, timeout};

pub const CONNECTIVITY_INTERVAL: Duration = Duration::from_secs(10);
pub const RECOVERY_INTERVAL: Duration = Duration::from_secs(30);
pub const DETECTOR_PROBE_DELAY: Duration = Duration::from_secs(30);
const DNS_TIMEOUT: Duration = Duration::from_secs(3);

/// Mutable state owned by the service that publishes `ResilienceStatus`.
/// Times are Unix milliseconds, injected so transitions can be replayed.
pub struct Resilience {
    internet_connected: bool,
    detector: DetectorRecoveryState,
    reason: Option<DetectorFallbackReason>,
    probe_generation: u64,
    pending_probe: Option<ProbeTicket>,
    recovery_session_available: bool,
    health: Vec<ServiceHealth>,
}

impl Resilience {
    pub fn new(now_ms: f64, recovery_session_available: bool) -> Self {
        Self {
            internet_connected: true,
            detector: DetectorRecoveryState {
                consecutive_errors: 0,
                fallback_active: false,
                probing: false,
            },
            reason: None,
            probe_generation: 0,
            pending_probe: None,
            recovery_session_available,
            health: [ServiceName::Audio, ServiceName::Stt, ServiceName::Detector]
                .into_iter()
                .map(|service| ServiceHealth {
                    service,
                    status: HealthStatus::Ok,
                    last_error: None,
                    last_updated: now_ms,
                })
                .collect(),
        }
    }

    pub fn status(&self) -> ResilienceStatus {
        let critical = self.health.iter().any(|health| {
            health.service == ServiceName::Audio && health.status == HealthStatus::Error
        });
        let degraded = !self.internet_connected
            || self.health.iter().any(|health| {
                health.status == HealthStatus::Degraded
                    || (matches!(health.service, ServiceName::Stt | ServiceName::Detector)
                        && health.status == HealthStatus::Error)
            });
        ResilienceStatus {
            overall_health: if critical {
                OverallHealth::Critical
            } else if degraded {
                OverallHealth::Degraded
            } else {
                OverallHealth::AllGood
            },
            internet_connected: self.internet_connected,
            claude_fallback_active: self.detector.fallback_active,
            detector_fallback_reason: self
                .reason
                .clone()
                .filter(|_| self.detector.fallback_active),
            recovery_session_available: self.recovery_session_available,
            health: self.health.clone(),
        }
    }

    pub fn update_health(
        &mut self,
        service: ServiceName,
        status: HealthStatus,
        last_error: Option<String>,
        now_ms: f64,
    ) {
        let current = self
            .health
            .iter_mut()
            .find(|health| health.service == service);
        let health = ServiceHealth {
            service,
            status,
            last_error: last_error.filter(|message| !message.is_empty()),
            last_updated: now_ms,
        };
        if let Some(current) = current {
            *current = health;
        } else {
            self.health.push(health);
        }
    }

    pub fn update_stt(&mut self, event: SttEvent, now_ms: f64) {
        let (status, message) = match event {
            SttEvent::Connected => (HealthStatus::Ok, None),
            SttEvent::Reconnecting { attempt, delay_ms } => (
                HealthStatus::Degraded,
                Some(format!(
                    "Deepgram reconnecting (Attempt {attempt}, delay {delay_ms}ms)"
                )),
            ),
            SttEvent::Disconnected => {
                (HealthStatus::Degraded, Some("Deepgram disconnected".into()))
            }
            SttEvent::Error(error) => (
                HealthStatus::Error,
                Some(format!("Deepgram error: {error}")),
            ),
        };
        self.update_health(ServiceName::Stt, status, message, now_ms);
    }

    /// Returns whether the connectivity changed.
    pub fn set_internet_connected(&mut self, connected: bool, now_ms: f64) -> bool {
        if self.internet_connected == connected {
            return false;
        }
        self.internet_connected = connected;
        self.set_fallback(
            (!connected).then(|| DetectorFallbackReason {
                kind: DetectorFallbackKind::Offline,
                message: "No internet — scripture detection is using local matching.".into(),
            }),
            now_ms,
        );
        true
    }

    /// A ticket schedules one probe after `DETECTOR_PROBE_DELAY`.
    pub fn detector_error(&mut self, message: &str, now_ms: f64) -> Option<ProbeTicket> {
        if !self.internet_connected {
            return None;
        }
        let was_fallback = self.detector.fallback_active;
        self.detector =
            transition_detector_recovery(self.detector, DetectorRecoveryEvent::RequestFailed);
        if !was_fallback && self.detector.fallback_active {
            self.set_fallback(
                Some(DetectorFallbackReason {
                    kind: DetectorFallbackKind::Provider,
                    message: format!("AI detection refused the request: {message}"),
                }),
                now_ms,
            );
            self.probe_generation = self.probe_generation.wrapping_add(1);
            let ticket = ProbeTicket(self.probe_generation);
            self.pending_probe = Some(ticket);
            return Some(ticket);
        }
        self.update_health(
            ServiceName::Detector,
            HealthStatus::Degraded,
            Some(format!("Claude failure: {message}")),
            now_ms,
        );
        None
    }

    /// Returns whether an outstanding provider retry is still appropriate.
    /// A timer that fires after connectivity changed or a successful request
    /// must not take the detector out of offline fallback.
    pub fn begin_detector_probe(&mut self, ticket: ProbeTicket, now_ms: f64) -> bool {
        if self.pending_probe != Some(ticket) {
            return false;
        }
        self.pending_probe = None;
        self.detector =
            transition_detector_recovery(self.detector, DetectorRecoveryEvent::BeginProbe);
        self.update_health(
            ServiceName::Detector,
            HealthStatus::Degraded,
            Some("Testing AI detector connectivity".into()),
            now_ms,
        );
        true
    }

    pub fn detector_success(&mut self, now_ms: f64) {
        self.detector =
            transition_detector_recovery(self.detector, DetectorRecoveryEvent::RequestSucceeded);
        if self.internet_connected {
            self.set_fallback(None, now_ms);
        } else {
            // An in-flight provider request can complete after going offline.
            self.detector.fallback_active = true;
        }
    }

    pub fn set_recovery_session_available(&mut self, available: bool) {
        self.recovery_session_available = available;
    }

    fn set_fallback(&mut self, reason: Option<DetectorFallbackReason>, now_ms: f64) {
        self.pending_probe = None;
        self.detector.fallback_active = reason.is_some();
        self.detector.probing = false;
        let (status, message) = match &reason {
            Some(reason) => (HealthStatus::Degraded, Some(reason.message.clone())),
            None => (HealthStatus::Ok, None),
        };
        self.reason = reason;
        self.update_health(ServiceName::Detector, status, message, now_ms);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeTicket(u64);

pub enum SttEvent {
    Connected,
    Reconnecting { attempt: u32, delay_ms: u64 },
    Disconnected,
    Error(String),
}

/// The runtime actor owns health state and the recovery file's UI status.
/// The orchestrator remains responsible for validating and applying snapshots.
pub struct ResilienceActor {
    state: Resilience,
    recovery: RecoveryFile,
    /// The snapshot last written, so an unchanged one is not written again.
    last_saved: Option<Value>,
    topics: Publisher,
    monitoring: Arc<AtomicBool>,
}

impl ResilienceActor {
    pub async fn register(
        core: &AppCore,
        user_data: &Path,
        capacity: NonZeroUsize,
    ) -> Result<(), CoreError> {
        let recovery = RecoveryFile::new(user_data);
        let check = recovery.clone();
        let available = match core.run(async move { check.exists().await }).await? {
            Ok(available) => available,
            Err(error) => {
                tracing::warn!(%error, "Could not check session recovery");
                false
            }
        };
        core.register(
            Self {
                state: Resilience::new(kairo_store::settings::now_ms(), available),
                recovery,
                last_saved: None,
                topics: core.publisher(),
                monitoring: Arc::default(),
            },
            capacity,
        )?;
        core.call(PublishInitialStatus).await
    }

    fn publish(&self) {
        self.topics.publish(self.state.status());
    }
}

struct PublishInitialStatus;
impl Message for PublishInitialStatus {
    type Actor = ResilienceActor;
}
impl Handle<PublishInitialStatus> for ResilienceActor {
    type Reply = ();

    async fn handle(&mut self, _: PublishInitialStatus) {
        self.publish();
    }
}

pub struct GetStatus;
impl Message for GetStatus {
    type Actor = ResilienceActor;
}
impl Handle<GetStatus> for ResilienceActor {
    type Reply = ResilienceStatus;

    async fn handle(&mut self, _: GetStatus) -> Self::Reply {
        self.state.status()
    }
}

pub struct HealthChanged {
    pub service: ServiceName,
    pub status: HealthStatus,
    pub error: Option<String>,
}
impl Message for HealthChanged {
    type Actor = ResilienceActor;
}
impl Handle<HealthChanged> for ResilienceActor {
    type Reply = ();

    async fn handle(&mut self, message: HealthChanged) {
        self.state.update_health(
            message.service,
            message.status,
            message.error,
            kairo_store::settings::now_ms(),
        );
        self.publish();
    }
}

impl Message for SttEvent {
    type Actor = ResilienceActor;
}
impl Handle<SttEvent> for ResilienceActor {
    type Reply = ();

    async fn handle(&mut self, message: SttEvent) {
        self.state
            .update_stt(message, kairo_store::settings::now_ms());
        self.publish();
    }
}

pub struct InternetConnectivity(pub bool);
impl Message for InternetConnectivity {
    type Actor = ResilienceActor;
}
impl Handle<InternetConnectivity> for ResilienceActor {
    type Reply = ();

    async fn handle(&mut self, message: InternetConnectivity) {
        // Checked every few seconds; subscribers hear only real changes.
        if self
            .state
            .set_internet_connected(message.0, kairo_store::settings::now_ms())
        {
            self.publish();
        }
    }
}

pub struct DetectorFailed(pub String);
impl Message for DetectorFailed {
    type Actor = ResilienceActor;
}
impl Handle<DetectorFailed> for ResilienceActor {
    /// Use the returned ticket after the retry delay.
    type Reply = Option<ProbeTicket>;

    async fn handle(&mut self, message: DetectorFailed) -> Self::Reply {
        let schedule = self
            .state
            .detector_error(&message.0, kairo_store::settings::now_ms());
        self.publish();
        schedule
    }
}

pub enum DetectorStateChange {
    Succeeded,
    BeginProbe(ProbeTicket),
}
impl Message for DetectorStateChange {
    type Actor = ResilienceActor;
}
impl Handle<DetectorStateChange> for ResilienceActor {
    type Reply = ();

    async fn handle(&mut self, message: DetectorStateChange) {
        let now = kairo_store::settings::now_ms();
        match message {
            DetectorStateChange::Succeeded => self.state.detector_success(now),
            DetectorStateChange::BeginProbe(ticket) => {
                self.state.begin_detector_probe(ticket, now);
            }
        }
        self.publish();
    }
}

/// Read the recovery snapshot for the current service. The caller validates
/// the snapshot and applies it to the orchestrator when one is returned.
pub struct LoadRecovery {
    pub active_service: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Create or resume a service before restoring transcription.")]
pub struct NoActiveService;

impl Message for LoadRecovery {
    type Actor = ResilienceActor;
}
impl Handle<LoadRecovery> for ResilienceActor {
    type Reply = Result<Option<Value>, NoActiveService>;

    async fn handle(&mut self, message: LoadRecovery) -> Self::Reply {
        if !message.active_service {
            return Err(NoActiveService);
        }
        Ok(match self.recovery.load().await {
            Ok(value) => value.filter(|value| kairo_domain::js::truthy(Some(value))),
            Err(error) => {
                tracing::warn!(%error, "Could not load session recovery");
                None
            }
        })
    }
}

pub struct DiscardRecovery;
impl Message for DiscardRecovery {
    type Actor = ResilienceActor;
}
impl Handle<DiscardRecovery> for ResilienceActor {
    type Reply = ();

    async fn handle(&mut self, _: DiscardRecovery) {
        match self.recovery.discard().await {
            Ok(_) => {
                self.last_saved = None;
                self.state.set_recovery_session_available(false);
                self.publish();
            }
            Err(error) => tracing::warn!(%error, "Could not discard session recovery"),
        }
    }
}

struct MonitoringPermit(Arc<AtomicBool>);
impl Drop for MonitoringPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct BeginMonitoring;
impl Message for BeginMonitoring {
    type Actor = ResilienceActor;
}
impl Handle<BeginMonitoring> for ResilienceActor {
    type Reply = Option<MonitoringPermit>;

    async fn handle(&mut self, _: BeginMonitoring) -> Self::Reply {
        self.monitoring
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| MonitoringPermit(self.monitoring.clone()))
    }
}

struct SaveRecovery(Value);
impl Message for SaveRecovery {
    type Actor = ResilienceActor;
}
impl Handle<SaveRecovery> for ResilienceActor {
    type Reply = ();

    async fn handle(&mut self, message: SaveRecovery) {
        // Saved on a timer; an unchanged snapshot is already on disk.
        if self.last_saved.as_ref() == Some(&message.0) {
            return;
        }
        match self.recovery.save(message.0.clone()).await {
            Ok(()) => self.last_saved = Some(message.0),
            Err(error) => tracing::warn!(%error, "Could not save session recovery"),
        }
    }
}

/// The crash hook and periodic snapshot writer share the legacy file format.
/// The consuming orchestrator validates the fields before restoring them.
#[derive(Clone)]
pub struct RecoveryFile {
    path: PathBuf,
}

impl RecoveryFile {
    pub fn new(user_data: &Path) -> Self {
        Self {
            path: user_data.join(kairo_store::paths::RECOVERY_FILE_NAME),
        }
    }

    pub async fn exists(&self) -> io::Result<bool> {
        let path = self.path.clone();
        blocking(move || path.try_exists()).await
    }

    pub async fn load(&self) -> io::Result<Option<Value>> {
        let path = self.path.clone();
        blocking(move || match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        })
        .await
    }

    pub async fn save(&self, snapshot: Value) -> io::Result<()> {
        let path = self.path.clone();
        blocking(move || kairo_store::crash::save_recovery_state(&path, &snapshot)).await
    }

    pub async fn discard(&self) -> io::Result<bool> {
        let path = self.path.clone();
        blocking(move || match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        })
        .await
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(io::Error::other)?
}

/// A timeout on the first DNS lookup means offline. A completed lookup
/// failure tries the second hostname before reporting offline.
pub async fn check_internet_connection() -> bool {
    match timeout(
        DNS_TIMEOUT,
        tokio::net::lookup_host(("api.deepgram.com", 443)),
    )
    .await
    {
        Ok(Ok(mut addresses)) => addresses.next().is_some(),
        Err(_) => false,
        Ok(_) => timeout(DNS_TIMEOUT, tokio::net::lookup_host(("google.com", 443)))
            .await
            .is_ok_and(|result| result.is_ok_and(|mut addresses| addresses.next().is_some())),
    }
}

/// Retain the future while monitoring is active. Dropping it stops further
/// polls and snapshots. Snapshot writes share the actor mailbox with restore
/// and discard, and a slow callback never queues another invocation of itself.
pub async fn monitor<Snapshot, SnapshotFuture>(
    core: &AppCore,
    mut snapshot: Snapshot,
) -> Result<(), CoreError>
where
    Snapshot: FnMut() -> SnapshotFuture + Send + 'static,
    SnapshotFuture: Future<Output = Result<Option<Value>, String>> + Send,
{
    let Some(permit) = core.call(BeginMonitoring).await? else {
        return Ok(());
    };
    let running_core = core.clone();
    core.run(async move {
        let _permit = permit;
        let start = Instant::now();
        let connectivity = async {
            let mut ticks = interval_at(start, CONNECTIVITY_INTERVAL);
            ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                let event = InternetConnectivity(check_internet_connection().await);
                if let Err(error) = running_core.call(event).await {
                    break error;
                }
            }
        };
        let persistence = async {
            let mut ticks = interval_at(start + RECOVERY_INTERVAL, RECOVERY_INTERVAL);
            ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                match snapshot().await {
                    Ok(Some(state)) => {
                        if let Err(error) = running_core.call(SaveRecovery(state)).await {
                            break error;
                        }
                    }
                    Ok(None) => {}
                    Err(message) => {
                        tracing::warn!(%message, "Could not get session recovery state")
                    }
                }
            }
        };
        tokio::select! {
            error = connectivity => Err(error),
            error = persistence => Err(error),
        }
    })
    .await?
}

/// The orchestrator retains this future for the ticket returned by a failed
/// detector request. Stale tickets are harmless after recovery or going offline.
pub async fn probe_after_delay(core: &AppCore, ticket: ProbeTicket) -> Result<(), CoreError> {
    let running_core = core.clone();
    core.run(async move {
        tokio::time::sleep(DETECTOR_PROBE_DELAY).await;
        running_core
            .call(DetectorStateChange::BeginProbe(ticket))
            .await
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_retries_after_two_failures_and_stays_degraded_until_success() {
        let mut resilience = Resilience::new(0.0, false);
        assert_eq!(resilience.detector_error("first", 1.0), None);
        assert_eq!(resilience.status().overall_health, OverallHealth::Degraded);
        assert!(!resilience.status().claude_fallback_active);
        let ticket = resilience.detector_error("second", 2.0).unwrap();
        assert_eq!(
            resilience
                .status()
                .detector_fallback_reason
                .unwrap()
                .message,
            "AI detection refused the request: second"
        );
        assert!(resilience.begin_detector_probe(ticket, 30_002.0));
        assert!(!resilience.begin_detector_probe(ticket, 30_003.0));
        assert!(!resilience.status().claude_fallback_active);
        assert_eq!(resilience.status().overall_health, OverallHealth::Degraded);
        resilience.detector_success(30_003.0);
        assert_eq!(resilience.status().overall_health, OverallHealth::AllGood);
        assert_eq!(resilience.detector_error("new failure", 30_004.0), None);
        let next = resilience
            .detector_error("another failure", 30_005.0)
            .unwrap();
        assert!(!resilience.begin_detector_probe(ticket, 60_004.0));
        assert!(resilience.begin_detector_probe(next, 60_005.0));
    }

    #[test]
    fn offline_degrades_but_audio_error_is_critical() {
        let mut resilience = Resilience::new(0.0, true);
        assert!(resilience.set_internet_connected(false, 1.0));
        assert!(!resilience.set_internet_connected(false, 2.0));
        let status = resilience.status();
        assert_eq!(status.overall_health, OverallHealth::Degraded);
        assert_eq!(
            status.detector_fallback_reason.unwrap().kind,
            DetectorFallbackKind::Offline
        );
        resilience.update_health(
            ServiceName::Audio,
            HealthStatus::Error,
            Some("mic gone".into()),
            3.0,
        );
        assert_eq!(resilience.status().overall_health, OverallHealth::Critical);
        resilience.set_internet_connected(true, 4.0);
        assert!(!resilience.status().claude_fallback_active);
        assert!(resilience.status().recovery_session_available);
        resilience.update_health(ServiceName::Audio, HealthStatus::Ok, None, 5.0);
        assert_eq!(resilience.status().overall_health, OverallHealth::AllGood);
    }

    #[tokio::test]
    async fn registered_actor_publishes_its_initial_state_and_updates() {
        let core = AppCore::new().unwrap();
        let profile = tempfile::tempdir().unwrap();
        std::fs::write(profile.path().join("session-recovery.json"), "{}").unwrap();
        let mut status = core.watch::<ResilienceStatus>();
        ResilienceActor::register(&core, profile.path(), NonZeroUsize::new(4).unwrap())
            .await
            .unwrap();
        assert!(status.next().await.unwrap().recovery_session_available);

        let monitoring = core.call(BeginMonitoring).await.unwrap().unwrap();
        assert!(core.call(BeginMonitoring).await.unwrap().is_none());
        drop(monitoring);
        assert!(core.call(BeginMonitoring).await.unwrap().is_some());

        core.send(InternetConnectivity(false)).unwrap();
        assert!(!status.next().await.unwrap().internet_connected);
        core.call(DiscardRecovery).await.unwrap();
        assert!(!status.next().await.unwrap().recovery_session_available);
        assert!(
            core.call(LoadRecovery {
                active_service: true
            })
            .await
            .unwrap()
            .unwrap()
            .is_none()
        );
        assert_eq!(
            core.call(LoadRecovery {
                active_service: false
            })
            .await
            .unwrap(),
            Err(NoActiveService)
        );
        assert_eq!(
            core.call(GetStatus).await.unwrap().overall_health,
            OverallHealth::Degraded
        );
    }

    #[tokio::test]
    async fn recovery_round_trips_legacy_json_and_handles_missing_or_corrupt_files() {
        let profile = tempfile::tempdir().unwrap();
        let recovery = RecoveryFile::new(profile.path());
        assert!(!recovery.exists().await.unwrap());
        assert!(recovery.load().await.unwrap().is_none());
        let value = serde_json::json!({"config": {}, "session": {"sessionId": "test"}});
        recovery.save(value.clone()).await.unwrap();
        assert!(recovery.exists().await.unwrap());
        assert_eq!(recovery.load().await.unwrap(), Some(value));
        std::fs::write(profile.path().join("session-recovery.json"), "invalid").unwrap();
        assert!(recovery.load().await.is_err());
        assert!(recovery.discard().await.unwrap());
        assert!(!recovery.discard().await.unwrap());
    }
}
