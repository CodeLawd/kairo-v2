//! Pack installation progress, from `local-bible-installer.ts`.
//!
//! Progress lives for the session, so a view that reopens sees an install in
//! flight. Duplicate requests share one download. Any failure becomes
//! operator-facing recovery text and can be retried.

use crate::api::BoxFuture;
use crate::pack::{DownloadError, TransferProgress};
use futures_util::FutureExt;
use futures_util::future::Shared;
use kairo_domain::scripture::{
    LocalBibleInstallPhase, LocalBibleInstallProgress, LocalBiblePackInstallResult,
};
use regex::Regex;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

/// Repeated `downloading` updates closer together than this are not announced.
const NOTIFY_INTERVAL: Duration = Duration::from_millis(100);

/// What went wrong before recovery text replaced it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InstallFailure {
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error("{0}")]
    Other(String),
}

/// A failed install: recovery text for the operator, and the cause for logs.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct InstallError {
    pub message: String,
    pub cause: InstallFailure,
}

pub type InstallFuture =
    Shared<BoxFuture<'static, Result<LocalBiblePackInstallResult, InstallError>>>;

/// The download and install steps, injectable for tests.
pub trait InstallerBackend: Send + Sync + 'static {
    fn download<'a>(
        &'a self,
        translation: &'a str,
        update: &'a mut (dyn FnMut(TransferProgress) + Send),
    ) -> BoxFuture<'a, Result<Vec<u8>, InstallFailure>>;

    fn install<'a>(
        &'a self,
        translation: &'a str,
        sqlite: Vec<u8>,
    ) -> BoxFuture<'a, Result<LocalBiblePackInstallResult, InstallFailure>>;
}

static TIMEOUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?i)timeout|timed out|aborted").unwrap());
static NETWORK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?i)fetch|network|internet|ENOTFOUND|ECONN").unwrap());
static VERIFY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?i)security check|invalid size|decompress").unwrap());

const TIMEOUT_TEXT: &str =
    "The download took too long. Check your internet connection and try again.";
const NETWORK_TEXT: &str =
    "Could not download this Bible. Check your internet connection and try again.";

/// `recoveryMessage`. Electron reads the cause's message; a transport
/// failure is matched by kind here, since `fetch` reports only "fetch failed".
pub fn recovery_message(failure: &InstallFailure) -> &'static str {
    match failure {
        InstallFailure::Download(DownloadError::Timeout(_)) => return TIMEOUT_TEXT,
        InstallFailure::Download(DownloadError::Network(_)) => return NETWORK_TEXT,
        _ => {}
    }
    let message = failure.to_string();
    if TIMEOUT.is_match(&message) {
        TIMEOUT_TEXT
    } else if NETWORK.is_match(&message) {
        NETWORK_TEXT
    } else if VERIFY.is_match(&message) {
        "The download could not be verified. Please try downloading it again."
    } else if message.contains("HTTP") {
        "The download is unavailable right now. Please try again shortly."
    } else {
        "This Bible could not be installed. Please try again. If it continues, restart Kairo."
    }
}

#[derive(Default)]
struct State {
    progress: HashMap<String, LocalBibleInstallProgress>,
    pending: HashMap<String, InstallFuture>,
    listeners: kairo_core::Listeners<LocalBibleInstallProgress>,
    revision: f64,
    last_notification: HashMap<String, Instant>,
}

enum Change {
    Start,
    Transfer(TransferProgress),
    Phase(LocalBibleInstallPhase),
    Failed(String),
}

/// Ends a [`LocalBibleInstaller::subscribe`] subscription.
pub struct Unsubscribe {
    listeners: kairo_core::Listeners<LocalBibleInstallProgress>,
    id: kairo_core::ListenerId,
}

impl Unsubscribe {
    pub fn unsubscribe(self) {
        self.listeners.remove(self.id);
    }
}

use kairo_domain::js::lock;

#[derive(Clone)]
pub struct LocalBibleInstaller {
    backend: Arc<dyn InstallerBackend>,
    state: Arc<Mutex<State>>,
}

impl LocalBibleInstaller {
    pub fn new(backend: Arc<dyn InstallerBackend>) -> Self {
        Self {
            backend,
            state: Arc::default(),
        }
    }

    pub fn get_progress(&self, translation: &str) -> Option<LocalBibleInstallProgress> {
        lock(&self.state)
            .progress
            .get(&translation.to_uppercase())
            .cloned()
    }

    pub fn clear(&self, translation: &str) {
        lock(&self.state)
            .progress
            .remove(&translation.to_uppercase());
    }

    pub fn subscribe(
        &self,
        listener: impl Fn(&LocalBibleInstallProgress) + Send + Sync + 'static,
    ) -> Unsubscribe {
        let listeners = lock(&self.state).listeners.clone();
        Unsubscribe {
            id: listeners.add(listener),
            listeners,
        }
    }

    fn update(state: &Mutex<State>, translation: &str, change: Change) {
        let (progress, listeners) = {
            let mut state = lock(state);
            let previous = state.progress.get(translation).cloned();
            let mut progress = previous.clone().unwrap_or(LocalBibleInstallProgress {
                translation: translation.to_owned(),
                phase: LocalBibleInstallPhase::Downloading,
                received_bytes: 0.0,
                total_bytes: None,
                revision: 0.0,
                error: None,
            });
            match change {
                Change::Start => {
                    progress.phase = LocalBibleInstallPhase::Downloading;
                    progress.received_bytes = 0.0;
                    progress.total_bytes = None;
                    progress.error = None;
                }
                Change::Transfer(transfer) => {
                    progress.phase = transfer.phase;
                    progress.received_bytes = transfer.received_bytes as f64;
                    progress.total_bytes = transfer.total_bytes.map(|n| n as f64);
                }
                Change::Phase(phase) => progress.phase = phase,
                Change::Failed(message) => {
                    progress.phase = LocalBibleInstallPhase::Failed;
                    progress.error = Some(message);
                }
            }
            state.revision += 1.0;
            progress.translation = translation.to_owned();
            progress.revision = state.revision;
            state
                .progress
                .insert(translation.to_owned(), progress.clone());
            let now = Instant::now();
            let downloading = LocalBibleInstallPhase::Downloading;
            if progress.phase == downloading
                && previous.is_some_and(|p| p.phase == downloading)
                && state
                    .last_notification
                    .get(translation)
                    .is_some_and(|last| now.duration_since(*last) < NOTIFY_INTERVAL)
            {
                return;
            }
            state.last_notification.insert(translation.to_owned(), now);
            (progress, state.listeners.clone())
        };
        // Outside the lock, so a listener may read progress.
        listeners.notify(&progress);
    }

    /// Download and install `translation`. A second request while one runs
    /// returns the same shared future.
    pub fn start(&self, translation: &str) -> InstallFuture {
        let id = translation.to_uppercase();
        let mut state = lock(&self.state);
        if let Some(existing) = state.pending.get(&id) {
            return existing.clone();
        }
        let backend = self.backend.clone();
        let shared = self.state.clone();
        let task_id = id.clone();
        let task: BoxFuture<'static, _> = Box::pin(async move {
            let id = task_id;
            Self::update(&shared, &id, Change::Start);
            let result = async {
                let mut on_transfer = |p| Self::update(&shared, &id, Change::Transfer(p));
                let bytes = backend.download(&id, &mut on_transfer).await?;
                Self::update(
                    &shared,
                    &id,
                    Change::Phase(LocalBibleInstallPhase::Installing),
                );
                YieldNow(false).await;
                backend.install(&id, bytes).await
            }
            .await;
            let outcome = match result {
                Ok(result) => {
                    Self::update(&shared, &id, Change::Phase(LocalBibleInstallPhase::Ready));
                    Ok(result)
                }
                Err(cause) => {
                    let message = recovery_message(&cause).to_owned();
                    tracing::warn!(translation = %id, %cause, "[Scripture] Local Bible install failed");
                    Self::update(&shared, &id, Change::Failed(message.clone()));
                    Err(InstallError { message, cause })
                }
            };
            lock(&shared).pending.remove(&id);
            outcome
        });
        let task = task.shared();
        state.pending.insert(id, task.clone());
        task
    }
}

/// Yields to the executor once, as `setImmediate` lets progress paint.
struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_text_follows_the_failure() {
        let text = |failure: InstallFailure| recovery_message(&failure);
        let download = |error| text(InstallFailure::Download(error));
        assert!(download(DownloadError::Timeout("x".into())).contains("took too long"));
        assert!(download(DownloadError::Network("x".into())).contains("internet connection"));
        assert!(download(DownloadError::SecurityCheck("NKJV".into())).contains("verified"));
        assert!(download(DownloadError::InvalidSize("NKJV".into())).contains("verified"));
        assert!(download(DownloadError::Decompress("NKJV".into())).contains("verified"));
        assert!(download(DownloadError::Http("NKJV".into(), 503)).contains("unavailable"));
        assert!(download(DownloadError::NotDownloadable("KJV".into())).contains("restart Kairo"));
        assert!(text(InstallFailure::Other("Request aborted".into())).contains("took too long"));
        assert!(text(InstallFailure::Other("getaddrinfo ENOTFOUND".into())).contains("internet"));
        assert!(text(InstallFailure::Other("Missing verse 1:1:1.".into())).contains("restart"));
        // `HTTP` is matched case-sensitively, as in Electron.
        assert!(text(InstallFailure::Other("http".into())).contains("restart"));
    }
}
