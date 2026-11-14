//! UI integration tests for the sermon notes review (prompt 079): scan on
//! open, debounced rescans on edit, highlight placement after edits, undo,
//! paste and IME composition, scan failure and retry, the reference filter
//! and jumps, and creating the playlist.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, ClipboardItem, Entity, EntityInputHandler, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};
use kairo_domain::scripture::{
    SermonNotesAnalysis, SermonPlanDraft, SermonPlanDraftItem, SermonReferenceMatch,
};
use serde_json::Value;
use ui_scripture::ScriptureWorkspace;
use ui_scripture::backend::{
    FeedSink, Listening, Reply, ScriptureAnswer, ScriptureBackend, ScriptureCall, ScriptureSnapshot,
};
use ui_scripture::review::{SermonReview, ids, step_match, utf16_to_byte, visible_matches};

// ─── A scripted core with a small scanner ───────────────────────────────────

const REFS: &[(&str, &str)] = &[("John 3:16", "John 3:16"), ("Rom 8:28", "Romans 8:28")];

/// Finds the known references, with UTF-16 offsets as the real scan reports.
fn scan(text: &str) -> SermonNotesAnalysis {
    let mut matches = Vec::new();
    for (needle, reference) in REFS {
        let mut from = 0;
        while let Some(at) = text[from..].find(needle) {
            let byte = from + at;
            let start = text[..byte].encode_utf16().count();
            let end = start + needle.encode_utf16().count();
            matches.push(SermonReferenceMatch {
                start: start as f64,
                end: end as f64,
                text: needle.to_string(),
                reference: reference.to_string(),
                translations: vec!["KJV".into()],
            });
            from = byte + needle.len();
        }
    }
    matches.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut items: Vec<SermonPlanDraftItem> = Vec::new();
    for m in &matches {
        if !items.iter().any(|i| i.reference == m.reference) {
            items.push(SermonPlanDraftItem {
                id: format!("i{}", items.len()),
                reference: m.reference.clone(),
                translation: "KJV".into(),
            });
        }
    }
    SermonNotesAnalysis { matches, items }
}

#[derive(Default)]
struct State {
    scans: Vec<String>,
    calls: Vec<ScriptureCall>,
    fail_scans: bool,
    fail_save: bool,
}

struct Core(Mutex<State>, SermonPlanDraft);

impl ScriptureBackend for Core {
    fn snapshot(&self) -> ScriptureSnapshot {
        let mut s = ScriptureSnapshot::default();
        s.options.default_translation = "KJV".into();
        s
    }
    fn listen(&self, _: FeedSink) -> Listening {
        Box::new(())
    }
    fn call(&self, call: ScriptureCall, reply: Reply) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(call.clone());
        let answer = match call {
            ScriptureCall::ImportSermonNotes => Ok(ScriptureAnswer::Draft(Some(self.1.clone()))),
            ScriptureCall::ScanSermonNotes(text) => {
                state.scans.push(text.clone());
                if state.fail_scans {
                    Err("scan failed".into())
                } else {
                    Ok(ScriptureAnswer::Analysis(scan(&text)))
                }
            }
            ScriptureCall::SavePlan(p) if state.fail_save => {
                let _ = p;
                Err("disk full".into())
            }
            ScriptureCall::SavePlan(p) => Ok(ScriptureAnswer::Plan(p)),
            ScriptureCall::Search { .. } => Ok(ScriptureAnswer::Results(Vec::new())),
            _ => Ok(ScriptureAnswer::Done),
        };
        drop(state);
        reply(answer);
    }
    fn save_recent_lookups(&self, _: Value) {}
}

fn draft(text: &str) -> SermonPlanDraft {
    SermonPlanDraft {
        title: "Grace".into(),
        source_file_name: "grace.docx".into(),
        text: text.into(),
        html: String::new(),
        matches: Vec::new(),
        items: Vec::new(),
    }
}

struct Opened {
    handle: AnyWindowHandle,
    page: Entity<ScriptureWorkspace>,
    core: Arc<Core>,
}

fn open(cx: &mut TestAppContext, text: &str) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let core = Arc::new(Core(Mutex::new(State::default()), draft(text)));
    let backend = core.clone();
    let slot: Arc<Mutex<Option<Entity<ScriptureWorkspace>>>> = Default::default();
    let out = slot.clone();
    let handle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1280.), px(900.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let page = cx.new(|cx| ScriptureWorkspace::new(backend, window, cx));
                    *out.lock().unwrap() = Some(page.clone());
                    page
                },
            )
        })
        .unwrap()
        .0;
    let page = slot.lock().unwrap().take().unwrap();
    let opened = Opened { handle, page, core };
    draw(cx, &opened);
    // Upload sermon notes → the review opens with the draft.
    click(cx, &opened, "scripture-upload-notes");
    opened
}

fn draw(cx: &mut TestAppContext, o: &Opened) {
    cx.run_until_parked();
    cx.update_window(o.handle, |_, w, cx| w.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

fn click(cx: &mut TestAppContext, o: &Opened, id: &str) {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(o.handle, move |_, w, cx| w.click(id, cx))
        .unwrap();
    draw(cx, o);
}

fn press(cx: &mut TestAppContext, o: &Opened, key: &'static str) {
    cx.update_window(o.handle, |_, w, cx| w.press(key, cx))
        .unwrap();
    draw(cx, o);
}

fn exists(cx: &mut TestAppContext, o: &Opened, id: &str) -> bool {
    let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
    cx.update_window(o.handle, move |_, w, _| w.try_find(id).is_some())
        .unwrap()
}

fn review(cx: &mut TestAppContext, o: &Opened) -> Option<Entity<SermonReview>> {
    cx.read(|cx| o.page.read(cx).review().cloned())
}

fn with<R>(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&SermonReview, &gpui_kit::App) -> R,
) -> R {
    let r = review(cx, o).expect("review open");
    cx.read(|cx| f(r.read(cx), cx))
}

/// Puts the caret at `byte` in the notes and focuses them.
fn caret(cx: &mut TestAppContext, o: &Opened, byte: usize) {
    let r = review(cx, o).unwrap();
    cx.update_window(o.handle, move |_, window, cx| {
        let notes = r.read(cx).notes().clone();
        notes.update(cx, |s, cx| {
            s.set_selected_range(byte..byte, cx);
            s.focus(window, cx);
        });
    })
    .unwrap();
    draw(cx, o);
}

fn focus_search(cx: &mut TestAppContext, o: &Opened) {
    let r = review(cx, o).unwrap();
    cx.update_window(o.handle, move |_, window, cx| {
        let search = r.read(cx).search_input().clone();
        search.update(cx, |s, cx| s.focus(window, cx));
    })
    .unwrap();
    draw(cx, o);
}

fn type_text(cx: &mut TestAppContext, o: &Opened, text: &'static str) {
    cx.update_window(o.handle, move |_, w, cx| w.input(text, cx))
        .unwrap();
    draw(cx, o);
}

fn settle(cx: &mut TestAppContext, o: &Opened) {
    cx.executor().advance_clock(Duration::from_millis(350));
    draw(cx, o);
}

const NOTES: &str = "Grace\nRead John 3:16 first.\nThen Rom 8:28 and John 3:16 again.";

// ─── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn offsets_and_navigation_rules() {
    let text = "é John 3:16 😀 x";
    assert_eq!(utf16_to_byte(text, 2), 3);
    // The emoji is two UTF-16 units and four bytes.
    assert_eq!(utf16_to_byte(text, 14), text.find(" x").unwrap());
    let a = scan(NOTES);
    assert_eq!(visible_matches(&a.matches, "rom").len(), 1);
    assert_eq!(visible_matches(&a.matches, "JOHN 3:16").len(), 2);
    assert_eq!(visible_matches(&a.matches, "kjv").len(), 3);
    assert_eq!(step_match(&[0, 2], 2, true), Some(0), "wraps forward");
    assert_eq!(step_match(&[0, 2], 0, false), Some(2), "wraps back");
    assert_eq!(step_match(&[], 0, true), None);
}

#[gpui_kit::test]
fn opening_scans_the_notes_and_highlights_each_reference(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    assert!(exists(cx, &o, ids::DIALOG));
    assert_eq!(o.core.0.lock().unwrap().scans, vec![NOTES.to_string()]);
    let painted = with(cx, &o, |r, _| r.painted().to_vec());
    let texts: Vec<&str> = painted.iter().map(|(r, _)| &NOTES[r.clone()]).collect();
    assert_eq!(texts, vec!["John 3:16", "Rom 8:28", "John 3:16"]);
    assert!(painted[0].1 && !painted[1].1, "the first is active");
    assert!(with(cx, &o, |r, _| r.can_confirm()));
    assert!(exists(cx, &o, &ids::reference(2)));
}

#[gpui_kit::test]
fn edits_rescan_once_after_the_pause_and_highlights_follow(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    caret(cx, &o, 0);
    type_text(cx, &o, "Rom 8:28 ");
    type_text(cx, &o, "and ");
    assert!(with(cx, &o, |r, _| r.scanning()), "scanning while typing");
    assert!(
        !with(cx, &o, |r, _| r.can_confirm()),
        "create waits for the scan"
    );
    assert_eq!(o.core.0.lock().unwrap().scans.len(), 1, "debounced");
    settle(cx, &o);
    let scans = o.core.0.lock().unwrap().scans.clone();
    assert_eq!(scans.len(), 2, "one rescan for both edits");
    let text = with(cx, &o, |r, cx| r.text(cx));
    assert!(text.starts_with("Rom 8:28 and Grace"));
    let painted = with(cx, &o, |r, _| r.painted().to_vec());
    let texts: Vec<&str> = painted.iter().map(|(r, _)| &text[r.clone()]).collect();
    assert_eq!(
        texts,
        vec!["Rom 8:28", "John 3:16", "Rom 8:28", "John 3:16"]
    );
}

#[gpui_kit::test]
fn undo_restores_the_text_and_rescans(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    caret(cx, &o, NOTES.len());
    type_text(cx, &o, " Rom 8:28");
    settle(cx, &o);
    assert_eq!(with(cx, &o, |r, _| r.analysis().matches.len()), 4);
    press(cx, &o, "cmd-z");
    settle(cx, &o);
    assert_eq!(with(cx, &o, |r, cx| r.text(cx)), NOTES);
    assert_eq!(with(cx, &o, |r, _| r.analysis().matches.len()), 3);
}

#[gpui_kit::test]
fn paste_adds_references(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    caret(cx, &o, NOTES.len());
    cx.write_to_clipboard(ClipboardItem::new_string(" Also John 3:16.".into()));
    press(cx, &o, "cmd-v");
    settle(cx, &o);
    assert!(with(cx, &o, |r, cx| r.text(cx)).ends_with("again. Also John 3:16."));
    assert_eq!(with(cx, &o, |r, _| r.analysis().matches.len()), 4);
}

#[gpui_kit::test]
fn ime_composition_commits_and_rescans(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    caret(cx, &o, 0);
    let r = review(cx, &o).unwrap();
    cx.update_window(o.handle, move |_, window, cx| {
        let notes = r.read(cx).notes().clone();
        notes.update(cx, |s, cx| {
            s.replace_and_mark_text_in_range(None, "め", None, window, cx);
            s.replace_and_mark_text_in_range(None, "恵み", None, window, cx);
            s.replace_text_in_range(None, "恵み ", window, cx);
        });
    })
    .unwrap();
    settle(cx, &o);
    let text = with(cx, &o, |r, cx| r.text(cx));
    assert!(text.starts_with("恵み Grace"), "{text}");
    let painted = with(cx, &o, |r, _| r.painted().to_vec());
    assert_eq!(
        &text[painted[0].0.clone()],
        "John 3:16",
        "offsets survive multibyte text"
    );
}

#[gpui_kit::test]
fn a_failed_scan_blocks_creating_until_retried(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    o.core.0.lock().unwrap().fail_scans = true;
    caret(cx, &o, 0);
    type_text(cx, &o, "x");
    settle(cx, &o);
    assert!(exists(cx, &o, ids::SCAN_ERROR));
    assert!(!with(cx, &o, |r, _| r.can_confirm()));
    o.core.0.lock().unwrap().fail_scans = false;
    click(cx, &o, ids::RETRY);
    assert!(!exists(cx, &o, ids::SCAN_ERROR));
    assert!(with(cx, &o, |r, _| r.can_confirm()));
}

#[gpui_kit::test]
fn the_filter_and_arrows_jump_through_references(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    click(cx, &o, ids::NEXT);
    click(cx, &o, ids::NEXT);
    assert_eq!(with(cx, &o, |r, _| r.active()), 2);
    // The selection lands on the reference in the notes.
    let selected = with(cx, &o, |r, cx| r.notes().read(cx).selected_range());
    assert_eq!(&NOTES[selected], "John 3:16");
    click(cx, &o, ids::NEXT);
    assert_eq!(with(cx, &o, |r, _| r.active()), 0, "wraps");
    // Filter to Romans: only it shows, and the arrows stay inside it.
    focus_search(cx, &o);
    type_text(cx, &o, "rom");
    assert!(exists(cx, &o, &ids::reference(1)));
    assert!(!exists(cx, &o, &ids::reference(0)));
    click(cx, &o, ids::NEXT);
    assert_eq!(with(cx, &o, |r, _| r.active()), 1);
    // A jump focuses the notes; go back to the search box.
    focus_search(cx, &o);
    type_text(cx, &o, "zzz");
    assert!(exists(cx, &o, ids::NO_MATCH));
    click(cx, &o, ids::CLEAR_SEARCH);
    assert!(exists(cx, &o, &ids::reference(0)));
    // A card click jumps too.
    click(cx, &o, &ids::reference(2));
    assert_eq!(with(cx, &o, |r, _| r.active()), 2);
}

#[gpui_kit::test]
fn notes_without_references_cannot_make_a_playlist(cx: &mut TestAppContext) {
    let o = open(cx, "Nothing to read today.");
    assert!(exists(cx, &o, ids::EMPTY));
    assert!(!with(cx, &o, |r, _| r.can_confirm()));
}

#[gpui_kit::test]
fn create_playlist_saves_the_reviewed_items_and_closes(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    let r = review(cx, &o).unwrap();
    cx.update_window(o.handle, move |_, window, cx| {
        let title = r.read(cx).title_input().clone();
        title.update(cx, |s, cx| s.set_value("  Sunday grace  ", window, cx));
    })
    .unwrap();
    click(cx, &o, ids::CREATE);
    assert!(review(cx, &o).is_none(), "the review closes");
    let saved = o.core.0.lock().unwrap().calls.iter().find_map(|c| match c {
        ScriptureCall::SavePlan(p) => Some(p.clone()),
        _ => None,
    });
    let saved = saved.expect("a playlist was saved");
    assert_eq!(saved.title, "Sunday grace");
    assert_eq!(saved.source_file_name, "grace.docx");
    assert_eq!(saved.source_text.as_deref(), Some(NOTES));
    assert_eq!(
        saved
            .items
            .iter()
            .map(|i| i.reference.as_str())
            .collect::<Vec<_>>(),
        vec!["John 3:16", "Romans 8:28"]
    );
    let model = cx.read(|cx| o.page.read(cx).model().clone());
    assert_eq!(
        cx.read(|cx| model.read(cx).source()),
        Some(ui_scripture::model::Source::Plan)
    );
}

#[gpui_kit::test]
fn an_empty_name_keeps_the_file_title(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    let r = review(cx, &o).unwrap();
    cx.update_window(o.handle, move |_, window, cx| {
        let title = r.read(cx).title_input().clone();
        title.update(cx, |s, cx| s.set_value("   ", window, cx));
    })
    .unwrap();
    click(cx, &o, ids::CREATE);
    let title = o.core.0.lock().unwrap().calls.iter().find_map(|c| match c {
        ScriptureCall::SavePlan(p) => Some(p.title.clone()),
        _ => None,
    });
    assert_eq!(title.as_deref(), Some("Grace"));
}

#[gpui_kit::test]
fn a_failed_save_keeps_the_review_open(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    o.core.0.lock().unwrap().fail_save = true;
    click(cx, &o, ids::CREATE);
    assert!(review(cx, &o).is_some());
    assert!(!with(cx, &o, |r, _| r.confirming()));
    let model = cx.read(|cx| o.page.read(cx).model().clone());
    assert_eq!(
        cx.read(|cx| model.read(cx).error().map(str::to_owned))
            .as_deref(),
        Some("disk full")
    );
}

#[gpui_kit::test]
fn cancel_and_escape_close_without_saving(cx: &mut TestAppContext) {
    let o = open(cx, NOTES);
    click(cx, &o, ids::CANCEL);
    assert!(review(cx, &o).is_none());
    click(cx, &o, "scripture-upload-notes");
    let r = review(cx, &o).unwrap();
    cx.update_window(o.handle, move |_, window, cx| {
        let focus = gpui_kit::Focusable::focus_handle(r.read(cx), cx);
        focus.focus(window, cx);
    })
    .unwrap();
    press(cx, &o, "escape");
    assert!(review(cx, &o).is_none());
    assert!(
        !o.core
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| matches!(c, ScriptureCall::SavePlan(_)))
    );
}
