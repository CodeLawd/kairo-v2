//! `detection-lookup.ts`: live detections resolve through the same local then
//! API.Bible fallback as manual search.

use crate::service::{ScriptureError, ScriptureService};

type Result<T> = std::result::Result<T, ScriptureError>;
use kairo_domain::js::number_to_string;
use kairo_domain::scripture::ScriptureVerse;
use std::future::Future;

/// A detected reference.
#[derive(Clone, Debug, PartialEq)]
pub struct DetectedReference {
    pub book: String,
    pub chapter: f64,
    pub verse_start: f64,
    pub verse_end: Option<f64>,
}

/// `lookupDetectedScripture`: searches `Book C:V[-E]` so API-only
/// translations resolve too.
pub async fn lookup_detected_scripture(
    service: &ScriptureService,
    reference: &DetectedReference,
    translation: &str,
) -> Result<Vec<ScriptureVerse>> {
    let end = reference
        .verse_end
        .map(|e| format!("-{}", number_to_string(e)))
        .unwrap_or_default();
    let query = format!(
        "{} {}:{}{end}",
        reference.book,
        number_to_string(reference.chapter),
        number_to_string(reference.verse_start)
    );
    Ok(service
        .search(&query, Some(translation))
        .await?
        .into_iter()
        .flat_map(|result| result.verses)
        .collect())
}

/// What [`lookup_with_translation_fallback`] found.
#[derive(Clone, Debug, PartialEq)]
pub struct FallbackLookup {
    pub verses: Vec<ScriptureVerse>,
    pub translation: String,
    /// The named translation that could not be shown, when the default was used.
    pub requested_translation: Option<String>,
}

/// When a translation the preacher named cannot be shown here, falls back to
/// the default so the operator still gets the verse, and reports the named
/// one so the card can say so.
pub async fn lookup_with_translation_fallback<F, Fut, E>(
    lookup: F,
    translation: &str,
    requested: Option<&str>,
    default_translation: &str,
) -> std::result::Result<FallbackLookup, E>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = std::result::Result<Vec<ScriptureVerse>, E>>,
{
    let verses = lookup(translation.to_owned()).await?;
    match requested {
        Some(requested)
            if verses.is_empty() && !requested.is_empty() && requested != default_translation =>
        {
            Ok(FallbackLookup {
                verses: lookup(default_translation.to_owned()).await?,
                translation: default_translation.to_owned(),
                requested_translation: Some(requested.to_owned()),
            })
        }
        _ => Ok(FallbackLookup {
            verses,
            translation: translation.to_owned(),
            requested_translation: None,
        }),
    }
}
