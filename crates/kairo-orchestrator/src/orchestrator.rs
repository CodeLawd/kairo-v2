//! The orchestrator. From `src/main/orchestrator.ts`.
//!
//! One actor: every public call takes the state lock for its whole run, so
//! calls are handled one at a time in arrival order, as on Electron's single
//! event loop. Auto-present countdowns are timer tasks that re-enter through
//! the same lock; a countdown cancelled or superseded before it fires does
//! nothing.

use crate::clients::{
    ColoredLine, EngineClient, MediaClient, ProgramClient, ScriptureClient, ScriptureSlideMeta,
    SessionGate, SettingsClient, ShowKind, SttClient, TranscriptEvent, UsageClient, chapter_verses,
};
use crate::detector::{
    Detector, DetectorEvent, ReadingPassage, ScriptureReference, can_auto_present,
};
use crate::dispatch::dispatch_to_outputs;
use crate::live_plan::LivePlanService;
use crate::voice::{
    BoxFuture, LiveScriptureContext, VoiceDeps, VoiceNavigation, follow_up_context, is_on_screen,
    pick_command_context, reading_translation,
};
use kairo_core::scripture_traces::ScriptureTraces;
use kairo_domain::documents::DocumentSlide;
use kairo_domain::js::number_to_string;
use kairo_domain::media::theme_for_push;
use kairo_domain::output_health::dispatch_health;
use kairo_domain::overlay::{
    MediaFit, OverlayBackgroundType, OverlayContentKind, OverlayDispatchResult, OverlayOutput,
    OverlayTheme,
};
use kairo_domain::overlay_content::format_overlay_verse_text;
use kairo_domain::overlay_outputs::output_theme_for;
use kairo_domain::program::ProgramSlideInfo;
use kairo_domain::scripture::live_progress::{
    PassageFollowState, passage_read_through, tokenize, update_passage_follow_strict,
};
use kairo_domain::scripture::plan::{PlanReference, match_plan_reference};
use kairo_domain::scripture::small::single_verse_suggestion;
use kairo_domain::scripture::trace::{
    CompleteInput, ConfirmationLevel, ScriptureResolver, ScriptureTraceStage as Stage,
    TraceFailureReason, TraceStartInput, TraceStatus, TranscriptSource,
};
use kairo_domain::scripture::{PendingAutoPresent, ScriptureSuggestion, ScriptureVerse};
use kairo_domain::settings::ScriptureAutomation;
use kairo_domain::status::{
    HealthStatus, OrchestratorConfig, OrchestratorStatus, ServiceHealth, ServiceName, SessionStats,
    SttProvider,
};
use kairo_scripture::model_detector::DetectionType;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// Managed services credential marker (`MANAGED` in `cloud/managed.ts`).
pub const MANAGED: &str = "kairo-managed";
/// Haiku: about $0.80/MTok in, $4.00/MTok out; ~900 tokens in, ~150 out.
pub const COST_PER_CALL_USD: f64 = (900.0 * 0.8 + 150.0 * 4.0) / 1_000_000.0;
/// How long a detected passage stays the place spoken commands step from.
const FOLLOW_UP_WINDOW_MS: f64 = 120_000.0;
pub const NO_ACTIVE_SERVICE: &str = "Create a service in Operator before starting transcription.";

use kairo_domain::js::now_ms;

fn random_suffix() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..11].to_owned()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub session_id: String,
    pub started_at: f64,
    pub total_detections: f64,
    pub total_presentations: f64,
    pub detector_calls: f64,
    pub avg_detector_latency_ms: f64,
    pub start_history_length: f64,
}

/// Verse content formatted once per push and shared by every destination.
#[derive(Clone, Debug, PartialEq)]
struct PushContent {
    reference: String,
    text: String,
    colored: Option<Vec<ColoredLine>>,
}

/// Builds a detector for a session's config.
pub type DetectorFactory = Arc<dyn Fn(&OrchestratorConfig) -> Arc<dyn Detector> + Send + Sync>;

/// Everything the orchestrator drives.
#[derive(Clone)]
pub struct Services {
    pub engine: Arc<dyn EngineClient>,
    pub program: Arc<dyn ProgramClient>,
    pub media: Arc<dyn MediaClient>,
    pub scripture: Arc<dyn ScriptureClient>,
    pub stt: Arc<dyn SttClient>,
    pub transcripts: Arc<dyn Fn() -> broadcast::Receiver<TranscriptEvent> + Send + Sync>,
    pub gate: Arc<dyn SessionGate>,
    pub settings: Arc<dyn SettingsClient>,
    pub usage: Arc<dyn UsageClient>,
    pub live_plan: LivePlanService,
    pub detector: DetectorFactory,
    /// The app core whose resilience actor owns health and detector
    /// fallback (`resilienceManager`).
    pub resilience: Option<kairo_core::AppCore>,
    /// Detection-to-screen latency, read by Settings → Scripture timing.
    pub traces: Arc<ScriptureTraces>,
}

struct State {
    cfg: Option<OrchestratorConfig>,
    running: bool,
    health: Vec<ServiceHealth>,
    auto_timers: HashMap<String, JoinHandle<()>>,
    /// Insertion order is kept for recovery snapshots.
    pending: Vec<ScriptureSuggestion>,
    /// The verse behind each card, so dismissing it releases the dedup.
    suggestion_refs: HashMap<String, (String, f64, f64)>,
    session: Option<Session>,
    detector: Option<Arc<dyn Detector>>,
    pumps: Vec<JoinHandle<()>>,
    /// Bumped by every `start` and `stop`, so a start that awaited STT
    /// without the lock can tell it was superseded.
    start_generation: u64,
    /// A `start` is awaiting STT.
    starting: bool,
    /// The passage on screen, followed as it is read aloud.
    follow: Option<Following>,
    /// The followed passage whose auto-clear is counting down.
    auto_clear: Option<(String, JoinHandle<()>)>,
    /// Chapters of recent readings, newest first, fetched when each began,
    /// so going back to a reading after an aside needs no lookup.
    chapters: Vec<ChapterText>,
}

/// Chapters kept, one per recent reading.
const CHAPTERS_KEPT: usize = 4;

struct ChapterText {
    book: String,
    chapter: f64,
    translation: String,
    verses: Vec<ScriptureVerse>,
}

impl ChapterText {
    fn is(&self, book: &str, chapter: f64, translation: &str) -> bool {
        self.book.eq_ignore_ascii_case(book)
            && self.chapter == chapter
            && self.translation == translation
    }

    /// Verses `start..=end`, when the chapter has every one of them.
    fn range(&self, start: f64, end: f64) -> Option<Vec<ScriptureVerse>> {
        let verses: Vec<ScriptureVerse> = self
            .verses
            .iter()
            .filter(|v| v.verse >= start && v.verse <= end)
            .cloned()
            .collect();
        (verses.len() as f64 == end - start + 1.0).then_some(verses)
    }
}

/// The verse's opening words were heard in order, so the reader went on
/// into it: all of its first four content words, and at least three.
fn opening_heard(verse: &str, text: &str) -> bool {
    let mut lead = tokenize(verse);
    lead.truncate(4);
    let heard = tokenize(text);
    let mut words = heard.iter();
    lead.len() >= 3 && lead.iter().all(|word| words.any(|h| h == word))
}

/// A passage being read aloud and its cards, one per verse.
struct Following {
    state: PassageFollowState,
    cards: Vec<ScriptureSuggestion>,
}

impl Following {
    fn current(&self) -> Option<&ScriptureVerse> {
        self.cards
            .iter()
            .find(|c| c.passage_index.unwrap_or(0.0) == self.state.current_index)
            .and_then(|c| c.verses.first())
    }
}

impl State {
    fn chapter(&self, book: &str, chapter: f64, translation: &str) -> Option<&ChapterText> {
        self.chapters
            .iter()
            .find(|c| c.is(book, chapter, translation))
    }

    fn pending(&self, id: &str) -> Option<&ScriptureSuggestion> {
        self.pending.iter().find(|s| s.id == id)
    }

    fn take_pending(&mut self, id: &str) -> Option<ScriptureSuggestion> {
        let at = self.pending.iter().position(|s| s.id == id)?;
        Some(self.pending.remove(at))
    }

    fn put_pending(&mut self, suggestion: ScriptureSuggestion) {
        match self.pending.iter_mut().find(|s| s.id == suggestion.id) {
            Some(slot) => *slot = suggestion,
            None => self.pending.push(suggestion),
        }
    }

    fn update_health(
        &mut self,
        service: ServiceName,
        status: HealthStatus,
        last_error: Option<String>,
    ) {
        let entry = ServiceHealth {
            service,
            status,
            last_error: last_error.filter(|e| !e.is_empty()),
            last_updated: now_ms(),
        };
        match self.health.iter_mut().find(|h| h.service == service) {
            Some(slot) => *slot = entry,
            None => self.health.push(entry),
        }
    }
}

/// Synchronously readable state for voice navigation.
#[derive(Default)]
struct Quick {
    running: bool,
    auto_mode: bool,
    follow_up: Option<(LiveScriptureContext, f64)>,
    follow_up_revision: i64,
}

struct Inner {
    services: Services,
    state: tokio::sync::Mutex<State>,
    /// Serializes pushes to the engine so screens see them in order. Held
    /// across engine awaits instead of `state`, which must never be.
    present: tokio::sync::Mutex<()>,
    quick: Mutex<Quick>,
    status: broadcast::Sender<OrchestratorStatus>,
    pending_auto: broadcast::Sender<PendingAutoPresent>,
    follow_progress: broadcast::Sender<Option<PassageFollowState>>,
    voice: std::sync::OnceLock<VoiceNavigation>,
    /// Bounds concurrent detection lookups so a slow one never stalls the
    /// detector pump, without letting a burst flood the Bible service.
    lookups: Arc<tokio::sync::Semaphore>,
    /// Sequence of the newest detection that could auto-present. A lookup
    /// that finishes behind it may still show a card but never a countdown.
    latest_auto: std::sync::atomic::AtomicU64,
    next_detection: std::sync::atomic::AtomicU64,
    /// When follow-along last moved the screen, so a spoken "next verse" that
    /// arrives right after does not step twice.
    scripture_live_at: Mutex<Option<std::time::Instant>>,
}

/// A spoken "next verse" this soon after a verse went live asks for the verse
/// already showing.
const NEXT_ALREADY_SHOWN: Duration = Duration::from_secs(4);

/// Detection lookups in flight at once.
const DETECTION_LOOKUPS: usize = 2;

#[derive(Clone)]
pub struct Orchestrator {
    inner: Arc<Inner>,
}

fn initial_health() -> Vec<ServiceHealth> {
    [
        ServiceName::Audio,
        ServiceName::Stt,
        ServiceName::Detector,
        ServiceName::Output,
    ]
    .into_iter()
    .map(|service| ServiceHealth {
        service,
        status: HealthStatus::Ok,
        last_error: None,
        last_updated: now_ms(),
    })
    .collect()
}

struct VoiceHost(Weak<Inner>);

impl VoiceDeps for VoiceHost {
    fn enabled(&self) -> bool {
        self.0
            .upgrade()
            .is_some_and(|i| i.quick.lock().unwrap_or_else(|e| e.into_inner()).running)
    }

    fn auto_mode(&self) -> bool {
        self.0
            .upgrade()
            .is_some_and(|i| i.quick.lock().unwrap_or_else(|e| e.into_inner()).auto_mode)
    }

    fn current(&self) -> Option<LiveScriptureContext> {
        let inner = self.0.upgrade()?;
        Orchestrator { inner }.command_context()
    }

    fn chapter(
        &self,
        book: String,
        chapter: f64,
        translation: String,
    ) -> BoxFuture<'_, Result<Vec<ScriptureVerse>, String>> {
        Box::pin(async move {
            let inner = self.0.upgrade().ok_or("stopped")?;
            chapter_verses(
                inner.services.scripture.as_ref(),
                &book,
                chapter,
                translation,
            )
            .await
        })
    }

    fn paused_reading(&self) -> BoxFuture<'_, Option<(String, f64, f64)>> {
        Box::pin(async move {
            let inner = self.0.upgrade()?;
            let detector = inner.state.lock().await.detector.clone()?;
            detector.paused_reading()
        })
    }

    fn present(&self, suggestion: ScriptureSuggestion) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let Some(inner) = self.0.upgrade() {
                let orchestrator = Orchestrator { inner };
                orchestrator.present_scripture(suggestion, None).await;
            }
        })
    }

    fn suggest(&self, suggestion: ScriptureSuggestion) {
        let Some(inner) = self.0.upgrade() else {
            return;
        };
        let orchestrator = Orchestrator { inner };
        orchestrator.note_follow_up(&suggestion);
        orchestrator
            .inner
            .services
            .scripture
            .receive_suggestion(suggestion.clone());
        // Pending is recorded once the state lock is free.
        tokio::spawn(async move {
            orchestrator
                .inner
                .state
                .lock()
                .await
                .put_pending(suggestion);
        });
    }

    fn retranslate_row(&self, passage_id: &str, translation: &str) -> bool {
        let Some(inner) = self.0.upgrade() else {
            return false;
        };
        {
            let mut quick = inner.quick.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((context, _)) = quick.follow_up.as_mut()
                && context.passage_id.as_deref() == Some(passage_id)
            {
                // The next spoken step continues in the new translation.
                context.translation = translation.to_owned();
            }
        }
        inner
            .services
            .scripture
            .request_row_translation(passage_id, translation);
        true
    }

    fn error(&self, error: String) {
        tracing::warn!(error, "voice navigation");
    }
}

/// `readingPassageOf`: the whole passage a presented verse belongs to.
pub fn reading_passage_of(suggestion: &ScriptureSuggestion) -> Option<ReadingPassage> {
    let first = suggestion.verses.first()?;
    if let Some(range) = suggestion
        .passage_reference
        .as_deref()
        .and_then(parse_passage_range)
        && range.0 == first.chapter
    {
        return Some(ReadingPassage {
            book: first.book.clone(),
            chapter: first.chapter,
            start: range.1,
            end: range.2,
            text: String::new(),
        });
    }
    let last = suggestion.verses.last()?;
    Some(ReadingPassage {
        book: first.book.clone(),
        chapter: first.chapter,
        start: first.verse,
        end: if last.chapter == first.chapter {
            last.verse
        } else {
            first.verse
        },
        text: String::new(),
    })
}

/// `/^(.+?)\s+(\d+):(\d+)(?:[-–—](\d+))?$/` → (chapter, start, end).
fn parse_passage_range(reference: &str) -> Option<(f64, f64, f64)> {
    let (_, tail) = reference.rsplit_once(char::is_whitespace)?;
    let (chapter, verses) = tail.split_once(':')?;
    let (start, end) = match verses.split_once(['-', '–', '—']) {
        Some((s, e)) => (s, Some(e)),
        None => (verses, None),
    };
    let num = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<f64>().ok())
            .flatten()
    };
    let start = num(start)?;
    Some((
        num(chapter)?,
        start,
        match end {
            Some(e) => num(e)?,
            None => start,
        },
    ))
}

impl Orchestrator {
    /// Spawns voice navigation on the current runtime.
    pub fn new(services: Services) -> Self {
        let (status, _) = broadcast::channel(64);
        let (pending_auto, _) = broadcast::channel(64);
        let (follow_progress, _) = broadcast::channel(64);
        let inner = Arc::new(Inner {
            services,
            state: tokio::sync::Mutex::new(State {
                cfg: None,
                running: false,
                health: initial_health(),
                auto_timers: HashMap::new(),
                pending: Vec::new(),
                suggestion_refs: HashMap::new(),
                session: None,
                detector: None,
                pumps: Vec::new(),
                start_generation: 0,
                starting: false,
                follow: None,
                auto_clear: None,
                chapters: Vec::new(),
            }),
            present: tokio::sync::Mutex::new(()),
            quick: Mutex::new(Quick::default()),
            status,
            pending_auto,
            follow_progress,
            voice: std::sync::OnceLock::new(),
            lookups: Arc::new(tokio::sync::Semaphore::new(DETECTION_LOOKUPS)),
            latest_auto: Default::default(),
            next_detection: Default::default(),
            scripture_live_at: Mutex::new(None),
        });
        let voice = VoiceNavigation::new(Arc::new(VoiceHost(Arc::downgrade(&inner))));
        let _ = inner.voice.set(voice);
        Self { inner }
    }

    fn voice(&self) -> &VoiceNavigation {
        self.inner
            .voice
            .get()
            .expect("voice navigation set at construction")
    }

    fn quick(&self) -> std::sync::MutexGuard<'_, Quick> {
        self.inner.quick.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn subscribe_status(&self) -> broadcast::Receiver<OrchestratorStatus> {
        self.inner.status.subscribe()
    }

    pub fn subscribe_pending_auto(&self) -> broadcast::Receiver<PendingAutoPresent> {
        self.inner.pending_auto.subscribe()
    }

    /// Where the reading of the passage on screen has got; `None` when
    /// nothing is followed.
    pub fn subscribe_follow_progress(&self) -> broadcast::Receiver<Option<PassageFollowState>> {
        self.inner.follow_progress.subscribe()
    }

    fn status_of(state: &State) -> OrchestratorStatus {
        OrchestratorStatus {
            running: state.running,
            auto_mode: state.cfg.as_ref().is_some_and(|c| c.auto_mode),
            health: state.health.clone(),
            total_presentations: state
                .session
                .as_ref()
                .map_or(0.0, |s| s.total_presentations),
        }
    }

    fn emit_status(&self, state: &State) {
        let _ = self.inner.status.send(Self::status_of(state));
    }

    pub async fn status(&self) -> OrchestratorStatus {
        Self::status_of(&*self.inner.state.lock().await)
    }

    pub async fn stats(&self) -> Option<SessionStats> {
        let state = self.inner.state.lock().await;
        self.stats_of(&state)
    }

    fn stats_of(&self, state: &State) -> Option<SessionStats> {
        let session = state.session.as_ref()?;
        let history = self.inner.services.stt.history();
        let total_words: usize = history
            .iter()
            .skip(session.start_history_length as usize)
            .map(|r| {
                if r.words.is_empty() {
                    r.text
                        .split(kairo_domain::js::is_whitespace)
                        .filter(|w| !w.is_empty())
                        .count()
                } else {
                    r.words.len()
                }
            })
            .sum();
        Some(SessionStats {
            session_id: session.session_id.clone(),
            started_at: session.started_at,
            duration_ms: now_ms() - session.started_at,
            total_words: total_words as f64,
            total_detections: session.total_detections,
            total_presentations: session.total_presentations,
            detector_calls: session.detector_calls,
            avg_detector_latency_ms: session.avg_detector_latency_ms,
            estimated_cost_usd: session.detector_calls * COST_PER_CALL_USD,
        })
    }

    /// `orchestrator:start` from the UI: needs the transcription licence and an
    /// active service. The caller's credentials are ignored; managed services
    /// and Deepgram are always used.
    pub async fn start_from_ui(&self, config: OrchestratorConfig) -> Result<(), String> {
        self.inner.services.gate.assert_transcription().await?;
        if !self.inner.services.gate.has_active_service() {
            return Err(NO_ACTIVE_SERVICE.into());
        }
        let hydrated = OrchestratorConfig {
            stt_api_key: MANAGED.into(),
            llm_api_key: MANAGED.into(),
            stt_provider: SttProvider::Deepgram,
            ..config
        };
        self.start(hydrated).await;
        Ok(())
    }

    pub async fn start(&self, config: OrchestratorConfig) {
        let mut state = self.inner.state.lock().await;
        if state.running {
            self.stop_locked(&mut state);
        }
        tracing::info!(provider = ?config.stt_provider, language = config.stt_language, "orchestrator starting");
        state.cfg = Some(config.clone());
        self.quick().auto_mode = config.auto_mode;
        self.create_detector(&mut state, &config);
        if !config.llm_api_key.is_empty() {
            state.update_health(ServiceName::Detector, HealthStatus::Ok, None);
        }
        self.configure_stt(&config);
        state.start_generation += 1;
        state.starting = true;
        let generation = state.start_generation;
        // Connecting STT can take seconds on bad Wi-Fi: stop, status and
        // approvals must not wait for it.
        drop(state);
        let started = self.inner.services.stt.start().await;
        let mut state = self.inner.state.lock().await;
        if state.start_generation != generation {
            // A newer start owns STT now; a stop wants it stopped.
            if !state.starting && started.is_ok() {
                self.inner.services.stt.stop();
            }
            return;
        }
        state.starting = false;
        Self::apply_stt_start(&mut state, started);
        self.attach_audio(&mut state);
        state.session = Some(Session {
            session_id: format!("session-{}", now_ms()),
            started_at: now_ms(),
            total_detections: 0.0,
            total_presentations: 0.0,
            detector_calls: 0.0,
            avg_detector_latency_ms: 0.0,
            start_history_length: self.inner.services.stt.history().len() as f64,
        });
        state.running = true;
        self.quick().running = true;
        self.emit_status(&state);
        tracing::info!(session = ?state.session.as_ref().map(|s| &s.session_id), "orchestrator started");
    }

    fn apply_stt_start(state: &mut State, started: Result<(), String>) {
        match started {
            Ok(()) => state.update_health(ServiceName::Stt, HealthStatus::Ok, None),
            Err(error) => {
                tracing::error!(error, "stt start failed");
                state.update_health(ServiceName::Stt, HealthStatus::Error, Some(error));
            }
        }
    }

    fn attach_audio(&self, state: &mut State) {
        match self.inner.services.stt.attach_audio() {
            Ok(()) => state.update_health(ServiceName::Audio, HealthStatus::Ok, None),
            Err(error) => {
                tracing::error!(error, "audio stream setup failed");
                state.update_health(ServiceName::Audio, HealthStatus::Error, Some(error));
            }
        }
    }

    /// Book names boosted, the live playlist's own books first.
    fn configure_stt(&self, config: &OrchestratorConfig) {
        let books: Vec<String> = self
            .inner
            .services
            .live_plan
            .with_index(|index| {
                index.map(|i| i.ordered.iter().map(|e| e.verse.book.clone()).collect())
            })
            .unwrap_or_default();
        let refs: Vec<&str> = books.iter().map(String::as_str).collect();
        self.inner.services.stt.configure(
            config.stt_provider,
            &config.stt_language,
            kairo_stt::keyterms::scripture_keyterms(&config.stt_language, &refs),
        );
    }

    pub async fn stop(&self) {
        let mut state = self.inner.state.lock().await;
        if state.starting {
            // The pending start sees the new generation and stops STT itself.
            state.starting = false;
            state.start_generation += 1;
            for pump in state.pumps.drain(..) {
                pump.abort();
            }
            if let Some(detector) = state.detector.take() {
                detector.destroy();
            }
            return;
        }
        self.stop_locked(&mut state);
    }

    fn stop_locked(&self, state: &mut State) {
        if !state.running {
            return;
        }
        tracing::info!("orchestrator stopping");
        state.running = false;
        state.start_generation += 1;
        {
            let mut quick = self.quick();
            quick.running = false;
            quick.follow_up = None;
        }
        self.voice().reset();
        for (_, timer) in state.auto_timers.drain() {
            timer.abort();
        }
        state.pending.clear();
        state.suggestion_refs.clear();
        self.end_follow(state);
        self.inner.services.stt.detach_audio();
        self.inner.services.stt.stop();
        for pump in state.pumps.drain(..) {
            pump.abort();
        }
        if let Some(detector) = state.detector.take() {
            detector.destroy();
        }
        self.emit_status(state);
        if let Some(stats) = self.stats_of(state) {
            tracing::info!(
                duration_sec = (stats.duration_ms / 1000.0).round(),
                words = stats.total_words,
                detections = stats.total_detections,
                presentations = stats.total_presentations,
                cost_usd = format!("{:.4}", stats.estimated_cost_usd),
                "session complete"
            );
        }
        state.session = None;
    }

    /// Builds the detector and subscribes it to live transcription; built even
    /// without a model key so local detection still drives the Operator.
    fn create_detector(&self, state: &mut State, config: &OrchestratorConfig) {
        for pump in state.pumps.drain(..) {
            pump.abort();
        }
        if let Some(old) = state.detector.take() {
            old.destroy();
        }
        state.suggestion_refs.clear();
        let detector = (self.inner.services.detector)(config);
        state.detector = Some(detector.clone());

        let weak = Arc::downgrade(&self.inner);
        let mut events = detector.subscribe();
        state.pumps.push(tokio::spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(e) => e,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                let Some(inner) = weak.upgrade() else { return };
                Orchestrator { inner }.on_detector_event(event).await;
            }
        }));

        // Offline or the model refused twice: detect locally until a probe.
        if let Some(core) = self.inner.services.resilience.clone() {
            let detector = detector.clone();
            let mut status = core.watch::<kairo_domain::status::ResilienceStatus>();
            state.pumps.push(tokio::spawn(async move {
                while let Some(status) = status.next().await {
                    // A weak link also pauses AI calls: they would time out
                    // and compete with the audio for the same upload.
                    detector.set_fallback_mode(
                        !status.internet_connected
                            || status.claude_fallback_active
                            || kairo_domain::status::stt_connection_weak(&status.health),
                    );
                }
            }));
        }

        // Finals drive analysis directly; commands are claimed first.
        let weak = Arc::downgrade(&self.inner);
        let mut transcripts = (self.inner.services.transcripts)();
        state.pumps.push(tokio::spawn(async move {
            loop {
                let event = match transcripts.recv().await {
                    Ok(e) => e,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                let Some(inner) = weak.upgrade() else { return };
                let orchestrator = Orchestrator { inner };
                let detector = orchestrator.inner.state.lock().await.detector.clone();
                let Some(detector) = detector else { continue };
                match event {
                    TranscriptEvent::Final(result) => {
                        if orchestrator.next_already_shown(&result.text) {
                            tracing::info!("\"next verse\" already on screen; not stepping again");
                            continue;
                        }
                        if !orchestrator.voice().claim(&result) {
                            detector.on_final(&result);
                        }
                        orchestrator.advance_follow(&result.text, true).await;
                    }
                    TranscriptEvent::Interim(result) => {
                        detector.on_interim(&result);
                        orchestrator.advance_follow(&result.text, false).await;
                    }
                }
            }
        }));
    }

    async fn on_detector_event(&self, event: DetectorEvent) {
        match event {
            DetectorEvent::Detection(refs) => self.spawn_detection(refs),
            DetectorEvent::QuoteRetracted(evidence) => {
                let mut state = self.inner.state.lock().await;
                let retracted: Vec<_> = state
                    .pending
                    .iter()
                    .filter(|s| {
                        s.suggestion_only == Some(true)
                            && s.quote_evidence.as_deref() == Some(evidence.as_str())
                    })
                    .cloned()
                    .collect();
                for suggestion in retracted {
                    self.dismiss_locked(&mut state, &suggestion.id);
                    let mut withdrawn = suggestion;
                    withdrawn.retracted = Some(true);
                    self.inner.services.scripture.receive_suggestion(withdrawn);
                }
            }
            DetectorEvent::PlanProgress { reference, item_id } => {
                self.inner
                    .services
                    .live_plan
                    .observe(&reference, Some(&item_id));
            }
            DetectorEvent::Stats {
                total_calls,
                average_latency_ms,
            } => {
                if let Some(session) = self.inner.state.lock().await.session.as_mut() {
                    session.detector_calls = total_calls as f64;
                    session.avg_detector_latency_ms = average_latency_ms;
                }
            }
            DetectorEvent::Error(message) => {
                tracing::error!(message, "detector error");
                let mut state = self.inner.state.lock().await;
                state.update_health(
                    ServiceName::Detector,
                    HealthStatus::Error,
                    Some(message.clone()),
                );
                self.emit_status(&state);
                drop(state);
                self.detector_failed(&message);
            }
            DetectorEvent::RequestSuccess => {
                if let Some(core) = &self.inner.services.resilience
                    && let Err(error) = core
                        .call(kairo_core::resilience::DetectorStateChange::Succeeded)
                        .await
                {
                    tracing::warn!(%error, "could not report detector success");
                }
            }
        }
    }

    /// Two failures in a row switch detection to the local fallback; a probe
    /// 30 s later lets the model try again. The fallback itself follows the
    /// published `ResilienceStatus` (see `create_detector`).
    fn detector_failed(&self, message: &str) {
        let Some(core) = self.inner.services.resilience.clone() else {
            return;
        };
        let message = message.to_owned();
        tokio::spawn(async move {
            use kairo_core::resilience::{DetectorFailed, probe_after_delay};
            match core.call(DetectorFailed(message)).await {
                Ok(Some(ticket)) => {
                    if let Err(error) = probe_after_delay(&core, ticket).await {
                        tracing::warn!(%error, "detector probe failed");
                    }
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(%error, "could not report detector failure"),
            }
        });
    }

    /// The connectivity poll and the periodic recovery save
    /// (`kairo_core::resilience::monitor`), until the returned task is aborted.
    pub fn spawn_monitoring(&self) -> Option<JoinHandle<()>> {
        let core = self.inner.services.resilience.clone()?;
        let weak = Arc::downgrade(&self.inner);
        Some(tokio::spawn(async move {
            let snapshot = move || {
                let weak = weak.clone();
                async move {
                    let inner = weak.upgrade().ok_or("orchestrator dropped")?;
                    Ok(Orchestrator { inner }.recovery_snapshot().await)
                }
            };
            if let Err(error) = kairo_core::resilience::monitor(&core, snapshot).await {
                tracing::warn!(%error, "resilience monitoring stopped");
            }
        }))
    }

    fn note_follow_up(&self, suggestion: &ScriptureSuggestion) {
        let mut quick = self.quick();
        quick.follow_up_revision -= 1;
        if let Some(context) = follow_up_context(suggestion, quick.follow_up_revision) {
            quick.follow_up = Some((context, now_ms()));
        }
    }

    /// The same two-minute window as a spoken translation follow-up.
    fn recent_follow_up(&self) -> Option<(LiveScriptureContext, f64)> {
        self.quick()
            .follow_up
            .clone()
            .filter(|(_, at)| now_ms() - at <= FOLLOW_UP_WINDOW_MS)
    }

    fn command_context(&self) -> Option<LiveScriptureContext> {
        pick_command_context(
            self.inner.services.program.scripture(),
            self.recent_follow_up(),
        )
    }

    pub async fn approve_suggestion(&self, id: &str) {
        let mut state = self.inner.state.lock().await;
        if let Some(suggestion) = state.pending(id).cloned() {
            Self::cancel_auto(&mut state, id);
            state.take_pending(id);
            drop(state);
            self.inner.services.usage.track("scripture_manual");
            self.present_scripture(suggestion, None).await;
            return;
        }
        drop(state);
        // A manual search card from the scripture service.
        if let Some(manual) = self.inner.services.scripture.pending_suggestion(id) {
            self.inner.services.usage.track("scripture_manual");
            self.present_scripture(manual, None).await;
        }
    }

    pub async fn dismiss_suggestion(&self, id: &str) {
        let mut state = self.inner.state.lock().await;
        self.dismiss_locked(&mut state, id);
    }

    fn dismiss_locked(&self, state: &mut State, id: &str) {
        Self::cancel_auto(state, id);
        state.take_pending(id);
        if let Some(following) = state.follow.as_mut() {
            following.cards.retain(|c| c.id != id);
        }
        // The next time the preacher says this verse it must come back.
        if let Some((book, chapter, verse)) = state.suggestion_refs.remove(id)
            && let Some(detector) = &state.detector
        {
            detector.release(&book, chapter, verse);
        }
        self.inner.services.scripture.dismiss_suggestion(id);
    }

    pub async fn dismiss_auto(&self, id: &str) {
        Self::cancel_auto(&mut *self.inner.state.lock().await, id);
        tracing::info!(id, "auto-present dismissed by user");
    }

    pub async fn set_auto_mode(&self, enabled: bool) {
        let mut state = self.inner.state.lock().await;
        if let Some(cfg) = state.cfg.as_mut() {
            cfg.auto_mode = enabled;
            self.quick().auto_mode = enabled;
        }
    }

    pub async fn set_confidence_threshold(&self, threshold: f64) {
        if let Some(cfg) = self.inner.state.lock().await.cfg.as_mut() {
            cfg.confidence_threshold = threshold.clamp(0.0, 1.0);
        }
    }

    /// Passage auto-present, the countdown, follow-along and auto-clear,
    /// changed mid-service.
    pub async fn set_automation(&self, automation: ScriptureAutomation) {
        let mut state = self.inner.state.lock().await;
        if let Some(cfg) = state.cfg.as_mut() {
            cfg.auto_present_passages = automation.auto_present_passages;
            cfg.auto_present_delay_sec = f64::from(automation.auto_present_delay_sec.value());
            cfg.follow_along = automation.follow_along;
            cfg.auto_clear = automation.auto_clear;
            cfg.auto_clear_delay_sec = automation.auto_clear_delay_sec.max(0.0);
        }
        if !automation.auto_clear {
            Self::cancel_auto_clear(&mut state);
        }
    }

    /// Auto-clear: takes scripture off the screens, but only while this verse
    /// is still what they show. Lyrics or a newer verse are left alone.
    async fn clear_scripture_if_live(&self, book: &str, chapter: f64, verse: f64) -> bool {
        let live = self.inner.services.program.scripture().map(|(c, _)| c);
        let showing = live.is_some_and(|c| {
            c.book.eq_ignore_ascii_case(book)
                && c.chapter == chapter
                && c.verse_start <= verse
                && verse <= c.verse_end.max(c.verse_start)
        });
        if !showing {
            return false;
        }
        tracing::info!(book, chapter, verse, "auto-clear: passage read through");
        self.clear_text().await
    }

    pub async fn set_scripture_translation(&self, translation: &str) {
        if let Some(cfg) = self.inner.state.lock().await.cfg.as_mut() {
            cfg.scripture_translation = translation.into();
        }
    }

    fn cancel_auto(state: &mut State, id: &str) {
        if let Some(timer) = state.auto_timers.remove(id) {
            timer.abort();
        }
    }

    /// Hands a detection to a bounded lookup task and returns at once, so a
    /// slow lookup cannot delay newer detections or retractions behind it.
    fn spawn_detection(&self, refs: Vec<ScriptureReference>) {
        use std::sync::atomic::Ordering;
        let seq = self.inner.next_detection.fetch_add(1, Ordering::SeqCst) + 1;
        if refs.iter().any(can_auto_present) {
            self.inner.latest_auto.store(seq, Ordering::SeqCst);
        }
        let this = self.clone();
        let lookups = self.inner.lookups.clone();
        tokio::spawn(async move {
            let Ok(_permit) = lookups.acquire_owned().await else {
                return;
            };
            this.handle_detection(refs, seq).await;
        });
    }

    async fn handle_detection(&self, refs: Vec<ScriptureReference>, seq: u64) {
        let mut state = self.inner.state.lock().await;
        let Some(cfg) = state.cfg.clone() else { return };
        if !state.running {
            return;
        }
        let session_id = state.session.as_ref().map(|s| s.session_id.clone());
        // One pass per translation named; none named takes the playlist's or default.
        let requests: Vec<(ScriptureReference, Option<String>)> = refs
            .into_iter()
            .flat_map(|r| {
                if r.translations.is_empty() {
                    vec![(r, None)]
                } else {
                    r.translations
                        .clone()
                        .into_iter()
                        .map(|t| (r.clone(), Some(t)))
                        .collect()
                }
            })
            .collect();
        for (reference, requested) in requests {
            let plan_ref = PlanReference {
                book: reference.book.clone(),
                chapter: reference.chapter,
                verse_start: reference.verse_start,
                verse_end: reference.verse_end,
            };
            let live_plan = &self.inner.services.live_plan;
            let plan_entries = live_plan
                .with_index(|index| match_plan_reference(index, &plan_ref, requested.as_deref()));
            let plan_entry = plan_entries.first().cloned();
            // Progress follows the playlist even in a translation it does not hold.
            let progress = plan_entry.clone().or_else(|| {
                requested.as_ref().and_then(|_| {
                    live_plan
                        .with_index(|index| match_plan_reference(index, &plan_ref, None))
                        .into_iter()
                        .next()
                })
            });
            if let Some(entry) = &progress {
                live_plan.observe(&entry.reference, Some(&entry.plan_item_id));
            }
            let reading = reading_translation(
                self.command_context().as_ref(),
                &reference.book,
                reference.chapter,
            );
            let mut translation = requested
                .clone()
                .or_else(|| plan_entry.as_ref().map(|e| e.translation.clone()))
                .or_else(|| reading.clone())
                .unwrap_or_else(|| cfg.scripture_translation.clone());
            let mut requested_translation = None;
            let trace = self.start_trace(&reference, &translation, &cfg);
            let traces = &self.inner.services.traces;
            let mut verses: Vec<ScriptureVerse> =
                plan_entries.iter().map(|e| e.verse.clone()).collect();
            // The chapter being read is already here: no lookup.
            if verses.is_empty()
                && let Some(found) = state
                    .chapter(&reference.book, reference.chapter, &translation)
                    .and_then(|c| {
                        c.range(
                            reference.verse_start,
                            reference.verse_end.unwrap_or(reference.verse_start),
                        )
                    })
            {
                verses = found;
            }
            if verses.is_empty() {
                // Lookups go to the network: never hold the state across them.
                drop(state);
                traces.mark(trace.as_deref(), Stage::BibleLookupStartedAt);
                let scripture = self.inner.services.scripture.clone();
                let lookup_ref = reference.clone();
                let quiet = requested
                    .clone()
                    .or_else(|| plan_entry.is_none().then(|| reading.clone()).flatten());
                let found =
                    kairo_scripture::api_bible::detection::lookup_with_translation_fallback(
                        |candidate: String| {
                            let scripture = scripture.clone();
                            let r = lookup_ref.clone();
                            async move {
                                let end = r
                                    .verse_end
                                    .map(|e| format!("-{}", number_to_string(e)))
                                    .unwrap_or_default();
                                let query = format!(
                                    "{} {}:{}{end}",
                                    r.book,
                                    number_to_string(r.chapter),
                                    number_to_string(r.verse_start)
                                );
                                Ok::<_, String>(match scripture.search(query, candidate).await {
                                    Ok(results) => {
                                        results.into_iter().flat_map(|x| x.verses).collect()
                                    }
                                    Err(error) => {
                                        tracing::warn!(book = r.book, error, "bible lookup error");
                                        Vec::new()
                                    }
                                })
                            }
                        },
                        &translation,
                        quiet.as_deref(),
                        &cfg.scripture_translation,
                    )
                    .await;
                traces.mark(trace.as_deref(), Stage::BibleLookupCompletedAt);
                state = self.inner.state.lock().await;
                if let Ok(found) = found {
                    verses = found.verses;
                    translation = found.translation;
                    requested_translation = requested.as_ref().and(found.requested_translation);
                }
            }
            // A lookup can finish after stop or after another service started.
            if !state.running || state.session.as_ref().map(|s| s.session_id.clone()) != session_id
            {
                traces.complete(trace.as_deref(), TraceStatus::Cancelled.into());
                return;
            }
            let detector = state.detector.clone();
            if reference.suggestion_only
                && !detector
                    .as_ref()
                    .is_some_and(|d| d.is_general_quote_current(&reference))
            {
                traces.complete(trace.as_deref(), TraceStatus::Superseded.into());
                continue;
            }
            let live = self.inner.services.program.scripture().map(|(c, _)| c);
            // No card reached the operator, or the passage is already showing
            // word for word (another card would only repeat it): let the next
            // mention try again.
            let on_screen = verses.len() == 1
                && is_on_screen(
                    live.as_ref(),
                    &reference.book,
                    reference.chapter,
                    reference.verse_start,
                    reference.verse_end,
                    &translation,
                );
            if verses.is_empty() || on_screen {
                if let Some(d) = &detector {
                    d.release(&reference.book, reference.chapter, reference.verse_start);
                }
                traces.complete(
                    trace.as_deref(),
                    if on_screen {
                        TraceStatus::Cancelled.into()
                    } else {
                        CompleteInput {
                            failure_reason: Some(TraceFailureReason::VerseNotFound),
                            ..TraceStatus::Failed.into()
                        }
                    },
                );
                continue;
            }
            let ranged = reference
                .verse_end
                .is_some_and(|e| e > reference.verse_start);
            let passage_id = format!(
                "{}-{}-{}-{}-{}-{}",
                reference
                    .book
                    .to_lowercase()
                    .split(kairo_domain::js::is_whitespace)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("-"),
                number_to_string(reference.chapter),
                number_to_string(reference.verse_start),
                number_to_string(reference.verse_end.unwrap_or(reference.verse_start)),
                translation.to_lowercase(),
                now_ms()
            );
            let passage_reference = if ranged {
                format!(
                    "{} {}:{}-{}",
                    reference.book,
                    number_to_string(reference.chapter),
                    number_to_string(reference.verse_start),
                    number_to_string(reference.verse_end.unwrap_or_default())
                )
            } else {
                format!(
                    "{} {}:{}",
                    reference.book,
                    number_to_string(reference.chapter),
                    number_to_string(reference.verse_start)
                )
            };
            let count = verses.len();
            for (index, verse) in verses.iter().enumerate() {
                let Value::Object(fields) = json!({
                    "id": format!("{}-{}", now_ms(), random_suffix()),
                    "reference": format!("{} {}:{}", verse.book, number_to_string(verse.chapter), number_to_string(verse.verse)),
                    "verses": if verse.text.is_empty() { vec![] } else { vec![verse.clone()] },
                    "translation": translation,
                    "requestedTranslation": requested_translation,
                    "confidence": reference.confidence,
                    "source": "auto",
                    "suggestionOnly": reference.suggestion_only.then_some(true),
                    "ambiguous": reference.ambiguous.then_some(true),
                    "provisional": reference.provisional.then_some(true),
                    "quoteEvidence": reference.quote_evidence,
                    "triggerText": reference.source_text,
                    "passageId": passage_id,
                    "passageReference": passage_reference,
                    "passageIndex": index,
                    "passageLength": count,
                    "planId": plan_entry.as_ref().map(|e| e.plan_id.clone()),
                    "planItemId": plan_entry.as_ref().map(|e| e.plan_item_id.clone()),
                    "planMatch": plan_entry.as_ref().map(|_| if reference.detection_type == DetectionType::Quote { "quote" } else { "reference" }),
                    "correlationId": trace,
                }) else {
                    unreachable!()
                };
                let suggestion = crate::suggestion::from_fields(fields);
                state.put_pending(suggestion.clone());
                state.suggestion_refs.insert(
                    suggestion.id.clone(),
                    (reference.book.clone(), reference.chapter, verse.verse),
                );
                if let Some(session) = state.session.as_mut() {
                    session.total_detections += 1.0;
                }
                self.inner
                    .services
                    .scripture
                    .receive_suggestion(suggestion.clone());
                if index == 0 {
                    traces.mark(trace.as_deref(), Stage::SuggestionPublishedAt);
                }
                // Each card moves the follow-up on, so "next verse" continues after it.
                self.note_follow_up(&suggestion);
                if cfg.auto_mode
                    && index == 0
                    && (count == 1 || cfg.auto_present_passages)
                    && can_auto_present(&reference)
                    && self
                        .inner
                        .latest_auto
                        .load(std::sync::atomic::Ordering::SeqCst)
                        <= seq
                    && reference.confidence >= cfg.confidence_threshold
                    && let Some(content) = self.prepare_projection(&suggestion)
                {
                    let delay_sec = if crate::detector::presents_at_once(&reference) {
                        0.0
                    } else {
                        cfg.auto_present_delay_sec
                    };
                    self.schedule_auto(&mut state, delay_sec, suggestion, content);
                }
            }
            state.update_health(ServiceName::Detector, HealthStatus::Ok, None);
        }
        self.emit_status(&state);
    }

    /// Starts a latency trace for a recognised reference. References without
    /// a receipt time (tests, manual entry) are not traced.
    fn start_trace(
        &self,
        reference: &ScriptureReference,
        translation: &str,
        cfg: &OrchestratorConfig,
    ) -> Option<String> {
        let received = reference.stt_received_at?;
        let range = reference
            .verse_end
            .filter(|end| *end > reference.verse_start)
            .map(|end| format!("-{}", number_to_string(end)))
            .unwrap_or_default();
        Some(
            self.inner.services.traces.start(TraceStartInput {
                correlation_id: String::new(),
                source_text: reference.source_text.clone(),
                transcript_source: reference
                    .transcript_source
                    .unwrap_or(TranscriptSource::Final),
                resolver: reference.resolver.unwrap_or(ScriptureResolver::Explicit),
                translation: Some(translation.into()),
                auto_present_delay_ms: Some(if crate::detector::presents_at_once(reference) {
                    0.0
                } else {
                    cfg.auto_present_delay_sec * 1000.0
                }),
                presenter_connection_mode: None,
                reference: Some(format!(
                    "{} {}:{}{range}",
                    reference.book,
                    number_to_string(reference.chapter),
                    number_to_string(reference.verse_start)
                )),
                stt_received_at: received,
                detection_started_at: reference.detection_started_at,
                citation_recognized_at: None,
            }),
        )
    }

    /// The operator's window to cancel before a verse reaches the room. One
    /// countdown at a time: a newer one supersedes the older, even at 0 s.
    fn schedule_auto(
        &self,
        state: &mut State,
        delay_sec: f64,
        suggestion: ScriptureSuggestion,
        content: PushContent,
    ) {
        let delay_ms = delay_sec * 1000.0;
        for (_, timer) in state.auto_timers.drain() {
            timer.abort();
        }
        let weak = Arc::downgrade(&self.inner);
        let id = suggestion.id.clone();
        let fired = suggestion.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs_f64((delay_ms / 1000.0).max(0.0))).await;
            let Some(inner) = weak.upgrade() else { return };
            let orchestrator = Orchestrator { inner };
            let mut state = orchestrator.inner.state.lock().await;
            if state.auto_timers.remove(&fired.id).is_none() {
                return;
            }
            state.take_pending(&fired.id);
            drop(state);
            orchestrator
                .inner
                .services
                .traces
                .mark(fired.correlation_id.as_deref(), Stage::CountdownCompletedAt);
            orchestrator.inner.services.usage.track("scripture_auto");
            orchestrator.present_scripture(fired, Some(content)).await;
        });
        state.auto_timers.insert(id.clone(), timer);
        self.inner.services.traces.mark(
            suggestion.correlation_id.as_deref(),
            Stage::CountdownStartedAt,
        );
        let _ = self.inner.pending_auto.send(PendingAutoPresent {
            suggestion_id: id,
            reference: suggestion.reference.clone(),
            expires_at: now_ms() + delay_ms,
        });
        tracing::info!(
            reference = suggestion.reference,
            delay_sec,
            "auto-present scheduled"
        );
    }

    /// Looks the card's verse up in another translation; presenting it later
    /// uses the new copy.
    pub async fn retranslate_suggestion(
        &self,
        suggestion: ScriptureSuggestion,
        translation: &str,
    ) -> Result<ScriptureSuggestion, String> {
        let results = self
            .inner
            .services
            .scripture
            .search(suggestion.reference.clone(), translation.into())
            .await?;
        let verses: Vec<ScriptureVerse> = results.into_iter().flat_map(|r| r.verses).collect();
        if verses.is_empty() {
            return Err(format!(
                "{translation} isn't available for {}. Download it or add an API.Bible key in Settings → Scripture.",
                suggestion.reference
            ));
        }
        let updated = ScriptureSuggestion {
            requested_translation: None,
            translation: translation.into(),
            verses,
            ..suggestion
        };
        self.inner.state.lock().await.put_pending(updated.clone());
        Ok(updated)
    }

    pub async fn present_scripture_directly(&self, suggestion: ScriptureSuggestion) {
        self.inner.services.usage.track("scripture_manual");
        self.present_scripture(suggestion, None).await;
    }

    /// "Next verse" spoken just after follow-along moved on by itself: the
    /// preacher is asking for what is already showing.
    fn next_already_shown(&self, text: &str) -> bool {
        matches!(
            crate::voice::parse_voice_command(text),
            Some(crate::voice::VoiceCommand::Step(1) | crate::voice::VoiceCommand::Resume)
        ) && self
            .inner
            .scripture_live_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|at| at.elapsed() < NEXT_ALREADY_SHOWN)
    }

    /// Follows the reading of what just went live. A single verse is
    /// followed too, so auto-clear can tell when it has been read.
    fn follow_presented(&self, state: &mut State, suggestion: &ScriptureSuggestion) {
        Self::cancel_auto_clear(state);
        let Some(passage) = suggestion.passage_id.clone() else {
            return self.end_follow(state);
        };
        let index = suggestion.passage_index.unwrap_or(0.0);
        let fresh = || PassageFollowState {
            passage_id: passage.clone(),
            current_index: index,
            matched_token_indexes: Vec::new(),
            read_to: None,
            heard_count: None,
        };
        let mut following = match state.follow.take() {
            Some(f) if f.state.passage_id == passage => f,
            _ => {
                self.prefetch_chapter(state, suggestion);
                Following {
                    state: fresh(),
                    cards: state
                        .pending
                        .iter()
                        .filter(|s| s.passage_id.as_deref() == Some(passage.as_str()))
                        .cloned()
                        .collect(),
                }
            }
        };
        // Follow-along itself presented this card: what was heard since stays.
        if following.state.current_index != index {
            following.state = fresh();
        }
        if !following
            .cards
            .iter()
            .any(|c| c.passage_index.unwrap_or(0.0) == index)
        {
            following.cards.push(suggestion.clone());
        }
        state.follow = Some(following);
        self.publish_follow(state);
    }

    /// Fetches the chapter a reading began in, so it can be followed past
    /// the verses detected and its verses are found without a lookup.
    fn prefetch_chapter(&self, state: &State, suggestion: &ScriptureSuggestion) {
        let Some(first) = suggestion.verses.first() else {
            return;
        };
        let (book, chapter, translation) = (
            first.book.clone(),
            first.chapter,
            suggestion.translation.clone(),
        );
        if state.chapter(&book, chapter, &translation).is_some() {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let scripture = self.inner.services.scripture.clone();
        tokio::spawn(async move {
            match chapter_verses(scripture.as_ref(), &book, chapter, translation.clone()).await {
                Ok(verses) if !verses.is_empty() => {
                    let Some(inner) = weak.upgrade() else { return };
                    let mut state = inner.state.lock().await;
                    state
                        .chapters
                        .retain(|c| !c.is(&book, chapter, &translation));
                    state.chapters.insert(
                        0,
                        ChapterText {
                            book,
                            chapter,
                            translation,
                            verses,
                        },
                    );
                    state.chapters.truncate(CHAPTERS_KEPT);
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(book, chapter, error, "chapter prefetch failed"),
            }
        });
    }

    /// The verse after the passage's last card, as a card of the passage,
    /// when the reader went on into it.
    fn read_on(state: &State, following: &Following, text: &str) -> Option<ScriptureSuggestion> {
        let last = following
            .cards
            .iter()
            .map(|c| c.passage_index.unwrap_or(0.0))
            .fold(f64::NEG_INFINITY, f64::max);
        let card = following
            .cards
            .iter()
            .find(|c| c.passage_index.unwrap_or(0.0) == last)?;
        let verse = card.verses.first()?;
        let chapter = state.chapter(&verse.book, verse.chapter, &card.translation)?;
        let next = chapter
            .verses
            .iter()
            .find(|v| v.verse == verse.verse + 1.0)?;
        if !opening_heard(&next.text, text) {
            return None;
        }
        Some(crate::suggestion::manual(
            format!("follow-{}-{}", following.state.passage_id, last + 1.0),
            crate::voice::reference_text(&next.book, next.chapter, next.verse, next.verse),
            vec![next.clone()],
            card.translation.clone(),
            text.to_owned(),
            Some(following.state.passage_id.clone()),
            Some(last + 1.0),
        ))
    }

    fn end_follow(&self, state: &mut State) {
        Self::cancel_auto_clear(state);
        if state.follow.take().is_some() {
            self.publish_follow(state);
        }
    }

    fn publish_follow(&self, state: &State) {
        let _ = self
            .inner
            .follow_progress
            .send(state.follow.as_ref().map(|f| f.state.clone()));
    }

    fn cancel_auto_clear(state: &mut State) {
        if let Some((_, timer)) = state.auto_clear.take() {
            timer.abort();
        }
    }

    /// A reading moves the screen to the next verse of the passage on
    /// screen (`advanceAutoFollowFromText`). Presenting is spawned, so the
    /// transcript path never waits on a screen.
    async fn advance_follow(&self, text: &str, final_text: bool) {
        if kairo_domain::js::trim(text).is_empty() {
            return;
        }
        let mut state = self.inner.state.lock().await;
        let follow_along = state.cfg.as_ref().is_some_and(|c| c.follow_along);
        let detector = state.detector.clone();
        let Some(following) = state.follow.as_mut() else {
            return;
        };
        let update =
            update_passage_follow_strict(&following.state, text, &following.cards, final_text);
        if final_text {
            tracing::info!(
                verse = following.state.current_index,
                read_to = update.state.read_to,
                heard = update.state.heard_count,
                next = update.next_suggestion_id.as_deref(),
                follow_along,
                "follow-along heard a phrase"
            );
        }
        // An interim only ever moves the screen on; otherwise it changes nothing.
        if !final_text && update.next_suggestion_id.is_none() {
            return;
        }
        let read = following.current().cloned();
        let Some(next) = update.next_suggestion_id else {
            following.state = update.state;
            let through = passage_read_through(&following.state, &following.cards);
            if through && let (Some(detector), Some(verse)) = (&detector, &read) {
                detector.note_read(&verse.book, verse.chapter, verse.verse);
            }
            self.publish_follow(&state);
            if !through {
                return;
            }
            // Reading on past the detected verses (finished speech only).
            let extra = state
                .follow
                .as_ref()
                .filter(|_| final_text && follow_along)
                .and_then(|f| Self::read_on(&state, f, text));
            let Some(card) = extra else {
                self.arm_auto_clear(&mut state);
                return;
            };
            if let Some(following) = state.follow.as_mut() {
                following.cards.push(card.clone());
                following.state = PassageFollowState {
                    current_index: card.passage_index.unwrap_or(0.0),
                    read_to: Some(0.0),
                    heard_count: Some(0.0),
                    ..following.state.clone()
                };
            }
            self.publish_follow(&state);
            drop(state);
            self.note_follow_step();
            let this = self.clone();
            tokio::spawn(async move { this.present_scripture(card, None).await });
            return;
        };
        // Follow-along off: the operator moves the screen, so the reading
        // stays on the verse that is showing.
        if !follow_along {
            return;
        }
        following.state = update.state;
        let card = following.cards.iter().find(|c| c.id == next).cloned();
        if let (Some(detector), Some(verse)) = (&detector, &read) {
            detector.note_read(&verse.book, verse.chapter, verse.verse);
        }
        self.publish_follow(&state);
        let Some(card) = card.filter(|c| c.suggestion_only != Some(true)) else {
            return;
        };
        Self::cancel_auto(&mut state, &card.id);
        state.take_pending(&card.id);
        drop(state);
        self.note_follow_step();
        let this = self.clone();
        tokio::spawn(async move { this.present_scripture(card, None).await });
    }

    /// Once the passage on screen has been read through, scripture clears
    /// after the configured pause, unless something else goes live first.
    fn arm_auto_clear(&self, state: &mut State) {
        let Some(delay) = state
            .cfg
            .as_ref()
            .filter(|c| c.auto_clear)
            .map(|c| c.auto_clear_delay_sec)
        else {
            return;
        };
        let Some(following) = &state.follow else {
            return;
        };
        let passage = following.state.passage_id.clone();
        let index = following.state.current_index;
        if state
            .auto_clear
            .as_ref()
            .is_some_and(|(armed, _)| *armed == passage)
        {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let armed = passage.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs_f64(delay.max(0.0))).await;
            let Some(inner) = weak.upgrade() else { return };
            let this = Orchestrator { inner };
            let verse = {
                let mut state = this.inner.state.lock().await;
                // Taken first: clearing ends the follow, which cancels this timer.
                if state.auto_clear.take().is_none() {
                    return;
                }
                let Some(verse) = state
                    .follow
                    .as_ref()
                    .filter(|f| f.state.passage_id == armed && f.state.current_index == index)
                    .and_then(Following::current)
                    .cloned()
                else {
                    return;
                };
                verse
            };
            this.clear_scripture_if_live(&verse.book, verse.chapter, verse.verse)
                .await;
        });
        state.auto_clear = Some((passage, timer));
    }

    /// Follow-along just moved the screen on by itself.
    pub fn note_follow_step(&self) {
        *self
            .inner
            .scripture_live_at
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
    }

    async fn present_scripture(
        &self,
        suggestion: ScriptureSuggestion,
        prepared: Option<PushContent>,
    ) {
        let _present = self.inner.present.lock().await;
        let suggestion = single_verse_suggestion(&suggestion);
        let content = prepared.or_else(|| self.prepare_projection(&suggestion));
        let traces = &self.inner.services.traces;
        let trace = suggestion.correlation_id.as_deref();
        traces.mark(trace, Stage::PresenterRequestStartedAt);
        let results = match &content {
            Some(content) => {
                self.dispatch_content(content, OverlayContentKind::Scripture)
                    .await
            }
            None => Vec::new(),
        };
        traces.mark(trace, Stage::PresenterRequestCompletedAt);
        // Accepted by a screen, not yet seen drawn: frame commit is not
        // correlated back across the engine process.
        traces.complete(
            trace,
            if results.iter().any(|r| r.ok) {
                CompleteInput {
                    confirmation: Some(ConfirmationLevel::RequestAccepted),
                    ..TraceStatus::Presented.into()
                }
            } else {
                CompleteInput {
                    failure_reason: Some(TraceFailureReason::PresenterRejected),
                    ..TraceStatus::Failed.into()
                }
            },
        );
        let mut state = self.inner.state.lock().await;
        let state = &mut *state;
        self.record_dispatch_health(state, &results);
        // Live in the program whether or not a screen took it.
        if !results.iter().any(|r| r.ok) {
            tracing::warn!(
                reference = suggestion.reference,
                "scripture is live but no screen took it"
            );
        }
        self.inner
            .services
            .live_plan
            .observe(&suggestion.reference, suggestion.plan_item_id.as_deref());
        if let Some(session) = state.session.as_mut() {
            session.total_presentations += 1.0;
        }
        // Reading this passage aloud from here on is following along.
        if let Some(detector) = &state.detector {
            let passage = reading_passage_of(&suggestion).map(|mut passage| {
                // Every card of this passage, so the whole reading is known.
                let mut verses: Vec<&ScriptureVerse> = state
                    .pending
                    .iter()
                    .filter(|s| s.passage_id.is_some() && s.passage_id == suggestion.passage_id)
                    .flat_map(|s| s.verses.iter())
                    .chain(suggestion.verses.iter())
                    .collect();
                verses.sort_by(|a, b| a.verse.total_cmp(&b.verse));
                verses.dedup_by(|a, b| a.verse == b.verse);
                passage.text = verses
                    .iter()
                    .map(|v| v.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                passage
            });
            detector.set_reading_passage(passage);
            if let Some(first) = suggestion.verses.first() {
                detector.note_shown(&first.book, first.chapter, first.verse);
            }
        }
        self.follow_presented(state, &suggestion);
        if let (Some(content), Some(first), Some(last)) = (
            &content,
            suggestion.verses.first(),
            suggestion.verses.last(),
        ) {
            self.inner.services.program.set_scripture_slide(
                content.reference.clone(),
                content.text.clone(),
                ScriptureSlideMeta {
                    book: first.book.clone(),
                    chapter: first.chapter,
                    verse_start: first.verse,
                    verse_end: last.verse,
                    translation: suggestion.translation.clone(),
                    passage_id: suggestion
                        .passage_id
                        .clone()
                        .unwrap_or_else(|| suggestion.id.clone()),
                    passage_index: suggestion.passage_index.unwrap_or(0.0),
                },
            );
        }
        self.emit_status(state);
    }

    pub async fn present_lyric_slide(
        &self,
        reference: &str,
        text: &str,
        colored: Option<Vec<ColoredLine>>,
        next: Option<ProgramSlideInfo>,
    ) -> Result<(), String> {
        if kairo_domain::js::trim(text).is_empty() {
            return Err("Slide is empty — nothing to push.".into());
        }
        let _present = self.inner.present.lock().await;
        let content = PushContent {
            reference: reference.into(),
            text: text.into(),
            colored,
        };
        let results = self
            .dispatch_content(&content, OverlayContentKind::Lyrics)
            .await;
        let mut state = self.inner.state.lock().await;
        self.record_dispatch_health(&mut state, &results);
        // Lyrics replaced the verse: reading on must not bring it back.
        self.end_follow(&mut state);
        if !results.iter().any(|r| r.ok) {
            tracing::warn!(reference, "lyric slide is live but no screen took it");
        }
        if let Some(session) = state.session.as_mut() {
            session.total_presentations += 1.0;
        }
        self.inner
            .services
            .program
            .set_slide(reference.into(), text.into(), next);
        self.emit_status(&state);
        Ok(())
    }

    /// Everything that can be prepared before the safety delay.
    fn prepare_projection(&self, suggestion: &ScriptureSuggestion) -> Option<PushContent> {
        let suggestion = single_verse_suggestion(suggestion);
        let overlay = self.inner.services.settings.overlay();
        let text = format_overlay_verse_text(
            &suggestion.verses,
            overlay.show_verse_numbers,
            Some(overlay.max_verses),
        )?;
        if text.is_empty() {
            return None;
        }
        Some(PushContent {
            // A playlist verse may carry its own reference line.
            reference: kairo_domain::scripture::query::verse_reference_line(
                suggestion.verses.first(),
                &suggestion.reference,
                &suggestion.translation,
                overlay.show_translation,
            ),
            text,
            colored: None,
        })
    }

    async fn dispatch_content(
        &self,
        content: &PushContent,
        kind: OverlayContentKind,
    ) -> Vec<OverlayDispatchResult> {
        let outputs = self.inner.services.settings.overlay().outputs;
        dispatch_to_outputs(&outputs, kind, |output| {
            let content = content.clone();
            Box::pin(async move { Ok(self.push_rendered(output, kind, content).await) })
        })
        .await
    }

    /// The dock's live background under `theme`, per `themeForPush`.
    fn with_live_background(
        &self,
        theme: OverlayTheme,
        force: bool,
        shows_backgrounds: bool,
    ) -> OverlayTheme {
        let media = &self.inner.services.media;
        let live = media.live_item();
        let playback = media.live_playback().unwrap_or_default();
        theme_for_push(
            theme,
            shows_backgrounds,
            live.as_ref().map(|i| (i.path.as_str(), i.kind)),
            &playback,
            Some(media.live_cue()),
            force,
        )
    }

    async fn push_rendered(
        &self,
        output: &OverlayOutput,
        kind: OverlayContentKind,
        content: PushContent,
    ) -> Option<String> {
        let engine = &self.inner.services.engine;
        // A missing sink still gets the slide when the engine composes the
        // output, so the monitor previews it; the push counts as not taken.
        let missing = engine.unavailable_reason(output);
        if missing.is_some() && !engine.composes(output) {
            return missing;
        }
        let theme = self.with_live_background(
            output_theme_for(output, kind),
            kind == OverlayContentKind::Lyrics,
            output.show.backgrounds,
        );
        let started = std::time::Instant::now();
        let shown = engine
            .show_slide(
                output.id.clone(),
                content.reference.clone(),
                content.text,
                theme,
                content.colored,
            )
            .await;
        let ms = started.elapsed().as_millis();
        tracing::info!(
            output = output.name,
            ?kind,
            reference = content.reference,
            shown,
            ms,
            "pushed"
        );
        if ms > 750 {
            tracing::warn!(output = output.name, ms, "slow push");
        }
        if let Some(reason) = missing {
            return Some(reason);
        }
        if shown {
            return None;
        }
        Some(
            match output.kind {
                kairo_domain::overlay::OverlayOutputKind::Screen => {
                    "Screen window is not available"
                }
                kairo_domain::overlay::OverlayOutputKind::Device => {
                    "Video card surface is not available"
                }
                _ => "NDI sender unavailable",
            }
            .into(),
        )
    }

    fn record_dispatch_health(&self, state: &mut State, results: &[OverlayDispatchResult]) {
        let line = dispatch_health(results).output;
        state.update_health(ServiceName::Output, line.status, line.last_error);
    }

    /// A document page on every output that shows documents.
    pub async fn present_document_page(
        &self,
        media_path: &str,
        slide: Option<DocumentSlide>,
    ) -> bool {
        let _present = self.inner.present.lock().await;
        let engine = &self.inner.services.engine;
        let targets = engine.live_targets(Some(ShowKind::Documents));
        if targets.is_empty() {
            return false;
        }
        let mut any = false;
        for output in &targets {
            // Reuse the current document surface for responsive page turns.
            if engine
                .swap_document_page(output.id.clone(), media_path.into(), slide.clone())
                .await
            {
                any = true;
                continue;
            }
            let mut theme = output_theme_for(output, OverlayContentKind::Scripture);
            theme.background.kind = OverlayBackgroundType::Image;
            theme.background.media_path = Some(media_path.into());
            theme.background.media_fit = Some(MediaFit::Contain);
            theme.background.hue = Some(0.0);
            theme.background.opacity = 1.0;
            theme.background.brightness = Some(1.0);
            theme.background.contrast = Some(1.0);
            theme.background.saturation = Some(1.0);
            any |= engine
                .show_document(output.id.clone(), theme, slide.clone())
                .await;
        }
        if !any {
            return false;
        }
        self.inner.services.media.clear_live_item();
        true
    }

    pub async fn present_live_background(&self) -> bool {
        let _present = self.inner.present.lock().await;
        if self.inner.services.media.live_item().is_none() {
            return false;
        }
        let targets = self
            .inner
            .services
            .engine
            .live_targets(Some(ShowKind::Backgrounds));
        if targets.is_empty() {
            return false;
        }
        let mut any = false;
        for output in &targets {
            any |= self.show_live_background_on(output, true).await;
        }
        any
    }

    async fn show_live_background_on(&self, output: &OverlayOutput, swap: bool) -> bool {
        let engine = &self.inner.services.engine;
        let theme = self.with_live_background(
            output_theme_for(output, OverlayContentKind::Scripture),
            true,
            true,
        );
        let swapped = swap
            && engine
                .set_background(output.id.clone(), theme.background.clone())
                .await;
        swapped
            || engine
                .show_slide(output.id.clone(), String::new(), String::new(), theme, None)
                .await
    }

    /// The Theme page's "Send test verse": John 3:16 (KJV) through the real routing.
    pub async fn test_overlay(&self) -> bool {
        let sample = crate::suggestion::manual(
            "overlay-test".into(),
            "John 3:16".into(),
            vec![serde_json::from_value(json!({
                "book": "John", "chapter": 3, "verse": 16,
                "text": "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life."
            }))
            .expect("sample verse")],
            "KJV".into(),
            String::new(),
            None,
            None,
        );
        let Some(content) = self.prepare_projection(&sample) else {
            return false;
        };
        let _present = self.inner.present.lock().await;
        self.dispatch_content(&content, OverlayContentKind::Scripture)
            .await
            .iter()
            .any(|r| r.ok)
    }

    /// Clears the text; where nothing was painted, the dock background goes up
    /// instead, or the surface is blanked.
    pub async fn clear_text(&self) -> bool {
        let _present = self.inner.present.lock().await;
        self.inner.services.program.clear_current();
        {
            let mut state = self.inner.state.lock().await;
            self.end_follow(&mut state);
            if let Some(detector) = &state.detector {
                detector.set_reading_passage(None);
            }
        }
        let engine = &self.inner.services.engine;
        let surfaces = engine.all_surfaces();
        if surfaces.is_empty() {
            return true;
        }
        let targets = engine.live_targets(Some(ShowKind::Backgrounds));
        let live = self.inner.services.media.live_item().is_some();
        for surface in surfaces {
            let target = targets.iter().find(|t| t.id == surface);
            let background = target.filter(|_| live).map(|t| {
                self.with_live_background(
                    output_theme_for(t, OverlayContentKind::Scripture),
                    true,
                    true,
                )
                .background
            });
            if engine.clear_text(surface.clone(), background).await {
                continue;
            }
            match target.filter(|_| live) {
                Some(target) => {
                    self.show_live_background_on(target, false).await;
                }
                None => engine.clear(surface).await,
            }
        }
        true
    }

    /// Blank every screen and feed and reset the live program.
    pub async fn clear_overlay(&self) -> bool {
        let _present = self.inner.present.lock().await;
        for surface in self.inner.services.engine.all_surfaces() {
            self.inner.services.engine.clear(surface).await;
        }
        self.inner.services.media.clear_live_item();
        self.inner.services.program.clear_program();
        // Blanked: reading on must not bring the verse back.
        self.end_follow(&mut *self.inner.state.lock().await);
        true
    }

    /// The `session-recovery.json` snapshot while running.
    pub async fn recovery_snapshot(&self) -> Option<Value> {
        let state = self.inner.state.lock().await;
        if !state.running {
            return None;
        }
        let session = state.session.as_ref()?;
        Some(json!({
            "session": session,
            "pendingSuggestions": state.pending,
            "transcriptHistory": self.inner.services.stt.history(),
            "config": state.cfg,
        }))
    }

    /// `restoreSession`: needs an active service. Each part of the blob is
    /// validated on its own; an unusable part is skipped.
    pub async fn restore_session(&self, snapshot: &Value) -> Result<(), String> {
        if snapshot.is_null() {
            return Ok(());
        }
        if !self.inner.services.gate.has_active_service() {
            return Err(NO_ACTIVE_SERVICE.into());
        }
        let mut state = self.inner.state.lock().await;
        if state.running {
            self.stop_locked(&mut state);
        }
        state.cfg = snapshot
            .get("config")
            .and_then(|c| serde_json::from_value(c.clone()).ok());
        state.session = snapshot
            .get("session")
            .and_then(|s| serde_json::from_value(s.clone()).ok());
        state.pending.clear();
        if let Some(list) = snapshot.get("pendingSuggestions").and_then(Value::as_array) {
            for raw in list {
                if let Ok(suggestion) = serde_json::from_value::<ScriptureSuggestion>(raw.clone()) {
                    self.inner
                        .services
                        .scripture
                        .receive_suggestion(suggestion.clone());
                    state.put_pending(suggestion);
                }
            }
        }
        if let Some(list) = snapshot.get("transcriptHistory").and_then(Value::as_array) {
            let history = list
                .iter()
                .filter_map(|r| serde_json::from_value(r.clone()).ok())
                .collect();
            self.inner.services.stt.set_history(history);
        }
        state.running = true;
        self.quick().running = true;
        if let Some(cfg) = state.cfg.clone() {
            self.quick().auto_mode = cfg.auto_mode;
            state.update_health(ServiceName::Detector, HealthStatus::Ok, None);
            // Same wiring as a cold start, key or no key.
            self.create_detector(&mut state, &cfg);
            self.configure_stt(&cfg);
            state.start_generation += 1;
            let generation = state.start_generation;
            drop(state);
            let started = self.inner.services.stt.start().await;
            state = self.inner.state.lock().await;
            if state.start_generation != generation {
                // Stopped (or started afresh) while STT connected.
                if !state.starting && started.is_ok() {
                    self.inner.services.stt.stop();
                }
                return Ok(());
            }
            Self::apply_stt_start(&mut state, started);
            self.attach_audio(&mut state);
        }
        self.emit_status(&state);
        tracing::info!(session = ?state.session.as_ref().map(|s| &s.session_id), "session recovery successful");
        Ok(())
    }

    /// Ids of the cards still pending, oldest first.
    pub async fn pending_ids(&self) -> Vec<String> {
        self.inner
            .state
            .lock()
            .await
            .pending
            .iter()
            .map(|s| s.id.clone())
            .collect()
    }

    /// The detector the session runs, for resilience's fallback switch.
    pub async fn detector(&self) -> Option<Arc<dyn Detector>> {
        self.inner.state.lock().await.detector.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_passage_spans_the_detected_range() {
        let verse = |n: f64| -> ScriptureVerse {
            serde_json::from_value(json!({ "book": "John", "chapter": 1, "verse": n, "text": "t" }))
                .unwrap()
        };
        let mut s = crate::suggestion::manual(
            "x".into(),
            "John 1:3".into(),
            vec![verse(3.0)],
            "KJV".into(),
            String::new(),
            None,
            None,
        );
        s.passage_reference = Some("John 1:1-10".into());
        assert_eq!(
            reading_passage_of(&s).map(|p| (p.start, p.end)),
            Some((1.0, 10.0))
        );
        s.passage_reference = Some("John 2:1".into());
        assert_eq!(
            reading_passage_of(&s).map(|p| (p.start, p.end)),
            Some((3.0, 3.0))
        );
        s.verses.clear();
        assert_eq!(reading_passage_of(&s), None);
        assert_eq!(
            parse_passage_range("1 John 3:16–18"),
            Some((3.0, 16.0, 18.0))
        );
        assert_eq!(parse_passage_range("John 3"), None);
    }

    #[test]
    fn cost_per_call_matches_electron() {
        assert!((COST_PER_CALL_USD - 0.00132).abs() < 1e-12);
    }
}
