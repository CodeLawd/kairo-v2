//! `api-bible-lookup.ts`: cache-first passage and chapter lookup. Cached text
//! is served only inside the 30-day window. Expired or revoked text is refused.

use super::cache::{ApiBibleCache, ApiBibleCachePassage, ApiBibleTranslationMeta, Coverage};
use super::client::{ApiBibleClient, ApiBiblePassage, RequestError};
use super::parse::USFM_BOOK_CODES;
use crate::api::{ApiChapter, ApiError, ApiPassage};
use kairo_domain::js::number_to_string;
use kairo_domain::scripture::offline_state::api_cache_messages as msg;
use kairo_domain::scripture::{ApiBibleCacheStatus, ScriptureResult, ScriptureVerse};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

/// Longest chapter in the canon (Psalm 119): caps whole-chapter probes.
pub const MAX_VERSES_PER_CHAPTER: i64 = 176;

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| kairo_store::settings::now_ms() as i64)
}

fn usfm(book_id: u32) -> &'static str {
    USFM_BOOK_CODES
        .get((book_id as usize).wrapping_sub(1))
        .copied()
        .unwrap_or("")
}

/// `buildPassageId`: `JHN.3.16` or `JHN.3.16-JHN.3.18`.
pub fn build_passage_id(
    book_id: u32,
    chapter: f64,
    verse_start: f64,
    verse_end: Option<f64>,
) -> String {
    let code = usfm(book_id);
    let start = format!(
        "{code}.{}.{}",
        number_to_string(chapter),
        number_to_string(verse_start)
    );
    match verse_end.filter(|e| *e != 0.0 && !e.is_nan()) {
        Some(end) => format!(
            "{start}-{code}.{}.{}",
            number_to_string(chapter),
            number_to_string(end)
        ),
        None => start,
    }
}

pub fn build_chapter_id(book_id: u32, chapter: f64) -> String {
    format!("{}.{}", usfm(book_id), number_to_string(chapter))
}

fn verses(passage: &ApiBiblePassage) -> Vec<ScriptureVerse> {
    passage
        .verses
        .iter()
        .map(|v| ScriptureVerse {
            book: v.book.clone(),
            chapter: v.chapter,
            verse: v.verse,
            text: v.text.clone(),
            extra: Default::default(),
        })
        .collect()
}

struct Inner {
    cache: Option<ApiBibleCache>,
    client: ApiBibleClient,
    clock: Clock,
    session_started_at: i64,
    revalidating: Mutex<HashMap<String, watch::Receiver<bool>>>,
}

#[derive(Clone)]
pub struct ApiBibleLookup(Arc<Inner>);

impl ApiBibleLookup {
    /// `cache` is `None` when OS encryption is unavailable: lookups still work
    /// online and nothing is cached.
    pub fn new(
        cache: Option<ApiBibleCache>,
        client: ApiBibleClient,
        clock: Clock,
        session_started_at: i64,
    ) -> Self {
        Self(Arc::new(Inner {
            cache,
            client,
            clock,
            session_started_at,
            revalidating: Mutex::new(HashMap::new()),
        }))
    }

    /// Resolves once background access revalidation has settled.
    pub async fn when_idle(&self) {
        let pending: Vec<_> = self.lock().values().cloned().collect();
        for mut rx in pending {
            let _ = rx.wait_for(|done| *done).await;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, watch::Receiver<bool>>> {
        self.0
            .revalidating
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    pub async fn lookup_passage(&self, r: ApiPassage) -> Result<Vec<ScriptureResult>, ApiError> {
        let now = (self.0.clock)();
        let (book, chapter, start) = (r.book_id as i64, r.chapter as i64, r.verse_start as i64);
        let end = r
            .verse_end
            .filter(|e| *e != 0.0 && !e.is_nan())
            .map(|e| e as i64);
        if let Some(cache) = &self.0.cache {
            if let Some(cached) = cache
                .get_range(&r.bible_id, book, chapter, start, end, now)
                .await?
            {
                // Confirm once per session that access still holds. Offline
                // keeps working; only an explicit refusal revokes the text.
                self.revalidate(&r.bible_id, now);
                return Ok(vec![passage_result(&r, cached)]);
            }
            self.refuse_unavailable(cache, &r.bible_id, now).await?;
        }
        let id = build_passage_id(r.book_id, r.chapter, r.verse_start, r.verse_end);
        let passage = match self
            .0
            .client
            .get_passage(&r.bible_id, &id, &r.book_name)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                return Err(self
                    .user_error(e, &r.bible_id, book, chapter, start, end)
                    .await);
            }
        };
        if passage.verses.is_empty() {
            return Ok(Vec::new());
        }
        let list = verses(&passage);
        let coverage = Coverage {
            chapter,
            verse_start: Some(start),
            verse_end: Some(end.unwrap_or(start)),
        };
        self.store(
            &r.bible_id,
            &r.translation,
            &r.book_name,
            book,
            &passage,
            &list,
            coverage,
            now,
        )
        .await?;
        Ok(vec![passage_result(&r, list)])
    }

    /// Whole chapter. Stored as fully covered, so later verse lookups inside
    /// it stay offline.
    pub async fn lookup_chapter(&self, r: ApiChapter) -> Result<Vec<ScriptureResult>, ApiError> {
        let now = (self.0.clock)();
        let (book, chapter) = (r.book_id as i64, r.chapter as i64);
        if let Some(cache) = &self.0.cache {
            let cached = cache
                .get_range(
                    &r.bible_id,
                    book,
                    chapter,
                    1,
                    Some(MAX_VERSES_PER_CHAPTER),
                    now,
                )
                .await?;
            if let Some(cached) = cached.filter(|v| !v.is_empty()) {
                self.revalidate(&r.bible_id, now);
                return Ok(vec![chapter_result(&r, cached)]);
            }
            self.refuse_unavailable(cache, &r.bible_id, now).await?;
        }
        let id = build_chapter_id(r.book_id, r.chapter);
        let passage = match self
            .0
            .client
            .get_chapter(&r.bible_id, &id, &r.book_name)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                return Err(self
                    .user_error(
                        e,
                        &r.bible_id,
                        book,
                        chapter,
                        1,
                        Some(MAX_VERSES_PER_CHAPTER),
                    )
                    .await);
            }
        };
        if passage.verses.is_empty() {
            return Ok(Vec::new());
        }
        let list = verses(&passage);
        let coverage = Coverage {
            chapter,
            verse_start: None,
            verse_end: None,
        };
        self.store(
            &r.bible_id,
            &r.translation,
            &r.book_name,
            book,
            &passage,
            &list,
            coverage,
            now,
        )
        .await?;
        Ok(vec![chapter_result(&r, list)])
    }

    async fn refuse_unavailable(
        &self,
        cache: &ApiBibleCache,
        bible_id: &str,
        now: i64,
    ) -> Result<(), ApiError> {
        let state = cache.translation_state(bible_id, now).await?;
        if state.is_some_and(|s| s.status == ApiBibleCacheStatus::Unavailable) {
            return Err(ApiError(msg::ACCESS_REVOKED.into()));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn store(
        &self,
        bible_id: &str,
        translation: &str,
        book_name: &str,
        book_id: i64,
        passage: &ApiBiblePassage,
        list: &[ScriptureVerse],
        coverage: Coverage,
        now: i64,
    ) -> Result<(), ApiError> {
        let Some(cache) = &self.0.cache else {
            return Ok(());
        };
        cache
            .upsert_translation(ApiBibleTranslationMeta {
                bible_id: bible_id.into(),
                translation: translation.into(),
                name: translation.into(),
                copyright: passage.copyright.clone(),
                total_chapters: None,
            })
            .await?;
        cache
            .put_passage(
                bible_id,
                ApiBibleCachePassage {
                    book_id,
                    book_name: book_name.into(),
                    verses: list.to_vec(),
                    fetched_at: now,
                    coverage: Some(coverage),
                },
            )
            .await?;
        cache.mark_access_checked(bible_id, now).await
    }

    fn revalidate(&self, bible_id: &str, now: i64) {
        let Some(cache) = self.0.cache.clone() else {
            return;
        };
        let (tx, rx) = watch::channel(false);
        {
            let mut map = self.lock();
            if map.contains_key(bible_id) {
                return;
            }
            map.insert(bible_id.to_owned(), rx);
        }
        let this = self.clone();
        let id = bible_id.to_owned();
        tokio::spawn(async move {
            let last = cache.last_access_check(&id).await.ok().flatten();
            if last.is_none_or(|at| at < this.0.session_started_at) {
                match this.0.client.get_bible(&id).await {
                    Ok(_) => {
                        let _ = cache.mark_access_checked(&id, now).await;
                    }
                    Err(e) if e.is_access_error() => {
                        let _ = cache.mark_unavailable(&id, msg::ACCESS_REVOKED).await;
                    }
                    // A network failure says nothing about entitlement.
                    Err(_) => {}
                }
            }
            this.lock().remove(&id);
            let _ = tx.send(true);
        });
    }

    async fn user_error(
        &self,
        error: RequestError,
        bible_id: &str,
        book: i64,
        chapter: i64,
        start: i64,
        end: Option<i64>,
    ) -> ApiError {
        let Some(cache) = &self.0.cache else {
            return ApiError(
                if error.is_access_error() {
                    msg::ACCESS_REVOKED
                } else {
                    msg::NOT_AVAILABLE_OFFLINE
                }
                .into(),
            );
        };
        if error.is_access_error() {
            if let Err(e) = cache.mark_unavailable(bible_id, msg::ACCESS_REVOKED).await {
                return e;
            }
            return ApiError(msg::ACCESS_REVOKED.into());
        }
        match cache.has_range(bible_id, book, chapter, start, end).await {
            // The text is on disk but past its 30-day licence window.
            Ok(true) => ApiError(msg::REFRESH_REQUIRED.into()),
            Ok(false) => ApiError(msg::NOT_AVAILABLE_OFFLINE.into()),
            Err(e) => e,
        }
    }
}

fn passage_result(r: &ApiPassage, verses: Vec<ScriptureVerse>) -> ScriptureResult {
    let end = match r.verse_end.filter(|e| *e != 0.0 && !e.is_nan()) {
        Some(e) => format!("–{}", number_to_string(e)),
        None => String::new(),
    };
    ScriptureResult {
        reference: format!(
            "{} {}:{}{end}",
            r.book_name,
            number_to_string(r.chapter),
            number_to_string(r.verse_start)
        ),
        translation: r.translation.clone(),
        verses,
    }
}

fn chapter_result(r: &ApiChapter, verses: Vec<ScriptureVerse>) -> ScriptureResult {
    ScriptureResult {
        reference: format!("{} {}", r.book_name, number_to_string(r.chapter)),
        translation: r.translation.clone(),
        verses,
    }
}
