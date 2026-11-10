//! Shared fixtures, from `local-bible-pack-fixture.ts`. Every verse is
//! generated placeholder text in the canonical shape (66 books, 1,189
//! chapters, 31,102 verses); none is Bible wording.
#![allow(dead_code)]

use kairo_domain::scripture::ScriptureResult;
use kairo_scripture::ScriptureService;
use kairo_scripture::api::{ApiBible, ApiChapter, ApiError, ApiPassage, ApiSearchHit, BoxFuture};
use kairo_scripture::bible_db::BibleDb;
use kairo_scripture::pack::{DownloadError, PackBody, PackFetcher};
use kairo_store::db::{BIBLE_VERSE_COUNTS, PackVerse};
use rusqlite::{Connection, params};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use futures_lite::future::block_on;

pub const FIXTURE_CHAPTERS: f64 = 1189.0;
pub const FIXTURE_VERSES: f64 = 31102.0;
/// Distinctive token embedded in one verse for phrase-search tests.
pub const FIXTURE_MARKER: &str = "zephyrquill";

pub fn verse_text(book: u32, chapter: u32, verse: u32) -> String {
    format!(
        "Synthetic fixture text for book {book} chapter {chapter} verse {verse}, written for automated testing only."
    )
}

/// Flat canonical rows in book, chapter, verse order.
pub fn fixture_verses() -> Vec<PackVerse> {
    let mut rows = Vec::new();
    for (book, counts) in BIBLE_VERSE_COUNTS.iter().enumerate() {
        for (chapter, &last) in counts.iter().enumerate() {
            let (book_id, chapter) = (book as u32 + 1, chapter as u32 + 1);
            for verse in 1..=last {
                rows.push(PackVerse {
                    book_id,
                    chapter,
                    verse,
                    text: verse_text(book_id, chapter, verse),
                });
            }
        }
    }
    rows
}

/// Fixture rows with John 3:16 marked for phrase search.
pub fn marked_fixture_verses() -> Vec<PackVerse> {
    let mut rows = fixture_verses();
    mark(
        &mut rows,
        43,
        3,
        16,
        &format!("{FIXTURE_MARKER} covenant of testing"),
    );
    rows
}

pub fn mark(rows: &mut [PackVerse], book: u32, chapter: u32, verse: u32, extra: &str) {
    let row = rows
        .iter_mut()
        .find(|r| (r.book_id, r.chapter, r.verse) == (book, chapter, verse))
        .expect("fixture verse");
    row.text = format!("{} {extra}", row.text);
}

/// AMPC numbering: Mark 9:44 and 9:46 omitted, 3 John 1:15 present.
pub fn ampc_verses() -> Vec<PackVerse> {
    let mut rows: Vec<_> = fixture_verses()
        .into_iter()
        .filter(|r| !(r.book_id == 41 && r.chapter == 9 && (r.verse == 44 || r.verse == 46)))
        .collect();
    let at = rows
        .iter()
        .position(|r| (r.book_id, r.chapter, r.verse) == (64, 1, 14))
        .unwrap();
    rows.insert(
        at + 1,
        PackVerse {
            book_id: 64,
            chapter: 1,
            verse: 15,
            text: "Synthetic AMPC final greeting zephyrquill.".into(),
        },
    );
    rows
}

/// A single-translation pack file in the production schema, with FTS.
pub fn write_pack(path: &Path, id: &str, name: &str, rows: &[PackVerse]) -> PathBuf {
    write_pack_with(path, &[(id, name)], id, rows, &[])
}

/// A pack holding `translations`, with `rows` under `rows_id` and `extra`
/// rows `(translation, book, chapter, verse, text)`.
pub fn write_pack_with(
    path: &Path,
    translations: &[(&str, &str)],
    rows_id: &str,
    rows: &[PackVerse],
    extra: &[(&str, u32, u32, u32, &str)],
) -> PathBuf {
    let mut conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE translations (id TEXT PRIMARY KEY, name TEXT NOT NULL, language TEXT NOT NULL DEFAULT 'en', is_default INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE verses (translation_id TEXT NOT NULL, book_id INTEGER NOT NULL, chapter INTEGER NOT NULL, verse INTEGER NOT NULL, text TEXT NOT NULL, PRIMARY KEY (translation_id, book_id, chapter, verse));
         CREATE VIRTUAL TABLE verses_fts USING fts5(text, translation_id UNINDEXED, book_id UNINDEXED, chapter UNINDEXED, verse UNINDEXED);",
    )
    .unwrap();
    let tx = conn.transaction().unwrap();
    for (id, name) in translations {
        tx.execute(
            "INSERT INTO translations (id, name, language, is_default) VALUES (?1, ?2, 'en', 0)",
            params![id, name],
        )
        .unwrap();
    }
    {
        let mut verse = tx
            .prepare("INSERT INTO verses VALUES (?1, ?2, ?3, ?4, ?5)")
            .unwrap();
        let mut fts = tx
            .prepare("INSERT INTO verses_fts VALUES (?1, ?2, ?3, ?4, ?5)")
            .unwrap();
        for r in rows {
            verse
                .execute(params![rows_id, r.book_id, r.chapter, r.verse, r.text])
                .unwrap();
            fts.execute(params![r.text, rows_id, r.book_id, r.chapter, r.verse])
                .unwrap();
        }
        for (id, book, chapter, v, text) in extra {
            verse.execute(params![id, book, chapter, v, text]).unwrap();
        }
    }
    tx.commit().unwrap();
    path.to_owned()
}

/// A fresh profile-style `bible.db` with a two-verse KJV stand-in as default.
pub fn open_dest_with_kjv(dir: &Path) -> BibleDb {
    block_on(async {
        let db = BibleDb::open(&dir.join("bible.db")).await.unwrap();
        db.import_translation(
            "KJV",
            "King James Version",
            "en",
            vec![
                PackVerse {
                    book_id: 43,
                    chapter: 3,
                    verse: 16,
                    text: "Synthetic KJV stand-in for John 3:16.".into(),
                },
                PackVerse {
                    book_id: 19,
                    chapter: 23,
                    verse: 1,
                    text: "Synthetic KJV stand-in for Psalm 23:1.".into(),
                },
            ],
            true,
        )
        .await
        .unwrap();
        db
    })
}

pub fn open_empty(dir: &Path) -> BibleDb {
    block_on(BibleDb::open(&dir.join("bible.db"))).unwrap()
}

pub fn service_with(db: BibleDb, fetcher: Arc<dyn PackFetcher>) -> ScriptureService {
    let service = ScriptureService::new(fetcher);
    service.attach_db(db);
    service
}

pub fn service_with_db(db: BibleDb) -> ScriptureService {
    service_with(db, Arc::new(NoNetwork))
}

/// Fails any request: tests that must stay offline.
pub struct NoNetwork;

impl PackFetcher for NoNetwork {
    fn get<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Box<dyn PackBody>, DownloadError>> {
        Box::pin(async { Err(DownloadError::Network("offline test".into())) })
    }
}

/// A canned response, delivered in the given chunks.
#[derive(Clone)]
pub struct Canned {
    pub status: u16,
    pub content_length: Option<u64>,
    pub chunks: Vec<Vec<u8>>,
    /// When set, the body never ends: it repeats the last chunk.
    pub endless: bool,
    pub dropped: Arc<AtomicBool>,
    pub requested: Arc<Mutex<Vec<String>>>,
}

impl Canned {
    pub fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_length: None,
            chunks: vec![body],
            endless: false,
            dropped: Arc::default(),
            requested: Arc::default(),
        }
    }
}

struct CannedBody {
    canned: Canned,
    next: usize,
}

impl Drop for CannedBody {
    fn drop(&mut self) {
        self.canned.dropped.store(true, Ordering::SeqCst);
    }
}

impl PackBody for CannedBody {
    fn status(&self) -> u16 {
        self.canned.status
    }
    fn content_length(&self) -> Option<u64> {
        self.canned.content_length
    }
    fn chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, DownloadError>> {
        Box::pin(async move {
            let chunk = match self.canned.chunks.get(self.next) {
                Some(chunk) => Some(chunk.clone()),
                None if self.canned.endless => self.canned.chunks.last().cloned(),
                None => None,
            };
            self.next += 1;
            Ok(chunk)
        })
    }
}

impl PackFetcher for Canned {
    fn get<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Box<dyn PackBody>, DownloadError>> {
        self.requested.lock().unwrap().push(url.to_owned());
        let body = CannedBody {
            canned: self.clone(),
            next: 0,
        };
        Box::pin(async move { Ok(Box::new(body) as Box<dyn PackBody>) })
    }
}

/// A counting API.Bible stand-in. Every call is recorded.
#[derive(Default)]
pub struct StubApi {
    pub calls: Mutex<Vec<String>>,
    pub hits: Vec<ApiSearchHit>,
    pub authorized: BTreeSet<String>,
    pub fail_listing: bool,
}

impl StubApi {
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn record(&self, call: &str) {
        self.calls.lock().unwrap().push(call.into());
    }
}

impl ApiBible for StubApi {
    fn authorized_translations(&self) -> BoxFuture<'_, Result<BTreeSet<String>, ApiError>> {
        self.record("listBibles");
        Box::pin(async move {
            if self.fail_listing {
                Err(ApiError("API.Bible is unreachable.".into()))
            } else {
                Ok(self.authorized.clone())
            }
        })
    }
    fn resolve_bible_id<'a>(
        &'a self,
        translation: &'a str,
    ) -> BoxFuture<'a, Result<String, ApiError>> {
        self.record("listBibles");
        Box::pin(async move {
            if self.authorized.contains(&translation.to_uppercase()) {
                Ok(format!("bible-{}", translation.to_lowercase()))
            } else {
                Err(ApiError(format!(
                    "{translation} is not authorized for this API.Bible key."
                )))
            }
        })
    }
    fn search<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: u32,
    ) -> BoxFuture<'a, Result<Vec<ApiSearchHit>, ApiError>> {
        self.record("search");
        Box::pin(async move { Ok(self.hits.clone()) })
    }
    fn lookup_passage(
        &self,
        passage: ApiPassage,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, ApiError>> {
        self.record("lookupPassage");
        Box::pin(async move {
            Ok(vec![ScriptureResult {
                reference: format!(
                    "{} {}:{}",
                    passage.book_name, passage.chapter, passage.verse_start
                ),
                verses: vec![],
                translation: passage.translation,
            }])
        })
    }
    fn lookup_chapter(
        &self,
        chapter: ApiChapter,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, ApiError>> {
        self.record("lookupChapter");
        Box::pin(async move {
            Ok(vec![ScriptureResult {
                reference: format!("{} {}", chapter.book_name, chapter.chapter),
                verses: vec![],
                translation: chapter.translation,
            }])
        })
    }
}
