//! UI tests for Imagine, driven by a scripted backend: connecting, drafts
//! that never publish on their own, refinement, restore, cancel and failure.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use ui_imagine::backend::{Action, ImageModel, Kind, Layout, Project, Quality, Slide, Version};
use ui_imagine::{
    Attachment, FeedSink, ImagineAnswer, ImagineBackend, ImagineCall, ImagineFeed, ImaginePage,
    ImagineSnapshot, Reply, ids,
};

#[derive(Default)]
struct State {
    connected: bool,
    projects: Vec<Project>,
    calls: Vec<ImagineCall>,
    /// Hold generations and refinements until cancelled.
    hold: bool,
    held: Vec<(u64, Reply)>,
    /// Fail the next refinement with this message.
    fail: Option<String>,
    next: usize,
    sink: Option<FeedSink>,
}

struct Fake(Mutex<State>);

fn uuid(n: usize) -> String {
    format!("00000000-0000-0000-0000-{n:012}")
}

fn version(id: String, parent: Option<String>, instruction: &str) -> Version {
    Version {
        file: format!("{id}.png"),
        id,
        parent,
        instruction: instruction.into(),
        quality: Quality::Medium,
        model: None,
        created: 0.,
        reply: None,
        prompt: Some(format!("Enhanced: {instruction}, in soft morning light")),
        slides: Vec::new(),
        theme: None,
        background: None,
    }
}

impl Fake {
    fn snapshot(s: &State) -> ImagineAnswer {
        ImagineAnswer::Snapshot(ImagineSnapshot {
            connected: s.connected,
            projects: s.projects.clone(),
            notice: None,
        })
    }
}

impl ImagineBackend for Fake {
    fn call(&self, call: ImagineCall, reply: Reply) {
        let mut s = self.0.lock().unwrap();
        s.calls.push(call.clone());
        s.next += 1;
        let n = s.next;
        let answer = match call {
            ImagineCall::Load => Ok(Self::snapshot(&s)),
            ImagineCall::Connect(key) if key == "bad" => {
                Err("OpenAI didn't accept this API key.".to_string())
            }
            ImagineCall::Connect(_) => {
                s.connected = true;
                Ok(Self::snapshot(&s))
            }
            ImagineCall::Disconnect => {
                s.connected = false;
                Ok(Self::snapshot(&s))
            }
            ImagineCall::Send {
                job,
                project,
                from,
                message,
                ..
            } => {
                let deck = message.contains("slides");
                if let Some(sink) = &s.sink {
                    if s.hold {
                        // Still planning: only its reasoning so far.
                        sink(ImagineFeed::Thought {
                            job,
                            text: "Working out the slide topics".into(),
                        });
                        s.held.push((job, reply));
                        return;
                    }
                    sink(ImagineFeed::Planned {
                        job,
                        reply: format!("On it: {message}"),
                        action: if message.starts_with('?') {
                            Action::Ask
                        } else if deck {
                            Action::Deck
                        } else {
                            Action::Image
                        },
                        slides: if deck { 3 } else { 0 },
                        titles: Vec::new(),
                    });
                }
                if s.hold {
                    s.held.push((job, reply));
                    return;
                }
                if message.starts_with('?') {
                    Ok(ImagineAnswer::Reply("Happy to help.".into()))
                } else if let Some(e) = s.fail.take() {
                    Err(e)
                } else {
                    let mut v = version(uuid(n * 10), from, &message);
                    v.reply = Some(format!("On it: {message}"));
                    if deck {
                        v.slides = (0..3)
                            .map(|i| Slide {
                                title: format!("Slide {i}"),
                                file: format!("{i}.png"),
                                layout: Some(Layout::Bullets),
                                body: vec!["A point".into()],
                                ..Slide::default()
                            })
                            .collect();
                    }
                    match project.and_then(|id| s.projects.iter_mut().find(|p| p.id == id)) {
                        Some(p) => {
                            p.selected = v.id.clone();
                            p.versions.push(v);
                            Ok(ImagineAnswer::Project(p.clone()))
                        }
                        None => {
                            let p = Project {
                                id: uuid(n),
                                title: message,
                                kind: if deck { Kind::Deck } else { Kind::Image },
                                created: n as f64,
                                selected: v.id.clone(),
                                versions: vec![v],
                                saved: Vec::new(),
                            };
                            s.projects.insert(0, p.clone());
                            Ok(ImagineAnswer::Project(p))
                        }
                    }
                }
            }
            ImagineCall::DownloadPptx { .. } => {
                Ok(ImagineAnswer::File(PathBuf::from("/Downloads/deck.pptx")))
            }
            ImagineCall::PickAttachment => Ok(ImagineAnswer::Attachment(Attachment {
                name: "outline.docx".into(),
                text: "Point one. Point two.".into(),
                truncated: false,
            })),
            ImagineCall::Cancel(job) => {
                if let Some(i) = s.held.iter().position(|(j, _)| *j == job) {
                    let (_, held) = s.held.remove(i);
                    held(Err("Cancelled.".into()));
                }
                Ok(ImagineAnswer::Done)
            }
            ImagineCall::Select { project, version } => {
                let p = s.projects.iter_mut().find(|p| p.id == project).unwrap();
                p.selected = version;
                Ok(ImagineAnswer::Project(p.clone()))
            }
            ImagineCall::SaveToMedia { project, version }
            | ImagineCall::SaveToSlides { project, version } => {
                let p = s.projects.iter_mut().find(|p| p.id == project).unwrap();
                p.saved.push(version);
                Ok(ImagineAnswer::Project(p.clone()))
            }
            ImagineCall::Delete(id) => {
                s.projects.retain(|p| p.id != id);
                Ok(ImagineAnswer::Done)
            }
        };
        drop(s);
        reply(answer);
    }

    fn listen(&self, sink: FeedSink) {
        self.0.lock().unwrap().sink = Some(sink);
    }

    fn image_path(&self, file: &str) -> PathBuf {
        PathBuf::from("/nonexistent").join(file)
    }
}

struct Opened {
    handle: AnyWindowHandle,
    page: Entity<ImaginePage>,
    fake: Arc<Fake>,
}

fn open(cx: &mut TestAppContext, state: State) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let fake = Arc::new(Fake(Mutex::new(state)));
    let backend = fake.clone();
    let slot: Arc<Mutex<Option<Entity<ImaginePage>>>> = Default::default();
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
                    let p = cx.new(|cx| ImaginePage::new(backend, window, cx));
                    *out.lock().unwrap() = Some(p.clone());
                    p
                },
            )
        })
        .unwrap()
        .0;
    let page = slot.lock().unwrap().take().unwrap();
    let o = Opened { handle, page, fake };
    draw(cx, &o);
    draw(cx, &o);
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

fn act(
    cx: &mut TestAppContext,
    o: &Opened,
    f: impl FnOnce(&mut ImaginePage, &mut gpui_kit::Window, &mut gpui_kit::Context<ImaginePage>),
) {
    let page = o.page.clone();
    cx.update_window(o.handle, move |_, w, cx| {
        page.update(cx, |p, cx| f(p, w, cx))
    })
    .unwrap();
    draw(cx, o);
}

fn read<R>(cx: &mut TestAppContext, o: &Opened, f: impl FnOnce(&ImaginePage) -> R) -> R {
    cx.read(|cx| f(o.page.read(cx)))
}

fn calls(o: &Opened) -> Vec<ImagineCall> {
    o.fake.0.lock().unwrap().calls.clone()
}

fn publishes(o: &Opened) -> usize {
    calls(o)
        .iter()
        .filter(|c| matches!(c, ImagineCall::SaveToMedia { .. }))
        .count()
}

fn send(cx: &mut TestAppContext, o: &Opened, text: &str) {
    let text = text.to_owned();
    act(cx, o, move |p, w, cx| p.set_prompt(&text, w, cx));
    click(cx, o, ids::SEND);
}

#[gpui_kit::test]
fn a_bad_key_stays_on_connect_and_a_good_one_opens_the_gallery(cx: &mut TestAppContext) {
    let o = open(cx, State::default());
    // First-time visitors meet Imagine before being asked for an API key.
    assert!(exists(cx, &o, ids::INTRO_CONTINUE));
    assert!(!exists(cx, &o, ids::CONNECT));
    assert!(!exists(cx, &o, ids::PROMPT));

    click(cx, &o, ids::INTRO_CONTINUE);
    assert!(exists(cx, &o, ids::CONNECT));
    assert!(exists(cx, &o, ids::INTRO_BACK));

    // Back returns to the introduction.
    click(cx, &o, ids::INTRO_BACK);
    assert!(exists(cx, &o, ids::INTRO_CONTINUE));
    click(cx, &o, ids::INTRO_CONTINUE);

    act(cx, &o, |p, w, cx| p.set_key("bad", w, cx));
    click(cx, &o, ids::CONNECT);
    assert!(exists(cx, &o, ids::ERROR));
    assert!(exists(cx, &o, ids::CONNECT));

    act(cx, &o, |p, w, cx| p.set_key("sk-good", w, cx));
    click(cx, &o, ids::CONNECT);
    assert!(read(cx, &o, |p| p.connected()));
    assert!(exists(cx, &o, ids::PROMPT));
    assert!(exists(cx, &o, &ids::template("worship")));

    // Disconnecting returns straight to the Connect card.
    click(cx, &o, ids::DISCONNECT);
    assert!(exists(cx, &o, ids::CONNECT));
}

#[gpui_kit::test]
fn generating_makes_a_draft_and_never_publishes(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    click(cx, &o, &ids::template("watercolor"));
    send(cx, &o, "a lighthouse");
    let generated = calls(&o)
        .into_iter()
        .find_map(|c| match c {
            ImagineCall::Send {
                message,
                style,
                quality,
                ..
            } => Some((
                format!("{message} / {}", style.unwrap_or_default()),
                quality,
            )),
            _ => None,
        })
        .unwrap();
    assert!(
        generated
            .0
            .starts_with("a lighthouse / In a soft watercolor")
    );
    assert_eq!(generated.1, Quality::Medium);
    // The new image opens as a conversation.
    assert!(exists(cx, &o, ids::CHAT));
    assert!(read(cx, &o, |p| p.open_project().is_some()));
    assert!(exists(cx, &o, &ids::version(0)));
    assert_eq!(publishes(&o), 0);

    // Back in the gallery, the creation's card reopens its conversation.
    let project = read(cx, &o, |p| p.projects()[0].id.clone());
    click(cx, &o, ids::BACK);
    assert!(!exists(cx, &o, ids::CHAT));
    click(cx, &o, &ids::project(&project));
    assert!(exists(cx, &o, ids::CHAT));
    assert!(exists(cx, &o, &ids::version(0)));
}

#[gpui_kit::test]
fn refining_restoring_and_saving_are_explicit(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    send(cx, &o, "a cross at dawn");
    let (project, v1) = read(cx, &o, |p| {
        (p.projects()[0].id.clone(), p.projects()[0].selected.clone())
    });
    let _ = project;
    assert!(exists(cx, &o, ids::CHAT));

    send(cx, &o, "warmer light");
    let v2 = read(cx, &o, |p| p.open_project().unwrap().selected.clone());
    assert_ne!(v1, v2);
    assert!(calls(&o).iter().any(|c| matches!(c,
        ImagineCall::Send { from: Some(from), message: instruction, .. } if *from == v1 && instruction == "warmer light")));

    // From v1: the next edit starts from the original, not v2.
    act(cx, &o, |p, _, cx| p.set_from_original(true, cx));
    send(cx, &o, "add clouds");
    assert!(calls(&o).iter().any(|c| matches!(c,
        ImagineCall::Send { from: Some(from), message: instruction, .. } if *from == v1 && instruction == "add clouds")));
    assert_eq!(
        read(cx, &o, |p| p.open_project().unwrap().versions.len()),
        3
    );

    // Restore v1; nothing is lost.
    click(cx, &o, &ids::version(0));
    let p = read(cx, &o, |p| p.open_project().cloned().unwrap());
    assert_eq!(p.selected, v1);
    assert_eq!(p.versions.len(), 3);
    assert_eq!(publishes(&o), 0);

    // Every version of the conversation is still there.
    for i in 0..3 {
        assert!(exists(cx, &o, &ids::version(i)));
    }
    click(cx, &o, &ids::save(0));
    assert_eq!(publishes(&o), 1);
    // Saved: the button is disabled, a second click sends nothing.
    click(cx, &o, &ids::save(0));
    assert_eq!(publishes(&o), 1);
}

#[gpui_kit::test]
fn the_model_choice_reaches_generation_and_refinement(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    assert_eq!(read(cx, &o, |p| p.model()), ImageModel::Flare);
    // Options panel: a click elsewhere closes it.
    click(cx, &o, ids::OPTIONS);
    click(cx, &o, &ids::option("model"));
    assert!(exists(cx, &o, &ids::choice("gpt-image-2.6-flare")));
    click(cx, &o, &ids::tab("creations"));
    assert!(
        !exists(cx, &o, ids::OPTIONS_PANEL),
        "a click outside closes the menu"
    );
    send(cx, &o, "a harbour");
    // Model › Best quality, in the conversation.
    click(cx, &o, ids::OPTIONS);
    click(cx, &o, &ids::option("model"));
    click(cx, &o, &ids::choice("gpt-image-2.6-sunburst"));
    assert!(!exists(cx, &o, ids::OPTIONS_PANEL));
    assert_eq!(read(cx, &o, |p| p.model()), ImageModel::Sunburst);
    // Quality is chosen independently of the model.
    act(cx, &o, |p, _, cx| p.set_quality(Quality::High, cx));
    send(cx, &o, "at night");

    let models: Vec<(ImageModel, Quality)> = calls(&o)
        .into_iter()
        .filter_map(|c| match c {
            ImagineCall::Send { model, quality, .. } => Some((model, quality)),
            _ => None,
        })
        .collect();
    assert_eq!(
        models,
        vec![
            (ImageModel::Flare, Quality::Medium),
            (ImageModel::Sunburst, Quality::High)
        ]
    );
}

#[gpui_kit::test]
fn cancel_stops_a_job_without_an_error(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            hold: true,
            ..State::default()
        },
    );
    send(cx, &o, "a field of wheat");
    let job = read(cx, &o, |p| p.jobs()[0].id);
    assert!(exists(cx, &o, &ids::job(job)));

    click(cx, &o, ids::CANCEL);
    assert!(read(cx, &o, |p| p.jobs().is_empty()));
    assert!(!exists(cx, &o, &ids::job(job)));
    assert!(!exists(cx, &o, ids::ERROR));
    assert!(calls(&o).contains(&ImagineCall::Cancel(job)));
    assert!(read(cx, &o, |p| p.projects().is_empty()));
}

#[gpui_kit::test]
fn a_failed_refinement_keeps_every_version(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    send(cx, &o, "a quiet lake");
    let project = read(cx, &o, |p| p.projects()[0].clone());
    o.fake.0.lock().unwrap().fail =
        Some("OpenAI declined this prompt under its content policy.".into());
    send(cx, &o, "something odd");

    assert!(exists(cx, &o, ids::ERROR));
    assert_eq!(
        read(cx, &o, |p| p.open_project().cloned().unwrap()),
        project
    );
    assert!(read(cx, &o, |p| p.jobs().is_empty()));
}

#[gpui_kit::test]
fn deleting_asks_first(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    send(cx, &o, "stars");
    let _project = read(cx, &o, |p| p.projects()[0].id.clone());
    click(cx, &o, ids::DELETE);
    assert!(read(cx, &o, |p| p.projects().len()) == 1);
    click(cx, &o, ids::DELETE_CONFIRM);
    assert!(read(cx, &o, |p| p.projects().is_empty()));
    assert!(read(cx, &o, |p| p.open_project().is_none()));
}

#[gpui_kit::test]
fn the_assistant_answers_and_a_deck_saves_to_slides(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            hold: true,
            ..State::default()
        },
    );
    // While the job runs, the assistant's plan shows before anything is drawn.
    send(cx, &o, "make 3 slides about prayer");
    let job = read(cx, &o, |p| p.jobs()[0].clone());
    // Its reasoning shows while it plans; the reply comes after.
    assert_eq!(job.thought, "Working out the slide topics");
    assert_eq!(job.reply, None);
    assert!(exists(cx, &o, ids::THOUGHT));
    assert!(exists(cx, &o, &ids::job(job.id)));
    click(cx, &o, ids::CANCEL);
    assert!(read(cx, &o, |p| p.jobs().is_empty()));

    o.fake.0.lock().unwrap().hold = false;
    send(cx, &o, "make 3 slides about prayer");
    let p = read(cx, &o, |p| p.open_project().cloned().unwrap());
    assert_eq!(
        p.current().reply.as_deref(),
        Some("On it: make 3 slides about prayer"),
        "the assistant's reply stays in the conversation"
    );
    assert_eq!(p.kind, Kind::Deck);
    assert_eq!(p.current().slides.len(), 3);
    click(cx, &o, &ids::version(0));
    click(cx, &o, ids::CANVAS_TO_SLIDES);
    let saved: Vec<_> = calls(&o)
        .into_iter()
        .filter(|c| matches!(c, ImagineCall::SaveToSlides { .. }))
        .collect();
    assert_eq!(saved.len(), 1, "a deck saves to Slides, not Media");
    assert_eq!(publishes(&o), 0);

    // A question gets an answer in the conversation and makes nothing.
    send(cx, &o, "? what size are these");
    assert_eq!(
        read(cx, &o, |p| p.open_project().unwrap().versions.len()),
        1
    );
}

#[gpui_kit::test]
fn messages_can_be_copied(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    send(cx, &o, "a quiet lake at dawn");
    let v = read(cx, &o, |p| p.open_project().unwrap().selected.clone());
    click(cx, &o, &ids::copy(&format!("{v}-you")));
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some("a quiet lake at dawn"));
    click(cx, &o, &ids::copy(&format!("{v}-reply")));
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some("On it: a quiet lake at dawn"));
}

#[gpui_kit::test]
fn a_creation_can_be_deleted_from_its_card(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    send(cx, &o, "a harvest field");
    let id = read(cx, &o, |p| p.projects()[0].id.clone());
    click(cx, &o, ids::BACK);
    // The trash asks over the card; Cancel keeps it, the card stays closed.
    click(cx, &o, &ids::card_delete(&id));
    assert!(!exists(cx, &o, ids::CHAT));
    click(cx, &o, &ids::card_keep(&id));
    assert!(!exists(cx, &o, &ids::card_confirm(&id)));
    assert_eq!(read(cx, &o, |p| p.projects().len()), 1);
    click(cx, &o, &ids::card_delete(&id));
    click(cx, &o, &ids::card_confirm(&id));
    assert!(read(cx, &o, |p| p.projects().is_empty()));
    assert!(calls(&o).contains(&ImagineCall::Delete(id)));
}

#[gpui_kit::test]
fn a_deck_opens_in_a_preview_beside_the_chat(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    // Attach source text; it travels with the next message.
    click(cx, &o, ids::ATTACH);
    assert_eq!(
        read(cx, &o, |p| p.attachment().map(|a| a.name.clone())),
        Some("outline.docx".into())
    );
    send(cx, &o, "make 3 slides from this");
    assert!(calls(&o).iter().any(|c| matches!(c,
        ImagineCall::Send { attachment: Some(a), .. } if a.text == "Point one. Point two.")));
    assert!(read(cx, &o, |p| p.attachment().is_none()));

    // The deck is one card; opening it shows the canvas.
    assert!(!exists(cx, &o, ids::CANVAS));
    click(cx, &o, &ids::version(0));
    assert!(exists(cx, &o, ids::CANVAS));
    assert!(exists(cx, &o, &ids::page(2)));
    click(cx, &o, &ids::page(1));
    assert_eq!(read(cx, &o, |p| p.canvas().unwrap().page), 1);
    click(cx, &o, ids::CANVAS_NEXT);
    assert_eq!(read(cx, &o, |p| p.canvas().unwrap().page), 2);
    click(cx, &o, ids::CANVAS_PREV);
    assert_eq!(read(cx, &o, |p| p.canvas().unwrap().page), 1);
    // The edge between the chat and the slides drags.
    let before = read(cx, &o, |p| p.chat_width());
    cx.update_window(o.handle, |_, w, cx| {
        let edge = w.find(ids::SPLIT).bounds();
        let from = edge.center();
        let to = gpui_kit::point(from.x + px(120.), from.y);
        w.drag(from, to, cx);
    })
    .unwrap();
    draw(cx, &o);
    let after = read(cx, &o, |p| p.chat_width());
    assert!(after > before + 100., "dragged from {before} to {after}");
    // A preview only: changes are asked for in the chat.
    assert!(!exists(cx, &o, "imagine-canvas-save"));
    click(cx, &o, ids::CANVAS_PPTX);
    assert!(
        calls(&o)
            .iter()
            .any(|c| matches!(c, ImagineCall::DownloadPptx { .. }))
    );
    click(cx, &o, ids::CANVAS_CLOSE);
    assert!(!exists(cx, &o, ids::CANVAS));
}

#[gpui_kit::test]
fn imagine_opens_on_templates(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    assert!(exists(cx, &o, &ids::template("worship")), "templates first");
    click(cx, &o, &ids::tab("creations"));
    assert!(!exists(cx, &o, &ids::template("worship")));
    // Picking Imagine from the rail again shows the templates.
    act(cx, &o, |p, _, cx| p.show_templates(cx));
    assert!(exists(cx, &o, &ids::template("worship")));
}

#[gpui_kit::test]
fn an_image_opens_in_a_preview_with_its_versions(cx: &mut TestAppContext) {
    let o = open(
        cx,
        State {
            connected: true,
            ..State::default()
        },
    );
    send(cx, &o, "a cross at dawn");
    send(cx, &o, "warmer light");
    assert!(!exists(cx, &o, ids::CANVAS));
    // The first version opens beside the chat, with both versions to page through.
    click(cx, &o, &ids::version(0));
    assert!(exists(cx, &o, ids::CANVAS));
    assert!(exists(cx, &o, ids::CANVAS_TO_MEDIA));
    assert!(!exists(cx, &o, ids::CANVAS_PPTX), "PowerPoint is for decks");
    assert_eq!(
        read(cx, &o, |p| p.canvas().unwrap().slides[0].notes.clone()),
        "a cross at dawn",
        "the preview shows what was asked; the enhanced prompt stays behind the scenes"
    );
    let first = read(cx, &o, |p| p.open_project().unwrap().versions[0].id.clone());
    assert_eq!(read(cx, &o, |p| p.canvas().unwrap().version.clone()), first);
    click(cx, &o, ids::CANVAS_NEXT);
    let second = read(cx, &o, |p| p.open_project().unwrap().versions[1].id.clone());
    assert_eq!(
        read(cx, &o, |p| p.canvas().unwrap().version.clone()),
        second
    );
    // Save acts on the version showing, once.
    click(cx, &o, ids::CANVAS_TO_MEDIA);
    assert!(calls(&o).iter().any(|c| matches!(c,
        ImagineCall::SaveToMedia { version, .. } if *version == second)));
    assert_eq!(publishes(&o), 1);
    click(cx, &o, ids::CANVAS_CLOSE);
    assert!(!exists(cx, &o, ids::CANVAS));
}
