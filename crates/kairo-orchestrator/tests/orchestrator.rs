//! The orchestrator end to end on a virtual clock: transcripts into a fake
//! STT, the real local detector, and a fake engine recording every call.

use kairo_domain::documents::DocumentSlide;
use kairo_domain::media::{MediaItem, MediaPlayback};
use kairo_domain::overlay::{OverlayBackground, OverlayOutput, OverlayOutputKind, OverlayTheme};
use kairo_domain::overlay_defaults::{make_overlay_output, normalize_overlay_settings};
use kairo_domain::program::ProgramSlideInfo;
use kairo_domain::scripture::{ScriptureResult, ScriptureSuggestion};
use kairo_domain::settings::OverlaySettings;
use kairo_domain::status::{OrchestratorConfig, SttProvider};
use kairo_domain::transcript::TranscriptResult;
use kairo_orchestrator::clients::*;
use kairo_orchestrator::detector::{Detector, LiveDetector};
use kairo_orchestrator::live_plan::{LivePlanService, PlanSource};
use kairo_orchestrator::orchestrator::{MANAGED, NO_ACTIVE_SERVICE, Orchestrator, Services};
use kairo_orchestrator::voice::{BoxFuture, LiveScriptureContext};
use kairo_scripture::model_detector::ModelConfig;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

// ─── Fakes ──────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Log(Mutex<Vec<String>>);

impl Log {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

type Calls = Arc<Mutex<Vec<Vec<String>>>>;

fn record(calls: &Calls, call: &[&str]) {
    calls
        .lock()
        .unwrap()
        .push(call.iter().map(|s| s.to_string()).collect());
}

struct Engine {
    calls: Calls,
    log: Arc<Log>,
    outputs: Vec<OverlayOutput>,
    shown_at: Arc<Mutex<Vec<tokio::time::Instant>>>,
}

impl EngineClient for Engine {
    fn unavailable_reason(&self, _o: &OverlayOutput) -> Option<String> {
        None
    }
    fn show_slide(
        &self,
        id: String,
        reference: String,
        text: String,
        _t: OverlayTheme,
        _c: Option<Vec<ColoredLine>>,
    ) -> BoxFuture<'_, bool> {
        record(&self.calls, &["ShowSlide", &id, &reference, &text]);
        self.log
            .push(format!("ShowSlide {id} {reference} | {text}"));
        self.shown_at
            .lock()
            .unwrap()
            .push(tokio::time::Instant::now());
        Box::pin(async { true })
    }
    fn set_background(&self, id: String, _b: OverlayBackground) -> BoxFuture<'_, bool> {
        self.log.push(format!("SetBackground {id}"));
        Box::pin(async { true })
    }
    fn swap_document_page(
        &self,
        id: String,
        path: String,
        _s: Option<DocumentSlide>,
    ) -> BoxFuture<'_, bool> {
        self.log.push(format!("SwapDocumentPage {id} {path}"));
        Box::pin(async { false })
    }
    fn show_document(
        &self,
        id: String,
        theme: OverlayTheme,
        _s: Option<DocumentSlide>,
    ) -> BoxFuture<'_, bool> {
        self.log.push(format!(
            "ShowDocument {id} {}",
            theme.background.media_path.unwrap_or_default()
        ));
        Box::pin(async { true })
    }
    fn clear_text(&self, id: String, _b: Option<OverlayBackground>) -> BoxFuture<'_, bool> {
        self.log.push(format!("ClearText {id}"));
        Box::pin(async { false })
    }
    fn clear(&self, id: String) -> BoxFuture<'_, ()> {
        self.log.push(format!("Clear {id}"));
        Box::pin(async {})
    }
    fn live_targets(&self, _s: Option<ShowKind>) -> Vec<OverlayOutput> {
        self.outputs.clone()
    }
    fn all_surfaces(&self) -> Vec<String> {
        self.outputs.iter().map(|o| o.id.clone()).collect()
    }
}

#[derive(Default)]
struct Program {
    calls: Calls,
    log: Arc<Log>,
    live: Mutex<Option<(LiveScriptureContext, f64)>>,
    revision: Mutex<i64>,
}

impl ProgramClient for Program {
    fn scripture(&self) -> Option<(LiveScriptureContext, f64)> {
        self.live.lock().unwrap().clone()
    }
    fn set_scripture_slide(&self, reference: String, _t: String, m: ScriptureSlideMeta) {
        record(&self.calls, &["Program scripture", &reference]);
        self.log.push(format!("Program scripture {reference}"));
        let mut revision = self.revision.lock().unwrap();
        *revision += 1;
        *self.live.lock().unwrap() = Some((
            LiveScriptureContext {
                book: m.book,
                chapter: m.chapter,
                verse_start: m.verse_start,
                verse_end: m.verse_end,
                translation: m.translation,
                revision: *revision,
                passage_id: Some(m.passage_id),
                passage_index: Some(m.passage_index),
            },
            1e15,
        ));
    }
    fn set_slide(&self, reference: String, _t: String, next: Option<ProgramSlideInfo>) {
        self.log.push(format!(
            "Program slide {reference} next={}",
            next.map(|n| n.reference).unwrap_or_default()
        ));
    }
    fn clear_current(&self) {
        self.log.push("Program clearCurrent".into());
        *self.live.lock().unwrap() = None;
    }
    fn clear_program(&self) {
        self.log.push("Program clear".into());
        *self.live.lock().unwrap() = None;
    }
}

#[derive(Default)]
struct Media {
    live: Mutex<Option<MediaItem>>,
}

impl MediaClient for Media {
    fn live_item(&self) -> Option<MediaItem> {
        self.live.lock().unwrap().clone()
    }
    fn live_playback(&self) -> Option<MediaPlayback> {
        None
    }
    fn live_cue(&self) -> f64 {
        0.0
    }
    fn clear_live_item(&self) {
        *self.live.lock().unwrap() = None;
    }
}

#[derive(Default)]
struct Scripture {
    cards: Mutex<Vec<ScriptureSuggestion>>,
    dismissed: Mutex<Vec<String>>,
    /// When set, searches wait for it, like a slow network.
    gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
}

impl ScriptureClient for Scripture {
    fn search(
        &self,
        query: String,
        translation: String,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, String>> {
        // "Book C:V[-E]" or "Book C": verses 1–30 of any chapter exist.
        let (book, rest) = query.rsplit_once(' ').unwrap();
        let (chapter, verses) = match rest.split_once(':') {
            Some((c, v)) => (c.to_string(), Some(v.to_string())),
            None => (rest.to_string(), None),
        };
        let chapter: f64 = chapter.parse().unwrap();
        let range: Vec<f64> = match verses {
            None => (1..=30).map(f64::from).collect(),
            Some(v) => {
                let (s, e) = v.split_once('-').unwrap_or((&v, &v));
                (s.parse::<u32>().unwrap()..=e.parse::<u32>().unwrap())
                    .map(f64::from)
                    .collect()
            }
        };
        let verses = range
            .into_iter()
            .map(|n| serde_json::from_value(json!({ "book": book, "chapter": chapter, "verse": n, "text": format!("{book} {chapter}:{n} text") })).unwrap())
            .collect();
        let result = serde_json::from_value(
            json!({ "reference": query, "verses": [], "translation": translation }),
        )
        .map(|mut r: ScriptureResult| {
            r.verses = verses;
            r
        });
        let gate = self.gate.lock().unwrap().clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            Ok(vec![result.unwrap()])
        })
    }
    fn receive_suggestion(&self, s: ScriptureSuggestion) {
        self.cards.lock().unwrap().push(s);
    }
    fn pending_suggestion(&self, _id: &str) -> Option<ScriptureSuggestion> {
        None
    }
    fn dismiss_suggestion(&self, id: &str) {
        self.dismissed.lock().unwrap().push(id.into());
    }
    fn request_row_translation(&self, _p: &str, _t: &str) {}
}

struct Stt {
    log: Arc<Log>,
    history: Mutex<Vec<TranscriptResult>>,
    /// When set, `start` waits for it, like a stalled socket connect.
    gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
}

impl SttClient for Stt {
    fn configure(&self, provider: SttProvider, language: &str, keyterms: Vec<String>) {
        self.log.push(format!(
            "Stt configure {provider:?} {language} {}",
            keyterms.len()
        ));
    }
    fn start(&self) -> BoxFuture<'_, Result<(), String>> {
        self.log.push("Stt start".into());
        let gate = self.gate.lock().unwrap().clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            Ok(())
        })
    }
    fn stop(&self) {
        self.log.push("Stt stop".into());
    }
    fn history(&self) -> Vec<TranscriptResult> {
        self.history.lock().unwrap().clone()
    }
    fn set_history(&self, h: Vec<TranscriptResult>) {
        *self.history.lock().unwrap() = h;
    }
    fn attach_audio(&self) -> Result<(), String> {
        self.log.push("Stt attach".into());
        Ok(())
    }
    fn detach_audio(&self) {
        self.log.push("Stt detach".into());
    }
}

struct Gate {
    service: bool,
}

impl SessionGate for Gate {
    fn assert_transcription(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
    fn has_active_service(&self) -> bool {
        self.service
    }
}

struct Settings(OverlaySettings);

impl SettingsClient for Settings {
    fn overlay(&self) -> OverlaySettings {
        self.0.clone()
    }
}

#[derive(Default)]
struct Usage(Mutex<Vec<String>>);

impl UsageClient for Usage {
    fn track(&self, event: &str) {
        self.0.lock().unwrap().push(event.into());
    }
}

struct NoPlans;

impl PlanSource for NoPlans {
    fn get(&self, _id: &str) -> Option<kairo_domain::scripture::SermonPlan> {
        None
    }
    fn live_plan_id(&self) -> Option<String> {
        None
    }
    fn set_live_plan_id(&mut self, _id: Option<&str>) {}
    fn list(&self) -> Vec<kairo_domain::scripture::SermonPlan> {
        Vec::new()
    }
    fn save(
        &mut self,
        plan: kairo_domain::scripture::SermonPlan,
        _now_ms: f64,
    ) -> kairo_domain::scripture::SermonPlan {
        plan
    }
    fn delete(&mut self, _id: &str) {}
}

struct Harness {
    calls: Calls,
    orchestrator: Orchestrator,
    engine_log: Arc<Log>,
    program_log: Arc<Log>,
    stt_log: Arc<Log>,
    scripture: Arc<Scripture>,
    stt: Arc<Stt>,
    usage: Arc<Usage>,
    transcripts: broadcast::Sender<TranscriptEvent>,
    shown_at: Arc<Mutex<Vec<tokio::time::Instant>>>,
}

fn harness(service: bool) -> Harness {
    let calls: Calls = Arc::default();
    let engine_log = Arc::new(Log::default());
    let program_log = Arc::new(Log::default());
    let stt_log = Arc::new(Log::default());
    let mut screen = make_overlay_output("screen", OverlayOutputKind::Screen);
    screen.enabled = true;
    let mut overlay = normalize_overlay_settings(None);
    overlay.outputs = vec![screen.clone()];
    overlay.show_translation = true;
    let scripture = Arc::new(Scripture::default());
    let stt = Arc::new(Stt {
        log: stt_log.clone(),
        history: Mutex::default(),
        gate: Mutex::default(),
    });
    let usage = Arc::new(Usage::default());
    let shown_at = Arc::new(Mutex::new(Vec::new()));
    let (transcripts, _) = broadcast::channel(64);
    let tx = transcripts.clone();
    let live_plan = LivePlanService::new(Box::new(NoPlans));
    let plan_for_detector = live_plan.clone();
    let services = Services {
        engine: Arc::new(Engine {
            calls: calls.clone(),
            log: engine_log.clone(),
            outputs: vec![screen],
            shown_at: shown_at.clone(),
        }),
        program: Arc::new(Program {
            calls: calls.clone(),
            log: program_log.clone(),
            ..Program::default()
        }),
        media: Arc::new(Media::default()),
        scripture: scripture.clone(),
        stt: stt.clone(),
        transcripts: Arc::new(move || tx.subscribe()),
        gate: Arc::new(Gate { service }),
        settings: Arc::new(Settings(overlay)),
        usage: usage.clone(),
        live_plan,
        detector: Arc::new(move |_cfg| {
            let plan = plan_for_detector.clone();
            let d: Arc<dyn Detector> = LiveDetector::new(
                None,
                ModelConfig::default(),
                None,
                Arc::new(move || plan.index()),
            );
            d
        }),
        resilience: None,
    };
    Harness {
        calls,
        orchestrator: Orchestrator::new(services),
        engine_log,
        program_log,
        stt_log,
        scripture,
        stt,
        usage,
        transcripts,
        shown_at,
    }
}

fn config(auto: bool) -> OrchestratorConfig {
    serde_json::from_value(json!({
        "audioDeviceId": "default", "sttProvider": "none", "sttApiKey": "user-key",
        "sttLanguage": "en-US", "llmProvider": "deepseek", "llmApiKey": "user-llm",
        "scriptureTranslation": "KJV", "autoMode": auto, "confidenceThreshold": 0.7,
        "autoPresentDelaySec": 1,
    }))
    .unwrap()
}

impl Harness {
    async fn say(&self, text: &str) {
        let id = format!("final-{text}");
        let result: TranscriptResult = serde_json::from_value(json!({
            "id": id, "text": text, "words": [], "timestamp": 0, "duration": 1, "isFinal": true
        }))
        .unwrap();
        self.stt.history.lock().unwrap().push(result.clone());
        self.transcripts
            .send(TranscriptEvent::Final(result))
            .unwrap();
        settle().await;
    }

    fn cards(&self) -> Vec<String> {
        self.scripture
            .cards
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.reference.clone())
            .collect()
    }
}

async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(1)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[tokio::test(start_paused = true)]
async fn starting_needs_an_active_service_and_ignores_caller_credentials() {
    let h = harness(false);
    assert_eq!(
        h.orchestrator
            .start_from_ui(config(false))
            .await
            .unwrap_err(),
        NO_ACTIVE_SERVICE
    );
    assert!(h.stt_log.take().is_empty());

    let h = harness(true);
    h.orchestrator.start_from_ui(config(false)).await.unwrap();
    assert_eq!(
        h.stt_log.take(),
        ["Stt configure Deepgram en-US 31", "Stt start", "Stt attach"]
    );
    let snapshot = h.orchestrator.recovery_snapshot().await.unwrap();
    assert_eq!(snapshot["config"]["sttApiKey"], MANAGED);
    assert_eq!(snapshot["config"]["llmApiKey"], MANAGED);
    assert_eq!(snapshot["config"]["sttProvider"], "deepgram");
    let keys: Vec<_> = snapshot.as_object().unwrap().keys().cloned().collect();
    assert_eq!(
        keys,
        [
            "session",
            "pendingSuggestions",
            "transcriptHistory",
            "config"
        ]
    );
    assert!(h.orchestrator.status().await.running);
}

#[tokio::test(start_paused = true)]
async fn a_stalled_stt_start_does_not_block_status_or_stop() {
    let h = harness(true);
    let gate = Arc::new(tokio::sync::Notify::new());
    *h.stt.gate.lock().unwrap() = Some(gate.clone());
    let orchestrator = h.orchestrator.clone();
    let start = tokio::spawn(async move { orchestrator.start_from_ui(config(false)).await });
    settle().await;
    let within = Duration::from_secs(1);
    let status = tokio::time::timeout(within, h.orchestrator.status()).await;
    assert!(!status.expect("status waits on STT").running);
    tokio::time::timeout(within, h.orchestrator.stop())
        .await
        .expect("stop waits on STT");

    // The connect finishing late must not revive the stopped session.
    gate.notify_one();
    start.await.unwrap().unwrap();
    assert!(!h.orchestrator.status().await.running);
    assert_eq!(
        h.stt_log.take(),
        ["Stt configure Deepgram en-US 31", "Stt start", "Stt stop"]
    );
}

#[tokio::test(start_paused = true)]
async fn a_lookup_finishing_after_stop_adds_no_card() {
    let h = harness(true);
    h.orchestrator.start_from_ui(config(false)).await.unwrap();
    let gate = Arc::new(tokio::sync::Notify::new());
    *h.scripture.gate.lock().unwrap() = Some(gate.clone());
    h.say("Turn with me to John 3:16").await;
    tokio::time::timeout(Duration::from_secs(1), h.orchestrator.stop())
        .await
        .expect("stop waits on the lookup");
    gate.notify_waiters();
    settle().await;
    assert!(h.cards().is_empty());
}

#[tokio::test(start_paused = true)]
async fn cards_for_several_translations_arrive_in_request_order() {
    let h = harness(true);
    h.orchestrator.start_from_ui(config(false)).await.unwrap();
    h.say("John 3:16 in the ESV and the NIV").await;
    let translations: Vec<String> = h
        .scripture
        .cards
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.translation.clone())
        .collect();
    assert_eq!(translations, ["ESV", "NIV"]);
}

#[tokio::test(start_paused = true)]
async fn an_auto_detection_presents_after_the_operator_delay() {
    let h = harness(true);
    let mut pending = h.orchestrator.subscribe_pending_auto();
    h.orchestrator.start_from_ui(config(true)).await.unwrap();
    h.say("Turn with me to John 3:16").await;
    assert_eq!(h.cards(), ["John 3:16"]);
    let auto = pending.try_recv().unwrap();
    assert_eq!(auto.reference, "John 3:16");
    assert!(h.engine_log.take().is_empty(), "nothing before the delay");
    tokio::time::sleep(Duration::from_millis(1001)).await;
    settle().await;
    assert_eq!(
        h.engine_log.take(),
        ["ShowSlide screen John 3:16 (KJV) | John 3:16 text"]
    );
    assert_eq!(h.program_log.take(), ["Program scripture John 3:16 (KJV)"]);
    assert_eq!(*h.usage.0.lock().unwrap(), ["scripture_auto"]);
    let stats = h.orchestrator.stats().await.unwrap();
    assert_eq!(
        (
            stats.total_detections,
            stats.total_presentations,
            stats.total_words
        ),
        (1.0, 1.0, 6.0)
    );
}

#[tokio::test(start_paused = true)]
async fn a_newer_citation_supersedes_the_running_countdown() {
    let h = harness(true);
    h.orchestrator.start_from_ui(config(true)).await.unwrap();
    h.say("John 3:16").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    h.say("Romans 8:28").await;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    settle().await;
    assert_eq!(
        h.engine_log.take(),
        ["ShowSlide screen Romans 8:28 (KJV) | Romans 8:28 text"]
    );
}

#[tokio::test(start_paused = true)]
async fn dismissing_a_card_lets_the_same_verse_come_back() {
    let h = harness(true);
    h.orchestrator.start_from_ui(config(false)).await.unwrap();
    h.say("John 3:16").await;
    h.say("John 3:16").await;
    assert_eq!(h.cards(), ["John 3:16"], "a repeat is deduplicated");
    let id = h.scripture.cards.lock().unwrap()[0].id.clone();
    h.orchestrator.dismiss_suggestion(&id).await;
    assert_eq!(*h.scripture.dismissed.lock().unwrap(), [id]);
    h.say("John 3:16").await;
    assert_eq!(h.cards(), ["John 3:16", "John 3:16"]);
    assert!(
        h.engine_log.take().is_empty(),
        "manual mode never presents by itself"
    );
}

#[tokio::test(start_paused = true)]
async fn approving_presents_and_next_verse_steps_on() {
    let h = harness(true);
    h.orchestrator.start_from_ui(config(false)).await.unwrap();
    h.say("John 3:16").await;
    let id = h.scripture.cards.lock().unwrap()[0].id.clone();
    h.orchestrator.approve_suggestion(&id).await;
    assert_eq!(
        h.engine_log.take(),
        ["ShowSlide screen John 3:16 (KJV) | John 3:16 text"]
    );
    // Manual mode: the spoken command becomes a card, not a push.
    h.orchestrator.set_auto_mode(false).await;
    h.say("Next verse.").await;
    assert_eq!(h.cards().last().unwrap(), "John 3:17");
    h.orchestrator.set_auto_mode(true).await;
    h.say("Okay, next verse.").await;
    settle().await;
    assert_eq!(
        h.engine_log.take(),
        ["ShowSlide screen John 3:17 (KJV) | John 3:17 text"]
    );
}

#[tokio::test(start_paused = true)]
async fn stop_cancels_countdowns_and_restore_needs_a_service() {
    let h = harness(true);
    h.orchestrator.start_from_ui(config(true)).await.unwrap();
    h.say("John 3:16").await;
    let snapshot = h.orchestrator.recovery_snapshot().await.unwrap();
    assert_eq!(snapshot["pendingSuggestions"].as_array().unwrap().len(), 1);
    h.orchestrator.stop().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    settle().await;
    assert!(h.engine_log.take().is_empty());
    assert!(h.orchestrator.stats().await.is_none());
    assert!(h.orchestrator.recovery_snapshot().await.is_none());

    let nobody = harness(false);
    assert_eq!(
        nobody
            .orchestrator
            .restore_session(&snapshot)
            .await
            .unwrap_err(),
        NO_ACTIVE_SERVICE
    );
    let again = harness(true);
    again.orchestrator.restore_session(&snapshot).await.unwrap();
    assert!(again.orchestrator.status().await.running);
    assert_eq!(again.cards(), ["John 3:16"]);
    assert_eq!(again.stt.history().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn lyrics_documents_backgrounds_and_clearing() {
    let h = harness(true);
    assert_eq!(
        h.orchestrator
            .present_lyric_slide("Song", "  ", None, None)
            .await
            .unwrap_err(),
        "Slide is empty — nothing to push."
    );
    let next = ProgramSlideInfo {
        reference: "Song 2".into(),
        text: "next".into(),
    };
    h.orchestrator
        .present_lyric_slide("Song", "Amazing grace", None, Some(next))
        .await
        .unwrap();
    assert_eq!(
        h.engine_log.take(),
        ["ShowSlide screen Song | Amazing grace"]
    );
    assert_eq!(h.program_log.take(), ["Program slide Song next=Song 2"]);
    assert!(
        h.orchestrator
            .present_document_page("/doc/p1.png", None)
            .await
    );
    assert_eq!(
        h.engine_log.take(),
        [
            "SwapDocumentPage screen /doc/p1.png",
            "ShowDocument screen /doc/p1.png"
        ]
    );
    assert!(
        !h.orchestrator.present_live_background().await,
        "no live background"
    );
    assert!(h.orchestrator.test_overlay().await);
    assert_eq!(
        h.engine_log.take()[0],
        "ShowSlide screen John 3:16 (KJV) | For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life."
    );
    assert!(h.orchestrator.clear_text().await);
    assert_eq!(h.engine_log.take(), ["ClearText screen", "Clear screen"]);
    assert!(h.orchestrator.clear_overlay().await);
    assert_eq!(h.engine_log.take(), ["Clear screen"]);
    assert_eq!(
        h.program_log.take(),
        ["Program clearCurrent", "Program clear"]
    );
}

#[tokio::test(start_paused = true)]
async fn retranslating_a_card_keeps_the_new_copy() {
    let h = harness(true);
    h.orchestrator.start_from_ui(config(false)).await.unwrap();
    h.say("John 3:16").await;
    let card = h.scripture.cards.lock().unwrap()[0].clone();
    let updated = h
        .orchestrator
        .retranslate_suggestion(card, "NKJV")
        .await
        .unwrap();
    assert_eq!(
        (updated.translation.as_str(), updated.verses.len()),
        ("NKJV", 1)
    );
    h.orchestrator.approve_suggestion(&updated.id).await;
    assert_eq!(
        h.engine_log.take(),
        ["ShowSlide screen John 3:16 (NKJV) | John 3:16 text"]
    );
}

/// Speech final to the engine's ShowSlide, without the operator delay.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn speech_final_to_show_slide_latency() {
    let h = harness(true);
    let mut cfg = config(true);
    cfg.auto_present_delay_sec = 0.0;
    h.orchestrator.start_from_ui(cfg).await.unwrap();
    let mut samples = Vec::new();
    let refs: Vec<String> = (1..=21u32)
        .map(|c| format!("John {c}:{}", (c % 9) + 1))
        .chain((1..=16u32).map(|c| format!("Romans {c}:{}", (c % 9) + 1)))
        .chain((1..=3u32).map(|c| format!("Acts {c}:{}", c + 1)))
        .collect();
    for text in &refs {
        let before = h.shown_at.lock().unwrap().len();
        let sent = tokio::time::Instant::now();
        h.say(text).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while h.shown_at.lock().unwrap().len() == before {
            assert!(
                tokio::time::Instant::now() < deadline,
                "{text} never reached the engine"
            );
            tokio::time::sleep(Duration::from_micros(200)).await;
        }
        samples.push(h.shown_at.lock().unwrap()[before] - sent);
    }
    samples.sort();
    eprintln!(
        "speech final to ShowSlide: p50 {:?} p95 {:?} max {:?}",
        samples[20], samples[37], samples[39]
    );
    assert!(samples[37] < Duration::from_millis(100));
}

// ─── TypeScript oracle ──────────────────────────────────────────────────────

fn scenarios() -> serde_json::Value {
    json!({ "scenarios": [
        { "autoMode": true, "steps": [{ "say": "Turn with me to John 3:16" }, { "wait": 1100 }, { "say": "Okay, next verse." }, { "wait": 50 }] },
        { "autoMode": true, "steps": [{ "say": "John 3:16" }, { "wait": 500 }, { "say": "Romans 8:28" }, { "wait": 1500 }] },
        { "autoMode": false, "steps": [{ "say": "John 3:16" }, { "say": "John 3:16" }, { "approve": 0 }, { "say": "Next verse." }, { "wait": 50 }] },
        { "autoMode": true, "steps": [{ "say": "Romans 8:28-30" }, { "wait": 1500 }, { "approve": 1 }, { "wait": 50 }] },
        { "autoMode": true, "steps": [{ "say": "John 3:16 in the NIV" }, { "wait": 1100 }] },
        { "autoMode": true, "steps": [{ "say": "John 3:16" }, { "wait": 1100 }, { "say": "John 3:16" }, { "wait": 1100 }] },
        { "autoMode": true, "steps": [{ "say": "Romans chapter eight" }, { "wait": 6500 }] },
        { "autoMode": true, "steps": [{ "say": "John 3:16" }, { "wait": 1100 }, { "say": "Previous verse." }, { "wait": 50 }, { "say": "Go back one." }, { "wait": 50 }] },
        { "autoMode": true, "steps": [{ "say": "Next week we read Romans." }, { "wait": 100 }, { "say": "Next verse." }, { "wait": 100 }] },
        { "autoMode": true, "steps": [{ "say": "John 3:16 and Romans 8:28" }, { "wait": 1500 }] },
        { "autoMode": false, "steps": [{ "say": "Ephesians 2:8" }, { "auto": true }, { "say": "Next verse." }, { "wait": 50 }, { "say": "Philippians 4:13" }, { "wait": 1100 }] },
    ]})
}

async fn run_native(scenario: &serde_json::Value) -> serde_json::Value {
    let h = harness(true);
    h.orchestrator
        .start_from_ui(config(scenario["autoMode"].as_bool().unwrap()))
        .await
        .unwrap();
    for step in scenario["steps"].as_array().unwrap() {
        if let Some(text) = step["say"].as_str() {
            h.say(text).await;
        } else if let Some(ms) = step["wait"].as_u64() {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            settle().await;
        } else if let Some(index) = step["approve"].as_u64() {
            let ids = h.orchestrator.pending_ids().await;
            h.orchestrator
                .approve_suggestion(&ids[index as usize])
                .await;
            settle().await;
        } else if let Some(auto) = step["auto"].as_bool() {
            h.orchestrator.set_auto_mode(auto).await;
        }
    }
    h.orchestrator.stop().await;
    json!({ "cards": h.cards(), "calls": *h.calls.lock().unwrap() })
}

/// The same scenarios through Electron's real orchestrator, detector and
/// wiring with recording stubs (`tools/ts-oracle/modules/orchestrator`).
#[tokio::test(start_paused = true)]
#[ignore = "requires the Electron checkout and its node_modules"]
async fn engine_calls_match_the_typescript_orchestrator() {
    use std::io::Write;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let electron = std::env::var_os("KAIRO_ELECTRON_DIR")
        .map_or_else(|| root.join("../kairo-presenter"), std::path::PathBuf::from)
        .canonicalize()
        .unwrap();
    let oracle = root
        .join("tools/ts-oracle/modules/orchestrator")
        .canonicalize()
        .unwrap();
    let input = scenarios();
    let mut child = std::process::Command::new(electron.join("node_modules/.bin/electron"))
        .args(["--import", "tsx", "--require"])
        .arg(oracle.join("preload.cjs"))
        .args(["--require", "./scripts/test-setup.cjs"])
        .arg(oracle.join("child.mts"))
        .current_dir(&electron)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    let scenarios = input["scenarios"].as_array().unwrap();
    assert_eq!(expected.len(), scenarios.len());
    for (scenario, expected) in scenarios.iter().zip(expected) {
        let found = run_native(scenario).await;
        assert_eq!(found, expected, "{scenario}");
    }
    eprintln!("{} orchestrator scenarios matched", scenarios.len());
}
