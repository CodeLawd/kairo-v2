//! `api-bible-cache.ts`: the encrypted, expiry-enforcing store for licensed
//! API.Bible text. Verse text is AES-256-GCM encrypted before it reaches
//! SQLite and is decrypted only after the whole range passes the 30-day check.

use super::policy::{API_CACHE_MAX_AGE_MS, is_fresh};
use crate::api::ApiError;
use kairo_domain::scripture::{ApiBibleCacheStatus, ApiBibleOfflineTranslation, ScriptureVerse};
use kairo_store::db::{Database, Schema};
use kairo_store::secrets::{ApiCacheCrypto, EncryptedValue};
use rusqlite::{OptionalExtension, Row, params};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// `Number.MAX_SAFE_INTEGER`, the open end of a whole-chapter coverage row.
const WHOLE_CHAPTER_END: i64 = 9_007_199_254_740_991;

pub type CacheResult<T> = Result<T, ApiError>;

fn db_error(error: impl std::fmt::Display) -> ApiError {
    ApiError(format!("API.Bible cache: {error}"))
}

/// A translation row without the licence flag (`ApiBibleCacheState`).
#[derive(Clone, Debug, PartialEq)]
pub struct ApiBibleCacheState {
    pub bible_id: String,
    pub translation: String,
    pub name: String,
    pub copyright: String,
    pub status: ApiBibleCacheStatus,
    pub cached_chapters: i64,
    pub total_chapters: i64,
    pub cached_verses: i64,
    pub fetched_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub error: Option<String>,
}

impl ApiBibleCacheState {
    pub fn with_licence(self, offline_download_enabled: bool) -> ApiBibleOfflineTranslation {
        ApiBibleOfflineTranslation {
            bible_id: self.bible_id,
            translation: self.translation,
            name: self.name,
            copyright: self.copyright,
            status: self.status,
            cached_chapters: self.cached_chapters as f64,
            total_chapters: self.total_chapters as f64,
            cached_verses: self.cached_verses as f64,
            fetched_at: self.fetched_at.map(|t| t as f64),
            expires_at: self.expires_at.map(|t| t as f64),
            offline_download_enabled,
            error: self.error,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApiBibleTranslationMeta {
    pub bible_id: String,
    pub translation: String,
    pub name: String,
    pub copyright: String,
    pub total_chapters: Option<i64>,
}

/// What the API was asked for. `None` bounds mean the whole chapter.
#[derive(Clone, Copy, Debug)]
pub struct Coverage {
    pub chapter: i64,
    pub verse_start: Option<i64>,
    pub verse_end: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct ApiBibleCachePassage {
    pub book_id: i64,
    pub book_name: String,
    pub verses: Vec<ScriptureVerse>,
    pub fetched_at: i64,
    pub coverage: Option<Coverage>,
}

/// Where a cached quote came from, rechecked before it is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheQuoteSource {
    pub bible_id: String,
    pub book_id: i64,
    pub chapter: i64,
    pub verse: i64,
    pub fetched_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CachedQuoteVerse {
    pub book: String,
    pub chapter: i64,
    pub verse: i64,
    pub translation: String,
    pub text: String,
    pub cache_source: CacheQuoteSource,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CachedQuoteBatch {
    pub rows: Vec<CachedQuoteVerse>,
    pub next_cursor: i64,
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteCorpusChange {
    Added,
    Removed,
}

type Listener = Arc<dyn Fn(QuoteCorpusChange) + Send + Sync>;

struct TranslationRow {
    bible_id: String,
    translation_id: String,
    name: String,
    copyright: String,
    status: String,
    total_chapters: i64,
    last_error: Option<String>,
}

fn translation_row(row: &Row<'_>) -> rusqlite::Result<TranslationRow> {
    Ok(TranslationRow {
        bible_id: row.get("bible_id")?,
        translation_id: row.get("translation_id")?,
        name: row.get("name")?,
        copyright: row.get("copyright")?,
        status: row.get("status")?,
        total_chapters: row.get("total_chapters")?,
        last_error: row.get("last_error")?,
    })
}

struct VerseRow {
    verse: i64,
    book_name: String,
    value: EncryptedValue,
    fetched_at: i64,
}

fn verse_row(row: &Row<'_>) -> rusqlite::Result<VerseRow> {
    Ok(VerseRow {
        verse: row.get("verse")?,
        book_name: row.get("book_name")?,
        value: EncryptedValue {
            ciphertext: row.get("ciphertext")?,
            iv: row.get("iv")?,
            auth_tag: row.get("auth_tag")?,
        },
        fetched_at: row.get("fetched_at")?,
    })
}

fn status_of(value: &str) -> ApiBibleCacheStatus {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .unwrap_or(ApiBibleCacheStatus::Partial)
}

fn status_text(status: ApiBibleCacheStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

struct Inner {
    db: Database,
    crypto: ApiCacheCrypto,
    listeners: Mutex<Vec<Listener>>,
    closed: AtomicBool,
}

/// Cloneable handle. All SQLite work runs on the database's own threads.
#[derive(Clone)]
pub struct ApiBibleCache(Arc<Inner>);

impl ApiBibleCache {
    /// `api-bible-cache.db` under the profile. Open the crypto first: it may
    /// discard a database whose key was lost.
    pub async fn open(path: &Path, crypto: ApiCacheCrypto) -> CacheResult<Self> {
        let db = Database::open(path, Schema::ApiBibleCache)
            .await
            .map_err(db_error)?;
        db.write(|conn| conn.execute_batch("PRAGMA foreign_keys = ON;"))
            .await
            .map_err(db_error)?;
        Ok(Self(Arc::new(Inner {
            db,
            crypto,
            listeners: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
        })))
    }

    /// Stop offering cached quotes; later reads see an empty corpus.
    pub fn close(&self) {
        self.0.closed.store(true, Ordering::SeqCst);
        self.notify(QuoteCorpusChange::Removed);
        self.listeners().clear();
    }

    fn listeners(&self) -> std::sync::MutexGuard<'_, Vec<Listener>> {
        self.0.listeners.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `Added` text only extends recognition. `Removed` text must stop being
    /// offered at once.
    pub fn on_quote_corpus_change(
        &self,
        listener: impl Fn(QuoteCorpusChange) + Send + Sync + 'static,
    ) {
        self.listeners().push(Arc::new(listener));
    }

    fn notify(&self, change: QuoteCorpusChange) {
        let listeners = self.listeners().clone();
        for listener in listeners {
            // Recognition cannot interrupt cache writes or revocation.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(change)));
        }
    }

    fn decrypt(&self, value: &EncryptedValue) -> CacheResult<String> {
        // Fail closed: a corrupt or foreign row is an error, never shown.
        self.0
            .crypto
            .decrypt(value)
            .map_err(|_| ApiError("API.Bible cache entry failed authentication.".into()))
    }

    /// Bounded batch of fresh, available verses for quote recognition.
    pub async fn read_quote_batch(
        &self,
        after: i64,
        limit: i64,
        now: i64,
    ) -> CacheResult<CachedQuoteBatch> {
        if self.0.closed.load(Ordering::SeqCst) {
            return Ok(CachedQuoteBatch {
                rows: Vec::new(),
                next_cursor: after,
                complete: true,
            });
        }
        let limit = limit.clamp(1, 128);
        let rows = self
            .0
            .db
            .read(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT v.rowid AS cursor, v.*, t.translation_id
                       FROM api_verses v JOIN api_translations t ON t.bible_id=v.bible_id
                      WHERE v.rowid > ? AND t.status <> 'unavailable' AND v.fetched_at > ?
                      ORDER BY v.rowid LIMIT ?",
                )?;
                stmt.query_map(params![after, now - API_CACHE_MAX_AGE_MS, limit], |row| {
                    Ok((
                        row.get::<_, i64>("cursor")?,
                        row.get::<_, String>("bible_id")?,
                        row.get::<_, i64>("book_id")?,
                        row.get::<_, i64>("chapter")?,
                        row.get::<_, String>("translation_id")?,
                        verse_row(row)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .map_err(db_error)?;
        let complete = (rows.len() as i64) < limit;
        let next_cursor = rows.last().map_or(after, |r| r.0);
        let mut out = Vec::with_capacity(rows.len());
        for (_, bible_id, book_id, chapter, translation, v) in rows {
            out.push(CachedQuoteVerse {
                book: v.book_name,
                chapter,
                verse: v.verse,
                translation,
                text: self.decrypt(&v.value)?,
                cache_source: CacheQuoteSource {
                    bible_id,
                    book_id,
                    chapter,
                    verse: v.verse,
                    fetched_at: v.fetched_at,
                    expires_at: v.fetched_at + API_CACHE_MAX_AGE_MS,
                },
            });
        }
        Ok(CachedQuoteBatch {
            rows: out,
            next_cursor,
            complete,
        })
    }

    /// Rechecked after search and output lookup, without decrypting text.
    pub async fn is_quote_source_eligible(&self, source: &CacheQuoteSource, now: i64) -> bool {
        if self.0.closed.load(Ordering::SeqCst) || source.expires_at <= now {
            return false;
        }
        let s = source.clone();
        let fetched = self
            .0
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT v.fetched_at FROM api_verses v
                       JOIN api_translations t ON t.bible_id=v.bible_id
                      WHERE v.bible_id=? AND v.book_id=? AND v.chapter=? AND v.verse=? AND t.status <> 'unavailable'",
                    params![s.bible_id, s.book_id, s.chapter, s.verse],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
            })
            .await
            .ok()
            .flatten();
        fetched.is_some_and(|at| at == source.fetched_at && is_fresh(at, now))
    }

    // ─── Translation metadata ───────────────────────────────────────────────

    pub async fn upsert_translation(&self, meta: ApiBibleTranslationMeta) -> CacheResult<()> {
        self.0
            .db
            .write(move |conn| {
                conn.execute(
                    "INSERT INTO api_translations (bible_id, translation_id, name, copyright, status, total_chapters)
                     VALUES (?1, ?2, ?3, ?4, 'partial', ?5)
                     ON CONFLICT(bible_id) DO UPDATE SET
                       translation_id = excluded.translation_id,
                       name = excluded.name,
                       copyright = excluded.copyright,
                       total_chapters = MAX(api_translations.total_chapters, excluded.total_chapters),
                       status = CASE WHEN api_translations.status IN ('unavailable', 'failed')
                                     THEN 'partial' ELSE api_translations.status END,
                       last_error = NULL",
                    params![
                        meta.bible_id,
                        meta.translation,
                        meta.name,
                        meta.copyright,
                        meta.total_chapters.unwrap_or(0)
                    ],
                )
            })
            .await
            .map_err(db_error)?;
        self.notify(QuoteCorpusChange::Added);
        Ok(())
    }

    pub async fn set_total_chapters(&self, bible_id: &str, total: i64) -> CacheResult<()> {
        let id = bible_id.to_owned();
        self.0
            .db
            .write(move |conn| {
                conn.execute(
                    "UPDATE api_translations SET total_chapters = ? WHERE bible_id = ?",
                    params![total, id],
                )
            })
            .await
            .map_err(db_error)?;
        Ok(())
    }

    pub async fn set_status(
        &self,
        bible_id: &str,
        status: ApiBibleCacheStatus,
        error: Option<String>,
    ) -> CacheResult<()> {
        let id = bible_id.to_owned();
        let text = status_text(status);
        self.0
            .db
            .write(move |conn| {
                conn.execute(
                    "UPDATE api_translations SET status = ?, last_error = ? WHERE bible_id = ?",
                    params![text, error, id],
                )
            })
            .await
            .map_err(db_error)?;
        self.notify(if status == ApiBibleCacheStatus::Unavailable {
            QuoteCorpusChange::Removed
        } else {
            QuoteCorpusChange::Added
        });
        Ok(())
    }

    /// Access was revoked: purge the licensed text at once and keep only the
    /// metadata row, so the operator can see why the translation went away.
    pub async fn mark_unavailable(&self, bible_id: &str, error: &str) -> CacheResult<()> {
        let id = bible_id.to_owned();
        let error = error.to_owned();
        self.0
            .db
            .write(move |conn| {
                let tx = conn.transaction()?;
                purge_content(&tx, &id)?;
                tx.execute(
                    "UPDATE api_translations SET status = 'unavailable', last_error = ?, total_chapters = 0 WHERE bible_id = ?",
                    params![error, id],
                )?;
                tx.commit()
            })
            .await
            .map_err(db_error)?;
        self.notify(QuoteCorpusChange::Removed);
        Ok(())
    }

    pub async fn mark_access_checked(&self, bible_id: &str, at: i64) -> CacheResult<()> {
        let id = bible_id.to_owned();
        self.0
            .db
            .write(move |conn| {
                conn.execute(
                    "UPDATE api_translations SET last_access_check = ? WHERE bible_id = ?",
                    params![at, id],
                )
            })
            .await
            .map_err(db_error)?;
        Ok(())
    }

    pub async fn last_access_check(&self, bible_id: &str) -> CacheResult<Option<i64>> {
        let id = bible_id.to_owned();
        self.0
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT last_access_check FROM api_translations WHERE bible_id = ?",
                    params![id],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .optional()
                .map(Option::flatten)
            })
            .await
            .map_err(db_error)
    }

    // ─── Reads ──────────────────────────────────────────────────────────────

    /// Every verse of the range, or `None` when the cache cannot fully and
    /// freshly satisfy it.
    pub async fn get_range(
        &self,
        bible_id: &str,
        book_id: i64,
        chapter: i64,
        verse_start: i64,
        verse_end: Option<i64>,
        now: i64,
    ) -> CacheResult<Option<Vec<ScriptureVerse>>> {
        let Some(translation) = self.translation_row(bible_id).await? else {
            return Ok(None);
        };
        if translation.status == "unavailable" {
            return Ok(None);
        }
        let end = verse_end.unwrap_or(verse_start);
        if end < verse_start {
            return Ok(None);
        }
        let id = bible_id.to_owned();
        let rows = self
            .0
            .db
            .read(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT verse, book_name, ciphertext, iv, auth_tag, fetched_at FROM api_verses
                      WHERE bible_id = ? AND book_id = ? AND chapter = ? AND verse BETWEEN ? AND ?
                      ORDER BY verse ASC",
                )?;
                stmt.query_map(params![id, book_id, chapter, verse_start, end], verse_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .map_err(db_error)?;
        if rows.is_empty() || rows.iter().any(|r| !is_fresh(r.fetched_at, now)) {
            return Ok(None);
        }
        // Complete when every integer verse is present, or a recorded fetch
        // covered the span: translations legitimately omit verse numbers.
        let contiguous = rows.len() as i64 == end - verse_start + 1
            && rows
                .iter()
                .enumerate()
                .all(|(i, r)| r.verse == verse_start + i as i64);
        if !contiguous
            && !self
                .is_range_covered(bible_id, book_id, chapter, verse_start, end, Some(now))
                .await?
        {
            return Ok(None);
        }
        let mut verses = Vec::with_capacity(rows.len());
        for row in rows {
            verses.push(ScriptureVerse {
                book: row.book_name,
                chapter: chapter as f64,
                verse: row.verse as f64,
                text: self.decrypt(&row.value)?,
                extra: Default::default(),
            });
        }
        Ok(Some(verses))
    }

    /// True when the range is on disk, fresh or not: explains expiry.
    pub async fn has_range(
        &self,
        bible_id: &str,
        book_id: i64,
        chapter: i64,
        verse_start: i64,
        verse_end: Option<i64>,
    ) -> CacheResult<bool> {
        let end = verse_end.unwrap_or(verse_start);
        if end < verse_start {
            return Ok(false);
        }
        let id = bible_id.to_owned();
        let n: i64 = self
            .0
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM api_verses
                      WHERE bible_id = ? AND book_id = ? AND chapter = ? AND verse BETWEEN ? AND ?",
                    params![id, book_id, chapter, verse_start, end],
                    |row| row.get(0),
                )
            })
            .await
            .map_err(db_error)?;
        if n == end - verse_start + 1 {
            return Ok(true);
        }
        Ok(n > 0
            && self
                .is_range_covered(bible_id, book_id, chapter, verse_start, end, None)
                .await?)
    }

    async fn is_range_covered(
        &self,
        bible_id: &str,
        book_id: i64,
        chapter: i64,
        verse_start: i64,
        verse_end: i64,
        now: Option<i64>,
    ) -> CacheResult<bool> {
        let id = bible_id.to_owned();
        let times: Vec<i64> = self
            .0
            .db
            .read(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT fetched_at FROM api_ranges
                      WHERE bible_id = ? AND book_id = ? AND chapter = ?
                        AND verse_start <= ? AND verse_end >= ?",
                )?;
                stmt.query_map(params![id, book_id, chapter, verse_start, verse_end], |r| {
                    r.get(0)
                })?
                .collect()
            })
            .await
            .map_err(db_error)?;
        Ok(times
            .iter()
            .any(|&at| now.is_none_or(|now| is_fresh(at, now))))
    }

    pub async fn translation_state(
        &self,
        bible_id: &str,
        now: i64,
    ) -> CacheResult<Option<ApiBibleCacheState>> {
        match self.translation_row(bible_id).await? {
            Some(row) => Ok(Some(self.to_state(row, now).await?)),
            None => Ok(None),
        }
    }

    pub async fn list_translation_states(&self, now: i64) -> CacheResult<Vec<ApiBibleCacheState>> {
        let rows = self
            .0
            .db
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT * FROM api_translations ORDER BY name ASC")?;
                stmt.query_map([], translation_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .map_err(db_error)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.to_state(row, now).await?);
        }
        Ok(out)
    }

    /// Chapter ids still to download, in the order supplied.
    pub async fn incomplete_chapters(
        &self,
        bible_id: &str,
        chapter_ids: Vec<String>,
        now: i64,
    ) -> CacheResult<Vec<String>> {
        let id = bible_id.to_owned();
        let complete: Vec<(String, Option<i64>)> = self
            .0
            .db
            .read(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT chapter_id, fetched_at FROM api_chapters WHERE bible_id = ? AND status = 'complete'",
                )?;
                stmt.query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .await
            .map_err(db_error)?;
        let done: std::collections::HashSet<String> = complete
            .into_iter()
            .filter(|(_, at)| at.is_some_and(|at| is_fresh(at, now)))
            .map(|(id, _)| id)
            .collect();
        Ok(chapter_ids
            .into_iter()
            .filter(|id| !done.contains(id))
            .collect())
    }

    // ─── Writes ─────────────────────────────────────────────────────────────

    fn encrypt_verses(
        &self,
        passage: &ApiBibleCachePassage,
    ) -> CacheResult<Vec<(i64, i64, EncryptedValue)>> {
        passage
            .verses
            .iter()
            .map(|v| {
                self.0
                    .crypto
                    .encrypt(&v.text)
                    .map(|value| (v.chapter as i64, v.verse as i64, value))
                    .map_err(|_| ApiError("Could not encrypt API.Bible text.".into()))
            })
            .collect()
    }

    /// Stores every verse of a passage in one transaction.
    pub async fn put_passage(
        &self,
        bible_id: &str,
        passage: ApiBibleCachePassage,
    ) -> CacheResult<()> {
        let encrypted = self.encrypt_verses(&passage)?;
        let id = bible_id.to_owned();
        self.0
            .db
            .write(move |conn| {
                let tx = conn.transaction()?;
                insert_passage(&tx, &id, &passage, &encrypted)?;
                tx.commit()
            })
            .await
            .map_err(db_error)?;
        self.notify(QuoteCorpusChange::Added);
        Ok(())
    }

    /// Replaces a whole chapter and its completion marker atomically, so a
    /// verse removed upstream cannot survive a refresh.
    pub async fn put_chapter(
        &self,
        bible_id: &str,
        chapter_id: &str,
        mut passage: ApiBibleCachePassage,
    ) -> CacheResult<()> {
        let chapter = passage
            .coverage
            .map(|c| c.chapter)
            .or_else(|| passage.verses.first().map(|v| v.chapter as i64));
        passage.coverage = Some(Coverage {
            chapter: chapter.unwrap_or(0),
            verse_start: None,
            verse_end: None,
        });
        let encrypted = self.encrypt_verses(&passage)?;
        let id = bible_id.to_owned();
        let chapter_id = chapter_id.to_owned();
        self.0
            .db
            .write(move |conn| {
                let tx = conn.transaction()?;
                if let Some(chapter) = chapter {
                    tx.execute(
                        "DELETE FROM api_verses WHERE bible_id = ? AND book_id = ? AND chapter = ?",
                        params![id, passage.book_id, chapter],
                    )?;
                    tx.execute(
                        "DELETE FROM api_ranges WHERE bible_id = ? AND book_id = ? AND chapter = ?",
                        params![id, passage.book_id, chapter],
                    )?;
                }
                insert_passage(&tx, &id, &passage, &encrypted)?;
                mark_chapter_complete(&tx, &id, &chapter_id, passage.fetched_at)?;
                tx.commit()
            })
            .await
            .map_err(db_error)?;
        self.notify(QuoteCorpusChange::Added);
        Ok(())
    }

    pub async fn remove_translation(&self, bible_id: &str) -> CacheResult<()> {
        let id = bible_id.to_owned();
        self.0
            .db
            .write(move |conn| {
                let tx = conn.transaction()?;
                purge_content(&tx, &id)?;
                tx.execute(
                    "DELETE FROM api_translations WHERE bible_id = ?",
                    params![id],
                )?;
                tx.commit()
            })
            .await
            .map_err(db_error)?;
        self.notify(QuoteCorpusChange::Removed);
        Ok(())
    }

    /// Row census per table, to prove removal leaves nothing behind.
    pub async fn count_rows(&self, bible_id: &str) -> CacheResult<[i64; 4]> {
        let id = bible_id.to_owned();
        self.0
            .db
            .read(move |conn| {
                let mut out = [0; 4];
                for (i, table) in [
                    "api_translations",
                    "api_verses",
                    "api_ranges",
                    "api_chapters",
                ]
                .iter()
                .enumerate()
                {
                    out[i] = conn.query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE bible_id = ?"),
                        params![id],
                        |r| r.get(0),
                    )?;
                }
                Ok(out)
            })
            .await
            .map_err(db_error)
    }

    // ─── Internals ──────────────────────────────────────────────────────────

    async fn translation_row(&self, bible_id: &str) -> CacheResult<Option<TranslationRow>> {
        let id = bible_id.to_owned();
        self.0
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT * FROM api_translations WHERE bible_id = ?",
                    params![id],
                    translation_row,
                )
                .optional()
            })
            .await
            .map_err(db_error)
    }

    async fn to_state(&self, row: TranslationRow, now: i64) -> CacheResult<ApiBibleCacheState> {
        let id = row.bible_id.clone();
        let (verses, oldest, chapters): (i64, Option<i64>, i64) = self
            .0
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT
                       (SELECT COUNT(*) FROM api_verses WHERE bible_id = ?1),
                       (SELECT MIN(fetched_at) FROM api_verses WHERE bible_id = ?1),
                       (SELECT COUNT(*) FROM api_chapters WHERE bible_id = ?1 AND status = 'complete')",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
            })
            .await
            .map_err(db_error)?;
        let stored = status_of(&row.status);
        use ApiBibleCacheStatus as S;
        let status = if matches!(stored, S::Unavailable | S::Failed) {
            stored
        } else if oldest.is_some_and(|at| !is_fresh(at, now)) {
            S::Stale
        } else if matches!(stored, S::Downloading | S::Paused) {
            stored
        } else if verses == 0 {
            S::NotDownloaded
        } else if row.total_chapters > 0 && chapters >= row.total_chapters {
            S::Downloaded
        } else {
            S::Partial
        };
        Ok(ApiBibleCacheState {
            bible_id: row.bible_id,
            translation: row.translation_id,
            name: row.name,
            copyright: row.copyright,
            status,
            cached_chapters: chapters,
            total_chapters: row.total_chapters,
            cached_verses: verses,
            fetched_at: oldest,
            expires_at: oldest.map(|at| at + API_CACHE_MAX_AGE_MS),
            error: row.last_error.filter(|e| !e.is_empty()),
        })
    }
}

fn purge_content(tx: &rusqlite::Transaction<'_>, bible_id: &str) -> rusqlite::Result<()> {
    for table in ["api_verses", "api_ranges", "api_chapters"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE bible_id = ?"),
            params![bible_id],
        )?;
    }
    Ok(())
}

fn insert_passage(
    tx: &rusqlite::Transaction<'_>,
    bible_id: &str,
    passage: &ApiBibleCachePassage,
    encrypted: &[(i64, i64, EncryptedValue)],
) -> rusqlite::Result<()> {
    if let Some(c) = passage.coverage {
        tx.execute(
            "INSERT INTO api_ranges (bible_id, book_id, chapter, verse_start, verse_end, fetched_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(bible_id, book_id, chapter, verse_start, verse_end)
               DO UPDATE SET fetched_at = excluded.fetched_at",
            params![
                bible_id,
                passage.book_id,
                c.chapter,
                c.verse_start.unwrap_or(0),
                c.verse_end.unwrap_or(WHOLE_CHAPTER_END),
                passage.fetched_at
            ],
        )?;
    }
    let mut insert = tx.prepare(
        "INSERT INTO api_verses
           (bible_id, book_id, book_name, chapter, verse, ciphertext, iv, auth_tag, fetched_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(bible_id, book_id, chapter, verse) DO UPDATE SET
           book_name = excluded.book_name, ciphertext = excluded.ciphertext,
           iv = excluded.iv, auth_tag = excluded.auth_tag, fetched_at = excluded.fetched_at",
    )?;
    for (chapter, verse, value) in encrypted {
        insert.execute(params![
            bible_id,
            passage.book_id,
            passage.book_name,
            chapter,
            verse,
            value.ciphertext,
            value.iv,
            value.auth_tag,
            passage.fetched_at
        ])?;
    }
    Ok(())
}

fn mark_chapter_complete(
    tx: &rusqlite::Transaction<'_>,
    bible_id: &str,
    chapter_id: &str,
    fetched_at: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO api_chapters (bible_id, chapter_id, status, fetched_at)
         VALUES (?, ?, 'complete', ?)
         ON CONFLICT(bible_id, chapter_id) DO UPDATE SET status = 'complete', fetched_at = excluded.fetched_at",
        params![bible_id, chapter_id, fetched_at],
    )?;
    Ok(())
}
