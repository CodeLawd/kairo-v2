//! UI integration tests for the Documents page (prompt 085), driven by a
//! scripted core: the empty state, import progress, cancel, failure and
//! retry, the converter offer, pushing a page, preview only, clicker paging,
//! the slideshow (timed, looping, waiting for video, stopped by a video
//! error), video control, rename, remove, settings, the slide width and a
//! 500-page deck.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Focusable, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};
use kairo_domain::documents::{
    DocumentPlaybackStatus, DocumentPrepareOptions, DocumentVideoCommand, ProjectionDocument,
    SlideConverterStatus,
};
use serde_json::{Value, json};
use ui_documents::backend::{
    DocumentsAnswer, DocumentsBackend, DocumentsCall, DocumentsFeed, DocumentsSnapshot, FeedSink,
    ImportKind, ImportOutcome, ImportReply, ImportStep, Listening, ProgressSink, Reply,
};
use ui_documents::{DocumentsEvent, DocumentsPage, ids};

struct Import {
    kind: Option<ImportKind>,
    options: DocumentPrepareOptions,
    cancel: Arc<AtomicBool>,
    progress: ProgressSink,
    reply: Option<ImportReply>,
}

struct State {
    documents: Vec<ProjectionDocument>,
    calls: Vec<DocumentsCall>,
    imports: Vec<Import>,
    sink: Option<FeedSink>,
    applied: bool,
    playback: Option<DocumentPlaybackStatus>,
    ui: Option<Value>,
    writes: Vec<Value>,
    fail: Option<String>,
}

struct Core(Mutex<State>);

fn doc(id: &str, name: &str, pages: usize) -> ProjectionDocument {
    let pages: Vec<String> = (0..pages).map(|i| format!("/docs/{id}/{i}.png")).collect();
    serde_json::from_value(json!({"id": id, "name": name, "format": "pdf", "pages": pages}))
        .unwrap()
}

fn with_video(mut d: ProjectionDocument, page: usize) -> ProjectionDocument {
    let mut videos = vec![Vec::new(); d.pages.len()];
    videos[page] = vec![
        serde_json::from_value(json!({"file": "/v.mp4", "box": {"x": 0.1, "y": 0.1, "width": 0.5, "height": 0.5}, "start": 0}))
            .unwrap(),
    ];
    d.videos = Some(videos);
    d.format = kairo_domain::documents::DocumentFormat::Pptx;
    d
}

fn playback(id: &str, page: usize, ended: bool, error: Option<&str>) -> DocumentPlaybackStatus {
    serde_json::from_value(json!({"id": id, "page": page, "videos": [
        {"currentTime": 3, "duration": 10, "paused": false, "ended": ended, "visible": true, "error": error}
    ]}))
    .unwrap()
}

fn status(state: &str) -> SlideConverterStatus {
    serde_json::from_value(json!({"state": state, "version": "1", "sizeBytes": 314572800.0, "receivedBytes": 104857600.0, "otherConverter": null})).unwrap()
}

impl DocumentsBackend for Core {
    fn snapshot(&self) -> DocumentsSnapshot {
        DocumentsSnapshot::default()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: DocumentsCall, reply: Reply) {
        let mut s = self.0.lock().unwrap();
        s.calls.push(call.clone());
        if let Some(e) = s.fail.clone()
            && !matches!(call, DocumentsCall::List | DocumentsCall::Playback { .. })
        {
            drop(s);
            return reply(Err(e));
        }
        let answer = match call {
            DocumentsCall::List => DocumentsAnswer::Documents(s.documents.clone()),
            DocumentsCall::Push { .. } => DocumentsAnswer::Pushed { applied: s.applied },
            DocumentsCall::Rename { id, name } => {
                if let Some(d) = s.documents.iter_mut().find(|d| d.id == id) {
                    d.name = name;
                }
                DocumentsAnswer::Documents(s.documents.clone())
            }
            DocumentsCall::Remove(id) => {
                s.documents.retain(|d| d.id != id);
                DocumentsAnswer::Documents(s.documents.clone())
            }
            DocumentsCall::Playback { .. } => DocumentsAnswer::Playback(s.playback.clone()),
            DocumentsCall::Libraries(_) => DocumentsAnswer::Libraries(Default::default()),
            DocumentsCall::ConverterStatus => DocumentsAnswer::Converter(status("missing")),
            _ => DocumentsAnswer::Done,
        };
        drop(s);
        reply(Ok(answer));
    }
    fn import(
        &self,
        kind: Option<ImportKind>,
        options: DocumentPrepareOptions,
        cancel: Arc<AtomicBool>,
        progress: ProgressSink,
        reply: ImportReply,
    ) {
        self.0.lock().unwrap().imports.push(Import {
            kind,
            options,
            cancel,
            progress,
            reply: Some(reply),
        });
    }
    fn read_ui(&self, _: &str) -> Option<Value> {
        self.0.lock().unwrap().ui.clone()
    }
    fn write_ui(&self, _: &str, value: Value) {
        self.0.lock().unwrap().writes.push(value);
    }
}

impl Core {
    fn new(documents: Vec<ProjectionDocument>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(State {
            documents,
            calls: Vec::new(),
            imports: Vec::new(),
            sink: None,
            applied: true,
            playback: None,
            ui: None,
            writes: Vec::new(),
            fail: None,
        })))
    }
    fn calls(&self) -> Vec<DocumentsCall> {
        self.0.lock().unwrap().calls.clone()
    }
    fn pushes(&self) -> Vec<usize> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                DocumentsCall::Push { page, .. } => Some(page),
                _ => None,
            })
            .collect()
    }
    fn feed(&self, f: DocumentsFeed) {
        let sink = self.0.lock().unwrap().sink.clone().unwrap();
        sink(f);
    }
    fn step(&self, n: usize, step: ImportStep) {
        let p = self.0.lock().unwrap().imports[n].progress.clone();
        p(step);
    }
    fn finish(&self, n: usize, outcome: Result<ImportOutcome, String>) {
        let reply = self.0.lock().unwrap().imports[n].reply.take().unwrap();
        reply(outcome);
    }
    fn imports(&self) -> usize {
        self.0.lock().unwrap().imports.len()
    }
}

struct Opened {
    handle: AnyWindowHandle,
    page: Entity<DocumentsPage>,
    events: Arc<Mutex<Vec<DocumentsEvent>>>,
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<Entity<DocumentsPage>>>> = Default::default();
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
                    let p = cx.new(|cx| DocumentsPage::new(core, window, cx));
                    *out.lock().unwrap() = Some(p.clone());
                    p
                },
            )
        })
        .unwrap()
        .0;
    let page = slot.lock().unwrap().take().unwrap();
    let events: Arc<Mutex<Vec<DocumentsEvent>>> = Default::default();
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&page, move |_, e: &DocumentsEvent, _| {
            seen.lock().unwrap().push(e.clone())
        })
        .detach()
    });
    let o = Opened {
        handle,
        page,
        events,
    };
    draw(cx, &o);
    draw(cx, &o);
    o
}

fn draw(cx: &mut TestAppContext, o: &Opened) {
    cx.run_until_parked();
    cx.update_window(o.handle, |_, w, cx| {
        w.render_frame(cx);
        kairo_ui_kit::a11y_audit::check(w);
    })
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

fn act(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut DocumentsPage, &mut gpui_kit::Window, &mut gpui_kit::Context<DocumentsPage>),
) {
    let page = o.page.clone();
    cx.update_window(o.handle, move |_, w, cx| {
        page.update(cx, |p, cx| f(p, w, cx))
    })
    .unwrap();
    draw(cx, o);
}

fn read<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&DocumentsPage) -> R) -> R {
    cx.read(|cx| f(o.page.read(cx)))
}

fn press(cx: &mut TestAppContext, o: &Opened, key: &'static str) {
    let page = o.page.clone();
    cx.update_window(o.handle, move |_, w, cx| {
        page.read(cx).focus_handle(cx).focus(w, cx);
        w.press(key, cx);
    })
    .unwrap();
    draw(cx, o);
}

fn advance(cx: &mut TestAppContext, o: &Opened, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    draw(cx, o);
}

#[gpui_kit::test]
fn an_empty_library_offers_import(cx: &mut TestAppContext) {
    let core = Core::new(vec![]);
    let o = open(cx, core.clone());
    assert!(exists(cx, &o, ids::EMPTY));
    assert!(exists(cx, &o, ids::NO_DOCUMENTS));
    click(cx, &o, ids::IMPORT_EMPTY);
    assert_eq!(core.imports(), 1);
    assert_eq!(core.0.lock().unwrap().imports[0].kind, None);
    // The picker closed: nothing changes, nothing shouts.
    core.finish(0, Ok(ImportOutcome::NotPicked));
    draw(cx, &o);
    assert!(read(cx, &o, |p| p.import_progress().is_none()
        && p.error().is_none()));
    // File → Import → PDF opens the PDF picker.
    core.feed(DocumentsFeed::Import(ImportKind::Pdf));
    draw(cx, &o);
    assert_eq!(
        core.0.lock().unwrap().imports[1].kind,
        Some(ImportKind::Pdf)
    );
}

#[gpui_kit::test]
fn import_shows_progress_then_selects_the_new_document(cx: &mut TestAppContext) {
    let core = Core::new(vec![]);
    let o = open(cx, core.clone());
    click(cx, &o, ids::IMPORT);
    core.step(
        0,
        ImportStep::Preparing {
            name: "Easter".into(),
            format: Some(kairo_domain::documents::DocumentFormat::Pdf),
        },
    );
    draw(cx, &o);
    assert!(exists(cx, &o, ids::IMPORT_DIALOG));
    assert_eq!(
        read(cx, &o, |p| p.import_progress().unwrap().line(false)),
        "Preparing…"
    );
    core.step(
        0,
        ImportStep::Rendering {
            completed: 1,
            total: 3,
        },
    );
    draw(cx, &o);
    assert_eq!(
        read(cx, &o, |p| p.import_progress().unwrap().line(false)),
        "Page 2 of 3"
    );
    core.step(0, ImportStep::Finishing);
    draw(cx, &o);
    // Cancel is off while finishing.
    click(cx, &o, ids::IMPORT_CANCEL);
    assert!(
        !core.0.lock().unwrap().imports[0]
            .cancel
            .load(Ordering::SeqCst)
    );
    core.finish(
        0,
        Ok(ImportOutcome::Imported {
            id: "d1".into(),
            documents: vec![doc("d1", "Easter", 3)],
        }),
    );
    draw(cx, &o);
    assert!(!exists(cx, &o, ids::IMPORT_DIALOG));
    assert_eq!(
        read(cx, &o, |p| p.selected().map(str::to_owned)),
        Some("d1".into())
    );
    assert!(exists(cx, &o, &ids::slide(2)));
}

#[gpui_kit::test]
fn cancel_stops_between_pages_without_an_error(cx: &mut TestAppContext) {
    let core = Core::new(vec![]);
    let o = open(cx, core.clone());
    click(cx, &o, ids::IMPORT);
    core.step(
        0,
        ImportStep::Rendering {
            completed: 0,
            total: 9,
        },
    );
    draw(cx, &o);
    click(cx, &o, ids::IMPORT_CANCEL);
    assert!(
        core.0.lock().unwrap().imports[0]
            .cancel
            .load(Ordering::SeqCst)
    );
    assert_eq!(
        read(cx, &o, |p| p.import_progress().unwrap().line(true)),
        "Canceling…"
    );
    core.finish(0, Ok(ImportOutcome::Cancelled));
    draw(cx, &o);
    assert!(!exists(cx, &o, ids::IMPORT_DIALOG));
    assert!(read(cx, &o, |p| p.error().is_none()));
}

#[gpui_kit::test]
fn a_failed_import_offers_choose_again_or_close(cx: &mut TestAppContext) {
    let core = Core::new(vec![]);
    let o = open(cx, core.clone());
    core.feed(DocumentsFeed::Import(ImportKind::Powerpoint));
    draw(cx, &o);
    core.finish(
        0,
        Err("Error invoking remote method 'documents:prepare': Error: Documents can contain up to 500 pages.".into()),
    );
    draw(cx, &o);
    assert!(exists(cx, &o, ids::IMPORT_FAILED));
    assert_eq!(
        read(cx, &o, |p| p.import_progress().unwrap().error.clone()),
        Some("Documents can contain up to 500 pages.".into())
    );
    click(cx, &o, ids::IMPORT_RETRY);
    assert_eq!(core.imports(), 2);
    assert_eq!(
        core.0.lock().unwrap().imports[1].kind,
        Some(ImportKind::Powerpoint)
    );
    core.finish(1, Err("Invalid page image.".into()));
    draw(cx, &o);
    click(cx, &o, ids::IMPORT_CLOSE);
    assert!(!exists(cx, &o, ids::IMPORT_DIALOG));
}

#[gpui_kit::test]
fn the_converter_offer_downloads_then_resumes(cx: &mut TestAppContext) {
    let core = Core::new(vec![]);
    let o = open(cx, core.clone());
    click(cx, &o, ids::IMPORT);
    let needs = serde_json::from_value(
        json!({"needsConverter": true, "token": "tok", "name": "Deck", "format": "pptx"}),
    )
    .unwrap();
    core.finish(0, Ok(ImportOutcome::NeedsConverter(needs)));
    draw(cx, &o);
    assert!(exists(cx, &o, ids::CONVERTER));
    assert!(exists(cx, &o, ids::CONVERTER_SIMPLIFIED));
    click(cx, &o, ids::CONVERTER_DOWNLOAD);
    assert!(core.calls().contains(&DocumentsCall::ConverterInstall));
    core.feed(DocumentsFeed::Converter(status("downloading")));
    draw(cx, &o);
    assert!(exists(cx, &o, ids::CONVERTER_PROGRESS));
    assert!(!exists(cx, &o, ids::CONVERTER_SIMPLIFIED));
    core.feed(DocumentsFeed::Converter(status("ready")));
    draw(cx, &o);
    assert!(!exists(cx, &o, ids::CONVERTER));
    assert_eq!(core.imports(), 2);
    assert_eq!(
        core.0.lock().unwrap().imports[1].options.resume.as_deref(),
        Some("tok")
    );
}

#[gpui_kit::test]
fn the_converter_offer_imports_simplified_or_stops(cx: &mut TestAppContext) {
    let core = Core::new(vec![]);
    let o = open(cx, core.clone());
    click(cx, &o, ids::IMPORT);
    let needs: kairo_domain::documents::DocumentNeedsConverter = serde_json::from_value(
        json!({"needsConverter": true, "token": "tok", "name": "Deck", "format": "pptx"}),
    )
    .unwrap();
    core.finish(0, Ok(ImportOutcome::NeedsConverter(needs.clone())));
    draw(cx, &o);
    click(cx, &o, ids::CONVERTER_SIMPLIFIED);
    let options = core.0.lock().unwrap().imports[1].options.clone();
    assert_eq!(options.simplified, Some(true));
    assert_eq!(options.resume.as_deref(), Some("tok"));
    core.finish(1, Ok(ImportOutcome::NeedsConverter(needs)));
    draw(cx, &o);
    click(cx, &o, ids::CONVERTER_DOWNLOAD);
    core.feed(DocumentsFeed::Converter(status("downloading")));
    draw(cx, &o);
    click(cx, &o, ids::CONVERTER_CANCEL);
    assert!(core.calls().contains(&DocumentsCall::ConverterCancel));
    assert!(!exists(cx, &o, ids::CONVERTER));
    // An old .ppt cannot be simplified; an unsupported computer is told why.
    click(cx, &o, ids::IMPORT);
    let ppt = serde_json::from_value(
        json!({"needsConverter": true, "token": "t2", "name": "Old", "format": "ppt"}),
    )
    .unwrap();
    core.finish(2, Ok(ImportOutcome::NeedsConverter(ppt)));
    draw(cx, &o);
    assert!(!exists(cx, &o, ids::CONVERTER_SIMPLIFIED));
    core.feed(DocumentsFeed::Converter(status("unsupported")));
    draw(cx, &o);
    assert!(!exists(cx, &o, ids::CONVERTER_DOWNLOAD));
}

#[gpui_kit::test]
fn pushing_a_page_marks_it_live_and_preview_only_says_so(cx: &mut TestAppContext) {
    let core = Core::new(vec![doc("a", "Announcements", 4)]);
    let o = open(cx, core.clone());
    click(cx, &o, &ids::document("a"));
    click(cx, &o, &ids::slide(2));
    assert_eq!(core.pushes(), [2]);
    assert_eq!(
        read(cx, &o, |p| p.live().map(|(i, n)| (i.to_owned(), n))),
        Some(("a".into(), 2))
    );
    assert_eq!(
        o.events.lock().unwrap().last(),
        Some(&DocumentsEvent::Pushed {
            label: "Announcements · Page 3".into(),
            applied: true
        })
    );
    assert!(!exists(cx, &o, ids::PREVIEW));
    core.0.lock().unwrap().applied = false;
    click(cx, &o, &ids::slide(3));
    assert!(exists(cx, &o, ids::PREVIEW_ONLY));
    assert_eq!(
        read(cx, &o, |p| p.status().to_owned()),
        "Preview only · No output connected"
    );
    // The output went to something else.
    core.feed(DocumentsFeed::LiveCleared);
    draw(cx, &o);
    assert!(read(cx, &o, |p| p.live().is_none()));
    assert!(!exists(cx, &o, ids::PREVIEW));
    // A failed push shows the reason.
    core.0.lock().unwrap().fail = Some("Document page not found.".into());
    click(cx, &o, &ids::slide(0));
    assert_eq!(
        read(cx, &o, |p| p.error().map(str::to_owned)),
        Some("Document page not found.".into())
    );
}

#[gpui_kit::test]
fn a_clicker_pages_forward_and_back(cx: &mut TestAppContext) {
    let core = Core::new(vec![doc("a", "Talk", 3)]);
    let o = open(cx, core.clone());
    click(cx, &o, &ids::document("a"));
    press(cx, &o, "pagedown");
    press(cx, &o, "right");
    assert_eq!(core.pushes(), [1, 2]);
    // Past the end does nothing.
    press(cx, &o, "pagedown");
    assert_eq!(core.pushes(), [1, 2]);
    press(cx, &o, "pageup");
    assert_eq!(core.pushes(), [1, 2, 1]);
}

#[gpui_kit::test]
fn the_slideshow_advances_loops_and_restarts_on_a_manual_push(cx: &mut TestAppContext) {
    let core = Core::new(vec![doc("a", "Loop", 3)]);
    let o = open(cx, core.clone());
    click(cx, &o, &ids::document("a"));
    act(cx, &o, |p, w, cx| p.set_slideshow_seconds(5., w, cx));
    click(cx, &o, ids::SLIDESHOW);
    assert!(read(cx, &o, |p| p.playing()));
    assert_eq!(read(cx, &o, |p| p.countdown()), 5);
    advance(cx, &o, 2_000);
    assert_eq!(read(cx, &o, |p| p.countdown()), 3);
    // A manual push restarts the dwell.
    click(cx, &o, &ids::slide(1));
    assert_eq!(read(cx, &o, |p| p.countdown()), 5);
    advance(cx, &o, 5_000);
    assert_eq!(core.pushes(), [1, 2]);
    advance(cx, &o, 5_000);
    assert_eq!(core.pushes(), [1, 2, 0], "loops back to page 1");
    // Without looping it stops on the last page.
    act(cx, &o, |p, _, cx| {
        p.push(2, cx);
    });
    click(cx, &o, ids::SLIDESHOW_SETTINGS);
    click(cx, &o, ids::LOOP);
    assert!(!read(cx, &o, |p| p.settings().slideshow_loop));
    advance(cx, &o, 5_000);
    assert!(!read(cx, &o, |p| p.playing()));
    assert_eq!(core.pushes(), [1, 2, 0, 2]);
}

#[gpui_kit::test]
fn slideshow_settings_save_and_auto_start(cx: &mut TestAppContext) {
    let core = Core::new(vec![doc("a", "Talk", 3)]);
    let o = open(cx, core.clone());
    click(cx, &o, &ids::document("a"));
    click(cx, &o, ids::SLIDESHOW_SETTINGS);
    assert!(exists(cx, &o, ids::SLIDESHOW_MENU));
    click(cx, &o, &ids::preset(30));
    click(cx, &o, ids::AUTO_START);
    let saved: Vec<_> = core
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            DocumentsCall::SaveSettings(s) => Some((s.slideshow_sec, s.slideshow_auto_start)),
            _ => None,
        })
        .collect();
    assert_eq!(saved, [(30., false), (30., true)]);
    // Out of range is clamped as the store does.
    act(cx, &o, |p, w, cx| p.set_slideshow_seconds(1., w, cx));
    assert_eq!(read(cx, &o, |p| p.settings().slideshow_sec), 2.);
    act(cx, &o, |p, _, cx| p.push(0, cx));
    assert!(read(cx, &o, |p| p.playing()), "starts on the first push");
}

#[gpui_kit::test]
fn a_video_slide_waits_for_playback_and_an_error_stops_the_run(cx: &mut TestAppContext) {
    let core = Core::new(vec![with_video(doc("v", "Video deck", 3), 1)]);
    let o = open(cx, core.clone());
    click(cx, &o, &ids::document("v"));
    // No videos on this page: nothing is read.
    advance(cx, &o, 1_000);
    assert_eq!(read(cx, &o, |p| p.polls()), 0);
    core.0.lock().unwrap().playback = Some(playback("v", 1, false, None));
    click(cx, &o, &ids::slide(1));
    advance(cx, &o, 400);
    assert!(exists(cx, &o, &ids::video(0)));
    click(cx, &o, ids::SLIDESHOW);
    advance(cx, &o, 20_000);
    assert_eq!(core.pushes(), [1], "waits while the video plays");
    let polls = read(cx, &o, |p| p.polls());
    assert!(
        (50..=62).contains(&polls),
        "about one read per 350 ms: {polls}"
    );
    // Controls go to the core.
    click(cx, &o, &ids::video_toggle(0));
    click(cx, &o, &ids::video_forward(0));
    let controls: Vec<_> = core
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            DocumentsCall::Control { command, .. } => Some(command),
            _ => None,
        })
        .collect();
    assert_eq!(
        controls,
        [
            DocumentVideoCommand::Pause,
            DocumentVideoCommand::Seek { seconds: 8. }
        ]
    );
    core.0.lock().unwrap().playback = Some(playback("v", 1, true, None));
    advance(cx, &o, 400);
    assert_eq!(core.pushes(), [1, 2], "advances when the video ends");
    // Back to the video, which now fails: the run stops.
    core.0.lock().unwrap().playback = Some(playback("v", 1, false, Some("Could not play")));
    act(cx, &o, |p, _, cx| p.push(1, cx));
    advance(cx, &o, 400);
    assert!(!read(cx, &o, |p| p.playing()));
}

#[gpui_kit::test]
fn a_video_slide_not_yet_pushed_says_to_push_it(cx: &mut TestAppContext) {
    let core = Core::new(vec![with_video(doc("v", "Video deck", 2), 0)]);
    let o = open(cx, core.clone());
    click(cx, &o, &ids::document("v"));
    advance(cx, &o, 400);
    assert!(exists(cx, &o, ids::PUSH_FOR_VIDEO));
}

#[gpui_kit::test]
fn rename_saves_cancels_and_rejects_bad_names(cx: &mut TestAppContext) {
    let core = Core::new(vec![doc("a", "Old", 1), doc("b", "Other", 1)]);
    let o = open(cx, core.clone());
    act(cx, &o, |p, w, cx| p.start_rename("a", w, cx));
    assert!(exists(cx, &o, ids::RENAME));
    cx.update_window(o.handle, |_, w, cx| {
        w.press("secondary-a", cx);
        w.input("New name", cx);
        w.press("enter", cx);
    })
    .unwrap();
    draw(cx, &o);
    assert!(core.calls().contains(&DocumentsCall::Rename {
        id: "a".into(),
        name: "New name".into()
    }));
    assert_eq!(read(cx, &o, |p| p.documents()[0].name.clone()), "New name");
    assert!(read(cx, &o, |p| p.renaming().is_none()));
    // Escape keeps the old name.
    act(cx, &o, |p, w, cx| p.start_rename("b", w, cx));
    cx.update_window(o.handle, |_, w, cx| {
        w.press("secondary-a", cx);
        w.input("Nope", cx);
        w.press("escape", cx);
    })
    .unwrap();
    draw(cx, &o);
    assert!(read(cx, &o, |p| p.renaming().is_none()));
    assert!(
        !core
            .calls()
            .iter()
            .any(|c| matches!(c, DocumentsCall::Rename { name, .. } if name == "Nope"))
    );
    // A slash is refused before the round trip.
    act(cx, &o, |p, w, cx| p.start_rename("b", w, cx));
    cx.update_window(o.handle, |_, w, cx| {
        w.press("secondary-a", cx);
        w.input("a/b", cx);
        w.press("enter", cx);
    })
    .unwrap();
    draw(cx, &o);
    assert_eq!(
        read(cx, &o, |p| p.error().map(str::to_owned)),
        Some("Document names cannot include slashes.".into())
    );
}

#[gpui_kit::test]
fn remove_moves_to_the_next_document(cx: &mut TestAppContext) {
    let core = Core::new(vec![doc("a", "A", 2), doc("b", "B", 2)]);
    let o = open(cx, core.clone());
    click(cx, &o, &ids::document("a"));
    click(cx, &o, &ids::slide(1));
    click(cx, &o, ids::REMOVE);
    assert!(core.calls().contains(&DocumentsCall::Remove("a".into())));
    assert_eq!(
        read(cx, &o, |p| p.selected().map(str::to_owned)),
        Some("b".into())
    );
    assert!(read(cx, &o, |p| p.live().is_none()));
    click(cx, &o, ids::REMOVE);
    assert!(exists(cx, &o, ids::EMPTY));
    assert!(exists(cx, &o, ids::NO_DOCUMENTS));
}

#[gpui_kit::test]
fn the_slide_width_is_read_back_and_saved(cx: &mut TestAppContext) {
    let core = Core::new(vec![doc("a", "A", 2)]);
    core.0.lock().unwrap().ui = Some(json!(300));
    let o = open(cx, core.clone());
    assert_eq!(read(cx, &o, |p| p.thumb()), 300.);
    act(cx, &o, |p, _, cx| p.set_slide_width(373., cx));
    assert_eq!(read(cx, &o, |p| p.thumb()), 370.);
    assert_eq!(core.0.lock().unwrap().writes, [json!(370.0)]);
}

#[gpui_kit::test]
fn a_500_page_deck_draws_only_the_visible_rows(cx: &mut TestAppContext) {
    let core = Core::new(vec![doc("big", "Big", 500)]);
    let o = open(cx, core.clone());
    let started = Instant::now();
    click(cx, &o, &ids::document("big"));
    let first = started.elapsed();
    assert!(exists(cx, &o, &ids::slide(0)));
    assert!(!exists(cx, &o, &ids::slide(499)));
    let drawn = (0..500).filter(|&i| exists(cx, &o, &ids::slide(i))).count();
    assert!(drawn < 60, "drew {drawn} of 500");
    // Paging to the end scrolls the live slide into view.
    act(cx, &o, |p, _, cx| p.push(499, cx));
    draw(cx, &o);
    assert!(exists(cx, &o, &ids::slide(499)));
    eprintln!("500 pages: open+draw {first:?}, {drawn} slides drawn");
}

// ─── The preview's embedded videos (`DocumentSlidePreview`) ──────────────

struct Clip {
    t: f64,
}

impl ui_media::backend::PreviewSource for Clip {
    fn next_frame(&mut self) -> Result<Option<ui_media::backend::Frame>, String> {
        if self.t >= 2.0 {
            return Ok(None);
        }
        let f = ui_media::backend::Frame {
            width: 4,
            height: 2,
            pixels: vec![120; 32],
            seconds: self.t,
        };
        self.t += 1. / 30.;
        Ok(Some(f))
    }
    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        self.t = seconds;
        Ok(())
    }
    fn duration(&self) -> Option<f64> {
        Some(2.0)
    }
    fn interval(&self) -> f64 {
        1. / 30.
    }
}

#[derive(Default)]
struct Media {
    opened: Mutex<Vec<String>>,
}

impl ui_media::MediaBackend for Media {
    fn library(&self) -> kairo_domain::media::MediaLibrary {
        serde_json::from_value(json!({"folder": "", "folders": [], "items": [], "playlists": [], "playback": {}, "liveItemId": null}))
            .unwrap()
    }
    fn listen(&self, _: ui_media::backend::FeedSink) -> ui_media::backend::Listening {
        Box::new(())
    }
    fn call(&self, _: ui_media::backend::MediaCall, _: ui_media::backend::Reply) {}
    fn open_preview(
        &self,
        path: &str,
        _: u32,
    ) -> Result<Box<dyn ui_media::backend::PreviewSource>, String> {
        self.opened.lock().unwrap().push(path.to_owned());
        Ok(Box::new(Clip { t: 0. }))
    }
    fn read_ui(&self, _: &str) -> Option<Value> {
        None
    }
    fn write_ui(&self, _: &str, _: Value) {}
}

fn with_clip(d: ProjectionDocument, page: usize) -> ProjectionDocument {
    let mut d = with_video(d, page);
    d.videos.as_mut().unwrap()[page][0].file = "clip.mp4".into();
    d
}

fn settle(cx: &mut TestAppContext, o: &Opened) {
    for _ in 0..5 {
        advance(cx, o, 50);
    }
}

#[gpui_kit::test]
fn with_no_output_the_preview_plays_the_slide_videos_itself(cx: &mut TestAppContext) {
    let core = Core::new(vec![with_clip(doc("a", "Promo", 3), 1)]);
    core.0.lock().unwrap().applied = false;
    let o = open(cx, core.clone());
    let media = Arc::new(Media::default());
    act(cx, &o, |p, _, cx| p.set_media(Some(media.clone()), cx));
    click(cx, &o, &ids::document("a"));
    click(cx, &o, &ids::slide(1));
    settle(cx, &o);
    assert!(read(cx, &o, |p| p.local_preview()));
    assert_eq!(*media.opened.lock().unwrap(), ["/docs/a/media/clip.mp4"]);
    // Waiting at the clip start, hidden, with the transport offered.
    let first = read(cx, &o, |p| p.playback().cloned()).expect("local playback");
    assert!(first.videos[0].paused && !first.videos[0].visible);
    assert!(exists(cx, &o, &ids::video_toggle(0)));
    // Play: shown and running, and nothing goes to the outputs.
    act(cx, &o, |p, _, cx| {
        p.control(0, DocumentVideoCommand::Play, cx)
    });
    settle(cx, &o);
    let playing = read(cx, &o, |p| p.playback().cloned()).unwrap();
    assert!(!playing.videos[0].paused && playing.videos[0].visible);
    assert_eq!(read(cx, &o, |p| p.clips_shown()), [true]);
    act(cx, &o, |p, _, cx| {
        p.control(0, DocumentVideoCommand::Pause, cx)
    });
    settle(cx, &o);
    assert!(read(cx, &o, |p| p.playback().unwrap().videos[0].paused));
    assert!(!core.calls().iter().any(|c| matches!(
        c,
        DocumentsCall::Control { .. } | DocumentsCall::Playback { .. }
    )));
    // Another deck: the players close.
    core.0.lock().unwrap().documents.push(doc("b", "Other", 2));
    let all = core.0.lock().unwrap().documents.clone();
    core.feed(DocumentsFeed::Documents(all));
    act(cx, &o, |p, _, cx| p.open_document("b", cx));
    draw(cx, &o);
    assert!(read(cx, &o, |p| p.clips_shown()).is_empty());
}

#[gpui_kit::test]
fn the_preview_follows_the_outputs_videos(cx: &mut TestAppContext) {
    let core = Core::new(vec![with_clip(doc("a", "Promo", 3), 1)]);
    let o = open(cx, core.clone());
    let media = Arc::new(Media::default());
    act(cx, &o, |p, _, cx| p.set_media(Some(media.clone()), cx));
    click(cx, &o, &ids::document("a"));
    click(cx, &o, &ids::slide(1));
    settle(cx, &o);
    // Not yet played on the outputs: hidden.
    assert_eq!(read(cx, &o, |p| p.clips_shown()), [false]);
    core.0.lock().unwrap().playback = Some(playback("a", 1, false, None));
    advance(cx, &o, 400);
    assert_eq!(read(cx, &o, |p| p.clips_shown()), [true]);
    // An error on the outputs hides it again.
    core.0.lock().unwrap().playback = Some(playback("a", 1, false, Some("Bad")));
    advance(cx, &o, 400);
    assert_eq!(read(cx, &o, |p| p.clips_shown()), [false]);
}
