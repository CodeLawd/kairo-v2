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
