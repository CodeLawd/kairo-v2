//! The booth (`Operator.tsx` second half, `OperatorQueueSearch.tsx`,
//! `ServicePanel.tsx`, `BoothToolbox.tsx`, `ProgramPanels.tsx`): live
//! transcript and nuggets, detected verses with auto-present and follow
//! along, the staged queue, the sermon list, service records, and the
//! program panels (timers, messages, props, video input, program sound).
//!
//! [`Booth`] owns the state and every flow; the views read it and call its
//! methods. The core is reached only through [`BoothBackend`].

pub mod backend;
pub mod detected;
pub mod queue;
pub mod service;
pub mod text;
pub mod toolbox;
pub mod transcript;
pub mod widgets;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{App, AppContext, Context, Entity, EventEmitter, Task, Window};
use kairo_domain::keyboard_shortcuts::{
    ShortcutKeyEvent, command_for_shortcut, shortcut_from_event,
};
use kairo_domain::program::{PresentationSettings, ProgramState};
use kairo_domain::scripture::live_progress::{
    DEFAULT_VISIBLE_GROUP_COUNT, PassageFollowState, ScriptureSuggestionGroup,
    group_scripture_suggestions, merge_scripture_suggestion, navigate_operator_suggestion_id,
    partition_operator_suggestion_groups,
};
use kairo_domain::scripture::query::{
    Direction, expand_scripture_result, get_adjacent_verse_queries,
};
use kairo_domain::scripture::small::{single_verse_suggestion, split_verse_suggestions};
use kairo_domain::scripture::{
    FollowProgress, LivePlanState, PendingAutoPresent, PlanMatch, ScriptureResult,
    ScriptureSuggestion, ScriptureTranslationOption, SermonPlan, SermonScriptureItem,
    SuggestionSource,
};
use kairo_domain::services::{ServiceCommand, ServiceRecord, ServiceSnapshot, ServiceStatus};
use kairo_domain::settings::{ScriptureAutomation, ShortcutBindings, ShortcutCommand};
use kairo_domain::status::{HealthStatus, OrchestratorStatus, ResilienceStatus, ServiceName};
use kairo_domain::transcript::TranscriptResult;

pub use backend::{
    BoothAnswer, BoothBackend, BoothCall, BoothFeed, BoothSnapshot, DeviceLabels, InputLevel,
    LegacyNugget, Licence, Listening, OnScreensLayer,
};
use text::{Highlight, MAX_SEGMENTS, manual_suggestion, projected_ids, strip_ipc_prefix};

/// A transcript line (`DisplaySegment`).
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub id: String,
    pub text: String,
    pub timestamp: f64,
    pub confidence: f64,
    pub highlights: Vec<Highlight>,
}

/// `resultToSegment`.
pub fn segment_of(result: &TranscriptResult) -> Segment {
    let confidence = if result.words.is_empty() {
        0.0
    } else {
        result.words.iter().map(|w| w.confidence).sum::<f64>() / result.words.len() as f64
    };
    Segment {
        id: result.id.clone(),
        text: result.text.clone(),
        timestamp: result.timestamp,
        confidence,
        highlights: Vec::new(),
    }
}

/// What the booth asks the application for that is not a core call.
#[derive(Clone, Debug, PartialEq)]
pub enum BoothEvent {
    /// `openProps(select)`: the props editor, at a prop or `timer:<id>`.
    OpenProps(Option<String>),
    /// Put the Scripture tool in front and focus its search (`Mod+F`).
    FocusSearch,
    /// `DeepgramKeyDialog`: open the activation dialog with this copy.
    OpenActivation { title: String, description: String },
    /// Close the activation dialog (transcription became available).
    CloseActivation,
    /// A passing error notice (`useTransferStore.notify({ tone: 'error' })`).
    Toast(String),
    /// A program request failed (`report` → `window.alert`).
    Alert(String),
}

/// The tool shown under the monitor (`useBoothToolboxStore.tab`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolTab {
    #[default]
    Search,
    Audio,
    Timers,
    Messages,
    Show,
    Live,
}

impl ToolTab {
    pub const ALL: [ToolTab; 6] = [
        ToolTab::Search,
        ToolTab::Audio,
        ToolTab::Timers,
        ToolTab::Messages,
        ToolTab::Show,
        ToolTab::Live,
    ];
    /// The stored id; `show` is kept so a remembered tab lands on props.
    pub fn id(self) -> &'static str {
        match self {
            ToolTab::Search => "search",
            ToolTab::Audio => "audio",
            ToolTab::Timers => "timers",
            ToolTab::Messages => "messages",
            ToolTab::Show => "show",
            ToolTab::Live => "live",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ToolTab::Search => "Scripture",
            ToolTab::Audio => "Audio",
            ToolTab::Timers => "Timers",
            ToolTab::Messages => "Messages",
            ToolTab::Show => "Props",
            ToolTab::Live => "Video",
        }
    }
    /// An unknown remembered tab falls back to Scripture.
    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|tab| tab.id() == id)
            .unwrap_or_default()
    }
}

/// How long a feed arrival took to reach a painted frame (measured).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintLatency(pub Duration);

pub struct Booth {
    backend: Arc<dyn BoothBackend>,
    // Transcript.
    pub(crate) segments: Vec<Segment>,
    pub(crate) interim: String,
    pub(crate) legacy_nuggets: Vec<LegacyNugget>,
    pub(crate) services: ServiceSnapshot,
    pub(crate) services_loaded: bool,
    pub(crate) service_error: String,
    pub(crate) pipeline_busy: bool,
    pub(crate) key_prompt_open: bool,
    pub(crate) nuggets_open: bool,
    pub(crate) level: InputLevel,
    // Detections.
    pub(crate) suggestions: Vec<ScriptureSuggestion>,
    pub(crate) selected: Option<String>,
    pub(crate) adjacent_loading: Option<String>,
    pub(crate) retranslating: Option<String>,
    pub(crate) translations: Vec<ScriptureTranslationOption>,
    pub(crate) follow: Option<PassageFollowState>,
    pub(crate) pending_auto: Vec<PendingAutoPresent>,
    auto_tasks: HashMap<String, Task<()>>,
    pub(crate) projection: Option<(String, String)>,
    /// Whether the last pointer press was on a verse card, so the clicker's
    /// next and previous present as well as select (`controllingCards`).
    pub(crate) cards_in_control: bool,
    // Queue and sermon.
    pub(crate) queue: Vec<ScriptureSuggestion>,
    pub(crate) queue_busy: Option<String>,
    pub(crate) queue_picks: HashMap<String, usize>,
    pub(crate) live_plan: Option<LivePlanState>,
    pub(crate) plans: Vec<SermonPlan>,
    // Status.
    pub(crate) transcribing: bool,
    pub(crate) auto_mode: bool,
    pub(crate) resilience: Option<ResilienceStatus>,
    pub(crate) licence: Licence,
    pub(crate) default_translation: String,
    pub(crate) shortcuts: ShortcutBindings,
    pub(crate) automation: ScriptureAutomation,
    // Program panels.
    pub(crate) program: ProgramState,
    pub(crate) presentation: PresentationSettings,
    pub(crate) devices: DeviceLabels,
    pub(crate) tab: ToolTab,
    /// The stage message was sent; its field empties on the next paint.
    pub(crate) clear_stage_draft: bool,
    // Paint tracing.
    pub(crate) marks: Vec<(String, Instant)>,
    latencies: Vec<PaintLatency>,
    /// Marks scheduled for the frame after the paint, not yet stamped.
    marks_in_flight: usize,
    stamp: u64,
    _listening: Option<Listening>,
    _feed: Option<Task<()>>,
}

impl EventEmitter<BoothEvent> for Booth {}

impl Booth {
    pub fn new(backend: Arc<dyn BoothBackend>, cx: &mut Context<Self>) -> Self {
        let snapshot = backend.snapshot();
        let mut booth = Self {
            backend: backend.clone(),
            segments: Vec::new(),
            interim: String::new(),
            legacy_nuggets: snapshot.legacy_nuggets,
            services: ServiceSnapshot {
                active_id: None,
                services: Vec::new(),
                extra: Default::default(),
            },
            services_loaded: false,
            service_error: String::new(),
            pipeline_busy: false,
            key_prompt_open: false,
            nuggets_open: false,
            level: InputLevel::default(),
            suggestions: Vec::new(),
            selected: None,
            adjacent_loading: None,
            retranslating: None,
            translations: snapshot.translations,
            follow: None,
            pending_auto: Vec::new(),
            auto_tasks: HashMap::new(),
            projection: None,
            cards_in_control: false,
            queue: Vec::new(),
            queue_busy: None,
            queue_picks: HashMap::new(),
            live_plan: snapshot.live_plan,
            plans: snapshot.plans,
            transcribing: snapshot.status.as_ref().is_some_and(|s| s.running),
            auto_mode: snapshot.status.as_ref().is_some_and(|s| s.auto_mode),
            resilience: snapshot.resilience,
            licence: snapshot.licence,
            default_translation: snapshot.default_translation,
            shortcuts: snapshot.shortcuts,
            automation: snapshot.automation,
            program: ProgramState::default(),
            presentation: snapshot.presentation.unwrap_or_default(),
            devices: snapshot.devices,
            tab: ToolTab::default(),
            clear_stage_draft: false,
            marks: Vec::new(),
            latencies: Vec::new(),
            marks_in_flight: 0,
            stamp: 0,
            _listening: None,
            _feed: None,
        };
        booth.set_program(snapshot.program);
        if booth
            .live_plan
            .as_ref()
            .is_some_and(|p| p.plan_id.is_some())
        {
            booth.tab = ToolTab::Search;
        }
        if let Some(services) = snapshot.services {
            booth.set_services(services);
        } else {
            // `services.command({ action: 'list' })`, unless a push lands first.
            booth.call(
                BoothCall::Service(ServiceCommand::List),
                cx,
                |this, result, _| match result {
                    Ok(BoothAnswer::Services(snapshot)) if !this.services_loaded => {
                        this.set_services(snapshot)
                    }
                    Ok(_) => {}
                    Err(error) => this.service_error = error,
                },
            );
        }
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<(BoothFeed, Instant)>();
        let sink: backend::FeedSink = Arc::new(move |feed| {
            let _ = tx.unbounded_send((feed, Instant::now()));
        });
        booth._listening = Some(backend.listen(sink));
        booth._feed = Some(cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some((feed, at)) = rx.next().await {
                if this
                    .update(cx, |this, cx| this.apply(feed, at, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        booth
    }

    pub fn backend(&self) -> &Arc<dyn BoothBackend> {
        &self.backend
    }

    // ─── Readers ────────────────────────────────────────────────────────────

    pub fn suggestions(&self) -> &[ScriptureSuggestion] {
        &self.suggestions
    }
    pub fn queue(&self) -> &[ScriptureSuggestion] {
        &self.queue
    }
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
    pub fn interim(&self) -> &str {
        &self.interim
    }
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub fn projection(&self) -> Option<&(String, String)> {
        self.projection.as_ref()
    }
    pub fn pending_auto(&self) -> &[PendingAutoPresent] {
        &self.pending_auto
    }
    pub fn follow(&self) -> Option<&PassageFollowState> {
        self.follow.as_ref()
    }
    pub fn transcribing(&self) -> bool {
        self.transcribing
    }
    pub fn auto_mode(&self) -> bool {
        self.auto_mode
    }
    pub fn key_prompt_open(&self) -> bool {
        self.key_prompt_open
    }
    pub fn service_error(&self) -> &str {
        &self.service_error
    }
    pub fn tab(&self) -> ToolTab {
        self.tab
    }
    pub fn program(&self) -> &ProgramState {
        &self.program
    }
    pub fn presentation(&self) -> &PresentationSettings {
        &self.presentation
    }
    pub fn latencies(&self) -> &[PaintLatency] {
        &self.latencies
    }
    /// Trace marks waiting for the frame after their card was painted.
    pub fn marks_in_flight(&self) -> usize {
        self.marks_in_flight
    }
    pub fn active_service(&self) -> Option<&ServiceRecord> {
        let id = self.services.active_id.as_deref()?;
        self.services.services.iter().find(|s| s.id == id)
    }
    /// The service's nuggets, or the legacy list when none is open.
    pub fn nuggets(&self) -> Vec<(String, String, f64)> {
        match self.active_service() {
            Some(service) => service
                .nuggets
                .iter()
                .map(|n| (n.id.clone(), n.text.clone(), n.captured_at))
                .collect(),
            None => self
                .legacy_nuggets
                .iter()
                .map(|n| (n.id.clone(), n.text.clone(), n.captured_at))
                .collect(),
        }
    }
    /// A service is open and not ended: live speech is shown.
    fn service_open(&self) -> bool {
        self.active_service()
            .is_some_and(|s| s.status != ServiceStatus::Ended)
    }
    /// `transcriptIdle`: nothing to read and nothing under way.
    pub fn transcript_idle(&self) -> bool {
        self.segments.is_empty()
            && self.interim.is_empty()
            && self.active_service().is_none()
            && !self.transcribing
    }
    /// What the microphone and the connection are doing, for the transcript.
    pub fn listen_state(&self) -> ListenState {
        ListenState::of(self.resilience.as_ref())
    }
    pub fn groups(&self) -> Vec<ScriptureSuggestionGroup> {
        group_scripture_suggestions(&self.suggestions)
    }
    pub fn projected(&self) -> HashSet<String> {
        projected_ids(self.projection.as_ref(), &self.suggestions)
    }
    /// The rows in list order: newest first.
    pub fn visible_groups(&self) -> Vec<ScriptureSuggestionGroup> {
        let groups = self.groups();
        let active = self
            .follow
            .as_ref()
            .map(|f| f.passage_id.clone())
            .or_else(|| {
                let reference = self.projection.as_ref().map(|p| p.0.as_str());
                groups
                    .iter()
                    .find(|g| {
                        g.suggestions
                            .iter()
                            .any(|s| Some(s.reference.as_str()) == reference)
                    })
                    .map(|g| g.id.clone())
            });
        let sections = partition_operator_suggestion_groups(
            &groups,
            active.as_deref(),
            DEFAULT_VISIBLE_GROUP_COUNT,
        );
        sections
            .current
            .into_iter()
            .chain(sections.history)
            .collect()
    }
    pub fn visible_ids(&self) -> Vec<String> {
        self.visible_groups()
            .iter()
            .flat_map(|g| g.suggestions.iter().map(|s| s.id.clone()))
            .collect()
    }
    /// The live sermon's passages, while one is attached.
    pub fn live_playlist(&self) -> Option<&SermonPlan> {
        let id = self.live_plan.as_ref()?.plan_id.as_deref()?;
        self.plans.iter().find(|p| p.id == id)
    }
    /// Plan items heard by the detector.
    pub fn matched_plan_items(&self) -> HashSet<String> {
        self.suggestions
            .iter()
            .filter(|s| s.source == SuggestionSource::Auto)
            .filter_map(|s| s.plan_item_id.clone())
            .collect()
    }

    // ─── Requests ───────────────────────────────────────────────────────────

    /// Runs `call` and hands its answer to `then` on the UI thread.
    pub(crate) fn call(
        &self,
        call: BoothCall,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<BoothAnswer, String>, &mut Context<Self>) + 'static,
    ) {
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.call(
            call,
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |this, cx| {
                then(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// A call whose failure is only logged (`.catch(console.error)`).
    fn fire(&self, call: BoothCall, cx: &mut Context<Self>) {
        let label = format!("{call:?}");
        self.call(call, cx, move |_, result, _| {
            if let Err(error) = result {
                tracing::error!(%error, call = %label.chars().take(60).collect::<String>(), "[Operator] request failed");
            }
        });
    }

    /// A program panel request: failures are shown (`report` → `alert`).
    pub(crate) fn program_call(&mut self, call: BoothCall, cx: &mut Context<Self>) {
        self.call(call, cx, |this, result, cx| match result {
            Err(error) => {
                tracing::error!(%error, "[Operator] program request failed");
                cx.emit(BoothEvent::Alert(error));
            }
            Ok(BoothAnswer::VideoInputs(inputs)) => {
                if inputs.is_empty() {
                    cx.emit(BoothEvent::Alert(
                        "No video inputs found. Connect a camera or capture card, then try again."
                            .into(),
                    ));
                }
                this.devices.video_inputs = inputs;
                cx.notify();
            }
            Ok(_) => {}
        });
    }

    /// Lets the clicker's next and previous present (a verse card was pressed).
    pub fn set_cards_in_control(&mut self, on: bool) {
        self.cards_in_control = on;
    }

    fn next_stamp(&mut self) -> String {
        self.stamp += 1;
        format!("{}-{}", now_ms() as u64, self.stamp)
    }

    // ─── Feed ───────────────────────────────────────────────────────────────

    pub(crate) fn apply(&mut self, feed: BoothFeed, at: Instant, cx: &mut Context<Self>) {
        match feed {
            BoothFeed::Transcript(result) => self.on_transcript(result),
            BoothFeed::Interim(text) => {
                if self.service_open() {
                    self.interim = text.clone();
                }
            }
            BoothFeed::Suggestion(suggestion) => self.on_suggestion(suggestion, at),
            BoothFeed::PendingAuto(pending) => self.on_pending_auto(pending, cx),
            BoothFeed::Follow(progress) => self.on_follow(progress),
            BoothFeed::Status(status) => self.on_status(status),
            BoothFeed::Resilience(status) => self.resilience = Some(status),
            BoothFeed::Services(snapshot) => self.set_services(snapshot),
            BoothFeed::RetranslateRow(row) => {
                let cards: Vec<_> = self
                    .suggestions
                    .iter()
                    .filter(|s| s.passage_id.as_deref().unwrap_or(&s.id) == row.passage_id)
                    .cloned()
                    .collect();
                if !cards.is_empty() && cards.iter().any(|c| c.translation != row.translation) {
                    self.retranslate(row.passage_id, cards, row.translation, cx);
                }
            }
            BoothFeed::Level(level) => self.level = level,
            BoothFeed::Program(state) => self.set_program(state),
            BoothFeed::Presentation(presentation) => self.presentation = presentation,
            BoothFeed::Licence(licence) => {
                let had = self.licence.transcription();
                self.licence = licence;
                // `DeepgramKeyDialog`: once transcription is usable, close and start.
                if self.key_prompt_open && licence.transcription() && !had {
                    self.key_prompt_open = false;
                    cx.emit(BoothEvent::CloseActivation);
                    self.toggle_pipeline(cx);
                }
            }
            BoothFeed::LivePlan(plan) => {
                let attached = plan.as_ref().and_then(|p| p.plan_id.clone());
                let before = self.live_plan.as_ref().and_then(|p| p.plan_id.clone());
                self.live_plan = plan;
                // Attaching a sermon brings its passage list into view.
                if attached.is_some() && attached != before {
                    self.tab = ToolTab::Search;
                }
            }
            BoothFeed::Plans(plans) => self.plans = plans,
            BoothFeed::Settings {
                default_translation,
                shortcuts,
                automation,
            } => {
                self.default_translation = default_translation;
                self.shortcuts = shortcuts;
                self.automation = automation;
            }
            BoothFeed::Devices(mut devices) => {
                // Video inputs come from the camera request, not this feed.
                if devices.video_inputs.is_empty() {
                    devices.video_inputs = std::mem::take(&mut self.devices.video_inputs);
                }
                self.devices = devices;
            }
        }
        cx.notify();
    }

    fn set_program(&mut self, state: ProgramState) {
        // `currentSlide` → `goLive`; a cleared slide drops the local preview
        // (`scriptureOutputClearToken`).
        match (&state.current, &self.program.current) {
            (Some(now), before) if before.as_ref() != Some(now) => {
                let reference = if now.reference.is_empty() {
                    "Projected".to_owned()
                } else {
                    now.reference.clone()
                };
                self.projection = Some((reference, now.text.clone()));
            }
            (None, Some(_)) => self.projection = None,
            _ => {}
        }
        self.program = state;
    }

    pub(crate) fn set_services(&mut self, snapshot: ServiceSnapshot) {
        let was = (self.services_loaded, self.services.active_id.clone());
        self.services = snapshot;
        self.services_loaded = true;
        if was != (true, self.services.active_id.clone()) {
            self.load_active_service();
        }
    }

    /// A different service (or the first load): its transcript and verses.
    fn load_active_service(&mut self) {
        let record = self.active_service().cloned();
        self.segments = record
            .as_ref()
            .map(|r| {
                let skip = r.transcript.len().saturating_sub(MAX_SEGMENTS);
                r.transcript.iter().skip(skip).map(segment_of).collect()
            })
            .unwrap_or_default();
        self.suggestions = record.map(|r| r.scriptures).unwrap_or_default();
        self.interim.clear();
        self.queue.clear();
        self.selected = None;
        self.pending_auto.clear();
        self.auto_tasks.clear();
        self.follow = None;
    }

    fn on_transcript(&mut self, result: TranscriptResult) {
        // The live feed shows only while a service is open; pausing is separate.
        if !self.service_open() {
            return;
        }
        self.segments.push(segment_of(&result));
        if self.segments.len() > MAX_SEGMENTS {
            let extra = self.segments.len() - MAX_SEGMENTS;
            self.segments.drain(..extra);
        }
        self.interim.clear();
    }

    fn on_status(&mut self, status: OrchestratorStatus) {
        if !status.running {
            self.interim.clear();
            self.pending_auto.clear();
            self.auto_tasks.clear();
        }
        self.transcribing = status.running;
        self.auto_mode = status.auto_mode;
        self.automation.auto_mode = status.auto_mode;
    }

    fn on_suggestion(&mut self, suggestion: ScriptureSuggestion, at: Instant) {
        if suggestion.retracted == Some(true) {
            self.suggestions.retain(|s| s.id != suggestion.id);
            for segment in &mut self.segments {
                segment.highlights.retain(|h| h.id != suggestion.id);
            }
            return;
        }
        // The latency trace's last hop closes once the frame is painted.
        if let Some(id) = suggestion.correlation_id.clone() {
            self.marks.push((id, at));
        } else {
            self.marks.push((String::new(), at));
        }
        let highlight = Highlight {
            id: suggestion.id.clone(),
            trigger_text: suggestion.trigger_text.clone(),
            reference: suggestion.reference.clone(),
            confidence: suggestion.confidence,
        };
        let lower = suggestion.trigger_text.to_lowercase();
        if let Some(segment) = self
            .segments
            .iter_mut()
            .rev()
            .find(|s| s.text.to_lowercase().contains(&lower))
        {
            segment.highlights.retain(|h| h.id != suggestion.id);
            segment.highlights.push(highlight);
        }
        // A spoken "next verse" joins its row and is selected there, or the
        // copy the row already holds when it was preloaded before.
        let spoken_step =
            suggestion.source == SuggestionSource::Manual && suggestion.passage_id.is_some();
        let already = spoken_step
            .then(|| {
                self.suggestions
                    .iter()
                    .find(|item| {
                        item.passage_id.as_deref().unwrap_or(&item.id)
                            == suggestion.passage_id.as_deref().unwrap_or_default()
                            && item.reference == suggestion.reference
                            && item.translation == suggestion.translation
                    })
                    .map(|item| item.id.clone())
            })
            .flatten();
        let id = suggestion.id.clone();
        self.insert_suggestion(suggestion);
        if spoken_step {
            self.selected = Some(already.unwrap_or(id));
        }
    }

    fn insert_suggestion(&mut self, incoming: ScriptureSuggestion) {
        if self.suggestions.iter().any(|s| s.id == incoming.id) {
            return;
        }
        self.suggestions = merge_scripture_suggestion(&self.suggestions, &incoming);
    }

    /// Called by the detected view after a frame paints a new card: closes
    /// the trace with two frame callbacks, as `markSuggestionRendered` did.
    pub(crate) fn flush_marks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.marks.is_empty() {
            return;
        }
        let marks = std::mem::take(&mut self.marks);
        self.marks_in_flight += marks.len();
        let backend = self.backend.clone();
        let this = cx.entity().downgrade();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |_, cx| {
                for (id, at) in marks {
                    let latency = PaintLatency(at.elapsed());
                    if !id.is_empty() {
                        // Telemetry must never break a live service.
                        backend.mark_rendered(&id);
                    }
                    let _ = this.update(cx, |this, _| {
                        this.marks_in_flight = this.marks_in_flight.saturating_sub(1);
                        this.latencies.push(latency);
                    });
                }
            });
        });
    }

    fn on_pending_auto(&mut self, pending: PendingAutoPresent, cx: &mut Context<Self>) {
        let id = pending.suggestion_id.clone();
        self.pending_auto.retain(|p| p.suggestion_id != id);
        self.pending_auto.push(pending.clone());
        let delay = Duration::from_millis((pending.expires_at - now_ms()).max(0.0) as u64);
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                this.auto_present_landed(&id);
                cx.notify();
            });
        });
        self.auto_tasks.insert(pending.suggestion_id, task);
    }

    /// The core presented a countdown's verse: follow it here.
    fn auto_present_landed(&mut self, id: &str) {
        self.auto_tasks.remove(id);
        if let Some(sug) = self.suggestions.iter().find(|s| s.id == id).cloned() {
            self.go_live(&sug.reference, &joined(&sug));
        }
        self.pending_auto.retain(|p| p.suggestion_id != id);
    }

    fn go_live(&mut self, reference: &str, text: &str) {
        self.projection = Some((reference.into(), text.into()));
    }

    // ─── Follow along ───────────────────────────────────────────────────────

    /// The core follows the reading; the card it moved to is picked here
    /// too, unless the operator picked another passage.
    fn on_follow(&mut self, progress: Option<FollowProgress>) {
        let Some(progress) = progress else {
            self.follow = None;
            return;
        };
        let moved = self.follow.as_ref().is_none_or(|f| {
            f.passage_id != progress.passage_id || f.current_index != progress.index
        });
        if moved
            && let Some(card) = self.suggestions.iter().find(|s| {
                s.passage_id.as_deref() == Some(progress.passage_id.as_str())
                    && s.passage_index.unwrap_or(0.0) == progress.index
            })
        {
            let picked_passage = self
                .selected
                .as_ref()
                .and_then(|id| self.suggestions.iter().find(|s| &s.id == id))
                .map(|s| s.passage_id.clone());
            if picked_passage.is_none() || picked_passage == Some(card.passage_id.clone()) {
                self.selected = Some(card.id.clone());
            }
        }
        self.follow = Some(PassageFollowState {
            passage_id: progress.passage_id,
            current_index: progress.index,
            matched_token_indexes: Vec::new(),
            read_to: progress.read_to,
            heard_count: progress.heard,
        });
    }

    /// Saves the automation options and applies them to the running service.
    pub fn set_automation(&mut self, automation: ScriptureAutomation, cx: &mut Context<Self>) {
        let before = self.automation;
        self.automation = automation;
        if before.auto_mode != automation.auto_mode {
            self.auto_mode = automation.auto_mode;
        }
        self.call(
            BoothCall::SetAutomation(automation),
            cx,
            move |this, result, cx| {
                if let Err(error) = result {
                    tracing::error!(%error, "[Operator] automation save failed");
                    this.automation = before;
                    this.auto_mode = before.auto_mode;
                    cx.notify();
                }
            },
        );
        cx.notify();
    }

    // ─── Actions ────────────────────────────────────────────────────────────

    /// `handleApproveSuggestion`.
    pub fn approve(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_owned());
        if let Some(sug) = self.suggestions.iter().find(|s| s.id == id).cloned() {
            self.go_live(&sug.reference, &joined(&sug));
        }
        let id = id.to_owned();
        self.call(
            BoothCall::Approve(id.clone()),
            cx,
            move |this, result, _| match result {
                Ok(_) => this.pending_auto.retain(|p| p.suggestion_id != id),
                Err(error) => tracing::error!(%error, "[Operator] present failed"),
            },
        );
        cx.notify();
    }

    /// `handleDismissSuggestion`.
    pub fn dismiss(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        self.call(
            BoothCall::Dismiss(id.clone()),
            cx,
            move |this, result, _| match result {
                Ok(_) => {
                    this.suggestions.retain(|s| s.id != id);
                    this.pending_auto.retain(|p| p.suggestion_id != id);
                    this.auto_tasks.remove(&id);
                }
                Err(error) => tracing::error!(%error, "[Operator] dismiss failed"),
            },
        );
    }

    /// `handleClearDetections`.
    pub fn clear_detections(&mut self, cx: &mut Context<Self>) {
        for s in std::mem::take(&mut self.suggestions) {
            self.fire(BoothCall::Dismiss(s.id), cx);
        }
        self.follow = None;
        cx.notify();
    }

    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() != Some(id) {
            self.selected = Some(id.to_owned());
            cx.notify();
        }
    }

    /// `retranslatePassage`: every card of a passage in another translation.
    pub fn retranslate(
        &mut self,
        passage_id: String,
        cards: Vec<ScriptureSuggestion>,
        translation: String,
        cx: &mut Context<Self>,
    ) {
        self.retranslating = Some(passage_id.clone());
        let total = cards.len();
        let results: std::rc::Rc<std::cell::RefCell<Vec<Result<ScriptureSuggestion, String>>>> =
            Default::default();
        for card in cards {
            let results = results.clone();
            let passage = passage_id.clone();
            self.call(
                BoothCall::Retranslate {
                    suggestion: card,
                    translation: translation.clone(),
                },
                cx,
                move |this, result, cx| {
                    results.borrow_mut().push(match result {
                        Ok(BoothAnswer::Suggestion(s)) => Ok(s),
                        Ok(_) => Err("Unexpected answer".into()),
                        Err(e) => Err(e),
                    });
                    if results.borrow().len() < total {
                        return;
                    }
                    if this.retranslating.as_deref() == Some(&passage) {
                        this.retranslating = None;
                    }
                    let all = std::mem::take(&mut *results.borrow_mut());
                    // All or nothing, as `Promise.all`.
                    if let Some(Err(error)) = all.iter().find(|r| r.is_err()) {
                        cx.emit(BoothEvent::Toast(strip_ipc_prefix(error)));
                        return;
                    }
                    let by_id: HashMap<String, ScriptureSuggestion> = all
                        .into_iter()
                        .flatten()
                        .map(|s| (s.id.clone(), s))
                        .collect();
                    for card in &mut this.suggestions {
                        if let Some(updated) = by_id.get(&card.id) {
                            *card = updated.clone();
                        }
                    }
                },
            );
        }
    }

    /// Re-reads the translation list when the picker opens.
    pub fn refresh_translations(&mut self, cx: &mut Context<Self>) {
        self.call(BoothCall::Translations, cx, |this, result, _| {
            // Keep the list already shown on failure.
            if let Ok(BoothAnswer::Translations(list)) = result {
                this.translations = list;
            }
        });
    }

    /// `loadAdjacentDetectedVerse`: preloads the verse before or after a row.
    pub fn load_adjacent(&mut self, group_id: &str, direction: Direction, cx: &mut Context<Self>) {
        if self.adjacent_loading.is_some() {
            return;
        }
        let Some(group) = self.groups().into_iter().find(|g| g.id == group_id) else {
            return;
        };
        let edge = match direction {
            Direction::Next => group.suggestions.last(),
            Direction::Previous => group.suggestions.first(),
        };
        let Some(edge) = edge.cloned() else {
            return;
        };
        let queries = get_adjacent_verse_queries(
            &[ScriptureResult {
                reference: edge.reference.clone(),
                translation: edge.translation.clone(),
                verses: edge.verses.clone(),
            }],
            direction,
        );
        self.adjacent_loading = Some(format!(
            "{group_id}:{}",
            if direction == Direction::Next {
                "next"
            } else {
                "previous"
            }
        ));
        cx.notify();
        self.try_adjacent(group, edge.translation, direction, queries, 0, cx);
    }

    fn try_adjacent(
        &mut self,
        group: ScriptureSuggestionGroup,
        translation: String,
        direction: Direction,
        queries: Vec<String>,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(query) = queries.get(index).cloned() else {
            self.adjacent_loading = None;
            return;
        };
        self.call(
            BoothCall::Search {
                query,
                translation: translation.clone(),
            },
            cx,
            move |this, result, cx| {
                let found = match result {
                    Ok(BoothAnswer::Results(results)) => results.into_iter().next(),
                    Ok(_) => None,
                    Err(error) => {
                        tracing::error!(%error, "[Operator] preload failed");
                        this.adjacent_loading = None;
                        return;
                    }
                };
                let adjacent = found.filter(|r| !r.verses.is_empty()).and_then(|result| {
                    if result.verses.len() > 1 && direction == Direction::Previous {
                        let last = result.verses.last()?.clone();
                        return Some(ScriptureResult {
                            reference: text::verse_reference(&last),
                            translation: result.translation.clone(),
                            verses: vec![last],
                        });
                    }
                    let expanded = expand_scripture_result(&result);
                    match direction {
                        Direction::Previous => expanded.last().cloned(),
                        Direction::Next => expanded.first().cloned(),
                    }
                });
                let Some(adjacent) = adjacent else {
                    this.try_adjacent(group, translation, direction, queries, index + 1, cx);
                    return;
                };
                if this.suggestions.iter().any(|s| {
                    s.reference == adjacent.reference && s.translation == adjacent.translation
                }) {
                    this.adjacent_loading = None;
                    return;
                }
                let indexes = group
                    .suggestions
                    .iter()
                    .map(|s| s.passage_index.unwrap_or(0.0));
                let passage_index = match direction {
                    Direction::Next => indexes.fold(f64::MIN, f64::max) + 1.0,
                    Direction::Previous => indexes.fold(f64::MAX, f64::min) - 1.0,
                };
                let length = group.suggestions.len() as f64 + 1.0;
                let mut suggestion = manual_suggestion(
                    format!(
                        "operator-adjacent-{}-{}-{}",
                        group.id,
                        adjacent.reference,
                        this.next_stamp()
                    ),
                    adjacent.reference.clone(),
                    adjacent.verses.clone(),
                    adjacent.translation.clone(),
                );
                suggestion.passage_id = Some(group.id.clone());
                suggestion.passage_reference = Some(group.reference.clone());
                suggestion.passage_index = Some(passage_index);
                suggestion.passage_length = Some(length);
                suggestion.preloaded_next = Some(direction == Direction::Next);
                let group_id = group.id.clone();
                this.call(
                    BoothCall::Register(suggestion.clone()),
                    cx,
                    move |this, result, _| {
                        this.adjacent_loading = None;
                        if let Err(error) = result {
                            tracing::error!(%error, "[Operator] preload register failed");
                            return;
                        }
                        for item in &mut this.suggestions {
                            if item.passage_id.as_deref() == Some(&group_id) {
                                item.passage_length = Some(length);
                            }
                        }
                        this.suggestions =
                            merge_scripture_suggestion(&this.suggestions, &suggestion);
                        this.selected = Some(suggestion.id.clone());
                    },
                );
            },
        );
    }

    /// `addToQueue`: the same passage twice is always a misclick.
    pub fn add_to_queue(&mut self, entry: &ScriptureSuggestion, cx: &mut Context<Self>) {
        if self.queue.iter().any(|q| q.reference == entry.reference) {
            return;
        }
        let mut staged = entry.clone();
        staged.id = format!("queued-{}-{}", entry.reference, self.next_stamp());
        self.queue.insert(0, staged);
        cx.notify();
    }

    pub fn remove_from_queue(&mut self, id: &str, cx: &mut Context<Self>) {
        self.queue.retain(|q| q.id != id);
        self.queue_picks.remove(id);
        cx.notify();
    }

    pub fn clear_queue(&mut self, cx: &mut Context<Self>) {
        self.queue.clear();
        self.queue_picks.clear();
        cx.notify();
    }

    pub fn pick_queued(&mut self, id: &str, index: usize, cx: &mut Context<Self>) {
        self.queue_picks.insert(id.to_owned(), index);
        cx.notify();
    }

    /// `enqueueSearchResults`: ranges stay one row.
    pub fn enqueue_results(&mut self, results: Vec<ScriptureResult>, cx: &mut Context<Self>) {
        let mut seen: HashSet<String> = self.queue.iter().map(|q| q.reference.clone()).collect();
        let mut incoming = Vec::new();
        for result in results {
            if !seen.insert(result.reference.clone()) {
                continue;
            }
            let count = result.verses.len();
            let stamp = self.next_stamp();
            let mut s = manual_suggestion(
                format!("manual-{}-{stamp}", result.reference),
                result.reference.clone(),
                result.verses,
                result.translation,
            );
            if count > 1 {
                s.passage_id = Some(format!("queue-{}-{stamp}", result.reference));
                s.passage_reference = Some(result.reference);
                s.passage_index = Some(0.0);
                s.passage_length = Some(count as f64);
            }
            incoming.push(s);
        }
        if incoming.is_empty() {
            return;
        }
        incoming.append(&mut self.queue);
        self.queue = incoming;
        cx.notify();
    }

    /// `handlePresentQueued`: one verse straight to the screens.
    pub fn present_queued(&mut self, id: &str, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.queue.iter().find(|q| q.id == id).cloned() else {
            return;
        };
        let slide = match entry.verses.get(index) {
            Some(verse) if index > 0 => {
                let mut one = entry.clone();
                one.id = format!("{}-{index}", entry.id);
                one.verses = vec![verse.clone()];
                single_verse_suggestion(&one)
            }
            _ => single_verse_suggestion(&entry),
        };
        self.queue_picks.insert(entry.id.clone(), index);
        self.queue_busy = Some(entry.id.clone());
        cx.notify();
        let slide_id = slide.id.clone();
        self.call(
            BoothCall::Register(slide.clone()),
            cx,
            move |this, result, cx| {
                if let Err(error) = result {
                    tracing::error!(%error, "[Operator] queue present failed");
                    this.queue_busy = None;
                    return;
                }
                this.call(BoothCall::Approve(slide_id), cx, move |this, result, _| {
                    this.queue_busy = None;
                    match result {
                        Ok(_) => this.go_live(&slide.reference, &joined(&slide)),
                        Err(error) => tracing::error!(%error, "[Operator] queue present failed"),
                    }
                });
            },
        );
    }

    /// `handlePresentPlaylistItem`: a sermon passage, verse by verse.
    pub fn present_plan_item(&mut self, item: SermonScriptureItem, cx: &mut Context<Self>) {
        self.queue_busy = Some(item.id.clone());
        cx.notify();
        if !item.available || item.verses.is_empty() {
            self.call(
                BoothCall::Search {
                    query: item.reference.clone(),
                    translation: item.translation.clone(),
                },
                cx,
                move |this, result, cx| {
                    let found = match result {
                        Ok(BoothAnswer::Results(r)) => {
                            r.into_iter().next().filter(|r| !r.verses.is_empty())
                        }
                        Ok(_) => None,
                        Err(error) => {
                            this.queue_busy = None;
                            this.service_error = error;
                            return;
                        }
                    };
                    match found {
                        Some(found) => this.present_plan_verses(
                            &item,
                            found.reference,
                            found.verses,
                            found.translation,
                            cx,
                        ),
                        None => {
                            this.queue_busy = None;
                            this.service_error = format!("Could not load {}.", item.reference);
                        }
                    }
                },
            );
        } else {
            let (reference, verses, translation) = (
                item.reference.clone(),
                item.verses.clone(),
                item.translation.clone(),
            );
            self.present_plan_verses(&item, reference, verses, translation, cx);
        }
    }

    fn present_plan_verses(
        &mut self,
        item: &SermonScriptureItem,
        reference: String,
        verses: Vec<kairo_domain::scripture::ScriptureVerse>,
        translation: String,
        cx: &mut Context<Self>,
    ) {
        let mut suggestion = manual_suggestion(
            format!("playlist-{}-{}", item.id, self.next_stamp()),
            reference,
            verses,
            translation,
        );
        suggestion.plan_id = self.live_plan.as_ref().and_then(|p| p.plan_id.clone());
        suggestion.plan_item_id = Some(item.id.clone());
        suggestion.plan_match = Some(PlanMatch::Reference);
        let slides = split_verse_suggestions(&suggestion);
        let Some(first) = slides.first().cloned() else {
            self.queue_busy = None;
            return;
        };
        let total = slides.len();
        let done: std::rc::Rc<std::cell::RefCell<(usize, Option<String>)>> = Default::default();
        let reference_label = item.reference.clone();
        for slide in slides.clone() {
            let done = done.clone();
            let slides = slides.clone();
            let first = first.clone();
            let reference_label = reference_label.clone();
            self.call(BoothCall::Register(slide), cx, move |this, result, cx| {
                {
                    let mut d = done.borrow_mut();
                    d.0 += 1;
                    if let Err(error) = result {
                        d.1.get_or_insert(error);
                    }
                    if d.0 < total {
                        return;
                    }
                }
                if let Some(error) = done.borrow_mut().1.take() {
                    this.queue_busy = None;
                    this.service_error = error;
                    return;
                }
                this.call(
                    BoothCall::PresentDirectly(first.clone()),
                    cx,
                    move |this, result, _| {
                        this.queue_busy = None;
                        if let Err(error) = result {
                            this.service_error = if error.is_empty() {
                                format!("Could not present {reference_label}.")
                            } else {
                                error
                            };
                            return;
                        }
                        for slide in slides {
                            this.insert_suggestion(slide);
                        }
                        let text = first
                            .verses
                            .first()
                            .map(|v| v.text.clone())
                            .unwrap_or_default();
                        this.go_live(&first.reference, &text);
                    },
                );
            });
        }
    }

    /// `handleClearText`.
    pub fn clear_text(&mut self, cx: &mut Context<Self>) {
        self.call(BoothCall::ClearText, cx, |this, result, _| {
            if let Err(error) = result {
                tracing::error!(%error, "[Operator] clear failed");
            }
            this.projection = None;
        });
    }

    /// `handleToggleAutoMode`.
    pub fn toggle_auto_mode(&mut self, cx: &mut Context<Self>) {
        let next = !self.auto_mode;
        self.call(
            BoothCall::SetAutoMode(next),
            cx,
            move |this, result, _| match result {
                Ok(_) => {
                    this.auto_mode = next;
                    this.automation.auto_mode = next;
                }
                Err(error) => tracing::error!(%error, "[Operator] automation toggle failed"),
            },
        );
    }

    /// `pressListen`: Start, Resume and Pause act at once; without
    /// transcription the activation prompt opens instead.
    pub fn press_listen(&mut self, cx: &mut Context<Self>) {
        if !self.transcribing && !self.licence.transcription() {
            self.open_key_prompt(cx);
            return;
        }
        self.toggle_pipeline(cx);
    }

    fn open_key_prompt(&mut self, cx: &mut Context<Self>) {
        if self.key_prompt_open {
            return;
        }
        self.key_prompt_open = true;
        cx.emit(BoothEvent::OpenActivation {
            title: "Set up live transcription".into(),
            description: if self.licence.activated {
                "Live transcription isn\u{2019}t available yet. It will start working once Kairo finishes setting it up.".into()
            } else {
                "Activate this computer for your church to use live transcription.".into()
            },
        });
        cx.notify();
    }

    /// The activation dialog was dismissed.
    pub fn close_key_prompt(&mut self, cx: &mut Context<Self>) {
        self.key_prompt_open = false;
        cx.notify();
    }

    /// `handleTogglePipeline`.
    fn toggle_pipeline(&mut self, cx: &mut Context<Self>) {
        if self.pipeline_busy {
            return;
        }
        if !self.transcribing && !self.licence.transcription() {
            self.open_key_prompt(cx);
            return;
        }
        self.pipeline_busy = true;
        self.service_error.clear();
        cx.notify();
        if self.transcribing {
            self.call(BoothCall::StopListening, cx, |this, result, _| {
                this.pipeline_busy = false;
                match result {
                    Ok(_) => this.transcribing = false,
                    Err(error) => this.service_error = error,
                }
            });
            return;
        }
        let start = |this: &mut Self, cx: &mut Context<Self>| {
            let auto_mode = this.auto_mode;
            this.call(
                BoothCall::StartListening { auto_mode },
                cx,
                |this, result, _| {
                    this.pipeline_busy = false;
                    match result {
                        Ok(_) => this.transcribing = true,
                        Err(error) => this.service_error = error,
                    }
                },
            );
        };
        // A service opens silently the first time the operator listens.
        if self.services.active_id.is_none() {
            self.call(
                BoothCall::Service(ServiceCommand::Start),
                cx,
                move |this, result, cx| match result {
                    Ok(BoothAnswer::Services(snapshot)) => {
                        this.set_services(snapshot);
                        start(this, cx);
                    }
                    Ok(_) => start(this, cx),
                    Err(error) => {
                        this.pipeline_busy = false;
                        this.service_error = error;
                    }
                },
            );
        } else {
            start(self, cx);
        }
    }

    /// `toggleNugget`: saves a transcript line as a quote.
    pub fn save_nugget(&mut self, segment_id: &str, cx: &mut Context<Self>) {
        if self.active_service().is_none() {
            self.service_error = "Start transcription to save this nugget.".into();
            cx.notify();
            return;
        }
        let Some(segment) = self.segments.iter().find(|s| s.id == segment_id) else {
            return;
        };
        let command = ServiceCommand::Nugget {
            text: segment.text.clone(),
            source_ids: vec![segment.id.clone()],
        };
        self.service_command(command, cx);
    }

    /// `removeNugget`.
    pub fn remove_nugget(&mut self, nugget_id: &str, cx: &mut Context<Self>) {
        match self.active_service().map(|s| s.id.clone()) {
            None => {
                self.legacy_nuggets.retain(|n| n.id != nugget_id);
                self.backend.save_legacy_nuggets(&self.legacy_nuggets);
                cx.notify();
            }
            Some(service_id) => self.service_command(
                ServiceCommand::RemoveNugget {
                    service_id,
                    nugget_id: nugget_id.into(),
                },
                cx,
            ),
        }
    }

    /// `exportNuggets`.
    pub fn export_nuggets(&mut self, cx: &mut Context<Self>) {
        let nuggets: Vec<(String, f64)> = self
            .nuggets()
            .into_iter()
            .map(|(_, text, at)| (text, at))
            .collect();
        if nuggets.is_empty() {
            return;
        }
        let body = text::nuggets_export(&nuggets, format_local_date_time);
        let date = chrono::Utc::now().format("%Y-%m-%d");
        self.program_call(
            BoothCall::Export {
                file_name: format!("kairo-message-nuggets-{date}.txt"),
                body,
            },
            cx,
        );
    }

    /// A `services.command` whose snapshot replaces the records.
    pub(crate) fn service_command(&mut self, command: ServiceCommand, cx: &mut Context<Self>) {
        self.call(
            BoothCall::Service(command),
            cx,
            |this, result, _| match result {
                Ok(BoothAnswer::Services(snapshot)) => this.set_services(snapshot),
                Ok(_) => {}
                Err(error) => this.service_error = error,
            },
        );
    }

    pub fn toggle_nuggets(&mut self, cx: &mut Context<Self>) {
        self.nuggets_open = !self.nuggets_open;
        cx.notify();
    }

    pub fn restore_session(&mut self, cx: &mut Context<Self>) {
        self.fire(BoothCall::RestoreSession, cx);
    }

    pub fn discard_session(&mut self, cx: &mut Context<Self>) {
        self.fire(BoothCall::DiscardSession, cx);
    }

    pub fn set_tab(&mut self, tab: ToolTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    /// `useDraftPresentation`'s setter: shown at once, saved behind.
    pub fn set_presentation(&mut self, presentation: PresentationSettings, cx: &mut Context<Self>) {
        self.backend.save_presentation(presentation.clone());
        self.presentation = presentation;
        cx.notify();
    }

    pub fn open_props(&mut self, select: Option<String>, cx: &mut Context<Self>) {
        cx.emit(BoothEvent::OpenProps(select));
    }

    // ─── Keyboard ───────────────────────────────────────────────────────────

    /// The operator's shortcuts (`handleKeyDown`). `typing` is true while a
    /// text field has focus. Returns whether the key was used.
    pub fn handle_key(
        &mut self,
        key: &str,
        modifiers: KeyModifiers,
        typing: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if typing || modifiers.repeat || self.key_prompt_open || self.nuggets_open {
            return false;
        }
        let dom = text::dom_key(key);
        let Some(shortcut) = shortcut_from_event(&ShortcutKeyEvent {
            key: &dom,
            meta_key: modifiers.meta,
            ctrl_key: modifiers.ctrl,
            alt_key: modifiers.alt,
            shift_key: modifiers.shift,
        }) else {
            return false;
        };
        let Some(command) = command_for_shortcut(&shortcut, &self.shortcuts) else {
            return false;
        };
        match command {
            ShortcutCommand::Previous | ShortcutCommand::Next => {
                let delta = if command == ShortcutCommand::Previous {
                    -1
                } else {
                    1
                };
                let visible = self.visible_ids();
                if let Some(next) =
                    navigate_operator_suggestion_id(&visible, self.selected.as_deref(), delta)
                    && Some(next.as_str()) != self.selected.as_deref()
                {
                    self.selected = Some(next.clone());
                    if self.cards_in_control {
                        self.approve(&next, cx);
                    }
                    cx.notify();
                }
            }
            ShortcutCommand::Search => {
                self.tab = ToolTab::Search;
                cx.emit(BoothEvent::FocusSearch);
                cx.notify();
            }
            ShortcutCommand::Auto => self.toggle_auto_mode(cx),
            ShortcutCommand::Clear => self.clear_text(cx),
            ShortcutCommand::Approve => {
                let id = self
                    .selected
                    .clone()
                    .or_else(|| self.visible_ids().into_iter().next());
                if let Some(id) = id {
                    self.approve(&id, cx);
                }
            }
            ShortcutCommand::Dismiss => {
                if let Some(first) = self.suggestions.first().map(|s| s.id.clone()) {
                    self.dismiss(&first, cx);
                }
            }
        }
        true
    }
}

/// The modifier state of a key press.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyModifiers {
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub repeat: bool,
}

impl From<&gpui_kit::KeyDownEvent> for KeyModifiers {
    fn from(event: &gpui_kit::KeyDownEvent) -> Self {
        let m = event.keystroke.modifiers;
        Self {
            meta: m.platform,
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            repeat: event.is_held,
        }
    }
}

fn joined(s: &ScriptureSuggestion) -> String {
    s.verses
        .iter()
        .map(|v| v.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// `toLocaleString()`.
pub(crate) fn format_local_date_time(ms: f64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| t.format("%-d/%-m/%Y, %-I:%M:%S %p").to_string())
        .unwrap_or_default()
}

/// `toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })`.
pub(crate) fn format_local_time(ms: f64, seconds: bool) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| {
            t.format(if seconds { "%I:%M:%S %p" } else { "%I:%M %p" })
                .to_string()
        })
        .unwrap_or_default()
}

/// Creates the booth and its four views.
pub struct BoothViews {
    pub booth: Entity<Booth>,
    pub transcript: Entity<transcript::TranscriptPanel>,
    pub detected: Entity<detected::DetectedPanel>,
    pub toolbox: Entity<toolbox::Toolbox>,
}

impl BoothViews {
    pub fn new(backend: Arc<dyn BoothBackend>, window: &mut Window, cx: &mut App) -> Self {
        let booth = cx.new(|cx| Booth::new(backend, cx));
        let transcript = cx.new(|cx| transcript::TranscriptPanel::new(booth.clone(), window, cx));
        let detected = cx.new(|cx| detected::DetectedPanel::new(booth.clone(), window, cx));
        let toolbox = cx.new(|cx| toolbox::Toolbox::new(booth.clone(), window, cx));
        Self {
            booth,
            transcript,
            detected,
            toolbox,
        }
    }
}

impl BoothViews {
    /// Puts the transcript, detected verses and toolbox into the workspace.
    pub fn install(&self, workspace: &Entity<crate::OperatorWorkspace>, cx: &mut App) {
        let transcript = self.transcript.clone();
        let detected = self.detected.clone();
        let toolbox = self.toolbox.clone();
        workspace.update(cx, |w, cx| {
            w.set_transcript(Some(transcript.into()), cx);
            w.set_content(Some(detected.into()), cx);
            let rail = w.rail().clone();
            rail.update(cx, |r, cx| r.set_toolbox(Some(toolbox.into()), cx));
        });
    }
}

/// What the transcript tells the operator about listening, worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenState {
    /// Transcription gave up (licence refused, reconnects exhausted).
    Stopped,
    MicUnavailable,
    MicInterrupted,
    Offline,
    /// The upload is struggling: captions may lag, AI detection is paused.
    WeakConnection,
    Reconnecting,
    Listening,
}

impl ListenState {
    pub fn of(resilience: Option<&ResilienceStatus>) -> Self {
        let Some(r) = resilience else {
            return Self::Listening;
        };
        let status = |service| {
            r.health
                .iter()
                .find(|h| h.service == service)
                .map(|h| h.status)
        };
        let stt = status(ServiceName::Stt);
        if stt == Some(HealthStatus::Error) {
            return Self::Stopped;
        }
        match status(ServiceName::Audio) {
            Some(HealthStatus::Error) => return Self::MicUnavailable,
            Some(HealthStatus::Degraded) => return Self::MicInterrupted,
            _ => {}
        }
        if !r.internet_connected {
            Self::Offline
        } else if kairo_domain::status::stt_connection_weak(&r.health) {
            Self::WeakConnection
        } else if stt == Some(HealthStatus::Degraded) {
            Self::Reconnecting
        } else {
            Self::Listening
        }
    }

    /// The short state beside the transcript title.
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::MicUnavailable => "Mic unavailable",
            Self::MicInterrupted => "Mic interrupted",
            Self::Offline => "Offline",
            Self::WeakConnection => "Weak connection",
            Self::Reconnecting => "Reconnecting",
            Self::Listening => "Listening",
        }
    }

    /// The banner's headline and what happens next; `None` when all is well.
    pub fn notice(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Self::Stopped => ("Transcription stopped", "Pause and resume to retry"),
            Self::MicUnavailable => ("Microphone unavailable", "Check the audio input"),
            Self::MicInterrupted => ("Microphone interrupted", "Reconnecting to the input"),
            Self::Offline => (
                "No internet connection",
                "Captions resume when it\u{2019}s back",
            ),
            Self::WeakConnection => ("Weak connection", "Captions may lag"),
            Self::Reconnecting => ("Reconnecting\u{2026}", "Audio is buffered"),
            Self::Listening => return None,
        })
    }

    /// Red rather than amber: transcription will not recover by itself.
    pub fn is_critical(self) -> bool {
        matches!(self, Self::Stopped | Self::MicUnavailable)
    }
}

#[cfg(test)]
mod listen_state_tests {
    use super::*;
    use kairo_domain::status::{OverallHealth, ServiceHealth, WEAK_CONNECTION};

    fn status(
        internet: bool,
        health: &[(ServiceName, HealthStatus, Option<&str>)],
    ) -> ResilienceStatus {
        ResilienceStatus {
            overall_health: OverallHealth::AllGood,
            internet_connected: internet,
            claude_fallback_active: false,
            detector_fallback_reason: None,
            recovery_session_available: false,
            health: health
                .iter()
                .map(|&(service, status, error)| ServiceHealth {
                    service,
                    status,
                    last_error: error.map(str::to_owned),
                    last_updated: 0.0,
                })
                .collect(),
        }
    }

    #[test]
    fn the_worst_problem_wins_and_all_good_is_listening() {
        use HealthStatus::*;
        use ServiceName::*;
        let of = |s: ResilienceStatus| ListenState::of(Some(&s));
        assert_eq!(ListenState::of(None), ListenState::Listening);
        assert_eq!(
            of(status(true, &[(Audio, Ok, None), (Stt, Ok, None)])),
            ListenState::Listening
        );
        assert_eq!(
            of(status(true, &[(Stt, Degraded, None)])),
            ListenState::Reconnecting
        );
        assert_eq!(
            of(status(true, &[(Stt, Degraded, Some(WEAK_CONNECTION))])),
            ListenState::WeakConnection
        );
        // Offline explains the reconnect better than "Reconnecting".
        assert_eq!(
            of(status(false, &[(Stt, Degraded, None)])),
            ListenState::Offline
        );
        assert_eq!(
            of(status(
                false,
                &[(Audio, Degraded, None), (Stt, Degraded, None)]
            )),
            ListenState::MicInterrupted
        );
        assert_eq!(
            of(status(true, &[(Audio, Error, None)])),
            ListenState::MicUnavailable
        );
        assert_eq!(
            of(status(true, &[(Audio, Error, None), (Stt, Error, None)])),
            ListenState::Stopped
        );
        assert!(ListenState::Listening.notice().is_none());
        assert!(ListenState::MicUnavailable.is_critical());
        assert!(!ListenState::Offline.is_critical());
    }
}
