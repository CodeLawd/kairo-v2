//! Bounded FTS retrieval and word rarity from `global-quote.ts`. Worker
//! thread only: these hold a SQLite connection.

use super::matching::{GlobalQuoteCandidate, RELAXED_SKIP, evidence_tokens, match_global_quote};
use kairo_domain::scripture::plan::verse_key;
use rusqlite::{Connection, params};
use std::collections::{HashMap, HashSet};

const RETRIEVE: &str = "SELECT b.name AS book, CAST(f.chapter AS INTEGER) AS chapter,
    CAST(f.verse AS INTEGER) AS verse, f.text, f.translation_id AS translation
  FROM verses_fts f JOIN books b ON b.id = CAST(f.book_id AS INTEGER)
  WHERE verses_fts MATCH ? ORDER BY rank LIMIT 120";

fn literal(t: &str) -> String {
    format!("\"{}\"", t.replace('"', "\"\""))
}

fn query(conn: &Connection, fts: &str) -> rusqlite::Result<Vec<GlobalQuoteCandidate>> {
    let mut stmt = conn.prepare_cached(RETRIEVE)?;
    stmt.query_map(params![fts], |row| {
        Ok(GlobalQuoteCandidate {
            book: row.get("book")?,
            chapter: row.get::<_, i64>("chapter")? as f64,
            verse: row.get::<_, i64>("verse")? as f64,
            text: row.get("text")?,
            translation: row.get("translation")?,
            ..Default::default()
        })
    })?
    .collect()
}

/// `retrieveGlobalQuote`: AND windows, then a relaxed OR when nothing matched.
/// No LIKE scans. At most 240 candidates.
pub fn retrieve_global_quote(
    conn: &Connection,
    text: &str,
) -> rusqlite::Result<Vec<GlobalQuoteCandidate>> {
    let mut seen = HashSet::new();
    let all: Vec<String> = evidence_tokens(text)
        .into_iter()
        .filter(|t| t.len() > 2 && seen.insert(t.clone()))
        .collect();
    let tokens = all[all.len().saturating_sub(12)..].to_vec();
    if tokens.len() < 3 {
        return Ok(Vec::new());
    }
    let mut results: Vec<GlobalQuoteCandidate> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut add = |rows: Vec<GlobalQuoteCandidate>, results: &mut Vec<GlobalQuoteCandidate>| {
        for row in rows {
            let key = format!(
                "{}|{}",
                row.translation.as_deref().unwrap_or("undefined"),
                verse_key(&row.book, row.chapter, row.verse)
            );
            match index.get(&key) {
                Some(&i) => results[i] = row,
                None => {
                    index.insert(key, results.len());
                    results.push(row);
                }
            }
        }
    };
    let n = tokens.len();
    let slice = |from: isize, to: isize| -> Vec<String> {
        // `Array.prototype.slice` with negative indices.
        let at = |i: isize| {
            if i < 0 {
                (n as isize + i).max(0) as usize
            } else {
                (i as usize).min(n)
            }
        };
        let (a, b) = (at(from), at(to));
        if a < b {
            tokens[a..b].to_vec()
        } else {
            Vec::new()
        }
    };
    let windows = [slice(-6, n as isize), slice(-9, -3), slice(0, 6)];
    for w in windows.iter().filter(|w| w.len() >= 3) {
        let fts = w
            .iter()
            .map(|t| literal(t))
            .collect::<Vec<_>>()
            .join(" AND ");
        add(query(conn, &fts)?, &mut results);
    }
    if match_global_quote(text, &results, None).is_empty() {
        let anchors: Vec<&String> = tokens
            .iter()
            .filter(|t| !RELAXED_SKIP.contains(&t.as_str()))
            .collect();
        let anchors = &anchors[anchors.len().saturating_sub(6)..];
        if anchors.len() >= 3 {
            let fts = anchors
                .iter()
                .map(|t| literal(t))
                .collect::<Vec<_>>()
                .join(" OR ");
            add(query(conn, &fts)?, &mut results);
        }
    }
    results.truncate(240);
    Ok(results)
}

/// `wordRarity`: verses per word per installed translation, cached.
pub struct WordRarityTable {
    translations: f64,
    cache: HashMap<String, f64>,
}

impl WordRarityTable {
    pub fn new(conn: &Connection) -> rusqlite::Result<Self> {
        let n: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT translation_id) FROM verses",
            [],
            |r| r.get(0),
        )?;
        Ok(Self {
            translations: n.max(1) as f64,
            cache: HashMap::new(),
        })
    }

    /// Prefix match, so the stem "garment" also counts "garments".
    pub fn rarity(&mut self, conn: &Connection, word: &str) -> rusqlite::Result<f64> {
        if let Some(&v) = self.cache.get(word) {
            return Ok(v);
        }
        let n: i64 = conn
            .prepare_cached("SELECT COUNT(*) FROM verses_fts WHERE verses_fts MATCH ?")?
            .query_row(params![format!("{}*", literal(word))], |r| r.get(0))?;
        let verses = n as f64 / self.translations;
        if self.cache.len() > 20_000 {
            self.cache.clear();
        }
        self.cache.insert(word.to_owned(), verses);
        Ok(verses)
    }
}
