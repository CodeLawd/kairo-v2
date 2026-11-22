//! Deterministic citation handling on an injected millisecond clock.

use std::sync::LazyLock;

use kairo_domain::{
    js::{is_whitespace, string_to_number},
    scripture::{
        plan::{DEFAULT_MIN_COVERAGE, SermonPlanIndex, match_plan_quote},
        spoken_translations::find_spoken_translations,
    },
};
use regex::Regex;

use crate::{
    citation::{
        ExplicitCitation, bare_book, canonical_book, corrupted_normalized, match_normalized,
        named_citation, normalize,
    },
    model_detector::{DetectionType, ModelDetection, ModelWork},
    quote_recovery::{
        ChapterVerseTokens, match_chapter_quote, recover_damaged_quote, tokenize_chapter,
    },
    scripture_signal::sounds_like_scripture,
    service::QuoteCandidate,
    verse_bounds::is_valid_scripture_reference,
};

macro_rules! rule {
    ($name:ident, $pattern:literal) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| {
            let pattern = $pattern.replace(
                r"\s",
                r"(?u:[\x09-\x0D\x20\u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}])",
            );
            Regex::new(&format!("(?-u:{pattern})")).expect("valid live citation rule")
        });
    };
}

rule!(VERSE_WORD, r"\bverses?\b");
rule!(
    VERSE_SIGNAL,
    r"\bverses?\b|:\s*\d+|\b\d+(?:\s|,)+\d+(?:\s*(?:-|to|thru|through|and)\s*\d+)?\b"
);
rule!(
    VERSE_FIRST,
    r"\bverses?\s+(\d+(?:\s*(?:-|to|through|thru|and)\s*\d+)?)\s+(?:of|in|from)\s+(?:the\s+)?chapter\s+(\d+)\b"
);
rule!(CHAPTER, r"\bchapter\s+\d+\b");
rule!(VERSE, r"\bverses?\s+\d+\b");
rule!(RANGE_LEAD, r"^(?:to|through|thru|and|-)\s*");
rule!(CONTINUATION, r"^(?:to|through|thru|and|-)\s*\d+\b");
rule!(NUMBER_FIRST, r"^\d+\b");
rule!(
    BARE_FRAGMENT,
    r"^\d+(?:\s*(?:-|to|thru|through|and|,)\s*\d+|\s+\d+)*[.,;!?]?(?:\s+(?:it reads|it says|says|and i quote)\b(?u:.*))?$"
);
rule!(
    RANGE,
    r"^(?:(?u:.*?)\bverses?\s+)?(\d+)(?:(?:\s*(?:-|to|thru|through|and)\s*|\s+)(\d+))?\b"
);
rule!(
    LIST,
    r"^(?:(?u:.*?)\bverses?\s+)?(\d+(?:(?:,\s*(?:and\s+)?|\s+and\s+)\d+)+)"
);
rule!(DIGITS, r"\d+");
rule!(RANGE_PENDING, r"\b(?:and|to|through|thru)\s*[,.;]?\s*$");
rule!(DISAMBIGUATED, r"\bverses?\b|:|-|\b(?:to|thru|through)\b");
rule!(CHAPTER_WORD, r"\bchapter\b");
rule!(CITATION_END, r"^\s*(?:[.,;:!?]|$)");
rule!(
    CITATION_LEAD,
    r"\b(?:in|to|into|from|read|open|turn|at|of|see)\s+(?:the\s+(?:book|gospel|epistle|letter)\s+of\s+)?$"
);
rule!(
    RESUME,
    r"\b(?:continue|continuing|carry\s+on|keep\s+going|go\s+on|go\s+back|going\s+back|come\s+back|coming\s+back|back\s+(?:to|in)|return(?:ing)?\s+to|pick\s+(?:it\s+|that\s+)?up|resume|where\s+we\s+(?:stopped|left\s+off)|our\s+(?:reading|text))\b"
);
rule!(
    READING_HINT,
    r"\b(?:where\s+we\s+(?:left\s+off|stopped)|our\s+(?:reading|text|passage)|the\s+(?:gospel|epistle|letter|passage|text|reading|chapter))\b"
);
rule!(
    INTRODUCED,
    r"\b(?:turn|turning|let's\s+read|let\s+us\s+read|we\s+are\s+reading|we're\s+reading|read(?:ing)?\s+(?:from|in)|our\s+(?:text|reading|scripture)|open\s+(?:your\s+bibles?\s+)?to)\b"
);
rule!(
    BOOK_LEAD,
    r"\b(?:back\s+(?:in|to)|return(?:ing)?\s+to|in|to)\s+(?:the\s+(?:book|gospel|epistle|letter)\s+of\s+)?((?:[1-3]\s)?[a-z]+(?:\s+of\s+solomon)?)\b"
);
// Whole phrases, so narration ("let's continue in prayer") stays narration.
const LEAD: &str = r"^(?:(?:okay|ok|alright|all\s+right|so|and|now|then|right|please|well)\s+)*(?:(?:let's|let\s+us|we'll|we\s+will|we're\s+going\s+to|we\s+are\s+going\s+to)\s+)?";
static RESUME_ONLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"{LEAD}(?:(?:(?:continue|carry\s+on|keep\s+going|go\s+on|pick\s+up|resume|get\s+back\s+to|go\s+back\s+to|come\s+back\s+to|return\s+to|back\s+to)(?:\s+(?:with|in|on))?\s+(?:our|the|that)\s+(?:reading|text|passage|scripture))|(?:(?:continue|carry\s+on|pick\s+(?:it\s+|things\s+)?up|go\s+back|start)\s+(?:from\s+)?where\s+we\s+(?:left\s+off|stopped))|(?:continue|resume)\s+reading)(?:\s+please)?$"
    ))
    .expect("valid resume rule")
});
static RESUME_BOOK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"{LEAD}(?:(?:go|turn|get|come|going|coming)\s+)?(?:back\s+(?:to|in(?:to)?)|return(?:ing)?\s+to)\s+(?:the\s+(?:book|gospel|epistle|letter)\s+of\s+)?(.+?)(?:\s+please)?$"
    ))
    .expect("valid resume rule")
});

pub use crate::reading_memory::{Reading, ReadingKind, ReadingMemory};

/// How long a passage read aloud keeps its words from counting as a quote.
const READ_ALOUD_MS: u64 = 15 * 60_000;
pub use kairo_domain::scripture_trace::{ScriptureResolver, TranscriptSource as TranscriptKind};

pub struct TranscriptEvent<'a> {
    pub at_ms: u64,
    pub kind: TranscriptKind,
    pub text: &'a str,
    pub audio_epoch: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CitationEmission {
    pub reference: ExplicitCitation,
    pub translations: Vec<String>,
    pub resolver: ScriptureResolver,
    pub confidence: f64,
    pub detection_type: DetectionType,
    pub superseded: bool,
    /// Another reading fitted a resume as well: wait for the operator.
    pub ambiguous: bool,
    pub source: TranscriptKind,
    pub received_at_ms: u64,
    pub emitted_at_ms: u64,
}

struct Context {
    book: &'static str,
    chapter: f64,
    updated: u64,
    awaiting_verse: bool,
    verse_start: Option<f64>,
    awaiting_range_end: bool,
    quote_text: String,
    quote_verses: Option<Vec<ChapterVerseTokens>>,
}

struct Cached {
    reference: ExplicitCitation,
    translations: Vec<String>,
    expires: u64,
    released: bool,
}

type ChapterSource<'a> = dyn FnMut(&str, f64) -> Vec<QuoteCandidate> + 'a;
type QuoteSource<'a> = dyn FnMut(&str) -> Vec<QuoteCandidate> + 'a;

/// Local data made available by the scripture service for one transcript event.
#[derive(Default)]
pub struct LocalEvidence<'a> {
    pub chapter: Option<&'a mut ChapterSource<'a>>,
    pub quote: Option<&'a mut QuoteSource<'a>>,
    pub plan: Option<&'a SermonPlanIndex>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedTranscript {
    pub text: String,
    pub received_at_ms: u64,
    /// A resume no rule placed; `text` lists the service's readings.
    pub resume: bool,
}

pub struct LiveCitationDetector {
    context: Option<Context>,
    pending_book: Option<(&'static str, u64)>,
    passage: Option<(&'static str, Option<f64>)>,
    fallback: Option<CitationEmission>,
    cache: Vec<Cached>,
    cache_window_ms: u64,
    last: Option<CitationEmission>,
    recent_finals: String,
    recent_at: u64,
    last_interim: String,
    spoken_translations: Vec<String>,
    audio_epoch: Option<u64>,
    recovery_text: String,
    recovery_at: u64,
    plan_text: String,
    plan_at: u64,
    reading: Option<(ExplicitCitation, u64)>,
    unresolved: Option<UnresolvedTranscript>,
    plan_progress: Option<(String, String)>,
    generation: u64,
    memory: ReadingMemory,
    /// The utterance being processed introduced its passage as a reading.
    introduced: bool,
}

impl Default for LiveCitationDetector {
    fn default() -> Self {
        Self::new(5 * 60_000)
    }
}

impl LiveCitationDetector {
    pub fn new(cache_window_ms: u64) -> Self {
        Self {
            context: None,
            pending_book: None,
            passage: None,
            fallback: None,
            cache: Vec::new(),
            cache_window_ms,
            last: None,
            recent_finals: String::new(),
            recent_at: 0,
            last_interim: String::new(),
            spoken_translations: Vec::new(),
            audio_epoch: None,
            recovery_text: String::new(),
            recovery_at: 0,
            plan_text: String::new(),
            plan_at: 0,
            reading: None,
            unresolved: None,
            plan_progress: None,
            generation: 0,
            memory: ReadingMemory::default(),
            introduced: false,
        }
    }

    /// Shares the local dedup cache and reading suppression with the async model.
    /// Newer local citations leave delayed model results eligible for review only.
    pub fn publish_model_result(
        &mut self,
        work: &ModelWork,
        detections: Vec<ModelDetection>,
        quote_evidence: &[QuoteCandidate],
        now: u64,
    ) -> Vec<CitationEmission> {
        let superseded = work.generation != self.generation;
        let mut out = Vec::new();
        for detection in detections {
            let reference = detection.reference;
            // A resume may only land in a passage this service has read.
            let credible = if work.transcript.resume {
                !superseded
                    && out.is_empty()
                    && self
                        .memory
                        .readings()
                        .iter()
                        .any(|r| r.book == reference.book && r.chapter == reference.chapter)
            } else {
                match detection.detection_type {
                    DetectionType::Explicit => {
                        VERSE_SIGNAL.is_match(&reference.source_text.to_ascii_lowercase())
                    }
                    DetectionType::Partial => {
                        !superseded
                            && VERSE_SIGNAL.is_match(&reference.source_text.to_ascii_lowercase())
                    }
                    DetectionType::Quote => quote_evidence.iter().any(|quote| {
                        quote.book == reference.book
                            && quote.chapter == reference.chapter
                            && (reference.verse_start
                                ..=reference.verse_end.unwrap_or(reference.verse_start))
                                .contains(&quote.verse)
                    }),
                    DetectionType::Paraphrase => false,
                }
            };
            if credible {
                if work.transcript.resume {
                    // Asked for on purpose, even for a verse heard before.
                    self.cache
                        .retain(|entry| !overlaps(&entry.reference, &reference));
                }
                self.publish_as(
                    CitationEmission {
                        reference,
                        translations: Vec::new(),
                        resolver: ScriptureResolver::Ai,
                        confidence: detection.confidence,
                        detection_type: detection.detection_type,
                        superseded,
                        ambiguous: false,
                        source: TranscriptKind::Final,
                        received_at_ms: work.transcript.received_at_ms,
                        emitted_at_ms: now,
                    },
                    &mut out,
                    work.transcript.resume,
                );
            }
        }
        out
    }

    pub fn run_regex_fallback(&mut self, work: &ModelWork, now: u64) -> Vec<CitationEmission> {
        if work.generation != self.generation {
            return Vec::new();
        }
        let mut out = Vec::new();
        for reference in match_normalized(&normalize(&work.transcript.text), false) {
            self.publish(
                CitationEmission {
                    reference,
                    translations: Vec::new(),
                    resolver: ScriptureResolver::Explicit,
                    confidence: 0.9,
                    detection_type: DetectionType::Explicit,
                    superseded: false,
                    ambiguous: false,
                    source: TranscriptKind::Final,
                    received_at_ms: work.transcript.received_at_ms,
                    emitted_at_ms: now,
                },
                &mut out,
            );
        }
        out
    }

    pub fn take_unresolved(&mut self) -> Option<UnresolvedTranscript> {
        self.unresolved.take()
    }

    pub fn take_plan_progress(&mut self) -> Option<(String, String)> {
        self.plan_progress.take()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn set_reading_passage(&mut self, passage: Option<ExplicitCitation>, at_ms: u64) {
        self.reading = passage.map(|reference| (reference, at_ms));
    }

    /// The service's readings, newest first.
    pub fn memory(&self) -> &ReadingMemory {
        &self.memory
    }

    /// This verse went on screen.
    pub fn note_shown(&mut self, reference: &ExplicitCitation, at_ms: u64) {
        self.memory.shown(reference, at_ms);
    }

    /// Follow-along heard this verse read to its end.
    pub fn note_read(&mut self, book: &'static str, chapter: f64, verse: f64, at_ms: u64) {
        self.memory.read(book, chapter, verse, at_ms);
    }

    fn supersede_model(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.plan_text.clear();
        self.plan_at = 0;
    }

    fn settle(&mut self) {
        self.recovery_text.clear();
        self.recent_finals.clear();
    }

    pub fn release(&mut self, reference: &ExplicitCitation) {
        for cached in &mut self.cache {
            if overlaps(&cached.reference, reference) {
                cached.released = true;
            }
        }
    }

    /// When the pending chapter fallback is due, if one is waiting.
    pub fn next_due_ms(&self) -> Option<u64> {
        self.fallback.as_ref().map(|pending| pending.emitted_at_ms)
    }

    /// Fires a due chapter fallback at its scheduled time.
    pub fn advance_to(&mut self, now: u64) -> Vec<CitationEmission> {
        let mut out = Vec::new();
        if self
            .fallback
            .as_ref()
            .is_some_and(|pending| pending.emitted_at_ms <= now)
        {
            let pending = self.fallback.take().expect("due fallback");
            self.supersede_model();
            self.publish(pending, &mut out);
        }
        out
    }

    pub fn process(&mut self, event: TranscriptEvent<'_>) -> Vec<CitationEmission> {
        self.process_with_evidence(event, LocalEvidence::default())
    }

    pub fn process_with_evidence(
        &mut self,
        event: TranscriptEvent<'_>,
        evidence: LocalEvidence<'_>,
    ) -> Vec<CitationEmission> {
        let now = event.at_ms;
        self.unresolved = None;
        self.plan_progress = None;
        let mut out = self.advance_to(now);
        if self.audio_epoch != event.audio_epoch {
            self.audio_epoch = event.audio_epoch;
            self.recovery_text.clear();
            self.recent_finals.clear();
            self.recent_at = 0;
            self.last_interim.clear();
        }
        let interim = event.kind == TranscriptKind::Interim;
        self.spoken_translations = find_spoken_translations(event.text);
        if interim && self.last_interim == event.text {
            return out;
        }
        if interim {
            event.text.clone_into(&mut self.last_interim);
        } else {
            self.last_interim.clear();
        }
        let translations = self.spoken_translations.clone();
        let spoken = normalize(event.text);
        let spoken = spoken.trim_matches(is_whitespace);
        self.introduced = INTRODUCED.is_match(spoken);
        self.expire(now);
        let recalled = self.recall(spoken, now);
        let references = if recalled.is_some() {
            Vec::new()
        } else {
            self.analyze(spoken, now, interim)
        };
        let emit = |reference| CitationEmission {
            reference,
            translations: Vec::new(),
            resolver: ScriptureResolver::Explicit,
            confidence: 0.9,
            detection_type: DetectionType::Explicit,
            superseded: false,
            ambiguous: false,
            source: event.kind,
            received_at_ms: now,
            emitted_at_ms: now,
        };
        if let Some((reference, ambiguous)) = recalled {
            // Interim speech may still be naming the verse ("verse 6…5").
            if !interim {
                self.supersede_model();
                // A resume is asked for on purpose, even for a verse heard
                // before; a bare mention of one ("as verse 62 says") is not.
                if RESUME.is_match(spoken) {
                    self.cache
                        .retain(|entry| !overlaps(&entry.reference, &reference));
                }
                self.publish(
                    CitationEmission {
                        resolver: ScriptureResolver::Reading,
                        ambiguous,
                        ..emit(reference)
                    },
                    &mut out,
                );
                self.settle();
            }
            return out;
        }
        if !references.is_empty() {
            self.supersede_model();
            for reference in references {
                self.publish(emit(reference), &mut out);
            }
            if !interim {
                self.settle();
            }
            return out;
        }
        if interim {
            return out;
        }

        if !translations.is_empty()
            && let Some(last) = &self.last
            && now.saturating_sub(last.emitted_at_ms) <= 120_000
        {
            let mut reference = last.reference.clone();
            reference.source_text = utf16_prefix(event.text, 150);
            let before = out.len();
            let mut follow_up = emit(reference);
            follow_up.received_at_ms = last.received_at_ms;
            follow_up.source = last.source;
            follow_up.resolver = last.resolver;
            follow_up.confidence = last.confidence;
            follow_up.detection_type = last.detection_type;
            follow_up.translations.clone_from(&translations);
            self.publish(follow_up, &mut out);
            if out.len() != before {
                self.settle();
                return out;
            }
        }
        if now.saturating_sub(self.recent_at) > 12_000 {
            self.recent_finals.clear();
        }
        self.recent_at = now;
        self.recent_finals.push(' ');
        self.recent_finals.push_str(event.text);
        self.recent_finals = utf16_suffix(self.recent_finals.trim_matches(is_whitespace), 1_200);

        if let Some(source) = evidence.chapter
            && let Some((reference, confidence)) = self.chapter_quote(event.text, source)
        {
            let mut quote = emit(reference);
            quote.resolver = ScriptureResolver::QuotationLocal;
            quote.detection_type = DetectionType::Quote;
            quote.confidence = confidence;
            self.supersede_model();
            self.publish(quote, &mut out);
            self.settle();
            return out;
        }

        if bare_book(spoken).is_some() {
            return out;
        }
        let remembered = self.with_remembered_book(spoken);
        let incomplete = match_normalized(&remembered, false);
        if let Some(chapter) = incomplete.last()
            && !incomplete
                .iter()
                .any(|r| VERSE_SIGNAL.is_match(&r.source_text))
        {
            if citation_shaped(&remembered, &chapter.source_text) {
                let mut pending = emit(chapter.clone());
                pending.emitted_at_ms = now.saturating_add(5_000);
                pending.translations.clear();
                self.fallback = Some(pending);
            }
            return out;
        }
        if let Some(index) = evidence.plan
            && let Some((reference, confidence)) = self.plan_quote(event.text, now, index)
        {
            let mut quote = emit(reference);
            quote.resolver = ScriptureResolver::SermonPlan;
            quote.detection_type = DetectionType::Quote;
            quote.confidence = confidence;
            let before = out.len();
            self.publish(quote, &mut out);
            self.plan_text.clear();
            self.plan_at = 0;
            if out.len() != before {
                self.supersede_model();
                self.settle();
                return out;
            }
        }

        if now.saturating_sub(self.recovery_at) > 12_000 {
            self.recovery_text.clear();
        }
        if corrupted_normalized(spoken) {
            self.recovery_text = event.text.to_owned();
            self.recovery_at = now;
        } else if !self.recovery_text.is_empty() {
            self.recovery_text.push(' ');
            self.recovery_text.push_str(event.text);
            self.recovery_text = utf16_suffix(&self.recovery_text, 1_200);
        }

        if !self.recovery_text.is_empty() {
            if let Some(search) = evidence.quote
                && let Some(found) = recover_damaged_quote(&self.recovery_text, search)
                && let Some(book) = canonical_book(&found.book.to_ascii_lowercase())
            {
                let mut quote = emit(ExplicitCitation {
                    book,
                    chapter: found.chapter,
                    verse_start: found.verse_start,
                    verse_end: found.verse_end,
                    source_text: utf16_prefix(&self.recovery_text, 150),
                });
                quote.resolver = ScriptureResolver::QuotationLocal;
                quote.detection_type = DetectionType::Quote;
                self.supersede_model();
                self.publish(quote, &mut out);
                self.settle();
            } else {
                let citations = window_citations(&self.recovery_text);
                if citations.is_empty() {
                    self.unresolved = Some(UnresolvedTranscript {
                        text: self.recovery_text.clone(),
                        received_at_ms: now,
                        resume: false,
                    });
                } else {
                    self.supersede_model();
                    // Electron's analyze() resolves this window before
                    // requesting a model. It leaves the recovery window in place.
                    for reference in citations {
                        self.publish(emit(reference), &mut out);
                    }
                }
            }
            return out;
        }

        let matches = window_citations(&self.recent_finals);
        if !matches.is_empty() {
            self.supersede_model();
            self.settle();
            for reference in matches {
                self.publish(emit(reference), &mut out);
            }
        } else if let Some(text) = self.resume_request(spoken, event.text) {
            self.unresolved = Some(UnresolvedTranscript {
                text,
                received_at_ms: now,
                resume: true,
            });
        } else if sounds_like_scripture(&self.recent_finals) {
            self.unresolved = Some(UnresolvedTranscript {
                text: self.recent_finals.clone(),
                received_at_ms: now,
                resume: false,
            });
        }
        out
    }

    /// A resume naming no verse or book the rules know ("where we left off
    /// in the Gospel"), put to the model with the service's readings.
    fn resume_request(&self, spoken: &str, text: &str) -> Option<String> {
        if self.memory.newest().is_none()
            || !RESUME.is_match(spoken)
            || !READING_HINT.is_match(spoken)
        {
            return None;
        }
        let readings = self
            .memory
            .readings()
            .iter()
            .map(|r| {
                format!(
                    "{} {}:{}-{} ({}, resume at verse {})",
                    r.book,
                    r.chapter,
                    r.start,
                    r.end,
                    if r.kind == ReadingKind::Main {
                        "main reading"
                    } else {
                        "aside"
                    },
                    r.resume_verse()
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        Some(format!(
            "{text}\n\nThe preacher is returning to a passage read earlier in this service. \
             Passages read so far, newest first: {readings}. Answer with the one verse to \
             resume at, from these passages only."
        ))
    }

    fn chapter_quote(
        &mut self,
        text: &str,
        source: &mut ChapterSource<'_>,
    ) -> Option<(ExplicitCitation, f64)> {
        let context = self.context.as_mut()?;
        if !context.awaiting_verse
            || context.verse_start.is_some()
            || text.trim_matches(is_whitespace).is_empty()
        {
            return None;
        }
        context.quote_text.push(' ');
        context.quote_text.push_str(text);
        context.quote_text = utf16_suffix(&context.quote_text, 600);
        let candidates = context.quote_verses.get_or_insert_with(|| {
            let verses = source(context.book, context.chapter);
            tokenize_chapter(verses.iter().filter_map(|verse| {
                is_valid_scripture_reference(context.book, context.chapter, verse.verse, None)
                    .then_some((verse.verse as u32, verse.text.as_str()))
            }))
        });
        let (verse, confidence) = match_chapter_quote(&context.quote_text, candidates)?;
        let reference = ExplicitCitation {
            book: context.book,
            chapter: context.chapter,
            verse_start: verse.into(),
            verse_end: None,
            source_text: utf16_prefix(text, 150),
        };
        self.context = None;
        Some((reference, confidence))
    }

    fn plan_quote(
        &mut self,
        text: &str,
        now: u64,
        index: &SermonPlanIndex,
    ) -> Option<(ExplicitCitation, f64)> {
        if text.trim_matches(is_whitespace).is_empty() {
            return None;
        }
        if now.saturating_sub(self.plan_at) > 60_000 {
            self.plan_text.clear();
        }
        self.plan_at = now;
        self.plan_text.push(' ');
        self.plan_text.push_str(text);
        self.plan_text = utf16_suffix(self.plan_text.trim_matches(is_whitespace), 600);
        let matched = match_plan_quote(Some(index), &self.plan_text, DEFAULT_MIN_COVERAGE)?;
        self.plan_progress = Some((matched.entry.reference, matched.entry.plan_item_id));
        let verse = matched.entry.verse;
        let book = canonical_book(&verse.book.to_ascii_lowercase())?;
        Some((
            ExplicitCitation {
                book,
                chapter: verse.chapter,
                verse_start: verse.verse,
                verse_end: None,
                source_text: utf16_prefix(text, 150),
            },
            matched.coverage.min(0.95),
        ))
    }

    fn with_remembered_book(&self, text: &str) -> String {
        if named_citation(text) {
            return text.to_owned();
        }
        let Some(book) = self
            .pending_book
            .map(|(book, _)| book)
            .or_else(|| self.passage.map(|(book, _)| book))
            .or_else(|| self.context.as_ref().map(|c| c.book))
        else {
            return text.to_owned();
        };
        if let Some(verse) = VERSE_FIRST.captures(text) {
            return format!("{book} chapter {} verse {}", &verse[2], &verse[1]);
        }
        if let Some(chapter) = CHAPTER.find(text) {
            return if VERSE_WORD.is_match(&text[..chapter.start()]) {
                text.to_owned()
            } else {
                format!("{book} {}", &text[chapter.start()..])
            };
        }
        let chapter = self
            .passage
            .and_then(|(_, chapter)| chapter)
            .or_else(|| self.context.as_ref().map(|c| c.chapter));
        if let Some(verse) = VERSE.find(text)
            && let Some(chapter) = chapter
        {
            return format!("{book} chapter {chapter} {}", &text[verse.start()..]);
        }
        text.to_owned()
    }

    /// A verse or a resume resolved against the service's readings, when the
    /// words leave out the chapter and no citation was just heard.
    fn recall(&mut self, spoken: &str, now: u64) -> Option<(ExplicitCitation, bool)> {
        if self.memory.newest().is_none() || named_citation(spoken) || CHAPTER_WORD.is_match(spoken)
        {
            return None;
        }
        let phrase: String = spoken
            .chars()
            .filter(|c| !matches!(c, '.' | '!' | '?' | ','))
            .collect();
        let phrase = phrase.trim_matches(is_whitespace);
        let resume = RESUME.is_match(spoken);
        // Right after an aside, "continue to verse 29" carries the aside on;
        // "continue from verse 20" goes back to the main reading.
        let fresh = |verse: f64| {
            self.context.as_ref().is_some_and(|c| {
                !resume
                    || c.verse_start
                        .is_some_and(|at| at <= verse && verse <= at + 3.0)
                    || self.memory.readings().iter().any(|r| {
                        r.book == c.book && r.chapter == c.chapter && r.kind == ReadingKind::Main
                    })
            })
        };
        let named = BOOK_LEAD
            .captures_iter(spoken)
            .find_map(|c| canonical_book(&c[1]))
            .or_else(|| self.pending_book.map(|(book, _)| book));
        let (reading, start, end, ambiguous) = if let Some(verse) = VERSE.find(spoken) {
            let range = RANGE.captures(&spoken[verse.start()..])?;
            let start = string_to_number(&range[1]);
            let end = range
                .get(2)
                .map(|m| string_to_number(m.as_str()))
                .filter(|end| *end > start);
            let found = if let Some(book) = named {
                let reading = self.memory.of_book(book, Some(start))?;
                (reading, false)
            } else if fresh(start) {
                // "John 3 … verse 16": the citation just heard.
                return None;
            } else if resume {
                let recall = self.memory.main_with(start)?;
                (recall.reading, recall.ambiguous)
            } else {
                let recall = self.memory.current_with(start)?;
                (recall.reading, recall.ambiguous)
            };
            (found.0, start, end, found.1)
        } else if RESUME_ONLY.is_match(phrase) {
            let reading = self.memory.newest_main()?;
            (reading, reading.resume_verse(), None, false)
        } else {
            let book = RESUME_BOOK
                .captures(phrase)
                .and_then(|c| canonical_book(&c[1]))?;
            let reading = self.memory.of_book(book, None)?;
            (reading, reading.resume_verse(), None, false)
        };
        let reference = reading.citation(start, end);
        if !is_valid_scripture_reference(reference.book, reference.chapter, start, end) {
            return None;
        }
        self.fallback = None;
        self.pending_book = None;
        self.passage = Some((reference.book, Some(reference.chapter)));
        self.context = Some(Context {
            book: reference.book,
            chapter: reference.chapter,
            updated: now,
            awaiting_verse: true,
            verse_start: Some(start),
            awaiting_range_end: RANGE_PENDING.is_match(spoken),
            quote_text: String::new(),
            quote_verses: None,
        });
        Some((reference, ambiguous))
    }

    fn expire(&mut self, now: u64) {
        if self
            .context
            .as_ref()
            .is_some_and(|c| now.saturating_sub(c.updated) > 12_000)
        {
            self.context = None;
        }
        if self
            .pending_book
            .is_some_and(|(_, at)| now.saturating_sub(at) > 12_000)
        {
            self.pending_book = None;
        }
    }

    fn analyze(&mut self, spoken: &str, now: u64, interim: bool) -> Vec<ExplicitCitation> {
        self.expire(now);
        if let Some(book) = bare_book(spoken) {
            self.fallback = None;
            self.pending_book = Some((book, now));
            self.passage = Some((book, None));
            self.context = None;
            return Vec::new();
        }
        let text = self.with_remembered_book(spoken);
        let direct = match_normalized(&text, !interim);
        if corrupted_normalized(&text) || (direct.is_empty() && named_citation(&text)) {
            self.fallback = None;
            self.context = None;
            self.pending_book = None;
            self.passage = None;
        }
        let verse_word = VERSE_WORD.is_match(&text);
        if let Some(last) = direct.last() {
            self.passage = Some((last.book, Some(last.chapter)));
            self.pending_book = None;
            self.context = Some(Context {
                book: last.book,
                chapter: last.chapter,
                updated: now,
                awaiting_verse: verse_word || !VERSE_SIGNAL.is_match(&last.source_text),
                verse_start: None,
                awaiting_range_end: false,
                quote_text: String::new(),
                quote_verses: None,
            });
        } else if let Some(context) = &mut self.context
            && verse_word
        {
            context.awaiting_verse = true;
            context.updated = now;
        }

        let mut references = if direct.is_empty() {
            self.context
                .as_mut()
                .filter(|context| context.awaiting_verse)
                .map_or_else(Vec::new, |context| fragments(context, &text, now, interim))
        } else {
            direct
                .into_iter()
                .filter(|r| VERSE_SIGNAL.is_match(&r.source_text) && (!interim || !ambiguous(r)))
                .collect()
        };
        references.retain(|r| {
            is_valid_scripture_reference(r.book, r.chapter, r.verse_start, r.verse_end)
        });
        if let Some(last) = references.last()
            && let Some(context) = &mut self.context
        {
            context.verse_start = Some(last.verse_start);
            context.awaiting_range_end = RANGE_PENDING.is_match(&text);
            context.awaiting_verse = true;
        }
        references
    }

    fn publish(&mut self, emission: CitationEmission, out: &mut Vec<CitationEmission>) {
        self.publish_as(emission, out, false);
    }

    /// `resume`: a model answer to a resume, which lands in a passage read
    /// earlier on purpose, so reading suppression does not apply.
    fn publish_as(
        &mut self,
        mut emission: CitationEmission,
        out: &mut Vec<CitationEmission>,
        resume: bool,
    ) {
        self.fallback = None;
        let now = emission.emitted_at_ms;
        self.cache.retain(|entry| now <= entry.expires);
        let reference = &emission.reference;
        if !is_valid_scripture_reference(
            reference.book,
            reference.chapter,
            reference.verse_start,
            reference.verse_end,
        ) || (!resume
            && matches!(
                emission.resolver,
                ScriptureResolver::QuotationLocal | ScriptureResolver::Ai
            )
            && (self.reading.as_ref().is_some_and(|(reading, at)| {
                now.saturating_sub(*at) <= READ_ALOUD_MS && contains(reading, reference)
            }) || self.memory.covers(reference, now, READ_ALOUD_MS)))
        {
            return;
        }
        let spoken = if emission.resolver == ScriptureResolver::Ai {
            &[][..]
        } else {
            self.spoken_translations.as_slice()
        };
        for translation in find_spoken_translations(&reference.source_text)
            .into_iter()
            .chain(spoken.iter().cloned())
        {
            if !emission.translations.contains(&translation) {
                emission.translations.push(translation);
            }
        }
        let mut cursor = 0;
        while cursor < self.cache.len() {
            let entry = &mut self.cache[cursor];
            if entry.released && emission.resolver != ScriptureResolver::Ai {
                if overlaps(&entry.reference, reference) {
                    self.cache.remove(cursor);
                    continue;
                }
            } else if contains(&entry.reference, reference) {
                emission
                    .translations
                    .retain(|translation| !entry.translations.contains(translation));
                if emission.translations.is_empty() {
                    return;
                }
                entry
                    .translations
                    .extend(emission.translations.iter().cloned());
            } else if contains(reference, &entry.reference) {
                self.cache.remove(cursor);
                continue;
            }
            cursor += 1;
        }
        if !self.cache.iter().any(|entry| {
            same_chapter(&entry.reference, reference)
                && entry.reference.verse_start == reference.verse_start
                && entry.reference.verse_end == reference.verse_end
        }) {
            self.cache.push(Cached {
                reference: reference.clone(),
                translations: emission.translations.clone(),
                expires: now.saturating_add(self.cache_window_ms),
                released: false,
            });
        }
        if matches!(
            emission.resolver,
            ScriptureResolver::Explicit | ScriptureResolver::Reading
        ) {
            self.memory
                .cite(&emission.reference, self.introduced, emission.emitted_at_ms);
        }
        self.last = Some(emission.clone());
        out.push(emission);
    }
}

fn fragments(context: &mut Context, text: &str, now: u64, interim: bool) -> Vec<ExplicitCitation> {
    let continuation = context.verse_start.filter(|_| {
        CONTINUATION.is_match(text) || (context.awaiting_range_end && NUMBER_FIRST.is_match(text))
    });
    let range_text = continuation.map_or_else(
        || text.to_owned(),
        |start| format!("{start} to {}", RANGE_LEAD.replace(text, "")),
    );
    let verse_word = VERSE_WORD.is_match(text);
    if !BARE_FRAGMENT.is_match(&range_text) && !VERSE_WORD.is_match(&range_text) {
        return Vec::new();
    }
    let Some(range) = RANGE.captures(&range_text) else {
        return Vec::new();
    };
    let verse_start = string_to_number(&range[1]);
    let verse_end = range.get(2).map(|m| string_to_number(m.as_str()));
    if interim && verse_end.is_none() && !verse_word {
        return Vec::new();
    }
    context.updated = now;
    let book = context.book;
    let chapter = context.chapter;
    let end = verse_end.map_or_else(String::new, |end| format!("-{end}"));
    let reference = ExplicitCitation {
        book,
        chapter,
        verse_start,
        verse_end,
        source_text: format!("{book} {chapter}:{verse_start}{end}"),
    };
    if let Some(list) = LIST.captures(text) {
        let numbers: Vec<_> = DIGITS
            .find_iter(&list[1])
            .map(|m| string_to_number(m.as_str()))
            .collect();
        if numbers.len() > 2 || numbers[1] != numbers[0] + 1.0 {
            return numbers
                .into_iter()
                .map(|verse_start| ExplicitCitation {
                    verse_start,
                    verse_end: None,
                    ..reference.clone()
                })
                .collect();
        }
    }
    vec![reference]
}

fn window_citations(text: &str) -> Vec<ExplicitCitation> {
    let mut citations = match_normalized(&normalize(text), true);
    citations.retain(|reference| VERSE_SIGNAL.is_match(&reference.source_text));
    citations
}

fn citation_shaped(text: &str, source: &str) -> bool {
    CHAPTER_WORD.is_match(source)
        || text.rfind(source).is_some_and(|at| {
            CITATION_END.is_match(&text[at + source.len()..]) || CITATION_LEAD.is_match(&text[..at])
        })
}

fn ambiguous(reference: &ExplicitCitation) -> bool {
    if reference.chapter != reference.verse_start
        || reference.verse_end.is_some()
        || DISAMBIGUATED.is_match(&reference.source_text)
    {
        return false;
    }
    let mut numbers = DIGITS.find_iter(&reference.source_text);
    match (numbers.next(), numbers.next(), numbers.next()) {
        (Some(first), Some(second), None) => {
            string_to_number(first.as_str()) == string_to_number(second.as_str())
        }
        _ => false,
    }
}

fn same_chapter(a: &ExplicitCitation, b: &ExplicitCitation) -> bool {
    a.book == b.book && a.chapter == b.chapter
}

fn contains(a: &ExplicitCitation, b: &ExplicitCitation) -> bool {
    same_chapter(a, b)
        && a.verse_start <= b.verse_start
        && a.verse_end.unwrap_or(a.verse_start) >= b.verse_end.unwrap_or(b.verse_start)
}

fn overlaps(a: &ExplicitCitation, b: &ExplicitCitation) -> bool {
    same_chapter(a, b)
        && a.verse_start <= b.verse_end.unwrap_or(b.verse_start)
        && b.verse_start <= a.verse_end.unwrap_or(a.verse_start)
}

pub(crate) fn utf16_prefix(text: &str, limit: usize) -> String {
    String::from_utf16_lossy(&text.encode_utf16().take(limit).collect::<Vec<_>>())
}

fn utf16_suffix(text: &str, limit: usize) -> String {
    let text: Vec<_> = text.encode_utf16().collect();
    String::from_utf16_lossy(&text[text.len().saturating_sub(limit)..])
}
