//! The phrase search's `LIKE` passes, answered from memory.
//!
//! Electron runs `SELECT … FROM verses v JOIN books b … WHERE v.text LIKE ?
//! AND (? IS NULL OR v.translation_id = ?) LIMIT 50`, which SQLite can only
//! answer by scanning every verse in rowid order: about 16 ms on the bundled
//! Bible on its own (measured), which would put a phrase search over its
//! 10 ms budget. This keeps every verse's ASCII-lower-cased text in rowid
//! order and applies SQLite's `LIKE` rules to it, so the same rows come back
//! in the same order; only the matching rows are then read from SQLite.
//!
//! Every write through `BibleDb` drops the index. A commit by any other
//! connection, another process included, changes the reader's
//! `PRAGMA data_version`; the index is then checked against the `verses`
//! table's largest rowid and row count, and rebuilt when they changed.

use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// SQLite's default `SQLITE_MAX_LIKE_PATTERN_LENGTH`. A longer pattern is an
/// error there, which the phrase search skips.
pub const MAX_PATTERN_BYTES: usize = 50_000;

/// A `LIKE` pattern of the form `%seg%seg%…%`, as the phrase search builds.
/// `_` matches one character; letters compare ASCII-case-insensitively, as
/// SQLite's built-in `LIKE` does without ICU.
pub struct LikePattern {
    segments: Vec<Segment>,
}

enum Segment {
    Literal(memchr::memmem::Finder<'static>),
    /// Holds at least one `_`.
    Wild(Vec<u8>),
}

impl LikePattern {
    /// `None` when SQLite would refuse the pattern as too long. Panics on a
    /// pattern that does not start and end with `%` (a programming error).
    pub fn new(pattern: &str) -> Option<Self> {
        assert!(pattern.len() >= 2 && pattern.starts_with('%') && pattern.ends_with('%'));
        if pattern.len() > MAX_PATTERN_BYTES {
            return None;
        }
        let segments = pattern
            .to_ascii_lowercase()
            .split('%')
            .filter(|segment| !segment.is_empty())
            .map(|segment| match segment.contains('_') {
                true => Segment::Wild(segment.as_bytes().to_vec()),
                false => {
                    Segment::Literal(memchr::memmem::Finder::new(segment.as_bytes()).into_owned())
                }
            })
            .collect();
        Some(Self { segments })
    }

    /// Whether ASCII-lower-cased `text` matches.
    pub fn matches(&self, text: &[u8]) -> bool {
        let mut from = 0;
        for segment in &self.segments {
            // Greedy leftmost matching is exact here: each segment is a fixed
            // number of characters, so an earlier start also ends earlier.
            match find_segment(&text[from..], segment) {
                Some(end) => from += end,
                None => return false,
            }
        }
        true
    }
}

/// The end of the first match of `segment` in `text`, in bytes.
fn find_segment(text: &[u8], segment: &Segment) -> Option<usize> {
    let segment = match segment {
        // A UTF-8 lead or ASCII byte never matches a continuation byte, so
        // every hit starts on a character boundary.
        Segment::Literal(finder) => {
            return finder.find(text).map(|at| at + finder.needle().len());
        }
        Segment::Wild(segment) => segment,
    };
    let mut start = 0;
    while start < text.len() {
        if let Some(end) = match_at(&text[start..], segment) {
            return Some(start + end);
        }
        start += char_len(text[start]);
    }
    None
}

fn match_at(text: &[u8], segment: &[u8]) -> Option<usize> {
    let mut at = 0;
    for &byte in segment {
        let &next = text.get(at)?;
        if byte == b'_' {
            at += char_len(next);
        } else if byte == next {
            at += 1;
        } else {
            return None;
        }
    }
    (at <= text.len()).then_some(at)
}

fn char_len(lead: u8) -> usize {
    match lead {
        0xF0.. => 4,
        0xE0.. => 3,
        0xC0.. => 2,
        _ => 1,
    }
}

struct Row {
    rowid: i64,
    translation: u32,
    text: Box<[u8]>,
}

pub struct LikeIndex {
    fingerprint: (i64, i64),
    translations: Vec<String>,
    rows: Vec<Row>,
}

fn fingerprint(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    conn.prepare_cached("SELECT coalesce(max(rowid), 0), count(*) FROM verses")?
        .query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
}

impl LikeIndex {
    fn build(conn: &Connection, fingerprint: (i64, i64)) -> rusqlite::Result<Self> {
        let mut translations: Vec<String> = Vec::new();
        let mut rows = Vec::new();
        // The inner join drops verses without a book, as Electron's query does.
        let mut statement = conn.prepare(
            "SELECT v.rowid, v.translation_id, v.text FROM verses v
               JOIN books b ON b.id = v.book_id ORDER BY v.rowid",
        )?;
        let mut cursor = statement.query([])?;
        while let Some(row) = cursor.next()? {
            let id = row.get_ref(1)?.as_str()?;
            let translation = match translations.iter().position(|t| t == id) {
                Some(index) => index,
                None => {
                    translations.push(id.to_owned());
                    translations.len() - 1
                }
            };
            let text = row.get_ref(2)?.as_str()?.to_ascii_lowercase();
            rows.push(Row {
                rowid: row.get(0)?,
                translation: translation as u32,
                text: text.into_bytes().into_boxed_slice(),
            });
        }
        Ok(Self {
            fingerprint,
            translations,
            rows,
        })
    }

    /// Rowids of the first `limit` matches in rowid order.
    fn matches(&self, pattern: &LikePattern, translation: Option<&str>, limit: usize) -> Vec<i64> {
        let filter = match translation {
            Some(id) => match self.translations.iter().position(|t| t == id) {
                Some(index) => Some(index as u32),
                None => return Vec::new(),
            },
            None => None,
        };
        self.rows
            .iter()
            .filter(|row| filter.is_none_or(|t| row.translation == t))
            .filter(|row| pattern.matches(&row.text))
            .take(limit)
            .map(|row| row.rowid)
            .collect()
    }
}

#[derive(Default)]
struct Slot {
    index: Option<Arc<LikeIndex>>,
    /// Per reader connection, the `data_version` at which it last confirmed
    /// the index. It changes only when another connection commits.
    confirmed: HashMap<usize, i64>,
}

/// The shared, lazily rebuilt index for one database.
#[derive(Clone, Default)]
pub struct LikeCache(Arc<Mutex<Slot>>);

impl LikeCache {
    /// Forget the index; the next search rebuilds it.
    pub fn invalidate(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Slot::default();
    }

    /// The current index, rebuilt on `conn` when the table has changed.
    pub fn get(&self, conn: &Connection) -> rusqlite::Result<Arc<LikeIndex>> {
        // Reader connections live as long as their worker, so the address names one.
        let key = std::ptr::from_ref(conn) as usize;
        let version: i64 = conn.query_row("PRAGMA data_version", [], |row| row.get(0))?;
        // Held while building, so concurrent searches build it once.
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(index) = &slot.index
            && slot.confirmed.get(&key) == Some(&version)
        {
            return Ok(index.clone());
        }
        let current = fingerprint(conn)?;
        let index = match slot
            .index
            .clone()
            .filter(|index| index.fingerprint == current)
        {
            Some(index) => index,
            None => {
                slot.confirmed.clear();
                let index = Arc::new(LikeIndex::build(conn, current)?);
                slot.index = Some(index.clone());
                index
            }
        };
        slot.confirmed.insert(key, version);
        Ok(index)
    }

    /// Rowids matching `pattern` in rowid order, as Electron's `LIKE` query.
    pub fn search(
        &self,
        conn: &Connection,
        pattern: &str,
        translation: Option<&str>,
        limit: usize,
    ) -> rusqlite::Result<Vec<i64>> {
        let Some(pattern) = LikePattern::new(pattern) else {
            return Err(rusqlite::Error::InvalidQuery);
        };
        Ok(self.get(conn)?.matches(&pattern, translation, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_like(conn: &Connection, text: &str, pattern: &str) -> bool {
        conn.query_row("SELECT ?1 LIKE ?2", [text, pattern], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn matches_sqlite_like_for_generated_texts_and_patterns() {
        let conn = Connection::open_in_memory().unwrap();
        let alphabet = [
            "a", "B", "c", "é", "É", "ß", "_", "%", " ", "z", "Ω", "😀", "-", "'",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let mut word = |len: usize| {
            (0..len)
                .map(|_| alphabet[next(alphabet.len())])
                .collect::<String>()
        };
        let mut checked = 0;
        for _ in 0..4_000 {
            let text = word(12);
            let inner = word(3);
            let pattern = format!("%{inner}%");
            let expected = sqlite_like(&conn, &text, &pattern);
            let pattern_ = LikePattern::new(&pattern).unwrap();
            assert_eq!(
                pattern_.matches(text.to_ascii_lowercase().as_bytes()),
                expected,
                "{text:?} LIKE {pattern:?}"
            );
            checked += usize::from(expected);
        }
        assert!(checked > 100, "too few positive cases: {checked}");
        for (text, pattern) in [
            ("The LORD is my shepherd", "%lord%shepherd%"),
            ("The LORD is my shepherd", "%shepherd%lord%"),
            ("café au lait", "%CAF_%"),
            ("CAFÉ", "%café%"),
            ("a😀b", "%a_b%"),
            ("ab", "%a_b%"),
            ("abc", "%%"),
            ("", "%a%"),
        ] {
            assert_eq!(
                LikePattern::new(pattern)
                    .unwrap()
                    .matches(text.to_ascii_lowercase().as_bytes()),
                sqlite_like(&conn, text, pattern),
                "{text:?} LIKE {pattern:?}"
            );
        }
        assert!(LikePattern::new(&format!("%{}%", "a".repeat(MAX_PATTERN_BYTES))).is_none());
    }
}
