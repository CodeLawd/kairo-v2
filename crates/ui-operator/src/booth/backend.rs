//! What the booth (transcript, detected verses, queue and the toolbox panels)
//! reads and asks for. The application implements [`BoothBackend`] over its
//! services; tests drive a scripted fake.
//!
//! Electron reached these through `window.api.orchestrator`, `scripture`,
//! `transcription`, `services`, `resilience` and `program`, and through the
//! renderer stores (`useBootstrapStore`, `useAppStore`, `useLicense`).

use std::sync::Arc;

use kairo_domain::{
    program::{PresentationSettings, ProgramState},
    scripture::{
        LivePlanState, PendingAutoPresent, ScriptureResult, ScriptureRowTranslation,
        ScriptureSuggestion, ScriptureTranslationOption, SermonPlan,
    },
    services::{ServiceCommand, ServiceSnapshot},
    settings::{ScriptureAutomation, ShortcutBindings},
    status::{OrchestratorStatus, ResilienceStatus},
    transcript::TranscriptResult,
};

/// A nugget saved before service records existed (`kairo-message-nuggets`).
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyNugget {
    pub id: String,
    pub text: String,
    pub captured_at: f64,
}

/// The input level (`useAppStore.audioLevel`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputLevel {
    pub rms: f64,
    pub clipping: bool,
}

/// Device labels by kind (`useDeviceLabels`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceLabels {
    pub video_inputs: Vec<String>,
    pub audio_inputs: Vec<String>,
    pub audio_outputs: Vec<String>,
}

/// Everything the booth starts from, read once when it opens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoothSnapshot {
    pub status: Option<OrchestratorStatus>,
    pub resilience: Option<ResilienceStatus>,
    /// `None` until the service records have loaded.
    pub services: Option<ServiceSnapshot>,
    pub legacy_nuggets: Vec<LegacyNugget>,
    pub default_translation: String,
    pub shortcuts: ShortcutBindings,
    pub automation: ScriptureAutomation,
    pub translations: Vec<ScriptureTranslationOption>,
    pub live_plan: Option<LivePlanState>,
    pub plans: Vec<SermonPlan>,
    pub licence: Licence,
    pub program: ProgramState,
    pub presentation: Option<PresentationSettings>,
    pub devices: DeviceLabels,
}

/// `useLicense()`: what transcription needs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Licence {
    pub activated: bool,
    pub deepgram: bool,
}

impl Licence {
    /// `hasDeepgram`.
    pub fn transcription(&self) -> bool {
        self.activated && self.deepgram
    }
}

/// A push from the core, in the order it happened. Each lives only from the
/// sender to the UI thread, so variants are not boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum BoothFeed {
    Transcript(TranscriptResult),
    Interim(String),
    Suggestion(ScriptureSuggestion),
    PendingAuto(PendingAutoPresent),
    Status(OrchestratorStatus),
    Resilience(ResilienceStatus),
    Services(ServiceSnapshot),
    RetranslateRow(ScriptureRowTranslation),
    Level(InputLevel),
    Program(ProgramState),
    Presentation(PresentationSettings),
    Licence(Licence),
    LivePlan(Option<LivePlanState>),
    Plans(Vec<SermonPlan>),
    Settings {
        default_translation: String,
        shortcuts: ShortcutBindings,
        automation: ScriptureAutomation,
    },
    Devices(DeviceLabels),
}

/// Which program layer `setOnScreens` acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnScreensLayer {
    Countdown,
    Clock,
}

/// One request. Each runs off the UI thread and answers once.
#[derive(Clone, Debug, PartialEq)]
pub enum BoothCall {
    /// `orchestrator.approveSuggestion`.
    Approve(String),
    /// `orchestrator.dismissSuggestion`.
    Dismiss(String),
    /// `scripture.register`.
    Register(ScriptureSuggestion),
    /// `scripture.presentDirectly`.
    PresentDirectly(ScriptureSuggestion),
    /// `scripture.search`; answers [`BoothAnswer::Results`].
    Search {
        query: String,
        translation: String,
    },
    /// `scripture.retranslate`; answers [`BoothAnswer::Suggestion`].
    Retranslate {
        suggestion: ScriptureSuggestion,
        translation: String,
    },
    /// `scripture.getTranslations`; answers [`BoothAnswer::Translations`].
    Translations,
    /// `scripture.setAutoMode`.
    SetAutoMode(bool),
    /// Follow-along moved the screen on: a spoken "next verse" right after
    /// asks for that verse, not the one after.
    FollowStepped,
    /// Saves every automation option and applies it to the running service.
    SetAutomation(ScriptureAutomation),
    /// Auto-clear: takes scripture off the screens while this verse is live.
    ClearScriptureIfLive {
        book: String,
        chapter: f64,
        verse: f64,
    },
    /// `orchestrator.start` with the configuration built from settings.
    StartListening {
        auto_mode: bool,
    },
    /// `orchestrator.stop`.
    StopListening,
    /// `services.command`; answers [`BoothAnswer::Services`].
    Service(ServiceCommand),
    /// `resilience.restoreSession`.
    RestoreSession,
    /// `resilience.discardSession`.
    DiscardSession,
    /// `clearLiveText`.
    ClearText,
    TimerSet {
        seconds: f64,
        id: String,
    },
    TimerStart(String),
    TimerPause(String),
    TimerReset(String),
    TimerShow(String),
    SetOnScreens {
        layer: OnScreensLayer,
        on: bool,
    },
    SetStageMessage(Option<String>),
    ShowMessage(String),
    ClearMessage,
    SetProp {
        id: String,
        on: bool,
    },
    ClearProps,
    /// `program.pickImage`; answers [`BoothAnswer::Picked`].
    PickImage,
    SetCamera {
        label: Option<String>,
        audio: Option<String>,
    },
    /// Asks the system for camera access, so inputs can be listed.
    RequestCameraAccess,
    /// `downloadFile`: writes `body` as a file for the operator.
    Export {
        file_name: String,
        body: String,
    },
    /// `navigator.clipboard.writeText` is done in the view; this is the
    /// account site (`account.openWeb`).
    OpenWeb(String),
}

/// A call's answer; short-lived, so variants are not boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum BoothAnswer {
    Done,
    Results(Vec<ScriptureResult>),
    Suggestion(ScriptureSuggestion),
    Translations(Vec<ScriptureTranslationOption>),
    Services(ServiceSnapshot),
    /// The chosen file, or `None` when the picker was closed.
    Picked(Option<String>),
    /// `RequestCameraAccess`: the video inputs, once access was granted.
    VideoInputs(Vec<String>),
    /// `ClearScriptureIfLive`: whether the verse was still live and cleared.
    Cleared(bool),
}

pub type Reply = Box<dyn FnOnce(Result<BoothAnswer, String>) + Send>;
pub type FeedSink = Arc<dyn Fn(BoothFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;

pub trait BoothBackend: Send + Sync + 'static {
    fn snapshot(&self) -> BoothSnapshot;
    /// Starts pushing to `sink` until the guard drops.
    fn listen(&self, sink: FeedSink) -> Listening;
    fn call(&self, call: BoothCall, reply: Reply);
    /// `scripture.markRendered`: the verse reached the operator's screen.
    fn mark_rendered(&self, correlation_id: &str);
    /// `useDraftPresentation`'s save.
    fn save_presentation(&self, presentation: PresentationSettings);
    /// Keeps the legacy nugget list (`localStorage` in Electron).
    fn save_legacy_nuggets(&self, nuggets: &[LegacyNugget]);
}
