//! The Settings window's backend over the services: `settings:*`, the audio
//! device lists and input meter, `output:sendTest`/`clearAll`, updates,
//! `workspace:*`, `documents:converter:*`, `scripture:*` packs and traces,
//! and `onboarding:reset`.

use std::sync::{Arc, Mutex};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use kairo_cloud::onboarding::OnboardingCommand;
use kairo_domain::documents::SlideConverterStatus;
use kairo_domain::scripture::{
    InstalledLocalBiblePack, LocalBiblePackStatus, ScriptureTranslationOption,
};
use kairo_domain::scripture_trace::ScriptureTraceRecord;
use kairo_domain::settings::AppSettings;
use kairo_domain::status::UpdateStatus;
use kairo_domain::workspace::{MediaFolderMigration, WorkspaceInfo, WorkspaceSongSync};
use kairo_stt::capture::CaptureEvent;
use serde_json::Value;
use ui_settings::{
    BackendEvent, InputDevice, MeterEvent, MeterHandle, Reply, SettingsBackend, UpdateAction,
};

use crate::handlers::Handlers;
use crate::services::ApplicationServices;

type Senders = Arc<Mutex<Vec<UnboundedSender<BackendEvent>>>>;

/// The backend. One per app: it subscribes to the services once and fans
/// pushes out to every open window.
pub struct ServicesSettings {
    services: Arc<ApplicationServices>,
    senders: Senders,
    _progress: kairo_scripture::installer::Unsubscribe,
}

fn broadcast(senders: &Senders, event: BackendEvent) {
    senders
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|tx| tx.unbounded_send(event.clone()).is_ok());
}

/// Runs `work` on the service runtime and answers on any executor; an
/// interrupted call answers the empty value.
fn on_runtime<T, F>(services: &Arc<ApplicationServices>, work: F) -> Reply<T>
where
    T: Default + Send + 'static,
    F: std::future::Future<Output = T> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    services.spawn(async move {
        let _ = tx.send(work.await);
    });
    Box::pin(async move { rx.await.unwrap_or_default() })
}

/// As [`on_runtime`] for fallible work: an interrupted call is an error.
fn try_on_runtime<T, F>(services: &Arc<ApplicationServices>, work: F) -> Reply<Result<T, String>>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    services.spawn(async move {
        let _ = tx.send(work.await);
    });
    Box::pin(async move {
        rx.await
            .unwrap_or_else(|_| Err("The request was interrupted.".into()))
    })
}

impl ServicesSettings {
    pub fn new(services: Arc<ApplicationServices>) -> Self {
        let senders: Senders = Arc::default();
        {
            let senders = senders.clone();
            services
                .updater
                .on_change(move |status| broadcast(&senders, BackendEvent::Update(status.clone())));
        }
        {
            let senders = senders.clone();
            let mut changes = services.converter.subscribe();
            services.spawn(async move {
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    match changes.recv().await {
                        Ok(status) => broadcast(&senders, BackendEvent::Converter(status)),
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });
        }
        let progress = {
            let senders = senders.clone();
            services
                .scripture
                .on_local_bible_install_progress(move |progress| {
                    broadcast(&senders, BackendEvent::PackProgress(progress.clone()))
                })
        };
        Self {
            services,
            senders,
            _progress: progress,
        }
    }

    fn blocking<T, F>(&self, work: F) -> Reply<Result<T, String>>
    where
        T: Send + 'static,
        F: FnOnce(&ApplicationServices) -> Result<T, String> + Send + 'static,
    {
        let services = self.services.clone();
        try_on_runtime(&self.services, async move {
            let core = services.core.clone();
            core.blocking(move || work(&services))
                .await
                .map_err(|e| e.to_string())?
        })
    }
}

/// What the meters say when macOS blocks the microphone.
const MICROPHONE_OFF: &str = "Kairo can't hear this input: microphone access is off. Allow it in System Settings › Privacy & Security › Microphone.";

impl SettingsBackend for ServicesSettings {
    fn settings(&self) -> Reply<Result<AppSettings, String>> {
        self.blocking(|services| {
            let value = services
                .settings_wiring
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_all();
            serde_json::from_value(value).map_err(|e| e.to_string())
        })
    }

    fn get_section(&self, key: &'static str) -> Reply<Result<Value, String>> {
        self.blocking(move |services| {
            Ok(services
                .settings_wiring
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(key))
        })
    }

    fn set_section(&self, key: &'static str, value: Value) -> Reply<Result<(), String>> {
        self.blocking(move |services| {
            services
                .settings_wiring
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set(key, value)
                .map_err(|e| e.to_string())
        })
    }

    fn input_devices(&self) -> Reply<Vec<InputDevice>> {
        on_runtime(&self.services, async move {
            tokio::task::spawn_blocking(|| {
                kairo_stt::devices::list_audio_input_devices()
                    .into_iter()
                    .map(|device| InputDevice {
                        kind: kairo_stt::devices::input_kind(&device).label().into(),
                        device,
                    })
                    .collect()
            })
            .await
            .unwrap_or_default()
        })
    }

    fn output_devices(&self) -> Reply<Vec<String>> {
        on_runtime(&self.services, async move {
            tokio::task::spawn_blocking(|| {
                kairo_audio::output::list_output_devices()
                    .into_iter()
                    .filter(|d| !d.is_default && !d.label.is_empty())
                    .map(|d| d.label)
                    .collect()
            })
            .await
            .unwrap_or_default()
        })
    }

    fn start_meter(
        &self,
        device_id: String,
    ) -> Reply<Result<(MeterHandle, UnboundedReceiver<MeterEvent>), String>> {
        try_on_runtime(&self.services, async move {
            // Without microphone access macOS hands the capture silence, so a
            // live input would look dead. Ask (once), or say how to allow it.
            if !kairo_stt::permission::ensure_microphone_access().await {
                tracing::warn!(device = %device_id, "[Audio] microphone access is off; not metering");
                return Err(MICROPHONE_OFF.to_owned());
            }
            tokio::task::spawn_blocking(move || {
                let (tx, rx) = unbounded();
                let handle = kairo_stt::capture::start_capture(&device_id, move |event| {
                    let event = match event {
                        CaptureEvent::Level(level) => MeterEvent::Level(level),
                        CaptureEvent::Error(error) => MeterEvent::Error(error.message),
                        _ => return,
                    };
                    let _ = tx.unbounded_send(event);
                })
                .map_err(|e| e.message)?;
                Ok((MeterHandle(Box::new(handle)), rx))
            })
            .await
            .map_err(|e| e.to_string())?
        })
    }

    fn send_test(&self) -> Reply<Result<bool, String>> {
        let orchestrator = self.services.orchestrator.clone();
        try_on_runtime(&self.services, async move {
            Ok(orchestrator.test_overlay().await)
        })
    }

    fn clear_all(&self) -> Reply<Result<bool, String>> {
        let services = self.services.clone();
        try_on_runtime(&self.services, async move {
            services
                .telemetry
                .track(kairo_domain::usage::UsageFeature::OutputClear, 1.)
                .await;
            Ok(services.orchestrator.clear_overlay().await)
        })
    }

    fn update_status(&self) -> UpdateStatus {
        self.services.updater.status()
    }

    fn update(&self, action: UpdateAction) {
        self.services.request(match action {
            UpdateAction::Check => ui_shell::ShellRequest::CheckForUpdates,
            UpdateAction::Download => ui_shell::ShellRequest::DownloadUpdate,
            UpdateAction::Install => ui_shell::ShellRequest::InstallUpdate,
        });
    }

    fn workspace(&self) -> Reply<Result<(WorkspaceInfo, MediaFolderMigration), String>> {
        self.blocking(|services| {
            let settings = services.settings.lock().unwrap_or_else(|e| e.into_inner());
            let mut workspace = services.workspace.lock().unwrap_or_else(|e| e.into_inner());
            Ok((
                workspace.info(&settings),
                workspace.media_migration(&settings),
            ))
        })
    }

    fn choose_workspace(
        &self,
        move_contents: bool,
    ) -> Reply<Result<Option<WorkspaceInfo>, String>> {
        let services = self.services.clone();
        try_on_runtime(&self.services, async move {
            let before = {
                let services = services.clone();
                services
                    .core
                    .clone()
                    .blocking(move || {
                        let settings = services.settings.lock().unwrap_or_else(|e| e.into_inner());
                        services
                            .workspace
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .info(&settings)
                            .root
                    })
                    .await
                    .map_err(|e| e.to_string())?
            };
            let info = Handlers(services).choose_workspace(move_contents).await?;
            // The handler answers the current folder when the picker is
            // cancelled; nothing changed, so there is nothing to report.
            Ok((info.root != before).then_some(info))
        })
    }

    fn resync_songs(&self) -> Reply<Result<WorkspaceSongSync, String>> {
        let lyrics = self.services.lyrics.clone();
        try_on_runtime(&self.services, async move {
            lyrics.sync_from_folder().await.map_err(|e| e.to_string())
        })
    }

    fn adopt_media(&self, move_contents: bool) -> Reply<Result<(), String>> {
        let services = self.services.clone();
        try_on_runtime(&self.services, async move {
            Handlers(services)
                .adopt_media(move_contents)
                .await
                .map(|_| ())
        })
    }

    fn reveal_workspace(&self, songs: bool) -> Reply<Result<(), String>> {
        let services = self.services.clone();
        try_on_runtime(&self.services, async move {
            Handlers(services).reveal_workspace(songs).await
        })
    }

    fn converter_status(&self) -> Reply<SlideConverterStatus> {
        let converter = self.services.converter.clone();
        Box::pin(async move { converter.status() })
    }

    fn converter_install(&self) {
        let converter = self.services.converter.clone();
        self.services.spawn(async move {
            converter.install().await;
        });
    }

    fn converter_cancel(&self) {
        self.services.converter.cancel();
    }

    fn converter_remove(&self) {
        let converter = self.services.converter.clone();
        self.services.spawn(async move {
            converter.remove().await;
        });
    }

    fn translations(&self) -> Reply<Result<Vec<ScriptureTranslationOption>, String>> {
        let scripture = self.services.scripture.clone();
        try_on_runtime(&self.services, async move {
            scripture
                .get_translation_options(true, false)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn installed_packs(&self) -> Reply<Result<Vec<InstalledLocalBiblePack>, String>> {
        let scripture = self.services.scripture.clone();
        try_on_runtime(&self.services, async move {
            scripture
                .list_installed_local_bible_packs()
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn pack_status(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>> {
        let scripture = self.services.scripture.clone();
        try_on_runtime(&self.services, async move {
            scripture
                .get_local_bible_pack_status(&translation)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn download_pack(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>> {
        let scripture = self.services.scripture.clone();
        try_on_runtime(&self.services, async move {
            let result = scripture
                .download_local_bible_translation(&translation)
                .await
                .map_err(|e| e.to_string())?;
            Ok(LocalBiblePackStatus {
                translation: result.translation,
                verse_count: result.verse_count,
                chapter_count: result.chapter_count,
                installed: true,
                installation: None,
            })
        })
    }

    fn remove_pack(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>> {
        let scripture = self.services.scripture.clone();
        try_on_runtime(&self.services, async move {
            scripture
                .remove_local_bible_translation(&translation)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn recent_traces(&self, limit: usize) -> Reply<Result<Vec<ScriptureTraceRecord>, String>> {
        let traces = self.services.traces.clone();
        Box::pin(async move { Ok(traces.recent_with_metrics(Some(limit))) })
    }

    fn reset_onboarding(&self) -> Reply<Result<(), String>> {
        let core = self.services.core.clone();
        try_on_runtime(&self.services, async move {
            core.call(OnboardingCommand::Reset)
                .await
                .map_err(|e| e.to_string())?
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    fn events(&self) -> UnboundedReceiver<BackendEvent> {
        let (tx, rx) = unbounded();
        self.senders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        rx
    }
}
