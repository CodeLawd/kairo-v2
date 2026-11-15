//! The Documents workspace (prompt 085): import with progress and cancel,
//! the page grid, pushing, the unattended slideshow, clicker paging, the
//! live slide's embedded video transport, rename and remove, and the slide
//! converter offer.

pub mod backend;
pub mod model;
pub mod view;

pub use backend::{
    DocumentsAnswer, DocumentsBackend, DocumentsCall, DocumentsFeed, DocumentsSnapshot, ImportKind,
    ImportOutcome, ImportStep,
};
pub use view::{DocumentsEvent, DocumentsPage, ids};
