//! Offline Bible lookup and search, from `src/main/services/scripture`.
//!
//! - [`service`]: `ScriptureService`, the search, lookup, pack and
//!   suggestion API the operator and orchestrator call.
//! - [`bible_db`]: queries over the profile's `bible.db`.
//! - [`reference`]: typed references and translation routing.
//! - [`pack`]: registry pack download, size and hash checks, status.
//! - [`installer`]: shared, session-long install progress.
//! - [`versification`] and [`verse_bounds`]: chapter and verse structure.
//! - [`api`]: what the service needs from API.Bible.
//! - [`quote`]: global quotation search on a worker thread.
//! - [`api_bible`]: the API.Bible client, encrypted cache, lookup and
//!   offline downloads.

pub mod api;
pub mod api_bible;
pub mod bible_db;
pub mod citation;
pub mod installer;
mod like_index;
pub mod live_detector;
pub mod model_detector;
pub mod pack;
pub mod quote;
pub mod quote_recovery;
pub mod reference;
pub mod scripture_signal;
pub mod sermon_notes;
pub mod service;
pub mod verse_bounds;
pub mod versification;

pub use service::{ScriptureError, ScriptureService, Subscription};
