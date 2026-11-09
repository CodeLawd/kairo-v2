//! Scripture verses, suggestions, translations and sermon playlists, from
//! `src/lib/ipc.ts` (prompt 012), and the scripture logic (prompt 015):
//!
//! - [`translations`] and [`versification`]: the translation registry.
//! - [`query`]: book names, reference parsing and search-box behaviour.
//! - [`live_progress`]: grouping, merging and following detected scripture.
//! - [`plan`]: sermon playlist matching and resolving.
//! - [`notes_review`]: sermon notes text and offsets, independent of any editor.
//! - [`spoken_translations`]: translations named aloud.
//! - [`trace`]: latency trace metrics, summaries and the trace store.
//! - [`offline_state`]: cached API.Bible notices and offline Bible rows.
//! - [`small`]: single-verse presentation, spoken verse text, detector
//!   recovery and Scripture-tab pushes.

mod js;
pub mod live_progress;
pub mod notes_review;
pub mod offline_state;
pub mod plan;
pub mod query;
pub mod small;
pub mod spoken_translations;
pub mod trace;
pub mod translations;
pub mod versification;

use crate::serde_util::{Extra, True, nullable, present, required, string_enum};
use serde::{Deserialize, Serialize};

/// A translation id such as `KJV`. Open-ended on purpose: a newly registered
/// or user-installed id needs no type change.
pub type ScriptureTranslation = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptureVerse {
    pub book: String,
    pub chapter: f64,
    pub verse: f64,
    pub text: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureRowTranslation {
    pub passage_id: String,
    pub translation: ScriptureTranslation,
}

/// Where licensed cached text came from; revalidated before use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureCacheSource {
    pub bible_id: String,
    pub book_id: f64,
    pub chapter: f64,
    pub verse: f64,
    pub fetched_at: f64,
    pub expires_at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum SuggestionSource {
        Auto = "auto",
        Manual = "manual",
    }
);
string_enum!(
    pub enum PlanMatch {
        Reference = "reference",
        Quote = "quote",
    }
);

/// A detected or chosen passage. Persisted inside service records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureSuggestion {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub cache_source: Option<ScriptureCacheSource>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub quote_corpus_version: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub quote_evidence: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub quote_revision: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub retracted: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub provisional: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub suggestion_only: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub ambiguous: Option<bool>,
    pub id: String,
    pub reference: String,
    pub verses: Vec<ScriptureVerse>,
    pub translation: ScriptureTranslation,
    pub confidence: f64,
    pub source: SuggestionSource,
    pub trigger_text: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub passage_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub passage_reference: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub passage_index: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub passage_length: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub correlation_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub preloaded_next: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub plan_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub plan_item_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub plan_match: Option<PlanMatch>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub requested_translation: Option<ScriptureTranslation>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptureResult {
    pub reference: String,
    pub verses: Vec<ScriptureVerse>,
    pub translation: ScriptureTranslation,
}

string_enum!(
    pub enum TranslationAccess {
        Local = "local",
        Api = "api",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureTranslationOption {
    pub id: ScriptureTranslation,
    pub name: String,
    pub access: TranslationAccess,
    pub available: bool,
    pub requires_api_key: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloadable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_approx: Option<String>,
}

string_enum!(
    pub enum ApiBibleAuthorizationState {
        Unchecked = "unchecked",
        Checking = "checking",
        Authorized = "authorized",
        Unauthorized = "unauthorized",
        Offline = "offline",
    }
);
string_enum!(
    pub enum ApiBibleCacheStatus {
        NotDownloaded = "not-downloaded",
        Partial = "partial",
        Downloading = "downloading",
        Paused = "paused",
        Downloaded = "downloaded",
        Stale = "stale",
        Unavailable = "unavailable",
        Failed = "failed",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiBibleOfflineTranslation {
    pub bible_id: String,
    pub translation: ScriptureTranslation,
    pub name: String,
    pub copyright: String,
    pub status: ApiBibleCacheStatus,
    pub cached_chapters: f64,
    pub total_chapters: f64,
    pub cached_verses: f64,
    #[serde(deserialize_with = "required")]
    pub fetched_at: Option<f64>,
    #[serde(deserialize_with = "required")]
    pub expires_at: Option<f64>,
    pub offline_download_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiBibleDownloadProgress {
    pub bible_id: String,
    pub status: ApiBibleCacheStatus,
    pub completed_chapters: f64,
    pub total_chapters: f64,
    pub cached_verses: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

string_enum!(
    pub enum LocalBibleInstallPhase {
        Downloading = "downloading",
        Verifying = "verifying",
        Installing = "installing",
        Ready = "ready",
        Failed = "failed",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBibleInstallProgress {
    pub translation: String,
    pub phase: LocalBibleInstallPhase,
    pub received_bytes: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<f64>,
    pub revision: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBiblePackStatus {
    pub translation: String,
    pub verse_count: f64,
    pub chapter_count: f64,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation: Option<LocalBibleInstallProgress>,
}

/// `InstalledLocalBiblePack extends LocalBiblePackStatus`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstalledLocalBiblePack {
    pub name: String,
    #[serde(flatten)]
    pub status: LocalBiblePackStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBiblePackInstallResult {
    pub translation: String,
    pub verse_count: f64,
    pub chapter_count: f64,
    pub installed: True,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonScriptureItem {
    pub id: String,
    pub reference: String,
    pub translation: ScriptureTranslation,
    pub verses: Vec<ScriptureVerse>,
    pub available: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub error: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A sermon playlist. Persisted in `kairo-sermon-plans.json` and service records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonPlan {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub source_text: Option<String>,
    pub id: String,
    pub title: String,
    pub source_file_name: String,
    pub items: Vec<SermonScriptureItem>,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `kairo-sermon-plans.json` (`SermonPlanSchema` in the sermon-plans service).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonPlanStore {
    pub plans: Vec<SermonPlan>,
    #[serde(deserialize_with = "required")]
    pub live_plan_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePlanState {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub next_reference: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub next_plan_item_id: Option<Option<String>>,
    #[serde(deserialize_with = "required")]
    pub plan_id: Option<String>,
    #[serde(deserialize_with = "required")]
    pub title: Option<String>,
    pub item_count: f64,
    pub unavailable_count: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonReferenceMatch {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub reference: String,
    pub translations: Vec<ScriptureTranslation>,
}

/// `Pick<SermonScriptureItem, 'id' | 'reference' | 'translation'>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonPlanDraftItem {
    pub id: String,
    pub reference: String,
    pub translation: ScriptureTranslation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonPlanDraft {
    pub title: String,
    pub source_file_name: String,
    pub text: String,
    pub html: String,
    pub matches: Vec<SermonReferenceMatch>,
    pub items: Vec<SermonPlanDraftItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonNotesAnalysis {
    pub matches: Vec<SermonReferenceMatch>,
    pub items: Vec<SermonPlanDraftItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAutoPresent {
    pub suggestion_id: String,
    pub reference: String,
    pub expires_at: f64,
}

/// `passages.ts` `SavedPassage`. Persisted in `kairo-passages.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPassage {
    pub id: String,
    pub reference: String,
    pub translation: ScriptureTranslation,
    pub verses: Vec<ScriptureVerse>,
    pub saved_at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `kairo-passages.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PassageStore {
    pub passages: Vec<SavedPassage>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `PassagesCommand`, tagged by `action`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum PassagesCommand {
    List,
    #[serde(rename_all = "camelCase")]
    Save {
        result: ScriptureResult,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        library_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Remove {
        passage_id: String,
    },
}
