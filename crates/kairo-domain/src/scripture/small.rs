//! Small scripture rules: single-verse presentation
//! (`single-verse-presentation.ts`), spoken verse text (`verse-text.ts`), the
//! detector recovery state machine (`detector-recovery.ts`) and pushing a
//! browsed verse from the Scripture tab (`scripture-tab-presentation.ts`).

use super::ScriptureSuggestion;
use super::js;
use serde::{Deserialize, Serialize};
use std::future::Future;

/// All scripture outputs present exactly one verse, even for legacy range payloads.
pub fn single_verse_suggestion(suggestion: &ScriptureSuggestion) -> ScriptureSuggestion {
    let Some(verse) = suggestion.verses.first() else {
        return suggestion.clone();
    };
    ScriptureSuggestion {
        reference: verse_reference(verse),
        verses: vec![verse.clone()],
        ..suggestion.clone()
    }
}

fn verse_reference(verse: &super::ScriptureVerse) -> String {
    format!(
        "{} {}:{}",
        verse.book,
        js::number_to_string(verse.chapter),
        js::number_to_string(verse.verse)
    )
}

/// One staged suggestion per verse, sharing a passage id.
pub fn split_verse_suggestions(suggestion: &ScriptureSuggestion) -> Vec<ScriptureSuggestion> {
    let length = suggestion.verses.len() as f64;
    suggestion
        .verses
        .iter()
        .enumerate()
        .map(|(index, verse)| ScriptureSuggestion {
            id: format!("{}-{index}", suggestion.id),
            reference: verse_reference(verse),
            verses: vec![verse.clone()],
            passage_id: Some(
                suggestion
                    .passage_id
                    .clone()
                    .unwrap_or_else(|| suggestion.id.clone()),
            ),
            passage_reference: Some(suggestion.reference.clone()),
            passage_index: Some(index as f64),
            passage_length: Some(length),
            ..suggestion.clone()
        })
        .collect()
}

/// Verse text as read aloud. The bundled KJV braces supplied words
/// (`all {ye} that labour`), which are read, and marginal notes
/// (`{firmament: Heb. expansion}`), which are not and always hold a colon.
pub fn spoken_verse_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open + 1..].find('}') else {
            break;
        };
        let inner = &rest[open + 1..open + 1 + close];
        out.push_str(&rest[..open]);
        out.push_str(if inner.contains(':') { " " } else { inner });
        rest = &rest[open + 1 + close + 1..];
    }
    out.push_str(rest);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectorRecoveryState {
    pub consecutive_errors: u32,
    pub fallback_active: bool,
    pub probing: bool,
}

crate::serde_util::string_enum!(
    pub enum DetectorRecoveryEvent {
        RequestFailed = "request_failed",
        BeginProbe = "begin_probe",
        RequestSucceeded = "request_succeeded",
    }
);

/// Two failures in a row switch to the fallback detector; a probe tries the
/// AI again but stays degraded until a request actually succeeds.
pub fn transition_detector_recovery(
    state: DetectorRecoveryState,
    event: DetectorRecoveryEvent,
) -> DetectorRecoveryState {
    match event {
        DetectorRecoveryEvent::RequestSucceeded => DetectorRecoveryState {
            consecutive_errors: 0,
            fallback_active: false,
            probing: false,
        },
        DetectorRecoveryEvent::BeginProbe => DetectorRecoveryState {
            fallback_active: false,
            probing: true,
            ..state
        },
        DetectorRecoveryEvent::RequestFailed => {
            let consecutive_errors = state.consecutive_errors.saturating_add(1);
            DetectorRecoveryState {
                consecutive_errors,
                fallback_active: consecutive_errors >= 2,
                probing: false,
            }
        }
    }
}

/// The scripture service calls the Scripture tab may use.
pub trait ScriptureTabPresentationApi {
    type Error;
    /// Shows a suggestion on the outputs without publishing it as detected content.
    fn present_directly(
        &self,
        suggestion: &ScriptureSuggestion,
    ) -> impl Future<Output = Result<(), Self::Error>>;
    /// Publishes a suggestion as detected content. Not used by a tab push.
    fn register(
        &self,
        suggestion: &ScriptureSuggestion,
    ) -> impl Future<Output = Result<(), Self::Error>>;
    /// Approves a detected suggestion. Not used by a tab push.
    fn approve(&self, id: &str) -> impl Future<Output = Result<(), Self::Error>>;
}

/// Presents a browsed verse without publishing it as transcription-detected content.
pub async fn push_scripture_from_tab<A: ScriptureTabPresentationApi>(
    api: &A,
    suggestion: &ScriptureSuggestion,
) -> Result<(), A::Error> {
    api.present_directly(suggestion).await
}
