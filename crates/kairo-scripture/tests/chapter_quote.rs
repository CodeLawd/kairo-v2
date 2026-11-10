use kairo_scripture::quote_recovery::{match_chapter_quote, tokenize_chapter};

#[test]
fn matches_unique_reading_across_translations() {
    let wording = "love patience kindness goodness faithfulness gentleness";
    let verses = tokenize_chapter([(1, "Jesus wept"), (2, wording), (2, wording)]);
    assert_eq!(match_chapter_quote(wording, &verses), Some((2, 0.85)));
    assert_eq!(
        match_chapter_quote("love patience kindness goodness", &verses),
        Some((2, 0.65))
    );
    assert_eq!(match_chapter_quote("love patience kindness", &verses), None);
}

#[test]
fn tolerates_one_wrong_word_but_rejects_ties() {
    let text = "love patience kindness mercy faithfulness gentleness holiness";
    let wording = "love patience kindness goodness faithfulness gentleness holiness";
    let verses = tokenize_chapter([(1, wording)]);
    assert_eq!(match_chapter_quote(text, &verses), Some((1, 0.65)));
    let tied = tokenize_chapter([(1, wording), (2, wording)]);
    assert_eq!(match_chapter_quote(text, &tied), None);
    assert_eq!(match_chapter_quote(wording, &tied), None);
}

#[test]
fn recovers_damaged_psalm_only_when_numbering_and_quote_agree() {
    use kairo_scripture::{quote_recovery::recover_damaged_quote, service::QuoteCandidate};

    let candidate = QuoteCandidate {
        book: "Psalms".into(),
        chapter: 115.0,
        verse: 12.0,
        text: "The Lord hath been mindful of us: he will bless us.".into(),
    };
    let quote = recover_damaged_quote(
        "Psalm 1one512-thirteen says, The Lord has been mindful of you. Put your name there.",
        |phrase| {
            assert_eq!(phrase, "The Lord has been mindful of you");
            vec![candidate.clone(), candidate.clone()]
        },
    )
    .unwrap();
    assert_eq!(quote.chapter, 115.0);
    assert_eq!(quote.verse_start, 12.0);
    assert_eq!(quote.verse_end, Some(13.0));

    for text in [
        "Psalm 1one512-thirteen says, The Lord",
        "Psalm 1one513-thirteen says, The Lord has been mindful of you.",
        "Psalm 1one512-hundred says, The Lord has been mindful of you.",
        "Psalm 1one512-thirteen says, The Lord is good to all people.",
    ] {
        assert_eq!(
            recover_damaged_quote(text, |_| vec![candidate.clone()]),
            None
        );
    }
}
