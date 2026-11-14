//! What the Settings window asks of the app: the `settings:*`, `audio`,
//! `output:*`, `app:updates:*`, `workspace:*`, `documents:converter:*` and
//! `scripture:*` calls `Settings.tsx` and its panels make over IPC.
//!
//! The app implements it over its services; tests use a fake. Each call
//! returns a future that does not need any particular runtime, so views
//! await it on GPUI's executor. A rejected IPC call becomes `Err(message)`.

use futures_channel::mpsc::UnboundedReceiver;
use kairo_domain::documents::SlideConverterStatus;
use kairo_domain::scripture::{
    InstalledLocalBiblePack, LocalBibleInstallProgress, LocalBiblePackStatus,
    ScriptureTranslationOption,
};
use kairo_domain::scripture_trace::ScriptureTraceRecord;
use kairo_domain::settings::AppSettings;
use kairo_domain::status::{AudioDevice, AudioLevel, UpdateStatus};
use kairo_domain::workspace::{MediaFolderMigration, WorkspaceInfo, WorkspaceSongSync};
use serde_json::Value;
use std::any::Any;

pub use ui_account::Reply;

/// An update action (`useUpdates`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAction {
    /// `check`.
    Check,
    /// `download`.
    Download,
    /// `install`: quits and installs, asking first while live.
    Install,
}

/// Something the services pushed while the window is open.
#[derive(Clone, Debug, PartialEq)]
pub enum BackendEvent {
    /// `app:updates:status`.
    Update(UpdateStatus),
    /// `documents:converter:changed`.
    Converter(SlideConverterStatus),
    /// `scripture:localBibleInstallProgress`.
    PackProgress(LocalBibleInstallProgress),
}

/// What the input meter reports.
#[derive(Clone, Debug, PartialEq)]
pub enum MeterEvent {
    /// A level reading.
    Level(AudioLevel),
    /// Capture failed or stopped (`getUserMedia` rejecting).
    Error(String),
}

/// A running input meter. Dropping it stops capture.
pub struct MeterHandle(pub Box<dyn Any + Send>);

/// An audio input and the kind its name suggests (`inputKindLabel`).
#[derive(Clone, Debug, PartialEq)]
pub struct InputDevice {
    /// The device.
    pub device: AudioDevice,
    /// `Built-in`, `External`, `Continuity` or `Virtual`.
    pub kind: String,
}

/// The app's side of the Settings window.
pub trait SettingsBackend: Send + Sync + 'static {
    /// The stored settings for the renderer (`useBootstrapStore.settings`).
    fn settings(&self) -> Reply<Result<AppSettings, String>>;
    /// `settings.get(key)`.
    fn get_section(&self, key: &'static str) -> Reply<Result<Value, String>>;
    /// `settings.set(key, value)`.
    fn set_section(&self, key: &'static str, value: Value) -> Reply<Result<(), String>>;

    /// `listAudioInputDevices`: the default first.
    fn input_devices(&self) -> Reply<Vec<InputDevice>>;
    /// Labels of the audio outputs, the system default left out.
    fn output_devices(&self) -> Reply<Vec<String>>;
    /// Starts metering `device_id` (`"default"` for the system default).
    fn start_meter(
        &self,
        device_id: String,
    ) -> Reply<Result<(MeterHandle, UnboundedReceiver<MeterEvent>), String>>;

    /// `output.sendTest`.
    fn send_test(&self) -> Reply<Result<bool, String>>;
    /// `output.clearAll`.
    fn clear_all(&self) -> Reply<Result<bool, String>>;

    /// `app:updates:status` now.
    fn update_status(&self) -> UpdateStatus;
    /// An update action; the result arrives as [`BackendEvent::Update`].
    fn update(&self, action: UpdateAction);

    /// `workspace.get` and `workspace.mediaMigration`.
    fn workspace(&self) -> Reply<Result<(WorkspaceInfo, MediaFolderMigration), String>>;
    /// `workspace.chooseFolder({ move })`. `Ok(None)`: the picker was cancelled.
    fn choose_workspace(&self, move_contents: bool)
    -> Reply<Result<Option<WorkspaceInfo>, String>>;
    /// `workspace.resyncSongs`.
    fn resync_songs(&self) -> Reply<Result<WorkspaceSongSync, String>>;
    /// `workspace.adoptMedia({ move })`.
    fn adopt_media(&self, move_contents: bool) -> Reply<Result<(), String>>;
    /// `workspace.reveal` (`songs` false) or `workspace.revealSongs`.
    fn reveal_workspace(&self, songs: bool) -> Reply<Result<(), String>>;

    /// `documents.converter.status`.
    fn converter_status(&self) -> Reply<SlideConverterStatus>;
    /// `documents.converter.install`.
    fn converter_install(&self);
    /// `documents.converter.cancel`.
    fn converter_cancel(&self);
    /// `documents.converter.remove`.
    fn converter_remove(&self);

    /// `scripture.getTranslations`.
    fn translations(&self) -> Reply<Result<Vec<ScriptureTranslationOption>, String>>;
    /// `scripture.listInstalledLocalBiblePacks`.
    fn installed_packs(&self) -> Reply<Result<Vec<InstalledLocalBiblePack>, String>>;
    /// `scripture.getLocalBiblePackStatus`.
    fn pack_status(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>>;
    /// `scripture.downloadLocalBibleTranslation`.
    fn download_pack(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>>;
    /// `scripture.removeLocalBibleTranslation`.
    fn remove_pack(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>>;
    /// `scripture.recentTraces(limit)`.
    fn recent_traces(&self, limit: usize) -> Reply<Result<Vec<ScriptureTraceRecord>, String>>;

    /// `onboarding.reset` ("Run setup again"): nothing else is reset.
    fn reset_onboarding(&self) -> Reply<Result<(), String>>;

    /// Pushes while the window is open.
    fn events(&self) -> UnboundedReceiver<BackendEvent>;
}
