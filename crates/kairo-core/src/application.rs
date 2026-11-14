//! Application lifecycle ordering formerly owned by Electron's main entry.
//!
//! Concrete service adapters live in the composition root to avoid dependency
//! cycles through cloud and the orchestrator. Local steps must only await disk;
//! background hooks enqueue work and return without waiting for a network call.

use std::{future::Future, pin::Pin, sync::Arc};

pub type LocalResult = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalStep {
    Settings,
    Workspace,
    DefaultMediaFolder,
    OpenLyrics,
    OpenScripture,
    MigrateNkjvDefault,
    DefaultTranslation,
    OfflineBibles,
    LivePlan,
    OnboardingMigration,
    Reporting,
    Updater,
    Window,
    SurfaceInit,
    Reconcile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundTask {
    License,
    Session,
    ScanMedia,
    ScanTracks,
    Ndi,
    ReconnectSession,
    PullOrgSecretsThrottled,
    WakeSermonUploader,
    WakeUsage,
}

/// Every hook is required; absent services cannot silently pass startup.
pub trait LifecycleServices: Send + Sync {
    fn local_step(&self, step: LocalStep) -> LocalResult;
    fn start_background(&self, task: BackgroundTask);
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("Application startup failed at {step:?}: {message}")]
pub struct StartupFailure {
    pub step: LocalStep,
    pub message: String,
}

pub struct ApplicationLifecycle {
    services: Arc<dyn LifecycleServices>,
}

impl ApplicationLifecycle {
    pub fn new(services: Arc<dyn LifecycleServices>) -> Self {
        Self { services }
    }

    async fn local(&self, step: LocalStep) -> Result<(), StartupFailure> {
        self.services
            .local_step(step)
            .await
            .map_err(|message| StartupFailure { step, message })
    }

    /// `app.whenReady`, including the work performed by `registerIpcHandlers`.
    /// The first failing local step stops startup, as the source's async ready
    /// callback does. Nonfatal migrations and offline setup catch their own
    /// failures inside their service adapters, preserving their source policy.
    pub async fn start(&self) -> Result<(), StartupFailure> {
        use LocalStep::*;
        for step in [
            Settings,
            Workspace,
            DefaultMediaFolder,
            OpenLyrics,
            OpenScripture,
            MigrateNkjvDefault,
            DefaultTranslation,
            OfflineBibles,
            LivePlan,
            OnboardingMigration,
            Reporting,
        ] {
            self.local(step).await?;
        }
        for task in [
            BackgroundTask::License,
            BackgroundTask::Session,
            BackgroundTask::ScanMedia,
            BackgroundTask::ScanTracks,
            BackgroundTask::Ndi,
        ] {
            self.services.start_background(task);
        }
        for step in [Updater, Window, SurfaceInit, Reconcile] {
            self.local(step).await?;
        }
        Ok(())
    }

    pub fn resume(&self) {
        self.services
            .start_background(BackgroundTask::ReconnectSession);
    }

    pub fn focus(&self) {
        for task in [
            BackgroundTask::PullOrgSecretsThrottled,
            BackgroundTask::WakeSermonUploader,
            BackgroundTask::WakeUsage,
        ] {
            self.services.start_background(task);
        }
    }
}
