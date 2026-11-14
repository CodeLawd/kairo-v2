//! What's New (`lib/whats-new.ts`): the changelog embedded at build time from
//! `data/changelog.json` (written by `tools/embed-changelog`), and which
//! entry to show after an update or on request.

use serde::Deserialize;
use std::cmp::Ordering;
use std::sync::OnceLock;

/// The public changelog page.
pub const CHANGELOG_URL: &str = "https://kairopresenter.com/changelog";

const DATA: &str = include_str!("../data/changelog.json");

/// A headline feature: a coloured mark, a short title, a sentence or two.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Highlight {
    /// A mark name (`operator`, `scripture`, … `tip`); unknown names fall back.
    pub icon: String,
    /// Title.
    pub title: String,
    /// Body.
    pub body: String,
}

/// One release.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ChangelogEntry {
    /// `1.0.33`.
    pub version: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// Title.
    pub title: String,
    /// Notes, shown as a list when there are no highlights.
    pub notes: Vec<String>,
    /// What the sheet leads with, if any.
    #[serde(default)]
    pub highlights: Vec<Highlight>,
}

/// The embedded changelog, newest first. A malformed embed is a build bug;
/// it reads as empty rather than taking the app down.
pub fn changelog() -> &'static [ChangelogEntry] {
    static ENTRIES: OnceLock<Vec<ChangelogEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| match serde_json::from_str(DATA) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::error!(%error, "[WhatsNew] embedded changelog is unreadable");
            Vec::new()
        }
    })
}

/// Compares dotted numeric versions (`1.0.9` < `1.0.10`). Parts that are
/// not numbers count as 0, as `parseInt(n) || 0` does.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |v: &str| -> Vec<i64> {
        v.split('.')
            .map(|n| {
                let digits: String = n
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                digits.parse::<i64>().unwrap_or(0)
            })
            .collect()
    };
    let (pa, pb) = (parse(a), parse(b));
    for i in 0..pa.len().max(pb.len()) {
        let d = pa
            .get(i)
            .copied()
            .unwrap_or(0)
            .cmp(&pb.get(i).copied().unwrap_or(0));
        if d != Ordering::Equal {
            return d;
        }
    }
    Ordering::Equal
}

/// The entry to show after updating from `last_seen` to `current`: the
/// newest in `(last_seen, current]`. `None` on a fresh install (empty
/// `last_seen`) or when no release in between has an entry.
pub fn entry_after_update<'a>(
    last_seen: &str,
    current: &str,
    entries: &'a [ChangelogEntry],
) -> Option<&'a ChangelogEntry> {
    if last_seen.is_empty() || compare_versions(current, last_seen) != Ordering::Greater {
        return None;
    }
    entries.iter().find(|e| {
        compare_versions(&e.version, last_seen) == Ordering::Greater
            && compare_versions(&e.version, current) != Ordering::Greater
    })
}

/// What the menu item shows: the newest entry not after `current`. In
/// development the top entry, even unreleased, so a draft can be previewed.
pub fn entry_on_request<'a>(
    current: &str,
    dev: bool,
    entries: &'a [ChangelogEntry],
) -> Option<&'a ChangelogEntry> {
    if dev {
        return entries.first();
    }
    entries
        .iter()
        .find(|e| compare_versions(&e.version, current) != Ordering::Greater)
}

/// `2026-11-08` as `November 8, 2026`; anything else unchanged.
pub fn format_date(iso: &str) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let parts: Vec<&str> = iso.split('-').collect();
    if let [y, m, d] = parts[..]
        && let (Ok(y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<usize>(), d.parse::<u32>())
        && (1..=12).contains(&m)
        && (1..=31).contains(&d)
    {
        return format!("{} {d}, {y}", MONTHS[m - 1]);
    }
    iso.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(v: &str) -> ChangelogEntry {
        ChangelogEntry {
            version: v.into(),
            date: "2026-01-01".into(),
            title: v.into(),
            notes: vec![],
            highlights: vec![],
        }
    }

    #[test]
    fn the_embedded_changelog_reads_newest_first() {
        let entries = changelog();
        assert!(!entries.is_empty());
        assert!(
            entries
                .windows(2)
                .all(|w| compare_versions(&w[0].version, &w[1].version) != Ordering::Less)
        );
    }

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(compare_versions("1.0.9", "1.0.10"), Ordering::Less);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("2.x", "2.0"), Ordering::Equal);
    }

    #[test]
    fn after_update_picks_the_newest_released_in_range() {
        let entries = [e("1.0.33"), e("1.0.31"), e("1.0.30")];
        assert_eq!(entry_after_update("", "1.0.33", &entries), None);
        assert_eq!(entry_after_update("1.0.33", "1.0.33", &entries), None);
        assert_eq!(
            entry_after_update("1.0.30", "1.0.32", &entries)
                .unwrap()
                .version,
            "1.0.31"
        );
        assert_eq!(entry_after_update("1.0.31", "1.0.32", &entries), None);
    }

    #[test]
    fn on_request_shows_released_or_draft() {
        let entries = [e("1.0.33"), e("1.0.31")];
        assert_eq!(
            entry_on_request("1.0.32", false, &entries).unwrap().version,
            "1.0.31"
        );
        assert_eq!(
            entry_on_request("1.0.32", true, &entries).unwrap().version,
            "1.0.33"
        );
        assert_eq!(entry_on_request("0.1", false, &entries), None);
    }

    #[test]
    fn dates_read_in_words() {
        assert_eq!(format_date("2026-11-08"), "November 8, 2026");
        assert_eq!(format_date("soon"), "soon");
    }
}
