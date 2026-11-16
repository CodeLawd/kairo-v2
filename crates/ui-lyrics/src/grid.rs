//! Pure rules of the song view's slide grid (`SectionSlideGrid`,
//! `SongDivider`, the arrow and hotkey handlers in `Lyrics.tsx`), tested
//! without a window.

use kairo_domain::lyrics::slides::{ColoredLyricLine, section_colored_slide_chunks};
use kairo_domain::lyrics::{LyricsSectionType, LyricsSong, SongPresentOptions};

/// Slide tile size range, in percent (`SLIDE_ZOOM_MIN`/`MAX`).
pub const ZOOM_MIN: f32 = 60.;
pub const ZOOM_MAX: f32 = 200.;
pub const ZOOM_DEFAULT: f32 = 100.;
/// The tile's minimum width at 100 % (`--lyrics-card-min`).
pub const CARD_MIN: f32 = 240.;
/// Grid gap (`.lyrics-slide-grid { gap: 8px }`).
pub const GAP: f32 = 8.;

/// One card of the grid.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideCard {
    /// Zero-based slide across the whole song.
    pub index: usize,
    /// The section it belongs to.
    pub section: usize,
    /// First slide of its section: carries the label and the hotkey.
    pub first: bool,
    pub kind: LyricsSectionType,
    pub label: String,
    pub hotkey: Option<String>,
    pub lines: Vec<ColoredLyricLine>,
}

/// An empty section has no cards but keeps a heading (and stays droppable).
#[derive(Clone, Debug, PartialEq)]
pub enum GridItem {
    Card(SlideCard),
    EmptySection {
        section: usize,
        kind: LyricsSectionType,
        label: String,
    },
}

/// Every card of a song, in push order, sections run together.
pub fn song_grid(song: &LyricsSong, gloss: &str) -> Vec<GridItem> {
    let opts = SongPresentOptions::default();
    let mut items = Vec::new();
    let mut next = 0;
    for (s, section) in song.sections.iter().enumerate() {
        let chunks = section_colored_slide_chunks(section, gloss, &opts);
        if chunks.is_empty() {
            items.push(GridItem::EmptySection {
                section: s,
                kind: section.kind,
                label: section.label.clone(),
            });
            continue;
        }
        for (i, lines) in chunks.into_iter().enumerate() {
            items.push(GridItem::Card(SlideCard {
                index: next,
                section: s,
                first: i == 0,
                kind: section.kind,
                label: section.label.clone(),
                hotkey: if i == 0 { section.hotkey.clone() } else { None },
                lines,
            }));
            next += 1;
        }
    }
    items
}

/// How many slides a song has.
pub fn slide_count(song: &LyricsSong) -> usize {
    let opts = SongPresentOptions::default();
    song.sections
        .iter()
        .map(|s| kairo_domain::lyrics::slides::section_slide_chunks(s, &opts).len())
        .sum()
}

/// The zero-based first slide of each section.
pub fn section_starts(song: &LyricsSong) -> Vec<usize> {
    let opts = SongPresentOptions::default();
    let mut next = 0;
    song.sections
        .iter()
        .map(|s| {
            let start = next;
            next += kairo_domain::lyrics::slides::section_slide_chunks(s, &opts).len();
            start
        })
        .collect()
}

/// `"N slide(s)"` on a playlist song's band.
pub fn slide_count_label(count: usize) -> String {
    format!("{count} slide{}", if count == 1 { "" } else { "s" })
}

/// Columns and card width for a grid `width` wide at `zoom`:
/// `repeat(auto-fill, minmax(min(100%, card), 1fr))`.
pub fn layout(width: f32, zoom: f32) -> (usize, f32) {
    let min = (CARD_MIN * zoom / 100.).min(width.max(1.));
    let columns = (((width + GAP) / (min + GAP)).floor() as usize).max(1);
    let card = (width - GAP * (columns as f32 - 1.)) / columns as f32;
    (columns, card.max(1.))
}

/// The tile's text size: 12/11/10 px by line count, grown with the tile.
pub fn text_size(lines: usize, zoom: f32) -> f32 {
    let base = match lines {
        0..=2 => 12.,
        3 => 11.,
        _ => 10.,
    };
    base * zoom / 100.
}

/// Where an arrow lands (`navigate` in `Lyrics.tsx`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The song to push.
    pub song: String,
    pub slide: usize,
}

/// One arrow press. `live` is the song and slide on screen (if any);
/// `selected` the open song; `playlist` the songs laid out, in order, with
/// their slide counts. Stepping off either end of a song carries on into the
/// next (first slide) or previous (last slide) song that has slides.
pub fn step(
    live: Option<(&str, usize)>,
    selected: &str,
    playlist: &[(String, usize)],
    count_of: impl Fn(&str) -> usize,
    forward: bool,
) -> Option<Step> {
    // Arrows step from what is live (a song scrolled out of view, or not on
    // screen at all) and fall back to the selected song.
    let live_song = live.map(|(id, _)| id).unwrap_or(selected);
    let current = live.filter(|(id, _)| *id == live_song).map(|(_, s)| s);
    let count = count_of(live_song);
    if let Some(cur) = current {
        let off = if forward { cur + 1 >= count } else { cur == 0 };
        if off && let Some(at) = playlist.iter().position(|(id, _)| id == live_song) {
            let mut i = at as isize + if forward { 1 } else { -1 };
            while i >= 0 && (i as usize) < playlist.len() {
                let (id, n) = &playlist[i as usize];
                if *n > 0 {
                    return Some(Step {
                        song: id.clone(),
                        slide: if forward { 0 } else { n - 1 },
                    });
                }
                i += if forward { 1 } else { -1 };
            }
        }
    }
    if count == 0 {
        return None;
    }
    let next = match current {
        None => 0,
        Some(c) if forward => (c + 1).min(count - 1),
        Some(c) => c.saturating_sub(1),
    };
    (current != Some(next)).then(|| Step {
        song: live_song.to_owned(),
        slide: next,
    })
}

/// The hint line under the grid.
pub fn hint(painting: bool, selecting: bool, picked: usize) -> &'static str {
    if painting {
        "Click a lyric line to paint · Esc turns paint off"
    } else if selecting || picked > 0 {
        "Right-click a selected slide to group or delete them all · Shift-click selects a range · Esc to cancel"
    } else {
        "Click a slide to go live · ⌘-click to select · ← → previous / next"
    }
}

/// `toTxt`: the whole song as plain text (Copy lyrics).
pub fn song_text(song: &LyricsSong) -> String {
    let mut lines = vec![format!("Title: {}", song.title)];
    if !song.artist.is_empty() {
        lines.push(format!("Artist: {}", song.artist));
    }
    if let Some(c) = song.copyright.as_deref().filter(|c| !c.is_empty()) {
        lines.push(format!("Copyright: {c}"));
    }
    if let Some(c) = song.ccli_number.as_deref().filter(|c| !c.is_empty()) {
        lines.push(format!("CCLI: {c}"));
    }
    lines.push(String::new());
    for s in &song.sections {
        lines.push(format!("[{}]", s.label));
        lines.extend(s.lines.iter().cloned());
        lines.push(String::new());
    }
    lines.join("\n").trim_end().to_owned()
}

/// `KEYS` in `SlideContextMenu.tsx`.
pub const HOTKEYS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890";

/// `SLIDE_LABEL_CHOICES`: the labels offered when grouping slides.
pub fn label_choices() -> Vec<kairo_domain::lyrics::reorder::SlideLabel> {
    use LyricsSectionType::*;
    [
        (Verse, "Verse 1"),
        (Verse, "Verse 2"),
        (Verse, "Verse 3"),
        (Verse, "Verse 4"),
        (PreChorus, "Pre-Chorus"),
        (Chorus, "Chorus"),
        (Chorus, "Chorus 2"),
        (Bridge, "Bridge"),
        (Tag, "Tag"),
        (Intro, "Intro"),
        (Outro, "Outro"),
        (Ending, "Ending"),
    ]
    .into_iter()
    .map(|(kind, label)| kairo_domain::lyrics::reorder::SlideLabel {
        kind,
        label: label.into(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song(id: &str, sections: serde_json::Value) -> LyricsSong {
        serde_json::from_value(json!({
            "id": id, "title": id, "artist": "", "sections": sections,
            "createdAt": 0.0, "updatedAt": 0.0,
        }))
        .unwrap()
    }

    #[test]
    fn the_grid_runs_sections_together() {
        let s = song(
            "a",
            json!([
                {"type": "verse", "label": "Verse 1", "lines": ["one", "two", "", "three"], "hotkey": "V"},
                {"type": "chorus", "label": "Chorus", "lines": []},
                {"type": "bridge", "label": "Bridge", "lines": ["four"]}
            ]),
        );
        let grid = song_grid(&s, "#AABED7");
        let cards: Vec<(usize, usize, bool, Option<String>)> = grid
            .iter()
            .filter_map(|g| match g {
                GridItem::Card(c) => Some((c.index, c.section, c.first, c.hotkey.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            cards,
            [
                (0, 0, true, Some("V".into())),
                (1, 0, false, None),
                // A section without lines is one "Empty" card, as in Electron.
                (2, 1, true, None),
                (3, 2, true, None)
            ]
        );
        assert_eq!(slide_count(&s), 4);
        assert_eq!(section_starts(&s), [0, 2, 3]);
    }

    #[test]
    fn layout_fills_columns_like_auto_fill() {
        // 240 px cards at 100 % (DECISIONS 113).
        assert_eq!(layout(800., 100.).0, 3);
        assert_eq!(layout(100., 100.), (1, 100.));
        assert_eq!(layout(800., 200.).0, 1);
        let (c, w) = layout(1000., 60.);
        assert_eq!(c, 6);
        assert!((w * 6. + GAP * 5. - 1000.).abs() < 0.01);
    }

    #[test]
    fn arrows_step_within_and_across_playlist_songs() {
        let list = vec![("a".to_string(), 2), ("b".into(), 0), ("c".into(), 3)];
        let count = |id: &str| list.iter().find(|(i, _)| i == id).map_or(0, |(_, n)| *n);
        let s = |song: &str, slide| {
            Some(Step {
                song: song.into(),
                slide,
            })
        };
        assert_eq!(step(None, "a", &list, count, true), s("a", 0));
        assert_eq!(step(Some(("a", 0)), "a", &list, count, true), s("a", 1));
        // Off the end of "a": "b" has no slides, so "c" from its first.
        assert_eq!(step(Some(("a", 1)), "a", &list, count, true), s("c", 0));
        assert_eq!(step(Some(("c", 0)), "a", &list, count, false), s("a", 1));
        // At the very start: nothing before.
        assert_eq!(step(Some(("a", 0)), "a", &list, count, false), None);
        // Not in a playlist: clamps at the end.
        assert_eq!(step(Some(("c", 2)), "c", &[], count, true), None);
        // Live in a song not laid out: steps that song.
        assert_eq!(step(Some(("c", 1)), "a", &[], count, true), s("c", 2));
        assert_eq!(step(None, "b", &[], count, true), None);
    }

    #[test]
    fn copy_text_and_sizes() {
        let s = song(
            "Grace",
            json!([{"type": "verse", "label": "V1", "lines": ["a", "b"]}]),
        );
        assert_eq!(song_text(&s), "Title: Grace\n\n[V1]\na\nb");
        assert_eq!(text_size(2, 100.), 12.);
        assert_eq!(text_size(4, 200.), 20.);
        assert_eq!(slide_count_label(1), "1 slide");
    }
}
