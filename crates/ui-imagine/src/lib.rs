//! The Imagine workspace: generate images with OpenAI, refine them into new
//! versions, compare and restore versions, and save one to Media only when
//! the operator asks. Nothing here goes live.

pub mod backend;
pub mod loader;
pub mod view;

pub use backend::{
    Attachment, FeedSink, ImagineAnswer, ImagineBackend, ImagineCall, ImagineFeed, ImagineSnapshot,
    Reply,
};
pub use view::{ImagineEvent, ImaginePage, TEMPLATES, Template, ids};
