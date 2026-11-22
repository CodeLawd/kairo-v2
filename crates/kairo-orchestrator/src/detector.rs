//! The live scripture detector, composed from prompt 035–037's parts: the
//! deterministic citation detector, the managed model queue and the regex
//! fallback. From the `ScriptureDetector` facade and
//! `live-detection-wiring.ts`.
//!
//! Transcripts are processed synchronously, in arrival order, on the
//! caller's task. A timer task sleeps until the next chapter fallback or
//! queued model work is due (or until woken by new work) and dispatches it; in fallback mode (offline or the provider refusing)
//! queued work goes through the local regex instead.

use futures_util::future::BoxFuture as ModelFuture;
use kairo_domain::cloud::ManagedRequest;
use kairo_domain::scripture::plan::SermonPlanIndex;
use kairo_domain::transcript::{InterimResult, TranscriptResult};
use kairo_scripture::citation::ExplicitCitation;
use kairo_scripture::live_detector::{
    CitationEmission, LiveCitationDetector, LocalEvidence, ReadingKind, ScriptureResolver,
    TranscriptEvent, TranscriptKind,
};
use kairo_scripture::model_detector::{
    DetectionType, ManagedDetectorTransport, ManagedModelDetector, ModelConfig, ModelError,
    ModelOutcome, ModelQueue, ModelWork,
};
use kairo_scripture::service::QuoteCandidate;
use kairo_scripture::verse_bounds::is_valid_scripture_reference;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, watch};

/// One detected reference, as the orchestrator handles it.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptureReference {
    pub book: String,
    pub chapter: f64,
    pub verse_start: f64,
    pub verse_end: Option<f64>,
    pub confidence: f64,
    pub detection_type: DetectionType,
    pub source_text: String,
    pub stt_received_at: Option<f64>,
    pub detection_started_at: Option<f64>,
    pub transcript_source: Option<TranscriptKind>,
    pub resolver: Option<ScriptureResolver>,
    /// A model result that landed after a newer local detection.
    pub superseded: bool,
    /// General corpus quotations need operator approval.
    pub suggestion_only: bool,
    pub ambiguous: bool,
    pub provisional: bool,
    pub quote_evidence: Option<String>,
    /// Translations named with the reference, in spoken order.
    pub translations: Vec<String>,
}

impl ScriptureReference {
    pub fn from_emission(e: &CitationEmission, epoch_ms: f64) -> Self {
        Self {
            book: e.reference.book.to_owned(),
            chapter: e.reference.chapter,
            verse_start: e.reference.verse_start,
            verse_end: e.reference.verse_end,
            confidence: e.confidence,
            detection_type: e.detection_type,
            source_text: e.reference.source_text.clone(),
            stt_received_at: Some(epoch_ms + e.received_at_ms as f64),
            detection_started_at: Some(epoch_ms + e.emitted_at_ms as f64),
            transcript_source: Some(e.source),
            resolver: Some(e.resolver),
            superseded: e.superseded,
            suggestion_only: false,
            ambiguous: e.ambiguous,
            provisional: false,
            quote_evidence: None,
            translations: e.translations.clone(),
        }
    }

    fn citation(&self) -> Option<ExplicitCitation> {
        Some(ExplicitCitation {
            book: canonical_book(&self.book)?,
            chapter: self.chapter,
            verse_start: self.verse_start,
            verse_end: self.verse_end,
            source_text: self.source_text.clone(),
        })
    }
}

/// The detector's own `&'static` name for a display book name.
fn canonical_book(book: &str) -> Option<&'static str> {
    kairo_scripture::citation::canonical_book(&book.to_lowercase())
}

/// `canAutoPresentScriptureReference`.
pub fn can_auto_present(reference: &ScriptureReference) -> bool {
    reference.detection_type != DetectionType::Partial
        && !reference.superseded
        && !reference.suggestion_only
        && is_valid_scripture_reference(
            &reference.book,
            reference.chapter,
            reference.verse_start,
            reference.verse_end,
        )
}

/// A reference spoken in full ("John 3:16") and read by the local parser, or
/// a verse placed in a remembered reading. It goes live at once: the safety
/// delay is for quotes, paraphrases, guesses and resumes two readings fit,
/// where the operator may need to step in.
pub fn presents_at_once(reference: &ScriptureReference) -> bool {
    reference.detection_type == DetectionType::Explicit
        && reference
            .resolver
            .is_none_or(|r| matches!(r, ScriptureResolver::Explicit | ScriptureResolver::Reading))
        && !reference.ambiguous
        && !reference.provisional
}

/// The passage the preacher is reading, so reading it aloud is not a quote.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadingPassage {
    pub book: String,
    pub chapter: f64,
    pub start: f64,
    pub end: f64,
    /// The passage's words, where known, so speech that reads it aloud is
    /// recognised even when quote search ranks only its parallels.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DetectorEvent {
    Detection(Vec<ScriptureReference>),
    /// A provisional general quote was withdrawn; evidence key.
    QuoteRetracted(String),
    PlanProgress {
        reference: String,
        item_id: String,
    },
    Stats {
        total_calls: u64,
        average_latency_ms: f64,
    },
    Error(String),
    RequestSuccess,
}

pub trait Detector: Send + Sync {
    fn on_final(&self, result: &TranscriptResult);
    fn on_interim(&self, result: &InterimResult);
    /// Lets the next mention of this verse through the dedup again.
    fn release(&self, book: &str, chapter: f64, verse_start: f64);
    fn set_reading_passage(&self, passage: Option<ReadingPassage>);
    /// This verse went on screen: the reading it belongs to has got here.
    fn note_shown(&self, book: &str, chapter: f64, verse: f64);
    /// Follow-along heard this verse read to its end.
    fn note_read(&self, book: &str, chapter: f64, verse: f64);
    /// Where the main reading paused, while an aside is the newest reading.
    fn paused_reading(&self) -> Option<(String, f64, f64)>;
    fn is_general_quote_current(&self, reference: &ScriptureReference) -> bool;
    fn set_fallback_mode(&self, on: bool);
    fn subscribe(&self) -> broadcast::Receiver<DetectorEvent>;
    fn destroy(&self);
}

/// Local Bible data for chapter-quote and plan matching. Synchronous: it
/// runs inside transcript processing.
pub trait LocalEvidenceSource: Send + Sync {
    fn chapter_verses(&self, book: &str, chapter: f64) -> Vec<QuoteCandidate>;
    fn quote_candidates(&self, phrase: &str) -> Vec<QuoteCandidate>;
}

pub type PlanIndexProvider = Arc<dyn Fn() -> Option<SermonPlanIndex> + Send + Sync>;

struct SharedTransport(Arc<dyn ManagedDetectorTransport>);

impl ManagedDetectorTransport for SharedTransport {
    fn request(&self, request: ManagedRequest) -> ModelFuture<'_, Result<Value, ModelError>> {
        self.0.request(request)
    }
}

struct State {
    live: LiveCitationDetector,
    queue: ModelQueue,
    fallback_mode: bool,
    model_busy: bool,
    destroyed: bool,
}

/// The production detector.
pub struct LiveDetector {
    state: Arc<Mutex<State>>,
    events: broadcast::Sender<DetectorEvent>,
    evidence: Option<Arc<dyn LocalEvidenceSource>>,
    plan: PlanIndexProvider,
    /// Wall-clock ms at the detector's zero, so emissions map to epoch time.
    epoch_ms: f64,
    started: tokio::time::Instant,
    cancel: watch::Sender<bool>,
    /// Wakes the timer task when work may have become due sooner.
    wake: Arc<tokio::sync::Notify>,
}

use kairo_domain::js::now_ms as now_epoch_ms;

impl LiveDetector {
    /// Spawns the ticker on the current runtime. Without `transport` the model
    /// path is off and only local detection runs.
    pub fn new(
        transport: Option<Arc<dyn ManagedDetectorTransport>>,
        config: ModelConfig,
        evidence: Option<Arc<dyn LocalEvidenceSource>>,
        plan: PlanIndexProvider,
    ) -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        let (cancel, cancel_rx) = watch::channel(false);
        let detector = Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                live: LiveCitationDetector::default(),
                queue: ModelQueue::default(),
                fallback_mode: false,
                model_busy: false,
                destroyed: false,
            })),
            events,
            evidence,
            plan,
            epoch_ms: now_epoch_ms(),
            started: tokio::time::Instant::now(),
            cancel,
            wake: Arc::new(tokio::sync::Notify::new()),
        });
        let model = transport.map(|t| {
            Arc::new(tokio::sync::Mutex::new(ManagedModelDetector::new(
                SharedTransport(t),
                config,
            )))
        });
        let weak = Arc::downgrade(&detector);
        let (wake, started) = (detector.wake.clone(), detector.started);
        tokio::spawn(async move {
            loop {
                // Registered before the work is checked, so a wake between
                // the check and the wait is not lost.
                let woken = wake.notified();
                let deadline = {
                    let Some(detector) = weak.upgrade() else {
                        return;
                    };
                    if detector.lock().destroyed {
                        return;
                    }
                    detector.tick(model.clone(), cancel_rx.clone());
                    detector.next_due_ms(model.is_some())
                };
                match deadline {
                    Some(due) => tokio::select! {
                        () = tokio::time::sleep_until(started + Duration::from_millis(due)) => {}
                        () = woken => {}
                    },
                    None => woken.await,
                }
            }
        });
        detector
    }

    /// When the timer must next run: a due chapter fallback, or queued work
    /// the model (or the regex fallback) can take.
    fn next_due_ms(&self, has_model: bool) -> Option<u64> {
        let state = self.lock();
        let queued = if !state.fallback_mode && has_model && state.model_busy {
            None
        } else {
            state.queue.next_due_ms()
        };
        match (state.live.next_due_ms(), queued) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn emit(&self, emissions: Vec<CitationEmission>) {
        if emissions.is_empty() {
            return;
        }
        let refs = emissions
            .iter()
            .map(|e| ScriptureReference::from_emission(e, self.epoch_ms))
            .collect();
        let _ = self.events.send(DetectorEvent::Detection(refs));
    }

    fn tick(
        &self,
        model: Option<Arc<tokio::sync::Mutex<ManagedModelDetector<SharedTransport>>>>,
        cancel: watch::Receiver<bool>,
    ) {
        let now = self.now();
        let (due, work) = {
            let mut state = self.lock();
            let due = state.live.advance_to(now);
            let work = if state.fallback_mode || model.is_none() {
                state.queue.take_fallback(now).map(|w| (w, true))
            } else if state.model_busy {
                None
            } else {
                state.queue.take_ready(now).map(|w| (w, false))
            };
            (due, work)
        };
        self.emit(due);
        let Some((work, fallback)) = work else { return };
        if fallback {
            let found = {
                let mut state = self.lock();
                state.queue.complete();
                state.live.run_regex_fallback(&work, self.now())
            };
            self.emit(found);
            return;
        }
        let Some(model) = model else { return };
        self.lock().model_busy = true;
        let state = self.state.clone();
        let events = self.events.clone();
        let epoch = self.epoch_ms;
        let started = self.started;
        let wake = self.wake.clone();
        tokio::spawn(async move {
            let mut model = model.lock().await;
            let outcome = model.run(&work, cancel).await;
            let stats = model.stats();
            drop(model);
            let now = started.elapsed().as_millis() as u64;
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            s.model_busy = false;
            s.queue.complete();
            wake.notify_one();
            let _ = events.send(DetectorEvent::Stats {
                total_calls: stats.total_calls,
                average_latency_ms: stats.average_latency_ms() as f64,
            });
            match outcome {
                ModelOutcome::Parsed(detections) => {
                    let found = s.live.publish_model_result(&work, detections, &[], now);
                    drop(s);
                    let _ = events.send(DetectorEvent::RequestSuccess);
                    if !found.is_empty() {
                        let refs = found
                            .iter()
                            .map(|e| ScriptureReference::from_emission(e, epoch))
                            .collect();
                        let _ = events.send(DetectorEvent::Detection(refs));
                    }
                }
                ModelOutcome::Failed(error) => {
                    drop(s);
                    let _ = events.send(DetectorEvent::Error(error.message));
                }
                ModelOutcome::InvalidResponse => {
                    drop(s);
                    let _ = events.send(DetectorEvent::Error(
                        "AI detection returned an unreadable answer".into(),
                    ));
                }
                ModelOutcome::Cancelled => {}
            }
        });
    }

    fn process(
        &self,
        kind: TranscriptKind,
        text: &str,
        epoch: Option<f64>,
        submit_model: bool,
    ) -> (bool, Option<ModelWork>) {
        let now = self.now();
        let plan = (self.plan)();
        let evidence = self.evidence.clone();
        let (found, progress, work, claimed) = {
            let mut state = self.lock();
            if state.destroyed {
                return (false, None);
            }
            let generation_before = state.live.generation();
            let event = TranscriptEvent {
                at_ms: now,
                kind,
                text,
                audio_epoch: epoch.map(|e| e as u64),
            };
            let found = match &evidence {
                Some(source) => {
                    let mut chapter =
                        |book: &str, chapter: f64| source.chapter_verses(book, chapter);
                    let mut quote = |phrase: &str| source.quote_candidates(phrase);
                    state.live.process_with_evidence(
                        event,
                        LocalEvidence {
                            chapter: Some(&mut chapter),
                            quote: Some(&mut quote),
                            plan: plan.as_ref(),
                        },
                    )
                }
                None => state.live.process_with_evidence(
                    event,
                    LocalEvidence {
                        chapter: None,
                        quote: None,
                        plan: plan.as_ref(),
                    },
                ),
            };
            let progress = state.live.take_plan_progress();
            let unresolved = state.live.take_unresolved();
            let work = unresolved.map(|transcript| ModelWork {
                transcript,
                generation: state.live.generation(),
            });
            if submit_model && let Some(work) = &work {
                state.queue.submit(work.clone());
            }
            let claimed =
                state.live.generation() != generation_before || state.live.next_due_ms().is_some();
            (found, progress, work, claimed)
        };
        self.wake.notify_one();
        if let Some((reference, item_id)) = progress {
            let _ = self
                .events
                .send(DetectorEvent::PlanProgress { reference, item_id });
        }
        let claimed = claimed || !found.is_empty();
        self.emit(found);
        (claimed, work)
    }

    /// Run authoritative local paths now, leaving the managed fallback queued
    /// only after the application's general-corpus lookup has had first refusal.
    pub fn final_before_model(&self, result: &TranscriptResult) -> (bool, Option<ModelWork>) {
        self.process(
            TranscriptKind::Final,
            &result.text,
            result.audio_epoch,
            false,
        )
    }

    /// Interim citations are authoritative and bypass the debounced quote pass.
    pub fn interim_before_quote(&self, result: &InterimResult) -> bool {
        self.process(
            TranscriptKind::Interim,
            &result.text,
            result.audio_epoch,
            false,
        )
        .0
    }

    /// A final search may complete after newer local speech. Such work must
    /// never re-enter the model queue for an obsolete resolution generation.
    pub fn submit_model_work(&self, work: ModelWork) {
        let mut state = self.lock();
        if !state.destroyed && state.live.generation() == work.generation {
            state.queue.submit(work);
            self.wake.notify_one();
        }
    }
}

impl Detector for LiveDetector {
    fn on_final(&self, result: &TranscriptResult) {
        self.process(
            TranscriptKind::Final,
            &result.text,
            result.audio_epoch,
            true,
        );
    }

    fn on_interim(&self, result: &InterimResult) {
        self.process(
            TranscriptKind::Interim,
            &result.text,
            result.audio_epoch,
            true,
        );
    }

    fn release(&self, book: &str, chapter: f64, verse_start: f64) {
        let Some(book) = canonical_book(book) else {
            return;
        };
        self.lock().live.release(&ExplicitCitation {
            book,
            chapter,
            verse_start,
            verse_end: None,
            source_text: String::new(),
        });
    }

    fn set_reading_passage(&self, passage: Option<ReadingPassage>) {
        let now = self.now();
        let citation = passage.and_then(|p| {
            Some(ExplicitCitation {
                book: canonical_book(&p.book)?,
                chapter: p.chapter,
                verse_start: p.start,
                verse_end: (p.end != p.start).then_some(p.end),
                source_text: String::new(),
            })
        });
        self.lock().live.set_reading_passage(citation, now);
        self.wake.notify_one();
    }

    fn note_shown(&self, book: &str, chapter: f64, verse: f64) {
        let Some(book) = canonical_book(book) else {
            return;
        };
        let now = self.now();
        self.lock().live.note_shown(
            &ExplicitCitation {
                book,
                chapter,
                verse_start: verse,
                verse_end: None,
                source_text: String::new(),
            },
            now,
        );
    }

    fn note_read(&self, book: &str, chapter: f64, verse: f64) {
        let Some(book) = canonical_book(book) else {
            return;
        };
        let now = self.now();
        self.lock().live.note_read(book, chapter, verse, now);
    }

    fn paused_reading(&self) -> Option<(String, f64, f64)> {
        let state = self.lock();
        let memory = state.live.memory();
        if memory.newest()?.kind != ReadingKind::Aside {
            return None;
        }
        let main = memory.newest_main()?;
        Some((main.book.to_owned(), main.chapter, main.resume_verse()))
    }

    /// General-corpus quotes are not produced by this composition, so any
    /// reference it emitted is current.
    fn is_general_quote_current(&self, reference: &ScriptureReference) -> bool {
        reference.citation().is_some() || !reference.suggestion_only
    }

    fn set_fallback_mode(&self, on: bool) {
        self.lock().fallback_mode = on;
        self.wake.notify_one();
    }

    fn subscribe(&self) -> broadcast::Receiver<DetectorEvent> {
        self.events.subscribe()
    }

    fn destroy(&self) {
        let _ = self.cancel.send(true);
        let mut state = self.lock();
        state.destroyed = true;
        state.queue.clear();
        self.wake.notify_one();
    }
}
