//! The Scripture page's view types and pure rules (`types.ts`, the helpers
//! in `Scripture.tsx`, `ScriptureSearchBar.tsx` and `VerseCardGrid.tsx`).
//! Nothing here touches GPUI, so it is tested without a window.

use kairo_domain::scripture::ScriptureResult;
use kairo_domain::scripture::query::{BookCompletion, expand_scripture_result};
use serde_json::Value;

/// `SendStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SendStatus {
    #[default]
    Idle,
    Sending,
    Sent,
    Error,
}

/// `ResultCard`: one verse card.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultCard {
    pub result: ScriptureResult,
    pub send_status: SendStatus,
    pub plan_item_id: Option<String>,
}

/// `ResultRow`: one playlist or search item; multi-verse items grid their cards.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultRow {
    pub id: String,
    pub reference: String,
    pub plan_item_id: Option<String>,
    pub note: Option<String>,
    pub cards: Vec<ResultCard>,
}

pub const CARD_ZOOM_MIN: f32 = 50.0;
pub const CARD_ZOOM_MAX: f32 = 250.0;
pub const CARD_ZOOM_DEFAULT: f32 = 100.0;
/// Minimum card width at 100%, the same as a lyric card.
pub const CARD_BASE_WIDTH: f32 = 240.0;

/// The card minimum width at a zoom (`Math.round(CARD_BASE_WIDTH * zoom/100)`).
pub fn card_min_width(zoom: f32) -> f32 {
    (CARD_BASE_WIDTH * zoom / 100.0).round()
}

/// `createResultRow`'s options.
#[derive(Clone, Debug, Default)]
pub struct RowOptions {
    pub id: Option<String>,
    pub plan_item_id: Option<String>,
    pub send_status: SendStatus,
    pub note: Option<String>,
}

/// `createResultRow`.
pub fn create_result_row(result: &ScriptureResult, options: RowOptions) -> ResultRow {
    let verses = expand_scripture_result(result);
    let source = if verses.is_empty() {
        vec![result.clone()]
    } else {
        verses
    };
    let cards = source
        .into_iter()
        .map(|result| ResultCard {
            result,
            send_status: options.send_status,
            plan_item_id: options.plan_item_id.clone(),
        })
        .collect();
    ResultRow {
        id: options
            .id
            .or_else(|| options.plan_item_id.clone())
            .unwrap_or_else(|| result.reference.clone()),
        reference: result.reference.clone(),
        plan_item_id: options.plan_item_id,
        note: options.note,
        cards,
    }
}

/// `flattenResultRows`.
pub fn flatten(rows: &[ResultRow]) -> Vec<&ResultCard> {
    rows.iter().flat_map(|row| row.cards.iter()).collect()
}

pub fn card_count(rows: &[ResultRow]) -> usize {
    rows.iter().map(|row| row.cards.len()).sum()
}

pub fn card_at(rows: &[ResultRow], flat: usize) -> Option<&ResultCard> {
    let (row, card) = locate(rows, flat)?;
    rows[row].cards.get(card)
}

/// `findFlatCardIndex`.
pub fn find_flat(
    rows: &[ResultRow],
    predicate: impl Fn(&ResultCard, &ResultRow) -> bool,
) -> Option<usize> {
    let mut flat = 0;
    for row in rows {
        for card in &row.cards {
            if predicate(card, row) {
                return Some(flat);
            }
            flat += 1;
        }
    }
    None
}

/// `locateFlatCard`: `(row, card in row)`.
pub fn locate(rows: &[ResultRow], flat: usize) -> Option<(usize, usize)> {
    let mut remaining = flat;
    for (index, row) in rows.iter().enumerate() {
        if remaining < row.cards.len() {
            return Some((index, remaining));
        }
        remaining -= row.cards.len();
    }
    None
}

/// `flatIndexAt`.
pub fn flat_index_at(rows: &[ResultRow], row: usize, card: usize) -> usize {
    rows[..row.min(rows.len())]
        .iter()
        .map(|r| r.cards.len())
        .sum::<usize>()
        + card
}

/// `mapCardStatus`.
pub fn set_card_status(rows: &mut [ResultRow], flat: usize, status: SendStatus) {
    if let Some((row, card)) = locate(rows, flat) {
        rows[row].cards[card].send_status = status;
    }
}

/// Which passages are loaded (`loadedPassages`); a change clears the picks.
pub fn loaded_key(rows: &[ResultRow]) -> String {
    rows.iter()
        .map(|row| format!("{}:{}", row.id, row.cards.len()))
        .collect::<Vec<_>>()
        .join("|")
}

/// Verse or chapter for the queue's toggle (`searchScope`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Verse,
    Chapter,
}

pub fn search_scope(row: &ResultRow) -> Scope {
    if row.id.starts_with("chapter-") || (!row.reference.contains(':') && row.cards.len() > 1) {
        Scope::Chapter
    } else {
        Scope::Verse
    }
}

/// The grid keys (`handleQueueKeyboard`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridKey {
    Left,
    Right,
    Up,
    Down,
}

/// The card a key moves to, or `None` when it stays put.
pub fn grid_move(rows: &[ResultRow], active: usize, key: GridKey) -> Option<usize> {
    let (row, card) = locate(rows, active)?;
    let total = card_count(rows);
    let next = match key {
        GridKey::Left => active.checked_sub(1)?,
        GridKey::Right => active + 1,
        GridKey::Up => {
            let prev = rows.get(row.checked_sub(1)?)?;
            flat_index_at(rows, row - 1, card.min(prev.cards.len().saturating_sub(1)))
        }
        GridKey::Down => {
            let next = rows.get(row + 1)?;
            flat_index_at(rows, row + 1, card.min(next.cards.len().saturating_sub(1)))
        }
    };
    let next = next.min(total.saturating_sub(1));
    (next != active).then_some(next)
}

// ─── Recent lookups (`kairo-scripture-recent-lookups`) ─────────────────────

/// The `ui-state` key, as Electron's `localStorage` key.
pub const RECENT_LOOKUPS_KEY: &str = "kairo-scripture-recent-lookups";
pub const RECENT_LOOKUPS_MAX: usize = 10;

/// `readRecentLookups`. The value is the JSON text `localStorage` held (a
/// string), or, from a store written natively, the array itself.
pub fn read_recent_lookups(value: Option<&Value>) -> Vec<String> {
    let parsed = match value {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok(),
        Some(other) => Some(other.clone()),
        None => None,
    };
    match parsed {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(s) => Some(s),
                _ => None,
            })
            .take(RECENT_LOOKUPS_MAX)
            .collect(),
        _ => Vec::new(),
    }
}

/// `rememberLookup`: newest first, no repeats, at most ten.
pub fn remember_lookup(current: &[String], reference: &str) -> Vec<String> {
    std::iter::once(reference.to_owned())
        .chain(current.iter().filter(|item| *item != reference).cloned())
        .take(RECENT_LOOKUPS_MAX)
        .collect()
}

/// The stored form: JSON text, as `localStorage.setItem` wrote it.
pub fn recent_lookups_value(lookups: &[String]) -> Value {
    Value::String(serde_json::to_string(lookups).unwrap_or_else(|_| "[]".into()))
}

// ─── The search bar's list ──────────────────────────────────────────────────

/// One row under the search field.
#[derive(Clone, Debug, PartialEq)]
pub enum BarItem {
    Book(BookCompletion),
    Verse(ScriptureResult),
    Recent(String),
}

/// The list: recent lookups after ▾, otherwise up to four book completions
/// then the live verse matches while they are current.
pub fn bar_items(
    recents_open: bool,
    recents: &[String],
    books: &[BookCompletion],
    suggestions_open: bool,
    suggestions: &[ScriptureResult],
) -> Vec<BarItem> {
    if recents_open {
        return recents.iter().cloned().map(BarItem::Recent).collect();
    }
    books
        .iter()
        .take(4)
        .cloned()
        .map(BarItem::Book)
        .chain(
            suggestions
                .iter()
                .filter(|_| suggestions_open)
                .cloned()
                .map(BarItem::Verse),
        )
        .collect()
}

/// ↑/↓ cycle through "nothing picked" (`None`) and every row.
pub fn cycle_active(active: Option<usize>, len: usize, down: bool) -> Option<usize> {
    let slots = len as i64 + 1;
    let current = active.map_or(-1, |a| a as i64);
    let step = if down { 1 } else { -1 };
    let next = (current + 1 + step + slots).rem_euclid(slots) - 1;
    (next >= 0).then_some(next as usize)
}

/// "John 3:16" or "John 3:16-18": smaller than its chapter.
pub fn is_within_chapter(result: &ScriptureResult) -> bool {
    result.reference.contains(':')
}

/// The verse text as read, without ¶ marks.
pub fn verse_text(result: &ScriptureResult) -> String {
    let joined = result
        .verses
        .iter()
        .map(|v| v.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = String::with_capacity(joined.len());
    let mut chars = joined.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '¶' {
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_owned()
}

/// `Highlighted`: `text` split into `(part, emphasised)`. The whole query
/// first; otherwise each word of three or more letters. Case-insensitive.
pub fn highlight(text: &str, query: &str) -> Vec<(String, bool)> {
    let phrase = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = |s: &str| s.chars().flat_map(char::to_lowercase).collect::<String>();
    let chars: Vec<char> = text.chars().collect();
    let folded: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let patterns: Vec<Vec<char>> = if !phrase.is_empty() && lower(text).contains(&lower(&phrase)) {
        vec![lower(&phrase).chars().collect()]
    } else {
        phrase
            .split(' ')
            .filter(|w| w.chars().count() >= 3)
            .map(|w| lower(w).chars().collect())
            .collect()
    };
    if patterns.is_empty() {
        return vec![(text.to_owned(), false)];
    }
    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut plain = String::new();
    let mut at = 0;
    while at < chars.len() {
        let hit = patterns
            .iter()
            .find(|p| !p.is_empty() && folded[at..].starts_with(p))
            .map(Vec::len);
        match hit {
            Some(len) => {
                if !plain.is_empty() {
                    parts.push((std::mem::take(&mut plain), false));
                }
                parts.push((chars[at..at + len].iter().collect(), true));
                at += len;
            }
            None => {
                plain.push(chars[at]);
                at += 1;
            }
        }
    }
    if !plain.is_empty() {
        parts.push((plain, false));
    }
    parts
}

// ─── The virtualised grid ───────────────────────────────────────────────────

/// Gap between cards (`gap-3`).
pub const CARD_GAP: f32 = 12.0;
/// The black label bar under a slide (34 px, DECISIONS 113) plus the
/// frame (1 px a side when live or picked).
pub const CARD_CHROME: f32 = 34.0 + 2.0;
/// A passage header bar (`min-h-9`) and its `mb-3`.
pub const HEADER_HEIGHT: f32 = 36.0;
pub const HEADER_GAP: f32 = 12.0;
/// `pb-7` under each passage when there is more than one.
pub const ROW_GAP: f32 = 28.0;

/// One line of the grid: a passage header, or a run of cards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridLine {
    Header {
        row: usize,
    },
    Cards {
        row: usize,
        first: usize,
        count: usize,
        last_in_row: bool,
    },
}

/// Columns that fit (`repeat(auto-fill, minmax(min(100%, w), 1fr))`).
pub fn columns(width: f32, min_width: f32) -> usize {
    (((width + CARD_GAP) / (min_width + CARD_GAP)).floor() as usize).max(1)
}

/// A card's width once stretched over the row.
pub fn card_width(width: f32, cols: usize) -> f32 {
    ((width - CARD_GAP * (cols as f32 - 1.0)) / cols as f32).max(1.0)
}

/// A card's height: a 16:9 slide plus its label bar.
pub fn card_height(card_width: f32) -> f32 {
    (card_width * 9.0 / 16.0).round() + CARD_CHROME
}

/// Whether a passage gets a header (several passages, or a note).
pub fn has_header(rows: &[ResultRow], row: &ResultRow) -> bool {
    rows.len() > 1 || row.note.is_some()
}

/// The grid as lines of `cols` cards, so only lines on screen are built.
pub fn grid_lines(rows: &[ResultRow], cols: usize) -> Vec<GridLine> {
    let cols = cols.max(1);
    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if has_header(rows, row) {
            lines.push(GridLine::Header { row: index });
        }
        let mut first = 0;
        while first < row.cards.len() {
            let count = cols.min(row.cards.len() - first);
            lines.push(GridLine::Cards {
                row: index,
                first,
                count,
                last_in_row: first + count >= row.cards.len(),
            });
            first += count;
        }
    }
    lines
}

/// Each line's height.
pub fn line_height(line: &GridLine, rows: &[ResultRow], card_height: f32) -> f32 {
    match line {
        GridLine::Header { .. } => HEADER_HEIGHT + HEADER_GAP,
        GridLine::Cards { last_in_row, .. } => {
            let tail = if *last_in_row {
                if rows.len() > 1 { ROW_GAP } else { 0.0 }
            } else {
                CARD_GAP
            };
            card_height + tail
        }
    }
}

/// The line holding a card (for scrolling to it).
pub fn line_of_card(lines: &[GridLine], rows: &[ResultRow], flat: usize) -> Option<usize> {
    let (row, card) = locate(rows, flat)?;
    lines.iter().position(|line| match line {
        GridLine::Cards {
            row: r,
            first,
            count,
            ..
        } => *r == row && card >= *first && card < first + count,
        GridLine::Header { .. } => false,
    })
}

/// The first line of a passage (its header, or its first cards).
pub fn line_of_row(lines: &[GridLine], row: usize) -> Option<usize> {
    lines.iter().position(|line| match line {
        GridLine::Header { row: r } | GridLine::Cards { row: r, .. } => *r == row,
    })
}

/// A count with its noun (`3 verses`, `1 verse`).
pub fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::scripture::ScriptureVerse;
    use serde_json::json;

    fn verse(n: f64) -> ScriptureVerse {
        serde_json::from_value(
            json!({"book":"John","chapter":3.0,"verse":n,"text":format!("v{n}")}),
        )
        .unwrap()
    }
    fn result(reference: &str, verses: &[f64]) -> ScriptureResult {
        ScriptureResult {
            reference: reference.into(),
            translation: "KJV".into(),
            verses: verses.iter().copied().map(verse).collect(),
        }
    }
    fn rows() -> Vec<ResultRow> {
        vec![
            create_result_row(
                &result("John 3:16-18", &[16., 17., 18.]),
                RowOptions::default(),
            ),
            create_result_row(&result("John 3:1", &[1.]), RowOptions::default()),
            create_result_row(&result("John 3:2-3", &[2., 3.]), RowOptions::default()),
        ]
    }

    #[test]
    fn a_range_becomes_one_row_of_verse_cards() {
        let row = &rows()[0];
        assert_eq!(row.cards.len(), 3);
        assert_eq!(row.id, "John 3:16-18");
        let empty = create_result_row(&result("John 3:99", &[]), RowOptions::default());
        assert_eq!(
            empty.cards.len(),
            1,
            "no verses keeps the passage as one card"
        );
    }

    #[test]
    fn flat_indexes_round_trip() {
        let rows = rows();
        assert_eq!(card_count(&rows), 6);
        assert_eq!(locate(&rows, 3), Some((1, 0)));
        assert_eq!(locate(&rows, 6), None);
        assert_eq!(flat_index_at(&rows, 2, 1), 5);
        assert_eq!(
            find_flat(&rows, |c, _| c.result.reference == "John 3:2"),
            Some(4)
        );
    }

    #[test]
    fn arrows_move_through_the_grid_and_clamp_between_rows() {
        let rows = rows();
        assert_eq!(grid_move(&rows, 0, GridKey::Left), None);
        assert_eq!(grid_move(&rows, 2, GridKey::Right), Some(3));
        assert_eq!(
            grid_move(&rows, 2, GridKey::Down),
            Some(3),
            "column clamps to the shorter row"
        );
        assert_eq!(grid_move(&rows, 4, GridKey::Up), Some(3));
        assert_eq!(grid_move(&rows, 0, GridKey::Up), None);
        assert_eq!(grid_move(&rows, 5, GridKey::Down), None);
        assert_eq!(grid_move(&rows, 5, GridKey::Right), None);
    }

    #[test]
    fn recent_lookups_read_both_forms_and_keep_ten() {
        let stored = json!("[\"John 3:16\",4,\"Psalm 23\"]");
        assert_eq!(
            read_recent_lookups(Some(&stored)),
            vec!["John 3:16", "Psalm 23"]
        );
        assert!(read_recent_lookups(Some(&json!("not json"))).is_empty());
        assert!(read_recent_lookups(None).is_empty());
        let mut list: Vec<String> = (0..10).map(|i| format!("R{i}")).collect();
        list = remember_lookup(&list, "R5");
        assert_eq!(list[0], "R5");
        assert_eq!(list.len(), 10);
        list = remember_lookup(&list, "New");
        assert_eq!(list.len(), 10);
        assert_eq!(list[0], "New");
        assert_eq!(
            read_recent_lookups(Some(&recent_lookups_value(&list))),
            list,
            "stored as JSON text, as localStorage held it"
        );
    }

    #[test]
    fn the_bar_list_cycles_through_nothing_and_every_row() {
        assert_eq!(cycle_active(None, 2, true), Some(0));
        assert_eq!(cycle_active(Some(1), 2, true), None);
        assert_eq!(cycle_active(None, 2, false), Some(1));
        assert_eq!(cycle_active(None, 0, true), None);
        let books = vec![
            BookCompletion {
                value: "John ".into(),
                book: "John".into(),
                typed_book: "jn".into()
            };
            6
        ];
        let items = bar_items(false, &[], &books, false, &[result("John 3:16", &[16.])]);
        assert_eq!(
            items.len(),
            4,
            "four books; matches hidden while not current"
        );
        assert!(matches!(
            bar_items(true, &["A".into()], &books, true, &[])[0],
            BarItem::Recent(_)
        ));
    }

    #[test]
    fn highlight_prefers_the_whole_phrase() {
        let parts = highlight("Love is patient, love is kind", "love is");
        assert_eq!(parts[0], ("Love is".into(), true));
        assert_eq!(parts.iter().filter(|p| p.1).count(), 2);
        let words = highlight("Charity suffereth long", "suffer long");
        assert_eq!(
            words
                .iter()
                .filter(|p| p.1)
                .map(|p| p.0.as_str())
                .collect::<Vec<_>>(),
            ["suffer", "long"]
        );
        assert_eq!(highlight("abc", "a b"), vec![("abc".into(), false)]);
        assert_eq!(
            verse_text(&ScriptureResult {
                reference: "x".into(),
                translation: "KJV".into(),
                verses: vec![
                    serde_json::from_value(
                        json!({"book":"J","chapter":1.0,"verse":1.0,"text":"¶ In the"})
                    )
                    .unwrap()
                ]
            }),
            "In the"
        );
    }

    #[test]
    fn the_grid_breaks_into_lines_with_headers_for_several_passages() {
        let rows = rows();
        let lines = grid_lines(&rows, 2);
        assert_eq!(lines.len(), 3 + 2 + 1 + 1);
        assert_eq!(line_of_card(&lines, &rows, 2), Some(2));
        assert_eq!(line_of_row(&lines, 1), Some(3));
        let one = vec![rows[0].clone()];
        assert!(
            !grid_lines(&one, 4)
                .iter()
                .any(|l| matches!(l, GridLine::Header { .. }))
        );
        assert_eq!(columns(800.0, 180.0), 4);
        assert_eq!(columns(10.0, 180.0), 1);
    }

    #[test]
    fn a_large_playlist_builds_few_lines_for_the_screen() {
        // scripture-lazy-cards: 54 items × 7 verses = 378 cards.
        let rows: Vec<_> = (0..54)
            .map(|i| {
                create_result_row(
                    &result(&format!("John 3:{i}"), &[1., 2., 3., 4., 5., 6., 7.]),
                    RowOptions {
                        id: Some(format!("row-{i}")),
                        plan_item_id: Some(format!("item-{i}")),
                        ..Default::default()
                    },
                )
            })
            .collect();
        assert_eq!(card_count(&rows), 378);
        let cols = columns(1200.0 - 48.0, 288.0);
        let lines = grid_lines(&rows, cols);
        let h = card_height(card_width(1152.0, cols));
        let visible = lines
            .iter()
            .scan(0.0, |y, l| {
                let at = *y;
                *y += line_height(l, &rows, h);
                Some(at)
            })
            .filter(|y| *y < 700.0)
            .count();
        assert!(
            visible < lines.len() / 10,
            "{visible} of {} lines on screen",
            lines.len()
        );
    }

    #[test]
    fn scope_reads_chapters_and_verses() {
        let mut row = create_result_row(&result("John 3", &[1., 2.]), RowOptions::default());
        assert_eq!(search_scope(&row), Scope::Chapter);
        row.reference = "John 3:1-2".into();
        assert_eq!(search_scope(&row), Scope::Verse);
        row.id = "chapter-John 3".into();
        assert_eq!(search_scope(&row), Scope::Chapter);
    }
}
