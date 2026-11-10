//! What the orchestrator drives. Each service is behind a trait so the
//! orchestrator is testable before the output engine (phase 4) exists, and
//! so a fake records exactly what was asked of it.

use crate::voice::{BoxFuture, LiveScriptureContext};
use kairo_domain::documents::DocumentSlide;
use kairo_domain::media::{MediaItem, MediaPlayback};
use kairo_domain::overlay::{OverlayBackground, OverlayOutput, OverlayTheme};
use kairo_domain::program::ProgramSlideInfo;
use kairo_domain::scripture::{ScriptureResult, ScriptureSuggestion, ScriptureVerse};
use kairo_domain::settings::OverlaySettings;
use kairo_domain::status::SttProvider;
use kairo_domain::transcript::{InterimResult, TranscriptResult};
use serde::{Deserialize, Serialize};

/// A lyric line with its gloss or paint colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColoredLine {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Content filter keys `liveTargets(show)` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowKind {
    Documents,
    Backgrounds,
}

/// The output engine's surfaces (`SurfaceManager` in Electron). One surface
/// per rendered output; surface ids are output ids.
pub trait EngineClient: Send + Sync {
    /// `None` when the output has a surface, else `outputState(output).reason`.
    fn unavailable_reason(&self, output: &OverlayOutput) -> Option<String>;
    fn show_slide(
        &self,
        output_id: String,
        reference: String,
        text: String,
        theme: OverlayTheme,
        colored: Option<Vec<ColoredLine>>,
    ) -> BoxFuture<'_, bool>;
    fn set_background(
        &self,
        output_id: String,
        background: OverlayBackground,
    ) -> BoxFuture<'_, bool>;
    fn swap_document_page(
        &self,
        output_id: String,
        media_path: String,
        slide: Option<DocumentSlide>,
    ) -> BoxFuture<'_, bool>;
    fn show_document(
        &self,
        output_id: String,
        theme: OverlayTheme,
        slide: Option<DocumentSlide>,
    ) -> BoxFuture<'_, bool>;
    /// `false` when nothing was painted on that surface.
    fn clear_text(
        &self,
        surface_id: String,
        background: Option<OverlayBackground>,
    ) -> BoxFuture<'_, bool>;
    fn clear(&self, surface_id: String) -> BoxFuture<'_, ()>;
    /// Outputs that can take a push now; with `show`, only those whose filter allows it.
    fn live_targets(&self, show: Option<ShowKind>) -> Vec<OverlayOutput>;
    fn all_surfaces(&self) -> Vec<String>;
}

/// Where the live scripture slide came from, for the program.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptureSlideMeta {
    pub book: String,
    pub chapter: f64,
    pub verse_start: f64,
    pub verse_end: f64,
    pub translation: String,
    pub passage_id: String,
    pub passage_index: f64,
}

/// The program state (`programService`).
pub trait ProgramClient: Send + Sync {
    /// The live scripture and when it went live (ms).
    fn scripture(&self) -> Option<(LiveScriptureContext, f64)>;
    fn set_scripture_slide(&self, reference: String, text: String, meta: ScriptureSlideMeta);
    fn set_slide(&self, reference: String, text: String, next: Option<ProgramSlideInfo>);
    fn clear_current(&self);
    fn clear_program(&self);
}

/// The Media dock's live background (`mediaService`).
pub trait MediaClient: Send + Sync {
    fn live_item(&self) -> Option<MediaItem>;
    fn live_playback(&self) -> Option<MediaPlayback>;
    fn live_cue(&self) -> f64;
    fn clear_live_item(&self);
}

/// Scripture data and the Operator's suggestion list (`scriptureService`).
pub trait ScriptureClient: Send + Sync {
    fn search(
        &self,
        query: String,
        translation: String,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, String>>;
    fn receive_suggestion(&self, suggestion: ScriptureSuggestion);
    fn pending_suggestion(&self, id: &str) -> Option<ScriptureSuggestion>;
    fn dismiss_suggestion(&self, id: &str);
    fn request_row_translation(&self, passage_id: &str, translation: &str);
}

/// Verses of a chapter, for voice navigation.
pub async fn chapter_verses(
    scripture: &dyn ScriptureClient,
    book: &str,
    chapter: f64,
    translation: String,
) -> Result<Vec<ScriptureVerse>, String> {
    let query = format!("{book} {}", kairo_domain::js::number_to_string(chapter));
    Ok(scripture
        .search(query, translation)
        .await?
        .into_iter()
        .flat_map(|r| r.verses)
        .collect())
}

/// Transcription (`sttService` plus the audio relay).
pub trait SttClient: Send + Sync {
    fn configure(&self, provider: SttProvider, language: &str, keyterms: Vec<String>);
    fn start(&self) -> BoxFuture<'_, Result<(), String>>;
    fn stop(&self);
    fn history(&self) -> Vec<TranscriptResult>;
    fn set_history(&self, history: Vec<TranscriptResult>);
    /// Creates the PCM relay stream and hands it to the provider.
    fn attach_audio(&self) -> Result<(), String>;
    fn detach_audio(&self);
}

/// Live transcript events the STT side delivers to the orchestrator.
#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptEvent {
    Final(TranscriptResult),
    Interim(InterimResult),
}

/// Licence and service checks for `orchestrator:start`.
pub trait SessionGate: Send + Sync {
    fn assert_transcription(&self) -> BoxFuture<'_, Result<(), String>>;
    fn has_active_service(&self) -> bool;
}

/// Overlay settings, read at use time.
pub trait SettingsClient: Send + Sync {
    fn overlay(&self) -> OverlaySettings;
}

/// `usageService.track`.
pub trait UsageClient: Send + Sync {
    fn track(&self, event: &str);
}
