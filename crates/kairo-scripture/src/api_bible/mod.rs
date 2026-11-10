//! Licensed API.Bible access and its strict offline policy (prompt 034).
//!
//! - [`client`]: every API.Bible call, through the managed gateway.
//! - [`cache`]: the encrypted, 30-day cache.
//! - [`lookup`]: cache-first passage and chapter lookup.
//! - [`downloads`]: resumable whole-translation downloads.
//! - [`policy`]: freshness and the offline licence gate.
//! - [`source`]: authorization and the offline-cache IPC surface.
//! - [`detection`]: live detection lookup with translation fallback.

pub mod cache;
pub mod client;
pub mod detection;
pub mod downloads;
pub mod lookup;
pub mod parse;
pub mod policy;
pub mod source;

pub use cache::ApiBibleCache;
pub use client::{ApiBibleClient, BibleTransport, RequestError};
pub use downloads::ApiBibleDownloadManager;
pub use lookup::ApiBibleLookup;
pub use source::{ApiBibleSource, OfflineBibles};
