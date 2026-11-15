//! `.kairo` transfer UI (prompt 091): the import review, the song export
//! picker and the one-line result notice (`TransferHost.tsx`,
//! `ImportReviewModal.tsx`, `ExportSongsModal.tsx`, `useTransfer.ts`).

pub mod model;
mod view;

use std::sync::Arc;

use kairo_domain::kairo_bundle::{
    TransferCommitRequest, TransferCommitResult, TransferExportRequest, TransferExportResult,
    TransferPreview,
};

pub use model::{ExportPicker, ImportReview, Notice, SongRow};
pub use view::{TransferHost, ids};

/// What the application pushes to the host.
#[derive(Clone, Debug, PartialEq)]
pub enum TransferFeed {
    /// A file to review (`transfer.onOpened`, the Import menu, a page's
    /// "Import .kairo file…").
    Opened(Box<TransferPreview>),
    /// A result line: an export, or a file that could not be read.
    Notice(Notice),
    /// "Export songs…": the picker, these songs ticked.
    PickSongs(Vec<String>),
    /// The song library changed while the picker is open.
    Songs(Vec<SongRow>),
}

pub type FeedSink = Arc<dyn Fn(TransferFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;
pub type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send>;

/// `window.api.transfer.*` and the song library.
pub trait TransferBackend: Send + Sync + 'static {
    fn listen(&self, sink: FeedSink) -> Listening;
    /// The song library, for the picker.
    fn songs(&self, reply: Reply<Vec<SongRow>>);
    /// `transfer.export`: the save dialog, then the file.
    fn export(&self, request: TransferExportRequest, reply: Reply<TransferExportResult>);
    /// `transfer.commit`, then the pages refresh what it changed.
    fn commit(&self, request: TransferCommitRequest, reply: Reply<TransferCommitResult>);
    /// `transfer.discard`: forget the parsed file.
    fn discard(&self, token: String);
}
