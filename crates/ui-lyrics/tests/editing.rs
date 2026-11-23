//! UI integration tests for lyrics editing and import (prompt 082), driven
//! by a scripted core: the song editor (fields, typing in Yoruba, IME,
//! split and join at the caret, structured paste, sections, its own undo,
//! spell-check marks, save, cancel and the discard question), Translate to
//! English, and Import Song (online, files with duplicates, paste).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, ClipboardItem, Entity, EntityInputHandler, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};
use kairo_domain::lyrics::{
    DuplicateMatch, DuplicateReason, LyricsImportPreview, LyricsImportSource, LyricsOnlineResult,
    LyricsSectionType, LyricsSong,
};
use serde_json::json;
use ui_lyrics::backend::{
    FeedSink, Listening, LyricsAnswer, LyricsBackend, LyricsCall, LyricsFeed, LyricsSnapshot,
    Reply, SongFile,
};
use ui_lyrics::editor::SongEditor;
use ui_lyrics::import::{self, FileAction, ImportDialog, Tab};
use ui_lyrics::model::Field;
use ui_lyrics::view::ids;
use ui_lyrics::{Lyrics, LyricsEvent, LyricsWorkspace};

type Script = Box<dyn Fn(&LyricsCall) -> Option<Result<LyricsAnswer, String>> + Send>;

#[derive(Default)]
struct State {
    snapshot: LyricsSnapshot,
    calls: Vec<LyricsCall>,
    sink: Option<FeedSink>,
    script: Option<Script>,
}

struct Core(Mutex<State>);

fn imported(title: &str, text: &str) -> LyricsSong {
    let sections = ui_lyrics::import::marked_text_sections(text);
    serde_json::from_value(json!({
        "id": format!("new-{title}"), "title": title, "artist": "",
        "sections": sections, "createdAt": 9.0, "updatedAt": 9.0,
    }))
    .unwrap()
}

impl LyricsBackend for Core {
    fn snapshot(&self) -> LyricsSnapshot {
        self.0.lock().unwrap().snapshot.clone()
    }
    fn listen(&self, sink: FeedSink) -> Listening {
        self.0.lock().unwrap().sink = Some(sink);
        Box::new(())
    }
    fn call(&self, call: LyricsCall, reply: Reply) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(call.clone());
        let scripted = state.script.as_ref().and_then(|s| s(&call));
        drop(state);
        reply(scripted.unwrap_or_else(|| {
            Ok(match &call {
                LyricsCall::Update(s) => LyricsAnswer::Song(s.clone()),
                LyricsCall::Import(LyricsImportSource::Text { title, text, .. })
                | LyricsCall::Import(LyricsImportSource::ProPresenter { title, text, .. }) => {
                    LyricsAnswer::Song(imported(title, text))
                }
                LyricsCall::Import(LyricsImportSource::Usr { filename, .. }) => LyricsAnswer::Song(
                    imported(filename.as_deref().unwrap_or("usr"), "[Verse 1]\nusr"),
                ),
                LyricsCall::Import(LyricsImportSource::Online { title, .. }) => {
                    LyricsAnswer::Song(imported(title, "[Verse 1]\nweb"))
                }
                LyricsCall::ImportBatch(sources) => LyricsAnswer::ImportBatch(
                    sources
                        .iter()
                        .map(|s| {
                            Ok(match s {
                                LyricsImportSource::Text { title, text, .. }
                                | LyricsImportSource::ProPresenter { title, text, .. } => {
                                    imported(title, text)
                                }
                                LyricsImportSource::Usr { filename, .. } => {
                                    imported(filename.as_deref().unwrap_or("usr"), "[Verse 1]\nusr")
                                }
                                LyricsImportSource::Online { title, .. } => {
                                    imported(title, "[Verse 1]\nweb")
                                }
                                _ => imported("song", "[Verse 1]\nx"),
                            })
                        })
                        .collect(),
                ),
                LyricsCall::PreviewFile(source) => {
                    let (title, text) = match source {
                        LyricsImportSource::Text { title, text, .. }
                        | LyricsImportSource::ProPresenter { title, text, .. } => {
                            (title.clone(), text.clone())
                        }
                        _ => ("usr".into(), "x".into()),
                    };
                    LyricsAnswer::ImportPreview(LyricsImportPreview {
                        song: imported(&title, &text),
                        duplicate: (title == "Amazing Grace").then(|| DuplicateMatch {
                            song_id: "a".into(),
                            title: "Amazing Grace".into(),
                            artist: "John Newton".into(),
                            reason: DuplicateReason::TitleArtist,
                        }),
                    })
                }
                LyricsCall::PreviewFiles(sources) => LyricsAnswer::ImportPreviews(
                    sources
                        .iter()
                        .map(|source| {
                            let (title, text) = match source {
                                LyricsImportSource::Text { title, text, .. }
                                | LyricsImportSource::ProPresenter { title, text, .. } => {
                                    (title.clone(), text.clone())
                                }
                                _ => ("usr".into(), "x".into()),
                            };
                            Ok(LyricsImportPreview {
                                song: imported(&title, &text),
                                duplicate: (title == "Amazing Grace").then(|| DuplicateMatch {
                                    song_id: "a".into(),
                                    title: "Amazing Grace".into(),
                                    artist: "John Newton".into(),
                                    reason: DuplicateReason::TitleArtist,
                                }),
                            })
                        })
                        .collect(),
                ),
                LyricsCall::Translate { sections, .. } => LyricsAnswer::Sections(
                    sections
                        .iter()
                        .cloned()
                        .map(|mut s| {
                            s.lines = s
                                .lines
                                .iter()
                                .flat_map(|l| [l.clone(), format!("({l} in English)")])
                                .collect();
                            s
                        })
                        .collect(),
                ),
                LyricsCall::SearchOnline(_) => LyricsAnswer::Online(vec![LyricsOnlineResult {
                    lyrics_available: None,
                    id: "w1".into(),
                    provider: kairo_domain::lyrics::LyricsProvider::Lrclib,
                    title: "Way Maker".into(),
                    artist: "Sinach".into(),
                    url: "https://example.test/w1".into(),
                    album: None,
                    release_year: None,
                    snippet: None,
                    existing_song_id: None,
                }]),
                _ => LyricsAnswer::Done,
            })
        }));
    }
    fn misspelled(&self, text: &str) -> Vec<std::ops::Range<usize>> {
        text.match_indices("wrld")
            .map(|(i, w)| i..i + w.len())
            .collect()
    }
}

impl Core {
    fn new(snapshot: LyricsSnapshot) -> Arc<Self> {
        Arc::new(Self(Mutex::new(State {
            snapshot,
            ..Default::default()
        })))
    }
    fn script(
        &self,
        f: impl Fn(&LyricsCall) -> Option<Result<LyricsAnswer, String>> + Send + 'static,
    ) {
        self.0.lock().unwrap().script = Some(Box::new(f));
    }
    fn calls(&self) -> Vec<LyricsCall> {
        self.0.lock().unwrap().calls.clone()
    }
    fn push(&self, feed: LyricsFeed) {
        let sink = self.0.lock().unwrap().sink.clone().unwrap();
        sink(feed);
    }
}

fn song(id: &str, title: &str) -> LyricsSong {
    serde_json::from_value(json!({
        "id": id, "title": title, "artist": "John Newton",
        "sections": [
            {"type": "verse", "label": "Verse 1", "lines": ["Ẹ ṣé o Olúwa", "Ọba mi", "", "third line"]},
            {"type": "chorus", "label": "Chorus", "lines": ["sing"]}
        ],
        "createdAt": 1.0, "updatedAt": 1.0,
    }))
    .unwrap()
}

fn base() -> LyricsSnapshot {
    LyricsSnapshot {
        songs: vec![song("a", "Amazing Grace"), song("b", "Blessed")],
        gloss_color: "#AABED7".into(),
        ..Default::default()
    }
}

struct Opened {
    handle: AnyWindowHandle,
    page: Entity<LyricsWorkspace>,
    model: Entity<Lyrics>,
    events: Arc<Mutex<Vec<LyricsEvent>>>,
}

fn open(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let slot: Arc<Mutex<Option<Entity<LyricsWorkspace>>>> = Default::default();
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
                    let page = cx.new(|cx| LyricsWorkspace::new(core, window, cx));
                    *out.lock().unwrap() = Some(page.clone());
                    page
                },
            )
        })
        .expect("open")
        .0;
    let page = slot.lock().unwrap().take().unwrap();
    let model = cx.read(|cx| page.read(cx).model().clone());
    let events: Arc<Mutex<Vec<LyricsEvent>>> = Default::default();
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&model, move |_, e: &LyricsEvent, _| {
            seen.lock().unwrap().push(e.clone())
        })
        .detach()
    });
    let o = Opened {
        handle,
        page,
        model,
        events,
    };
    draw(cx, &o);
    o
}

fn draw(cx: &mut TestAppContext, o: &Opened) {
    cx.run_until_parked();
    cx.update_window(o.handle, |_, window, cx| {
        window.render_frame(cx);
        kairo_ui_kit::a11y_audit::check(window);
    })
    .unwrap();
    cx.run_until_parked();
}

fn exists(cx: &mut TestAppContext, o: &Opened, id: &str) -> bool {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(o.handle, move |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

fn click(cx: &mut TestAppContext, o: &Opened, id: &str) {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(o.handle, move |_, window, cx| window.click(id, cx))
        .unwrap();
    draw(cx, o);
}

fn press(cx: &mut TestAppContext, o: &Opened, key: &'static str) {
    cx.update_window(o.handle, |_, window, cx| window.press(key, cx))
        .unwrap();
    draw(cx, o);
}

fn act(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut Lyrics, &mut gpui_kit::Context<Lyrics>),
) {
    cx.update(|cx| o.model.update(cx, f));
    draw(cx, o);
}

fn read<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&Lyrics) -> R) -> R {
    cx.read(|cx| f(o.model.read(cx)))
}

fn editor(cx: &mut TestAppContext, o: &Opened) -> Entity<SongEditor> {
    cx.read(|cx| o.page.read(cx).editor().cloned().expect("editor open"))
}

fn ed<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&SongEditor) -> R) -> R {
    let e = editor(cx, o);
    cx.read(|cx| f(e.read(cx)))
}

fn ed_do(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut SongEditor, &mut gpui_kit::Window, &mut gpui_kit::Context<SongEditor>) + 'static,
) {
    let e = editor(cx, o);
    cx.update_window(o.handle, move |_, window, cx| {
        e.update(cx, |e, cx| f(e, window, cx))
    })
    .unwrap();
    draw(cx, o);
}

fn slides(cx: &mut TestAppContext, o: &Opened) -> Vec<(String, Vec<String>)> {
    ed(cx, o, |e| {
        e.doc()
            .blocks()
            .iter()
            .map(|b| {
                (
                    b.label.clone(),
                    b.slides.iter().map(|s| s.text.clone()).collect(),
                )
            })
            .collect()
    })
}

fn slide_key(cx: &mut TestAppContext, o: &Opened, block: usize, slide: usize) -> u64 {
    ed(cx, o, |e| e.doc().blocks()[block].slides[slide].key)
}

/// Puts the caret in a slide's field at `byte`.
fn caret(cx: &mut TestAppContext, o: &Opened, key: u64, byte: usize) {
    let e = editor(cx, o);
    cx.update_window(o.handle, move |_, window, cx| {
        let state = e.read(cx).slide_state(key).unwrap().clone();
        state.update(cx, |s, cx| {
            s.set_selected_range(byte..byte, cx);
            s.focus(window, cx);
        });
    })
    .unwrap();
    draw(cx, o);
}

fn type_text(cx: &mut TestAppContext, o: &Opened, text: &'static str) {
    cx.update_window(o.handle, move |_, w, cx| w.input(text, cx))
        .unwrap();
    draw(cx, o);
}

fn editing_sections(cx: &mut TestAppContext, o: &Opened) -> Vec<String> {
    read(cx, o, |m| {
        m.editing()
            .unwrap()
            .fields
            .sections
            .iter()
            .map(|s| s.lines_text.clone())
            .collect()
    })
}

fn open_editor(cx: &mut TestAppContext, core: Arc<Core>) -> Opened {
    let o = open(cx, core);
    act(cx, &o, |m, cx| m.select_song("a".into(), cx));
    click(cx, &o, ids::EDIT);
    o
}

// ─── The editor ─────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn edit_opens_the_song_as_sections_of_slides(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    assert!(exists(cx, &o, ui_lyrics::editor::ids::EDITOR));
    assert_eq!(
        slides(cx, &o),
        [
            (
                "Verse 1".into(),
                vec!["Ẹ ṣé o Olúwa\nỌba mi".into(), "third line".into()]
            ),
            ("Chorus".into(), vec!["sing".into()]),
        ]
    );
    assert!(!read(cx, &o, |m| m.editing().unwrap().dirty()));
}

#[gpui_kit::test]
fn typing_yoruba_and_ime_composition_reach_the_song(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    let key = slide_key(cx, &o, 1, 0);
    caret(cx, &o, key, "sing".len());
    type_text(cx, &o, " ẹ̀yin");
    let e = editor(cx, &o);
    cx.update_window(o.handle, move |_, window, cx| {
        let state = e.read(cx).slide_state(key).unwrap().clone();
        state.update(cx, |s, cx| {
            s.replace_and_mark_text_in_range(None, "め", None, window, cx);
            s.replace_and_mark_text_in_range(None, "恵み", None, window, cx);
            s.replace_text_in_range(None, "恵み", window, cx);
        });
    })
    .unwrap();
    draw(cx, &o);
    assert_eq!(editing_sections(cx, &o)[1], "sing ẹ̀yin恵み");
    assert!(read(cx, &o, |m| m.editing().unwrap().dirty()));
}

#[gpui_kit::test]
fn option_enter_splits_at_the_caret_and_backspace_joins(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    let key = slide_key(cx, &o, 0, 0);
    caret(cx, &o, key, "Ẹ ṣé o Olúwa\n".len());
    press(cx, &o, "alt-enter");
    assert_eq!(
        slides(cx, &o)[0].1,
        ["Ẹ ṣé o Olúwa", "Ọba mi", "third line"]
    );
    assert_eq!(
        editing_sections(cx, &o)[0],
        "Ẹ ṣé o Olúwa\n\nỌba mi\n\nthird line"
    );
    // The caret is at the start of the new slide: Backspace joins it back.
    press(cx, &o, "backspace");
    assert_eq!(slides(cx, &o)[0].1, ["Ẹ ṣé o Olúwa\nỌba mi", "third line"]);
    // Backspace at the start of a section's first slide does nothing special.
    caret(cx, &o, key, 0);
    press(cx, &o, "backspace");
    assert_eq!(slides(cx, &o)[0].1.len(), 2);
}

#[gpui_kit::test]
fn command_z_undoes_splits_and_typing_and_shift_redoes(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    let key = slide_key(cx, &o, 1, 0);
    caret(cx, &o, key, 4);
    type_text(cx, &o, "!");
    press(cx, &o, "cmd-enter");
    assert_eq!(slides(cx, &o)[1].1, ["sing!", ""]);
    press(cx, &o, "cmd-z");
    assert_eq!(slides(cx, &o)[1].1, ["sing!"]);
    press(cx, &o, "cmd-z");
    assert_eq!(slides(cx, &o)[1].1, ["sing"]);
    press(cx, &o, "cmd-shift-z");
    assert_eq!(slides(cx, &o)[1].1, ["sing!"]);
}

#[gpui_kit::test]
fn a_pasted_song_keeps_its_sections(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    let key = slide_key(cx, &o, 1, 0);
    caret(cx, &o, key, 4);
    let used = std::cell::Cell::new(false);
    ed_do(cx, &o, move |e, window, cx| {
        let _ = e.paste_into(key, "\n\n[Bridge]\nhigher\n\nstill", window, cx);
    });
    let _ = used;
    let t = slides(cx, &o);
    assert_eq!(
        t[2],
        ("Bridge".into(), vec!["higher".into(), "still".into()])
    );
    // Plain text is left to the field.
    let consumed = {
        let e = editor(cx, &o);
        let key2 = key;
        cx.update_window(o.handle, move |_, window, cx| {
            e.update(cx, |e, cx| e.paste_into(key2, "plain", window, cx))
        })
        .unwrap()
    };
    assert!(!consumed);
}

#[gpui_kit::test]
fn sections_change_type_move_and_split_from_a_slide(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    ed_do(cx, &o, |e, w, cx| {
        e.set_type(1, LyricsSectionType::Bridge, w, cx)
    });
    assert_eq!(slides(cx, &o)[1].0, "Bridge");
    ed_do(cx, &o, |e, w, cx| e.move_block(1, 0, w, cx));
    assert_eq!(slides(cx, &o)[0].0, "Bridge");
    let third = slide_key(cx, &o, 1, 1);
    ed_do(cx, &o, move |e, w, cx| {
        e.start_section_at(third, LyricsSectionType::Tag, w, cx)
    });
    assert_eq!(slides(cx, &o)[2], ("Tag".into(), vec!["third line".into()]));
    ed_do(cx, &o, |e, w, cx| {
        e.add_section(LyricsSectionType::Ending, w, cx)
    });
    assert_eq!(slides(cx, &o).last().unwrap().0, "Ending");
    ed_do(cx, &o, |e, w, cx| e.delete_block(0, w, cx));
    assert_eq!(slides(cx, &o)[0].0, "Verse 1");
    ed_do(cx, &o, |e, w, cx| e.undo(w, cx));
    assert_eq!(slides(cx, &o)[0].0, "Bridge");
}

#[gpui_kit::test]
fn misspelt_words_are_marked_after_typing_stops(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    let key = slide_key(cx, &o, 1, 0);
    caret(cx, &o, key, 4);
    type_text(cx, &o, " wrld");
    assert!(
        ed(cx, &o, |e| e.misspelled(key).is_empty()),
        "waits for the pause"
    );
    cx.executor().advance_clock(Duration::from_millis(350));
    draw(cx, &o);
    assert_eq!(ed(cx, &o, |e| e.misspelled(key).to_vec()), vec![5usize..9]);
}

#[gpui_kit::test]
fn save_writes_the_fields_and_sections_over_the_song(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open_editor(cx, core.clone());
    act(cx, &o, |m, cx| {
        m.set_edit_field(Field::Title, "  Amazing Grace (My Chains)  ".into(), cx)
    });
    act(cx, &o, |m, cx| {
        m.set_edit_field(Field::Ccli, "4768151".into(), cx)
    });
    press(cx, &o, "cmd-s");
    let saved = core
        .calls()
        .into_iter()
        .find_map(|c| match c {
            LyricsCall::Update(s) => Some(s),
            _ => None,
        })
        .expect("saved");
    assert_eq!(saved.id, "a");
    assert_eq!(saved.title, "Amazing Grace (My Chains)");
    assert_eq!(saved.ccli_number.as_deref(), Some("4768151"));
    assert_eq!(
        saved.sections[0].lines,
        ["Ẹ ṣé o Olúwa", "Ọba mi", "", "third line"]
    );
    assert!(read(cx, &o, |m| m.editing().is_none()));
    assert!(cx.read(|cx| o.page.read(cx).editor().is_none()));
}

#[gpui_kit::test]
fn a_failed_save_stays_in_the_editor_with_the_error(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| matches!(c, LyricsCall::Update(_)).then(|| Err("Error: disk full".into())));
    let o = open_editor(cx, core);
    click(cx, &o, ids::EDIT_SAVE);
    assert!(exists(cx, &o, ui_lyrics::editor::ids::ERROR));
    assert_eq!(
        read(cx, &o, |m| m.editing().unwrap().error.clone()),
        Some("disk full".into())
    );
    assert!(!read(cx, &o, |m| m.editing().unwrap().saving));
}

#[gpui_kit::test]
fn a_new_song_is_imported_from_its_marked_text(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open(cx, core.clone());
    click(cx, &o, ids::NEW_SONG);
    assert!(read(cx, &o, |m| m.editing().unwrap().new));
    let key = slide_key(cx, &o, 0, 0);
    caret(cx, &o, key, 0);
    type_text(cx, &o, "Ọlọ́run");
    press(cx, &o, "cmd-s");
    let source = core
        .calls()
        .into_iter()
        .find_map(|c| match c {
            LyricsCall::Import(s) => Some(s),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        source,
        LyricsImportSource::Text {
            title: "Untitled".into(),
            artist: String::new(),
            text: "[Verse 1]\nỌlọ́run".into(),
            copyright: None,
        }
    );
    assert_eq!(
        read(cx, &o, |m| m.selected_id().map(str::to_owned)),
        Some("new-Untitled".into())
    );
}

#[gpui_kit::test]
fn cancel_on_a_new_song_leaves_nothing_selected(cx: &mut TestAppContext) {
    let o = open(cx, Core::new(base()));
    act(cx, &o, |m, cx| m.select_song("a".into(), cx));
    act(cx, &o, |m, cx| m.new_song(cx));
    click(cx, &o, ids::EDIT_CANCEL);
    assert!(read(cx, &o, |m| m.editing().is_none()
        && m.selected_id().is_none()));
}

#[gpui_kit::test]
fn leaving_a_dirty_editor_asks_first(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    // A clean editor just closes.
    act(cx, &o, |m, cx| m.select_song("b".into(), cx));
    assert!(read(cx, &o, |m| m.editing().is_none()));
    act(cx, &o, |m, cx| m.edit(cx));
    act(cx, &o, |m, cx| {
        m.set_edit_field(Field::Artist, "Someone".into(), cx)
    });
    act(cx, &o, |m, cx| m.select_song("a".into(), cx));
    assert!(
        o.events
            .lock()
            .unwrap()
            .contains(&LyricsEvent::ConfirmDiscard)
    );
    assert_eq!(
        read(cx, &o, |m| m.selected_id().map(str::to_owned)),
        Some("b".into())
    );
    act(cx, &o, |m, cx| m.keep_editing(cx));
    assert!(read(cx, &o, |m| m.editing().is_some()));
    act(cx, &o, |m, cx| m.select_song("a".into(), cx));
    act(cx, &o, |m, cx| m.discard_and_continue(cx));
    assert!(read(cx, &o, |m| m.editing().is_none()));
    assert_eq!(
        read(cx, &o, |m| m.selected_id().map(str::to_owned)),
        Some("a".into())
    );
}

// ─── Translate ──────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn translate_adds_glosses_and_undo_strips_them(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open(cx, core.clone());
    act(cx, &o, |m, cx| m.select_song("a".into(), cx));
    assert!(!read(cx, &o, |m| m.can_undo_translation()));
    act(cx, &o, |m, cx| m.translate("yo", cx));
    let sent = core.calls().into_iter().find_map(|c| match c {
        LyricsCall::Translate {
            source_language, ..
        } => Some(source_language),
        _ => None,
    });
    assert_eq!(sent.as_deref(), Some("yo"));
    let lines = read(cx, &o, |m| m.song("a").unwrap().sections[1].lines.clone());
    assert_eq!(lines, ["sing", "(sing in English)"]);
    assert!(read(cx, &o, |m| m.can_undo_translation()));
    act(cx, &o, |m, cx| m.undo_translation(cx));
    assert_eq!(
        read(cx, &o, |m| m.song("a").unwrap().sections[1].lines.clone()),
        ["sing"]
    );
}

#[gpui_kit::test]
fn a_failed_translation_shows_its_error(cx: &mut TestAppContext) {
    let core = Core::new(base());
    core.script(|c| {
        matches!(c, LyricsCall::Translate { .. })
            .then(|| Err("Translation is unavailable right now.".into()))
    });
    let o = open(cx, core);
    act(cx, &o, |m, cx| m.select_song("a".into(), cx));
    act(cx, &o, |m, cx| m.translate("auto", cx));
    assert!(exists(cx, &o, ids::TRANSLATE_ERROR));
    assert!(!read(cx, &o, |m| m.translating()));
}

// ─── Import ─────────────────────────────────────────────────────────────────

fn dialog(cx: &mut TestAppContext, o: &Opened) -> Entity<ImportDialog> {
    cx.read(|cx| {
        o.page
            .read(cx)
            .import_dialog()
            .cloned()
            .expect("import open")
    })
}

fn dlg<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&ImportDialog) -> R) -> R {
    let d = dialog(cx, o);
    cx.read(|cx| f(d.read(cx)))
}

fn dlg_do(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut ImportDialog, &mut gpui_kit::Window, &mut gpui_kit::Context<ImportDialog>)
    + 'static,
) {
    let d = dialog(cx, o);
    cx.update_window(o.handle, move |_, window, cx| {
        d.update(cx, |d, cx| f(d, window, cx))
    })
    .unwrap();
    draw(cx, o);
}

fn file(name: &str, text: &str) -> SongFile {
    SongFile {
        name: name.into(),
        key: format!("{name}:1"),
        content: Ok(text.as_bytes().to_vec()),
    }
}

#[gpui_kit::test]
fn the_import_menu_and_file_import_open_the_dialog(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open(cx, core.clone());
    core.push(LyricsFeed::OpenImport);
    draw(cx, &o);
    assert!(exists(cx, &o, import::ids::DIALOG));
    assert_eq!(dlg(cx, &o, |d| d.tab()), Tab::File);
    click(cx, &o, import::ids::CLOSE);
    assert!(!exists(cx, &o, import::ids::DIALOG));

    click(cx, &o, ids::UPLOAD_LYRICS);
    assert!(exists(cx, &o, import::ids::DIALOG));
    assert_eq!(dlg(cx, &o, |d| d.tab()), Tab::File);
}

#[gpui_kit::test]
fn the_online_tab_searches_after_a_pause_and_imports(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open(cx, core.clone());
    act(cx, &o, |m, cx| m.open_import(cx));
    dlg_do(cx, &o, |d, w, cx| d.set_tab(Tab::Online, w, cx));
    dlg_do(cx, &o, |d, _, cx| d.set_query("wa".into(), cx));
    assert!(exists(cx, &o, import::ids::ONLINE_EMPTY));
    dlg_do(cx, &o, |d, _, cx| d.set_query("way maker".into(), cx));
    assert!(
        !core
            .calls()
            .iter()
            .any(|c| matches!(c, LyricsCall::SearchOnline(_))),
        "debounced"
    );
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx, &o);
    assert_eq!(dlg(cx, &o, |d| d.online_results().len()), 1);
    click(cx, &o, &format!("{}-act", import::ids::result(0)));
    assert_eq!(
        read(cx, &o, |m| m.selected_id().map(str::to_owned)),
        Some("new-Way Maker".into())
    );
    assert!(!read(cx, &o, |m| m.import_open()));
}

#[gpui_kit::test]
fn files_queue_with_duplicates_and_only_add_saves(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open(cx, core.clone());
    act(cx, &o, |m, cx| m.open_import(cx));
    dlg_do(cx, &o, |d, w, cx| d.set_tab(Tab::File, w, cx));
    dlg_do(cx, &o, |d, _, cx| {
        d.read_files(
            vec![
                file(
                    "Amazing Grace.txt",
                    "Amazing Grace\n\nAmazing grace how sweet\nthe sound\n",
                ),
                file("Ọba Mi.txt", "Ọba mi\nOlúwa\n"),
                file("Second Oba.txt", "Ọba mi\nOlúwa\n"),
                file("notes.docx", "x"),
            ],
            cx,
        )
    });
    let q = dlg(cx, &o, |d| d.queue().to_vec());
    assert_eq!(q.len(), 3);
    assert_eq!(
        q[0].action,
        FileAction::Skip,
        "a library duplicate starts skipped"
    );
    assert!(q[0].duplicate.is_some() && !q[0].in_batch);
    assert_eq!(
        dlg(cx, &o, |d| d.file_errors().to_vec()),
        [(
            "notes.docx".into(),
            "Only .usr, .txt, .rtf and .pro files can be imported.".into()
        )]
    );
    assert!(
        !core.calls().iter().any(|c| matches!(
            c,
            LyricsCall::Import(_) | LyricsCall::ImportBatch(_) | LyricsCall::Update(_)
        )),
        "nothing saved yet"
    );
    // Replace the library copy, keeping its id.
    dlg_do(cx, &o, |d, _, cx| d.set_action(FileAction::Replace, cx));
    click(cx, &o, import::ids::CONFIRM);
    let updates: Vec<String> = core
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            LyricsCall::Update(s) => Some(s.id),
            _ => None,
        })
        .collect();
    assert_eq!(updates, ["a"]);
    assert!(!read(cx, &o, |m| m.import_open()));
}

#[gpui_kit::test]
fn a_renamed_queued_file_is_written_over_the_import(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open(cx, core.clone());
    act(cx, &o, |m, cx| m.open_import(cx));
    dlg_do(cx, &o, |d, w, cx| d.set_tab(Tab::File, w, cx));
    dlg_do(cx, &o, |d, _, cx| {
        d.read_files(vec![file("way.txt", "Way maker\nmiracle worker\n")], cx)
    });
    dlg_do(cx, &o, |d, _, cx| {
        d.edit_selected_lyrics("[Chorus]\nWay maker".into(), cx)
    });
    let q = dlg(cx, &o, |d| d.selected().cloned().unwrap());
    assert_eq!(q.song.sections[0].kind, LyricsSectionType::Chorus);
    assert!(
        matches!(q.source, LyricsImportSource::Text { ref text, .. } if text == "[Chorus]\nWay maker")
    );
    dlg_do(cx, &o, |d, _, cx| d.confirm_files(cx));
    assert!(
        core.calls()
            .iter()
            .any(|c| matches!(c, LyricsCall::Import(_)))
    );
}

#[gpui_kit::test]
fn files_that_cannot_be_read_are_named(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open(cx, core.clone());
    act(cx, &o, |m, cx| m.open_import(cx));
    dlg_do(cx, &o, |d, w, cx| d.set_tab(Tab::File, w, cx));
    dlg_do(cx, &o, |d, _, cx| {
        d.read_files(
            vec![SongFile {
                name: "locked.txt".into(),
                key: "l".into(),
                content: Err("Permission denied".into()),
            }],
            cx,
        )
    });
    assert_eq!(
        dlg(cx, &o, |d| d.error().map(str::to_owned)),
        Some("None of those files could be read.".into())
    );
    assert_eq!(dlg(cx, &o, |d| d.file_errors().len()), 1);
    dlg_do(cx, &o, |d, _, cx| {
        d.read_files(vec![file("song.pdf", "x")], cx)
    });
    assert_eq!(
        dlg(cx, &o, |d| d.error().map(str::to_owned)),
        Some("Only .usr, .txt, .rtf and .pro files are supported.".into())
    );
}

#[gpui_kit::test]
fn paste_reads_the_clipboard_and_imports(cx: &mut TestAppContext) {
    let core = Core::new(base());
    let o = open(cx, core.clone());
    cx.write_to_clipboard(ClipboardItem::new_string(
        "Way Maker\nSinach\n\nYou are here, moving in our midst\nI worship You, I worship You\n\nWay maker, miracle worker\npromise keeper\n".into(),
    ));
    act(cx, &o, |m, cx| m.open_import(cx));
    dlg_do(cx, &o, |d, w, cx| d.set_tab(Tab::Paste, w, cx));
    let (_, _, _, text) = cx.read(|cx| dialog_form(cx, &o));
    assert!(
        text.contains("miracle worker"),
        "read on opening the tab: {text}"
    );
    assert!(dlg(cx, &o, |d| d
        .note()
        .is_some_and(|n| n.starts_with("Pasted"))));
    // Fill the title if the parser did not, then import.
    dlg_do(cx, &o, |d, w, cx| {
        let _ = d.apply_paste("", false, false, w, cx);
    });
    assert_eq!(
        dlg(cx, &o, |d| d.note().map(str::to_owned)),
        Some("No song text on the clipboard — copy the lyrics, then try again.".into())
    );
    dlg_do(cx, &o, |d, w, cx| d.set_paste_title("Way Maker", w, cx));
    click(cx, &o, import::ids::CONFIRM);
    let source = core.calls().into_iter().find_map(|c| match c {
        LyricsCall::Import(LyricsImportSource::Text { title, text, .. }) => Some((title, text)),
        _ => None,
    });
    let (title, text) = source.expect("imported");
    assert_eq!(title, "Way Maker");
    assert!(text.contains("miracle worker"));
    assert_eq!(
        read(cx, &o, |m| m.selected_id().map(str::to_owned)),
        Some("new-Way Maker".into())
    );
}

fn dialog_form(cx: &gpui_kit::App, o: &Opened) -> (String, String, String, String) {
    match o.page.read(cx).import_dialog() {
        Some(d) => d.read(cx).paste_form(cx),
        None => Default::default(),
    }
}

#[gpui_kit::test]
fn a_slide_box_fits_its_lines_without_spare_rows(cx: &mut TestAppContext) {
    let o = open_editor(cx, Core::new(base()));
    let key = slide_key(cx, &o, 0, 0);
    let e = editor(cx, &o);
    let (height, line) = cx
        .update_window(o.handle, move |_, _, cx| {
            let state = e.read(cx).slide_state(key).unwrap().clone();
            let s = state.read(cx);
            (s.input_bounds().size.height, s.line_height().unwrap())
        })
        .unwrap();
    // Two lines: room for both, and less than a spare row more.
    assert!(height >= line * 2., "{height:?}");
    assert!(height < line * 3., "{height:?}");
}
