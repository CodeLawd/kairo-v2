//! Prompt 107: automated performance budgets that are stable without the
//! Electron checkout or a network.
//!
//! - Live detector, per transcript event on the fast path (no local evidence
//!   lookups): p99 under 2 ms (06-testing-performance-strategy.md row D04).
//! - Scripture search through `ScriptureService::search` on a full-shape
//!   offline Bible (31,102 generated verses, not Bible wording): p95 under
//!   16 ms, one 60 Hz frame (DECISIONS 107).
//!
//! The budgets are asserted as written in optimised builds and when
//! `KAIRO_PERF_STRICT=1`; an unoptimised build gets four times the budget,
//! which only catches gross regressions.
mod common;

use std::time::{Duration, Instant};

use kairo_scripture::bible_db::BibleDb;
use kairo_scripture::live_detector::{LiveCitationDetector, TranscriptEvent, TranscriptKind};
use kairo_store::db::{BIBLE_VERSE_COUNTS, PackVerse};

fn strict() -> bool {
    !cfg!(debug_assertions) || std::env::var("KAIRO_PERF_STRICT").as_deref() == Ok("1")
}

fn budget(ms: f64) -> Duration {
    Duration::from_secs_f64(if strict() { ms } else { ms * 4.0 } / 1e3)
}

fn at(sorted: &[Duration], q: f64) -> Duration {
    sorted[((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1)]
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

const WORDS: &[&str] = &[
    "light", "river", "stone", "morning", "mercy", "field", "harvest", "faithful", "garden",
    "voice", "mountain", "people", "kingdom", "bread", "water", "shepherd", "city", "heart",
    "peace", "strength", "word", "path", "house", "fire", "promise", "song", "wisdom", "gate",
    "covenant", "spirit", "ground", "seed", "cloud", "evening", "servant", "hand", "glory", "way",
];

/// Generated text in the canonical shape, so FTS sees realistic word
/// frequencies; none of it is Bible wording.
fn generated_bible() -> Vec<PackVerse> {
    let mut rng = Lcg(107);
    let mut rows = Vec::new();
    for (book, counts) in BIBLE_VERSE_COUNTS.iter().enumerate() {
        for (chapter, &last) in counts.iter().enumerate() {
            for verse in 1..=last {
                let len = 12 + rng.next() as usize % 18;
                let text = (0..len)
                    .map(|_| WORDS[rng.next() as usize % WORDS.len()])
                    .collect::<Vec<_>>()
                    .join(" ");
                rows.push(PackVerse {
                    book_id: book as u32 + 1,
                    chapter: chapter as u32 + 1,
                    verse,
                    text,
                });
            }
        }
    }
    rows
}

#[test]
fn scripture_search_p95_is_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::block_on(async {
        let db = BibleDb::open(&dir.path().join("bible.db")).await.unwrap();
        db.import_translation("KJV", "King James Version", "en", generated_bible(), true)
            .await
            .unwrap();
        db
    });
    let service = common::service_with_db(db);

    let mut rng = Lcg(2026);
    let mut queries: Vec<String> = [
        "John 3:16",
        "Psalm 23:1-3",
        "Romans 8",
        "Gen 1:1",
        "1 Cor 13:4-7",
        "Revelation 22:21",
        "Jude 1:25",
        "Obadiah 1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for _ in 0..200 {
        let len = 1 + rng.next() as usize % 4;
        queries.push(
            (0..len)
                .map(|_| WORDS[rng.next() as usize % WORDS.len()])
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    // Empty, whitespace-only, punctuation-only and unknown-word queries must
    // stay cheap and return nothing rather than fail.
    for edge in ["", "   ", "?!*\"", "zzqxv", "John 99:99"] {
        let results = common::block_on(service.search(edge, Some("KJV"))).unwrap();
        assert!(
            results.is_empty(),
            "{edge:?} returned {} results",
            results.len()
        );
    }

    // Warm the statement cache and the phrase index.
    for q in queries.iter().take(8) {
        let _ = common::block_on(service.search(q, Some("KJV"))).unwrap();
    }
    // Shared CI runners stall for hundreds of ms at random, which can push one
    // round's p95 over budget. A real regression slows every round, so the
    // best of three rounds is asserted.
    let mut best: Option<Vec<Duration>> = None;
    for _ in 0..3 {
        let mut samples = Vec::new();
        let mut hits = 0usize;
        for q in &queries {
            let start = Instant::now();
            let results = common::block_on(service.search(q, Some("KJV"))).unwrap();
            samples.push(start.elapsed());
            hits += usize::from(!results.is_empty());
        }
        assert!(
            hits > queries.len() / 2,
            "only {hits} of {} queries found verses",
            queries.len()
        );
        samples.sort_unstable();
        if best
            .as_ref()
            .is_none_or(|b| at(&samples, 0.95) < at(b, 0.95))
        {
            best = Some(samples);
        }
    }
    let samples = best.unwrap();
    let p95 = at(&samples, 0.95);
    eprintln!(
        "scripture search n={} p50 {:?} p95 {p95:?} max {:?} (strict: {})",
        samples.len(),
        at(&samples, 0.5),
        samples[samples.len() - 1],
        strict()
    );
    assert!(
        p95 < budget(16.0),
        "scripture search p95 {p95:?} over budget"
    );
}

#[test]
fn detector_fast_path_p99_is_within_budget() {
    // A sermon-like stream: interim growth then a final, every 100 ms, with
    // spoken and written citations, partial references and plain speech.
    let sentences = [
        "turn with me to John chapter three verse sixteen",
        "and the Bible says in Romans 8:28 that all things work together",
        "let us read Psalm twenty three",
        "we are going to look at first Corinthians thirteen four through seven",
        "this morning I want to talk about faith and patience",
        "Genesis one one in the beginning",
        "open your Bibles to the book of",
        "Hebrews eleven",
        "chapter eleven verse one now faith is the substance",
        "so as we close let us pray together",
        // Resolved against the reading memory.
        "let's continue from verse twenty",
        "go back to verse four",
        "back in Romans verse twenty nine",
        "let's continue our reading",
    ];
    let mut owned = Vec::new();
    let mut at_ms = 0u64;
    for round in 0..20 {
        for s in &sentences {
            let words: Vec<&str> = s.split(' ').collect();
            for n in 1..words.len() {
                at_ms += 100;
                owned.push((at_ms, TranscriptKind::Interim, words[..n].join(" ")));
            }
            at_ms += 100;
            owned.push((at_ms, TranscriptKind::Final, format!("{s} {round}")));
        }
    }
    let mut detector = LiveCitationDetector::default();
    let mut samples = Vec::with_capacity(owned.len());
    let mut emitted = 0usize;
    for (at_ms, kind, text) in &owned {
        let start = Instant::now();
        let found = detector.process(TranscriptEvent {
            at_ms: *at_ms,
            kind: *kind,
            text,
            audio_epoch: Some(1),
        });
        samples.push(start.elapsed());
        emitted += found.len();
    }
    emitted += detector.advance_to(at_ms + 5_001).len();
    assert!(emitted > 0, "the replay found no citations");
    samples.sort_unstable();
    let p99 = at(&samples, 0.99);
    eprintln!(
        "detector per event n={} p50 {:?} p99 {p99:?} max {:?}, {emitted} citations (strict: {})",
        samples.len(),
        at(&samples, 0.5),
        samples[samples.len() - 1],
        strict()
    );
    assert!(p99 < budget(2.0), "detector p99 {p99:?} over budget");
}
