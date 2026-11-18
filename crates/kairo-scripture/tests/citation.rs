use kairo_scripture::citation::{has_corrupted_scripture_citation, match_explicit_scriptures};

#[test]
fn recognizes_spoken_and_digit_citations() {
    for (text, book, chapter, verse, end) in [
        ("John 3:16", "John", 3.0, 16.0, None),
        (
            "Romans chapter 8. Look at verse 28.",
            "Romans",
            8.0,
            28.0,
            None,
        ),
        (
            "First Corinthians thirteen verses four through seven",
            "1 Corinthians",
            13.0,
            4.0,
            Some(7.0),
        ),
        ("Genesis fifty five", "Genesis", 50.0, 5.0, None),
        ("John 316", "John", 3.0, 16.0, None),
        (
            "John nineteen nineteen twenty eight to thirty",
            "John",
            19.0,
            28.0,
            Some(30.0),
        ),
        (
            "Mark chapter four verse 26 going to 29",
            "Mark",
            4.0,
            26.0,
            Some(29.0),
        ),
        ("Proverbs 5:15 up to 23", "Proverbs", 5.0, 15.0, Some(23.0)),
    ] {
        let refs = match_explicit_scriptures(text, true);
        assert_eq!(refs.len(), 1, "{text}");
        let r = &refs[0];
        assert_eq!(
            (r.book, r.chapter, r.verse_start, r.verse_end),
            (book, chapter, verse, end),
            "{text}"
        );
    }
}

#[test]
fn leaves_unsettled_fused_numbers_and_damaged_citations_unresolved() {
    assert!(match_explicit_scriptures("John 316", false).is_empty());
    assert!(match_explicit_scriptures("Genesis 111", true).is_empty());
    assert!(has_corrupted_scripture_citation(
        "Psalm 1one512-thirteen says"
    ));
    assert!(!has_corrupted_scripture_citation("Psalm twenty4"));
}

/// ASCII and non-ASCII transcripts take different compiled patterns; both
/// keep JavaScript's ASCII-only word boundary.
#[test]
fn non_ascii_text_matches_like_ascii_text() {
    let refs = |text: &str| -> Vec<(String, f64, f64)> {
        match_explicit_scriptures(text, false)
            .into_iter()
            .map(|c| (c.book.to_string(), c.chapter, c.verse_start))
            .collect()
    };
    let expected = vec![("John".to_string(), 3.0, 16.0)];
    assert_eq!(refs("as José said, John 3:16"), expected);
    assert_eq!(refs("as Jose said, John 3:16"), expected);
    // `é` is a word character to Unicode but not to JavaScript, so "éjohn"
    // still starts a word at "john" exactly as Electron sees it.
    assert_eq!(refs("éjohn 3:16"), refs("-john 3:16"));
}
