//! UI tests for the Engage page (prompt 090) against a scripted core: the
//! engagements list and its cache, starting and saving engagements, the
//! join card, Slido connection states, sample questions, moderation,
//! replies, polls and engagement settings.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::audience::*;
use kairo_domain::program::PresentationSettings;
use ui_audience::backend::{
    AccountView, AudienceAnswer, AudienceBackend, AudienceCall, AudienceFeed, AudienceSnapshot,
    FeedSink, Listening, Reply,
};
use ui_audience::model::{self, QuestionTab};
use ui_audience::page::{Activity, AudiencePage, Dialog, SettingsSection, ids};

fn source(id: &str, kind: AudienceStatusKind, state: AudienceSourceState) -> AudienceSourceStatus {
    AudienceSourceStatus {
        id: id.into(),
        kind,
        label: id.into(),
        state,
        step: None,
        error: None,
        joined: None,
        can_open: None,
        code: None,
        event_id: None,
        event_name: None,
        moderator: None,
        skip_review: None,
        updated_at: None,
    }
}

fn question(
    id: &str,
    source: &str,
    state: AudienceQuestionState,
    votes: f64,
    at: f64,
) -> AudienceQuestion {
    AudienceQuestion {
        id: id.into(),
        source_id: source.into(),
        text: format!("Question {id}"),
        author: Some("Sarah".into()),
        votes,
        state,
        original_text: None,
        edited_by: None,
        sent_by: None,
        sent_at: None,
        created_at: at,
    }
}

fn session(code: &str) -> AudienceSession {
    AudienceSession {
        room_id: "room".into(),
        code: code.into(),
        url: format!("https://usekairo.com/q/{code}"),
        title: "Sunday".into(),
        qr_data_url: "data:image/png;base64,AAEC".into(),
        settings: AudienceRoomSettings::default(),
    }
}

fn empty() -> AudienceState {
    AudienceState {
        engage: None,
        live_poll_id: None,
        sample_available: true,
        questions: vec![],
        sources: vec![],
        live_id: None,
        session: None,
    }
}

fn live_kairo() -> AudienceState {
    let mut s = empty();
    s.sources = vec![source(
        "kairo",
        AudienceStatusKind::Kairo,
        AudienceSourceState::Live,
    )];
    s.session = Some(session("ABC123"));
    s.engage = Some(EngageSnapshot {
        engagement: Engagement {
            id: "e1".into(),
            code: "ABC123".into(),
            name: "Sunday".into(),
            org_name: "Church".into(),
            starts_at: None,
            ends_at: None,
            status: EngagementStatus::Open,
            activities: EngagementActivities {
                qa: QaActivity {
                    enabled: true,
                    settings: AudienceRoomSettings::default(),
                },
                polls: PollsActivity { enabled: true },
            },
            active_poll_id: None,
        },
        moderators: Some(vec![Moderator {
            id: "m1".into(),
            email: "helper@example.com".into(),
            name: None,
            invited_at: 0.0,
            last_seen_at: None,
            revoked: false,
            active: None,
        }]),
        replies: vec![],
        polls: vec![PollView {
            poll: Poll {
                id: "p1".into(),
                title: "Next song?".into(),
                options: vec![
                    PollOption {
                        id: "a".into(),
                        text: "One".into(),
                    },
                    PollOption {
                        id: "b".into(),
                        text: "Two".into(),
                    },
                ],
                multiple: false,
                state: EngagementStatus::Draft,
                show_results: false,
            },
            totals: BTreeMap::from([("a".into(), 3.0), ("b".into(), 1.0)]),
            voters: 4.0,
            voted: vec![],
        }],
    });
    s.questions = vec![
        question("q1", "kairo", AudienceQuestionState::Review, 1.0, 1000.0),
        question("q2", "kairo", AudienceQuestionState::Approved, 5.0, 500.0),
        question("q3", "kairo", AudienceQuestionState::Answered, 0.0, 100.0),
    ];
    s
}

type Script =
    Box<dyn Fn(&AudienceCall, &AudienceState) -> Option<Result<AudienceAnswer, String>> + Send>;

#[derive(Default)]
struct State {
    state: Option<AudienceState>,
    calls: Vec<AudienceCall>,
    sink: Option<FeedSink>,
    ui: BTreeMap<String, String>,
    styles: Vec<kairo_domain::program::ProgramQuestionStyle>,
    qr: Vec<String>,
    list: Vec<EngagementIndex>,
    script: Option<Script>,
}

struct Core(Mutex<State>);

impl Core {
    fn new(state: AudienceState) -> Arc<Self> {
        Arc::new(Self(Mutex::new(State {
            state: Some(state),
            ..Default::default()
        })))
    }
    fn calls(&self) -> Vec<AudienceCall> {
        self.0.lock().unwrap().calls.clone()
    }
    fn push(&self, f: AudienceFeed) {
        let sink = self.0.lock().unwrap().sink.clone().unwrap();
        sink(f);
    }
    fn script(
        &self,
        f: impl Fn(&AudienceCall, &AudienceState) -> Option<Result<AudienceAnswer, String>>
        + Send
        + 'static,
    ) {
        self.0.lock().unwrap().script = Some(Box::new(f));
    }
}

impl AudienceBackend for Core {
    fn snapshot(&self) -> AudienceSnapshot {
        AudienceSnapshot {
            state: self.0.lock().unwrap().state.clone().unwrap(),
            account: AccountView {
                signed_in: true,
                online: true,
            },
            style: PresentationSettings::default().question,
        }
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: AudienceCall, reply: Reply<AudienceAnswer>) {
        let mut s = self.0.lock().unwrap();
        s.calls.push(call.clone());
        let current = s.state.clone().unwrap();
        if let Some(r) = s.script.as_ref().and_then(|f| f(&call, &current)) {
            if let Ok(AudienceAnswer::State(next)) = &r {
                s.state = Some((**next).clone());
            }
            drop(s);
            reply(r);
            return;
        }
        let answer = match &call {
            AudienceCall::ListEngagements => AudienceAnswer::Engagements(s.list.clone()),
            AudienceCall::OpenAudiencePage => {
                AudienceAnswer::Url("https://usekairo.com/q/ABC123".into())
            }
            AudienceCall::CreateEngagement(_) | AudienceCall::DeleteEngagement(_) => {
                AudienceAnswer::Done
            }
            AudienceCall::Moderate { id, state } => {
                let mut next = current.clone();
                if let Some(q) = next.questions.iter_mut().find(|q| q.id == *id) {
                    q.state = *state;
                }
                s.state = Some(next.clone());
                AudienceAnswer::State(Box::new(next))
            }
            AudienceCall::Show(id) => {
                let mut next = current.clone();
                next.live_id = Some(id.clone());
                s.state = Some(next.clone());
                AudienceAnswer::State(Box::new(next))
            }
            AudienceCall::ShowJoinCode => {
                let mut next = current.clone();
                next.live_id = Some("join".into());
                s.state = Some(next.clone());
                AudienceAnswer::State(Box::new(next))
            }
            AudienceCall::Clear => {
                let mut next = current.clone();
                next.live_id = None;
                s.state = Some(next.clone());
                AudienceAnswer::State(Box::new(next))
            }
            AudienceCall::ShowPoll(id) => {
                let mut next = current.clone();
                next.live_poll_id = Some(Some(id.clone()));
                s.state = Some(next.clone());
                AudienceAnswer::State(Box::new(next))
            }
            AudienceCall::Start { kind, .. } if kind == "sample" => {
                let mut next = current.clone();
                next.sources.push(source(
                    "sample",
                    AudienceStatusKind::Kairo,
                    AudienceSourceState::Live,
                ));
                next.questions.push(question(
                    "s1",
                    "sample",
                    AudienceQuestionState::Approved,
                    0.0,
                    0.0,
                ));
                s.state = Some(next.clone());
                AudienceAnswer::State(Box::new(next))
            }
            AudienceCall::Stop(id) => {
                let mut next = current.clone();
                for src in next.sources.iter_mut().filter(|x| x.id == *id) {
                    src.state = AudienceSourceState::Stopped;
                }
                s.state = Some(next.clone());
                AudienceAnswer::State(Box::new(next))
            }
            _ => AudienceAnswer::State(Box::new(current)),
        };
        drop(s);
        reply(Ok(answer));
    }
    fn save_style(&self, style: kairo_domain::program::ProgramQuestionStyle, reply: Reply<()>) {
        self.0.lock().unwrap().styles.push(style);
        reply(Ok(()));
    }
    fn save_qr(&self, _: Vec<u8>, name: String, reply: Reply<()>) {
        self.0.lock().unwrap().qr.push(name);
        reply(Ok(()));
    }
    fn read_ui(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().ui.get(key).cloned()
    }
    fn write_ui(&self, key: &str, value: Option<String>) {
        let mut s = self.0.lock().unwrap();
        match value {
            Some(v) => s.ui.insert(key.into(), v),
            None => s.ui.remove(key),
        };
    }
}

struct Opened {
    handle: AnyWindowHandle,
    page: Entity<AudiencePage>,
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Rc<RefCell<Option<Entity<AudiencePage>>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1400.), px(1000.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let p = cx.new(|cx| AudiencePage::new(core, window, cx));
                    *out.borrow_mut() = Some(p.clone());
                    p
                },
            )
        })
        .unwrap()
        .0;
    let page = slot.borrow_mut().take().unwrap();
    let o = Opened { handle, page };
    for _ in 0..3 {
        draw(cx, &o);
    }
    o
}

fn draw(cx: &mut TestAppContext, o: &Opened) {
    cx.run_until_parked();
    cx.update_window(o.handle, |_, w, cx| w.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

fn exists(cx: &mut TestAppContext, o: &Opened, id: &str) -> bool {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(o.handle, move |_, w, _| w.try_find(id).is_some())
        .unwrap()
}

fn click(cx: &mut TestAppContext, o: &Opened, id: &str) {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(o.handle, move |_, w, cx| w.click(id, cx))
        .unwrap();
    draw(cx, o);
}

fn act<R>(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut AudiencePage, &mut gpui_kit::Window, &mut gpui_kit::Context<AudiencePage>) -> R,
) -> R {
    let p = o.page.clone();
    let r = cx
        .update_window(o.handle, move |_, w, cx| p.update(cx, |p, cx| f(p, w, cx)))
        .unwrap();
    draw(cx, o);
    r
}

fn read<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&AudiencePage) -> R) -> R {
    cx.read(|cx| f(o.page.read(cx)))
}

fn type_in(
    cx: &mut TestAppContext,
    o: &Opened,
    pick: impl Fn(&ui_audience::page::PageInputs) -> Entity<gpui_kit::component::input::InputState>,
    text: &str,
) {
    let input = read(cx, o, |p| pick(&p.inputs()));
    let text = text.to_owned();
    cx.update_window(o.handle, move |_, w, cx| {
        input.update(cx, |i, cx| i.set_value(text, w, cx))
    })
    .unwrap();
    draw(cx, o);
}

fn item(code: &str, status: EngagementStatus) -> EngagementIndex {
    EngagementIndex {
        id: code.into(),
        code: code.into(),
        name: format!("Service {code}"),
        status,
        starts_at: None,
        ends_at: None,
    }
}

#[gpui_kit::test]
fn the_list_loads_caches_and_says_when_offline(cx: &mut TestAppContext) {
    let core = Core::new(empty());
    core.0.lock().unwrap().list = vec![
        item("AAA111", EngagementStatus::Open),
        item("BBB222", EngagementStatus::Draft),
    ];
    let o = open(cx, core.clone());
    assert_eq!(read(cx, &o, |p| p.items().map(|i| i.len())), Some(2));
    let cached = core
        .0
        .lock()
        .unwrap()
        .ui
        .get(model::CACHE_KEY)
        .cloned()
        .unwrap();
    assert_eq!(
        model::read_cache(Some(&cached)).unwrap().len(),
        2,
        "the list is cached"
    );
    assert!(exists(cx, &o, &ids::engagement("AAA111")));
    // Offline: the list stays and the message says why.
    core.push(AudienceFeed::Account(AccountView {
        signed_in: true,
        online: false,
    }));
    draw(cx, &o);
    assert_eq!(
        read(cx, &o, |p| p.error_at("list").map(str::to_owned)).as_deref(),
        Some(model::OFFLINE)
    );
    // Signed out: the cache goes and the sign-in card shows.
    core.push(AudienceFeed::Account(AccountView {
        signed_in: false,
        online: false,
    }));
    draw(cx, &o);
    assert!(!core.0.lock().unwrap().ui.contains_key(model::CACHE_KEY));
    assert!(exists(cx, &o, ids::SIGNED_OUT));
}

#[gpui_kit::test]
fn a_cached_list_shows_at_once_and_a_failed_load_says_try_again(cx: &mut TestAppContext) {
    let core = Core::new(empty());
    core.0.lock().unwrap().ui.insert(
        model::CACHE_KEY.into(),
        model::write_cache(Some(&[item("OLD000", EngagementStatus::Closed)])).unwrap(),
    );
    core.script(|c, _| {
        matches!(c, AudienceCall::ListEngagements).then(|| Err("Please sign in again.".into()))
    });
    let o = open(cx, core);
    assert!(
        exists(cx, &o, &ids::engagement("OLD000")),
        "the cache shows while loading fails"
    );
    assert!(exists(cx, &o, ids::LIST_ERROR));
}

#[gpui_kit::test]
fn opening_and_deleting_an_engagement(cx: &mut TestAppContext) {
    let core = Core::new(empty());
    core.0.lock().unwrap().list = vec![item("AAA111", EngagementStatus::Open)];
    let o = open(cx, core.clone());
    click(cx, &o, &ids::engagement("AAA111"));
    assert!(
        core.calls()
            .contains(&AudienceCall::Resume("AAA111".into()))
    );
    act(cx, &o, |p, _, cx| p.ask_delete(Some("AAA111".into()), cx));
    assert!(exists(cx, &o, &ids::delete_confirm("AAA111")));
    click(cx, &o, &ids::delete_cancel("AAA111"));
    assert!(!exists(cx, &o, &ids::delete_confirm("AAA111")));
    act(cx, &o, |p, _, cx| p.ask_delete(Some("AAA111".into()), cx));
    click(cx, &o, &ids::delete_confirm("AAA111"));
    let calls = core.calls();
    assert!(calls.contains(&AudienceCall::DeleteEngagement("AAA111".into())));
    assert!(
        calls
            .iter()
            .filter(|c| matches!(c, AudienceCall::GetState))
            .count()
            >= 2,
        "the state is read again"
    );
}

#[gpui_kit::test]
fn a_new_engagement_starts_or_saves_and_checks_its_dates(cx: &mut TestAppContext) {
    let core = Core::new(empty());
    let o = open(cx, core.clone());
    click(cx, &o, ids::NEW_ENGAGEMENT);
    assert_eq!(
        read(cx, &o, |p| p.dialog().cloned()),
        Some(Dialog::NewEngagement)
    );
    type_in(cx, &o, |i| i.new_starts.clone(), "2026-03-14");
    type_in(cx, &o, |i| i.new_ends.clone(), "2026-03-12");
    assert!(exists(cx, &o, ids::NEW_DATES));
    act(cx, &o, |p, _, cx| p.submit_new(true, cx));
    assert!(
        !core
            .calls()
            .iter()
            .any(|c| matches!(c, AudienceCall::Start { .. })),
        "invalid dates do nothing"
    );
    type_in(cx, &o, |i| i.new_ends.clone(), "");
    click(cx, &o, &ids::room("new-review"));
    click(cx, &o, ids::NEW_SAVE);
    let created = core.calls().into_iter().find_map(|c| match c {
        AudienceCall::CreateEngagement(input) => Some(input),
        _ => None,
    });
    let created = created.expect("saved");
    assert_eq!(
        created.title,
        model::DEFAULT_TITLE,
        "an empty title gets the default"
    );
    assert_eq!(created.status, Some(DraftOrOpen::Draft));
    assert!(created.settings.unwrap().review);
    assert!(created.starts_at.flatten().is_some());
    assert_eq!(read(cx, &o, |p| p.dialog().cloned()), None);

    // Start now, failing: the dialog stays with the message.
    core.script(|c, _| {
        matches!(c, AudienceCall::Start { .. }).then(|| Err("Could not reach Kairo.".into()))
    });
    click(cx, &o, ids::NEW_ENGAGEMENT);
    type_in(cx, &o, |i| i.new_title.clone(), "Easter");
    click(cx, &o, ids::NEW_START);
    assert!(exists(cx, &o, ids::NEW_ERROR));
    assert_eq!(
        read(cx, &o, |p| p.dialog().cloned()),
        Some(Dialog::NewEngagement)
    );
}

#[gpui_kit::test]
fn the_join_card_shows_the_code_copies_downloads_and_ends(cx: &mut TestAppContext) {
    let core = Core::new(live_kairo());
    let o = open(cx, core.clone());
    assert!(exists(cx, &o, ids::KAIRO_CARD));
    click(cx, &o, ids::SHOW_CODE);
    assert_eq!(
        read(cx, &o, |p| p.state().live_id.clone()).as_deref(),
        Some("join")
    );
    click(cx, &o, ids::SHOW_CODE);
    assert!(
        core.calls().contains(&AudienceCall::Clear),
        "Hide code clears"
    );
    click(cx, &o, ids::COPY_JOIN);
    assert_eq!(
        read(cx, &o, |p| p.copied().map(str::to_owned)).as_deref(),
        Some("join")
    );
    let pasted = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(pasted.as_deref(), Some("https://usekairo.com/q/ABC123"));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(model::COPIED_MS + 10));
    draw(cx, &o);
    assert_eq!(read(cx, &o, |p| p.copied().map(str::to_owned)), None);
    click(cx, &o, ids::QR);
    assert_eq!(core.0.lock().unwrap().qr, ["kairo-abc123-qr.png"]);
    click(cx, &o, ids::END);
    assert!(exists(cx, &o, ids::END_CONFIRM));
    click(cx, &o, ids::END_KEEP);
    assert!(!exists(cx, &o, ids::END_CONFIRM));
    click(cx, &o, ids::END);
    click(cx, &o, ids::END_CONFIRM);
    assert!(core.calls().contains(&AudienceCall::Stop("kairo".into())));
}

#[gpui_kit::test]
fn slido_connects_shows_its_steps_and_reports_failure(cx: &mut TestAppContext) {
    let core = Core::new(empty());
    let o = open(cx, core.clone());
    click(cx, &o, ids::CONNECT_SLIDO);
    assert!(exists(cx, &o, ids::SLIDO_FORM));
    core.script(|c, _| {
        matches!(c, AudienceCall::Start { kind, .. } if kind == "slido").then(|| {
            Err(
                "No live Slido event with that code. Check the code, or start the event in Slido."
                    .into(),
            )
        })
    });
    type_in(cx, &o, |i| i.slido_code.clone(), "#9374525");
    click(cx, &o, ids::SLIDO_SKIP);
    click(cx, &o, ids::SLIDO_SUBMIT);
    assert!(exists(cx, &o, ids::SLIDO_ERROR));
    let start = core.calls().into_iter().find_map(|c| match c {
        AudienceCall::Start { kind, options } if kind == "slido" => Some(options),
        _ => None,
    });
    assert_eq!(start.as_ref().and_then(|o| o.skip_review), Some(true));

    // Now it connects: the card shows the sign-in step.
    core.script(|c, cur| match c {
        AudienceCall::Start { kind, .. } if kind == "slido" => {
            let mut next = cur.clone();
            let mut s = source(
                "slido-1",
                AudienceStatusKind::Slido,
                AudienceSourceState::Connecting,
            );
            s.code = Some("9374525".into());
            s.moderator = Some(true);
            s.step = Some(AudienceSourceStep::SignIn);
            next.sources.push(s);
            Some(Ok(AudienceAnswer::State(Box::new(next))))
        }
        _ => None,
    });
    click(cx, &o, ids::SLIDO_SUBMIT);
    assert!(!exists(cx, &o, ids::SLIDO_FORM));
    assert!(exists(cx, &o, &ids::slido("slido-1")));
    assert_eq!(
        read(cx, &o, |p| p.active_source()).as_deref(),
        Some("slido-1")
    );
    assert_eq!(
        read(cx, &o, |p| p.activity()),
        Activity::Qa,
        "Polls are a Kairo engagement's"
    );
    click(cx, &o, &ids::slido_window("slido-1"));
    assert!(core.calls().contains(&AudienceCall::Open("slido-1".into())));
    // Switching Include pending reconnects.
    click(cx, &o, &ids::slido_settings("slido-1"));
    click(cx, &o, &ids::slido_pending("slido-1"));
    let calls = core.calls();
    let n = calls.len();
    assert!(matches!(&calls[n - 2], AudienceCall::Stop(id) if id == "slido-1"));
    assert!(
        matches!(&calls[n - 1], AudienceCall::Start { options, .. } if options.moderator == Some(false))
    );
}

#[gpui_kit::test]
fn sample_questions_load_and_go(cx: &mut TestAppContext) {
    let core = Core::new(empty());
    let o = open(cx, core);
    click(cx, &o, ids::LOAD_SAMPLE);
    assert!(exists(cx, &o, ids::SAMPLE));
    assert_eq!(
        read(cx, &o, |p| p.active_source()).as_deref(),
        Some("sample")
    );
    assert!(!exists(cx, &o, ids::LOAD_SAMPLE));
    click(cx, &o, ids::SAMPLE_REMOVE);
    assert!(!exists(cx, &o, ids::SAMPLE));
}

#[gpui_kit::test]
fn questions_sort_moderate_show_and_reply(cx: &mut TestAppContext) {
    let core = Core::new(live_kairo());
    let o = open(cx, core.clone());
    // Open holds approved and waiting questions, most votes first.
    let ids_now: Vec<String> = read(cx, &o, |p| {
        p.visible_questions().into_iter().map(|q| q.id).collect()
    });
    assert_eq!(ids_now, ["q2", "q1"]);
    act(cx, &o, |p, _, cx| {
        p.set_sort(AudienceQuestionSort::Newest, cx)
    });
    let ids_now: Vec<String> = read(cx, &o, |p| {
        p.visible_questions().into_iter().map(|q| q.id).collect()
    });
    assert_eq!(ids_now, ["q1", "q2"]);
    assert_eq!(read(cx, &o, |p| p.tab_count(QuestionTab::Answered)), 1);
    click(cx, &o, &ids::action("q1", "approve"));
    assert!(core.calls().contains(&AudienceCall::Moderate {
        id: "q1".into(),
        state: AudienceQuestionState::Approved
    }));
    click(cx, &o, &ids::action("q2", "show"));
    assert_eq!(
        read(cx, &o, |p| p.state().live_id.clone()).as_deref(),
        Some("q2")
    );
    click(cx, &o, &ids::action("q2", "clear"));
    assert!(core.calls().contains(&AudienceCall::Clear));
    // Reply to everyone.
    click(cx, &o, &ids::action("q2", "reply"));
    type_in(cx, &o, |i| i.reply.clone(), "  Thanks!  ");
    click(cx, &o, &ids::action("q2", "reply-send"));
    assert!(
        core.calls()
            .contains(&AudienceCall::Command(EngageCommand::Reply {
                question_id: "q2".into(),
                text: "Thanks!".into(),
                audience: ReplyAudience::Everyone,
            }))
    );
    assert!(!exists(cx, &o, &ids::action("q2", "reply-send")));
    // Answered tab and its empty copy elsewhere.
    act(cx, &o, |p, _, cx| p.set_tab(QuestionTab::Hidden, cx));
    assert!(exists(cx, &o, ids::QUESTIONS_EMPTY));
    act(cx, &o, |p, _, cx| p.set_tab(QuestionTab::Answered, cx));
    click(cx, &o, &ids::action("q3", "reopen"));
    assert!(core.calls().contains(&AudienceCall::Moderate {
        id: "q3".into(),
        state: AudienceQuestionState::Approved
    }));
}

#[gpui_kit::test]
fn fixing_a_typo_needs_a_change(cx: &mut TestAppContext) {
    let core = Core::new(live_kairo());
    let o = open(cx, core.clone());
    act(cx, &o, |p, w, cx| p.open_composer("q1", true, w, cx));
    act(cx, &o, |p, _, cx| p.save_edit(cx));
    assert!(
        !core
            .calls()
            .iter()
            .any(|c| matches!(c, AudienceCall::Command(EngageCommand::Edit { .. }))),
        "unchanged text is not sent"
    );
    type_in(cx, &o, |i| i.edit.clone(), "Question q1, fixed");
    act(cx, &o, |p, _, cx| p.save_edit(cx));
    assert!(
        core.calls()
            .contains(&AudienceCall::Command(EngageCommand::Edit {
                id: "q1".into(),
                text: "Question q1, fixed".into()
            }))
    );
}

#[gpui_kit::test]
fn polls_are_written_opened_shown_and_closed(cx: &mut TestAppContext) {
    let core = Core::new(live_kairo());
    let o = open(cx, core.clone());
    act(cx, &o, |p, _, cx| p.set_activity(Activity::Polls, cx));
    assert!(exists(cx, &o, &ids::poll("p1")));
    click(cx, &o, ids::NEW_POLL);
    // Needs a question and two answers.
    act(cx, &o, |p, _, cx| p.save_poll(cx));
    assert!(
        !core
            .calls()
            .iter()
            .any(|c| matches!(c, AudienceCall::Command(EngageCommand::PollSave { .. })))
    );
    type_in(cx, &o, |i| i.poll_title.clone().unwrap(), "Lunch?");
    type_in(cx, &o, |i| i.poll_options[0].clone(), "Yes");
    type_in(cx, &o, |i| i.poll_options[1].clone(), "  ");
    act(cx, &o, |p, w, cx| p.add_poll_option(w, cx));
    type_in(cx, &o, |i| i.poll_options[2].clone(), "No");
    act(cx, &o, |p, _, cx| p.remove_poll_option(1, cx));
    click(cx, &o, ids::POLL_SAVE);
    let saved = core.calls().into_iter().find_map(|c| match c {
        AudienceCall::Command(EngageCommand::PollSave { poll }) => Some(poll),
        _ => None,
    });
    let saved = saved.expect("saved");
    assert_eq!(saved["title"], "Lunch?");
    assert_eq!(saved["options"].as_array().unwrap().len(), 2);
    assert!(saved.get("id").is_none());
    assert_eq!(read(cx, &o, |p| p.dialog().cloned()), None);
    // Open voting, show it, results.
    click(cx, &o, &ids::poll_action("p1", "open"));
    assert!(
        core.calls()
            .contains(&AudienceCall::Command(EngageCommand::PollState {
                id: "p1".into(),
                state: OpenOrClosed::Open,
                show_results: false
            }))
    );
    click(cx, &o, &ids::poll_action("p1", "screen"));
    assert_eq!(
        read(cx, &o, |p| p.state().live_poll_id.clone()),
        Some(Some("p1".into()))
    );
    click(cx, &o, &ids::poll_action("p1", "screen"));
    assert!(core.calls().contains(&AudienceCall::ClearPoll));
    // A poll that failed says so on its card.
    core.script(|c, _| {
        matches!(c, AudienceCall::Command(EngageCommand::PollState { .. }))
            .then(|| Err("Voting could not change.".into()))
    });
    click(cx, &o, &ids::poll_action("p1", "open"));
    assert_eq!(
        read(cx, &o, |p| p.error_at("poll:p1").map(str::to_owned)).as_deref(),
        Some("Voting could not change.")
    );
}

#[gpui_kit::test]
fn without_an_engagement_polls_say_so(cx: &mut TestAppContext) {
    let core = Core::new(empty());
    let o = open(cx, core);
    act(cx, &o, |p, _, cx| p.set_activity(Activity::Polls, cx));
    assert!(exists(cx, &o, ids::POLLS_NONE));
}

#[gpui_kit::test]
fn settings_change_the_room_invite_revoke_and_save_the_look(cx: &mut TestAppContext) {
    let core = Core::new(live_kairo());
    let o = open(cx, core.clone());
    click(cx, &o, ids::SETTINGS);
    assert_eq!(
        read(cx, &o, |p| p.dialog().cloned()),
        Some(Dialog::Settings(SettingsSection::General))
    );
    click(cx, &o, &ids::room("review"));
    assert!(
        core.calls()
            .iter()
            .any(|c| matches!(c, AudienceCall::SetRoomSettings(s) if s.review))
    );
    click(cx, &o, ids::SET_COPY);
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
        Some("https://usekairo.com/q/ABC123")
    );

    click(cx, &o, ids::SET_COLLAB);
    type_in(cx, &o, |i| i.invite.clone(), "new@example.com");
    click(cx, &o, ids::INVITE);
    assert!(core.calls().contains(&AudienceCall::Invite {
        email: "new@example.com".into(),
        name: String::new()
    }));
    let after = read(cx, &o, |p| p.inputs().invite.clone());
    assert_eq!(
        cx.read(|cx| after.read(cx).value().to_string()),
        "",
        "the field empties after an invite"
    );
    click(cx, &o, &ids::moderator_menu("m1"));
    click(cx, &o, &ids::revoke("m1"));
    assert!(core.calls().contains(&AudienceCall::Revoke("m1".into())));

    click(cx, &o, ids::SET_SCREENS);
    assert!(exists(cx, &o, ids::PREVIEW));
    click(cx, &o, &ids::preset("light"));
    assert!(read(cx, &o, |p| p.style_dirty()));
    click(cx, &o, ids::SET_RESET);
    assert!(!read(cx, &o, |p| p.style_dirty()));
    click(cx, &o, &ids::preset("warm"));
    click(cx, &o, ids::SET_SAVE);
    let saved = core.0.lock().unwrap().styles.last().cloned().unwrap();
    assert_eq!(saved.background, "#3b2110");
    assert!(!read(cx, &o, |p| p.style_dirty()));
    // Done saves a change and closes.
    click(cx, &o, &ids::preset("midnight"));
    act(cx, &o, |p, _, cx| p.settings_done(cx));
    assert_eq!(core.0.lock().unwrap().styles.len(), 2);
    assert_eq!(read(cx, &o, |p| p.dialog().cloned()), None);
}

#[gpui_kit::test]
fn a_long_list_draws_only_what_is_in_view(cx: &mut TestAppContext) {
    let mut state = live_kairo();
    state.questions = (0..500)
        .map(|i| {
            question(
                &format!("n{i}"),
                "kairo",
                AudienceQuestionState::Approved,
                f64::from(i),
                0.0,
            )
        })
        .collect();
    let core = Core::new(state);
    let o = open(cx, core);
    let drawn = cx
        .update_window(o.handle, |_, w, _| {
            (0..500)
                .filter(|i| {
                    w.try_find(
                        Box::leak(ids::question(&format!("n{i}")).into_boxed_str()) as &'static str
                    )
                    .is_some()
                })
                .count()
        })
        .unwrap();
    eprintln!("virtualised: {drawn} of 500 questions drawn");
    assert!(drawn > 0 && drawn < 60, "{drawn} of 500 questions drawn");
}
