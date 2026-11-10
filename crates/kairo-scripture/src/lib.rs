//! Offline Bible lookup and search, from `src/main/services/scripture`.
//!
//! - [`service`]: `ScriptureService`, the search, lookup, pack and
//!   suggestion API the operator and orchestrator call.
//! - [`bible_db`]: queries over the profile's `bible.db`.
//! - [`reference`]: typed references and translation routing.
//! - [`pack`]: registry pack download, size and hash checks, status.
//! - [`installer`]: shared, session-long install progress.
//! - [`versification`] and [`verse_bounds`]: chapter and verse structure.
//! - [`api`]: what the service needs from API.Bible (prompt 034).

pub mod api;
pub mod bible_db;
pub mod installer;
mod like_index;
pub mod pack;
pub mod reference;
pub mod service;
pub mod verse_bounds;
pub mod versification;

pub use service::{ScriptureError, ScriptureService, Subscription};
