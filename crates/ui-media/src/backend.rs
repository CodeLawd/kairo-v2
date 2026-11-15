//! What the media dock reads and asks for. The application implements
//! [`MediaBackend`] over its media service and actions; tests drive a
//! scripted fake.
//!
//! Electron reached these through `window.api.media` and `localStorage`.

use std::path::PathBuf;
use std::sync::Arc;

use kairo_domain::media::MediaLibrary;
use serde_json::{Map, Value};

/// A push from the core.
/// Short-lived, so variants are not boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum MediaFeed {
    /// `media:library`.
    Library(MediaLibrary),
    /// File → Import → Images or Videos (`useImportRequest(['image', 'video'])`).
    Import(ImportKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportKind {
    Image,
    Video,
}

/// One request. Every call answers the library unless noted.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaCall {
    /// `media.push`; answers [`MediaAnswer::Pushed`].
    Push(String),
    ChooseFolder,
    CreatePlaylist(String),
    RenamePlaylist {
        id: String,
        name: String,
    },
    DeletePlaylist(String),
    SetPlaylistItems {
        id: String,
        item_ids: Vec<String>,
    },
    SetItemOrder(Vec<String>),
    /// Deletes the file; a live item comes off the screens too.
    DeleteItem(String),
    RenameItem {
        id: String,
        name: String,
    },
    /// Shows the file in Finder; answers [`MediaAnswer::Done`].
    RevealItem(String),
    /// Answers [`MediaAnswer::Done`].
    CopyItems(Vec<String>),
    /// Answers [`MediaAnswer::Done`].
    CutItems {
        ids: Vec<String>,
        from_playlist: Option<String>,
    },
    PasteItems(Option<String>),
    /// Answers [`MediaAnswer::Flag`].
    ClipboardHasFiles,
    /// The Add backgrounds dialog.
    AddMediaToPlaylist(String),
    /// File → Import with the dialog.
    ImportFiles(ImportKind),
    /// Files dropped from the OS, copied into the folder (and a playlist).
    ImportPaths {
        paths: Vec<PathBuf>,
        playlist: Option<String>,
    },
    /// `media.setPlayback(id, patch, nextCue)`.
    SetPlayback {
        id: String,
        patch: Map<String, Value>,
        next_cue: bool,
    },
    /// The library again (after a failed write).
    GetLibrary,
    /// A video's poster frame (cached PNG); answers [`MediaAnswer::Poster`].
    Poster(String),
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum MediaAnswer {
    Done,
    Library(MediaLibrary),
    /// `{ applied, reason }`: `reason` is `backgroundTargetProblem` when
    /// nothing showed it.
    Pushed {
        applied: bool,
        reason: Option<String>,
    },
    Flag(bool),
    /// The poster image and the clip's duration.
    Poster {
        image: PathBuf,
        duration: Option<f64>,
    },
}

/// One decoded preview frame (RGBA, tightly packed).
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub seconds: f64,
}

/// A file opened for private preview playback, decoded in this process.
pub trait PreviewSource: Send {
    fn next_frame(&mut self) -> Result<Option<Frame>, String>;
    fn seek(&mut self, seconds: f64) -> Result<(), String>;
    fn duration(&self) -> Option<f64>;
    fn interval(&self) -> f64;
}

pub type Reply = Box<dyn FnOnce(Result<MediaAnswer, String>) + Send>;
pub type FeedSink = Arc<dyn Fn(MediaFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;

pub trait MediaBackend: Send + Sync + 'static {
    fn library(&self) -> MediaLibrary;
    fn listen(&self, sink: FeedSink) -> Listening;
    fn call(&self, call: MediaCall, reply: Reply);
    /// Opens `path` for preview, scaled to `max_width`. Called off the UI thread.
    fn open_preview(&self, path: &str, max_width: u32) -> Result<Box<dyn PreviewSource>, String>;
    /// Per-viewer UI state (`localStorage` in Electron).
    fn read_ui(&self, key: &str) -> Option<Value>;
    fn write_ui(&self, key: &str, value: Value);
}
