//! Bible queries over the profile's `bible.db`, from `bible-db.ts`.
//!
//! Every call runs on `kairo-store`'s SQLite workers. A phrase search runs all
//! of its passes in one reader job, so it sees one snapshot and costs one
//! round trip. Book names resolve in memory from a table built at open, in the
//! same order as Electron's, because the prefix fallback returns the first hit.

use crate::like_index::LikeCache;
use kairo_domain::js::{is_whitespace, trim};
use kairo_domain::scripture::small::spoken_verse_text;
use kairo_store::db::{Database, PackVerse, Schema};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, kairo_store::db::Error>;

/// Tiny words that drown phrase FTS (such as "love is patient").
const FTS_STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "of", "to", "in", "on", "at", "by", "for", "is", "are", "was",
    "were", "be", "been", "as", "it", "he", "she", "we", "they", "my", "me", "his", "her", "our",
    "your", "their",
];

const VERSE_COLUMNS: &str = "v.translation_id, v.book_id, v.chapter, v.verse, v.text, \
                             b.name AS book_name, b.abbreviation AS book_abbr";

const SEARCH_FTS: &str = "
    SELECT f.translation_id, CAST(f.book_id AS INTEGER) AS book_id,
           CAST(f.chapter AS INTEGER) AS chapter, CAST(f.verse AS INTEGER) AS verse, f.text,
           b.name AS book_name, b.abbreviation AS book_abbr
      FROM verses_fts f JOIN books b ON b.id = CAST(f.book_id AS INTEGER)
     WHERE verses_fts MATCH ?1
       AND (?2 IS NULL OR f.translation_id = ?2)
     ORDER BY rank
     LIMIT 50";

/// One verse row with its book's name, as `BibleVerse` in Electron.
#[derive(Clone, Debug, PartialEq)]
pub struct BibleVerse {
    pub translation_id: String,
    pub book_id: i64,
    pub book_name: String,
    pub book_abbr: String,
    pub chapter: i64,
    pub verse: i64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BibleBook {
    pub id: u32,
    pub name: String,
    pub abbreviation: String,
    pub testament: String,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BibleTranslation {
    pub id: String,
    pub name: String,
    pub language: String,
    pub is_default: bool,
}

/// Normalized alias to book id, in insertion order.
#[derive(Default)]
struct BookLookup {
    order: Vec<(String, u32)>,
    index: HashMap<String, u32>,
}

impl BookLookup {
    fn add(&mut self, key: String, id: u32) {
        if !self.index.contains_key(&key) {
            self.index.insert(key.clone(), id);
            self.order.push((key, id));
        }
    }

    fn add_name(&mut self, name: &str, id: u32) {
        if name.is_empty() {
            return;
        }
        // With-space and without-space variants, then without trailing periods ("Gen." is "gen").
        let key = normalized(name);
        let stripped = key.trim_end_matches('.').to_owned();
        self.add(key, id);
        self.add(compact(name), id);
        self.add(stripped, id);
    }

    fn resolve(&self, book: &str) -> Option<u32> {
        let normalized = normalized(book);
        let stripped = normalized.trim_end_matches('.');
        self.index
            .get(&normalized)
            .or_else(|| self.index.get(&compact(book)))
            .or_else(|| self.index.get(stripped))
            .copied()
            .or_else(|| self.fuzzy(&normalized))
    }

    /// Last resort: the first key that starts with the input, preferring
    /// single-word keys, so partial input like "phi" finds Philippians.
    fn fuzzy(&self, input: &str) -> Option<u32> {
        if input.encode_utf16().count() < 2 {
            return None;
        }
        let first = |single: bool| {
            self.order
                .iter()
                .find(|(key, _)| key.starts_with(input) && (!single || !key.contains(' ')))
                .map(|(_, id)| *id)
        };
        first(true).or_else(|| first(false))
    }
}

/// `s.toLowerCase().replace(/\s+/g, ' ').trim()`.
fn normalized(s: &str) -> String {
    trim(&collapse(&s.to_lowercase(), is_whitespace)).to_owned()
}

/// `s.toLowerCase().replace(/\s+/g, '')`.
fn compact(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|&c| !is_whitespace(c))
        .collect()
}

/// Replace each run of characters matching `is_gap` with one space.
fn collapse(s: &str, is_gap: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_gap = false;
    for c in s.chars() {
        if is_gap(c) {
            if !in_gap {
                out.push(' ');
            }
            in_gap = true;
        } else {
            out.push(c);
            in_gap = false;
        }
    }
    out
}

/// `query.toLowerCase().replace(/["*^(){}]/g, ' ').split(/\s+/)`, keeping
/// tokens longer than two UTF-16 units that are not stopwords.
fn significant_search_tokens(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .replace(['"', '*', '^', '(', ')', '{', '}'], " ")
        .split(is_whitespace)
        .filter(|token| token.encode_utf16().count() > 2 && !FTS_STOPWORDS.contains(token))
        .map(str::to_owned)
        .collect()
}

/// Lower case, every character outside `[a-z0-9]` as a space, runs collapsed, trimmed.
fn score_text(s: &str) -> String {
    let lower = s.to_lowercase();
    collapse(&lower, |c| !matches!(c, 'a'..='z' | '0'..='9'))
        .trim_matches(' ')
        .to_owned()
}

/// Prefer near-exact remembered phrases (Psalm 23) over loose token hits.
fn score_phrase_match(text: &str, query: &str, tokens: &[String]) -> u32 {
    let hay = score_text(&spoken_verse_text(text));
    let needle = score_text(query);
    if hay.is_empty() || needle.is_empty() {
        return 0;
    }
    if hay.contains(&needle) {
        return 1000;
    }
    if tokens.len() >= 2 {
        if hay.contains(&tokens.join(" ")) {
            return 900;
        }
        // Tokens in order with gaps: "lord … shepherd". `hay` is ASCII, so
        // byte offsets equal JavaScript's UTF-16 offsets for any token found.
        let mut from = 0;
        let mut in_order = 0;
        for token in tokens {
            let Some(at) = hay[from..].find(token.as_str()) else {
                break;
            };
            in_order += 1;
            from += at + token.len();
        }
        if in_order == tokens.len() {
            return 500 + in_order as u32 * 20;
        }
        return in_order as u32 * 15;
    }
    if tokens.iter().any(|token| hay.contains(token.as_str())) {
        10
    } else {
        0
    }
}

fn verse_from_row(row: &Row<'_>) -> rusqlite::Result<BibleVerse> {
    Ok(BibleVerse {
        translation_id: row.get("translation_id")?,
        book_id: row.get("book_id")?,
        chapter: row.get("chapter")?,
        verse: row.get("verse")?,
        text: row.get("text")?,
        book_name: row.get("book_name")?,
        book_abbr: row.get("book_abbr")?,
    })
}

fn collect_verses(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<BibleVerse>> {
    let mut statement = conn.prepare_cached(sql)?;
    statement
        .query_map(params, verse_from_row)?
        .collect::<rusqlite::Result<_>>()
}

/// `searchText`: FTS on significant tokens, then an in-order LIKE pass for
/// multi-word queries, then a plain LIKE when nothing scored. A failing pass
/// (an FTS syntax error, for example) is skipped, as in Electron.
fn search_text(
    conn: &Connection,
    like_cache: &LikeCache,
    query: &str,
    translation: Option<&str>,
    limit: usize,
) -> Vec<BibleVerse> {
    let cleaned = trim(&collapse(
        &query.replace(['"', '*', '^', '(', ')'], " "),
        is_whitespace,
    ))
    .to_owned();
    if cleaned.is_empty() {
        return Vec::new();
    }
    let tokens = significant_search_tokens(&cleaned);
    let fts_query = if tokens.is_empty() {
        cleaned.clone()
    } else {
        tokens.join(" ")
    };
    // `v.text LIKE ? AND (? IS NULL OR v.translation_id = ?) LIMIT 50`, in rowid order.
    let like = |pattern: &str| -> rusqlite::Result<Vec<BibleVerse>> {
        let rowids = like_cache.search(conn, pattern, translation, 50)?;
        let sql = format!(
            "SELECT {VERSE_COLUMNS} FROM verses v JOIN books b ON b.id = v.book_id WHERE v.rowid = ?1"
        );
        let mut statement = conn.prepare_cached(&sql)?;
        rowids
            .into_iter()
            .map(|rowid| statement.query_row([rowid], verse_from_row))
            .collect()
    };
    let mut seen = HashSet::new();
    let mut scored: Vec<(BibleVerse, u32)> = Vec::new();
    let mut push_rows = |rows: Vec<BibleVerse>, scored: &mut Vec<(BibleVerse, u32)>| {
        for verse in rows {
            let key = (
                verse.translation_id.clone(),
                verse.book_id,
                verse.chapter,
                verse.verse,
            );
            if !seen.insert(key) {
                continue;
            }
            let score = score_phrase_match(&verse.text, &cleaned, &tokens);
            if score == 0 && tokens.len() >= 2 {
                continue;
            }
            scored.push((verse, score));
        }
    };

    if let Ok(rows) = collect_verses(conn, SEARCH_FTS, params![fts_query, translation]) {
        push_rows(rows, &mut scored);
    }
    // Exact-ish phrase pass (helps when FTS token order is noisy).
    if tokens.len() >= 2 {
        let needle = format!("%{}%", tokens.join("%"));
        if let Ok(rows) = like(&needle) {
            push_rows(rows, &mut scored);
        }
    }
    if scored.is_empty() {
        let needle = format!("%{cleaned}%");
        if let Ok(rows) = like(&needle) {
            push_rows(rows, &mut scored);
        }
    }
    // Stable, like `Array.prototype.sort`.
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .take(limit)
        .map(|(verse, _)| verse)
        .collect()
}

/// A cloneable handle to the Bible database.
#[derive(Clone)]
pub struct BibleDb {
    db: Database,
    books: Arc<BookLookup>,
    like: LikeCache,
}

impl BibleDb {
    /// Open (or create) `bible.db` at `path`.
    pub async fn open(path: &Path) -> Result<Self> {
        Self::from_database(Database::open(path, Schema::Bible).await?).await
    }

    /// The writable profile copy, seeded from the bundled database on first run only.
    pub async fn open_profile(user_data: &Path, bundled_bible: &Path) -> Result<Self> {
        Self::from_database(Database::open_bible(user_data, bundled_bible).await?).await
    }

    async fn from_database(db: Database) -> Result<Self> {
        let rows: Vec<(u32, String, String, String)> = db
            .read(|conn| {
                let mut statement =
                    conn.prepare("SELECT id, name, abbreviation, aliases FROM books")?;
                statement
                    .query_map([], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })?
                    .collect()
            })
            .await?;
        let mut books = BookLookup::default();
        for (id, name, abbreviation, aliases) in rows {
            books.add_name(&name, id);
            books.add_name(&abbreviation, id);
            // A malformed alias list is ignored here, as in Electron.
            let aliases: Vec<String> = serde_json::from_str(&aliases).unwrap_or_default();
            for alias in aliases {
                books.add_name(&alias, id);
            }
        }
        // Build the phrase index now, so the first search does not pay for it.
        let like = LikeCache::default();
        let warm = like.clone();
        db.read(move |conn| warm.get(conn).map(drop)).await?;
        Ok(Self {
            db,
            books: Arc::new(books),
            like,
        })
    }

    /// The book id for a name, abbreviation or alias, or a unique-enough prefix.
    pub fn resolve_book_id(&self, book: &str) -> Option<u32> {
        self.books.resolve(book)
    }

    pub async fn get_verse(
        &self,
        translation: &str,
        book: &str,
        chapter: f64,
        verse: f64,
    ) -> Result<Option<BibleVerse>> {
        let Some(book_id) = self.resolve_book_id(book) else {
            return Ok(None);
        };
        let translation = translation.to_owned();
        self.db
            .read(move |conn| {
                let sql = format!(
                    "SELECT {VERSE_COLUMNS} FROM verses v JOIN books b ON b.id = v.book_id
                      WHERE v.translation_id = ?1 AND v.book_id = ?2 AND v.chapter = ?3 AND v.verse = ?4"
                );
                conn.prepare_cached(&sql)?
                    .query_row(params![translation, book_id, chapter, verse], verse_from_row)
                    .optional()
            })
            .await
    }

    pub async fn get_verse_range(
        &self,
        translation: &str,
        book: &str,
        chapter: f64,
        start: f64,
        end: f64,
    ) -> Result<Vec<BibleVerse>> {
        let Some(book_id) = self.resolve_book_id(book) else {
            return Ok(Vec::new());
        };
        let translation = translation.to_owned();
        self.db
            .read(move |conn| {
                let sql = format!(
                    "SELECT {VERSE_COLUMNS} FROM verses v JOIN books b ON b.id = v.book_id
                      WHERE v.translation_id = ?1 AND v.book_id = ?2 AND v.chapter = ?3
                        AND v.verse >= ?4 AND v.verse <= ?5
                      ORDER BY v.verse"
                );
                collect_verses(
                    conn,
                    &sql,
                    params![translation, book_id, chapter, start, end],
                )
            })
            .await
    }

    pub async fn get_chapter(
        &self,
        translation: &str,
        book: &str,
        chapter: f64,
    ) -> Result<Vec<BibleVerse>> {
        let Some(book_id) = self.resolve_book_id(book) else {
            return Ok(Vec::new());
        };
        let translation = translation.to_owned();
        self.db
            .read(move |conn| {
                let sql = format!(
                    "SELECT {VERSE_COLUMNS} FROM verses v JOIN books b ON b.id = v.book_id
                      WHERE v.translation_id = ?1 AND v.book_id = ?2 AND v.chapter = ?3
                      ORDER BY v.verse"
                );
                collect_verses(conn, &sql, params![translation, book_id, chapter])
            })
            .await
    }

    /// Phrase search, best matches first. `translation` filters exactly
    /// (case-sensitive), as the SQL in Electron does.
    pub async fn search_text(
        &self,
        query: &str,
        translation: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BibleVerse>> {
        let query = query.to_owned();
        let translation = translation.map(str::to_owned);
        let like = self.like.clone();
        self.db
            .read(move |conn| {
                Ok(search_text(
                    conn,
                    &like,
                    &query,
                    translation.as_deref(),
                    limit,
                ))
            })
            .await
    }

    /// The canonical book for a name. A malformed alias list is an error here,
    /// where Electron's `JSON.parse` would throw.
    pub async fn resolve_book_name(&self, input: &str) -> Result<Option<BibleBook>> {
        let Some(book_id) = self.resolve_book_id(input) else {
            return Ok(None);
        };
        let row: Option<(u32, String, String, String, String)> = self
            .db
            .read(move |conn| {
                conn.prepare_cached(
                    "SELECT id, name, abbreviation, testament, aliases FROM books WHERE id = ?1",
                )?
                .query_row([book_id], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })
                .optional()
            })
            .await?;
        let Some((id, name, abbreviation, testament, aliases)) = row else {
            return Ok(None);
        };
        Ok(Some(BibleBook {
            id,
            name,
            abbreviation,
            testament,
            aliases: serde_json::from_str(&aliases)?,
        }))
    }

    /// Every translation, the default first, then by name.
    pub async fn get_all_translations(&self) -> Result<Vec<BibleTranslation>> {
        self.db
            .read(|conn| {
                let mut statement = conn.prepare_cached(
                    "SELECT id, name, language, is_default FROM translations
                      ORDER BY is_default DESC, name",
                )?;
                statement
                    .query_map([], |row| {
                        Ok(BibleTranslation {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            language: row.get(2)?,
                            is_default: row.get::<_, i64>(3)? == 1,
                        })
                    })?
                    .collect()
            })
            .await
    }

    /// A random verse from `translation`, or from the default translation.
    pub async fn get_random_verse(&self, translation: Option<&str>) -> Result<Option<BibleVerse>> {
        let translation = translation.map(str::to_owned);
        self.db
            .read(move |conn| {
                let filter = match translation {
                    Some(_) => "v.translation_id = ?1",
                    None => {
                        "v.translation_id = (SELECT id FROM translations WHERE is_default = 1 LIMIT 1)"
                    }
                };
                let sql = format!(
                    "SELECT {VERSE_COLUMNS} FROM verses v JOIN books b ON b.id = v.book_id
                      WHERE {filter} ORDER BY RANDOM() LIMIT 1"
                );
                let mut statement = conn.prepare_cached(&sql)?;
                match translation {
                    Some(id) => statement.query_row([id], verse_from_row),
                    None => statement.query_row([], verse_from_row),
                }
                .optional()
            })
            .await
    }

    pub async fn has_translation(&self, id: &str) -> Result<bool> {
        let id = id.to_owned();
        self.db
            .read(move |conn| {
                conn.prepare_cached("SELECT COUNT(*) FROM translations WHERE id = ?1")?
                    .query_row([id], |row| row.get::<_, i64>(0))
            })
            .await
            .map(|n| n > 0)
    }

    pub async fn get_verse_count(&self, translation_id: &str) -> Result<u64> {
        let id = translation_id.to_owned();
        self.db
            .read(move |conn| {
                conn.prepare_cached("SELECT COUNT(*) FROM verses WHERE translation_id = ?1")?
                    .query_row([id], |row| row.get::<_, i64>(0))
            })
            .await
            .map(|n| n as u64)
    }

    pub async fn get_chapter_count(&self, translation_id: &str) -> Result<u64> {
        let id = translation_id.to_owned();
        self.db
            .read(move |conn| {
                conn.prepare_cached(
                    "SELECT COUNT(*) FROM
                       (SELECT DISTINCT book_id, chapter FROM verses WHERE translation_id = ?1)",
                )?
                .query_row([id], |row| row.get::<_, i64>(0))
            })
            .await
            .map(|n| n as u64)
    }

    pub async fn set_default_translation(&self, id: &str) -> Result<()> {
        let id = id.to_owned();
        self.db
            .write(move |conn| {
                conn.execute(
                    "UPDATE translations SET is_default = (CASE WHEN id = ?1 THEN 1 ELSE 0 END)",
                    [id],
                )
                .map(drop)
            })
            .await
    }

    /// Bulk insert for seeding: clears the translation's FTS rows, records the
    /// translation, then inserts verses in transactions of 2,000.
    pub async fn import_translation(
        &self,
        id: &str,
        name: &str,
        language: &str,
        verses: Vec<PackVerse>,
        is_default: bool,
    ) -> Result<()> {
        let (id, name, language) = (id.to_owned(), name.to_owned(), language.to_owned());
        let total = verses.len();
        self.db
            .write(move |conn| {
                conn.execute("DELETE FROM verses_fts WHERE translation_id = ?1", [&id])?;
                conn.execute(
                    "INSERT OR REPLACE INTO translations (id, name, language, is_default)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, name, language, i64::from(is_default)],
                )?;
                for batch in verses.chunks(2_000) {
                    let tx = conn.transaction()?;
                    insert_verses(&tx, &id, batch)?;
                    tx.commit()?;
                }
                Ok(())
            })
            .await?;
        self.like.invalidate();
        tracing::info!(verses = total, "[BibleDB] Translation imported");
        Ok(())
    }

    /// Replace every row of one translation (verses, FTS entries and the
    /// translation record) in a single transaction. A failure part way rolls
    /// back and leaves the previously installed copy untouched. Callers
    /// validate the pack first. The record never takes the default flag.
    pub async fn import_translation_pack(
        &self,
        id: &str,
        name: &str,
        language: &str,
        verses: Vec<PackVerse>,
    ) -> Result<()> {
        let (id, name, language) = (id.to_owned(), name.to_owned(), language.to_owned());
        let total = verses.len();
        let log_id = id.clone();
        self.db
            .write(move |conn| {
                let tx = conn.transaction()?;
                tx.execute("DELETE FROM verses_fts WHERE translation_id = ?1", [&id])?;
                tx.execute("DELETE FROM verses WHERE translation_id = ?1", [&id])?;
                tx.execute(
                    "INSERT OR REPLACE INTO translations (id, name, language, is_default)
                     VALUES (?1, ?2, ?3, 0)",
                    params![id, name, language],
                )?;
                insert_verses(&tx, &id, &verses)?;
                tx.commit()
            })
            .await?;
        self.like.invalidate();
        tracing::info!(id = %log_id, verses = total, "[BibleDB] Local Bible pack imported");
        Ok(())
    }

    /// Delete one translation's verses, FTS entries and record in a single
    /// transaction. Returns how many verses it had.
    pub async fn remove_translation(&self, id: &str) -> Result<u64> {
        let before = self.get_verse_count(id).await?;
        let id = id.to_owned();
        let log_id = id.clone();
        self.db
            .write(move |conn| {
                let tx = conn.transaction()?;
                tx.execute("DELETE FROM verses_fts WHERE translation_id = ?1", [&id])?;
                tx.execute("DELETE FROM verses WHERE translation_id = ?1", [&id])?;
                tx.execute("DELETE FROM translations WHERE id = ?1", [&id])?;
                tx.commit()
            })
            .await?;
        self.like.invalidate();
        tracing::info!(id = %log_id, verses = before, "[BibleDB] Local Bible translation removed");
        Ok(before)
    }

    /// Raw access for callers that need a query this type does not offer.
    pub fn database(&self) -> &Database {
        &self.db
    }
}

fn insert_verses(conn: &Connection, id: &str, verses: &[PackVerse]) -> rusqlite::Result<()> {
    let mut verse = conn.prepare_cached(
        "INSERT OR REPLACE INTO verses (translation_id, book_id, chapter, verse, text)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut fts = conn.prepare_cached(
        "INSERT INTO verses_fts (text, translation_id, book_id, chapter, verse)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for row in verses {
        verse.execute(params![id, row.book_id, row.chapter, row.verse, row.text])?;
        fts.execute(params![row.text, id, row.book_id, row.chapter, row.verse])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_drop_stopwords_short_words_and_fts_syntax() {
        assert_eq!(
            significant_search_tokens("The LORD is my (shepherd) {notes} \"ok\""),
            ["lord", "shepherd", "notes"]
        );
        assert!(significant_search_tokens("is a an").is_empty());
    }

    #[test]
    fn phrase_scores_follow_electron_tiers() {
        let tokens = significant_search_tokens("lord shepherd");
        let text = "The LORD is my shepherd; I shall not want.";
        assert_eq!(
            score_phrase_match(text, "lord is my shepherd", &tokens),
            1000
        );
        assert_eq!(score_phrase_match(text, "lord shepherd", &tokens), 540);
        let backwards = significant_search_tokens("shepherd lord");
        assert_eq!(score_phrase_match(text, "shepherd lord", &backwards), 15);
        let joined = significant_search_tokens("the lord shepherd");
        assert_eq!(
            score_phrase_match("lord shepherd", "the lord shepherd", &joined),
            900
        );
        let single = significant_search_tokens("the shepherd");
        assert_eq!(score_phrase_match(text, "the shepherd", &single), 10);
        assert_eq!(score_phrase_match("", "lord", &single), 0);
        // Marginal notes are not read; supplied words are.
        assert_eq!(
            score_phrase_match("all {ye} that labour {x: note}", "ye that labour", &[]),
            1000
        );
    }

    #[test]
    fn book_lookup_keeps_insertion_order_for_prefixes() {
        let mut books = BookLookup::default();
        books.add_name("Philippians", 50);
        books.add_name("Philemon", 57);
        books.add_name("1 John", 62);
        books.add_name("Gen.", 1);
        assert_eq!(books.resolve("phil"), Some(50));
        assert_eq!(books.resolve("PHILE"), Some(57));
        assert_eq!(books.resolve("1john"), Some(62));
        assert_eq!(books.resolve("  1   JOHN "), Some(62));
        assert_eq!(books.resolve("gen"), Some(1));
        assert_eq!(books.resolve("1 j"), Some(62));
        assert_eq!(books.resolve("p"), None);
        assert_eq!(books.resolve("zzz"), None);
    }
}
