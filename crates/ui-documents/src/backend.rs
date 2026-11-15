//! What the Documents page reads and asks for. The application implements
//! [`DocumentsBackend`] over its documents service, slide converter and
//! actions; tests drive a scripted fake.
//!
//! Electron reached these through `window.api.documents`, `settings` and
//! `localStorage`. The page no longer renders PDFs itself: one
//! [`DocumentsBackend::import`] call prepares, renders (PDFium in the
//! service) and finishes, reporting progress as it goes.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use kairo_domain::collections::{LibrariesCommand, LibraryState};
use kairo_domain::documents::{
    DocumentFormat, DocumentNeedsConverter, DocumentPlaybackStatus, DocumentPrepareOptions,
    DocumentVideoCommand, DocumentsSettings, ProjectionDocument, SlideConverterStatus,
};
use kairo_domain::settings::ShortcutBindings;
use serde_json::Value;

/// Which picker File → Import opened (`useImportRequest(['pdf',
/// 'powerpoint'])`). The page's own button passes neither: one picker for
/// both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportKind {
    Pdf,
    Powerpoint,
}

impl ImportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportKind::Pdf => "pdf",
            ImportKind::Powerpoint => "powerpoint",
        }
    }
}

/// A push from the core.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentsFeed {
    /// The library, read off the UI thread at start.
    Documents(Vec<ProjectionDocument>),
    /// The documents libraries changed.
    Libraries(LibraryState),
    /// `settings.documents` changed elsewhere (Settings → Documents).
    Settings(DocumentsSettings),
    /// `display.shortcuts` changed.
    Shortcuts(ShortcutBindings),
    /// File → Import → PDF or PowerPoint.
    Import(ImportKind),
    /// The slide converter's status (`converter.onChanged`).
    Converter(SlideConverterStatus),
    /// The live output no longer shows a document page (cleared, or
    /// something else went live).
    LiveCleared,
}

/// Where an import has got to (`DocumentImportProgress`, less the error).
#[derive(Clone, Debug, PartialEq)]
pub enum ImportStep {
    /// The file was picked and is being read or converted.
    Preparing {
        name: String,
        format: Option<DocumentFormat>,
    },
    /// Pages rendered so far, of the total.
    Rendering { completed: usize, total: usize },
    /// Writing the document.
    Finishing,
}

/// How an import ended.
#[derive(Clone, Debug, PartialEq)]
pub enum ImportOutcome {
    /// The picker was closed without a file.
    NotPicked,
    /// Nothing converts this deck accurately: ask, then resume.
    NeedsConverter(DocumentNeedsConverter),
    /// The operator cancelled; the half-written import is gone.
    Cancelled,
    /// Imported; the library now.
    Imported {
        id: String,
        documents: Vec<ProjectionDocument>,
    },
}

/// One request.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentsCall {
    /// Answers [`DocumentsAnswer::Documents`].
    List,
    /// `documents.push`; answers [`DocumentsAnswer::Pushed`].
    Push { id: String, page: usize },
    /// Answers [`DocumentsAnswer::Documents`].
    Rename { id: String, name: String },
    /// Answers [`DocumentsAnswer::Documents`].
    Remove(String),
    /// Answers [`DocumentsAnswer::Playback`].
    Playback { id: String, page: usize },
    /// Answers [`DocumentsAnswer::Done`].
    Control {
        id: String,
        page: usize,
        video: usize,
        command: DocumentVideoCommand,
    },
    /// `settings.set('documents', …)`; answers [`DocumentsAnswer::Done`].
    SaveSettings(DocumentsSettings),
    /// Answers [`DocumentsAnswer::Libraries`].
    Libraries(LibrariesCommand),
    /// Answers [`DocumentsAnswer::Converter`].
    ConverterStatus,
    /// Starts the download; progress arrives as [`DocumentsFeed::Converter`].
    /// Answers [`DocumentsAnswer::Done`].
    ConverterInstall,
    /// Stops the download. Answers [`DocumentsAnswer::Done`].
    ConverterCancel,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DocumentsAnswer {
    Done,
    Documents(Vec<ProjectionDocument>),
    /// `applied` is false when no output showed it (preview only).
    Pushed {
        applied: bool,
    },
    Playback(Option<DocumentPlaybackStatus>),
    Libraries(LibraryState),
    Converter(SlideConverterStatus),
}

/// Everything the page starts from, read on the UI thread (cheap).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocumentsSnapshot {
    pub libraries: LibraryState,
    pub settings: DocumentsSettings,
    pub shortcuts: ShortcutBindings,
}

pub type Reply = Box<dyn FnOnce(Result<DocumentsAnswer, String>) + Send>;
pub type ImportReply = Box<dyn FnOnce(Result<ImportOutcome, String>) + Send>;
pub type ProgressSink = Arc<dyn Fn(ImportStep) + Send + Sync>;
pub type FeedSink = Arc<dyn Fn(DocumentsFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;

pub trait DocumentsBackend: Send + Sync + 'static {
    fn snapshot(&self) -> DocumentsSnapshot;
    fn listen(&self, sink: FeedSink) -> Listening;
    fn call(&self, call: DocumentsCall, reply: Reply);
    /// Picks a file (unless `options.resume` names one already picked),
    /// prepares, renders and finishes it. `cancel` is checked between pages.
    fn import(
        &self,
        kind: Option<ImportKind>,
        options: DocumentPrepareOptions,
        cancel: Arc<AtomicBool>,
        progress: ProgressSink,
        reply: ImportReply,
    );
    /// Per-viewer UI state (`localStorage` in Electron).
    fn read_ui(&self, key: &str) -> Option<Value>;
    fn write_ui(&self, key: &str, value: Value);
}
