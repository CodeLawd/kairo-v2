//! General-corpus quotation lookup before the managed detector fallback.
//! Partial speech needs two changing matches; finals are independently versioned
//! so a newer partial cannot discard their fallback or published evidence.
use kairo_domain::scripture::plan::verse_key;
use kairo_domain::scripture::spoken_translations::find_spoken_translations;
use kairo_domain::transcript::{InterimResult, TranscriptResult};
use kairo_orchestrator::detector::{
    Detector, DetectorEvent, LiveDetector, ReadingPassage, ScriptureReference,
};
use kairo_orchestrator::voice::BoxFuture;
use kairo_scripture::live_detector::{ScriptureResolver, TranscriptKind};
use kairo_scripture::model_detector::{DetectionType, ModelWork};
use kairo_scripture::quote::{
    GlobalQuoteCandidate, GlobalQuoteMatch, QuoteCorpus, fresh_speech_supports,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::broadcast;

trait QuoteSearch: Send + Sync {
    fn search(&self, text: String) -> BoxFuture<'_, Vec<GlobalQuoteMatch>>;
    fn current(&self, candidate: &GlobalQuoteCandidate) -> bool;
}
impl QuoteSearch for QuoteCorpus {
    fn search(&self, text: String) -> BoxFuture<'_, Vec<GlobalQuoteMatch>> {
        Box::pin(async move {
            self.search_general_quote(&text, false)
                .await
                .unwrap_or_default()
        })
    }
    fn current(&self, candidate: &GlobalQuoteCandidate) -> bool {
        // Eligibility is checked once more at the actual presentation boundary.
        // This is a local cache read on a core worker, never on the UI thread.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.is_general_quote_eligible(candidate))
        })
    }
}
#[derive(Default)]
struct State {
    epoch: Option<f64>,
    revision: u64,
    final_revision: u64,
    generation: u64,
    destroyed: bool,
    recent: String,
    recent_at: f64,
    last_interim: String,
    interim_pending: Option<Search>,
    interim_timer: bool,
    reading: Option<ReadingPassage>,
    versions: VecDeque<String>,
    published: HashMap<String, Vec<GlobalQuoteCandidate>>,
    provisional: Vec<String>,
    provisional_quote: Option<(String, String, String)>,
}
struct Search {
    text: String,
    fresh: String,
    evidence: String,
    revision: u64,
    final_revision: u64,
    generation: u64,
    provisional: bool,
    received: f64,
    work: Option<ModelWork>,
}

pub struct QuoteDetector {
    local: Arc<LiveDetector>,
    source: Arc<dyn QuoteSearch>,
    state: Mutex<State>,
    events: broadcast::Sender<DetectorEvent>,
    weak: Weak<Self>,
}
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
fn suffix(text: &str, limit: usize) -> String {
    let units = text.encode_utf16().collect::<Vec<_>>();
    String::from_utf16_lossy(&units[units.len().saturating_sub(limit)..])
}
fn evidence(epoch: Option<f64>, start: Option<f64>, fallback: &str) -> String {
    format!(
        "{}:{}",
        epoch
            .map(kairo_domain::js::number_to_string)
            .unwrap_or_else(|| "legacy".into()),
        start
            .map(kairo_domain::js::number_to_string)
            .unwrap_or_else(|| fallback.into())
    )
}
fn keys(matches: &[GlobalQuoteCandidate]) -> String {
    matches
        .iter()
        .map(|m| verse_key(&m.book, m.chapter, m.verse))
        .collect::<Vec<_>>()
        .join(",")
}

impl QuoteDetector {
    pub fn new(local: Arc<LiveDetector>, corpus: QuoteCorpus) -> Arc<Self> {
        let detector = Self::with_source(local, Arc::new(corpus.clone()));
        let weak = Arc::downgrade(&detector);
        corpus.on_general_quote_corpus_change(move || {
            if let Some(detector) = weak.upgrade() {
                detector.reset();
            }
        });
        detector
    }
    fn with_source(local: Arc<LiveDetector>, source: Arc<dyn QuoteSearch>) -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        let detector = Arc::new_cyclic(|weak| Self {
            local: local.clone(),
            source,
            state: Mutex::default(),
            events,
            weak: weak.clone(),
        });
        let mut notices = local.subscribe();
        let weak = Arc::downgrade(&detector);
        tokio::spawn(async move {
            loop {
                match notices.recv().await {
                    Ok(notice) => {
                        let Some(detector) = weak.upgrade() else {
                            break;
                        };
                        if lock(&detector.state).destroyed {
                            break;
                        }
                        let _ = detector.events.send(notice);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        detector
    }
    fn retract(&self, provisional_only: bool) {
        let mut state = lock(&self.state);
        let evidence = if provisional_only {
            std::mem::take(&mut state.provisional)
        } else {
            state.published.keys().cloned().collect()
        };
        for key in evidence {
            state.published.remove(&key);
            let _ = self.events.send(DetectorEvent::QuoteRetracted(key));
        }
        state.provisional_quote = None;
    }
    fn reset(&self) {
        {
            let mut state = lock(&self.state);
            state.revision += 1;
            state.final_revision += 1;
            state.generation += 1;
            state.versions.clear();
            state.interim_pending = None;
        }
        self.retract(false);
    }
    fn adopt_epoch(&self, epoch: Option<f64>) {
        let changed = {
            let mut state = lock(&self.state);
            if state.epoch == epoch {
                false
            } else {
                state.epoch = epoch;
                state.recent.clear();
                state.recent_at = 0.;
                state.last_interim.clear();
                true
            }
        };
        if changed {
            self.reset();
        }
    }
    fn search(&self, search: Search) {
        let weak = self.weak.clone();
        tokio::spawn(async move {
            let Some(detector) = weak.upgrade() else {
                return;
            };
            let matches = detector.source.search(search.text.clone()).await;
            detector.publish(search, matches);
        });
    }
    fn publish(&self, search: Search, mut matches: Vec<GlobalQuoteMatch>) {
        let mut state = lock(&self.state);
        if state.destroyed
            || search.generation != state.generation
            || (search.provisional && search.revision != state.revision)
            || (!search.provisional && search.final_revision != state.final_revision)
        {
            return;
        }
        if search.fresh != search.text {
            matches.retain(|m| fresh_speech_supports(&search.fresh, &m.candidate.text));
        }
        if !matches.is_empty()
            && matches.iter().all(|m| {
                state.reading.as_ref().is_some_and(|p| {
                    p.book.eq_ignore_ascii_case(&m.candidate.book)
                        && p.chapter == m.candidate.chapter
                        && m.candidate.verse >= p.start
                        && m.candidate.verse <= p.end
                })
            })
        {
            return;
        }
        if matches.is_empty() {
            state.provisional_quote = None;
            if let Some(ix) = state
                .provisional
                .iter()
                .position(|key| key == &search.evidence)
            {
                state.provisional.remove(ix);
                state.published.remove(&search.evidence);
                let _ = self
                    .events
                    .send(DetectorEvent::QuoteRetracted(search.evidence));
            }
            if let Some(work) = search.work {
                drop(state);
                self.local.submit_model_work(work);
            }
            return;
        }
        let candidates = matches
            .iter()
            .map(|m| m.candidate.clone())
            .collect::<Vec<_>>();
        let key = keys(&candidates);
        let published = state
            .published
            .get(&search.evidence)
            .map(|values| keys(values));
        let provisional_before = state.provisional.contains(&search.evidence);
        if published.as_deref() == Some(&key) && !search.provisional && !provisional_before {
            return;
        }
        if published.is_some() && (published.as_deref() != Some(&key) || !search.provisional) {
            let _ = self
                .events
                .send(DetectorEvent::QuoteRetracted(search.evidence.clone()));
        }
        if published.as_deref() == Some(&key) && search.provisional {
            return;
        }
        if search.provisional {
            let previous = state.provisional_quote.replace((
                search.evidence.clone(),
                search.text.clone(),
                key.clone(),
            ));
            if !previous
                .is_some_and(|(e, text, k)| e == search.evidence && text != search.text && k == key)
            {
                return;
            }
        } else {
            state.provisional_quote = None;
        }
        if !state.versions.contains(&search.evidence) {
            state.versions.push_back(search.evidence.clone());
        }
        if state.versions.len() > 200
            && let Some(oldest) = state.versions.pop_front()
        {
            state.published.remove(&oldest);
        }
        state.published.insert(search.evidence.clone(), candidates);
        if search.provisional {
            if !state.provisional.contains(&search.evidence) {
                state.provisional.push(search.evidence.clone());
            }
        } else {
            state.provisional.retain(|key| key != &search.evidence);
        }
        let translations = find_spoken_translations(&search.text);
        let refs = matches
            .into_iter()
            .map(|m| ScriptureReference {
                book: m.candidate.book,
                chapter: m.candidate.chapter,
                verse_start: m.candidate.verse,
                verse_end: None,
                confidence: 0.7,
                detection_type: DetectionType::Quote,
                source_text: suffix(&search.text, 150),
                stt_received_at: Some(search.received),
                detection_started_at: Some(search.received),
                transcript_source: Some(if search.provisional {
                    TranscriptKind::Interim
                } else {
                    TranscriptKind::Final
                }),
                resolver: Some(ScriptureResolver::QuotationLocal),
                superseded: false,
                suggestion_only: true,
                ambiguous: m.ambiguous,
                provisional: search.provisional,
                quote_evidence: Some(search.evidence.clone()),
                translations: translations.clone(),
            })
            .collect();
        drop(state);
        let _ = self.events.send(DetectorEvent::Detection(refs));
    }
}
impl Detector for QuoteDetector {
    fn on_final(&self, result: &TranscriptResult) {
        self.adopt_epoch(result.audio_epoch);
        {
            let mut state = lock(&self.state);
            state.revision += 1;
            state.final_revision += 1;
            state.interim_pending = None;
            state.last_interim.clear();
        }
        let (claimed, work) = self.local.final_before_model(result);
        if claimed {
            {
                let mut state = lock(&self.state);
                state.recent.clear();
                state.generation += 1;
            }
            self.retract(true);
            return;
        }
        let now = kairo_store::settings::now_ms();
        let search = {
            let mut state = lock(&self.state);
            if now - state.recent_at > 12_000. {
                state.recent.clear();
            }
            state.recent = suffix(format!("{} {}", state.recent, result.text).trim(), 1200);
            state.recent_at = now;
            Search {
                text: state.recent.clone(),
                fresh: result.text.clone(),
                evidence: evidence(result.audio_epoch, result.audio_start, &result.id),
                revision: state.revision,
                final_revision: state.final_revision,
                generation: state.generation,
                provisional: false,
                received: now,
                work,
            }
        };
        self.search(search);
    }
    fn on_interim(&self, result: &InterimResult) {
        self.adopt_epoch(result.audio_epoch);
        if self.local.interim_before_quote(result) {
            lock(&self.state).interim_pending = None;
            self.retract(true);
            return;
        }
        let schedule = {
            let mut state = lock(&self.state);
            if state.last_interim == result.text || state.destroyed {
                return;
            }
            state.last_interim.clone_from(&result.text);
            state.revision += 1;
            state.interim_pending = Some(Search {
                text: suffix(format!("{} {}", state.recent, result.text).trim(), 1200),
                fresh: result.text.clone(),
                evidence: evidence(
                    result.audio_epoch,
                    result.audio_start,
                    &kairo_domain::js::number_to_string(state.recent_at),
                ),
                revision: state.revision,
                final_revision: state.final_revision,
                generation: state.generation,
                provisional: true,
                received: kairo_store::settings::now_ms(),
                work: None,
            });
            if state.interim_timer {
                false
            } else {
                state.interim_timer = true;
                true
            }
        };
        if schedule {
            let weak = self.weak.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                if let Some(detector) = weak.upgrade() {
                    let pending = {
                        let mut state = lock(&detector.state);
                        state.interim_timer = false;
                        state.interim_pending.take()
                    };
                    if let Some(pending) = pending {
                        detector.search(pending);
                    }
                }
            });
        }
    }
    fn release(&self, book: &str, chapter: f64, verse: f64) {
        self.local.release(book, chapter, verse);
    }
    fn set_reading_passage(&self, passage: Option<ReadingPassage>) {
        self.local.set_reading_passage(passage.clone());
        lock(&self.state).reading = passage;
    }
    fn is_general_quote_current(&self, reference: &ScriptureReference) -> bool {
        let Some(evidence) = &reference.quote_evidence else {
            return true;
        };
        let source = {
            let state = lock(&self.state);
            if state.destroyed {
                return false;
            }
            state
                .published
                .get(evidence)
                .and_then(|values| {
                    values.iter().find(|v| {
                        verse_key(&v.book, v.chapter, v.verse)
                            == verse_key(&reference.book, reference.chapter, reference.verse_start)
                    })
                })
                .cloned()
        };
        source.is_some_and(|candidate| self.source.current(&candidate))
    }
    fn set_fallback_mode(&self, on: bool) {
        self.local.set_fallback_mode(on);
    }
    fn subscribe(&self) -> broadcast::Receiver<DetectorEvent> {
        self.events.subscribe()
    }
    fn destroy(&self) {
        self.reset();
        lock(&self.state).destroyed = true;
        self.local.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct Source {
        allowed: AtomicBool,
        calls: AtomicUsize,
    }
    impl QuoteSearch for Source {
        fn search(&self, text: String) -> BoxFuture<'_, Vec<GlobalQuoteMatch>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move {
                if text.contains("give and it shall") {
                    vec![GlobalQuoteMatch { candidate:GlobalQuoteCandidate { book:"Luke".into(),chapter:6.,verse:38.,text:"give and it shall be given unto you good measure pressed down shaken together".into(),..Default::default() },score:1.,ambiguous:false }]
                } else {
                    vec![]
                }
            })
        }
        fn current(&self, _: &GlobalQuoteCandidate) -> bool {
            self.allowed.load(Ordering::Relaxed)
        }
    }
    fn detector() -> (
        Arc<QuoteDetector>,
        Arc<Source>,
        broadcast::Receiver<DetectorEvent>,
    ) {
        let source = Arc::new(Source {
            allowed: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        });
        let local = LiveDetector::new(None, Default::default(), None, Arc::new(|| None));
        let detector = QuoteDetector::with_source(local, source.clone());
        let events = detector.subscribe();
        (detector, source, events)
    }
    fn final_result(text: &str) -> TranscriptResult {
        serde_json::from_value(serde_json::json!({"id":"final","text":text,"words":[],"isFinal":true,"timestamp":1,"duration":1,"audioEpoch":1,"audioStart":0})).unwrap()
    }
    fn interim(text: &str) -> InterimResult {
        InterimResult {
            audio_epoch: Some(1.),
            audio_start: Some(0.),
            audio_duration: None,
            text: text.into(),
            stability: 1.,
            timestamp: 1.,
        }
    }
    async fn detection(events: &mut broadcast::Receiver<DetectorEvent>) -> Vec<ScriptureReference> {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let DetectorEvent::Detection(refs) = events.recv().await.unwrap() {
                    break refs;
                }
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn final_quote_is_approval_only_and_rechecked_at_publication() {
        let (detector, source, mut events) = detector();
        detector.on_final(&final_result(
            "give and it shall be given unto you good measure pressed down",
        ));
        let refs = detection(&mut events).await;
        assert!(refs[0].suggestion_only);
        assert!(!refs[0].provisional);
        assert_eq!(refs[0].confidence, 0.7);
        assert!(detector.is_general_quote_current(&refs[0]));
        source.allowed.store(false, Ordering::Relaxed);
        assert!(!detector.is_general_quote_current(&refs[0]));
        detector.destroy();
    }
    #[tokio::test]
    async fn partial_requires_two_changing_matches_then_retracts_if_words_change() {
        let (detector, _, mut events) = detector();
        detector.on_interim(&interim("give and it shall be given unto you good measure"));
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(events.try_recv().is_err());
        detector.on_interim(&interim(
            "give and it shall be given unto you good measure pressed down",
        ));
        let refs = detection(&mut events).await;
        assert!(refs[0].provisional);
        detector.on_interim(&interim("ordinary announcements"));
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event, DetectorEvent::QuoteRetracted("1:0".into()));
        assert!(!detector.is_general_quote_current(&refs[0]));
        detector.destroy();
    }
    #[tokio::test]
    async fn corpus_or_audio_epoch_changes_withdraw_prior_evidence() {
        let (detector, _, mut events) = detector();
        detector.on_final(&final_result(
            "give and it shall be given unto you good measure pressed down",
        ));
        let refs = detection(&mut events).await;
        detector.reset();
        assert_eq!(
            events.recv().await.unwrap(),
            DetectorEvent::QuoteRetracted("1:0".into())
        );
        assert!(!detector.is_general_quote_current(&refs[0]));
        detector.on_final(&final_result(
            "give and it shall be given unto you good measure pressed down",
        ));
        let refs = detection(&mut events).await;
        let mut next = final_result("ordinary announcements");
        next.audio_epoch = Some(2.);
        detector.on_final(&next);
        assert_eq!(
            events.recv().await.unwrap(),
            DetectorEvent::QuoteRetracted("1:0".into())
        );
        assert!(!detector.is_general_quote_current(&refs[0]));
        detector.destroy();
    }
    #[tokio::test]
    async fn explicit_citations_take_priority_over_global_search() {
        let (detector, source, mut events) = detector();
        detector.on_final(&final_result("John chapter three verse sixteen"));
        let refs = detection(&mut events).await;
        assert_eq!(refs[0].resolver, Some(ScriptureResolver::Explicit));
        assert_eq!(source.calls.load(Ordering::Relaxed), 0);
        detector.destroy();
    }
}
