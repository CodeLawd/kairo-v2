//! `cache-quote-index.ts`: the worker's transient index of licensed text.
//! Plaintext lives only in an in-memory SQLite database, never on disk.

use super::matching::{GlobalQuoteCandidate, QuoteSource};
use super::retrieval::retrieve_global_quote;
use crate::api_bible::cache::CachedQuoteVerse;
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = "PRAGMA temp_store = MEMORY;
CREATE TABLE books (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE VIRTUAL TABLE verses_fts USING fts5(text, translation_id UNINDEXED,
  book_id UNINDEXED, chapter UNINDEXED, verse UNINDEXED);
CREATE TABLE quote_sources (row_id INTEGER PRIMARY KEY, bible_id TEXT NOT NULL,
  book_id INTEGER NOT NULL, book_name TEXT NOT NULL, chapter INTEGER NOT NULL,
  verse INTEGER NOT NULL, translation TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL);
CREATE INDEX quote_expiry ON quote_sources(expires_at);
CREATE INDEX quote_identity ON quote_sources(bible_id,book_name,chapter,verse);";

#[derive(Default)]
pub struct CacheQuoteIndex {
    db: Option<Connection>,
    version: i64,
    ready: bool,
    /// Earliest expiry held: until then a search has nothing to purge.
    next_expiry: Option<i64>,
}

impl CacheQuoteIndex {
    pub fn new() -> Self {
        Self {
            version: -1,
            ..Default::default()
        }
    }

    /// Clears the previous view at once. A partial index is never queried.
    pub fn begin(&mut self, version: i64) -> rusqlite::Result<()> {
        self.db = None;
        self.version = version;
        self.ready = false;
        self.next_expiry = None;
        let db = Connection::open_in_memory()?;
        db.execute_batch(SCHEMA)?;
        self.db = Some(db);
        Ok(())
    }

    pub fn append(&mut self, version: i64, rows: &[CachedQuoteVerse]) -> rusqlite::Result<()> {
        if version != self.version || self.ready {
            return Ok(());
        }
        let Some(db) = self.db.as_mut() else {
            return Ok(());
        };
        let tx = db.transaction()?;
        let mut earliest = self.next_expiry;
        for row in rows {
            let s = &row.cache_source;
            tx.prepare_cached("INSERT OR IGNORE INTO books (id,name) VALUES (?,?)")?
                .execute(params![s.book_id, row.book])?;
            tx.prepare_cached("INSERT INTO verses_fts (text,translation_id,book_id,chapter,verse) VALUES (?,?,?,?,?)")?
                .execute(params![row.text, s.bible_id, s.book_id, row.chapter, row.verse])?;
            let id = tx.last_insert_rowid();
            tx.prepare_cached(
                "INSERT INTO quote_sources (row_id,bible_id,book_id,book_name,chapter,verse,translation,fetched_at,expires_at)
                 VALUES (?,?,?,?,?,?,?,?,?)",
            )?
            .execute(params![id, s.bible_id, s.book_id, row.book, row.chapter, row.verse, row.translation, s.fetched_at, s.expires_at])?;
            earliest = Some(earliest.map_or(s.expires_at, |e| e.min(s.expires_at)));
        }
        tx.commit()?;
        self.next_expiry = earliest;
        Ok(())
    }

    pub fn commit(&mut self, version: i64) {
        if self.version == version {
            self.ready = true;
        }
    }

    /// Expiry is enforced here even before the coordinator's timer fires.
    pub fn search(&mut self, text: &str, now: i64) -> rusqlite::Result<Vec<GlobalQuoteCandidate>> {
        if !self.ready {
            return Ok(Vec::new());
        }
        let Some(db) = self.db.as_ref() else {
            return Ok(Vec::new());
        };
        if self.next_expiry.is_some_and(|at| now >= at) {
            db.execute(
                "DELETE FROM verses_fts WHERE rowid IN (SELECT row_id FROM quote_sources WHERE expires_at <= ?)",
                params![now],
            )?;
            db.execute(
                "DELETE FROM quote_sources WHERE expires_at <= ?",
                params![now],
            )?;
            self.next_expiry =
                db.query_row("SELECT MIN(expires_at) FROM quote_sources", [], |r| {
                    r.get(0)
                })?;
        }
        let mut out = Vec::new();
        for candidate in retrieve_global_quote(db, text)? {
            // The index stores the API.Bible id in `translation_id`.
            let row = db
                .prepare_cached(
                    "SELECT bible_id, book_id, chapter, verse, translation, fetched_at, expires_at FROM quote_sources
                      WHERE bible_id=? AND book_name=? AND chapter=? AND verse=? AND expires_at > ?",
                )?
                .query_row(
                    params![candidate.translation, candidate.book, candidate.chapter as i64, candidate.verse as i64, now],
                    |r| {
                        Ok((
                            r.get::<_, Option<String>>(4)?,
                            QuoteSource {
                                bible_id: r.get(0)?,
                                book_id: r.get(1)?,
                                chapter: r.get(2)?,
                                verse: r.get(3)?,
                                fetched_at: r.get(5)?,
                                expires_at: r.get(6)?,
                            },
                        ))
                    },
                )
                .optional()?;
            if let Some((translation, source)) = row {
                out.push(GlobalQuoteCandidate {
                    translation,
                    cache_source: Some(source),
                    ..candidate
                });
            }
        }
        Ok(out)
    }

    pub fn close(&mut self) {
        self.ready = false;
        self.db = None;
    }
}
