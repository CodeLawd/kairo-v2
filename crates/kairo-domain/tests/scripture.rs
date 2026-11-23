//! The Electron scripture tests (`tests/scripture-*.test.ts`,
//! `sermon-*.test.ts`, `spoken-translations`, `single-verse-presentation`,
//! `detector-recovery`, `api-bible-offline-state`, `offline-bible-view-model`)
//! ported assertion by assertion. `oracle_scripture.rs` holds the differential
//! comparisons. The reference parse benchmark is at the end:
//! `cargo test -p kairo-domain --release --test scripture -- --ignored --nocapture`

use kairo_domain::scripture::live_progress::*;
use kairo_domain::scripture::notes_review::*;
use kairo_domain::scripture::offline_state::{api_cache_messages as msg, *};
use kairo_domain::scripture::plan::*;
use kairo_domain::scripture::query::*;
use kairo_domain::scripture::small::*;
use kairo_domain::scripture::spoken_translations::find_spoken_translations;
use kairo_domain::scripture::trace::*;
use kairo_domain::scripture::{
    ApiBibleCacheStatus, ApiBibleDownloadProgress, ApiBibleOfflineTranslation, ScriptureResult,
    ScriptureSuggestion, ScriptureTranslationOption, SermonPlan, SermonReferenceMatch,
    SermonScriptureItem,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::Instant;

fn parse<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

fn norm(input: &str) -> Option<String> {
    normalize_scripture_query(input)
}

// ─── scripture-query.test.ts ──────────────────────────────────────────────────

#[test]
fn normalizes_references() {
    assert_eq!(norm("jos 1 5 9").as_deref(), Some("Joshua 1:5–9"));
    for (input, out) in [
        ("josh 1", "Joshua 1"),
        ("joshu 1:9", "Joshua 1:9"),
        ("genes 1:1", "Genesis 1:1"),
        ("philipp 4:6", "Philippians 4:6"),
        ("ez 1", "Ezra 1"),
        ("ezek 1", "Ezekiel 1"),
        ("Psalms 23", "Psalms 23"),
        ("ps 23", "Psalms 23"),
        ("psa 23", "Psalms 23"),
        ("pss 23", "Psalms 23"),
        ("psalm 23:1", "Psalms 23:1"),
        ("John 3:16-18", "John 3:16–18"),
        ("rom 8 28", "Romans 8:28"),
        ("1 cor 13 4 7", "1 Corinthians 13:4–7"),
        ("2tim 1 7", "2 Timothy 1:7"),
    ] {
        assert_eq!(norm(input).as_deref(), Some(out), "{input}");
    }
    // Edge cases: chapter bounds, empty and malformed input.
    for input in [
        "ps 0",
        "ps 151",
        "",
        "   ",
        "John",
        "John 3:",
        "John 3::16",
        "xyz 3:16",
        "jo 3:16",
    ] {
        assert_eq!(norm(input), None, "{input}");
    }
}

#[test]
fn book_completions() {
    assert_eq!(
        get_book_completion("josh"),
        Some(BookCompletion {
            value: "Joshua ".into(),
            book: "Joshua".into(),
            typed_book: "josh".into()
        })
    );
    assert_eq!(
        get_book_completion("rom 8"),
        Some(BookCompletion {
            value: "Romans 8".into(),
            book: "Romans".into(),
            typed_book: "rom".into()
        })
    );
    assert_eq!(get_book_completion("jo"), None);
    assert_eq!(get_book_completion("Joshua"), None);
    let values = |input| {
        get_book_completions(input)
            .into_iter()
            .map(|c| c.value)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        values("jo 1 2"),
        ["Job 1 2", "Joel 1 2", "John 1 2", "Jonah 1 2", "Joshua 1 2"]
    );
    assert_eq!(values("ez 1"), ["Ezra 1"]);
    assert!(values("").is_empty());
}

#[test]
fn phrase_and_live_suggest_rules() {
    assert!(is_likely_phrase_query("for God so loved"));
    assert!(!is_likely_phrase_query("jos 1 5 9"));
    assert!(!is_likely_phrase_query("John"));
    assert!(!is_likely_phrase_query("12 4 8"));
    assert!(should_live_suggest_scripture_query("love is patient"));
    assert!(should_live_suggest_scripture_query("John 3:16"));
    assert!(should_live_suggest_scripture_query("rom 8"));
    assert!(!should_live_suggest_scripture_query("Jo"));
    assert!(!should_live_suggest_scripture_query("John"));
}

#[test]
fn submitted_queries() {
    assert_eq!(
        resolve_submitted_scripture_query("Ezek 1 2 3"),
        "Ezekiel 1:2–3"
    );
    assert_eq!(resolve_submitted_scripture_query("psa 23"), "Psalms 23");
    assert_eq!(resolve_submitted_scripture_query("josh 1 9"), "Joshua 1:9");
    assert_eq!(
        resolve_submitted_scripture_query("for God so loved"),
        "for God so loved"
    );
    assert_eq!(resolve_submitted_scripture_query("love 123"), "love 123");
}

fn one(book: &str, chapter: f64, verses: &[f64], translation: &str) -> ScriptureResult {
    parse(json!({
        "reference": format!("{book} {chapter}:{}", verses[0]),
        "translation": translation,
        "verses": verses.iter().map(|v| json!({"book": book, "chapter": chapter, "verse": v, "text": "t"})).collect::<Vec<_>>(),
    }))
}

#[test]
fn expanding_combining_and_adjacent_verses() {
    let passage = one("Ezekiel", 1.0, &[2.0, 3.0], "KJV");
    let refs: Vec<_> = expand_scripture_result(&passage)
        .into_iter()
        .map(|r| r.reference)
        .collect();
    assert_eq!(refs, ["Ezekiel 1:2", "Ezekiel 1:3"]);

    let loaded = [
        one("Joshua", 1.0, &[8.0], "KJV"),
        one("Joshua", 1.0, &[9.0], "KJV"),
    ];
    assert_eq!(
        get_adjacent_verse_queries(&loaded, Direction::Next),
        ["Joshua 1:10", "Joshua 2:1"]
    );
    assert_eq!(
        get_adjacent_verse_queries(&loaded, Direction::Previous),
        ["Joshua 1:7"]
    );

    let mark = |verse, translation| [one("Mark", 9.0, &[verse], translation)];
    assert_eq!(
        get_adjacent_verse_queries(&mark(43.0, "AMPC"), Direction::Next)[0],
        "Mark 9:45"
    );
    assert_eq!(
        get_adjacent_verse_queries(&mark(45.0, "AMPC"), Direction::Next)[0],
        "Mark 9:47"
    );
    assert_eq!(
        get_adjacent_verse_queries(&mark(47.0, "AMPC"), Direction::Previous),
        ["Mark 9:45"]
    );
    assert_eq!(
        get_adjacent_verse_queries(&mark(45.0, "AMPC"), Direction::Previous),
        ["Mark 9:43"]
    );
    assert_eq!(
        get_adjacent_verse_queries(&mark(43.0, "KJV"), Direction::Next)[0],
        "Mark 9:44"
    );
    assert_eq!(
        get_adjacent_verse_queries(&mark(45.0, "NKJV"), Direction::Previous),
        ["Mark 9:44"]
    );
    assert_eq!(
        get_adjacent_verse_queries(&[one("Joshua", 2.0, &[1.0], "KJV")], Direction::Previous),
        ["Joshua 1:1–176"]
    );
    // Edge cases: nothing loaded, an empty result, before Genesis 1:1.
    assert!(get_adjacent_verse_queries(&[], Direction::Next).is_empty());
    assert!(
        get_adjacent_verse_queries(
            &[ScriptureResult {
                reference: "x".into(),
                translation: "KJV".into(),
                verses: vec![]
            }],
            Direction::Next
        )
        .is_empty()
    );
    assert!(
        get_adjacent_verse_queries(&[one("Genesis", 1.0, &[1.0], "KJV")], Direction::Previous)
            .is_empty()
    );

    let combined = combine_scripture_results(&[
        one("Psalms", 1.0, &[2.0], "KJV"),
        one("Psalms", 1.0, &[9.0], "KJV"),
    ])
    .unwrap();
    assert_eq!(combined.reference, "Psalms 1:2–9");
    assert_eq!(
        combined.verses.iter().map(|v| v.verse).collect::<Vec<_>>(),
        [2.0, 9.0]
    );
    assert_eq!(combine_scripture_results(&[]), None);
}

#[test]
fn reloads_passages_in_a_new_translation() {
    let mut range = one("John", 3.0, &[16.0, 17.0], "KJV");
    range.reference = "John 3:16–17".into();
    let passages = [
        one("John", 3.0, &[16.0], "KJV"),
        one("Romans", 8.0, &[28.0], "KJV"),
        range,
    ];
    let mut queried = Vec::new();
    let out = block_on(reload_passages_in_translation(
        &passages,
        &"NIV".to_string(),
        |query: String, translation: String| {
            queried.push(query.clone());
            let hit = match query.as_str() {
                "John 3:16" => Ok(vec![ScriptureResult {
                    reference: query,
                    translation,
                    verses: passages[0].verses.clone(),
                }]),
                "Romans 8:28" => Ok(vec![]),
                _ => Err("offline"),
            };
            std::future::ready(hit)
        },
    ));
    assert_eq!(out.results[0].translation, "NIV");
    assert_eq!(out.results[1].translation, "KJV");
    // A failed lookup keeps the passage, like an empty one.
    assert_eq!(out.results[2], passages[2]);
    assert_eq!(out.unavailable, ["Romans 8:28", "John 3:16–17"]);
    assert_eq!(queried, ["John 3:16", "Romans 8:28", "John 3:16–17"]);
}

// ─── scripture-live-progress.test.ts ─────────────────────────────────────────

const GENESIS: [&str; 8] = [
    "And God spake unto Noah, saying,",
    "Go forth of the ark, thou, and thy wife, and thy sons, and thy sons' wives with thee.",
    "Bring forth with thee every living thing that is with thee, of all flesh.",
    "Every beast, every creeping thing, and every fowl, and whatsoever creepeth upon the earth.",
    "And Noah builded an altar unto the Lord.",
    "And the Lord smelled a sweet savour.",
    "And the Lord said in his heart, I will not again curse the ground any more for man's sake.",
    "While the earth remaineth, seedtime and harvest, and cold and heat shall not cease.",
];

fn suggestion(
    id: &str,
    book: &str,
    chapter: f64,
    verse: f64,
    text: &str,
    passage: Option<(&str, f64)>,
) -> ScriptureSuggestion {
    let mut value = json!({
        "id": id, "reference": format!("{book} {chapter}:{verse}"),
        "verses": [{"book": book, "chapter": chapter, "verse": verse, "text": text}],
        "translation": "KJV", "confidence": 0.97, "source": "auto", "triggerText": "",
    });
    if let Some((passage_id, index)) = passage {
        value["passageId"] = json!(passage_id);
        value["passageIndex"] = json!(index);
        value["passageReference"] = json!(format!("{book} {chapter}:15-22"));
        value["passageLength"] = json!(8);
    }
    parse(value)
}

fn genesis() -> Vec<ScriptureSuggestion> {
    GENESIS
        .iter()
        .enumerate()
        .map(|(i, text)| {
            suggestion(
                &format!("genesis-8-{}", i + 15),
                "Genesis",
                8.0,
                (i + 15) as f64,
                text,
                Some(("genesis-8-15-22", i as f64)),
            )
        })
        .collect()
}

fn state(passage_id: &str, current: f64) -> PassageFollowState {
    PassageFollowState {
        passage_id: passage_id.into(),
        current_index: current,
        matched_token_indexes: vec![],
        read_to: None,
        heard_count: None,
    }
}

#[test]
fn groups_and_reading_progress() {
    let list = genesis();
    let groups = group_scripture_suggestions(&list);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].reference, "Genesis 8:15-22");
    assert_eq!(groups[0].suggestions.len(), 8);

    let progress = find_reading_progress(
        "Then God spoke to Noah saying go out of the ark, you and your wife, and your sons and your sons wives with you",
        &list,
    )
    .unwrap();
    assert_eq!((progress.matched_index, progress.active_index), (1, 2));
    assert_eq!(progress.active_suggestion_id, "genesis-8-17");
    // Edge case: too few heard words.
    assert_eq!(find_reading_progress("and the", &list), None);
}

#[test]
fn merging_and_auto_follow() {
    let list = genesis();
    let mut guessed = list[6].clone();
    guessed.passage_id = None;
    guessed.passage_reference = None;
    let merged = merge_scripture_suggestion(&[guessed], &list[0]);
    assert_eq!(
        merged.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["genesis-8-15"]
    );

    let sent: HashSet<String> = ["genesis-8-15", "genesis-8-16"].map(String::from).into();
    assert_eq!(
        next_auto_follow_suggestion(&list, "genesis-8-17", "genesis-8-15-22", &sent)
            .unwrap()
            .id,
        "genesis-8-17"
    );
    assert!(
        next_auto_follow_suggestion(&list, "genesis-8-17", "another-passage", &HashSet::new())
            .is_none()
    );
    let later: HashSet<String> = ["genesis-8-18".to_string()].into();
    assert!(
        next_auto_follow_suggestion(&list, "genesis-8-16", "genesis-8-15-22", &later).is_none()
    );

    let mut fresh = list[1].clone();
    fresh.id = "fresh-detection".into();
    fresh.passage_id = Some("fresh-passage".into());
    fresh.passage_length = Some(1.0);
    assert_eq!(merge_scripture_suggestion(&list, &fresh), list);
    fresh.translation = "NKJV".into();
    assert_eq!(
        merge_scripture_suggestion(&list, &fresh).len(),
        list.len() + 1
    );
}

#[test]
fn passage_follow_holds_until_the_sentence_ends() {
    let list = genesis();
    let partial = update_passage_follow(
        &state("genesis-8-15-22", 1.0),
        "Go out of the ark, you and your wife",
        &list,
        true,
    );
    let commentary = update_passage_follow(
        &partial.state,
        "The ark reminds us that obedience can be difficult in every generation",
        &list,
        true,
    );
    let resumed = update_passage_follow(
        &commentary.state,
        "and your sons and your sons wives with you",
        &list,
        true,
    );
    assert_eq!(partial.next_suggestion_id, None);
    assert_eq!(commentary.next_suggestion_id, None);
    assert_eq!(resumed.next_suggestion_id.as_deref(), Some("genesis-8-17"));
    assert_eq!(resumed.state.current_index, 2.0);

    let john: Vec<_> = [
        "In the beginning was the Word, and the Word was with God, and the Word was God.",
        "The same was in the beginning with God.",
        "All things were made by him; and without him was not any thing made that was made.",
    ]
    .iter()
    .enumerate()
    .map(|(i, t)| {
        suggestion(
            &format!("john-1-{}", i + 1),
            "John",
            1.0,
            (i + 1) as f64,
            t,
            Some(("john-1-1-3", i as f64)),
        )
    })
    .collect();
    let mut current = state("john-1-1-3", 0.0);
    for live in [
        "In the beginning",
        "In the beginning was the Word",
        "In the beginning was the Word and the Word was with God",
    ] {
        for _ in 0..3 {
            let update = update_passage_follow(&current, live, &john, false);
            assert_eq!(update.next_suggestion_id, None, "moved on at {live}");
            current = update.state;
        }
    }
    let done = update_passage_follow(
        &current,
        "In the beginning was the Word and the Word was with God and the Word was God",
        &john,
        false,
    );
    assert_eq!(done.next_suggestion_id.as_deref(), Some("john-1-2"));
    let shared = update_passage_follow(
        &state("john-1-1-3", 0.0),
        "In the beginning was the Word and the Word was with God",
        &john,
        true,
    );
    assert_eq!(shared.next_suggestion_id, None);
}

#[test]
fn strict_follow_waits_for_most_of_the_verse() {
    let matthew: Vec<_> = [
        "Blessed are the poor in spirit, For theirs is the kingdom of heaven.",
        "Blessed are those who mourn, For they shall be comforted.",
    ]
    .iter()
    .enumerate()
    .map(|(i, t)| {
        suggestion(
            &format!("matt-5-{}", i + 3),
            "Matthew",
            5.0,
            (i + 3) as f64,
            t,
            Some(("matt-5-3-4", i as f64)),
        )
    })
    .collect();
    let start = state("matt-5-3-4", 0.0);
    let next = |text: &str, strict: bool, final_text: bool| {
        let update = if strict {
            update_passage_follow_strict(&start, text, &matthew, final_text)
        } else {
            update_passage_follow(&start, text, &matthew, final_text)
        };
        update.next_suggestion_id
    };
    // Verse 4's opening heard a third of the way into verse 3: the Electron
    // rule jumps; the native rule stays on the verse being read.
    let early = "Blessed are the poor. Blessed are those who";
    assert_eq!(next(early, false, true).as_deref(), Some("matt-5-4"));
    assert_eq!(next(early, true, true), None);
    // Verse 3 read to its closing words, then verse 4 begins (its last word
    // misheard): live text moves on at once.
    let most = "Blessed are the poor in spirit for theirs is the kingdom. Blessed are those who";
    assert_eq!(next(most, true, false).as_deref(), Some("matt-5-4"));
    // Mid-verse, verse 4's opening on live text never moves on.
    let mid = "Blessed are the poor in spirit. Blessed are those who";
    assert_eq!(next(mid, true, false), None);
    assert_eq!(next(mid, true, true), None);
    // A verse read to its end moves on, on live text too.
    let whole = "Blessed are the poor in spirit for theirs is the kingdom of heaven";
    assert_eq!(next(whole, true, false).as_deref(), Some("matt-5-4"));
    // At the closing words with the last one misheard ("heaven" as
    // "seven"), the next verse's first words move on, live text too.
    let misheard = "Blessed are the poor in spirit for theirs is the kingdom of seven. Blessed are";
    assert_eq!(next(misheard, true, false).as_deref(), Some("matt-5-4"));
    // Stopping before the last words to explain is not finishing the verse.
    let paused = "Blessed are the poor in spirit for theirs is the kingdom";
    assert_eq!(next(paused, false, true).as_deref(), Some("matt-5-4"));
    assert_eq!(next(paused, true, true), None);
}

#[test]
fn operator_sections_navigation_and_sending() {
    let list = genesis();
    let (kept, sent) =
        apply_operator_suggestion_sent(list[..1].to_vec(), &HashSet::new(), "genesis-8-15");
    assert_eq!(kept.len(), 1);
    assert_eq!(sent, HashSet::from(["genesis-8-15".to_string()]));

    let ids: Vec<String> = ["verse-1", "verse-2", "verse-3"].map(String::from).into();
    assert_eq!(
        navigate_operator_suggestion_id(&ids, None, 1).as_deref(),
        Some("verse-1")
    );
    assert_eq!(
        navigate_operator_suggestion_id(&ids, Some("verse-1"), 1).as_deref(),
        Some("verse-2")
    );
    assert_eq!(
        navigate_operator_suggestion_id(&ids, Some("verse-2"), -1).as_deref(),
        Some("verse-1")
    );
    assert_eq!(
        navigate_operator_suggestion_id(&ids, Some("verse-3"), 1).as_deref(),
        Some("verse-3")
    );
    assert_eq!(navigate_operator_suggestion_id(&[], None, 1), None);

    let groups: Vec<_> = ["old", "active", "newer", "newest"]
        .iter()
        .map(|id| ScriptureSuggestionGroup {
            id: id.to_string(),
            reference: id.to_string(),
            suggestions: vec![],
        })
        .collect();
    let sections = partition_operator_suggestion_groups(&groups, Some("active"), 3);
    assert_eq!(
        sections
            .current
            .iter()
            .map(|g| g.id.as_str())
            .collect::<Vec<_>>(),
        ["newest", "newer", "active"]
    );
    assert_eq!(
        sections
            .history
            .iter()
            .map(|g| g.id.as_str())
            .collect::<Vec<_>>(),
        ["old"]
    );
}

// ─── single-verse-presentation, detector-recovery, tab push ──────────────────

#[test]
fn ranges_stage_separate_verses() {
    let range: ScriptureSuggestion = parse(
        json!({"id": "range", "reference": "Acts 4:8–9", "verses": [{"book": "Acts", "chapter": 4, "verse": 8, "text": "Verse 8"}, {"book": "Acts", "chapter": 4, "verse": 9, "text": "Verse 9"}], "translation": "NKJV", "confidence": 1, "source": "manual", "triggerText": "Acts 4", "planId": "sermon", "planItemId": "item"}),
    );
    let slides = split_verse_suggestions(&range);
    assert_eq!(
        slides
            .iter()
            .map(|s| (s.reference.as_str(), s.verses.len(), s.passage_index))
            .collect::<Vec<_>>(),
        [("Acts 4:8", 1, Some(0.0)), ("Acts 4:9", 1, Some(1.0))]
    );
    assert_eq!(slides[1].plan_item_id.as_deref(), Some("item"));
    assert_eq!(slides[0].passage_id, slides[1].passage_id);
    assert_eq!(single_verse_suggestion(&range).verses, range.verses[..1]);
    assert_eq!(single_verse_suggestion(&range).reference, "Acts 4:8");
}

#[test]
fn detector_stays_degraded_until_a_request_succeeds() {
    let initial = DetectorRecoveryState {
        consecutive_errors: 2,
        fallback_active: true,
        probing: false,
    };
    let probing = transition_detector_recovery(initial, DetectorRecoveryEvent::BeginProbe);
    let recovered = transition_detector_recovery(probing, DetectorRecoveryEvent::RequestSucceeded);
    assert_eq!(
        probing,
        DetectorRecoveryState {
            consecutive_errors: 2,
            fallback_active: false,
            probing: true
        }
    );
    assert_eq!(
        recovered,
        DetectorRecoveryState {
            consecutive_errors: 0,
            fallback_active: false,
            probing: false
        }
    );
    let once = transition_detector_recovery(recovered, DetectorRecoveryEvent::RequestFailed);
    assert!(!once.fallback_active);
    assert!(
        transition_detector_recovery(once, DetectorRecoveryEvent::RequestFailed).fallback_active
    );
}

#[test]
fn spoken_verse_text_drops_notes() {
    assert_eq!(
        spoken_verse_text("all {ye} that labour"),
        "all ye that labour"
    );
    assert_eq!(
        spoken_verse_text("the {firmament: Heb. expansion} of"),
        "the   of"
    );
    assert_eq!(spoken_verse_text("unclosed {brace"), "unclosed {brace");
}

// ─── spoken-translations.test.ts ─────────────────────────────────────────────

#[test]
fn spoken_translations() {
    let cases: &[(&str, &[&str])] = &[
        ("John 3:16 in the NLT", &["NLT"]),
        ("read it in the n l t please", &["NLT"]),
        ("the N.I.V. puts it this way", &["NIV"]),
        ("in the N-K-J-V", &["NKJV"]),
        ("the New Living Translation says", &["NLT"]),
        ("from the New International Version", &["NIV"]),
        ("in the King James it says", &["KJV"]),
        ("the English Standard Version renders it", &["ESV"]),
        ("in the New King James Version", &["NKJV"]),
        ("the New American Standard Bible", &["NASB"]),
        ("let us read it in the NLT and the NIV", &["NLT", "NIV"]),
        ("NIV, NLT and then the NIV again", &["NIV", "NLT"]),
        ("the message today is about grace", &[]),
        ("his voice was amplified across the hall", &[]),
        ("we stream on the web every Sunday", &[]),
        ("the passion of Christ", &[]),
        ("in the Message Bible it reads", &["MSG"]),
        ("the Amplified says", &["AMPC"]),
        ("the Passion Translation puts it", &["TPT"]),
        ("john 3:16 in the niv", &["NIV"]),
        ("the nkjv says", &["NKJV"]),
        ("read the esv and the kjv", &["ESV", "KJV"]),
        ("The NLT, in John 3:16, says", &["NLT"]),
        ("John 3:16 (NIV)", &["NIV"]),
        ("John 3:16 — New Living Translation.", &["NLT"]),
        ("the King James and the New King James", &["KJV", "NKJV"]),
        ("in the KJV and the NKJV", &["KJV", "NKJV"]),
        ("nltd and kjvx are not translations", &[]),
        ("we serve the new living God", &[]),
        ("a new international partnership", &[]),
        ("", &[]),
        ("For God so loved the world", &[]),
    ];
    for (text, ids) in cases {
        assert_eq!(find_spoken_translations(text), *ids, "{text}");
    }
}

// ─── sermon-plan-match.test.ts, sermon-plan-resolve.test.ts ──────────────────

const R28: &str = "And we know that all things work together for good to those who love God, to those who are the called according to His purpose.";
const R29: &str = "For whom He foreknew, He also predestined to be conformed to the image of His Son, that He might be the firstborn among many brethren.";

fn item(id: &str, reference: &str, translation: &str, verses: &[(&str, f64, f64, &str)]) -> Value {
    json!({"id": id, "reference": reference, "translation": translation, "available": true,
        "verses": verses.iter().map(|(b, c, v, t)| json!({"book": b, "chapter": c, "verse": v, "text": t})).collect::<Vec<_>>()})
}

fn plan(items: Vec<Value>) -> SermonPlan {
    parse(
        json!({"id": "plan-1", "title": "Sunday", "sourceFileName": "notes.docx", "items": items, "createdAt": 0, "updatedAt": 0}),
    )
}

fn reference(book: &str, chapter: f64, start: f64, end: Option<f64>) -> PlanReference {
    PlanReference {
        book: book.into(),
        chapter,
        verse_start: start,
        verse_end: end,
    }
}

#[test]
fn sermon_plan_matching() {
    let single = item(
        "item-single",
        "Romans 8:28",
        "NKJV",
        &[("Romans", 8.0, 28.0, R28)],
    );
    let range = item(
        "item-range",
        "Romans 8:28–29",
        "NKJV",
        &[("Romans", 8.0, 28.0, R28), ("Romans", 8.0, 29.0, R29)],
    );

    let index = build_sermon_plan_index(&plan(vec![range.clone()]));
    assert_eq!(index.by_verse_key.len(), 2);
    let first = index
        .by_verse_key
        .get(&verse_key("Romans", 8.0, 28.0))
        .unwrap();
    assert_eq!(
        (
            first.index_in_item,
            first.item_length,
            first.reference.as_str(),
            first.item_reference.as_str()
        ),
        (0, 2, "Romans 8:28", "Romans 8:28–29")
    );
    assert_eq!(
        match_plan_reference(
            Some(&index),
            &reference("Romans", 8.0, 28.0, Some(29.0)),
            None
        )
        .len(),
        2
    );

    let mut unavailable = item("u", "John 3:16", "NKJV", &[]);
    unavailable["available"] = json!(false);
    let empty = build_sermon_plan_index(&plan(vec![
        unavailable,
        item("e", "John 3:17", "NKJV", &[]),
    ]));
    assert!(empty.by_verse_key.is_empty() && empty.quotable.is_empty());

    for items in [
        vec![range.clone(), single.clone()],
        vec![single.clone(), range.clone()],
    ] {
        let index = build_sermon_plan_index(&plan(items));
        assert_eq!(
            index
                .by_verse_key
                .get(&verse_key("Romans", 8.0, 28.0))
                .unwrap()
                .plan_item_id,
            "item-single"
        );
    }

    let only_single = build_sermon_plan_index(&plan(vec![single.clone()]));
    assert!(
        match_plan_reference(
            Some(&only_single),
            &reference("Romans", 8.0, 28.0, Some(30.0)),
            None
        )
        .is_empty()
    );
    assert!(
        match_plan_reference(
            Some(&only_single),
            &reference("Romans", 9.0, 28.0, None),
            None
        )
        .is_empty()
    );
    assert!(match_plan_reference(None, &reference("Romans", 8.0, 28.0, None), None).is_empty());
    assert_eq!(
        match_plan_reference(
            Some(&only_single),
            &reference("romans", 8.0, 28.0, None),
            None
        )
        .len(),
        1
    );

    let quote = match_plan_quote(
        Some(&only_single),
        "and we know that all things work together for good to those who love God",
        DEFAULT_MIN_COVERAGE,
    )
    .unwrap();
    assert_eq!(quote.entry.plan_item_id, "item-single");
    assert!(quote.coverage >= 0.6);
    assert!(match_plan_quote(Some(&only_single), "God is good, and the things He called us to do work out for our purpose when we love Him, together as one people who know that", DEFAULT_MIN_COVERAGE).is_none());
    assert!(match_plan_quote(None, R28, DEFAULT_MIN_COVERAGE).is_none());
    assert!(match_plan_quote(Some(&only_single), "   ", DEFAULT_MIN_COVERAGE).is_none());

    let short = build_sermon_plan_index(&plan(vec![item(
        "short",
        "John 11:35",
        "NKJV",
        &[("John", 11.0, 35.0, "Jesus wept.")],
    )]));
    assert!(short.quotable.is_empty());

    let next = item(
        "next",
        "John 3:16",
        "NKJV",
        &[(
            "John",
            3.0,
            16.0,
            "For God so loved the world that he gave his only begotten son",
        )],
    );
    let mut index = build_sermon_plan_index(&plan(vec![range.clone(), next]));
    assert_eq!(index.ordered[index.expected_index].reference, "Romans 8:28");
    advance_plan_expectation(&mut index, "Romans 8:28", None);
    assert_eq!(index.ordered[index.expected_index].reference, "Romans 8:29");
    assert!(!advance_plan_expectation(&mut index, "Romans 8:28", None));
    advance_plan_expectation(&mut index, "Romans 8:29", None);
    assert_eq!(index.ordered[index.expected_index].reference, "John 3:16");
    assert!(!advance_plan_expectation(&mut index, "Genesis 1:1", None));
    advance_plan_expectation(&mut index, "John 3:16", None);
    assert!(index.ordered.get(index.expected_index).is_none());

    let wording = "the Lord has been mindful of us and he will bless us";
    let mut tie = build_sermon_plan_index(&plan(vec![
        item(
            "earlier",
            "Psalms 115:12",
            "NKJV",
            &[("Psalms", 115.0, 12.0, wording)],
        ),
        item(
            "expected",
            "Psalms 116:12",
            "NKJV",
            &[("Psalms", 116.0, 12.0, wording)],
        ),
    ]));
    tie.expected_index = 1;
    assert_eq!(
        match_plan_quote(Some(&tie), wording, DEFAULT_MIN_COVERAGE)
            .unwrap()
            .entry
            .plan_item_id,
        "expected"
    );
    assert!(match_plan_quote(Some(&tie), "good morning church", DEFAULT_MIN_COVERAGE).is_none());

    let translations = build_sermon_plan_index(&plan(vec![
        item("nkjv", "Romans 8:28", "NKJV", &[("Romans", 8.0, 28.0, R28)]),
        item(
            "nlt",
            "Romans 8:28",
            "NLT",
            &[(
                "Romans",
                8.0,
                28.0,
                "And we know that God causes everything to work together for the good of those who love God.",
            )],
        ),
    ]));
    let r = reference("Romans", 8.0, 28.0, None);
    assert_eq!(
        match_plan_reference(Some(&translations), &r, Some("NLT"))[0].plan_item_id,
        "nlt"
    );
    assert_eq!(
        match_plan_reference(Some(&translations), &r, Some("nkjv"))[0].plan_item_id,
        "nkjv"
    );
    assert!(match_plan_reference(Some(&translations), &r, Some("NIV")).is_empty());
    assert_eq!(
        match_plan_reference(Some(&translations), &r, None)[0].plan_item_id,
        "nkjv"
    );
}

fn option(id: &str, access: &str, available: bool) -> ScriptureTranslationOption {
    parse(
        json!({"id": id, "name": id, "access": access, "available": available, "requiresApiKey": access == "api"}),
    )
}

#[test]
fn sermon_plan_resolving() {
    let options = [
        option("NLT", "api", false),
        option("KJV", "local", true),
        option("BSB", "local", true),
    ];
    assert_eq!(
        translation_fallback_order("NLT", "KJV", &options),
        ["NLT", "KJV", "BBE", "BSB"]
    );

    let item: SermonScriptureItem = parse(
        json!({"id": "phil", "reference": "Philippians 4:6–7", "translation": "NLT", "verses": [], "available": false, "error": "NLT text is unavailable"}),
    );
    let mut search = |query: String, translation: String| {
        let found = if translation == "KJV" && query.contains("Philippians") {
            vec![one("Philippians", 4.0, &[6.0, 7.0], "KJV")]
        } else {
            vec![]
        };
        std::future::ready(Ok::<_, String>(found))
    };
    let resolved = block_on(resolve_sermon_plan_item(
        &item,
        "KJV",
        &mut search,
        &[option("KJV", "local", true)],
    ));
    assert!(resolved.available);
    assert_eq!(resolved.translation, "KJV");
    assert_eq!(resolved.verses.len(), 2);
    assert!(resolved.error.unwrap().contains("NLT unavailable"));

    // Failure path: every lookup fails, the last error is kept.
    let mut failing = |_: String, translation: String| {
        std::future::ready(Err::<Vec<ScriptureResult>, _>(format!(
            "{translation} down"
        )))
    };
    let failed = block_on(resolve_sermon_plan_item(&item, "KJV", &mut failing, &[]));
    assert!(!failed.available);
    assert_eq!(failed.error.as_deref(), Some("BSB down"));
}

// ─── sermon-notes-review.test.ts ─────────────────────────────────────────────

fn label(text: &str, reference: &str) -> Option<String> {
    normalized_reference_label(&SermonReferenceMatch {
        start: 0.0,
        end: 6.0,
        text: text.into(),
        reference: reference.into(),
        translations: vec!["NKJV".into()],
    })
}

#[test]
fn notes_labels_html_and_offsets() {
    assert_eq!(label("ps 2 3", "Psalms 2:3").as_deref(), Some("Psalms 2:3"));
    assert_eq!(label("Mark 4:2 NLT", "Mark 4:2"), None);
    let html = plain_text_to_editor_html("ABEL  Genesis 4:3-5\nNOAH");
    assert!(html.contains("ABEL&nbsp;&nbsp;Genesis 4:3-5"));
    assert!(html.contains("<p>NOAH</p>"));
    assert_eq!(plain_text_to_editor_html(""), "<p><br></p>");

    let lines = [
        "STS BIBLICAL EXAMPLES",
        "",
        "INTRODUCTION",
        "#3 FATHER ABRAHAM Genesis 14:18-20 and Genesis 28:20-22",
        "#4 DAVID 1 Samuel 17:48-51",
        "E Esther 2:5-11; Esth 4:14",
        "Hebrews 7:1-7",
    ];
    let doc = SermonNotesNode::block(
        "doc",
        lines
            .iter()
            .map(|l| {
                SermonNotesNode::block(
                    "paragraph",
                    if l.is_empty() {
                        vec![]
                    } else {
                        vec![SermonNotesNode::text(*l)]
                    },
                )
            })
            .collect(),
    );
    let text = notes_plain_text(&doc, SERMON_NOTES_BLOCK_SEPARATOR);
    assert_eq!(text, lines.join("\n"));
    for needle in [
        "Genesis 14:18-20",
        "Genesis 28:20-22",
        "1 Samuel 17:48-51",
        "Esther 2:5-11",
        "Hebrews 7:1-7",
    ] {
        let start = text.find(needle).unwrap() as f64;
        let range = map_text_range_to_pos(
            &doc,
            start,
            start + needle.len() as f64,
            SERMON_NOTES_BLOCK_SEPARATOR,
        )
        .unwrap();
        // Each block adds 2 positions and 1 separator character before it.
        assert_eq!(range.to - range.from, needle.len(), "{needle}");
    }
    // The naive offset+1 fails on the second paragraph; the mapping does not.
    let two = SermonNotesNode::block(
        "doc",
        vec![
            SermonNotesNode::block("paragraph", vec![SermonNotesNode::text("Intro")]),
            SermonNotesNode::block(
                "paragraph",
                vec![SermonNotesNode::text("Read Genesis 5:28-29 next")],
            ),
        ],
    );
    let start = 11.0;
    let range = map_text_range_to_pos(&two, start, start + 15.0, "\n").unwrap();
    assert_eq!(range, DocRange { from: 13, to: 28 });
    assert_ne!(range.from, start as usize + 1);
    // Edge cases: negative, reversed and empty ranges.
    assert_eq!(map_text_range_to_pos(&two, -1.0, 2.0, "\n"), None);
    assert_eq!(map_text_range_to_pos(&two, 3.0, 2.0, "\n"), None);
    assert_eq!(map_text_range_to_pos(&two, 2.0, 2.0, "\n"), None);
}

// ─── scripture-trace.test.ts ─────────────────────────────────────────────────

const T0: f64 = 1_700_000_000_000.0;

fn trace(changes: Value) -> ScriptureLatencyTrace {
    let mut value = json!({"correlationId": "c1", "reference": "Psalm 119:9", "sourceText": "turn with me", "transcriptSource": "interim", "resolver": "explicit", "status": "active", "sttReceivedAt": T0});
    for (k, v) in changes.as_object().unwrap() {
        value[k] = v.clone();
    }
    parse(value)
}

fn presented() -> ScriptureLatencyTrace {
    trace(
        json!({"status": "presented", "detectionStartedAt": T0, "citationRecognizedAt": T0 + 3.0, "bibleLookupStartedAt": T0 + 4.0, "bibleLookupCompletedAt": T0 + 21.0, "suggestionPublishedAt": T0 + 29.0, "countdownStartedAt": T0 + 31.0, "countdownCompletedAt": T0 + 1032.0, "presenterRequestStartedAt": T0 + 1033.0, "presenterRequestCompletedAt": T0 + 1215.0, "presenterStateCheckStartedAt": T0 + 1216.0, "presenterStateConfirmedAt": T0 + 1297.0, "confirmation": "visible-state"}),
    )
}

#[test]
fn trace_metrics() {
    let m = derive_metrics(&presented());
    assert_eq!(
        (m.detection_ms, m.bible_lookup_ms, m.suggestion_publish_ms),
        (Some(3.0), Some(17.0), Some(8.0))
    );
    assert_eq!(
        (m.preparation_ms, m.countdown_ms, m.presenter_request_ms),
        (Some(31.0), Some(1001.0), Some(182.0))
    );
    assert_eq!(
        (m.presenter_confirmation_ms, m.total_ms),
        (Some(81.0), Some(1297.0))
    );
    assert_eq!(
        m.slowest_stage,
        Some(SlowestStage {
            name: "Output request".into(),
            duration_ms: 182.0
        })
    );

    let mut unconfirmed = presented();
    unconfirmed.presenter_state_confirmed_at = None;
    let m = derive_metrics(&unconfirmed);
    assert_eq!(
        (m.total_ms, m.total_to_presenter_request_ms),
        (None, Some(1215.0))
    );

    let m = derive_metrics(&trace(json!({"citationRecognizedAt": T0 + 5.0})));
    assert_eq!(
        (
            m.detection_ms,
            m.bible_lookup_ms,
            m.countdown_ms,
            m.total_ms
        ),
        (Some(5.0), None, None, None)
    );
    assert_eq!(
        derive_metrics(&trace(json!({"citationRecognizedAt": T0 - 40.0}))).detection_ms,
        None
    );

    let stages = recorded_stages(&presented());
    assert_eq!(
        stages[..3].iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        [
            ScriptureTraceStage::DetectionStartedAt,
            ScriptureTraceStage::CitationRecognizedAt,
            ScriptureTraceStage::BibleLookupStartedAt
        ]
    );
    assert!(stages.windows(2).all(|w| w[0].1 <= w[1].1));

    assert!(!is_terminal(TraceStatus::Active));
    for status in [
        TraceStatus::Presented,
        TraceStatus::Cancelled,
        TraceStatus::Superseded,
        TraceStatus::Failed,
    ] {
        assert!(is_terminal(status));
    }
    assert_eq!(percentile(&[], 0.95), Some(0.0));
    assert_eq!(percentile(&[5.0], 0.5), Some(5.0));
    assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.5), Some(2.0));
    assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 1.0), Some(4.0));

    let visible = derive_metrics(&trace(
        json!({"status": "presented", "citationRecognizedAt": T0 + 3.0, "bibleLookupStartedAt": T0 + 4.0, "bibleLookupCompletedAt": T0 + 21.0, "suggestionPublishedAt": T0 + 29.0, "suggestionRenderedAt": T0 + 47.0, "countdownStartedAt": T0 + 48.0, "countdownCompletedAt": T0 + 1048.0, "presenterRequestStartedAt": T0 + 1049.0, "presenterRequestCompletedAt": T0 + 1231.0}),
    ));
    assert_eq!(
        (visible.suggestion_render_ms, visible.operator_visible_ms),
        (Some(18.0), Some(47.0))
    );
    let unpainted = derive_metrics(&trace(json!({"suggestionPublishedAt": T0 + 20.0})));
    assert_eq!(
        (
            unpainted.operator_visible_ms,
            unpainted.suggestion_render_ms
        ),
        (None, None)
    );
}

fn presented_with(resolver: &str, detection: f64, request: f64) -> ScriptureLatencyTrace {
    trace(
        json!({"resolver": resolver, "status": "presented", "citationRecognizedAt": T0 + detection, "presenterRequestStartedAt": T0 + 1000.0, "presenterRequestCompletedAt": T0 + 1000.0 + request}),
    )
}

#[test]
fn trace_summaries() {
    let traces: Vec<_> = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 100.0]
        .iter()
        .map(|ms| presented_with("explicit", *ms, ms * 2.0))
        .collect();
    let summary = &summarize_by_resolver(&traces)[0];
    let d = &summary.stages["detectionMs"];
    assert_eq!(
        (
            summary.presented,
            d.p50,
            d.min,
            d.mean,
            d.p90,
            d.p95,
            d.p99,
            d.max
        ),
        (10.0, 5.0, 1.0, 14.5, 9.0, 100.0, 100.0, 100.0)
    );
    assert_eq!(summary.stages["presenterRequestMs"].p50, 10.0);

    let summary = &summarize_by_resolver(&[
        presented_with("explicit", 5.0, 10.0),
        trace(json!({"status": "cancelled", "citationRecognizedAt": T0 + 9000.0})),
        trace(json!({"status": "superseded", "citationRecognizedAt": T0 + 9000.0})),
        trace(json!({"status": "failed", "failureReason": "presenter-disconnected"})),
    ])[0];
    assert_eq!(
        (
            summary.presented,
            summary.cancelled,
            summary.superseded,
            summary.failed
        ),
        (1.0, 1.0, 1.0, 1.0)
    );
    assert_eq!(
        (
            summary.stages["detectionMs"].count,
            summary.stages["detectionMs"].p95
        ),
        (1.0, 5.0)
    );

    let summaries = summarize_by_resolver(&[
        presented_with("ai", 400.0, 10.0),
        presented_with("explicit", 3.0, 10.0),
        presented_with("explicit", 4.0, 10.0),
    ]);
    assert_eq!(
        summaries.iter().map(|s| s.resolver).collect::<Vec<_>>(),
        [ScriptureResolver::Explicit, ScriptureResolver::Ai]
    );
    assert_eq!(summaries[1].stages["detectionMs"].p50, 400.0);
    assert!(
        !summarize_by_resolver(&[presented_with("explicit", 5.0, 10.0)])[0]
            .stages
            .contains_key("presenterConfirmationMs")
    );

    let cancelled = &summarize_by_resolver(&[trace(
        json!({"status": "cancelled", "suggestionPublishedAt": T0 + 20.0, "suggestionRenderedAt": T0 + 44.0}),
    )])[0];
    assert_eq!((cancelled.cancelled, cancelled.presented), (1.0, 0.0));
    assert_eq!(
        (
            cancelled.stages["operatorVisibleMs"].count,
            cancelled.stages["operatorVisibleMs"].p50
        ),
        (1.0, 44.0)
    );
}

fn start(store: &mut ScriptureTraceStore, id: &str) {
    store.start(parse(json!({"correlationId": id, "sourceText": "psalm one nineteen nine", "transcriptSource": "interim", "resolver": "explicit", "sttReceivedAt": T0})));
}

#[test]
fn trace_store() {
    use ScriptureTraceStage as S;
    let mut store = ScriptureTraceStore::default();
    start(&mut store, "a");
    store.mark("a", S::CitationRecognizedAt, T0 + 4.0);
    store.mark("a", S::BibleLookupStartedAt, T0 + 5.0);
    store.mark("a", S::BibleLookupCompletedAt, T0 + 19.0);
    assert_eq!(store.metrics("a").unwrap().detection_ms, Some(4.0));
    assert_eq!(store.metrics("a").unwrap().bible_lookup_ms, Some(14.0));

    start(&mut store, "metadata");
    store.annotate(
        "metadata",
        TraceAnnotation {
            translation: Some("KJV".into()),
            auto_present_delay_ms: Some(1000.0),
            presenter_connection_mode: Some(PresenterConnectionMode::Connected),
            presenter_output_mode: Some("library".into()),
        },
    );
    let t = store.get("metadata").unwrap();
    assert_eq!(
        (
            t.translation.as_deref(),
            t.auto_present_delay_ms,
            t.presenter_output_mode.as_deref()
        ),
        (Some("KJV"), Some(1000.0), Some("library"))
    );

    let mut store = ScriptureTraceStore::default();
    start(&mut store, "a");
    store.mark("a", S::SuggestionRenderedAt, T0 + 20.0);
    store.mark("a", S::SuggestionRenderedAt, T0 + 45.0);
    assert_eq!(
        store.get("a").unwrap().suggestion_rendered_at,
        Some(T0 + 20.0)
    );

    let mut store = ScriptureTraceStore::default();
    start(&mut store, "a");
    store.complete("a", TraceStatus::Cancelled);
    store.mark("a", S::PresenterRequestCompletedAt, T0 + 900.0);
    store.complete("a", TraceStatus::Presented);
    assert_eq!(store.get("a").unwrap().status, TraceStatus::Cancelled);
    assert_eq!(store.get("a").unwrap().presenter_request_completed_at, None);

    let mut store = ScriptureTraceStore::default();
    start(&mut store, "a");
    store.complete(
        "a",
        CompleteInput {
            confirmation: Some(ConfirmationLevel::RequestAccepted),
            ..TraceStatus::Presented.into()
        },
    );
    store.mark("a", S::SuggestionRenderedAt, T0 + 30.0);
    assert_eq!(store.metrics("a").unwrap().operator_visible_ms, Some(30.0));

    let mut store = ScriptureTraceStore::default();
    start(&mut store, "a");
    store.complete(
        "a",
        CompleteInput {
            failure_reason: Some(TraceFailureReason::PresenterDisconnected),
            error: Some("ECONNREFUSED".into()),
            ..TraceStatus::Failed.into()
        },
    );
    assert_eq!(
        store.get("a").unwrap().error.as_deref(),
        Some("ECONNREFUSED")
    );
    start(&mut store, "ai");
    store.complete(
        "ai",
        CompleteInput {
            superseded_by: Some("explicit-1".into()),
            ..TraceStatus::Superseded.into()
        },
    );
    assert_eq!(
        store.get("ai").unwrap().superseded_by.as_deref(),
        Some("explicit-1")
    );

    let mut store = ScriptureTraceStore::default();
    store.mark_now("nope", S::CountdownStartedAt);
    store.complete("nope", TraceStatus::Presented);
    assert!(store.get("nope").is_none() && store.is_empty());

    let mut store = ScriptureTraceStore::new(3);
    for id in ["a", "b", "c", "d"] {
        start(&mut store, id);
    }
    assert_eq!(store.len(), 3);
    assert!(store.get("a").is_none());
    let ids = |store: &ScriptureTraceStore, limit| {
        store
            .get_recent(limit)
            .iter()
            .map(|t| t.correlation_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&store, DEFAULT_RECENT_LIMIT), ["d", "c", "b"]);
    assert_eq!(ids(&store, 2), ["d", "c"]);
}

// ─── api-bible-offline-state.test.ts, offline-bible-view-model.test.ts ───────

#[test]
fn cache_notices() {
    assert_eq!(get_scripture_cache_notice(""), None);
    assert_eq!(
        get_scripture_cache_notice("No results. Try a reference like “John 3:16”."),
        None
    );
    let notice = get_scripture_cache_notice(msg::NOT_AVAILABLE_OFFLINE).unwrap();
    assert_eq!(notice.kind, ScriptureCacheNoticeKind::OfflineMissing);
    assert!(notice.hint.to_lowercase().contains("connection"));
    let notice = get_scripture_cache_notice(msg::REFRESH_REQUIRED).unwrap();
    assert_eq!(notice.kind, ScriptureCacheNoticeKind::RefreshRequired);
    assert!(notice.hint.contains("Settings"));
    assert_eq!(
        get_scripture_cache_notice(msg::ACCESS_REVOKED)
            .unwrap()
            .kind,
        ScriptureCacheNoticeKind::AccessRevoked
    );
    assert_eq!(get_scripture_cache_notice("Search failed"), None);
    // The source returns an inherited function here; the port returns nothing.
    assert_eq!(get_scripture_cache_notice("toString"), None);
    assert!(should_show_api_bible_attribution(Some(&option(
        "NKJV", "api", true
    ))));
    assert!(!should_show_api_bible_attribution(Some(&option(
        "KJV", "local", true
    ))));
    assert!(!should_show_api_bible_attribution(None));
    assert_eq!(API_BIBLE_ATTRIBUTION_URL, "https://scripture.api.bible");
}

const NOW: f64 = 1_787_616_000_000.0;

fn row(status: ApiBibleCacheStatus, changes: Value) -> ApiBibleOfflineTranslation {
    let mut value = json!({"bibleId": "nkjv-01", "translation": "NKJV", "name": "New King James Version", "copyright": "© Thomas Nelson", "status": status, "cachedChapters": 1189, "totalChapters": 1189, "cachedVerses": 31102, "fetchedAt": NOW, "expiresAt": NOW + 2_592_000_000.0, "offlineDownloadEnabled": true});
    for (k, v) in changes.as_object().unwrap() {
        value[k] = v.clone();
    }
    parse(value)
}

#[test]
fn offline_bible_rows() {
    use ApiBibleCacheStatus as S;
    use OfflineBibleAction as A;
    let actions = |status, enabled: bool| {
        get_offline_bible_actions(&row(status, json!({"offlineDownloadEnabled": enabled})))
    };
    assert_eq!(actions(S::Downloaded, true), [A::Remove]);
    assert_eq!(actions(S::Stale, true), [A::Refresh, A::Remove]);
    assert_eq!(actions(S::Downloading, true), [A::Pause]);
    assert_eq!(actions(S::Paused, true), [A::Resume, A::Remove]);
    assert_eq!(actions(S::Partial, true), [A::Resume, A::Remove]);
    assert_eq!(actions(S::Unavailable, true), [A::Remove]);
    assert_eq!(actions(S::Failed, true), [A::Retry, A::Remove]);
    assert_eq!(actions(S::NotDownloaded, true), [A::Download]);
    assert!(actions(S::NotDownloaded, false).is_empty());
    for status in [S::NotDownloaded, S::Paused, S::Partial] {
        assert!(
            !actions(status, false)
                .iter()
                .any(|a| matches!(a, A::Download | A::Resume | A::Retry))
        );
    }

    assert_eq!(
        get_offline_bible_status_label(&row(S::Downloaded, json!({}))),
        "Downloaded"
    );
    assert_eq!(
        get_offline_bible_status_label(&row(S::Stale, json!({}))),
        "Refresh required"
    );
    assert_eq!(
        get_offline_bible_status_label(&row(S::Unavailable, json!({}))),
        "No longer licensed"
    );
    assert_eq!(
        get_offline_bible_status_label(&row(S::Downloading, json!({"cachedChapters": 100}))),
        "Downloading 100 of 1189 chapters"
    );

    let percent = |cached: f64, total: f64| {
        get_offline_bible_progress_percent(&row(
            S::Downloaded,
            json!({"cachedChapters": cached, "totalChapters": total}),
        ))
    };
    assert_eq!(
        (
            percent(594.0, 1188.0),
            percent(5.0, 0.0),
            percent(2000.0, 1189.0)
        ),
        (50.0, 0.0, 100.0)
    );

    let expiry = |at: Value| {
        format_offline_bible_expiry(&row(S::Downloaded, json!({"expiresAt": at})), NOW, |_| {
            "Aug 26, 2026".into()
        })
    };
    assert_eq!(expiry(Value::Null), "");
    assert_eq!(expiry(json!(NOW - 1.0)), "Expired");
    assert!(expiry(json!(NOW + 86_400_000.0)).starts_with("Expires "));

    let rows = [
        row(S::Downloaded, json!({})),
        row(
            S::NotDownloaded,
            json!({"bibleId": "nlt-01", "translation": "NLT"}),
        ),
    ];
    let progress: ApiBibleDownloadProgress = parse(
        json!({"bibleId": "nlt-01", "status": "downloading", "completedChapters": 10, "totalChapters": 1189, "cachedVerses": 200}),
    );
    let merged = merge_offline_download_progress(&rows, &progress);
    assert_eq!(
        (
            merged[0].status,
            merged[1].status,
            merged[1].cached_chapters,
            merged[1].cached_verses
        ),
        (S::Downloaded, S::Downloading, 10.0, 200.0)
    );
    let removal: ApiBibleDownloadProgress = parse(
        json!({"bibleId": "nkjv-01", "status": "not-downloaded", "completedChapters": 0, "totalChapters": 0, "cachedVerses": 0}),
    );
    let merged = merge_offline_download_progress(&rows[..1], &removal);
    assert_eq!(
        (
            merged[0].status,
            merged[0].cached_verses,
            merged[0].expires_at
        ),
        (S::NotDownloaded, 0.0, None)
    );
}

// ─── Performance (prompt 015 section 8: reference parse under 20 µs) ─────────

#[test]
#[ignore = "benchmark; run in release with --ignored --nocapture"]
fn reference_parse_speed() {
    let inputs = [
        "John 3:16",
        "1 cor 13 4 7",
        "jos 1 5 9",
        "Song of Solomon 2:1-4",
        "philipp 4:6",
        "for God so loved the world",
        "Revelation 22:21",
        "xyz 1:1",
    ];
    let mut runs: Vec<f64> = (0..21)
        .map(|_| {
            let rounds = 20_000;
            let started = Instant::now();
            for _ in 0..rounds {
                for input in inputs {
                    std::hint::black_box(normalize_scripture_query(std::hint::black_box(input)));
                }
            }
            started.elapsed().as_secs_f64() * 1e6 / (rounds * inputs.len()) as f64
        })
        .collect();
    runs.sort_by(f64::total_cmp);
    let (median, worst) = (runs[runs.len() / 2], runs[runs.len() - 1]);
    println!(
        "normalize_scripture_query: median {median:.3} µs, slowest run {worst:.3} µs per call"
    );
    assert!(
        median < 20.0,
        "reference parse median {median} µs exceeds 20 µs"
    );
}

#[test]
fn a_playlist_verse_keeps_its_own_reference_line() {
    use kairo_domain::scripture::query::{VERSE_LABEL_KEY, verse_reference_line};
    let mut verse: kairo_domain::scripture::ScriptureVerse = serde_json::from_value(
        serde_json::json!({"book": "Esther", "chapter": 8.0, "verse": 9.0, "text": "So"}),
    )
    .unwrap();
    assert_eq!(
        verse_reference_line(Some(&verse), "Esther 8:9", "NKJV", true),
        "Esther 8:9 (NKJV)"
    );
    verse
        .extra
        .insert(VERSE_LABEL_KEY.into(), "Esther 8:9 · New King James".into());
    // Shown as written: no translation added on top.
    assert_eq!(
        verse_reference_line(Some(&verse), "Esther 8:9", "NKJV", true),
        "Esther 8:9 · New King James"
    );
    verse.extra.insert(VERSE_LABEL_KEY.into(), "  ".into());
    assert_eq!(
        verse_reference_line(Some(&verse), "Esther 8:9", "NKJV", false),
        "Esther 8:9",
        "a blank line falls back"
    );
}

#[test]
fn a_passage_with_added_verses_is_titled_with_its_runs() {
    use kairo_domain::scripture::live_progress::verse_list_reference;
    assert_eq!(
        verse_list_reference("Leviticus", 26.0, &[3.0, 4.0, 5.0, 9.0, 10.0]),
        "Leviticus 26:3-5, 9-10"
    );
    assert_eq!(
        verse_list_reference("Ecclesiastes", 11.0, &[2.0, 1.0, 3.0, 3.0]),
        "Ecclesiastes 11:1-3"
    );
    assert_eq!(verse_list_reference("John", 3.0, &[16.0]), "John 3:16");
}
