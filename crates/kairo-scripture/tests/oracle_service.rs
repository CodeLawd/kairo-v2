//! Prompt 033: `ScriptureService` against Electron's, through `kairo-oracle`.
//!
//! The cached comparison builds the same generated Bible on both sides (six
//! translations, including an unregistered id, a lower-case id and an
//! incomplete registry pack) and runs a corpus of references, phrases,
//! FTS-syntax edge cases, quote and chapter helpers, options, status and the
//! installed-pack list. Results must match exactly, ordering included.
//!
//! The ignored test repeats this against the real bundled `bible.db` and
//! measures search latency. Refresh the cache with
//! `cargo run -p ts-oracle -- --regenerate`.

mod common;

use common::block_on;
use kairo_domain::json::to_js_value;
use kairo_scripture::ScriptureService;
use kairo_scripture::bible_db::BibleDb;
use kairo_scripture::reference::parse_scripture_reference;
use kairo_scripture::verse_bounds::is_valid_scripture_reference;
use kairo_store::db::PackVerse;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

fn js<T: Serialize + ?Sized>(value: &T) -> Value {
    to_js_value(value).unwrap()
}

/// A small deterministic generator (64-bit LCG), so both sides see the same corpus.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next() as usize % items.len()]
    }
}

const WORDS: &[&str] = &[
    "the",
    "and",
    "of",
    "LORD",
    "is",
    "my",
    "shepherd",
    "love",
    "patient",
    "kind",
    "grace",
    "peace",
    "faith",
    "hope",
    "light",
    "darkness",
    "water",
    "spirit",
    "heaven",
    "earth",
    "word",
    "world",
    "life",
    "eternal",
    "son",
    "God",
    "gave",
    "believe",
    "perish",
    "everlasting",
    "beginning",
    "created",
    "green",
    "pastures",
    "still",
    "waters",
    "restoreth",
    "soul",
    "righteousness",
    "valley",
    "shadow",
    "death",
    "fear",
    "evil",
    "rod",
    "staff",
    "comfort",
    "table",
    "enemies",
    "cup",
    "runneth",
    "over",
    "goodness",
    "mercy",
    "dwell",
    "house",
    "forever",
    "charity",
    "envieth",
    "vaunteth",
    "puffed",
    "seeketh",
    "own",
    "thinketh",
    "rejoiceth",
    "truth",
    "beareth",
    "all",
    "things",
    "café",
    "naïve",
    "co-heirs",
    "don't",
    "Jesus'",
    "Zion",
    "it",
    "he",
    "was",
    "to",
    "in",
    "for",
    "be",
];

/// Chapters each translation holds: (book id, chapter, verses).
const CHAPTERS: &[(u32, u32, u32)] = &[
    (1, 1, 12),
    (19, 23, 6),
    (43, 3, 18),
    (46, 13, 13),
    (41, 9, 8),
    (64, 1, 15),
    (65, 1, 5),
    (50, 1, 4),
    (57, 1, 4),
];

fn sentence(rng: &mut Lcg) -> String {
    let len = 6 + rng.next() as usize % 14;
    let mut words: Vec<String> = (0..len).map(|_| rng.pick(WORDS).to_string()).collect();
    match rng.next() % 9 {
        0 => words.insert(2, "{ye}".into()),
        1 => words.push("{note: Heb. expansion}".into()),
        2 => words[0] = format!("\"{}", words[0]),
        3 => words.insert(1, "(selah)".into()),
        _ => {}
    }
    let punctuation = [".", ";", ",", ":", "!", "?"];
    let mut text = String::new();
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            text.push_str(if rng.next().is_multiple_of(7) { ", " } else { " " });
        }
        text.push_str(word);
    }
    text.push_str(rng.pick(&punctuation));
    text
}

struct Translation {
    id: &'static str,
    name: &'static str,
    is_default: bool,
    verses: Vec<PackVerse>,
}

fn fixture() -> Vec<Translation> {
    let mut rng = Lcg(33);
    let ids: &[(&str, &str, bool)] = &[
        ("KJV", "King James Version", true),
        ("BSB", "Berean Standard Bible", false),
        ("NKJV", "Synthetic Fixture Translation", false),
        ("ZZT", "", false),
        ("web", "World English (lower-case id)", false),
        ("AMPC", "Amplified Bible, Classic Edition", false),
    ];
    ids.iter()
        .enumerate()
        .map(|(i, &(id, name, is_default))| {
            let mut verses = Vec::new();
            for (n, &(book_id, chapter, count)) in CHAPTERS.iter().enumerate() {
                // Not every translation holds every chapter.
                if (n + i) % 4 == 3 {
                    continue;
                }
                for verse in 1..=count {
                    verses.push(PackVerse {
                        book_id,
                        chapter,
                        verse,
                        text: sentence(&mut rng),
                    });
                }
            }
            // A shared exact verse across translations for ordering by translation.
            verses.push(PackVerse {
                book_id: 19,
                chapter: 117,
                verse: 1,
                text: "O praise the LORD, all ye nations: praise him, all ye people.".into(),
            });
            Translation {
                id,
                name,
                is_default,
                verses,
            }
        })
        .collect()
}

fn fixture_json(translations: &[Translation]) -> Value {
    Value::Array(
        translations
            .iter()
            .map(|t| {
                let rows: Vec<Value> = t
                    .verses
                    .iter()
                    .map(|v| json!([v.book_id, v.chapter, v.verse, v.text]))
                    .collect();
                json!([t.id, t.name, "en", t.is_default, rows])
            })
            .collect(),
    )
}

fn phrase_queries(translations: &[Translation]) -> Vec<String> {
    let mut rng = Lcg(7);
    let mut queries: Vec<String> = [
        "",
        "   ",
        "the and of",
        "is",
        "love",
        "LOVE patient",
        "patient love",
        "love is patient",
        "shepherd lord",
        "the LORD is my shepherd",
        "grace peace",
        "faith hope love",
        "praise the LORD, all ye nations",
        "praise nations",
        "ye people",
        "selah",
        "Heb expansion",
        "note",
        "\"grace\"",
        "faith*",
        "hope OR",
        "NEAR(love grace)",
        "text:love",
        "love AND peace",
        "love NOT peace",
        "don't",
        "co-heirs",
        "Jesus'",
        "café",
        "naive",
        "naïve",
        "123",
        "love 123",
        "zzz",
        "{ye} that",
        "(selah) grace",
        "^love",
        "a an the of",
        "everlasting life",
        "life everlasting",
        "valley shadow death",
        "cup runneth over",
        "goodness mercy dwell house forever",
        "charity envieth not",
        "zion",
        "light darkness",
        "world gave son believe",
        "x y",
        "lo",
        "love\npeace",
        "love\u{2028}peace",
        "grace\u{a0}peace",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // Exact spans and shuffled word pairs from the corpus itself.
    for _ in 0..60 {
        let t = rng.pick(translations);
        let verse = rng.pick(&t.verses);
        let words: Vec<&str> = verse.text.split(' ').collect();
        let start = rng.next() as usize % words.len();
        let len = 1 + rng.next() as usize % 5;
        let end = (start + len).min(words.len());
        queries.push(words[start..end].join(" "));
        let a = rng.pick(&words);
        let b = rng.pick(&words);
        queries.push(format!("{b} {a}"));
    }
    queries
}

const REFERENCES: &[&str] = &[
    "John 3:16",
    "john 3 16",
    "Jn 3:16-18",
    "jhn 3:16–18",
    "John 3:16 — 18",
    "Psalm 23",
    "ps 23",
    "Psalms 23:1-6",
    "Genesis 1:1–3",
    "Gen 1",
    "gen. 1:2",
    "1 Cor 13:4",
    "1co 13 4 7",
    "I Corinthians 13:4",
    "Mark 9:44",
    "Mark 9:43-48",
    "Mark 9",
    "Foo 3:16",
    "Phil 1:1",
    "phi 1:1",
    "philem 1:2",
    "Rev 22:21",
    "John 3:0",
    "John 3:16-0",
    "John 3:16-14",
    "John 999",
    "John 3:99999999999999999999",
    "Psalms 151",
    "Psalms 0",
    "Psalm 117",
    "Psalm 117:1",
    "3 John 1:15",
    "3 John 1:14",
    "Jude 1",
    "jude 1:3",
    "obadiah 1",
    "zzz 1:1",
    "j 3:16",
    "Jo 3:16",
    "Song 1:1",
    "  John   3:16  ",
    "John 3:16a",
    "John 3:16-18-20",
    "John3:16",
];

const TRANSLATIONS: &[Option<&str>] = &[
    None,
    Some("KJV"),
    Some("BSB"),
    Some("kjv"),
    Some("NKJV"),
    Some("NIV"),
    Some("ZZT"),
    Some("web"),
    Some("WEB"),
    Some("AMPC"),
];

/// The corpus as oracle operations.
fn operations(translations: &[Translation]) -> Vec<Value> {
    let mut ops = Vec::new();
    for query in REFERENCES
        .iter()
        .map(|s| s.to_string())
        .chain(phrase_queries(translations))
    {
        for translation in TRANSLATIONS {
            ops.push(json!({ "op": "search", "query": query, "translation": translation }));
        }
    }
    for phrase in [
        "the LORD is my shepherd",
        "you and me",
        "it is he",
        "hath been grace",
        "",
    ] {
        ops.push(json!({ "op": "quote", "phrase": phrase }));
    }
    for (book, chapter) in [
        ("John", 3),
        ("ps", 23),
        ("Psalms", 117),
        ("nope", 1),
        ("Mark", 9),
    ] {
        ops.push(json!({ "op": "chapterVerses", "book": book, "chapter": chapter }));
    }
    ops.push(json!({ "op": "options" }));
    for translation in ["NKJV", "nkjv", "ZZT", "KJV", "AMPC", "NIV", "web"] {
        ops.push(json!({ "op": "status", "translation": translation }));
    }
    ops.push(json!({ "op": "installed" }));
    ops
}

fn outcome<T: Serialize>(result: Result<T, kairo_scripture::ScriptureError>) -> Value {
    match result {
        Ok(value) => js(&value),
        Err(error) => json!({ "throw": error.to_string() }),
    }
}

fn candidates(list: Vec<kairo_scripture::service::QuoteCandidate>) -> Value {
    Value::Array(
        list.into_iter()
            .map(|c| json!({ "book": c.book, "chapter": c.chapter, "verse": c.verse, "text": c.text }))
            .collect(),
    )
}

async fn run(service: &ScriptureService, ops: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for op in ops {
        let s = |key: &str| op[key].as_str().unwrap_or_default().to_owned();
        out.push(match op["op"].as_str().unwrap() {
            "search" => outcome(
                service
                    .search(&s("query"), op["translation"].as_str())
                    .await,
            ),
            "quote" => match service.search_local_quote_candidates(&s("phrase")).await {
                Ok(list) => candidates(list),
                Err(error) => json!({ "throw": error.to_string() }),
            },
            "chapterVerses" => {
                let chapter = op["chapter"].as_f64().unwrap();
                match service.get_local_chapter_verses(&s("book"), chapter).await {
                    Ok(list) => candidates(list),
                    Err(error) => json!({ "throw": error.to_string() }),
                }
            }
            "options" => outcome(service.get_translation_options(false, false).await),
            "status" => outcome(service.get_local_bible_pack_status(&s("translation")).await),
            "installed" => outcome(service.list_installed_local_bible_packs().await),
            other => panic!("unknown op {other}"),
        });
    }
    out
}

fn compare_runs(name: &str, args: &Value, ops: &[Value], actual: Vec<Value>) {
    let expected = kairo_oracle::oracle_value(name, args.clone());
    let expected = expected.as_array().expect("oracle results");
    assert_eq!(expected.len(), actual.len());
    let mut failures = Vec::new();
    for ((op, expected), actual) in ops.iter().zip(expected).zip(&actual) {
        let differences = kairo_oracle::compare(expected, actual);
        if !differences.is_empty() {
            failures.push(format!("{op}\n  {}", differences.join("\n  ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} operations differ:\n{}",
        failures.len(),
        ops.len(),
        failures
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn service_matches_electron_on_a_generated_bible() {
    let translations = fixture();
    let ops = operations(&translations);
    let dir = tempfile::tempdir().unwrap();
    let actual = block_on(async {
        let db = BibleDb::open(&dir.path().join("fixture.db")).await.unwrap();
        for t in &translations {
            db.import_translation(t.id, t.name, "en", t.verses.clone(), t.is_default)
                .await
                .unwrap();
        }
        let service = ScriptureService::new(Arc::new(common::NoNetwork));
        service.attach_db(db);
        run(&service, &ops).await
    });
    // The corpus must exercise hits, misses and both search paths.
    let hits = actual
        .iter()
        .filter(|v| v.as_array().is_some_and(|a| !a.is_empty()))
        .count();
    let thrown = actual.iter().filter(|v| v.get("throw").is_some()).count();
    eprintln!(
        "{} operations, {hits} with results, {thrown} thrown",
        ops.len()
    );
    assert!(
        hits > ops.len() / 3 && hits < ops.len() - 50,
        "{hits} of {}",
        ops.len()
    );
    assert_eq!(thrown, 0);
    let args = json!([fixture_json(&translations), ops]);
    compare_runs("oracle:scripture-service.run", &args, &ops, actual);
}

#[test]
fn references_parse_as_electron_does() {
    let mut inputs: Vec<String> = REFERENCES.iter().map(|s| s.to_string()).collect();
    inputs.extend(
        [
            "",
            " ",
            "love 123",
            "Psalms 23\n",
            "Psalms\u{2028}23",
            "Foo\u{a0}3:16",
            "Foo 3:16 ",
            "a 1:2",
            "1 2:3",
            "x 01:002",
            "Book 3:4 - 5",
            "Book 3:4 -5",
            "Book 3:4- 5",
            "Book 3:4—",
            "Romans 8:28—30",
            "Genesis 21:1–2",
            "jos 1 5 9",
            "joshu 1:9",
            "ez 1",
            "Song of Songs 2",
            "1 Thess 4:13-18",
            "Psalm 119:105",
            "Rev. 21:4",
            "1st John 1:9",
            "First Peter 5:7",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    let cases: Vec<_> = inputs
        .iter()
        .map(|input| {
            let output = match parse_scripture_reference(input) {
                None => Value::Null,
                Some(r) if r.is_chapter => {
                    json!({ "book": r.book, "chapter": r.chapter, "isChapter": true })
                }
                Some(r) => {
                    let mut value = json!({ "book": r.book, "chapter": r.chapter, "verseStart": r.verse_start });
                    if let Some(end) = r.verse_end {
                        value["verseEnd"] = json!(end);
                    }
                    value
                }
            };
            (json!([input]), output)
        })
        .collect();
    kairo_oracle::assert_all_match("oracle:scripture-service.parseReference", &cases);
}

#[test]
fn verse_bounds_match_electron() {
    let mut cases = Vec::new();
    for book in [
        "Genesis",
        "3 John",
        "Matthew",
        "Revelation",
        "Psalms",
        "Nope",
        "constructor",
        "genesis",
    ] {
        for chapter in [0.0, 1.0, 2.0, 22.0, 26.0, 150.0, 151.0, 1.5] {
            for (start, end) in [
                (1.0, None),
                (0.0, None),
                (14.0, Some(15.0)),
                (15.0, None),
                (3.0, Some(2.0)),
                (20.0, Some(74.0)),
                (2.0, Some(2.5)),
            ] {
                let args = match end {
                    Some(end) => json!([book, chapter, start, end]),
                    None => json!([book, chapter, start]),
                };
                cases.push((
                    args,
                    json!(is_valid_scripture_reference(book, chapter, start, end)),
                ));
            }
        }
    }
    kairo_oracle::assert_all_match("oracle:scripture-service.verseBounds", &cases);
}

/// The real bundled Bible: a larger corpus, then search latency.
#[test]
#[ignore = "Requires the Electron checkout's resources/bible.db and Electron as Node"]
fn service_matches_electron_on_the_bundled_bible_and_search_is_fast() {
    let bundled = kairo_oracle::electron_dir().join("resources/bible.db");
    let dir = tempfile::tempdir().unwrap();
    let service = ScriptureService::new(Arc::new(common::NoNetwork));
    block_on(service.open(dir.path(), &bundled)).unwrap();

    // Phrases: spans of real verses, every 97th verse of each translation.
    // A second handle: opening includes building its in-memory phrase index.
    let start = std::time::Instant::now();
    let db = block_on(BibleDb::open(&dir.path().join("bible.db"))).unwrap();
    let opened = start.elapsed();
    let (verses, bytes): (i64, i64) = block_on(db.database().read(|conn| {
        conn.query_row(
            "SELECT count(*), sum(length(CAST(text AS BLOB))) FROM verses",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    }))
    .unwrap();
    eprintln!("open with phrase index: {opened:?} for {verses} verses, {bytes} bytes of text");
    let texts: Vec<(String, String)> = block_on(db.database().read(|conn| {
        let mut statement = conn.prepare(
            "SELECT translation_id, text FROM verses WHERE rowid % 97 = 0 ORDER BY rowid LIMIT 400",
        )?;
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
    }))
    .unwrap();
    let mut rng = Lcg(2026);
    let mut queries: Vec<(String, Option<String>)> = Vec::new();
    for (translation, text) in &texts {
        let words: Vec<&str> = text.split_whitespace().collect();
        let start = rng.next() as usize % words.len();
        let len = 2 + rng.next() as usize % 5;
        let span = words[start..(start + len).min(words.len())].join(" ");
        let other = ["KJV", "BSB", "BBE", "NIV"][rng.next() as usize % 4];
        queries.push((span.clone(), Some(translation.clone())));
        queries.push((span.to_lowercase(), Some(other.into())));
    }
    for reference in REFERENCES {
        for translation in ["KJV", "BSB", "BBE"] {
            queries.push((reference.to_string(), Some(translation.into())));
        }
    }
    for phrase in [
        "love is patient",
        "the lord is my shepherd",
        "for god so loved the world",
        "in the beginning",
        "grace",
        "faith hope love",
    ] {
        queries.push((phrase.into(), None));
    }
    let ops: Vec<Value> = queries
        .iter()
        .map(|(query, translation)| json!({ "op": "search", "query": query, "translation": translation }))
        .chain([json!({ "op": "options" }), json!({ "op": "installed" })])
        .collect();

    let mut timings = Vec::new();
    let actual = block_on(async {
        let mut out = Vec::new();
        for op in &ops {
            let start = std::time::Instant::now();
            out.extend(run(&service, std::slice::from_ref(op)).await);
            if op["op"] == "search" {
                timings.push(start.elapsed());
            }
        }
        out
    });
    timings.sort_unstable();
    let at = |q: f64| timings[((timings.len() as f64 * q).ceil() as usize).saturating_sub(1)];
    eprintln!(
        "{} searches on the bundled Bible: p50 {:?}, p95 {:?}, p99 {:?}, max {:?}",
        timings.len(),
        at(0.50),
        at(0.95),
        at(0.99),
        timings.last().unwrap()
    );

    let mut oracle = kairo_oracle::Oracle::new(
        kairo_oracle::Mode::Live,
        dir.path().into(),
        kairo_oracle::electron_dir(),
    );
    let args = json!([bundled, ops]);
    let expected = oracle
        .results("oracle:scripture-service.runDb", &[args])
        .unwrap()
        .remove(0);
    let expected = expected.as_array().unwrap();
    let mut failures = Vec::new();
    for ((op, expected), actual) in ops.iter().zip(expected).zip(&actual) {
        let differences = kairo_oracle::compare(expected, actual);
        if !differences.is_empty() {
            failures.push(format!("{op}\n  {}", differences.join("\n  ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} operations differ:\n{}",
        failures.len(),
        ops.len(),
        failures
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    eprintln!(
        "{} operations match Electron on the bundled Bible",
        ops.len()
    );

    // Electron's own service on the same queries, for comparison (timing only).
    let timed: Vec<Value> = ["lord shepherd", "love patient", "beginning god created"]
        .iter()
        .map(|query| json!({ "op": "timedSearch", "query": query, "translation": "KJV", "runs": 30 }))
        .collect();
    let electron = oracle
        .results("oracle:scripture-service.runDb", &[json!([bundled, timed])])
        .unwrap()
        .remove(0);
    for (op, median) in timed.iter().zip(electron.as_array().unwrap()) {
        eprintln!("Electron {}: median {median} ms", op["query"]);
    }

    // baseline.md's warm KJV FTS queries, here through the whole service
    // search: one warmup and 30 timed runs each.
    for query in ["lord shepherd", "love patient", "beginning god created"] {
        let mut runs: Vec<_> = (0..31)
            .map(|_| {
                let start = std::time::Instant::now();
                let hits = block_on(service.search(query, Some("KJV"))).unwrap().len();
                (start.elapsed(), hits)
            })
            .skip(1)
            .collect();
        runs.sort_unstable();
        eprintln!(
            "{query}: {} results, median {:?}, range {:?}–{:?}",
            runs[0].1, runs[15].0, runs[0].0, runs[29].0
        );
    }
}
