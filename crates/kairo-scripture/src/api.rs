//! The API.Bible boundary the lookup service calls.
//!
//! Prompt 034 owns the client, the encrypted cache and authorization. This
//! trait is what `ScriptureService` needs from them, so lookups route exactly
//! as Electron's `search` does: with no source attached, the service behaves
//! as Electron does with an empty API key, never touching the network.

use kairo_domain::scripture::ScriptureResult;
use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An API.Bible failure, carrying the message Electron would throw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError(pub String);

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ApiError {}

/// One `ApiBibleClient.search` hit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApiSearchHit {
    pub reference: String,
    pub text: String,
    pub book_id: Option<String>,
    pub chapter: Option<f64>,
    pub verse: Option<f64>,
}

/// A passage request for `ApiBibleLookup.lookupPassage`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiPassage {
    pub bible_id: String,
    pub translation: String,
    pub book_id: u32,
    pub book_name: String,
    pub chapter: f64,
    pub verse_start: f64,
    pub verse_end: Option<f64>,
}

/// A chapter request for `ApiBibleLookup.lookupChapter`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiChapter {
    pub bible_id: String,
    pub translation: String,
    pub book_id: u32,
    pub book_name: String,
    pub chapter: f64,
}

pub trait ApiBible: Send + Sync {
    /// Catalogue ids the key authorizes (`loadApiBibleIds`), upper-cased.
    fn authorized_translations(&self) -> BoxFuture<'_, Result<BTreeSet<String>, ApiError>>;

    /// `resolveBibleId`: the API.Bible id for a catalogue translation, or the
    /// "not authorized for this API.Bible key" error.
    fn resolve_bible_id<'a>(
        &'a self,
        translation: &'a str,
    ) -> BoxFuture<'a, Result<String, ApiError>>;

    fn search<'a>(
        &'a self,
        bible_id: &'a str,
        query: &'a str,
        limit: u32,
    ) -> BoxFuture<'a, Result<Vec<ApiSearchHit>, ApiError>>;

    fn lookup_passage(
        &self,
        passage: ApiPassage,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, ApiError>>;

    fn lookup_chapter(
        &self,
        chapter: ApiChapter,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, ApiError>>;
}
