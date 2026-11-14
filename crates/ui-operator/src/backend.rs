//! What the operator workspace reads and asks for. The application
//! implements [`OperatorBackend`] over its services; tests use a fake.

use kairo_core::preview::PreviewHub;
use kairo_domain::{
    media::{MediaItem, MediaLibrary, MediaPlayback, normalize_media_playback},
    overlay::OverlayOutput,
    program::ProgramState,
};
use kairo_engine_proto::VideoTime;

use crate::layers::Layer;

/// Everything the live rail draws from, read in one go.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveSnapshot {
    /// `settings.overlay.outputs`, as stored.
    pub outputs: Vec<OverlayOutput>,
    pub program: ProgramState,
    pub media: MediaLibrary,
    /// `useTracksPlaybackStore` `library.liveId !== null`.
    pub track_live: bool,
    /// A document page is the live background (`liveDocumentPreview`).
    pub document_live: bool,
}

impl Default for LiveSnapshot {
    /// Nothing configured and nothing live, before the first read.
    fn default() -> Self {
        Self {
            outputs: Vec::new(),
            program: ProgramState::default(),
            media: MediaLibrary {
                folder: String::new(),
                folders: Vec::new(),
                items: Vec::new(),
                playlists: Vec::new(),
                playback: Default::default(),
                live_item_id: None,
                live_playback: None,
                live_cue: None,
                live_paused: false,
                error: None,
            },
            track_live: false,
            document_live: false,
        }
    }
}

/// `liveMediaFromLibrary`: the live dock item with its playback.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveMedia {
    pub item: MediaItem,
    pub playback: MediaPlayback,
    pub paused: bool,
    pub cue: f64,
}

impl LiveSnapshot {
    /// `None` when nothing is live, or the live id names an item that is gone.
    pub fn live_media(&self) -> Option<LiveMedia> {
        let id = self.media.live_item_id.as_deref()?;
        let item = self.media.items.iter().find(|item| item.id == id)?;
        let playback = self
            .media
            .live_playback
            .clone()
            .or_else(|| self.media.playback.get(id).cloned())
            .unwrap_or_else(|| normalize_media_playback(&serde_json::Value::Null));
        Some(LiveMedia {
            item: item.clone(),
            playback,
            paused: self.media.live_paused,
            cue: self.media.live_cue.unwrap_or(0.0),
        })
    }

    /// `hasText`: a slide is on screen.
    pub fn has_text(&self) -> bool {
        self.program.current.is_some()
    }

    /// `hasBackground`: a dock item or a document page.
    pub fn has_background(&self) -> bool {
        self.live_media().is_some() || self.document_live
    }
}

/// A request the workspace makes. Each runs to completion off the UI thread
/// and reports back through the `done` callback.
#[derive(Clone, Debug, PartialEq)]
pub enum OperatorCommand {
    /// Takes down one layer and nothing else.
    Clear(Layer),
    /// `clearLiveAll` (text and background together), then each of `others`.
    ClearAll { others: Vec<Layer> },
    /// `media.setPaused`.
    SetPaused(bool),
    /// `media.seek`, already clamped.
    Seek(f64),
    /// `media.setPlayback(id, { loop })`.
    SetLoop { item_id: String, looped: bool },
}

pub type Done = Box<dyn FnOnce(Result<(), String>) + Send>;
pub type TimeReply = Box<dyn FnOnce(Option<VideoTime>) + Send>;

pub trait OperatorBackend: Send + Sync + 'static {
    fn snapshot(&self) -> LiveSnapshot;
    fn run(&self, command: OperatorCommand, done: Done);
    /// The background video's clock on `output`, `None` while it has none
    /// or the engine cannot say.
    fn video_time(&self, output: &str, reply: TimeReply);
    /// Shared by every live monitor; `None` when previews cannot start
    /// (no engine, or the map could not be created).
    fn preview_hub(&self) -> Option<PreviewHub>;
}
