//! Prompt 037: global quotation search. Ported from Electron's
//! global-quote, cache-quote-adapter and quote-recovery tests, plus a
//! corpus differential against Electron's `matchGlobalQuote` through
//! `kairo-oracle`, with scores compared to a tolerance of 1e-9 (scores are
//! exact multiples of 0.5, so any real difference is at least 0.5).
//!
//! The ignored test repeats the differential with retrieval against the
//! real bundled `bible.db` and measures p95 search latency on both sides.

mod common;

use kairo_domain::scripture::ApiBibleCacheStatus;
use kairo_scripture::api_bible::ApiBibleCache;
use kairo_scripture::api_bible::cache::{
    ApiBibleCachePassage, ApiBibleTranslationMeta, CacheQuoteSource, CachedQuoteVerse,
};
use kairo_scripture::api_bible::policy::API_CACHE_MAX_AGE_MS;
use kairo_scripture::bible_db::BibleDb;
use kairo_scripture::quote::QuoteCorpus;
use kairo_scripture::quote::index::CacheQuoteIndex;
use kairo_scripture::quote::matching::{
    GlobalQuoteCandidate, fresh_speech_supports, match_global_quote,
};
use kairo_scripture::quote::retrieval::retrieve_global_quote;
use kairo_scripture::quote::worker::{
    Command, Event, QuoteSearchOutcome, QuoteWorkerClient, Spawner, run_worker,
};
use kairo_store::db::PackVerse;
use kairo_store::secrets::{ApiCacheCrypto, KeyProtector, SecretError};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

fn cand(book: &str, chapter: f64, verse: f64, text: &str) -> GlobalQuoteCandidate {
    GlobalQuoteCandidate {
        book: book.into(),
        chapter,
        verse,
        text: text.into(),
        ..Default::default()
    }
}

const LUKE: &str = "Give, and it shall be given unto you; good measure, pressed down, and shaken together, and running over.";
const JOHN: &str = "Love not the world, neither the things that are in the world. If any man love the world, the love of the Father is not in him.";

fn luke() -> GlobalQuoteCandidate {
    cand("Luke", 6.0, 38.0, LUKE)
}
fn john() -> GlobalQuoteCandidate {
    cand("1 John", 2.0, 15.0, JOHN)
}

fn top(text: &str, candidates: &[GlobalQuoteCandidate]) -> Option<(String, f64)> {
    match_global_quote(text, candidates, None)
        .first()
        .map(|m| (m.candidate.book.clone(), m.candidate.verse))
}

// ─── global-quote.test.ts ──────────────────────────────────────────────────

#[test]
fn quotations_resolve_with_commentary_negation_and_one_error() {
    let both = [luke(), john()];
    assert_eq!(
        top(
            "Listen church, give and it shall be given unto you good measure pressed down",
            &both
        )
        .unwrap()
        .0,
        "Luke"
    );
    assert_eq!(
        top(
            "Love not the world neither the things that are in the world",
            &both
        )
        .unwrap()
        .0,
        "1 John"
    );
    assert!(
        match_global_quote(
            "Love the world and its things because the world is good",
            &both,
            None
        )
        .is_empty()
    );
    assert_eq!(
        top(
            "give and it shall be given unto you good pressure pressed down and shaken together",
            &both
        )
        .unwrap()
        .0,
        "Luke"
    );
    for text in [
        "God is good and we love him",
        "give generously to your neighbours this week",
        "the world is full of things",
    ] {
        assert!(match_global_quote(text, &both, None).is_empty(), "{text}");
    }
}

#[test]
fn translations_merge_and_distinct_passages_stay_alternatives() {
    let text = "Give and it shall be given unto you good measure pressed down";
    let one = match_global_quote(text, &[luke(), luke()], None);
    assert_eq!(one.len(), 1);
    assert!(!one[0].ambiguous);
    let two = match_global_quote(
        &text.to_lowercase(),
        &[luke(), cand("Matthew", 7.0, 2.0, LUKE)],
        None,
    );
    assert_eq!(two.len(), 2);
    assert!(two.iter().all(|m| m.ambiguous));
    let gospels = match_global_quote(
        "If I may but touch his garment I shall be whole",
        &[
            cand(
                "Matthew",
                9.0,
                21.0,
                "For she said within herself, If I may but touch his garment, I shall be whole.",
            ),
            cand(
                "Mark",
                5.0,
                28.0,
                "For she said, If I may touch but his clothes, I shall be whole.",
            ),
        ],
        None,
    );
    assert!(!gospels.is_empty());
}

#[test]
fn formulae_and_negated_quotations_do_not_match() {
    let narration = cand(
        "Luke",
        1.0,
        1.0,
        "And it came to pass that he said unto them, behold, hear my words.",
    );
    assert!(
        match_global_quote(
            "and it came to pass that he said unto them behold",
            &[narration],
            None
        )
        .is_empty()
    );
    assert!(
        match_global_quote(
            "Do not give and it shall be given unto you good measure pressed down",
            &[luke()],
            None
        )
        .is_empty()
    );
    let short = cand(
        "Luke",
        6.0,
        38.0,
        "Give, and it shall be given unto you; good measure, pressed down, and shaken together",
    );
    let m = match_global_quote(
        "do not be afraid church give and it shall be given unto you good measure pressed down",
        &[short],
        None,
    );
    assert_eq!(m[0].candidate.verse, 38.0);
}

#[test]
fn paraphrases_are_possible_only_with_rare_words() {
    let hem = cand(
        "Matthew",
        14.0,
        36.0,
        "And besought him that they might only touch the hem of his garment: and as many as touched were made perfectly whole.",
    );
    let counts: HashMap<&str, f64> = [
        ("touch", 52.0),
        ("hem", 6.0),
        ("garment", 83.0),
        ("whole", 244.0),
    ]
    .into();
    let mut rarity = |w: &str| counts.get(w).copied().unwrap_or(1000.0);
    let heard = "If only I touch the hem of his garments, I shall be made whole.";
    assert!(match_global_quote(heard, std::slice::from_ref(&hem), None).is_empty());
    let possible = match_global_quote(heard, &[hem], Some(&mut rarity));
    assert_eq!(
        (possible[0].candidate.verse, possible[0].ambiguous),
        (36.0, true)
    );

    let jer = cand(
        "Jeremiah",
        51.0,
        50.0,
        "Ye that have escaped the sword, go away, stand not still: remember the LORD afar off, and let Jerusalem come into your mind.",
    );
    let common: HashMap<&str, f64> = [("stand", 275.0), ("off", 521.0), ("let", 1239.0)].into();
    let mut rarity = |w: &str| common.get(w).copied().unwrap_or(1000.0);
    assert!(
        match_global_quote(
            "Please turn off your phones and let us stand for the word of God",
            &[jer],
            Some(&mut rarity)
        )
        .is_empty()
    );
}

#[test]
fn fresh_speech_support() {
    assert!(fresh_speech_supports("Good measure.", LUKE));
    assert!(fresh_speech_supports("pressed down and shaken", LUKE));
    assert!(!fresh_speech_supports("now turn to your neighbour", LUKE));
    assert!(!fresh_speech_supports("", LUKE));
}

// ─── Retrieval over a local Bible ──────────────────────────────────────────

const FIXTURE: &[(u32, u32, u32, &str)] = &[
    (42, 6, 38, LUKE),
    (62, 2, 15, JOHN),
    (
        62,
        4,
        4,
        "Ye are of God, little children, and have overcome them: because greater is he that is in you, than he that is in the world.",
    ),
    (
        40,
        7,
        2,
        "For with what judgment ye judge, ye shall be judged: and with what measure ye mete, it shall be measured to you again.",
    ),
    (19, 23, 1, "The LORD is my shepherd; I shall not want."),
    (
        43,
        3,
        16,
        "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
    ),
    (
        40,
        14,
        36,
        "And besought him that they might only touch the hem of his garment: and as many as touched were made perfectly whole.",
    ),
];

fn fixture_db(dir: &Path) -> PathBuf {
    common::block_on(async {
        let db = BibleDb::open(&dir.join("bible.db")).await.unwrap();
        for id in ["KJV", "AKJV"] {
            let verses = FIXTURE
                .iter()
                .map(|(book_id, chapter, verse, text)| PackVerse {
                    book_id: *book_id,
                    chapter: *chapter,
                    verse: *verse,
                    text: (*text).into(),
                })
                .collect();
            db.import_translation(id, id, "en", verses, id == "KJV")
                .await
                .unwrap();
        }
    });
    dir.join("bible.db")
}

#[test]
fn fts_retrieval_finds_demo_quotations_and_dedupes_by_translation() {
    let dir = TempDir::new().unwrap();
    let path = fixture_db(dir.path());
    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    for (text, book, chapter, verse) in [
        (
            "Give and it shall be given unto you good measure pressed down",
            "Luke",
            6.0,
            38.0,
        ),
        (
            "Love not the world neither the things that are in the world",
            "1 John",
            2.0,
            15.0,
        ),
        (
            "Greater is he that is in you than he that is in the world",
            "1 John",
            4.0,
            4.0,
        ),
    ] {
        let candidates = retrieve_global_quote(&conn, text).unwrap();
        assert!(candidates.len() <= 240);
        let found = match_global_quote(text, &candidates, None);
        assert!(
            found.iter().any(|m| m.candidate.book == book
                && m.candidate.chapter == chapter
                && m.candidate.verse == verse),
            "{text}"
        );
        assert!(found.len() <= 3);
    }
    assert!(
        retrieve_global_quote(&conn, "the and of")
            .unwrap()
            .is_empty()
    );
    // Both translations are retrieved; they score as one verse.
    let both = retrieve_global_quote(&conn, "The LORD is my shepherd I shall not want").unwrap();
    assert!(
        both.iter()
            .filter(|c| c.verse == 1.0 && c.chapter == 23.0)
            .count()
            >= 2
    );
}

// ─── cache-quote-index ─────────────────────────────────────────────────────

const NOW: i64 = 1_793_404_800_000;
const LICENSED: &str =
    "Distinctive downloaded wording shines through this extraordinary passage today";

fn licensed_row(expires_at: i64) -> CachedQuoteVerse {
    CachedQuoteVerse {
        book: "John".into(),
        chapter: 3,
        verse: 16,
        translation: "TPT".into(),
        text: LICENSED.into(),
        cache_source: CacheQuoteSource {
            bible_id: "licensed".into(),
            book_id: 43,
            chapter: 3,
            verse: 16,
            fetched_at: NOW,
            expires_at,
        },
    }
}

#[test]
fn licensed_text_is_searchable_only_once_committed_and_expires_exactly() {
    let mut index = CacheQuoteIndex::new();
    index.begin(1).unwrap();
    index.append(1, &[licensed_row(NOW + 100)]).unwrap();
    assert!(index.search(LICENSED, NOW).unwrap().is_empty());
    index.commit(1);
    let found = index.search(LICENSED, NOW).unwrap();
    assert_eq!(found.len(), 1);
    let source = found[0].cache_source.as_ref().unwrap();
    assert_eq!(
        (source.bible_id.as_str(), found[0].translation.as_deref()),
        ("licensed", Some("TPT"))
    );
    assert!(index.search(LICENSED, NOW + 100).unwrap().is_empty());
    // A stale version's rows and commit are ignored.
    index.begin(2).unwrap();
    index.append(1, &[licensed_row(NOW + 100)]).unwrap();
    index.commit(1);
    assert!(index.search(LICENSED, NOW).unwrap().is_empty());
    index.close();
    assert!(index.search(LICENSED, NOW).unwrap().is_empty());
}

// ─── Worker thread and client ──────────────────────────────────────────────

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .unwrap()
}

#[test]
fn the_worker_searches_public_and_licensed_text_on_its_own_thread() {
    let dir = TempDir::new().unwrap();
    let path = fixture_db(dir.path());
    rt().block_on(async {
        let client = QuoteWorkerClient::new(path);
        let m = client
            .search(
                "Give and it shall be given unto you good measure pressed down",
                false,
            )
            .await
            .unwrap()
            .found()
            .unwrap();
        assert_eq!(m[0].candidate.book, "Luke");
        assert!(
            client
                .search(LICENSED, false)
                .await
                .unwrap()
                .found()
                .unwrap()
                .is_empty()
        );
        client.begin_cache(1).unwrap();
        client
            .append_cache(1, vec![licensed_row(i64::MAX)])
            .await
            .unwrap();
        client.commit_cache(1);
        let m = client
            .search(LICENSED, false)
            .await
            .unwrap()
            .found()
            .unwrap();
        assert_eq!(
            m[0].candidate.cache_source.as_ref().unwrap().bible_id,
            "licensed"
        );
        // Possible matches use rarity from the installed Bibles.
        let p = client
            .search(
                "If only I touch the hem of his garments I shall be made whole",
                true,
            )
            .await
            .unwrap()
            .found()
            .unwrap();
        assert!(p.iter().all(|m| m.ambiguous));
        client.close();
        assert_eq!(
            client.search("x y z", false).await.unwrap_err(),
            "Quotation worker is closed"
        );
        assert_eq!(
            client.append_cache(1, vec![]).await.unwrap_err(),
            "Quotation worker unavailable"
        );
    });
}

/// A worker that answers searches only when released, to observe queueing.
fn gated(
    path: PathBuf,
    gate: Arc<Mutex<Vec<std::sync::mpsc::Sender<()>>>>,
    starts: Arc<AtomicUsize>,
) -> Spawner {
    Arc::new(
        move |_: &Path,
              commands: std::sync::mpsc::Receiver<Command>,
              events: tokio::sync::mpsc::UnboundedSender<Event>| {
            starts.fetch_add(1, Ordering::SeqCst);
            let (path, gate) = (path.clone(), gate.clone());
            std::thread::spawn(move || {
                let (forward, inner) = std::sync::mpsc::channel();
                let worker = std::thread::spawn(move || run_worker(&path, inner, events));
                for command in commands {
                    if matches!(command, Command::Search { .. }) {
                        let (tx, rx) = std::sync::mpsc::channel();
                        gate.lock().unwrap().push(tx);
                        if rx.recv().is_err() {
                            return;
                        }
                    }
                    if forward.send(command).is_err() {
                        return;
                    }
                }
                drop(forward);
                let _ = worker.join();
            });
        },
    )
}

#[test]
fn only_the_newest_queued_search_runs() {
    let dir = TempDir::new().unwrap();
    let path = fixture_db(dir.path());
    let gate = Arc::new(Mutex::new(Vec::new()));
    rt().block_on(async {
        let client = QuoteWorkerClient::with_spawner(
            path.clone(),
            gated(path, gate.clone(), Default::default()),
        );
        let text = "Love not the world neither the things that are in the world";
        let (a, b, c) = (client.clone(), client.clone(), client.clone());
        let first = tokio::spawn(async move { a.search(text, false).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let second = tokio::spawn(async move { b.search(text, false).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let third = tokio::spawn(async move { c.search(text, false).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        // The replaced search resolves superseded without reaching the worker.
        assert!(matches!(
            second.await.unwrap().unwrap(),
            QuoteSearchOutcome::Superseded
        ));
        for _ in 0..2 {
            while gate.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            gate.lock().unwrap().remove(0).send(()).unwrap();
        }
        assert_eq!(
            first.await.unwrap().unwrap().found().unwrap()[0]
                .candidate
                .book,
            "1 John"
        );
        assert_eq!(
            third.await.unwrap().unwrap().found().unwrap()[0]
                .candidate
                .book,
            "1 John"
        );
        client.close();
    });
}

#[test]
fn a_missing_database_fails_and_restarts_lazily() {
    let dir = TempDir::new().unwrap();
    rt().block_on(async {
        let client = QuoteWorkerClient::new(dir.path().join("bible.db"));
        let failures = Arc::new(AtomicUsize::new(0));
        let count = failures.clone();
        client.on_failure(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        assert!(client.search("one two three", false).await.is_err());
        assert!(client.search("one two three", false).await.is_err());
        assert_eq!(failures.load(Ordering::SeqCst), 2);
        // Once the database exists, the next search starts a working thread.
        fixture_db(dir.path());
        let m = client
            .search(
                "Give and it shall be given unto you good measure pressed down",
                false,
            )
            .await
            .unwrap()
            .found()
            .unwrap();
        assert_eq!(m[0].candidate.book, "Luke");
        client.close();
        assert_eq!(
            failures.load(Ordering::SeqCst),
            2,
            "closing is not a failure"
        );
    });
}

#[test]
fn a_hung_search_times_out_after_five_seconds() {
    let dir = TempDir::new().unwrap();
    let path = fixture_db(dir.path());
    let gate = Arc::new(Mutex::new(Vec::new()));
    rt().block_on(async {
        let client = QuoteWorkerClient::with_spawner(
            path.clone(),
            gated(path, gate.clone(), Default::default()),
        );
        let start = std::time::Instant::now();
        let err = client
            .search("Love not the world neither the things", false)
            .await
            .unwrap_err();
        assert_eq!(err, "Quotation search timed out");
        assert!(start.elapsed() >= Duration::from_secs(5));
        client.close();
    });
}

// ─── Corpus coordinator with the encrypted cache ──────────────────────────

struct Keyring(Mutex<HashMap<String, [u8; 32]>>);
impl KeyProtector for Keyring {
    fn load(&self, slot: &str) -> Result<Option<[u8; 32]>, SecretError> {
        Ok(self.0.lock().unwrap().get(slot).copied())
    }
    fn save(&self, slot: &str, key: &[u8; 32]) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(slot.into(), *key);
        Ok(())
    }
}

async fn licensed_cache(dir: &Path, fetched_at: i64) -> ApiBibleCache {
    let (crypto, _) =
        ApiCacheCrypto::open(dir, &Keyring(Mutex::new(HashMap::new())), "Kairo").unwrap();
    let cache = ApiBibleCache::open(&dir.join("api-bible-cache.db"), crypto)
        .await
        .unwrap();
    cache
        .upsert_translation(ApiBibleTranslationMeta {
            bible_id: "licensed".into(),
            translation: "TPT".into(),
            name: "Passion".into(),
            copyright: "copyright".into(),
            total_chapters: None,
        })
        .await
        .unwrap();
    let verse = kairo_domain::scripture::ScriptureVerse {
        book: "John".into(),
        chapter: 3.0,
        verse: 16.0,
        text: LICENSED.into(),
        extra: Default::default(),
    };
    cache
        .put_passage(
            "licensed",
            ApiBibleCachePassage {
                book_id: 43,
                book_name: "John".into(),
                verses: vec![verse],
                fetched_at,
                coverage: None,
            },
        )
        .await
        .unwrap();
    cache
}

async fn eventually<F: std::future::Future<Output = bool>>(mut check: impl FnMut() -> F) {
    for _ in 0..200 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("condition not reached");
}

/// The real worker, with a handle on each spawned thread's command channel
/// so a test can kill it.
fn killable(starts: Arc<Mutex<Vec<tokio::sync::mpsc::UnboundedSender<Event>>>>) -> Spawner {
    Arc::new(
        move |path: &Path, commands, events: tokio::sync::mpsc::UnboundedSender<Event>| {
            starts.lock().unwrap().push(events.clone());
            let path = path.to_owned();
            std::thread::spawn(move || run_worker(&path, commands, events));
        },
    )
}

#[test]
fn licensed_text_survives_a_crash_and_is_retracted_on_revocation() {
    let dir = TempDir::new().unwrap();
    let path = fixture_db(dir.path());
    rt().block_on(async {
        let cache = licensed_cache(dir.path(), kairo_store::settings::now_ms() as i64).await;
        let workers = Arc::new(Mutex::new(Vec::new()));
        let corpus = QuoteCorpus::with_spawner(killable(workers.clone()));
        corpus.open(path);
        corpus.attach_api_cache(Some(cache.clone()));
        eventually(|| async {
            corpus
                .search_general_quote(LICENSED, false)
                .await
                .unwrap()
                .found()
                .unwrap()
                .len()
                == 1
        })
        .await;
        let found = corpus
            .search_general_quote(LICENSED, false)
            .await
            .unwrap()
            .found()
            .unwrap()
            .remove(0);
        assert_eq!(found.candidate.quote_corpus_version, Some(corpus.version()));
        assert!(corpus.is_general_quote_eligible(&found.candidate).await);

        let invalidations = Arc::new(AtomicUsize::new(0));
        let count = invalidations.clone();
        corpus.on_general_quote_corpus_change(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        // Simulate a native crash of the current worker thread.
        let current = workers.lock().unwrap().last().unwrap().clone();
        current
            .send(Event::Failed("simulated native worker crash".into()))
            .unwrap();
        eventually(|| async { workers.lock().unwrap().len() >= 2 }).await;
        eventually(|| async {
            corpus
                .search_general_quote(LICENSED, false)
                .await
                .map(|m| m.found().is_some_and(|m| m.len() == 1))
                .unwrap_or(false)
        })
        .await;
        // A crash loses only the worker's copy: shown cards stay valid.
        assert_eq!(invalidations.load(Ordering::SeqCst), 0);
        assert!(corpus.is_general_quote_eligible(&found.candidate).await);

        // Adding text does not retract anything.
        cache
            .put_passage(
                "licensed",
                ApiBibleCachePassage {
                    book_id: 45,
                    book_name: "Romans".into(),
                    verses: vec![kairo_domain::scripture::ScriptureVerse {
                        book: "Romans".into(),
                        chapter: 8.0,
                        verse: 28.0,
                        text: "All things work together for good".into(),
                        extra: Default::default(),
                    }],
                    fetched_at: kairo_store::settings::now_ms() as i64,
                    coverage: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(invalidations.load(Ordering::SeqCst), 0);
        assert!(corpus.is_general_quote_eligible(&found.candidate).await);

        cache
            .mark_unavailable("licensed", "Access revoked")
            .await
            .unwrap();
        assert!(invalidations.load(Ordering::SeqCst) >= 1);
        assert!(!corpus.is_general_quote_eligible(&found.candidate).await);
        eventually(|| async {
            corpus
                .search_general_quote(LICENSED, false)
                .await
                .unwrap()
                .found()
                .unwrap()
                .is_empty()
        })
        .await;
        // Public text is unaffected.
        let public = corpus
            .search_general_quote(
                "Love not the world neither the things that are in the world",
                false,
            )
            .await
            .unwrap()
            .found()
            .unwrap();
        assert_eq!(public[0].candidate.book, "1 John");
        corpus.close();
        assert!(!corpus.is_general_quote_eligible(&found.candidate).await);
    });
}

#[test]
fn expired_text_is_never_exported_and_a_failed_index_keeps_public_matching() {
    let dir = TempDir::new().unwrap();
    let path = fixture_db(dir.path());
    rt().block_on(async {
        let cache = licensed_cache(
            dir.path(),
            kairo_store::settings::now_ms() as i64 - API_CACHE_MAX_AGE_MS,
        )
        .await;
        assert_eq!(
            cache
                .translation_state("licensed", kairo_store::settings::now_ms() as i64)
                .await
                .unwrap()
                .unwrap()
                .status,
            ApiBibleCacheStatus::Stale
        );
        let corpus = QuoteCorpus::new();
        corpus.open(path);
        corpus.attach_api_cache(Some(cache));
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            corpus
                .search_general_quote(LICENSED, false)
                .await
                .unwrap()
                .found()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            corpus
                .search_general_quote(
                    "Give and it shall be given unto you good measure pressed down",
                    false
                )
                .await
                .unwrap()
                .found()
                .unwrap()[0]
                .candidate
                .book,
            "Luke"
        );
        corpus.close();
        assert!(
            corpus
                .search_general_quote("Give and it shall be given", false)
                .await
                .unwrap()
                .found()
                .unwrap()
                .is_empty()
        );
    });
}

// ─── Differential against Electron ─────────────────────────────────────────

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as usize
    }
    fn below(&mut self, n: usize) -> usize {
        self.next() % n.max(1)
    }
}

const VERSES: &[(&str, f64, f64, &str)] = &[
    ("Luke", 6.0, 38.0, LUKE),
    ("1 John", 2.0, 15.0, JOHN),
    (
        "1 John",
        4.0,
        4.0,
        "Ye are of God, little children, and have overcome them: because greater is he that is in you, than he that is in the world.",
    ),
    (
        "Matthew",
        9.0,
        21.0,
        "For she said within herself, If I may but touch his garment, I shall be whole.",
    ),
    (
        "Mark",
        5.0,
        28.0,
        "For she said, If I may touch but his clothes, I shall be whole.",
    ),
    (
        "Matthew",
        14.0,
        36.0,
        "And besought him that they might only touch the hem of his garment: and as many as touched were made perfectly whole.",
    ),
    (
        "John",
        14.0,
        6.0,
        "Jesus saith unto him, I am the way, the truth, and the life: no man cometh unto the Father, but by me.",
    ),
    (
        "Psalms",
        23.0,
        1.0,
        "The LORD is my shepherd; I shall not want.",
    ),
    (
        "John",
        3.0,
        16.0,
        "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
    ),
    (
        "Proverbs",
        3.0,
        5.0,
        "Trust in the LORD with all thine heart; and lean not unto thine own understanding.",
    ),
    (
        "Philippians",
        4.0,
        13.0,
        "I can do all things through Christ which strengtheneth me.",
    ),
    (
        "Romans",
        8.0,
        28.0,
        "And we know that all things work together for good to them that love God, to them who are the called according to his purpose.",
    ),
    (
        "Isaiah",
        40.0,
        31.0,
        "But they that wait upon the LORD shall renew their strength; they shall mount up with wings as eagles; they shall run, and not be weary; and they shall walk, and not faint.",
    ),
    (
        "Jeremiah",
        51.0,
        50.0,
        "Ye that have escaped the sword, go away, stand not still: remember the LORD afar off, and let Jerusalem come into your mind.",
    ),
    (
        "Luke",
        1.0,
        1.0,
        "And it came to pass that he said unto them, behold, hear my words.",
    ),
    (
        "Genesis",
        1.0,
        1.0,
        "In the beginning God created the heaven and the earth.",
    ),
    (
        "Hebrews",
        11.0,
        1.0,
        "Now faith is the substance of things hoped for, the evidence of things not seen.",
    ),
    (
        "Matthew",
        7.0,
        2.0,
        "For with what judgment ye judge, ye shall be judged: and with what measure ye mete, it shall be measured to you again.",
    ),
    (
        "Psalms",
        46.0,
        10.0,
        "Be still, and know that I am God: I will be exalted among the heathen, I will be exalted in the earth.",
    ),
    (
        "Joshua",
        1.0,
        9.0,
        "Have not I commanded thee? Be strong and of a good courage; be not afraid, neither be thou dismayed: for the LORD thy God is with thee whithersoever thou goest.",
    ),
];

const COMMENTARY: &[&str] = &[
    "Listen church,",
    "and the Bible says",
    "do not be afraid church",
    "turn to your neighbour and say",
    "amen,",
    "I want you to hear this",
    "it says right here",
    "now watch this",
    "",
];
const NOISE: &[&str] = &[
    "pressure", "glory", "brother", "phone", "not", "amen", "uh", "river",
];
const GENERIC: &[&str] = &[
    "God is good and we love him",
    "give generously to your neighbours this week",
    "the world is full of things",
    "Please turn off your phones and let us stand for the word of God",
    "we thank you for coming this morning",
];

fn corpus_cases() -> Vec<Value> {
    let mut r = Lcg(0x0037_0037);
    let mut cases = Vec::new();
    let words = |text: &str| {
        text.split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    for i in 0..400 {
        // Candidates: a random subset, with parallel translation copies.
        let mut candidates = Vec::new();
        for _ in 0..(3 + r.below(8)) {
            let (book, chapter, verse, text) = VERSES[r.below(VERSES.len())];
            let translation = ["KJV", "AKJV"][r.below(2)];
            candidates.push(json!({ "book": book, "chapter": chapter, "verse": verse, "text": text, "translation": translation }));
        }
        let source = &candidates[r.below(candidates.len())];
        let w = words(source["text"].as_str().unwrap());
        let len = 3 + r.below(w.len().saturating_sub(2));
        let start = r.below(w.len().saturating_sub(len) + 1);
        let mut spoken: Vec<String> = w[start..(start + len).min(w.len())].to_vec();
        match r.below(6) {
            0 if !spoken.is_empty() => {
                let at = r.below(spoken.len());
                spoken[at] = NOISE[r.below(NOISE.len())].into();
            }
            1 => spoken.insert(0, "not".into()),
            2 => spoken.reverse(),
            3 => spoken = words(GENERIC[r.below(GENERIC.len())]),
            _ => {}
        }
        let text = format!(
            "{} {}",
            COMMENTARY[r.below(COMMENTARY.len())],
            spoken.join(" ")
        );
        let counts = (i % 3 == 0).then(|| {
            let mut map = serde_json::Map::new();
            for word in words(&source["text"].as_str().unwrap().to_lowercase()) {
                let word: String = word.chars().filter(char::is_ascii_alphanumeric).collect();
                map.insert(word, json!([6, 52, 83, 150, 151, 244, 1000][r.below(7)]));
            }
            Value::Object(map)
        });
        cases.push(json!([text, candidates, counts]));
    }
    cases
}

fn run_rust(case: &Value) -> Value {
    let text = case[0].as_str().unwrap();
    let candidates: Vec<GlobalQuoteCandidate> = serde_json::from_value(case[1].clone()).unwrap();
    let matches = match case[2].as_object() {
        Some(counts) => {
            let mut rarity = |w: &str| counts.get(w).and_then(Value::as_f64).unwrap_or(1000.0);
            match_global_quote(text, &candidates, Some(&mut rarity))
        }
        None => match_global_quote(text, &candidates, None),
    };
    serde_json::to_value(matches).unwrap()
}

#[test]
fn corpus_differential_matches_electron_scores() {
    let cases = corpus_cases();
    let expected = kairo_oracle::oracle_value("oracle:global-quote.matchMany", json!([cases]));
    let expected = expected.as_array().expect("oracle results");
    assert_eq!(expected.len(), cases.len());
    let mut failures = Vec::new();
    let (mut confident, mut possible) = (0, 0);
    for (case, expected) in cases.iter().zip(expected) {
        let actual = run_rust(case);
        for m in actual.as_array().unwrap() {
            if m["ambiguous"] == json!(true) && case[2].is_object() {
                possible += 1
            } else {
                confident += 1
            }
        }
        let differences = kairo_oracle::compare_with_tolerance(expected, &actual, 1e-9);
        if !differences.is_empty() {
            failures.push(format!("{}\n  {}", case[0], differences.join("\n  ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(6)].join("\n")
    );
    // The corpus exercises both paths.
    assert!(
        confident > 50 && possible > 0,
        "confident {confident}, possible {possible}"
    );
}

fn bundled_bible() -> PathBuf {
    kairo_oracle::electron_dir().join("resources/bible.db")
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1)]
}

#[test]
#[ignore = "needs the Electron checkout with node_modules and resources/bible.db"]
fn bundled_bible_differential_and_p95() {
    let bundled = bundled_bible();
    let mut texts: Vec<String> = corpus_cases()
        .iter()
        .map(|c| c[0].as_str().unwrap().to_owned())
        .collect();
    texts.truncate(200);
    for possible in [false, true] {
        let mut oracle = kairo_oracle::Oracle::new(
            kairo_oracle::Mode::Live,
            TempDir::new().unwrap().path().into(),
            kairo_oracle::electron_dir(),
        );
        let electron = oracle
            .results(
                "oracle:global-quote.runDb",
                &[json!([bundled, texts, possible, 5])],
            )
            .unwrap()
            .remove(0);

        assert!(
            electron.get("results").is_some(),
            "{}",
            &electron.to_string()[..electron.to_string().len().min(600)]
        );
        let copy = TempDir::new().unwrap();
        std::fs::copy(&bundled, copy.path().join("bible.db")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (etx, mut erx) = tokio::sync::mpsc::unbounded_channel();
        let path = copy.path().join("bible.db");
        let worker = std::thread::spawn(move || run_worker(&path, rx, etx));
        let mut recv = || loop {
            match erx.blocking_recv().unwrap() {
                Event::Ready => continue,
                Event::Search { result, .. } => return result.unwrap(),
                Event::Failed(e) => panic!("{e}"),
                Event::CacheAck { .. } => continue,
            }
        };
        let mut failures = Vec::new();
        for (i, (text, expected)) in texts
            .iter()
            .zip(electron["results"].as_array().unwrap())
            .enumerate()
        {
            tx.send(Command::Search {
                id: i as u64,
                text: text.clone(),
                possible,
            })
            .unwrap();
            let actual = serde_json::to_value(recv()).unwrap();
            let differences = kairo_oracle::compare_with_tolerance(expected, &actual, 1e-9);
            if !differences.is_empty() {
                failures.push(format!("{text}\n  {}", differences.join("\n  ")));
            }
        }
        let mut timings = Vec::new();
        for _ in 0..5 {
            for (i, text) in texts.iter().enumerate() {
                let start = std::time::Instant::now();
                tx.send(Command::Search {
                    id: i as u64,
                    text: text.clone(),
                    possible,
                })
                .unwrap();
                recv();
                timings.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        drop(tx);
        worker.join().unwrap();
        timings.sort_by(f64::total_cmp);
        let mut theirs: Vec<f64> = electron["timings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        theirs.sort_by(f64::total_cmp);
        eprintln!(
            "possible={possible}: {} searches. Rust p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms. Electron p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms",
            timings.len(),
            percentile(&timings, 0.5),
            percentile(&timings, 0.95),
            timings.last().unwrap(),
            percentile(&theirs, 0.5),
            percentile(&theirs, 0.95),
            theirs.last().unwrap()
        );
        assert!(
            failures.is_empty(),
            "{} of {} differ:\n{}",
            failures.len(),
            texts.len(),
            failures[..failures.len().min(6)].join("\n")
        );
    }
}
