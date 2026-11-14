//! What the Scripture page reads and asks for. The application implements
//! [`ScriptureBackend`] over its services; tests drive a scripted fake.
//!
//! Electron reached these through `window.api.scripture`, `settings`,
//! `passages`, `libraries` and `transfer`, and the renderer stores
//! (`useBootstrapStore`, `useAppStore`, `usePassagesStore`, `useLibraries`).

use std::sync::Arc;

use kairo_domain::collections::{LibrariesCommand, LibraryState};
use kairo_domain::scripture::{
    LivePlanState, PassagesCommand, SavedPassage, ScriptureResult, ScriptureSuggestion,
    ScriptureTranslationOption, SermonNotesAnalysis, SermonPlan, SermonPlanDraft,
};
use serde_json::Value;

/// The overlay and scripture settings the page shows and the Options panel
/// changes.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideOptions {
    /// `overlay.showVerseNumbers`.
    pub show_verse_numbers: bool,
    /// `overlay.showTranslation`.
    pub show_translation: bool,
    /// `overlay.maxVerses`; 0 for no cap.
    pub max_verses: f64,
    /// `scripture.defaultTranslation`.
    pub default_translation: String,
}

impl Default for SlideOptions {
    fn default() -> Self {
        Self {
            show_verse_numbers: true,
            show_translation: true,
            max_verses: 0.0,
            default_translation: kairo_domain::settings::DEFAULT_TRANSLATION_ID.into(),
        }
    }
}

/// Everything the page starts from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptureSnapshot {
    pub options: SlideOptions,
    pub translations: Vec<ScriptureTranslationOption>,
    pub plans: Vec<SermonPlan>,
    pub live_plan: Option<LivePlanState>,
    pub passages: Vec<SavedPassage>,
    /// The scripture libraries.
    pub libraries: LibraryState,
    /// The stored `kairo-scripture-recent-lookups` value.
    pub recent_lookups: Option<Value>,
    /// `shouldShowApiBibleWarning(...)` for this launch.
    pub api_bible_warning: bool,
}

/// A push from the core.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptureFeed {
    Options(SlideOptions),
    Translations(Vec<ScriptureTranslationOption>),
    /// The stored playlists (an import or another screen changed them).
    Plans(Vec<SermonPlan>),
    LivePlan(Option<LivePlanState>),
    Passages(Vec<SavedPassage>),
    Libraries(LibraryState),
    /// Header Clear (or the Operator's) emptied the screens.
    OutputCleared,
    ApiBibleWarning(bool),
}

/// What the Options panel saves.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionChange {
    /// Both `overlay.showVerseNumbers` and `scripture.showVerseNumbers`.
    VerseNumbers(bool),
    ShowTranslation(bool),
    /// `overlay.maxVerses`: 1, or 0 for no cap.
    MaxVerses(f64),
    DefaultTranslation(String),
}

/// One request. Each runs off the UI thread and answers once; short-lived,
/// so variants are not boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptureCall {
    /// `scripture.search`; answers [`ScriptureAnswer::Results`].
    Search { query: String, translation: String },
    /// `scripture.saveSermonPlan`; answers [`ScriptureAnswer::Plan`].
    SavePlan(SermonPlan),
    /// `scripture.deleteSermonPlan`.
    DeletePlan(String),
    /// `scripture.setLivePlan`; answers [`ScriptureAnswer::LivePlan`].
    SetLivePlan(String),
    /// `pushScriptureFromTab` (`scripture.presentDirectly`).
    Present(ScriptureSuggestion),
    /// `passages.command`; answers [`ScriptureAnswer::Passages`].
    Passages(PassagesCommand),
    /// `libraries.command` for scripture; answers [`ScriptureAnswer::Libraries`].
    Libraries(LibrariesCommand),
    /// `settings.set('overlay' | 'scripture', ...)`.
    SaveOption(OptionChange),
    /// `scripture.importSermonNotes`: the file dialog and the scan; answers
    /// [`ScriptureAnswer::Draft`] (`None` when the dialog was cancelled).
    ImportSermonNotes,
    /// `scripture.scanSermonNotes(text)`, off the UI thread; answers
    /// [`ScriptureAnswer::Analysis`].
    ScanSermonNotes(String),
    /// `exportKairo({ kind: 'playlist', planId })`.
    ExportPlaylist(String),
    /// `importKairo()`.
    ImportKairo,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptureAnswer {
    Done,
    Results(Vec<ScriptureResult>),
    Plan(SermonPlan),
    LivePlan(LivePlanState),
    Passages(Vec<SavedPassage>),
    Libraries(LibraryState),
    Draft(Option<SermonPlanDraft>),
    Analysis(SermonNotesAnalysis),
}

pub type Reply = Box<dyn FnOnce(Result<ScriptureAnswer, String>) + Send>;
pub type FeedSink = Arc<dyn Fn(ScriptureFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;

pub trait ScriptureBackend: Send + Sync + 'static {
    fn snapshot(&self) -> ScriptureSnapshot;
    /// Starts pushing to `sink` until the guard drops.
    fn listen(&self, sink: FeedSink) -> Listening;
    fn call(&self, call: ScriptureCall, reply: Reply);
    /// Stores `kairo-scripture-recent-lookups` in `ui-state`.
    fn save_recent_lookups(&self, value: Value);
}
