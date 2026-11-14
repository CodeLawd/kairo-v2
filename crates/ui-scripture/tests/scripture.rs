//! UI integration tests for the Scripture page (prompt 078), driven by a
//! scripted core: lookup and fallbacks, live suggestions, going live, grid
//! keys, Prev/Next, playlists, picks, translation reloads, options, header
//! Clear and the lazy grid (`scripture-lazy-cards.integration.cjs`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::scripture::{
    ScriptureResult, ScriptureTranslationOption, ScriptureVerse, SermonPlan, SermonScriptureItem,
};
use serde_json::{Value, json};
use ui_scripture::backend::{
    FeedSink, Listening, OptionChange, Reply, ScriptureAnswer, ScriptureBackend, ScriptureCall,
    ScriptureFeed, ScriptureSnapshot,
};
use ui_scripture::model::{PickGesture, Source};
use ui_scripture::rows::SendStatus;
use ui_scripture::view::ids;
use ui_scripture::{Scripture, ScriptureEvent, ScriptureWorkspace};

// ─── The scripted core ──────────────────────────────────────────────────────

type Script = Box<dyn Fn(&ScriptureCall) -> Option<Result<ScriptureAnswer, String>> + Send>;

#[derive(Default)]
struct State {
    snapshot: ScriptureSnapshot,
    calls: Vec<ScriptureCall>,
    sink: Option<FeedSink>,
    recents: Vec<Value>,
    script: Option<Script>,
}

struct Core(Mutex<State>);

impl Core {
    fn new(snapshot: ScriptureSnapshot) -> Arc<Self> {
        Arc::new(Self(Mutex::new(State {
            snapshot,
            ..Default::default()
        })))
    }
    fn script(
        &self,
        f: impl Fn(&ScriptureCall) -> Option<Result<ScriptureAnswer, String>> + Send + 'static,
    ) {
        self.0.lock().unwrap().script = Some(Box::new(f));
    }
    fn push(&self, feed: ScriptureFeed) {
        let sink = self.0.lock().unwrap().sink.clone().expect("listening");
        sink(feed);
    }
    fn calls(&self) -> Vec<ScriptureCall> {
        self.0.lock().unwrap().calls.clone()
    }
    fn presented(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                ScriptureCall::Present(s) => Some(s.reference),
                _ => None,
            })
            .collect()
    }
    fn saved_plans(&self) -> Vec<SermonPlan> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                ScriptureCall::SavePlan(p) => Some(p),
                _ => None,
            })
            .collect()
    }
}

impl ScriptureBackend for Core {
    fn snapshot(&self) -> ScriptureSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: ScriptureCall, reply: Reply) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(call.clone());
        let answer = state
            .script
            .as_ref()
            .and_then(|s| s(&call))
            .unwrap_or_else(|| {
                Ok(match &call {
                    ScriptureCall::SavePlan(p) => ScriptureAnswer::Plan(p.clone()),
                    ScriptureCall::Search { .. } => ScriptureAnswer::Results(Vec::new()),
                    _ => ScriptureAnswer::Done,
                })
            });
        drop(state);
        reply(answer);
    }
    fn save_recent_lookups(&self, value: Value) {
        self.0.lock().unwrap().recents.push(value);
    }
}

// ─── Fixtures ───────────────────────────────────────────────────────────────

fn verse(book: &str, chapter: f64, n: f64) -> ScriptureVerse {
    serde_json::from_value(json!({"book": book, "chapter": chapter, "verse": n, "text": format!("{book} {chapter}:{n} text")}))
        .unwrap()
}

fn passage(
    reference: &str,
    book: &str,
    chapter: f64,
    verses: &[f64],
    translation: &str,
) -> ScriptureResult {
    ScriptureResult {
        reference: reference.into(),
        translation: translation.into(),
        verses: verses.iter().map(|n| verse(book, chapter, *n)).collect(),
    }
}

fn option(id: &str, available: bool) -> ScriptureTranslationOption {
    serde_json::from_value(json!({"id": id, "name": format!("{id} Bible"), "access": "local", "available": available, "requiresApiKey": false}))
        .unwrap()
}

fn plan(id: &str, items: Vec<SermonScriptureItem>) -> SermonPlan {
    SermonPlan {
        source_text: None,
        id: id.into(),
        title: format!("Plan {id}"),
        source_file_name: "Manual".into(),
        items,
        created_at: 1.0,
        updated_at: 1.0,
        extra: Default::default(),
    }
}

fn item(id: &str, result: &ScriptureResult) -> SermonScriptureItem {
    SermonScriptureItem {
        id: id.into(),
        reference: result.reference.clone(),
        translation: result.translation.clone(),
        verses: result.verses.clone(),
        available: true,
        error: None,
        extra: Default::default(),
    }
}

fn base() -> ScriptureSnapshot {
    let mut s = ScriptureSnapshot {
        translations: vec![
            option("KJV", true),
            option("WEB", true),
            option("NIV", false),
        ],
        ..Default::default()
    };
    s.options.default_translation = "KJV".into();
    s
}

/// A core where "John 3:16-18" (and any John 3 lookup) answers in the
/// requested translation.
fn bible() -> impl Fn(&ScriptureCall) -> Option<Result<ScriptureAnswer, String>> + Send + 'static {
    |call| match call {
        ScriptureCall::Search { query, translation } => {
            let t = translation.as_str();
            let r = match query.as_str() {
                "John 3:16-18" | "John 3:16–18" => {
                    vec![passage("John 3:16-18", "John", 3.0, &[16., 17., 18.], t)]
                }
                "John 3" => vec![passage(
                    "John 3",
                    "John",
                    3.0,
                    &[14., 15., 16., 17., 18., 19.],
                    t,
                )],
                "John 3:19" => vec![passage("John 3:19", "John", 3.0, &[19.], t)],
                "John 3:15" => vec![passage("John 3:15", "John", 3.0, &[15.], t)],
                q if q.starts_with("love") => vec![passage(
                    "1 Corinthians 13:4",
                    "1 Corinthians",
                    13.0,
                    &[4.],
                    t,
                )],
                _ => vec![],
            };
            Some(Ok(ScriptureAnswer::Results(r)))
        }
        _ => None,
    }
}

// ─── Harness ────────────────────────────────────────────────────────────────

struct Opened {
    handle: AnyWindowHandle,
    page: Entity<ScriptureWorkspace>,
    model: Entity<Scripture>,
    events: Arc<Mutex<Vec<ScriptureEvent>>>,
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<Entity<ScriptureWorkspace>>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1200.), px(700.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let page = cx.new(|cx| ScriptureWorkspace::new(core, window, cx));
                    *out.lock().unwrap() = Some(page.clone());
                    page
                },
            )
        })
        .expect("open")
        .0;
    let page = slot.lock().unwrap().take().unwrap();
    let model = cx.read(|cx| page.read(cx).model().clone());
    let events: Arc<Mutex<Vec<ScriptureEvent>>> = Default::default();
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&model, move |_, e: &ScriptureEvent, _| {
            seen.lock().unwrap().push(e.clone())
        })
        .detach()
    });
    let opened = Opened {
        handle,
        page,
        model,
        events,
    };
    draw(cx, &opened);
    opened
}

fn draw(cx: &mut TestAppContext, opened: &Opened) {
    cx.run_until_parked();
    cx.update_window(opened.handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
    // The grid measures its width on the first frame and lays out on the next.
    cx.update_window(opened.handle, |_, window, cx| window.render_frame(cx))
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
    draw(cx, opened);
}

fn lookup(cx: &mut TestAppContext, opened: &Opened, query: &str) {
    let q = query.to_owned();
    cx.update(|cx| {
        opened.model.update(cx, |m, cx| {
            m.set_query(q, cx);
            m.submit(cx);
        })
    });
    draw(cx, opened);
}

fn read<R>(cx: &mut TestAppContext, opened: &Opened, f: impl FnOnce(&Scripture) -> R) -> R {
    cx.read(|cx| f(opened.model.read(cx)))
}

fn focus_page(cx: &mut TestAppContext, opened: &Opened) {
    // A click on a card focuses the page; tests that only press keys focus it.
    let page = opened.page.clone();
    cx.update_window(opened.handle, move |_, window, cx| {
        let handle = page.read(cx).page_focus().clone();
        handle.focus(window, cx);
    })
    .unwrap();
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn the_page_opens_empty_with_the_hint(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core);
    assert!(exists(cx, &opened, ids::EMPTY));
    assert!(exists(cx, &opened, ids::QUERY));
    assert!(!exists(cx, &opened, ids::DOCK), "no queue without cards");
    assert!(!exists(cx, &opened, ids::GO_LIVE));
}

#[gpui_kit::test]
fn a_lookup_fills_the_grid_and_is_remembered(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core.clone());
    lookup(cx, &opened, "John 3:16-18");
    assert!(exists(cx, &opened, &ids::card(0)));
    assert!(exists(cx, &opened, &ids::card(2)));
    assert!(exists(cx, &opened, ids::DOCK));
    assert!(!exists(cx, &opened, ids::EMPTY));
    assert_eq!(read(cx, &opened, |m| m.source()), Some(Source::Search));
    let recents = core.0.lock().unwrap().recents.clone();
    assert_eq!(
        recents.last(),
        Some(&json!("[\"John 3:16-18\"]")),
        "stored as localStorage text"
    );
    assert_eq!(
        read(cx, &opened, |m| m.recent_lookups().to_vec()),
        vec!["John 3:16-18"]
    );
    // A lone passage has no header bar.
    assert!(!exists(cx, &opened, &ids::header(0)));
}

#[gpui_kit::test]
fn recent_lookups_come_back_from_ui_state(cx: &mut TestAppContext) {
    let mut snap = base();
    snap.recent_lookups = Some(json!("[\"Psalm 23\",\"John 3:16\"]"));
    let opened = open(cx, Core::new(snap));
    assert_eq!(
        read(cx, &opened, |m| m.recent_lookups().to_vec()),
        vec!["Psalm 23", "John 3:16"]
    );
}

#[gpui_kit::test]
fn no_results_explains_what_to_type(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core.clone());
    lookup(cx, &opened, "zzz nothing");
    assert!(exists(cx, &opened, ids::ERROR));
    assert!(read(cx, &opened, |m| m
        .error()
        .unwrap()
        .starts_with("No results.")));
    assert!(
        core.0.lock().unwrap().recents.is_empty(),
        "a miss is not remembered"
    );
}

#[gpui_kit::test]
fn a_reference_missing_in_the_bible_falls_back(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|call| match call {
        ScriptureCall::Search { translation, .. } if translation == "KJV" => {
            Some(Ok(ScriptureAnswer::Results(vec![])))
        }
        ScriptureCall::Search { translation, .. } => {
            Some(Ok(ScriptureAnswer::Results(vec![passage(
                "John 3:16",
                "John",
                3.0,
                &[16.],
                translation,
            )])))
        }
        _ => None,
    });
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16");
    let error = read(cx, &opened, |m| m.error().map(str::to_owned)).unwrap();
    assert!(
        error.starts_with("KJV text unavailable — showing "),
        "{error}"
    );
    assert!(exists(cx, &opened, &ids::card(0)));
}

#[gpui_kit::test]
fn a_failed_search_shows_the_cache_notice_or_the_message(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|call| match call {
        ScriptureCall::Search { .. } => Some(Err("database is locked".into())),
        _ => None,
    });
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16");
    assert_eq!(
        read(cx, &opened, |m| m.error().map(str::to_owned)).as_deref(),
        Some("database is locked")
    );
    assert!(read(cx, &opened, |m| m.rows().is_empty()));
}

#[gpui_kit::test]
fn live_suggestions_arrive_after_the_pause(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core.clone());
    let page = opened.page.clone();
    cx.update_window(opened.handle, move |_, window, cx| {
        page.update(cx, |p, cx| p.focus_query(window, cx))
    })
    .unwrap();
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.set_query("love is patient".into(), cx))
    });
    draw(cx, &opened);
    assert!(
        read(cx, &opened, |m| m.suggestions().is_empty()),
        "debounced"
    );
    cx.executor().advance_clock(Duration::from_millis(260));
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.suggestions().len()), 1);
    assert!(exists(cx, &opened, ids::SUGGESTIONS));
    // ↓ then Enter adds the verse on its own.
    press(cx, &opened, "down");
    press(cx, &opened, "enter");
    assert_eq!(
        read(cx, &opened, |m| m.query().to_owned()),
        "1 Corinthians 13:4"
    );
    assert!(exists(cx, &opened, &ids::card(0)));
    assert!(!exists(cx, &opened, ids::SUGGESTIONS), "the list closes");
}

#[gpui_kit::test]
fn escape_dismisses_the_list(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core);
    let page = opened.page.clone();
    cx.update_window(opened.handle, move |_, window, cx| {
        page.update(cx, |p, cx| p.focus_query(window, cx))
    })
    .unwrap();
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.set_query("love is patient".into(), cx))
    });
    cx.executor().advance_clock(Duration::from_millis(260));
    draw(cx, &opened);
    assert!(exists(cx, &opened, ids::SUGGESTIONS));
    press(cx, &opened, "escape");
    assert!(!exists(cx, &opened, ids::SUGGESTIONS));
}

#[gpui_kit::test]
fn shift_enter_opens_the_whole_chapter_on_the_verse(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core);
    let r = passage("John 3:16", "John", 3.0, &[16.], "KJV");
    cx.update(|cx| opened.model.update(cx, |m, cx| m.open_chapter(r, cx)));
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.query().to_owned()), "John 3");
    assert_eq!(read(cx, &opened, rows_len), 6);
    assert_eq!(
        read(cx, &opened, |m| m.active()),
        2,
        "John 3:16 is the third card"
    );
    assert_eq!(
        read(cx, &opened, |m| m.scope()),
        Some(ui_scripture::rows::Scope::Chapter)
    );
    click(cx, &opened, ids::SCOPE_VERSE);
    assert_eq!(read(cx, &opened, rows_len), 1);
    assert_eq!(
        read(cx, &opened, |m| m.scope()),
        Some(ui_scripture::rows::Scope::Verse)
    );
}

fn rows_len(m: &Scripture) -> usize {
    ui_scripture::rows::card_count(m.rows())
}

#[gpui_kit::test]
fn a_card_click_goes_live_then_settles(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core.clone());
    lookup(cx, &opened, "John 3:16-18");
    click(cx, &opened, &ids::card(1));
    assert_eq!(core.presented(), vec!["John 3:17"]);
    assert_eq!(read(cx, &opened, |m| (m.active(), m.cue())), (1, true));
    let status = |cx: &mut TestAppContext| read(cx, &opened, |m| m.rows()[0].cards[1].send_status);
    assert_eq!(status(cx), SendStatus::Sent);
    cx.executor().advance_clock(Duration::from_millis(3100));
    draw(cx, &opened);
    assert_eq!(status(cx), SendStatus::Idle);
}

#[gpui_kit::test]
fn a_failed_send_marks_the_card(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let script = bible();
    core.script(move |call| match call {
        ScriptureCall::Present(_) => Some(Err("no screens".into())),
        other => script(other),
    });
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16-18");
    click(cx, &opened, ids::GO_LIVE);
    assert_eq!(
        read(cx, &opened, |m| m.rows()[0].cards[0].send_status),
        SendStatus::Error
    );
}

#[gpui_kit::test]
fn arrow_keys_walk_the_grid_and_send(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core.clone());
    lookup(cx, &opened, "John 3:16-18");
    focus_page(cx, &opened);
    press(cx, &opened, "right");
    press(cx, &opened, "right");
    press(cx, &opened, "right"); // clamps at the last card
    assert_eq!(read(cx, &opened, |m| m.active()), 2);
    assert_eq!(core.presented(), vec!["John 3:17", "John 3:18"]);
    press(cx, &opened, "left");
    press(cx, &opened, "enter");
    assert_eq!(
        core.presented(),
        vec!["John 3:17", "John 3:18", "John 3:17", "John 3:17"]
    );
}

#[gpui_kit::test]
fn keys_in_the_lookup_field_do_not_send(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core.clone());
    lookup(cx, &opened, "John 3:16-18");
    let page = opened.page.clone();
    cx.update_window(opened.handle, move |_, window, cx| {
        page.update(cx, |p, cx| p.focus_query(window, cx))
    })
    .unwrap();
    press(cx, &opened, "right");
    assert!(core.presented().is_empty());
}

#[gpui_kit::test]
fn next_and_previous_extend_the_passage(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core.clone());
    lookup(cx, &opened, "John 3:16-18");
    click(cx, &opened, ids::NEXT);
    assert_eq!(read(cx, &opened, rows_len), 4);
    assert_eq!(
        read(cx, &opened, |m| m.rows()[0].reference.clone()),
        "John 3:16–19"
    );
    assert_eq!(read(cx, &opened, |m| (m.active(), m.cue())), (3, true));
    click(cx, &opened, ids::PREV);
    assert_eq!(
        read(cx, &opened, |m| m.rows()[0].cards[0]
            .result
            .reference
            .clone()),
        "John 3:15"
    );
    assert_eq!(read(cx, &opened, |m| m.active()), 0);
}

#[gpui_kit::test]
fn no_adjacent_verse_says_so(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let script = bible();
    core.script(move |call| match call {
        ScriptureCall::Search { query, .. } if !query.starts_with("John 3:16") => {
            Some(Ok(ScriptureAnswer::Results(vec![])))
        }
        other => script(other),
    });
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16-18");
    click(cx, &opened, ids::NEXT);
    assert_eq!(
        read(cx, &opened, |m| m.error().map(str::to_owned)).as_deref(),
        Some("No next verse is available in KJV.")
    );
}

#[gpui_kit::test]
fn a_playlist_opens_as_rows_with_headers(cx: &mut TestAppContext) {
    let a = passage("John 3:16-18", "John", 3.0, &[16., 17., 18.], "KJV");
    let b = passage("John 3:19", "John", 3.0, &[19.], "KJV");
    let mut snap = base();
    snap.plans = vec![plan("p1", vec![item("i1", &a), item("i2", &b)])];
    let core = Core::new(snap);
    let opened = open(cx, core.clone());
    click(cx, &opened, &ids::plan("p1"));
    assert_eq!(read(cx, &opened, |m| m.source()), Some(Source::Plan));
    assert!(exists(cx, &opened, &ids::header(0)));
    assert!(exists(cx, &opened, &ids::header(1)));
    assert!(exists(cx, &opened, &ids::item("i2")));
    assert!(
        core.saved_plans().is_empty(),
        "nothing to fill, nothing saved"
    );
    // A sidebar item focuses without going live.
    click(cx, &opened, &ids::item("i2"));
    assert_eq!(
        read(cx, &opened, |m| (m.active(), m.cue(), m.focus())),
        (3, false, true)
    );
    assert!(core.presented().is_empty());
    assert_eq!(
        read(cx, &opened, |m| m.active_item_id().map(str::to_owned)).as_deref(),
        Some("i2")
    );
    // Use for this service.
    core.script(|call| match call {
        ScriptureCall::SetLivePlan(id) => Some(Ok(ScriptureAnswer::LivePlan(
            serde_json::from_value(
                json!({"planId": id, "title": "Plan", "itemCount": 2, "unavailableCount": 0}),
            )
            .unwrap(),
        ))),
        _ => None,
    });
    click(cx, &opened, ids::USE_FOR_SERVICE);
    assert_eq!(
        read(cx, &opened, |m| m
            .live_plan()
            .and_then(|l| l.plan_id.clone()))
        .as_deref(),
        Some("p1")
    );
}

#[gpui_kit::test]
fn missing_playlist_text_is_filled_and_saved(cx: &mut TestAppContext) {
    let a = passage("John 3:16-18", "John", 3.0, &[], "KJV");
    let mut missing = item("i1", &a);
    missing.available = false;
    let mut snap = base();
    snap.plans = vec![plan("p1", vec![missing])];
    let core = Core::new(snap);
    core.script(bible());
    let opened = open(cx, core.clone());
    click(cx, &opened, &ids::plan("p1"));
    let saved = core.saved_plans();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].items[0].available);
    assert_eq!(read(cx, &opened, rows_len), 3);
}

#[gpui_kit::test]
fn create_rename_and_delete_a_playlist(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    click(cx, &opened, ids::NEW_PLAYLIST);
    let id = read(cx, &opened, |m| m.plans()[0].id.clone());
    assert_eq!(
        read(cx, &opened, |m| m.renaming().map(str::to_owned)),
        Some(id.clone())
    );
    assert!(exists(cx, &opened, ids::RENAME));
    // An empty name is refused.
    cx.update(|cx| opened.model.update(cx, |m, cx| m.save_rename("   ", cx)));
    assert_eq!(
        read(cx, &opened, |m| m.error().map(str::to_owned)).as_deref(),
        Some("Playlist name cannot be empty.")
    );
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.save_rename(" Sunday ", cx))
    });
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.plans()[0].title.clone()), "Sunday");
    assert_eq!(core.saved_plans().last().unwrap().title, "Sunday");
    // Delete asks first.
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.request_delete(Some(id.clone()), cx))
    });
    draw(cx, &opened);
    click(cx, &opened, &ids::delete_confirm(&id));
    assert!(read(cx, &opened, |m| m.plans().is_empty()));
    assert!(core.calls().contains(&ScriptureCall::DeletePlan(id)));
}

#[gpui_kit::test]
fn a_failed_rename_is_put_back(cx: &mut TestAppContext) {
    let mut snap = base();
    snap.plans = vec![plan("p1", vec![])];
    let core = Core::new(snap);
    core.script(|call| match call {
        ScriptureCall::SavePlan(_) => Some(Err("disk full".into())),
        _ => None,
    });
    let opened = open(cx, core);
    cx.update(|cx| {
        opened.model.update(cx, |m, cx| {
            m.start_rename("p1".into(), cx);
            m.save_rename("New", cx);
        })
    });
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.plans()[0].title.clone()), "Plan p1");
    assert_eq!(
        read(cx, &opened, |m| m.error().map(str::to_owned)).as_deref(),
        Some("disk full")
    );
}

#[gpui_kit::test]
fn picked_cards_go_to_a_playlist(cx: &mut TestAppContext) {
    let mut snap = base();
    snap.plans = vec![plan("p1", vec![])];
    let core = Core::new(snap);
    core.script(bible());
    let opened = open(cx, core.clone());
    lookup(cx, &opened, "John 3:16-18");
    cx.update(|cx| {
        opened.model.update(cx, |m, cx| {
            m.pick_card(0, PickGesture::Toggle, cx);
            m.pick_card(2, PickGesture::Range, cx);
        })
    });
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.picks().len()), 3);
    assert!(exists(cx, &opened, "selection-bar"));
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.add_picked_to_playlist("p1", cx))
    });
    draw(cx, &opened);
    let saved = core.saved_plans();
    assert_eq!(saved.last().unwrap().items.len(), 1, "one passage per row");
    assert_eq!(saved.last().unwrap().items[0].verses.len(), 3);
    assert_eq!(read(cx, &opened, |m| m.picks().len()), 0);
    assert!(
        opened
            .events
            .lock()
            .unwrap()
            .contains(&ScriptureEvent::Notify("Added 1 passage to Plan p1".into()))
    );
}

#[gpui_kit::test]
fn a_new_search_drops_the_picks(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16-18");
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.pick_card(1, PickGesture::Toggle, cx))
    });
    draw(cx, &opened);
    lookup(cx, &opened, "John 3:19");
    assert_eq!(read(cx, &opened, |m| m.picks().len()), 0);
}

#[gpui_kit::test]
fn reorder_and_remove_playlist_items(cx: &mut TestAppContext) {
    let a = passage("John 3:16", "John", 3.0, &[16.], "KJV");
    let b = passage("John 3:19", "John", 3.0, &[19.], "KJV");
    let mut snap = base();
    snap.plans = vec![plan("p1", vec![item("i1", &a), item("i2", &b)])];
    let core = Core::new(snap);
    let opened = open(cx, core.clone());
    click(cx, &opened, &ids::plan("p1"));
    cx.update(|cx| {
        opened.model.update(cx, |m, cx| {
            m.reorder_plan_item(
                "i2",
                "i1",
                kairo_domain::media_order::DropPosition::Before,
                cx,
            )
        })
    });
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.rows()[0].id.clone()), "i2");
    assert_eq!(core.saved_plans().last().unwrap().items[0].id, "i2");
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.remove_plan_items(vec!["i1".into()], cx))
    });
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.rows().len()), 1);
    assert!(!exists(cx, &opened, &ids::item("i1")));
}

#[gpui_kit::test]
fn a_failed_reorder_puts_the_rows_back(cx: &mut TestAppContext) {
    let a = passage("John 3:16", "John", 3.0, &[16.], "KJV");
    let b = passage("John 3:19", "John", 3.0, &[19.], "KJV");
    let mut snap = base();
    snap.plans = vec![plan("p1", vec![item("i1", &a), item("i2", &b)])];
    let core = Core::new(snap);
    let opened = open(cx, core.clone());
    click(cx, &opened, &ids::plan("p1"));
    core.script(|call| match call {
        ScriptureCall::SavePlan(_) => Some(Err(String::new())),
        _ => None,
    });
    cx.update(|cx| {
        opened.model.update(cx, |m, cx| {
            m.reorder_plan_item(
                "i2",
                "i1",
                kairo_domain::media_order::DropPosition::Before,
                cx,
            )
        })
    });
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.rows()[0].id.clone()), "i1");
    assert_eq!(
        read(cx, &opened, |m| m.error().map(str::to_owned)).as_deref(),
        Some("Could not save the new passage order.")
    );
}

#[gpui_kit::test]
fn changing_translation_reloads_search_cards(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16-18");
    cx.update(|cx| {
        opened.model.update(cx, |m, cx| {
            m.set_active(2, cx);
            m.set_translation("WEB".into(), cx)
        })
    });
    draw(cx, &opened);
    assert!(read(cx, &opened, |m| m.rows()[0]
        .cards
        .iter()
        .all(|c| c.result.translation == "WEB")));
    assert_eq!(
        read(cx, &opened, |m| m.active()),
        2,
        "the same verse stays picked"
    );
    assert!(read(cx, &opened, |m| m.error().is_none()));
}

#[gpui_kit::test]
fn a_translation_without_the_text_keeps_the_old_one(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let script = bible();
    core.script(move |call| match call {
        ScriptureCall::Search { translation, .. } if translation == "WEB" => {
            Some(Ok(ScriptureAnswer::Results(vec![])))
        }
        other => script(other),
    });
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16-18");
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.set_translation("WEB".into(), cx))
    });
    draw(cx, &opened);
    assert_eq!(
        read(cx, &opened, |m| m.error().map(str::to_owned)).as_deref(),
        Some(
            "WEB text unavailable for John 3:16–18. Those verses still show the previous translation."
        )
    );
}

#[gpui_kit::test]
fn options_save_as_they_change(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let opened = open(cx, core.clone());
    cx.update(|cx| {
        opened.model.update(cx, |m, cx| {
            m.change_option(OptionChange::MaxVerses(1.0), cx)
        })
    });
    draw(cx, &opened);
    assert_eq!(read(cx, &opened, |m| m.options().max_verses), 1.0);
    assert!(
        core.calls()
            .contains(&ScriptureCall::SaveOption(OptionChange::MaxVerses(1.0)))
    );
}

#[gpui_kit::test]
fn header_clear_drops_the_live_badges(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16-18");
    click(cx, &opened, &ids::card(0));
    cx.update(|cx| opened.model.update(cx, |m, cx| m.header_cleared(cx)));
    draw(cx, &opened);
    assert!(!read(cx, &opened, |m| m.cue()));
    assert!(read(cx, &opened, |m| m.rows()[0]
        .cards
        .iter()
        .all(|c| c.send_status == SendStatus::Idle)));
}

#[gpui_kit::test]
fn the_unavailable_default_warns(cx: &mut TestAppContext) {
    let mut snap = base();
    snap.options.default_translation = "NIV".into();
    snap.api_bible_warning = true;
    let opened = open(cx, Core::new(snap));
    assert!(exists(cx, &opened, ids::NOTICE));
}

#[gpui_kit::test]
fn clear_empties_the_page_and_focuses_the_field(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(bible());
    let opened = open(cx, core);
    lookup(cx, &opened, "John 3:16-18");
    click(cx, &opened, ids::CLEAR);
    assert!(exists(cx, &opened, ids::EMPTY));
    assert!(
        opened
            .events
            .lock()
            .unwrap()
            .contains(&ScriptureEvent::FocusQuery)
    );
}

#[gpui_kit::test]
fn playlists_from_elsewhere_are_merged(cx: &mut TestAppContext) {
    let mut snap = base();
    snap.plans = vec![plan("p1", vec![])];
    let core = Core::new(snap);
    let opened = open(cx, core.clone());
    let mut newer = plan("p1", vec![]);
    newer.title = "Renamed".into();
    newer.updated_at = 5.0;
    core.push(ScriptureFeed::Plans(vec![plan("p2", vec![]), newer]));
    draw(cx, &opened);
    let titles = read(cx, &opened, |m| {
        m.plans()
            .iter()
            .map(|p| p.title.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(titles, vec!["Plan p2", "Renamed"]);
    assert!(exists(cx, &opened, &ids::plan("p2")));
}

/// `scripture-lazy-cards.integration.cjs`: 54 items × 7 verses. Only the
/// cards near the viewport are built; scrolling builds more; every card
/// keeps its index.
#[gpui_kit::test]
fn the_grid_builds_only_what_is_on_screen(cx: &mut TestAppContext) {
    let items: Vec<_> = (0..54)
        .map(|i| {
            item(
                &format!("item-{i}"),
                &passage(
                    &format!("John {}:1-7", i + 1),
                    "John",
                    (i + 1) as f64,
                    &[1., 2., 3., 4., 5., 6., 7.],
                    "KJV",
                ),
            )
        })
        .collect();
    let mut snap = base();
    snap.plans = vec![plan("big", items)];
    let core = Core::new(snap);
    let opened = open(cx, core);
    click(cx, &opened, &ids::plan("big"));
    assert_eq!(read(cx, &opened, rows_len), 378);
    let drawn = |cx: &mut TestAppContext| {
        (0..378)
            .filter(|i| exists(cx, &opened, &ids::card(*i)))
            .count()
    };
    let initial = drawn(cx);
    assert!(initial > 0, "the cards on screen are drawn");
    assert!(
        initial < 378 / 4,
        "open defers off-screen slides (drew {initial}/378)"
    );
    // Jumping to the last item builds its cards.
    cx.update(|cx| {
        opened
            .model
            .update(cx, |m, cx| m.select_plan_item("item-53", cx))
    });
    draw(cx, &opened);
    assert!(exists(cx, &opened, &ids::card(377)));
    assert!(!exists(cx, &opened, &ids::card(0)));
}

/// `scripture-grid.bench` equivalent: frame time to show a 360-card playlist
/// (60 items × 6 verses) after it opens. Run with
/// `cargo test -p ui-scripture --release -- --ignored --nocapture grid_bench`.
#[gpui_kit::test]
#[ignore]
fn grid_bench(cx: &mut TestAppContext) {
    let items: Vec<_> = (0..60)
        .map(|i| {
            item(
                &format!("b-{i}"),
                &passage(
                    &format!("John {}:1-6", i + 1),
                    "John",
                    (i + 1) as f64,
                    &[1., 2., 3., 4., 5., 6.],
                    "KJV",
                ),
            )
        })
        .collect();
    let mut snap = base();
    snap.plans = vec![plan("bench", items)];
    let opened = open(cx, Core::new(snap));
    let mut times = Vec::new();
    for _ in 0..15 {
        cx.update(|cx| opened.model.update(cx, |m, cx| m.clear_results(cx)));
        draw(cx, &opened);
        cx.update(|cx| opened.model.update(cx, |m, cx| m.open_plan("bench", cx)));
        cx.run_until_parked();
        let started = std::time::Instant::now();
        cx.update_window(opened.handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    assert_eq!(read(cx, &opened, rows_len), 360);
    eprintln!(
        "grid_bench: 360 cards, first frame median {:.2} ms, min {:.2}, max {:.2}",
        times[times.len() / 2],
        times[0],
        times[times.len() - 1]
    );
}
