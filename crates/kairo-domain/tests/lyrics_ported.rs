//! The Electron test files for the lyrics modules, ported one test per
//! TypeScript test with the same name. Sources: `tests/lyrics-*.test.ts`,
//! `tests/rtf-text.test.ts` in `../kairo-presenter`. Tests of functions owned
//! by other prompts are listed in `native-migration/DECISIONS.md` (prompt 014).

use kairo_domain::lyrics::clipboard::*;
use kairo_domain::lyrics::duplicate::*;
use kairo_domain::lyrics::hotkeys::*;
use kairo_domain::lyrics::online_error::*;
use kairo_domain::lyrics::query::*;
use kairo_domain::lyrics::reflow::*;
use kairo_domain::lyrics::reorder::*;
use kairo_domain::lyrics::rtf::*;
use kairo_domain::lyrics::section_edit::*;
use kairo_domain::lyrics::section_json::*;
use kairo_domain::lyrics::slides::*;
use kairo_domain::lyrics::style::*;
use kairo_domain::lyrics::translate::*;
use kairo_domain::lyrics::{
    DuplicateMatch, DuplicateReason, LyricsSectionType as T, LyricsSong, LyricsSongSection,
    SongPresentOptions,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn section(value: Value) -> LyricsSongSection {
    serde_json::from_value(value).unwrap()
}

fn sections(value: Value) -> Vec<LyricsSongSection> {
    serde_json::from_value(value).unwrap()
}

fn song_of(sections: Vec<LyricsSongSection>) -> LyricsSong {
    LyricsSong {
        id: String::new(),
        title: String::new(),
        artist: String::new(),
        copyright: None,
        ccli_number: None,
        is_favorite: None,
        source: None,
        sections,
        created_at: 0.0,
        updated_at: 0.0,
        extra: Default::default(),
    }
}

fn pairs(v: &[(&str, &str)]) -> GlossPairs {
    v.iter()
        .map(|(k, g)| (k.to_string(), g.to_string()))
        .collect()
}

// ─── tests/lyrics-clipboard.test.ts ──────────────────────────────────────────

const GENIUS: &str = "37 Contributors\nTranslations\nWay Maker Lyrics\n[Verse 1]\nYou are here, moving in our midst\nI worship You, I worship You\n\n[Chorus]\nWay maker, miracle worker\nPromise keeper, light in the darkness\nYou might also like\nMy God, that is who You are12345Embed";

#[test]
fn parse_clipboard_song_lifts_the_title_out_of_the_genius_heading() {
    assert_eq!(parse_clipboard_song(GENIUS).unwrap().title, "Way Maker");
}

#[test]
fn parse_clipboard_song_drops_contributor_translation_and_promo_chrome() {
    let draft = parse_clipboard_song(GENIUS).unwrap();
    assert!(!draft.text.contains("Contributors"));
    assert!(!draft.text.contains("Translations"));
    assert!(!draft.text.contains("You might also like"));
}

#[test]
fn parse_clipboard_song_strips_the_trailing_embed_counter() {
    let draft = parse_clipboard_song(GENIUS).unwrap();
    assert!(
        draft
            .text
            .trim_end()
            .ends_with("My God, that is who You are")
    );
}

#[test]
fn parse_clipboard_song_keeps_section_headers_intact() {
    let draft = parse_clipboard_song(GENIUS).unwrap();
    assert!(draft.text.contains("[Verse 1]"));
    assert!(draft.text.contains("[Chorus]"));
}

#[test]
fn parse_clipboard_song_splits_an_artist_title_lyrics_heading() {
    let draft = parse_clipboard_song(
        "Sinach - Way Maker lyrics\nLine one\nLine two\nLine three\nLine four",
    )
    .unwrap();
    assert_eq!(draft.artist, "Sinach");
    assert_eq!(draft.title, "Way Maker");
}

#[test]
fn parse_clipboard_song_pulls_a_byline_and_a_copyright_line_out_of_the_body() {
    let draft = parse_clipboard_song(
        "Great Is Thy Faithfulness Lyrics\nby Thomas Chisholm\n© 1923 Hope Publishing\nLine one\nLine two\nLine three\nLine four",
    )
    .unwrap();
    assert_eq!(draft.artist, "Thomas Chisholm");
    assert_eq!(draft.copyright, "© 1923 Hope Publishing");
    assert!(!draft.text.contains("Hope Publishing"));
}

#[test]
fn parse_clipboard_song_counts_lyric_lines_not_section_headers() {
    assert_eq!(parse_clipboard_song(GENIUS).unwrap().line_count, 5);
}

#[test]
fn parse_clipboard_song_returns_null_when_nothing_survives_cleaning() {
    assert_eq!(
        parse_clipboard_song("37 Contributors\nTranslations\n"),
        None
    );
    assert_eq!(parse_clipboard_song("   "), None);
}

#[test]
fn looks_like_lyrics_rejects_a_bare_url_or_a_one_liner() {
    assert!(!looks_like_lyrics("https://genius.com/some-song-lyrics"));
    assert!(!looks_like_lyrics("Way Maker"));
    assert!(looks_like_lyrics(GENIUS));
}

fn lifted(title: &str, artist: &str, text: &str) -> LiftedHeading {
    LiftedHeading {
        title: title.into(),
        artist: artist.into(),
        text: text.into(),
    }
}

#[test]
fn a_short_opening_line_above_a_blank_line_is_the_title() {
    assert_eq!(
        lift_heading_title("Way Maker\n\nYou are here\nMoving in our midst"),
        lifted("Way Maker", "", "You are here\nMoving in our midst")
    );
}

#[test]
fn a_title_and_an_artist_stacked_above_a_blank_line_are_both_lifted() {
    assert_eq!(
        lift_heading_title("WAY MAKER\nSinach\n\nVerse 1\nYou are here"),
        lifted("WAY MAKER", "Sinach", "Verse 1\nYou are here")
    );
}

#[test]
fn headings_that_are_not_titles_leave_the_text_untouched() {
    // Five TypeScript tests: lyrics straight away, a section header, a long
    // first line, nothing after the heading, two lines with no blank.
    for input in [
        "You are here moving\nin our midst",
        "[Verse 1]\n\nYou are here",
        "I will sing of the mercies of the Lord forever and ever more amen\n\nAnd again",
        "Way Maker\n\n",
        "Jesus you are\nthe one I love\nforever more",
    ] {
        assert_eq!(
            lift_heading_title(input),
            lifted("", "", input),
            "{input:?}"
        );
    }
}

#[test]
fn a_songselect_style_header_is_recognised_with_either_separator() {
    assert!(has_song_metadata(
        "Title=Way Maker\nAuthor=Sinach\n\nYou are here"
    ));
    assert!(has_song_metadata("Title: Way Maker\n\nYou are here"));
    assert!(has_song_metadata(
        "CCLI Song Number: 7115744\n\nYou are here"
    ));
}

#[test]
fn a_plain_lyrics_file_has_no_header() {
    assert!(!has_song_metadata(
        "Way Maker\n\nYou are here\nMoving in our midst"
    ));
    assert!(!has_song_metadata("[Verse 1]\nYou are here"));
}

#[test]
fn a_lyric_line_that_happens_to_contain_a_colon_is_not_a_header() {
    assert!(!has_song_metadata("Chorus: sing it loud\nAgain and again"));
}

#[test]
fn a_copyright_line_at_the_foot_of_a_sheet_does_not_make_it_an_export() {
    assert!(!has_song_metadata(
        "Way Maker\n\nYou are here\n\n\n\n\n\n\nCopyright: 2015 Integrity"
    ));
}

// ─── tests/lyrics-duplicate.test.ts ──────────────────────────────────────────

fn song(over: Value) -> LyricsSong {
    let mut base = json!({
        "id": "song-1",
        "title": "Placeholder Song",
        "artist": "Placeholder Artist",
        "sections": [{
            "type": "verse",
            "label": "Verse 1",
            "lines": ["First line here", "Second line here", "Third line here", "Fourth line here"],
        }],
        "createdAt": 1,
        "updatedAt": 1,
    });
    for (k, v) in over.as_object().unwrap() {
        base[k] = v.clone();
    }
    serde_json::from_value(base).unwrap()
}

fn verse(lines: &[&str]) -> Value {
    json!([{ "type": "verse", "label": "Verse 1", "lines": lines }])
}

#[test]
fn the_same_title_and_artist_is_a_duplicate_whatever_the_casing() {
    let found = find_duplicate(
        &song(json!({"id": "new", "title": "WAY MAKER", "artist": "sinach"})),
        &[song(
            json!({"id": "old", "title": "Way Maker", "artist": "Sinach"}),
        )],
    )
    .unwrap();
    assert_eq!(found.song_id, "old");
    assert_eq!(found.reason, DuplicateReason::TitleArtist);
}

#[test]
fn a_live_suffix_or_a_featured_credit_does_not_make_it_a_different_song() {
    assert_eq!(
        song_key("Way Maker (Live)", "Sinach"),
        song_key("Way Maker", "Sinach")
    );
    assert_eq!(
        song_key("Way Maker", "Sinach feat. Someone"),
        song_key("Way Maker", "Sinach")
    );
}

#[test]
fn the_same_lyrics_under_a_different_title_are_caught() {
    let lines = [
        "Ide mi ja",
        "Halleluyah mo ti de Zion",
        "Ide mi ma ja o",
        "We call him Jehovah",
    ];
    let existing =
        song(json!({"id": "old", "title": "Ide mi ja", "artist": "", "sections": verse(&lines)}));
    let mut more = lines.to_vec();
    more.push("Iron breaker");
    let incoming =
        song(json!({"id": "new", "title": "Untitled 4", "artist": "", "sections": verse(&more)}));
    let found = find_duplicate(&incoming, &[existing]).unwrap();
    assert_eq!(found.song_id, "old");
    assert_eq!(found.reason, DuplicateReason::Lyrics);
}

#[test]
fn punctuation_and_diacritics_do_not_hide_a_lyric_match() {
    let a = song(
        json!({"id": "old", "title": "A", "artist": "", "sections": verse(&["Ese! Ese! Ese o", "Modupe o", "Baba mi", "Olorun mi"])}),
    );
    let b = song(
        json!({"id": "new", "title": "B", "artist": "", "sections": verse(&["Èsé, èsé, èsé o", "Modupe o", "Baba mi", "Olorun mi"])}),
    );
    assert_eq!(
        find_duplicate(&b, &[a]).unwrap().reason,
        DuplicateReason::Lyrics
    );
}

#[test]
fn a_ccli_number_settles_it_before_anything_else_is_considered() {
    let found = find_duplicate(
        &song(
            json!({"id": "new", "title": "Completely Different Name", "artist": "Someone Else", "ccliNumber": "7115744"}),
        ),
        &[song(json!({"id": "old", "ccliNumber": "7115744"}))],
    );
    assert_eq!(found.unwrap().reason, DuplicateReason::Ccli);
}

#[test]
fn two_different_songs_are_not_duplicates() {
    let other = song(
        json!({"id": "old", "title": "Another Song", "artist": "Another Artist", "sections": verse(&["Nothing", "In", "Common", "Here"])}),
    );
    assert_eq!(
        find_duplicate(
            &song(json!({"id": "new", "title": "Way Maker", "artist": "Sinach"})),
            &[other]
        ),
        None
    );
}

#[test]
fn a_short_chorus_is_matched_on_title_only_never_on_its_handful_of_lines() {
    let chorus = json!([{ "type": "chorus", "label": "Chorus", "lines": ["Hallelujah", "Amen"] }]);
    let a = song(json!({"id": "old", "title": "Chorus A", "artist": "", "sections": chorus}));
    let b = song(json!({"id": "new", "title": "Chorus B", "artist": "", "sections": chorus}));
    assert_eq!(find_duplicate(&b, &[a]), None);
}

#[test]
fn an_empty_library_has_no_duplicates_in_it() {
    assert_eq!(find_duplicate(&song(json!({})), &[]), None);
}

#[test]
fn containment_measures_the_smaller_set_against_the_larger() {
    let set = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<HashSet<_>>();
    assert_eq!(
        containment(&set(&["a", "b"]), &set(&["a", "b", "c", "d"])),
        1.0
    );
    assert_eq!(containment(&set(&["a", "x"]), &set(&["a", "b"])), 0.5);
    assert_eq!(containment(&set(&[]), &set(&["a"])), 0.0);
}

#[test]
fn the_badge_says_which_signal_matched() {
    let m = |title: &str, artist: &str, reason| DuplicateMatch {
        song_id: "x".into(),
        title: title.into(),
        artist: artist.into(),
        reason,
    };
    assert!(
        describe_duplicate(&m("Way Maker", "Sinach", DuplicateReason::Lyrics))
            .contains("Same lyrics as \"Way Maker\" by Sinach")
    );
    assert!(
        describe_duplicate(&m("Ide mi ja", "", DuplicateReason::TitleArtist))
            .ends_with("Same title and artist as \"Ide mi ja\"")
    );
}

#[test]
fn two_songs_both_called_untitled_are_not_the_same_song() {
    let a = song(
        json!({"id": "old", "title": "Untitled", "artist": "", "sections": verse(&["One", "Two", "Three", "Four"])}),
    );
    let b = song(
        json!({"id": "new", "title": "Untitled 3", "artist": "", "sections": verse(&["Five", "Six", "Seven", "Eight"])}),
    );
    assert_eq!(find_duplicate(&b, &[a]), None);
}

#[test]
fn placeholder_titles_still_match_on_their_lyrics() {
    let lines = ["One line", "Two line", "Three line", "Four line"];
    let a =
        song(json!({"id": "old", "title": "Untitled", "artist": "", "sections": verse(&lines)}));
    let b =
        song(json!({"id": "new", "title": "Untitled 3", "artist": "", "sections": verse(&lines)}));
    assert_eq!(
        find_duplicate(&b, &[a]).unwrap().reason,
        DuplicateReason::Lyrics
    );
}

// ─── tests/lyrics-hotkeys.test.ts ────────────────────────────────────────────

fn hk_section(kind: &str, label: &str, hotkey: Option<&str>) -> LyricsSongSection {
    let mut v = json!({
        "type": kind,
        "label": label,
        "lines": [format!("{label} line one"), "", format!("{label} line two")],
    });
    if let Some(k) = hotkey {
        v["hotkey"] = json!(k);
    }
    section(v)
}

fn keys(sections: &[LyricsSongSection]) -> Vec<Option<&str>> {
    sections.iter().map(|s| s.hotkey.as_deref()).collect()
}

#[test]
fn only_single_letters_and_digits_are_hotkeys_stored_upper_case() {
    assert_eq!(normalize_hotkey(Some("c")).as_deref(), Some("C"));
    assert_eq!(normalize_hotkey(Some(" 3 ")).as_deref(), Some("3"));
    assert_eq!(normalize_hotkey(Some("CC")), None);
    assert_eq!(normalize_hotkey(Some("!")), None);
    // `normalizeHotkey(7)`: a value that is not a string.
    assert_eq!(normalize_hotkey(None), None);
    assert_eq!(hotkey_from_event("a").as_deref(), Some("A"));
    assert_eq!(hotkey_from_event("ArrowLeft"), None);
    assert_eq!(hotkey_from_event(" "), None);
}

#[test]
fn auto_assign_follows_propresenter() {
    let result = auto_assign_hotkeys(&[
        hk_section("verse", "Verse 1", None),
        hk_section("chorus", "Chorus", None),
        hk_section("verse", "Verse 2", None),
        hk_section("chorus", "Chorus 2", None),
        hk_section("bridge", "Bridge", None),
        hk_section("tag", "Tag", Some("X")),
    ]);
    assert_eq!(
        keys(&result),
        vec![Some("1"), Some("C"), Some("2"), None, Some("B"), Some("X")]
    );
}

#[test]
fn auto_assign_never_steals_a_key_already_chosen_by_hand() {
    let result = auto_assign_hotkeys(&[
        hk_section("verse", "Verse 1", Some("C")),
        hk_section("chorus", "Chorus", None),
    ]);
    assert_eq!(keys(&result), vec![Some("C"), None]);
}

#[test]
fn giving_a_key_to_one_section_takes_it_off_the_other() {
    let result = set_section_hotkey(
        &[
            hk_section("verse", "Verse 1", Some("A")),
            hk_section("chorus", "Chorus", None),
        ],
        1,
        Some("a"),
    );
    assert_eq!(keys(&result), vec![None, Some("A")]);
    assert_eq!(section_for_hotkey(&result, "A"), Some(1));
    assert_eq!(section_for_hotkey(&result, "Z"), None);
    let cleared = set_section_hotkey(&result, 1, Some(""));
    assert_eq!(keys(&cleared), vec![None, None]);
}

#[test]
fn splitting_a_section_by_relabelling_keeps_its_hotkey_on_one_half_only() {
    let relabelled = label_lyric_slides(
        &[hk_section("verse", "Verse 1", Some("A"))],
        [1],
        &SlideLabel {
            kind: T::Verse,
            label: "Verse 2".into(),
        },
    );
    assert_eq!(
        relabelled
            .iter()
            .filter(|s| s.hotkey.as_deref() == Some("A"))
            .count(),
        1
    );
}

#[test]
fn hotkeys_survive_the_sqlite_section_format_and_plain_sections_stay_plain() {
    assert_eq!(
        serialize_section_lines(&s(&["a", "b"]), None, None),
        r#"["a","b"]"#
    );
    let stored = serialize_section_lines(&s(&["a", "b"]), None, Some("c"));
    assert_eq!(
        parse_section_lines(&stored),
        SectionLines {
            lines: s(&["a", "b"]),
            line_colors: None,
            hotkey: Some("C".into())
        }
    );
    let colored = parse_section_lines(&serialize_section_lines(
        &s(&["a"]),
        Some(&[Some("#ff0000".into())]),
        Some("2"),
    ));
    assert_eq!(
        colored,
        SectionLines {
            lines: s(&["a"]),
            line_colors: Some(vec![Some("#ff0000".into())]),
            hotkey: Some("2".into())
        }
    );
    assert_eq!(
        parse_section_lines(r#"["x"]"#),
        SectionLines {
            lines: s(&["x"]),
            ..Default::default()
        }
    );
}

// ─── tests/lyrics-reflow.test.ts ─────────────────────────────────────────────

#[test]
fn blank_lines_split_slides_each_slide_knows_the_line_it_starts_on() {
    let parsed = parse_reflow("[Verse 1]\nOne\nTwo\n\n\n\nThree\n");
    assert_eq!(
        parsed[0].slides,
        vec![
            ReflowSlide {
                start_line: 1,
                lines: s(&["One", "Two"])
            },
            ReflowSlide {
                start_line: 6,
                lines: s(&["Three"])
            },
        ]
    );
    assert_eq!(parsed[0].lines_text, "One\nTwo\n\nThree");
}

#[test]
fn lyrics_before_any_header_become_the_first_verse_type_follows_the_label() {
    let parsed = parse_reflow("Opening line\n\n[Pre-Chorus]\nRising\n[Bridge 2]\nTurn");
    let summary: Vec<_> = parsed
        .iter()
        .map(|s| (s.label.as_str(), s.kind, s.header_line))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("Verse 1", T::Verse, None),
            ("Pre-Chorus", T::PreChorus, Some(2)),
            ("Bridge 2", T::Bridge, Some(4))
        ]
    );
}

#[test]
fn an_empty_header_is_still_a_section() {
    let parsed = parse_reflow("[Chorus]\n\n[Verse 2]\nWords");
    assert_eq!(parsed[0].label, "Chorus");
    assert!(parsed[0].slides.is_empty());
}

#[test]
fn only_a_whole_line_in_brackets_is_a_header() {
    assert_eq!(
        reflow_header_label("  [Chorus]  ").as_deref(),
        Some("Chorus")
    );
    assert_eq!(reflow_header_label("[Chorus"), None);
    assert_eq!(reflow_header_label("Sing [loud] now"), None);
}

#[test]
fn the_next_free_label_verses_numbered_the_rest_bare_first() {
    let parsed = parse_reflow("[Verse 1]\nA\n\n[Chorus]\nB");
    let labels: Vec<&str> = parsed.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(next_section_label(&labels, T::Verse), "Verse 2");
    assert_eq!(next_section_label(&labels, T::Chorus), "Chorus 2");
    assert_eq!(next_section_label(&labels, T::Bridge), "Bridge");
}

// ─── tests/lyrics-reorder.test.ts ────────────────────────────────────────────

fn base_sections() -> Vec<LyricsSongSection> {
    sections(json!([
        { "type": "verse", "label": "Verse 1", "lines": ["First", "", "Second"], "lineColors": ["#FF0000", null, "#00FF00"] },
        { "type": "chorus", "label": "Chorus", "lines": ["Third"] },
    ]))
}

fn slide_texts(value: &[LyricsSongSection]) -> Vec<String> {
    build_slides(
        &song_of(value.to_vec()),
        &SongPresentOptions::default(),
        None,
    )
    .into_iter()
    .map(|slide| slide.lines.join(" "))
    .collect()
}

#[test]
fn moves_slides_in_either_direction_across_sections_without_changing_source() {
    let base = base_sections();
    let result = reorder_lyric_slide(&base, 0, 2);
    assert_eq!(slide_texts(&result), s(&["Second", "Third", "First"]));
    assert_eq!(result[2].label, "Verse 1");
    assert_eq!(
        result[2].line_colors.as_ref().unwrap()[0].as_deref(),
        Some("#FF0000")
    );
    assert_eq!(
        slide_texts(&reorder_lyric_slide(&base, 2, 0)),
        s(&["Third", "First", "Second"])
    );
    assert_eq!(slide_texts(&base), s(&["First", "Second", "Third"]));
}

#[test]
fn reorders_within_a_section_and_preserves_blank_slide_boundaries() {
    let result = reorder_lyric_slide(&base_sections(), 0, 1);
    assert_eq!(result[0].lines, s(&["Second", "", "First"]));
    assert_eq!(
        result[0].line_colors,
        Some(vec![Some("#00FF00".into()), None, Some("#FF0000".into())])
    );
}

#[test]
fn invalid_and_unchanged_moves_leave_sections_untouched() {
    let base = base_sections();
    assert_eq!(reorder_lyric_slide(&base, 1, 1), base);
    assert_eq!(reorder_lyric_slide(&base, -1, 0), base);
    assert_eq!(reorder_lyric_slide(&base, 0, 10), base);
}

#[test]
fn labelling_slides_splits_a_section_and_keeps_order_and_colours() {
    let long = sections(json!([
        { "type": "verse", "label": "Verse 1", "lines": ["A", "", "B", "", "C", "", "D"], "lineColors": [null, null, "#FF0000", null, null, null, null] },
    ]));
    let result = label_lyric_slides(
        &long,
        [1, 2],
        &SlideLabel {
            kind: T::Chorus,
            label: "Chorus".into(),
        },
    );
    let labels: Vec<&str> = result.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, vec!["Verse 1", "Chorus", "Verse 1"]);
    assert_eq!(result[1].lines, s(&["B", "", "C"]));
    assert_eq!(result[1].kind, T::Chorus);
    assert_eq!(
        result[1].line_colors.as_ref().unwrap()[0].as_deref(),
        Some("#FF0000")
    );
    assert_eq!(slide_texts(&result), s(&["A", "B", "C", "D"]));
}

#[test]
fn labelled_slides_merge_into_a_neighbouring_section_with_the_same_label() {
    let result = label_lyric_slides(
        &base_sections(),
        [1],
        &SlideLabel {
            kind: T::Chorus,
            label: "Chorus".into(),
        },
    );
    let labels: Vec<&str> = result.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, vec!["Verse 1", "Chorus"]);
    assert_eq!(result[1].lines, s(&["Second", "", "Third"]));
}

#[test]
fn labelling_nothing_valid_leaves_sections_untouched() {
    let base = base_sections();
    assert_eq!(
        label_lyric_slides(
            &base,
            [9],
            &SlideLabel {
                kind: T::Bridge,
                label: "Bridge".into()
            }
        ),
        base
    );
}

#[test]
fn a_slide_moved_into_another_section_joins_it_with_no_fragments_left_behind() {
    let song = sections(json!([
        { "type": "chorus", "label": "Chorus", "lines": ["C1", "", "C2"] },
        { "type": "verse", "label": "Verse 2", "lines": ["V1", "", "V2", "", "V3"] },
    ]));
    let result = move_lyric_slide(&song, 2, 1, Side::After);
    let labels: Vec<&str> = result.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, vec!["Chorus", "Verse 2"]);
    assert_eq!(result[0].lines, s(&["C1", "", "C2", "", "V1"]));
    assert_eq!(result[1].lines, s(&["V2", "", "V3"]));
}

#[test]
fn moving_heals_sections_already_split_into_same_label_fragments() {
    let fragmented = sections(json!([
        { "type": "verse", "label": "Verse 2", "lines": ["A"] },
        { "type": "verse", "label": "Verse 2", "lines": ["B"] },
        { "type": "verse", "label": "Verse 2", "lines": ["C"] },
    ]));
    let result = move_lyric_slide(&fragmented, 0, 2, Side::After);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].lines, s(&["B", "", "C", "", "A"]));
}

#[test]
fn dropping_a_slide_beside_itself_in_the_same_section_changes_nothing() {
    let base = base_sections();
    assert_eq!(move_lyric_slide(&base, 0, 0, Side::After), base);
    assert_eq!(move_lyric_slide(&base, 0, 9, Side::After), base);
}

#[test]
fn deleting_slides_removes_just_those_slides_and_rejoins_the_rest() {
    let base = sections(json!([
        { "type": "verse", "label": "Verse 1", "lines": ["a1", "", "a2", "", "a3"] },
        { "type": "chorus", "label": "Chorus", "lines": ["c1"], "hotkey": "C" },
        { "type": "verse", "label": "Verse 1", "lines": ["b1"] },
    ]));
    let one_gone = delete_lyric_slides(&base, [1]);
    let lines: Vec<_> = one_gone.iter().map(|s| s.lines.clone()).collect();
    assert_eq!(lines, vec![s(&["a1", "", "a3"]), s(&["c1"]), s(&["b1"])]);
    let chorus_gone = delete_lyric_slides(&base, [3]);
    assert_eq!(chorus_gone.len(), 1);
    assert_eq!(chorus_gone[0].label, "Verse 1");
    assert_eq!(
        chorus_gone[0].lines,
        s(&["a1", "", "a2", "", "a3", "", "b1"])
    );
    assert_eq!(delete_lyric_slides(&base, [99]), base);
}

#[test]
fn quick_edit_replaces_one_slide_keeping_colours_on_unchanged_lines() {
    let song = sections(json!([
        { "type": "verse", "label": "Verse 1", "lines": ["One", "Two", "", "Three"], "lineColors": ["#FF0000", null, null, "#00FF00"] },
    ]));
    assert_eq!(lyric_slide_lines(&song, 0), Some(s(&["One", "Two"])));
    assert_eq!(lyric_slide_lines(&song, 5), None);
    let edited = replace_lyric_slide(&song, 0, &s(&["One", "Two changed  ", ""]));
    assert_eq!(edited[0].lines, s(&["One", "Two changed", "", "Three"]));
    assert_eq!(
        edited[0].line_colors,
        Some(vec![
            Some("#FF0000".into()),
            None,
            None,
            Some("#00FF00".into())
        ])
    );
    assert_eq!(replace_lyric_slide(&song, 0, &s(&["  ", ""])), song);
}

// ─── tests/lyrics-section-edit.test.ts ───────────────────────────────────────

#[test]
fn split_text_at_cursor_starts_the_new_block_on_the_cursor_line() {
    assert_eq!(
        split_text_at_cursor("A\nB\nC", 3.0),
        ("A".into(), "B\nC".into())
    );
}

#[test]
fn split_text_at_cursor_at_start_leaves_before_empty() {
    assert_eq!(
        split_text_at_cursor("A\nB", 0.0),
        ("".into(), "A\nB".into())
    );
}

#[test]
fn expand_jammed_lines_splits_double_spaced_phrases() {
    assert_eq!(
        expand_jammed_lines("Ide na lolu le  Ide noku daji\n\nPower!"),
        "Ide na lolu le\nIde noku daji\n\nPower!"
    );
}

fn editable(key: &str, kind: T, label: &str, text: &str) -> EditableSection {
    EditableSection {
        key: key.into(),
        kind,
        label: label.into(),
        lines_text: text.into(),
    }
}

#[test]
fn split_section_at_cursor_inserts_a_typed_section_after_the_cursor_line() {
    let base = [editable(
        "a",
        T::Chorus,
        "Chorus",
        "Line one\nLine two\nLine three",
    )];
    let next = split_section_at_cursor(&base, 0, 9.0, || "k1".into(), Some(T::Bridge)).unwrap();
    assert_eq!(next.len(), 2);
    assert_eq!(next[0].lines_text, "Line one");
    assert_eq!(next[1].kind, T::Bridge);
    assert_eq!(next[1].label, "Bridge");
    assert_eq!(next[1].lines_text, "Line two\nLine three");
    assert_eq!(next[1].key, "k1");
}

#[test]
fn split_section_at_cursor_returns_null_when_nothing_follows_the_cursor() {
    let base = [editable("a", T::Verse, "Verse", "Only line")];
    assert_eq!(
        split_section_at_cursor(&base, 0, 9.0, || "x".into(), None),
        None
    );
}

#[test]
fn default_label_for_type_numbers_repeats() {
    assert_eq!(default_label_for_type(T::Verse, 1), "Verse");
    assert_eq!(default_label_for_type(T::Verse, 2), "Verse 2");
}

fn marked(kind: T, label: &str, lines: &[&str]) -> MarkedSection {
    MarkedSection {
        kind,
        label: label.into(),
        lines: s(lines),
    }
}

#[test]
fn marked_text_becomes_sections_labels_and_all() {
    assert_eq!(
        parse_marked_sections("[Verse 1]\nLine one\nLine two\n\n[Chorus]\nSing it"),
        vec![
            marked(T::Verse, "Verse 1", &["Line one", "Line two"]),
            marked(T::Chorus, "Chorus", &["Sing it"])
        ]
    );
}

#[test]
fn lyrics_before_the_first_marker_are_kept_as_a_verse_not_dropped() {
    let parsed = parse_marked_sections("Straight into the song\n\n[Chorus]\nSing it");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].kind, T::Verse);
    assert_eq!(parsed[0].lines, s(&["Straight into the song"]));
}

#[test]
fn blank_lines_inside_a_section_survive_they_are_the_slide_breaks() {
    assert_eq!(
        parse_marked_sections("[Verse 1]\nOne\n\nTwo")[0].lines,
        s(&["One", "", "Two"])
    );
}

#[test]
fn an_ordinal_does_not_change_the_section_type() {
    assert_eq!(section_type_from_label("Pre-Chorus 2"), T::PreChorus);
    assert_eq!(section_type_from_label("Chorus"), T::Chorus);
    assert_eq!(section_type_from_label("Something Else"), T::Verse);
}

#[test]
fn empty_text_produces_no_sections() {
    assert!(parse_marked_sections("").is_empty());
}

// ─── tests/lyrics-slides.test.ts ─────────────────────────────────────────────

#[test]
fn wrap_line_keeps_a_short_line_intact() {
    assert_eq!(wrap_line("Hello world", 40.0), s(&["Hello world"]));
}

#[test]
fn wrap_line_splits_on_word_boundaries() {
    assert_eq!(
        wrap_line("one two three four", 9.0),
        s(&["one two", "three", "four"])
    );
}

#[test]
fn preserve_slide_breaks_cases() {
    assert_eq!(
        preserve_slide_breaks(&["A", "", "B", "C"]),
        s(&["A", "", "B", "C"])
    );
    assert_eq!(preserve_slide_breaks(&["", "A", "B", ""]), s(&["A", "B"]));
    assert_eq!(
        preserve_slide_breaks(&["A", "", "", "B"]),
        s(&["A", "", "B"])
    );
}

#[test]
fn insert_slide_breaks_cases() {
    assert_eq!(insert_slide_breaks("A\nB\nC\nD", 2.0), "A\nB\n\nC\nD");
    assert_eq!(insert_slide_breaks("A\nB\nC", 1.0), "A\n\nB\n\nC");
    assert_eq!(insert_slide_breaks("A\n\nB\nC\nD", 2.0), "A\nB\n\nC\nD");
}

#[test]
fn apply_import_slide_breaks_cases() {
    assert_eq!(
        apply_import_slide_breaks(&["A", "B", "C", "D"], None),
        s(&["A", "B", "", "C", "D"])
    );
    assert_eq!(
        apply_import_slide_breaks(&["A", "", "B", "C", "D"], None),
        s(&["A", "", "B", "C", "D"])
    );
}

#[test]
fn split_into_slide_chunks_cases() {
    let chunks = |v: &[&str], max| split_into_slide_chunks(&s(v), Some(max));
    assert_eq!(
        chunks(&["A", "B", "C", "D"], 40.0),
        vec![s(&["A", "B", "C", "D"])]
    );
    assert_eq!(
        chunks(&["A", "B", "", "C", "D"], 40.0),
        vec![s(&["A", "B"]), s(&["C", "D"])]
    );
    assert_eq!(
        chunks(&["A", "", "B", "C"], 40.0),
        vec![s(&["A"]), s(&["B", "C"])]
    );
    assert_eq!(
        chunks(&["one two three four"], 9.0),
        vec![s(&["one two", "three", "four"])]
    );
}

// ─── tests/lyrics-style.test.ts ──────────────────────────────────────────────

#[test]
fn resolve_lyric_line_color_cases() {
    assert_eq!(
        resolve_lyric_line_color("(The arm of the Lord)", "#D4A017", None).as_deref(),
        Some("#D4A017")
    );
    assert_eq!(
        resolve_lyric_line_color("Aka aka ya", "#D4A017", None),
        None
    );
    assert_eq!(
        resolve_lyric_line_color("Aka aka ya", "#D4A017", Some("#FF0000")).as_deref(),
        Some("#FF0000")
    );
}

#[test]
fn normalize_gloss_color_falls_back_to_default() {
    assert_eq!(normalize_gloss_color(Some("")), DEFAULT_GLOSS_COLOR);
    assert_eq!(normalize_gloss_color(Some("#abc")), "#AABBCC");
}

#[test]
fn split_into_colored_slide_chunks_keeps_gloss_color_through_wrap() {
    let chunks = split_into_colored_slide_chunks(
        &s(&["Aka aka ya", "(The arm of the Lord)"]),
        None,
        "#D4A017",
        None,
    );
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0][0].color, None);
    assert_eq!(chunks[0][1].color.as_deref(), Some("#D4A017"));
}

// ─── tests/lyrics-query.test.ts ──────────────────────────────────────────────

fn ta(title: &str, artist: &str) -> TitleArtist {
    TitleArtist {
        title: title.into(),
        artist: artist.into(),
    }
}

#[test]
fn title_by_artist_splits_into_one_confident_pair() {
    let parts = parse_song_query("blessed be the name of the lord by dunsin oyekan");
    assert_eq!(
        parts.pairs,
        vec![ta("blessed be the name of the lord", "dunsin oyekan")]
    );
    assert_eq!(parts.titles, s(&["blessed be the name of the lord"]));
}

#[test]
fn a_dashed_query_yields_both_orders() {
    assert_eq!(
        parse_song_query("Sinach - Way Maker").pairs,
        vec![ta("Way Maker", "Sinach"), ta("Sinach", "Way Maker")]
    );
}

#[test]
fn colon_and_pipe_separators_split_the_same_way() {
    assert_eq!(parse_song_query("Nathaniel Bassey | Ese").pairs.len(), 2);
    assert_eq!(parse_song_query("Nathaniel Bassey: Ese").pairs.len(), 2);
}

#[test]
fn lyrics_and_other_noise_words_are_stripped_from_the_parts() {
    assert_eq!(
        parse_song_query("imole de lyrics by dunsin oyekan").pairs,
        vec![ta("imole de", "dunsin oyekan")]
    );
}

#[test]
fn a_plain_title_is_left_whole() {
    let parts = parse_song_query("blessed be the name of the lord");
    assert!(parts.pairs.is_empty());
    assert_eq!(parts.full, "blessed be the name of the lord");
}

#[test]
fn a_lyric_line_containing_the_word_by_is_not_mistaken_for_a_credit() {
    assert_eq!(
        parse_song_query("led by the spirit of").pairs,
        vec![ta("led", "the spirit of")]
    );
    assert!(parse_song_query("carried by").pairs.is_empty());
}

#[test]
fn variants_search_what_was_typed_first_then_the_title_alone() {
    let variants = query_variants("way maker by sinach");
    assert_eq!(variants[0], "way maker by sinach");
    assert!(variants.contains(&"way maker".to_string()));
}

#[test]
fn a_short_compound_title_is_also_tried_as_one_token() {
    assert!(query_variants("ami oluwa").contains(&"amioluwa".to_string()));
    assert!(
        !query_variants("blessed be the name of the lord")
            .iter()
            .any(|v| v.contains("blessedbe"))
    );
}

#[test]
fn variants_never_repeat_a_query() {
    let variants = query_variants("Ese - Ese");
    let unique: HashSet<String> = variants.iter().map(|v| v.to_lowercase()).collect();
    assert_eq!(unique.len(), variants.len());
}

#[test]
fn an_empty_query_produces_no_variants() {
    assert!(query_variants("   ").is_empty());
}

// ─── tests/lyrics-online-error.test.ts (formatOnlineLyricsError only) ────────

#[test]
fn format_online_lyrics_error_strips_electron_ipc_wrapper() {
    assert_eq!(
        format_online_lyrics_error(
            "Error invoking remote method 'lyrics:previewOnline': Error: Lyrics were fetched but no sections could be parsed."
        ),
        "This page didn't return usable lyrics."
    );
}

#[test]
fn format_online_lyrics_error_maps_empty_page_errors() {
    assert_eq!(
        format_online_lyrics_error("No lyric text was found on this page."),
        "This page didn't return usable lyrics."
    );
}

// ─── tests/lyrics-translate.test.ts ──────────────────────────────────────────

#[test]
fn is_gloss_line_matches_parenthetical_only_lines() {
    assert!(is_gloss_line("(Hello world)"));
    assert!(is_gloss_line("  (Hello)  "));
    assert!(!is_gloss_line("Hello (world)"));
    assert!(!is_gloss_line("()"));
}

#[test]
fn format_gloss_wraps_and_unwraps_nested_parens() {
    assert_eq!(format_gloss("Hello"), "(Hello)");
    assert_eq!(format_gloss("(Hello)"), "(Hello)");
    assert_eq!(format_gloss("((Hello))"), "(Hello)");
}

#[test]
fn should_translate_line_skips_blanks_markers_and_glosses() {
    assert!(should_translate_line("Odudu dabu Jesus no"));
    assert!(!should_translate_line(""));
    assert!(!should_translate_line("[Chorus]"));
    assert!(!should_translate_line("(Already glossed)"));
}

#[test]
fn collect_translatable_indices_lists_only_singable_lines() {
    assert_eq!(
        collect_translatable_indices(&s(&["[Verse]", "Line one", "", "Line two", "(old gloss)"])),
        vec![1, 3]
    );
}

fn glosses(v: &[(usize, &str)]) -> HashMap<usize, String> {
    v.iter().map(|(i, g)| (*i, g.to_string())).collect()
}

#[test]
fn apply_line_glosses_cases() {
    assert_eq!(
        apply_line_glosses(
            &s(&["Odudu dabu Jesus no", "Ide na lolu le"]),
            &glosses(&[
                (0, "There is no other Name like Jesus"),
                (1, "He made the lame walk")
            ])
        ),
        s(&[
            "Odudu dabu Jesus no",
            "(There is no other Name like Jesus)",
            "Ide na lolu le",
            "(He made the lame walk)"
        ])
    );
    assert_eq!(
        apply_line_glosses(&s(&["A", "", "B"]), &glosses(&[(0, "Alpha"), (2, "Beta")])),
        s(&["A", "(Alpha)", "", "B", "(Beta)"])
    );
    assert_eq!(
        apply_line_glosses(
            &s(&["Odudu dabu Jesus no", "(Old gloss)", "Next line"]),
            &glosses(&[(0, "New gloss"), (2, "Next")])
        ),
        s(&["Odudu dabu Jesus no", "(New gloss)", "Next line", "(Next)"])
    );
    assert_eq!(
        apply_line_glosses(&s(&["[Chorus]", "Sing this"]), &glosses(&[(1, "English")])),
        s(&["[Chorus]", "Sing this", "(English)"])
    );
}

#[test]
fn strip_line_glosses_removes_bilingual_glosses_and_keeps_originals() {
    assert_eq!(
        strip_line_glosses(&s(&[
            "Odudu dabu Jesus no",
            "(There is no other Name like Jesus)",
            "",
            "Power!",
            "(Power!)"
        ])),
        s(&["Odudu dabu Jesus no", "", "Power!"])
    );
}

#[test]
fn sections_have_glosses_detects_gloss_presence() {
    let a = s(&["Hello"]);
    let b = s(&["Hola", "(Hello)"]);
    assert!(!sections_have_glosses([a.as_slice()]));
    assert!(sections_have_glosses([b.as_slice()]));
}

#[test]
fn should_apply_gloss_cases() {
    assert!(!should_apply_gloss("Hello", "Hola", Some("en")));
    assert!(!should_apply_gloss("Hello", "Hello", Some("es")));
    assert!(should_apply_gloss("Odudu", "Spirit", Some("yo")));
    assert!(is_english_language_code(Some("en-GB")));
    assert!(!is_english_language_code(Some("yo")));
    assert!(is_likely_english_lyric("I see Jesus"));
    assert!(!is_likely_english_lyric("Ide na lolu le"));
    assert!(is_junk_gloss("Refrain"));
    assert!(!should_apply_gloss("I see Jesus", "Refrain", Some("und")));
    assert!(should_apply_gloss(
        "Ide na lolu le",
        "He made the lame walk",
        Some("und")
    ));
}

#[test]
fn lookup_online_gloss_matches_apostrophe_doubled_vowel_hymn_spellings() {
    let p = pairs(&[
        (
            "ija dopin oguun si tan",
            "The strife is o'er, the battle done;",
        ),
        ("omo olorun se gun ota", "The victory of life is won;"),
    ]);
    assert_eq!(
        lookup_online_gloss("Ija d'opin, ogun si'tan", &p).as_deref(),
        Some("The strife is o'er, the battle done;")
    );
    assert_eq!(
        gloss_match_key("Ija d'opin, ogun si'tan"),
        "ija dopin ogun sitan"
    );
}

#[test]
fn lookup_online_gloss_ignores_weak_substring_junk_matches() {
    let p = pairs(&[
        ("i see jesus x4", "Refrain"),
        ("ide na lolu le", "He made the lame walk"),
    ]);
    assert_eq!(lookup_online_gloss("I see Jesus", &p), None);
    assert_eq!(
        lookup_online_gloss("Ide na lolu le", &p).as_deref(),
        Some("He made the lame walk")
    );
}

#[test]
fn lookup_online_gloss_matches_shorter_slide_lines_to_longer_site_lines() {
    let p = pairs(&[
        (
            "odu jesus nana tumale chaka chaka",
            "The Name of Jesus is the greatest",
        ),
        ("e chego mano", "You are not the same as men"),
        ("odudu dabu jesus no", "There is no other Name like Jesus"),
    ]);
    assert_eq!(
        lookup_online_gloss("Odu Jesus nana tumale", &p).as_deref(),
        Some("The Name of Jesus is the greatest")
    );
    assert_eq!(
        lookup_online_gloss("E chego mano Jesus! E chego mano", &p).as_deref(),
        Some("You are not the same as men")
    );
}

#[test]
fn extract_inline_gloss_pairs_peels_glued_section_labels() {
    let p = extract_inline_gloss_pairs(
        "ChorusOdudu dabu Jesus No\n(no name like the name of Jesus)\nVerseIde na lolu le (He made the lame to walk)",
    );
    assert_eq!(
        lookup_online_gloss("Ide na lolu le", &p).as_deref(),
        Some("He made the lame to walk")
    );
}

#[test]
fn extract_inline_gloss_pairs_reads_lyric_site_parentheticals() {
    let p = extract_inline_gloss_pairs(
        "Odudu dabu Jesus no (There is no other Name like Jesus)\nIde na lolu le (He made the lame to walk)",
    );
    assert_eq!(
        lookup_online_gloss("Odudu dabu Jesus no", &p).as_deref(),
        Some("There is no other Name like Jesus")
    );
    assert_eq!(
        lookup_online_gloss("Ide na lolu le", &p).as_deref(),
        Some("He made the lame to walk")
    );
}

#[test]
fn expand_inline_glosses_to_lines_converts_end_of_line_glosses() {
    assert_eq!(
        expand_inline_glosses_to_lines("Odudu dabu Jesus no (There is no other Name like Jesus)"),
        "Odudu dabu Jesus no\n(There is no other Name like Jesus)"
    );
}

// ─── tests/rtf-text.test.ts ──────────────────────────────────────────────────

const TEXTEDIT: &str = "{\\rtf1\\ansi\\ansicpg1252\\cocoartf2758\n{\\fonttbl\\f0\\fswiss\\fcharset0 Helvetica;}\n{\\colortbl;\\red255\\green255\\blue255;}\n\\pard\\tx720\\partightenfactor0\n\\f0\\fs28 \\cf0 [Verse 1]\\\nLine one of the song\\\nLine two of the song\\\n\\\n[Chorus]\\\nThe chorus line\\\n}";

#[test]
fn the_body_text_comes_out_the_font_and_colour_tables_do_not() {
    let text = rtf_to_plain_text(TEXTEDIT);
    assert!(text.contains("Line one of the song"));
    assert!(!text.contains("Helvetica"));
    assert!(
        !["red255", "fonttbl", "colortbl"]
            .iter()
            .any(|w| text.contains(w))
    );
}

#[test]
fn paragraph_breaks_survive_because_sections_are_read_from_them() {
    let text = rtf_to_plain_text(TEXTEDIT);
    assert_eq!(
        text.split('\n').collect::<Vec<_>>(),
        vec![
            "[Verse 1]",
            "Line one of the song",
            "Line two of the song",
            "",
            "[Chorus]",
            "The chorus line"
        ]
    );
}

#[test]
fn par_is_a_line_break_too() {
    assert_eq!(
        rtf_to_plain_text(r"{\rtf1\ansi One\par Two\par Three}"),
        "One\nTwo\nThree"
    );
}

#[test]
fn words_curly_quotes_and_dashes_are_decoded_not_dropped() {
    assert_eq!(
        rtf_to_plain_text(r"{\rtf1\ansi Jesus\'92 name \'97 forever}"),
        "Jesus’ name — forever"
    );
}

#[test]
fn escaped_braces_and_backslashes_are_literal_text() {
    assert_eq!(
        rtf_to_plain_text(r"{\rtf1\ansi \{C\} 1 \\ 2}"),
        "{C} 1 \\ 2"
    );
}

#[test]
fn non_latin_lyrics_survive_as_unicode_escapes() {
    assert_eq!(rtf_to_plain_text(r"{\rtf1\ansi\uc1 \u200?s\u233?"), "Èsé");
}

#[test]
fn a_uc0_document_keeps_the_character_that_follows() {
    assert_eq!(rtf_to_plain_text(r"{\rtf1\ansi\uc0 \u200 se}"), "Èse");
}

#[test]
fn control_words_with_parameters_never_leak_into_the_lyrics() {
    assert_eq!(
        rtf_to_plain_text(r"{\rtf1\ansi\fs24\b\i Holy\b0\i0  Spirit\fs20}"),
        "Holy Spirit"
    );
}

#[test]
fn an_ignorable_destination_contributes_nothing() {
    assert_eq!(
        rtf_to_plain_text(
            r"{\rtf1\ansi{\*\generator Riched20 10.0}{\info{\title Draft}}Real lyric}"
        ),
        "Real lyric"
    );
}

#[test]
fn indentation_and_padding_collapse_to_single_spaces() {
    assert_eq!(rtf_to_plain_text(r"{\rtf1\ansi A\tab B     C}"), "A B C");
}

#[test]
fn input_that_is_not_rtf_returns_nothing_at_all() {
    assert_eq!(rtf_to_plain_text("[Verse 1]\nJust a text file"), "");
    assert_eq!(rtf_to_plain_text(""), "");
    assert!(looks_like_rtf("{\\rtf1\\ansi x}"));
    assert!(!looks_like_rtf("plain text"));
}

#[test]
fn an_ignorable_destination_is_skipped_whole() {
    let text = rtf_to_plain_text(
        "{\\rtf1\\ansi{\\*\\expandedcolortbl;;}{\\*\\somethingnew x}Sample Song Title\\\n\\\nFirst line}",
    );
    assert_eq!(
        text.split('\n').collect::<Vec<_>>(),
        vec!["Sample Song Title", "", "First line"]
    );
}
