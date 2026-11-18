//! UI integration tests for the booth (prompt 077), driven by a scripted
//! core: suggestion cards and the paint trace, the clicker, auto-present,
//! retranslation, recovery, nuggets, the activation prompt, the queue, the
//! service records and the program panels.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::program::PresentationSettings;
use kairo_domain::scripture::{
    PendingAutoPresent, ScriptureResult, ScriptureRowTranslation, ScriptureSuggestion,
    ScriptureVerse,
};
use kairo_domain::services::{ServiceCommand, ServiceSnapshot};
use kairo_domain::status::ResilienceStatus;
use kairo_domain::transcript::TranscriptResult;
use serde_json::json;
use ui_operator::booth::backend::{
    BoothAnswer, BoothCall, BoothFeed, BoothSnapshot, FeedSink, LegacyNugget, Licence, Listening,
    Reply,
};
use ui_operator::booth::text::manual_suggestion;
use ui_operator::booth::{Booth, BoothBackend, BoothEvent, BoothViews, ToolTab, detected, queue};

// ─── The scripted core ──────────────────────────────────────────────────────

type Script = Box<dyn Fn(&BoothCall) -> Option<Result<BoothAnswer, String>> + Send>;

#[derive(Default)]
struct State {
    snapshot: BoothSnapshot,
    calls: Vec<BoothCall>,
    marks: Vec<String>,
    sink: Option<FeedSink>,
    legacy_saved: Vec<Vec<LegacyNugget>>,
    presentation_saved: usize,
    /// Answers by call; `None` keeps the request waiting.
    script: Option<Script>,
    waiting: Vec<(BoothCall, Reply)>,
}

struct Core(Mutex<State>);

impl Core {
    fn new(snapshot: BoothSnapshot) -> Arc<Self> {
        Arc::new(Self(Mutex::new(State {
            snapshot,
            ..Default::default()
        })))
    }
    fn script(
        &self,
        f: impl Fn(&BoothCall) -> Option<Result<BoothAnswer, String>> + Send + 'static,
    ) {
        self.0.lock().unwrap().script = Some(Box::new(f));
    }
    fn push(&self, feed: BoothFeed) {
        let sink = self.0.lock().unwrap().sink.clone().expect("listening");
        sink(feed);
    }
    fn calls(&self) -> Vec<BoothCall> {
        self.0.lock().unwrap().calls.clone()
    }
    fn marks(&self) -> Vec<String> {
        self.0.lock().unwrap().marks.clone()
    }
}

impl BoothBackend for Core {
    fn snapshot(&self) -> BoothSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: BoothCall, reply: Reply) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(call.clone());
        let answer = match &state.script {
            Some(script) => script(&call),
            None => Some(Ok(BoothAnswer::Done)),
        };
        match answer {
            Some(answer) => {
                drop(state);
                reply(answer);
            }
            None => state.waiting.push((call, reply)),
        }
    }
    fn mark_rendered(&self, correlation_id: &str) {
        self.0.lock().unwrap().marks.push(correlation_id.into());
    }
    fn save_presentation(&self, _: PresentationSettings) {
        self.0.lock().unwrap().presentation_saved += 1;
    }
    fn save_legacy_nuggets(&self, nuggets: &[LegacyNugget]) {
        self.0.lock().unwrap().legacy_saved.push(nuggets.to_vec());
    }
}

// ─── Fixtures ───────────────────────────────────────────────────────────────

fn verse(book: &str, chapter: f64, n: f64, text: &str) -> ScriptureVerse {
    serde_json::from_value(json!({"book": book, "chapter": chapter, "verse": n, "text": text}))
        .expect("verse")
}

fn detection(id: &str, reference: &str, text: &str) -> ScriptureSuggestion {
    let (book, rest) = reference.rsplit_once(' ').unwrap();
    let (c, v) = rest.split_once(':').unwrap();
    let mut s = manual_suggestion(
        id.into(),
        reference.into(),
        vec![verse(book, c.parse().unwrap(), v.parse().unwrap(), text)],
        "KJV".into(),
    );
    s.source = kairo_domain::scripture::SuggestionSource::Auto;
    s.correlation_id = Some(format!("trace-{id}"));
    s
}

fn service(id: &str, status: &str) -> ServiceSnapshot {
    serde_json::from_value(json!({
        "activeId": id,
        "services": [{
            "id": id, "title": "", "speaker": "", "createdAt": 1_700_000_000_000.0_f64,
            "endedAt": null, "status": status, "transcript": [], "scriptures": [],
            "notes": [], "nuggets": [], "analysisError": null,
            "upload": {"status": "idle", "sermonId": null, "attempts": 0, "lastAttemptAt": null, "error": null, "retryable": true}
        }]
    }))
    .expect("service")
}

fn no_service() -> ServiceSnapshot {
    serde_json::from_value(json!({"activeId": null, "services": []})).expect("snapshot")
}

fn transcript(id: &str, text: &str) -> TranscriptResult {
    serde_json::from_value(json!({
        "id": id, "text": text, "words": [], "isFinal": true, "timestamp": 1.0, "duration": 1.0
    }))
    .expect("transcript")
}

fn resilience(recovery: bool, stt: &str) -> ResilienceStatus {
    serde_json::from_value(json!({
        "overallHealth": "ALL_GOOD", "internetConnected": true, "claudeFallbackActive": false,

        "detectorFallbackReason": null, "recoverySessionAvailable": recovery,
        "health": [{"service": "stt", "status": stt, "lastUpdated": 0.0}]
    }))
    .expect("resilience")
}

// ─── Harness ────────────────────────────────────────────────────────────────

struct Opened {
    handle: AnyWindowHandle,
    views: BoothViews,
    events: Arc<Mutex<Vec<BoothEvent>>>,
}

impl Opened {
    fn booth(&self) -> &Entity<Booth> {
        &self.views.booth
    }
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<BoothViews>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1280.), px(800.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let views = BoothViews::new(core, window, cx);
                    let root = views.detected.clone();
                    let transcript = views.transcript.clone();
                    let toolbox = views.toolbox.clone();
                    *out.lock().unwrap() = Some(views);
                    cx.new(|_| Root {
                        root,
                        transcript,
                        toolbox,
                    })
                },
            )
        })
        .expect("open")
        .0;
    let views = slot.lock().unwrap().take().unwrap();
    let events: Arc<Mutex<Vec<BoothEvent>>> = Default::default();
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&views.booth, move |_, e: &BoothEvent, _| {
            seen.lock().unwrap().push(e.clone())
        })
        .detach()
    });
    let opened = Opened {
        handle,
        views,
        events,
    };
    draw(cx, &opened);
    opened
}

/// The three booth views side by side, as the workspace lays them out.
struct Root {
    root: Entity<detected::DetectedPanel>,
    transcript: Entity<ui_operator::booth::transcript::TranscriptPanel>,
    toolbox: Entity<ui_operator::booth::toolbox::Toolbox>,
}
impl gpui_kit::Render for Root {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        _: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{ParentElement, Styled};
        gpui_kit::component::h_flex()
            .size_full()
            .child(
                gpui_kit::div()
                    .w(px(320.))
                    .h_full()
                    .child(self.transcript.clone()),
            )
            .child(gpui_kit::div().flex_1().h_full().child(self.root.clone()))
            .child(
                gpui_kit::div()
                    .w(px(360.))
                    .h_full()
                    .child(self.toolbox.clone()),
            )
    }
}

fn draw(cx: &mut TestAppContext, opened: &Opened) {
    cx.run_until_parked();
    cx.update_window(opened.handle, |_, window, cx| {
        window.render_frame(cx);
        kairo_ui_kit::a11y_audit::check(window);
    })
    .unwrap();
    cx.run_until_parked();
}

fn exists(cx: &mut TestAppContext, opened: &Opened, id: &str) -> bool {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(opened.handle, move |_, window, _| {
        window.try_find(id).is_some()
    })
    .unwrap()
}

fn click(cx: &mut TestAppContext, opened: &Opened, id: &str) {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(opened.handle, move |_, window, cx| window.click(id, cx))
        .unwrap();
    draw(cx, opened);
}

fn press(cx: &mut TestAppContext, opened: &Opened, key: &'static str) {
    cx.update_window(opened.handle, |_, window, cx| window.press(key, cx))
        .unwrap();
    cx.run_until_parked();
}

fn base() -> BoothSnapshot {
    BoothSnapshot {
        services: Some(service("svc-1", "live")),
        default_translation: "KJV".into(),
        shortcuts: Default::default(),
        ..Default::default()
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn a_suggestion_is_painted_then_its_trace_closes(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    assert!(exists(cx, &opened, detected::ids::EMPTY));

    core.push(BoothFeed::Suggestion(detection(
        "s1",
        "John 3:16",
        "For God so loved",
    )));
    draw(cx, &opened);
    assert!(exists(cx, &opened, &detected::ids::row("s1")));
    // The mark is not stamped on arrival: it waits for the frame after the
    // paint, as the source's two rAFs did. (The test platform runs no
    // platform frames, so the stamp itself is checked in the app.)
    assert!(core.marks().is_empty());
    assert_eq!(cx.read(|cx| opened.booth().read(cx).marks_in_flight()), 1);
    assert!(!exists(cx, &opened, detected::ids::EMPTY));
}

#[gpui_kit::test]
fn a_retracted_suggestion_leaves_the_list(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    core.push(BoothFeed::Suggestion(detection(
        "s1",
        "John 3:16",
        "For God so loved",
    )));
    draw(cx, &opened);
    let mut gone = detection("s1", "John 3:16", "For God so loved");
    gone.retracted = Some(true);
    core.push(BoothFeed::Suggestion(gone));
    draw(cx, &opened);
    assert!(!exists(cx, &opened, &detected::ids::row("s1")));
    // A retraction is not painted, so it adds no trace.
    draw(cx, &opened);
    assert_eq!(cx.read(|cx| opened.booth().read(cx).marks_in_flight()), 1);
}

#[gpui_kit::test]
fn rapid_clicker_presses_walk_the_list_and_stop_at_its_ends(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    cx.update_window(opened.handle, |_, window, _| window.activate_window())
        .unwrap();
    for (id, r) in [
        ("a", "Psalm 23:1"),
        ("b", "John 3:16"),
        ("c", "Romans 8:28"),
    ] {
        core.push(BoothFeed::Suggestion(detection(id, r, "text")));
    }
    draw(cx, &opened);
    let visible = cx.read(|cx| opened.booth().read(cx).visible_ids());
    assert_eq!(visible.len(), 3);
    for _ in 0..10 {
        press(cx, &opened, "pagedown");
    }
    let selected = cx.read(|cx| opened.booth().read(cx).selected().map(str::to_owned));
    assert_eq!(selected.as_deref(), visible.last().map(String::as_str));
    for _ in 0..10 {
        press(cx, &opened, "pageup");
    }
    let selected = cx.read(|cx| opened.booth().read(cx).selected().map(str::to_owned));
    assert_eq!(selected.as_deref(), visible.first().map(String::as_str));
    // Selection alone presents nothing.
    assert!(
        !core
            .calls()
            .iter()
            .any(|c| matches!(c, BoothCall::Approve(_)))
    );
}

#[gpui_kit::test]
fn after_a_card_press_the_clicker_presents_each_step(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    cx.update_window(opened.handle, |_, window, _| window.activate_window())
        .unwrap();
    for (id, r) in [("a", "Psalm 23:1"), ("b", "John 3:16")] {
        core.push(BoothFeed::Suggestion(detection(id, r, "text")));
    }
    draw(cx, &opened);
    let visible = cx.read(|cx| opened.booth().read(cx).visible_ids());
    click(cx, &opened, &detected::ids::verse_text(&visible[0]));
    cx.update(|cx| {
        opened
            .booth()
            .update(cx, |b, _| b.set_cards_in_control(true))
    });
    press(cx, &opened, "pagedown");
    let approved: Vec<String> = core
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            BoothCall::Approve(id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(approved, vec![visible[0].clone(), visible[1].clone()]);
}

#[gpui_kit::test]
fn shortcuts_stand_down_while_the_operator_types(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    core.push(BoothFeed::Suggestion(detection("a", "John 3:16", "text")));
    draw(cx, &opened);
    let used = cx.update(|cx| {
        opened.booth().update(cx, |b, cx| {
            b.handle_key("pagedown", Default::default(), true, cx)
        })
    });
    assert!(!used);
    let used = cx.update(|cx| {
        opened.booth().update(cx, |b, cx| {
            let held = ui_operator::booth::KeyModifiers {
                repeat: true,
                ..Default::default()
            };
            b.handle_key("pagedown", held, false, cx)
        })
    });
    assert!(!used, "a held key does not repeat");
}

#[gpui_kit::test]
fn an_auto_present_countdown_lands_on_screen(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    core.push(BoothFeed::Suggestion(detection(
        "a",
        "John 3:16",
        "For God",
    )));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64;
    core.push(BoothFeed::PendingAuto(PendingAutoPresent {
        suggestion_id: "a".into(),
        reference: "John 3:16".into(),
        expires_at: now + 3000.0,
    }));
    draw(cx, &opened);
    assert_eq!(
        cx.read(|cx| opened.booth().read(cx).pending_auto().len()),
        1
    );
    cx.executor().advance_clock(Duration::from_secs(4));
    draw(cx, &opened);
    let booth = cx.read(|cx| {
        let b = opened.booth().read(cx);
        (b.pending_auto().len(), b.projection().cloned())
    });
    assert_eq!(booth.0, 0);
    assert_eq!(booth.1.map(|p| p.0), Some("John 3:16".to_owned()));
}

#[gpui_kit::test]
fn dismissing_clears_a_pending_countdown(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    core.push(BoothFeed::Suggestion(detection(
        "a",
        "John 3:16",
        "For God",
    )));
    core.push(BoothFeed::PendingAuto(
        serde_json::from_value(
            json!({"suggestionId": "a", "reference": "John 3:16", "expiresAt": 9e15}),
        )
        .unwrap(),
    ));
    draw(cx, &opened);
    click(cx, &opened, &detected::ids::dismiss("a"));
    let b = cx.read(|cx| {
        let b = opened.booth().read(cx);
        (b.suggestions().len(), b.pending_auto().len())
    });
    assert_eq!(b, (0, 0));
    assert!(core.calls().contains(&BoothCall::Dismiss("a".into())));
}

#[gpui_kit::test]
fn retranslation_is_all_or_nothing(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    core.push(BoothFeed::Suggestion(detection(
        "a",
        "John 3:16",
        "For God",
    )));
    draw(cx, &opened);
    core.script(|call| match call {
        BoothCall::Retranslate { .. } => Some(Err(
            "Error invoking remote method 'scripture:retranslate': Error: NIV is not installed"
                .into(),
        )),
        _ => Some(Ok(BoothAnswer::Done)),
    });
    core.push(BoothFeed::RetranslateRow(
        serde_json::from_value(json!({"passageId": "a", "translation": "NIV"})).unwrap(),
    ));
    draw(cx, &opened);
    let toast = opened
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, BoothEvent::Toast(t) if t == "NIV is not installed"));
    assert!(toast, "the main-process prefix is stripped");
    assert_eq!(
        cx.read(|cx| opened.booth().read(cx).suggestions()[0].translation.clone()),
        "KJV"
    );

    core.script(|call| match call {
        BoothCall::Retranslate {
            suggestion,
            translation,
        } => {
            let mut s = suggestion.clone();
            s.translation = translation.clone();
            Some(Ok(BoothAnswer::Suggestion(s)))
        }
        _ => Some(Ok(BoothAnswer::Done)),
    });
    let row: ScriptureRowTranslation =
        serde_json::from_value(json!({"passageId": "a", "translation": "ESV"})).unwrap();
    core.push(BoothFeed::RetranslateRow(row));
    draw(cx, &opened);
    assert_eq!(
        cx.read(|cx| opened.booth().read(cx).suggestions()[0].translation.clone()),
        "ESV"
    );
}

#[gpui_kit::test]
fn the_recovery_prompt_restores_or_discards(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    assert!(!exists(cx, &opened, detected::ids::RESTORE));
    core.push(BoothFeed::Resilience(resilience(true, "ok")));
    draw(cx, &opened);
    click(cx, &opened, detected::ids::RESTORE);
    click(cx, &opened, detected::ids::DISCARD);
    let calls = core.calls();
    assert!(calls.contains(&BoothCall::RestoreSession));
    assert!(calls.contains(&BoothCall::DiscardSession));
}

fn listening(core: &Core) {
    core.push(BoothFeed::Status(
        serde_json::from_value(json!({
            "running": true, "autoMode": false, "health": [], "totalPresentations": 0
        }))
        .expect("status"),
    ));
}

#[gpui_kit::test]
fn a_degraded_stt_shows_reconnecting(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    listening(&core);
    core.push(BoothFeed::Resilience(resilience(false, "degraded")));
    draw(cx, &opened);
    assert!(exists(
        cx,
        &opened,
        ui_operator::booth::transcript::ids::RECONNECTING
    ));
    assert!(!cx.read(|cx| opened.booth().read(cx).stt_weak()));
}

#[gpui_kit::test]
fn a_weak_connection_warns_while_listening_only(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    let mut weak = resilience(false, "degraded");
    weak.health[0].last_error = Some(kairo_domain::status::WEAK_CONNECTION.into());
    core.push(BoothFeed::Resilience(weak));
    draw(cx, &opened);
    let id = ui_operator::booth::transcript::ids::RECONNECTING;
    assert!(!exists(cx, &opened, id), "not while stopped");
    listening(&core);
    draw(cx, &opened);
    assert!(exists(cx, &opened, id));
    assert!(cx.read(|cx| opened.booth().read(cx).stt_weak()));
}

#[gpui_kit::test]
fn legacy_nuggets_show_without_a_service_and_save_on_removal(cx: &mut TestAppContext) {
    let mut snapshot = base();
    snapshot.services = Some(no_service());
    snapshot.legacy_nuggets = vec![
        LegacyNugget {
            id: "n1".into(),
            text: "Grace upon grace".into(),
            captured_at: 1.0,
        },
        LegacyNugget {
            id: "n2".into(),
            text: "Be still".into(),
            captured_at: 2.0,
        },
    ];
    let core = Core::new(snapshot);
    let opened = open(cx, core.clone());
    assert_eq!(cx.read(|cx| opened.booth().read(cx).nuggets().len()), 2);
    click(cx, &opened, ui_operator::booth::transcript::ids::NUGGETS);
    assert!(exists(cx, &opened, "nuggets-popover"));
    click(cx, &opened, "nugget-remove-n1");
    let saved = core.0.lock().unwrap().legacy_saved.clone();
    assert_eq!(saved.last().map(|n| n.len()), Some(1));
    // Export writes the dated file.
    click(
        cx,
        &opened,
        ui_operator::booth::transcript::ids::NUGGETS_EXPORT,
    );
    assert!(core.calls().iter().any(|c| matches!(c,
        BoothCall::Export { file_name, body } if file_name.starts_with("kairo-message-nuggets-") && body.contains("Be still"))));
}

#[gpui_kit::test]
fn saving_a_nugget_needs_a_service(cx: &mut TestAppContext) {
    let mut snapshot = base();
    snapshot.services = Some(no_service());
    let core = Core::new(snapshot);
    let opened = open(cx, core.clone());
    cx.update(|cx| opened.booth().update(cx, |b, cx| b.save_nugget("x", cx)));
    assert_eq!(
        cx.read(|cx| opened.booth().read(cx).service_error().to_owned()),
        "Start transcription to save this nugget."
    );
}

#[gpui_kit::test]
fn listening_without_transcription_asks_for_activation_then_starts(cx: &mut TestAppContext) {
    let mut snapshot = base();
    snapshot.services = Some(no_service());
    let core = Core::new(snapshot);
    core.script(|call| match call {
        BoothCall::Service(ServiceCommand::Start) => {
            Some(Ok(BoothAnswer::Services(service("svc-9", "live"))))
        }
        _ => Some(Ok(BoothAnswer::Done)),
    });
    let opened = open(cx, core.clone());
    click(cx, &opened, ui_operator::booth::transcript::ids::START);
    assert!(opened.events.lock().unwrap().iter().any(|e| matches!(e,
        BoothEvent::OpenActivation { description, .. } if description.starts_with("Activate this computer"))));
    assert!(
        !core
            .calls()
            .iter()
            .any(|c| matches!(c, BoothCall::StartListening { .. }))
    );

    core.push(BoothFeed::Licence(Licence {
        activated: true,
        deepgram: true,
    }));
    draw(cx, &opened);
    assert!(
        opened
            .events
            .lock()
            .unwrap()
            .contains(&BoothEvent::CloseActivation)
    );
    let calls = core.calls();
    let start = calls
        .iter()
        .position(|c| *c == BoothCall::Service(ServiceCommand::Start));
    let listen = calls
        .iter()
        .position(|c| matches!(c, BoothCall::StartListening { .. }));
    assert!(
        start.is_some() && listen > start,
        "a service opens before listening: {calls:?}"
    );
    assert!(cx.read(|cx| opened.booth().read(cx).transcribing()));
}

#[gpui_kit::test]
fn transcript_lines_show_only_while_a_service_is_open(cx: &mut TestAppContext) {
    let mut snapshot = base();
    snapshot.services = Some(no_service());
    let core = Core::new(snapshot);
    let opened = open(cx, core.clone());
    core.push(BoothFeed::Transcript(transcript("t1", "ignored")));
    draw(cx, &opened);
    assert!(cx.read(|cx| opened.booth().read(cx).segments().is_empty()));
    core.push(BoothFeed::Services(service("svc-1", "live")));
    core.push(BoothFeed::Interim("for god so".into()));
    draw(cx, &opened);
    assert!(exists(cx, &opened, "transcript-interim"));
    core.push(BoothFeed::Transcript(transcript(
        "t2",
        "For God so loved the world",
    )));
    draw(cx, &opened);
    let b = cx.read(|cx| {
        (
            opened.booth().read(cx).segments().len(),
            opened.booth().read(cx).interim().to_owned(),
        )
    });
    assert_eq!(b, (1, String::new()));
}

#[gpui_kit::test]
fn search_results_stage_once_and_present_verse_by_verse(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    let range = ScriptureResult {
        reference: "John 1:1-2".into(),
        translation: "KJV".into(),
        verses: vec![
            verse("John", 1.0, 1.0, "In the beginning"),
            verse("John", 1.0, 2.0, "The same was"),
        ],
    };
    cx.update(|cx| {
        opened.booth().update(cx, |b, cx| {
            b.enqueue_results(vec![range.clone()], cx);
            b.enqueue_results(vec![range.clone()], cx);
        })
    });
    draw(cx, &opened);
    let queue = cx.read(|cx| opened.booth().read(cx).queue().to_vec());
    assert_eq!(queue.len(), 1, "the same passage twice is a misclick");
    assert_eq!(queue[0].verses.len(), 2, "a range stays one row");
    click(cx, &opened, &queue::ids::chip(&queue[0].id, 1));
    let calls = core.calls();
    let registered = calls.iter().find_map(|c| match c {
        BoothCall::Register(s) => Some(s.clone()),
        _ => None,
    });
    assert_eq!(registered.map(|s| s.verses.len()), Some(1));
    assert!(calls.iter().any(|c| matches!(c, BoothCall::Approve(_))));
    assert_eq!(
        cx.read(|cx| opened.booth().read(cx).projection().map(|p| p.0.clone())),
        Some("John 1:2".to_owned()),
        "the second verse goes up alone"
    );
}

#[gpui_kit::test]
fn a_failed_queue_search_says_so(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|call| match call {
        BoothCall::Search { .. } => Some(Ok(BoothAnswer::Results(Vec::new()))),
        _ => Some(Ok(BoothAnswer::Done)),
    });
    let opened = open(cx, core.clone());
    let search = cx.read(|cx| {
        opened
            .views
            .toolbox
            .read(cx)
            .queue()
            .read(cx)
            .search()
            .clone()
    });
    cx.update_window(opened.handle, |_, window, cx| {
        search.update(cx, |s, cx| s.run("nothing like this".into(), window, cx))
    })
    .unwrap();
    draw(cx, &opened);
    assert_eq!(
        cx.read(|cx| search.read(cx).error().map(str::to_owned)),
        Some("No match — try \u{201c}John 3:16\u{201d} or a short phrase.".to_owned())
    );
}

#[gpui_kit::test]
fn ending_a_service_saves_it_with_its_name(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|call| match call {
        BoothCall::Service(ServiceCommand::End { .. }) => {
            Some(Ok(BoothAnswer::Services(no_service())))
        }
        _ => Some(Ok(BoothAnswer::Done)),
    });
    let opened = open(cx, core.clone());
    // The session bar shows once something is under way.
    assert!(exists(cx, &opened, ui_operator::booth::service::ids::END));
    click(cx, &opened, ui_operator::booth::service::ids::END);
    assert!(exists(cx, &opened, ui_operator::booth::service::ids::SAVE));
    click(cx, &opened, ui_operator::booth::service::ids::SAVE);
    assert!(core.calls().iter().any(|c| matches!(c,
        BoothCall::Service(ServiceCommand::End { title, plan_id: None, .. }) if title.is_empty())));
    assert!(!exists(cx, &opened, ui_operator::booth::service::ids::SAVE));
}

#[gpui_kit::test]
fn a_failed_end_keeps_the_dialog_with_the_error(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|call| match call {
        BoothCall::Service(ServiceCommand::End { .. }) => Some(Err("Disk full".into())),
        _ => Some(Ok(BoothAnswer::Done)),
    });
    let opened = open(cx, core.clone());
    click(cx, &opened, ui_operator::booth::service::ids::END);
    click(cx, &opened, ui_operator::booth::service::ids::SAVE);
    assert!(exists(cx, &opened, ui_operator::booth::service::ids::ERROR));
    assert!(exists(cx, &opened, ui_operator::booth::service::ids::SAVE));
}

#[gpui_kit::test]
fn timers_start_and_stage_messages_send(cx: &mut TestAppContext) {
    let mut snapshot = base();
    snapshot.presentation = Some(PresentationSettings::default());
    let core = Core::new(snapshot);
    let opened = open(cx, core.clone());
    cx.update(|cx| {
        opened
            .booth()
            .update(cx, |b, cx| b.set_tab(ToolTab::Timers, cx))
    });
    draw(cx, &opened);
    let id = cx.read(|cx| opened.booth().read(cx).presentation().timers[0].id.clone());
    click(
        cx,
        &opened,
        &ui_operator::booth::toolbox::ids::timer_play(&id),
    );
    assert!(core.calls().contains(&BoothCall::TimerStart(id.clone())));
    click(cx, &opened, ui_operator::booth::toolbox::ids::NEW_TIMER);
    assert_eq!(
        cx.read(|cx| opened.booth().read(cx).presentation().timers.len()),
        2
    );
    assert!(core.0.lock().unwrap().presentation_saved >= 1);
}

#[gpui_kit::test]
fn a_failed_program_request_is_reported(cx: &mut TestAppContext) {
    let mut snapshot = base();
    snapshot.presentation = Some(PresentationSettings::default());
    let core = Core::new(snapshot);
    core.script(|call| match call {
        BoothCall::TimerStart(_) => Some(Err("No screens on".into())),
        _ => Some(Ok(BoothAnswer::Done)),
    });
    let opened = open(cx, core.clone());
    cx.update(|cx| {
        opened
            .booth()
            .update(cx, |b, cx| b.set_tab(ToolTab::Timers, cx))
    });
    draw(cx, &opened);
    let id = cx.read(|cx| opened.booth().read(cx).presentation().timers[0].id.clone());
    click(
        cx,
        &opened,
        &ui_operator::booth::toolbox::ids::timer_play(&id),
    );
    assert!(
        opened
            .events
            .lock()
            .unwrap()
            .contains(&BoothEvent::Alert("No screens on".into()))
    );
}

#[gpui_kit::test]
fn the_toolbox_tabs_switch_panels(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    for tab in ToolTab::ALL {
        click(
            cx,
            &opened,
            &ui_operator::booth::toolbox::ids::tab(tab.id()),
        );
        assert_eq!(cx.read(|cx| opened.booth().read(cx).tab()), tab);
    }
    click(cx, &opened, &ui_operator::booth::toolbox::ids::tab("show"));
    assert!(exists(
        cx,
        &opened,
        ui_operator::booth::toolbox::ids::PROPS_EMPTY
    ));
    click(cx, &opened, &ui_operator::booth::toolbox::ids::tab("live"));
    assert!(exists(
        cx,
        &opened,
        ui_operator::booth::toolbox::ids::CAMERA_ACCESS
    ));
}

#[gpui_kit::test]
fn suggestion_arrival_paints_within_one_frame(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    for i in 0..20 {
        core.push(BoothFeed::Suggestion(detection(
            &format!("s{i}"),
            &format!("Psalm {}:1", i + 1),
            "text",
        )));
        // One frame after the event reaches the UI, the card is there.
        draw(cx, &opened);
        assert!(exists(cx, &opened, &detected::ids::row(&format!("s{i}"))));
    }
}

// ─── Automation ─────────────────────────────────────────────────────────────

const VERSE_ONE: &str = "Drink water from your own cistern and running water from your own well";
const VERSE_TWO: &str = "Should your fountains be dispersed abroad streams of water in the streets";

/// A two-verse passage, Proverbs 5:15–16, as detection sends it.
fn passage() -> [ScriptureSuggestion; 2] {
    let card = |id: &str, n: f64, text: &str| {
        let mut s = detection(id, &format!("Proverbs 5:{n}"), text);
        s.passage_id = Some("prov-5".into());
        s.passage_index = Some(n - 15.0);
        s.passage_length = Some(2.0);
        s
    };
    [card("p15", 15.0, VERSE_ONE), card("p16", 16.0, VERSE_TWO)]
}

fn automated(follow_along: bool, auto_clear: bool) -> Arc<Core> {
    let mut snapshot = base();
    snapshot.automation.follow_along = follow_along;
    snapshot.automation.auto_clear = auto_clear;
    snapshot.automation.auto_clear_delay_sec = 4.0;
    let core = Core::new(snapshot);
    core.script(|call| match call {
        BoothCall::ClearScriptureIfLive { .. } => Some(Ok(BoothAnswer::Cleared(true))),
        _ => Some(Ok(BoothAnswer::Done)),
    });
    core
}

fn read_passage_aloud(cx: &mut TestAppContext, core: &Arc<Core>, opened: &Opened) {
    for card in passage() {
        core.push(BoothFeed::Suggestion(card));
    }
    draw(cx, opened);
    cx.update(|cx| opened.booth().update(cx, |b, cx| b.approve("p15", cx)));
    core.push(BoothFeed::Transcript(transcript("t1", VERSE_ONE)));
    draw(cx, opened);
    core.push(BoothFeed::Transcript(transcript("t2", VERSE_TWO)));
    draw(cx, opened);
}

fn cleared(core: &Core) -> Vec<(String, f64, f64)> {
    core.calls()
        .into_iter()
        .filter_map(|call| match call {
            BoothCall::ClearScriptureIfLive {
                book,
                chapter,
                verse,
            } => Some((book, chapter, verse)),
            _ => None,
        })
        .collect()
}

#[gpui_kit::test]
fn a_read_through_passage_clears_after_the_pause(cx: &mut TestAppContext) {
    let core = automated(true, true);
    let opened = open(cx, core.clone());
    read_passage_aloud(cx, &core, &opened);
    assert!(
        core.calls().contains(&BoothCall::Approve("p16".into())),
        "follow-along moved to the second verse"
    );
    assert!(cleared(&core).is_empty(), "nothing clears before the pause");
    cx.executor().advance_clock(Duration::from_secs(5));
    draw(cx, &opened);
    assert_eq!(cleared(&core), [("Proverbs".to_owned(), 5.0, 16.0)]);
    assert_eq!(
        cx.read(|cx| opened.booth().read(cx).projection().cloned()),
        None
    );
}

#[gpui_kit::test]
fn follow_along_off_leaves_the_verse_for_the_operator(cx: &mut TestAppContext) {
    let core = automated(false, false);
    let opened = open(cx, core.clone());
    read_passage_aloud(cx, &core, &opened);
    assert!(!core.calls().contains(&BoothCall::Approve("p16".into())));
    cx.executor().advance_clock(Duration::from_secs(20));
    draw(cx, &opened);
    assert!(cleared(&core).is_empty());
}

#[gpui_kit::test]
fn auto_clear_off_keeps_the_last_verse_up(cx: &mut TestAppContext) {
    let core = automated(true, false);
    let opened = open(cx, core.clone());
    read_passage_aloud(cx, &core, &opened);
    cx.executor().advance_clock(Duration::from_secs(20));
    draw(cx, &opened);
    assert!(cleared(&core).is_empty());
}
