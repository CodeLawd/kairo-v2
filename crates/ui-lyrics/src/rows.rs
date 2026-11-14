//! Pure rules of the Lyrics library, tested without a window: the list
//! filter and sort (`applyFilter`, `applySort`, `filteredSongs`), quick-open
//! ranking (`SongQuickOpen.tsx`), the review notice (`LyricsReviewNotice`),
//! setlist drop slots and the song usage dates (`SongUsageModal.tsx`).

use std::cmp::Ordering;
use std::collections::HashMap;

use kairo_domain::collections::{LibraryState, items_in_library};
use kairo_domain::lyrics::{
    LyricsOnlineResult, LyricsProvider, LyricsReview, LyricsSectionType, LyricsSong,
    UnverifiedReason,
};
use kairo_ui_kit::tokens::SectionKind;

/// Debounce before the full-text search (`setTimeout(..., 180)`).
pub const FTS_AFTER_MS: u64 = 180;
/// Shortest query the full-text search takes.
pub const MIN_FTS_QUERY: usize = 2;
/// Shortest query worth sending to the catalogues.
pub const MIN_ONLINE_QUERY: usize = 3;
/// How long a setlist row shows ✓ after a drop.
pub const LANDED_MS: u64 = 1200;
/// Days the usage report starts back from.
pub const USAGE_DAYS: i64 = 90;

/// `String.prototype.localeCompare` with the root collation.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    thread_local! {
        static COLLATOR: std::cell::RefCell<feruca::Collator> = std::cell::RefCell::new(
            feruca::Collator::new(feruca::Tailoring::Cldr(feruca::Locale::Root), false, false),
        );
    }
    COLLATOR.with(|c| c.borrow_mut().collate(a, b))
}

/// The rail source (`LibrarySource`; `recent` is kept for stored values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Favorites,
    Recent,
}

/// `SortType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Title,
    Artist,
    Recent,
    Added,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::Title, Sort::Artist, Sort::Recent, Sort::Added];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Title => "Title",
            Sort::Artist => "Artist",
            Sort::Recent => "Last modified",
            Sort::Added => "Date added",
        }
    }
}

fn favorite(song: &LyricsSong) -> bool {
    song.is_favorite == Some(true)
}

/// `applySort`. Stable, as `Array.prototype.sort` is.
pub fn sort_songs(songs: &mut [&LyricsSong], sort: Sort) {
    match sort {
        Sort::Title => songs.sort_by(|a, b| locale_compare(&a.title, &b.title)),
        Sort::Artist => songs.sort_by(|a, b| locale_compare(&a.artist, &b.artist)),
        Sort::Recent => songs.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at)),
        Sort::Added => songs.sort_by(|a, b| b.created_at.total_cmp(&a.created_at)),
    }
}

/// What narrows the list.
#[derive(Clone, Copy, Debug)]
pub struct ListScope<'a> {
    pub libraries: &'a LibraryState,
    pub active_library: &'a str,
    pub query: &'a str,
    /// Full-text hits for the query, once they arrive.
    pub fts: Option<&'a [LyricsSong]>,
    /// The setlist on screen (its song order), or `None` for a library view.
    pub setlist: Option<&'a [String]>,
    pub filter: Filter,
    pub sort: Sort,
}

/// `filteredSongs`: indices into `songs`, in the order shown.
pub fn filtered_songs(songs: &[LyricsSong], scope: ListScope<'_>) -> Vec<usize> {
    let index: HashMap<&str, usize> = songs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();
    // The library narrows the pool first.
    let mut pool: Vec<&LyricsSong> =
        items_in_library(scope.libraries, songs, scope.active_library, |s| &s.id);
    let q = scope.query.trim().to_lowercase();
    if !q.is_empty() {
        let mut matched: Vec<&LyricsSong> = pool
            .iter()
            .copied()
            .filter(|s| {
                s.title.to_lowercase().contains(&q)
                    || s.artist.to_lowercase().contains(&q)
                    || s.ccli_number.as_deref().unwrap_or("").contains(&q)
            })
            .collect();
        // Full-text hits add lyric-body matches within the pool.
        if let Some(hits) = scope.fts {
            for hit in hits {
                if matched.iter().any(|s| s.id == hit.id) {
                    continue;
                }
                if let Some(held) = pool.iter().find(|s| s.id == hit.id) {
                    matched.push(held);
                }
            }
        }
        pool = matched;
    }
    // A setlist is an order: never re-sorted, only searched within.
    if let Some(order) = scope.setlist {
        return order
            .iter()
            .filter(|id| pool.iter().any(|s| s.id == **id))
            .filter_map(|id| index.get(id.as_str()).copied())
            .collect();
    }
    if scope.filter == Filter::Favorites {
        pool.retain(|s| favorite(s));
    }
    let sort = if scope.filter == Filter::Recent {
        Sort::Recent
    } else {
        scope.sort
    };
    sort_songs(&mut pool, sort);
    pool.iter()
        .filter_map(|s| index.get(s.id.as_str()).copied())
        .collect()
}

/// What the list shows beneath (or instead of) its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListFooter {
    None,
    /// "No songs yet": empty and no query.
    NoSongs,
    /// "Searching library…".
    Searching,
    /// "Nothing in your library matches that." (query of 1–2 characters).
    NoMatch,
    /// The same, with "Search online".
    NoMatchSearchOnline,
    /// "Search online for more".
    SearchOnlineForMore,
    /// The web section (after the operator asked).
    Web,
}

/// The rules under the song list in `Lyrics.tsx`.
pub fn list_footer(
    shown: usize,
    query: &str,
    fts_searching: bool,
    online_requested: bool,
) -> ListFooter {
    let q = query.trim().chars().count();
    if shown == 0 && q == 0 {
        return ListFooter::NoSongs;
    }
    if q >= MIN_ONLINE_QUERY && online_requested {
        return ListFooter::Web;
    }
    if q == 0 {
        return ListFooter::None;
    }
    if shown == 0 {
        if fts_searching {
            return ListFooter::Searching;
        }
        return if q < MIN_ONLINE_QUERY {
            ListFooter::NoMatch
        } else {
            ListFooter::NoMatchSearchOnline
        };
    }
    if q >= MIN_ONLINE_QUERY {
        ListFooter::SearchOnlineForMore
    } else {
        ListFooter::None
    }
}

/// "N song(s)".
pub fn count_label(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// `providerLabel`.
pub fn provider_label(provider: LyricsProvider) -> &'static str {
    match provider {
        LyricsProvider::Lrclib => "LRCLIB",
        LyricsProvider::Web => "Web",
        LyricsProvider::Genius => "Genius",
    }
}

/// The web row's second line: artist, year and provider.
pub fn online_meta(result: &LyricsOnlineResult) -> String {
    let artist = if result.artist.is_empty() {
        "Unknown artist"
    } else {
        result.artist.as_str()
    };
    [
        Some(artist),
        result.release_year.as_deref().filter(|y| !y.is_empty()),
        Some(provider_label(result.provider)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

/// Lyrics of this hit cannot be imported automatically.
pub fn lyrics_unavailable(result: &LyricsOnlineResult) -> bool {
    result.lyrics_available == Some(false)
}

/// Copy when a web hit's lyrics cannot be imported (quick open).
pub const UNAVAILABLE_QUICK: &str = "Found online. Lyrics are not available for automatic import; open the source to view this song.";
/// The same, in the page's preview pane.
pub const UNAVAILABLE_PANE: &str = "Found online. Lyrics are not available for automatic import. Open the source, or paste lyrics to add the song.";
/// Quick open's empty web answer.
pub const NO_LYRICS_FOUND: &str = "Could not find available lyrics. Try the artist, another spelling, or paste lyrics to add the song.";

/// `LyricsReviewNotice`: reviewed is a quiet line; unverified is a box with
/// a title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewNotice {
    Reviewed(String),
    Unverified { title: String, detail: String },
}

pub fn review_notice(review: Option<&LyricsReview>) -> Option<ReviewNotice> {
    match review? {
        LyricsReview::Reviewed { removed_lines } => Some(ReviewNotice::Reviewed(format!(
            "Reviewed by DeepSeek{}. Check the lyrics before importing.",
            if *removed_lines > 0.0 {
                " · Unrelated page text removed"
            } else {
                ""
            }
        ))),
        LyricsReview::Unverified { reason } => {
            let detail = match reason {
                UnverifiedReason::MissingKey => {
                    "Activate this computer in Account → Licence & Devices to review online lyrics."
                }
                UnverifiedReason::TooLarge => "This source was too large to review safely.",
                _ => "DeepSeek could not complete the review.",
            };
            Some(ReviewNotice::Unverified {
                title: "Unverified lyrics".into(),
                detail: format!(
                    "{detail} The original text is shown; check for unrelated content before importing."
                ),
            })
        }
    }
}

/// `LyricsSectionType` as the token's section kind.
pub fn section_kind(kind: LyricsSectionType) -> SectionKind {
    match kind {
        LyricsSectionType::Verse => SectionKind::Verse,
        LyricsSectionType::Chorus => SectionKind::Chorus,
        LyricsSectionType::PreChorus => SectionKind::PreChorus,
        LyricsSectionType::Bridge => SectionKind::Bridge,
        LyricsSectionType::Tag => SectionKind::Tag,
        LyricsSectionType::Intro => SectionKind::Intro,
        LyricsSectionType::Outro => SectionKind::Outro,
        LyricsSectionType::Ending => SectionKind::Ending,
    }
}

/// The insert index for `dropIntoSetlist`: `insert_at` counts gaps in the
/// list as shown; a song already on it is moved, so its own slot is
/// discounted. `None` when nothing would change.
pub fn setlist_drop_index(song_ids: &[String], song_id: &str, insert_at: usize) -> Option<usize> {
    let current = song_ids.iter().position(|id| id == song_id);
    let index = match current {
        Some(c) if c < insert_at => insert_at - 1,
        _ => insert_at,
    };
    (current != Some(index)).then_some(index)
}

// ─── Quick open ─────────────────────────────────────────────────────────────

/// `searchable`: lower case, runs of non-letters and non-digits become one
/// space, trimmed.
pub fn searchable(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut gap = false;
    for c in value.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out
}

/// One local quick-open hit.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickHit {
    /// Index into the library.
    pub song: usize,
    /// The first lyric line that matched, when the match was in the lyrics.
    pub matched_lyric: Option<String>,
}

/// A song's searchable title, artist and lyric lines (with the originals).
type QuickEntry = (String, String, Vec<(String, String)>);

/// Searchable copies of a song, built once per library change.
#[derive(Clone, Debug, Default)]
pub struct QuickIndex {
    entries: Vec<QuickEntry>,
}

impl QuickIndex {
    pub fn new(songs: &[LyricsSong]) -> Self {
        Self {
            entries: songs
                .iter()
                .map(|s| {
                    (
                        searchable(&s.title),
                        searchable(&s.artist),
                        s.sections
                            .iter()
                            .flat_map(|sec| sec.lines.iter())
                            .map(|l| (searchable(l), l.clone()))
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

/// `results` in `SongQuickOpen`: everything A–Z with no query; otherwise
/// exact title, title prefix, title, artist, then lyric matches, ties A–Z.
pub fn quick_open(songs: &[LyricsSong], index: &QuickIndex, query: &str) -> Vec<QuickHit> {
    let needle = searchable(query);
    let mut ranked: Vec<(u8, QuickHit)> = songs
        .iter()
        .enumerate()
        .filter_map(|(i, _)| {
            let (title, artist, lines) = index.entries.get(i)?;
            if needle.is_empty() {
                return Some((
                    4,
                    QuickHit {
                        song: i,
                        matched_lyric: None,
                    },
                ));
            }
            let matched_lyric = lines
                .iter()
                .find(|(s, _)| s.contains(&needle))
                .map(|(_, l)| l.clone());
            let score = if *title == needle {
                0
            } else if title.starts_with(&needle) {
                1
            } else if title.contains(&needle) {
                2
            } else if artist.contains(&needle) {
                3
            } else if matched_lyric.is_some() {
                4
            } else {
                return None;
            };
            Some((
                score,
                QuickHit {
                    song: i,
                    matched_lyric,
                },
            ))
        })
        .collect();
    ranked.sort_by(|(sa, a), (sb, b)| {
        let titles = || locale_compare(&songs[a.song].title, &songs[b.song].title);
        if needle.is_empty() {
            titles()
        } else {
            sa.cmp(sb).then_with(titles)
        }
    });
    ranked.into_iter().map(|(_, hit)| hit).collect()
}

/// The count line over the results.
pub fn quick_count(query: &str, count: usize) -> String {
    if query.trim().is_empty() {
        count_label(count, "song")
    } else {
        format!("{count} {}", if count == 1 { "match" } else { "matches" })
    }
}

// ─── Song usage ─────────────────────────────────────────────────────────────

/// Parses `yyyy-mm-dd`.
pub fn parse_day(value: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

/// Epoch ms of local midnight at the start of `day` (`startOfDay`).
pub fn start_of_day(day: chrono::NaiveDate) -> f64 {
    use chrono::TimeZone;
    let midnight = day.and_hms_opt(0, 0, 0).unwrap_or_default();
    chrono::Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|t| t.timestamp_millis() as f64)
        .unwrap_or_else(|| midnight.and_utc().timestamp_millis() as f64)
}

/// The `[from, to)` range of the report: `to` takes its whole day.
pub fn usage_range(from: chrono::NaiveDate, to: chrono::NaiveDate) -> (f64, f64) {
    (
        start_of_day(from),
        start_of_day(to) + 24.0 * 60.0 * 60.0 * 1000.0,
    )
}

/// The footer line of the report.
pub fn usage_footer(saved: Option<&str>, missing_ccli: usize) -> String {
    if let Some(path) = saved {
        return format!("Saved to {path}");
    }
    if missing_ccli > 0 {
        return format!(
            "{missing_ccli} song{} no CCLI number — add it in the song editor.",
            if missing_ccli == 1 { " has" } else { "s have" }
        );
    }
    "One line per song per service, local dates.".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::collections::Library;
    use serde_json::json;

    fn song(id: &str, title: &str, artist: &str, at: f64) -> LyricsSong {
        serde_json::from_value(json!({
            "id": id, "title": title, "artist": artist,
            "sections": [{"type": "verse", "label": "Verse 1", "lines": [format!("{title} line one")]}],
            "createdAt": at, "updatedAt": 100.0 - at,
        }))
        .unwrap()
    }

    fn scope<'a>(libraries: &'a LibraryState) -> ListScope<'a> {
        ListScope {
            libraries,
            active_library: "default",
            query: "",
            fts: None,
            setlist: None,
            filter: Filter::All,
            sort: Sort::Title,
        }
    }

    #[test]
    fn the_list_sorts_filters_and_keeps_setlist_order() {
        let mut a = song("a", "Amazing Grace", "Newton", 1.0);
        a.is_favorite = Some(true);
        let songs = vec![
            song("c", "Cornerstone", "Hillsong", 3.0),
            a,
            song("b", "Blessed Be", "Redman", 2.0),
        ];
        let libraries = LibraryState::default();
        assert_eq!(filtered_songs(&songs, scope(&libraries)), vec![1, 2, 0]);
        let by_added = ListScope {
            sort: Sort::Added,
            ..scope(&libraries)
        };
        assert_eq!(filtered_songs(&songs, by_added), vec![0, 2, 1]);
        let favs = ListScope {
            filter: Filter::Favorites,
            ..scope(&libraries)
        };
        assert_eq!(filtered_songs(&songs, favs), vec![1]);
        let order = vec!["b".to_string(), "gone".into(), "c".into()];
        let list = ListScope {
            setlist: Some(&order),
            sort: Sort::Artist,
            ..scope(&libraries)
        };
        assert_eq!(filtered_songs(&songs, list), vec![2, 0]);
    }

    #[test]
    fn a_query_matches_metadata_then_adds_fts_hits_in_the_library() {
        let mut songs = vec![
            song("a", "Amazing Grace", "Newton", 1.0),
            song("b", "Blessed Be", "Redman", 2.0),
            song("c", "Cornerstone", "Hillsong", 3.0),
        ];
        songs[2].ccli_number = Some("12345".into());
        let mut libraries = LibraryState::default();
        libraries.libraries.push(Library {
            id: "x".into(),
            name: "X".into(),
            created_at: 0.0,
            updated_at: 0.0,
            extra: Default::default(),
        });
        libraries.assignments.insert("b".into(), "x".into());
        let q = ListScope {
            query: "  NEWTON ",
            ..scope(&libraries)
        };
        assert_eq!(filtered_songs(&songs, q), vec![0]);
        let ccli = ListScope {
            query: "234",
            ..scope(&libraries)
        };
        assert_eq!(filtered_songs(&songs, ccli), vec![2]);
        let hits = vec![songs[1].clone(), songs[2].clone()];
        let fts = ListScope {
            query: "zzz",
            fts: Some(&hits),
            active_library: "x",
            ..scope(&libraries)
        };
        // Only "b" is in library x.
        assert_eq!(filtered_songs(&songs, fts), vec![1]);
    }

    #[test]
    fn the_footer_follows_the_query_and_the_web_opt_in() {
        assert_eq!(list_footer(0, "", false, false), ListFooter::NoSongs);
        assert_eq!(list_footer(3, "", false, false), ListFooter::None);
        assert_eq!(list_footer(0, "ab", true, false), ListFooter::Searching);
        assert_eq!(list_footer(0, "ab", false, false), ListFooter::NoMatch);
        assert_eq!(
            list_footer(0, "abc", false, false),
            ListFooter::NoMatchSearchOnline
        );
        assert_eq!(
            list_footer(2, "abc", false, false),
            ListFooter::SearchOnlineForMore
        );
        assert_eq!(list_footer(2, "ab", false, false), ListFooter::None);
        assert_eq!(list_footer(0, "abc", false, true), ListFooter::Web);
    }

    #[test]
    fn drop_slots_discount_the_song_own_place() {
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(setlist_drop_index(&ids, "a", 3), Some(2));
        assert_eq!(setlist_drop_index(&ids, "a", 0), None);
        assert_eq!(setlist_drop_index(&ids, "a", 1), None);
        assert_eq!(setlist_drop_index(&ids, "c", 0), Some(0));
        assert_eq!(setlist_drop_index(&ids, "new", 1), Some(1));
    }

    #[test]
    fn quick_open_ranks_title_artist_then_lyrics() {
        let songs = vec![
            song("1", "Way Maker", "Sinach", 1.0),
            song("2", "Maker of All", "X", 2.0),
            song("3", "The Maker", "Y", 3.0),
            song("4", "Other", "The Makers", 4.0),
            song("5", "Zion", "Z", 5.0),
            song("6", "Maker", "Q", 6.0),
        ];
        let index = QuickIndex::new(&songs);
        let order: Vec<usize> = quick_open(&songs, &index, "maker")
            .into_iter()
            .map(|h| h.song)
            .collect();
        assert_eq!(order, vec![5, 1, 2, 0, 3]);
        let lyric = quick_open(&songs, &index, "zion line");
        assert_eq!(lyric[0].matched_lyric.as_deref(), Some("Zion line one"));
        let all: Vec<usize> = quick_open(&songs, &index, "  ")
            .into_iter()
            .map(|h| h.song)
            .collect();
        assert_eq!(all, vec![5, 1, 3, 2, 0, 4]);
        assert_eq!(searchable("  Way—Maker!! (Live) "), "way maker live");
        assert_eq!(quick_count("", 1), "1 song");
        assert_eq!(quick_count("x", 2), "2 matches");
    }

    #[test]
    fn review_notice_copy() {
        assert_eq!(review_notice(None), None);
        assert_eq!(
            review_notice(Some(&LyricsReview::Reviewed { removed_lines: 2.0 })),
            Some(ReviewNotice::Reviewed(
                "Reviewed by DeepSeek · Unrelated page text removed. Check the lyrics before importing."
                    .into()
            ))
        );
        let Some(ReviewNotice::Unverified { detail, .. }) =
            review_notice(Some(&LyricsReview::Unverified {
                reason: UnverifiedReason::MissingKey,
            }))
        else {
            panic!()
        };
        assert!(detail.starts_with("Activate this computer"));
    }

    #[test]
    fn usage_dates_and_footer() {
        let day = parse_day("2026-11-14").unwrap();
        let (from, to) = usage_range(day, day);
        assert_eq!(to - from, 86_400_000.0);
        assert!(parse_day("14/11/2026").is_none());
        assert_eq!(usage_footer(Some("/x.csv"), 3), "Saved to /x.csv");
        assert_eq!(
            usage_footer(None, 1),
            "1 song has no CCLI number — add it in the song editor."
        );
        assert_eq!(
            usage_footer(None, 0),
            "One line per song per service, local dates."
        );
    }
}
