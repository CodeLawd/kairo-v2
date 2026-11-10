use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::Instant,
};

use kairo_scripture::live_detector::{
    CitationEmission, LiveCitationDetector, LocalEvidence, TranscriptEvent, TranscriptKind,
};
use kairo_scripture::service::QuoteCandidate;
use serde_json::{Value, json};

fn replay_with_quotes(
    events: &[Value],
    timings: &mut Vec<u128>,
    quotes: &[QuoteCandidate],
) -> Vec<CitationEmission> {
    let mut detector = LiveCitationDetector::default();
    let mut found = Vec::new();
    let mut chapter = |book: &str, chapter: f64| {
        quotes
            .iter()
            .filter(|verse| verse.book == book && verse.chapter == chapter)
            .cloned()
            .collect()
    };
    let mut search = |_: &str| quotes.to_vec();
    for event in events {
        let input = TranscriptEvent {
            at_ms: event["atMs"].as_u64().unwrap(),
            kind: if event["type"] == "interim" {
                TranscriptKind::Interim
            } else {
                TranscriptKind::Final
            },
            text: event["text"].as_str().unwrap(),
            audio_epoch: event["audioEpoch"].as_u64(),
        };
        let start = Instant::now();
        found.extend(detector.process_with_evidence(
            input,
            LocalEvidence {
                chapter: (!quotes.is_empty()).then_some(&mut chapter),
                quote: (!quotes.is_empty()).then_some(&mut search),
                plan: None,
            },
        ));
        timings.push(start.elapsed().as_nanos());
    }
    found.extend(
        detector.advance_to(
            events
                .last()
                .map_or(0, |event| event["atMs"].as_u64().unwrap())
                + 5_001,
        ),
    );
    found
}

fn replay(events: &[Value], timings: &mut Vec<u128>) -> Vec<CitationEmission> {
    replay_with_quotes(events, timings, &[])
}

fn projection(r: CitationEmission) -> Value {
    json!([
        r.reference.book,
        r.reference.chapter as u32,
        r.reference.verse_start as u32,
        r.reference.verse_end.map(|end| end as u32),
        r.source.as_str(),
        r.received_at_ms,
        r.emitted_at_ms,
        r.translations,
        r.resolver.as_str(),
        r.confidence,
        r.reference.source_text,
    ])
}

fn event(at_ms: u64, kind: &str, text: &str) -> Value {
    json!({ "atMs": at_ms, "type": kind, "text": text })
}

#[test]
fn waits_for_fragment_and_cancels_chapter_timer() {
    let found = replay(
        &[
            event(0, "final", "Go to Romans."),
            event(500, "final", "chapter eight"),
            event(1_000, "interim", "twenty eight"),
            event(1_500, "final", "twenty eight"),
        ],
        &mut Vec::new(),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].reference.verse_start, 28.0);
    assert_eq!(found[0].emitted_at_ms, 1_500);

    let found = replay(
        &[event(500, "final", "Romans chapter eight")],
        &mut Vec::new(),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].reference.verse_start, 1.0);
    assert_eq!(found[0].received_at_ms, 500);
    assert_eq!(found[0].emitted_at_ms, 5_500);
}

#[test]
#[ignore = "requires Electron checkout and its node_modules"]
fn live_detector_matches_electron_replay() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let electron = std::env::var_os("KAIRO_ELECTRON_DIR")
        .map_or_else(|| root.join("../kairo-presenter"), std::path::PathBuf::from)
        .canonicalize()
        .unwrap();
    let source =
        std::fs::read(electron.join("tests/fixtures/scripture-replay/quotation-corpus.json"))
            .unwrap();
    let corpus: Value = serde_json::from_slice(&source).unwrap();
    let mut cases: Vec<Vec<Value>> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scenario| scenario["events"].as_array().unwrap().clone())
        .collect();
    for book in [
        "John",
        "Luke",
        "Genesis",
        "Philippians",
        "Psalm",
        "First Corinthians",
    ] {
        for chapter in [1, 2, 3, 8, 13, 23, 115] {
            for verse in [1, 4, 16, 28] {
                cases.push(vec![
                    event(0, "interim", &format!("{book} {chapter}")),
                    event(200, "interim", &format!("{book} {chapter} {verse}")),
                    event(
                        400,
                        "final",
                        &format!("{book} chapter {chapter} verse {verse}"),
                    ),
                    event(800, "final", "Read that in the NIV."),
                    event(1_000, "final", &format!("{book} chapter {chapter}")),
                ]);
            }
        }
    }

    for book in ["Psalm", "John", "Hebrews"] {
        let chapter = if book == "Psalm" { 66 } else { 3 };
        for separator in [" ", ", ", ","] {
            for connector in [
                "",
                "verse ",
                "we will read from verse ",
                "we shall read from verse ",
                "we begin to read from verse ",
            ] {
                let text = format!("{book} {chapter}{separator}{connector}eight to nine.");
                cases.push(vec![event(0, "interim", &text), event(350, "final", &text)]);
            }
        }
    }
    cases.push(vec![
        event(0, "final", "John chapter three"),
        event(13_000, "final", "verse sixteen"),
        event(14_000, "interim", "Romans 8:28"),
    ]);
    cases.push(vec![
        event(0, "final", "Second Corinthians"),
        event(250, "final", "chapter seven"),
        event(500, "final", "verses two, five, and nine"),
    ]);
    cases.push(vec![
        event(0, "final", "In John chapter three."),
        event(
            400,
            "final",
            "For God so loved the world, that he gave his only begotten Son.",
        ),
    ]);
    cases.push(vec![
        event(0, "final", "Psalm 1one512-thirteen says, The Lord"),
        event(400, "final", "has been mindful of you."),
    ]);
    cases.push(vec![
        event(0, "final", "Psalm 1one512-thirteen says, The Lord"),
        event(400, "final", "Paul writes in Romans"),
        event(800, "final", "8:28"),
    ]);
    let quotes = [
        QuoteCandidate {
            book: "John".into(), chapter: 3.0, verse: 16.0,
            text: "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.".into(),
        },
        QuoteCandidate {
            book: "Psalms".into(), chapter: 115.0, verse: 12.0,
            text: "The Lord hath been mindful of us: he will bless us.".into(),
        },
    ];
    let quote_input: Vec<_> = quotes
        .iter()
        .map(|q| {
            json!({
                "book": q.book, "chapter": q.chapter, "verse": q.verse, "text": q.text,
            })
        })
        .collect();
    let mut child = Command::new(electron.join("node_modules/.bin/electron"))
        .args(["--import", "tsx", "--require", "./scripts/test-setup.cjs"])
        .arg(root.join("tools/ts-oracle/modules/detector-replay/child.mts"))
        .current_dir(&electron)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&json!({ "cases": cases, "quotes": quote_input })).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<Vec<Value>> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(expected.len(), cases.len());
    let mut timings = Vec::new();
    for (events, expected) in cases.iter().zip(expected) {
        let found: Vec<_> = replay_with_quotes(events, &mut timings, &quotes)
            .into_iter()
            .map(projection)
            .collect();
        assert_eq!(found, expected, "{events:?}");
    }
    timings.sort_unstable();
    eprintln!(
        "{} scenarios, {} transcript events, p99 {} ns",
        cases.len(),
        timings.len(),
        timings[(timings.len() * 99).div_ceil(100) - 1]
    );
}

#[test]
fn ports_spoken_citation_gap_cases() {
    for (segments, expected) in [
        (&["John chapter three from verse sixteen."][..], "John 3:16"),
        (
            &["Turn with me to John chapter 3, beginning at verse 16."][..],
            "John 3:16",
        ),
        (
            &["Romans chapter 8 starting from verse 28."][..],
            "Romans 8:28",
        ),
        (&["Hebrews 11 in verse 6"][..], "Hebrews 11:6"),
        (
            &["Romans chapter 8, and the Bible says in verse 28"][..],
            "Romans 8:28",
        ),
        (&["Hebrews chapter 11, the 6th verse"][..], "Hebrews 11:6"),
        (&["Romans chapter 8. Look at verse 28."][..], "Romans 8:28"),
        (
            &["Let us read verse 28 of Romans chapter 8."][..],
            "Romans 8:28",
        ),
        (&["In verse 28 of Romans 8, Paul says"][..], "Romans 8:28"),
        (&["Verses 28 to 30 of Romans 8."][..], "Romans 8:28-30"),
        (
            &["Go to Romans.", "Chapter 8.", "Verse 28."][..],
            "Romans 8:28",
        ),
        (
            &[
                "Open your Bible to the book of Romans.",
                "Chapter 8 verse 28.",
            ][..],
            "Romans 8:28",
        ),
        (
            &["Go to Romans.", "So chapter 8 verse 28."][..],
            "Romans 8:28",
        ),
        (
            &["Go to Romans.", "Now, chapter 8, verse 28."][..],
            "Romans 8:28",
        ),
        (
            &["Psalm one hundred and nineteen verse one hundred and five"][..],
            "Psalms 119:105",
        ),
        (&["1st Corinthians 13 verse 4"][..], "1 Corinthians 13:4"),
        (&["2nd Chronicles 7:14"][..], "2 Chronicles 7:14"),
        (&["3rd John verse 4"][..], "3 John 1:4"),
        (&["I Corinthians 13:4"][..], "1 Corinthians 13:4"),
        (&["II Timothy 3:16"][..], "2 Timothy 3:16"),
        (&["Phillipians 4:13"][..], "Philippians 4:13"),
        (
            &["Philipians chapter four verse thirteen"][..],
            "Philippians 4:13",
        ),
        (&["Philippines 4:13"][..], "Philippians 4:13"),
        (&["Hebrew 11:1"][..], "Hebrews 11:1"),
        (&["Roman 8:28"][..], "Romans 8:28"),
        (&["Song of Songs 2:4"][..], "Song of Solomon 2:4"),
        (
            &["Go to Phillipians.", "Chapter 4 verse 13."][..],
            "Philippians 4:13",
        ),
        (&["Luke 4:18,19"][..], "Luke 4:18-19"),
        (&["Luke 4 18, 19"][..], "Luke 4:18-19"),
        (&["John 316."][..], "John 3:16"),
        (&["Romans 828."][..], "Romans 8:28"),
        (&["Jeremiah 2911."][..], "Jeremiah 29:11"),
        (&["Philippians 413"][..], "Philippians 4:13"),
        (&["Psalm 1195"][..], "Psalms 119:5"),
        (&["Genesis 1 versus 2."][..], "Genesis 1:2"),
        (&["John's 3:16."][..], "John 3:16"),
        (&["Roman's 8:28."][..], "Romans 8:28"),
        (
            &["Numbers chapter 17, uh, verse number 4."][..],
            "Numbers 17:4",
        ),
        (&["Romans. 8. 28."][..], "Romans 8:28"),
        (&["Psalm one eleven two."][..], "Psalms 111:2"),
        (&["Psalms one hundred and ten five."][..], "Psalms 110:5"),
        (&["Romans 8, verse 28."][..], "Romans 8:28"),
        (&["Romans eight twenty eight."][..], "Romans 8:28"),
        (
            &["Romans chapter 8.", "Look at verse 28."][..],
            "Romans 8:28",
        ),
        (&["Isaiah 40.", "31."][..], "Isaiah 40:31"),
        (&["Proverbs 3 5 and 6"][..], "Proverbs 3:5-6"),
        (&["Galatians 5 22 and 23"][..], "Galatians 5:22-23"),
    ] {
        let events: Vec<_> = segments
            .iter()
            .enumerate()
            .map(|(index, text)| event(index as u64 * 100, "final", text))
            .collect();
        let found = replay(&events, &mut Vec::new());
        let keys: Vec<_> = found
            .into_iter()
            .map(|r| {
                let r = r.reference;
                let end = r
                    .verse_end
                    .map_or_else(String::new, |end| format!("-{end}"));
                format!("{} {}:{}{end}", r.book, r.chapter, r.verse_start)
            })
            .collect();
        assert_eq!(keys, [expected], "{segments:?}");
    }
}
