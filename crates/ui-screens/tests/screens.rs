//! Interaction tests derived from screen-configuration.integration.cjs.
use futures_channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, ElementId, Entity, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};
use kairo_domain::{overlay::OverlayOutputKind, settings::AppSettings};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use ui_screens::backend::Events;
use ui_screens::{
    Command, Reply, ScreensBackend, ScreensWindow, Snapshot,
    model::{Edit, apply},
};

struct Fake {
    snapshot: Mutex<Snapshot>,
    ui: Mutex<HashMap<String, String>>,
    events: Mutex<Vec<UnboundedSender<Result<Snapshot, String>>>>,
    error: Mutex<Option<String>>,
    load_error: Mutex<Option<String>>,
    pause: Mutex<Option<futures_channel::oneshot::Receiver<()>>>,
    calls: Mutex<Vec<String>>,
    preview_engine: Option<Arc<kairo_output::fake::FakeEngine>>,
    preview_themes: Arc<Mutex<Vec<kairo_domain::overlay::OverlayTheme>>>,
    preview_alive: Arc<std::sync::atomic::AtomicUsize>,
}
impl Fake {
    fn new() -> Self {
        let mut settings =
            AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone());
        settings.overlay.outputs = vec![
            kairo_domain::overlay_defaults::make_overlay_output(
                "screen-a",
                OverlayOutputKind::Screen,
            ),
            kairo_domain::overlay_defaults::make_overlay_output(
                "screen-b",
                OverlayOutputKind::Screen,
            ),
        ];
        settings.overlay.outputs[0].enabled = true;
        settings.overlay.outputs[1].enabled = true;
        settings.overlay.outputs[0].name = "Screen A".into();
        settings.overlay.outputs[1].name = "Screen B".into();
        Self {
            snapshot: Mutex::new(Snapshot::new(settings)),
            ui: Mutex::default(),
            events: Mutex::default(),
            error: Mutex::default(),
            load_error: Mutex::default(),
            pause: Mutex::default(),
            calls: Mutex::default(),
            preview_engine: None,
            preview_themes: Arc::default(),
            preview_alive: Arc::default(),
        }
    }
}
fn ready<T: Send + 'static>(v: T) -> Reply<T> {
    Box::pin(async move { v })
}
impl ScreensBackend for Fake {
    fn open_preview(&self) -> Option<ui_screens::backend::ThemePreview> {
        use kairo_core::engine::OutputEngine;
        let engine = self.preview_engine.clone()?;
        let output = "test-screens-sample".to_string();
        engine.command(kairo_engine_proto::Command::OpenSurface(
            kairo_engine_proto::SurfaceSpec {
                output: output.clone(),
                label: "Test theme sample".into(),
                sink: kairo_engine_proto::SinkSpec::Preview,
            },
        ));
        let hub = kairo_core::preview::PreviewHub::new(engine.clone()).unwrap();
        let themes = self.preview_themes.clone();
        self.preview_alive
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Some(ui_screens::backend::ThemePreview::new(
            hub,
            output.clone(),
            move |theme| themes.lock().unwrap().push(theme),
            PreviewLifetime {
                engine,
                output,
                alive: self.preview_alive.clone(),
            },
        ))
    }
    fn load(&self) -> Reply<Result<Snapshot, String>> {
        ready(
            self.load_error
                .lock()
                .unwrap()
                .clone()
                .map_or_else(|| Ok(self.snapshot.lock().unwrap().clone()), Err),
        )
    }
    fn edit(&self, edit: Edit) -> Reply<Result<AppSettings, String>> {
        if let Some(error) = self.error.lock().unwrap().clone() {
            return ready(Err(error));
        }
        let mut s = self.snapshot.lock().unwrap();
        let result = apply(&mut s.settings, &edit).map(|_| s.settings.clone());
        if let Some(pause) = self.pause.lock().unwrap().take() {
            Box::pin(async move {
                let _ = pause.await;
                result
            })
        } else {
            ready(result)
        }
    }
    fn run(&self, command: Command) -> Reply<Result<String, String>> {
        self.calls.lock().unwrap().push(format!("{command:?}"));
        ready(
            self.error
                .lock()
                .unwrap()
                .clone()
                .map_or(Ok("Done".into()), Err),
        )
    }
    fn events(&self) -> Events {
        let (tx, rx) = unbounded();
        self.events.lock().unwrap().push(tx);
        Events {
            receiver: rx,
            lifetime: Box::new(()),
        }
    }
    fn read_ui(&self, key: &str) -> Option<String> {
        self.ui.lock().unwrap().get(key).cloned()
    }
    fn write_ui(&self, key: &str, value: String) {
        self.ui.lock().unwrap().insert(key.into(), value);
    }
}
struct Opened {
    view: Entity<ScreensWindow>,
    handle: AnyWindowHandle,
    fake: Arc<Fake>,
}
fn open(cx: &mut TestAppContext, fake: Arc<Fake>, initial: Option<&'static str>) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let backend = fake.clone();
    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1280.), px(900.)),
                ))),
                ..Default::default()
            },
            cx,
            move |w, cx| cx.new(|cx| ScreensWindow::new(backend, initial, (1000., 800.), w, cx)),
        )
        .unwrap()
    });
    let o = Opened { view, handle, fake };
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
fn click(cx: &mut TestAppContext, o: &Opened, id: impl Into<ElementId>) {
    cx.update_window(o.handle, |_, w, cx| w.click(id, cx))
        .unwrap();
    draw(cx, o);
}
fn shows(cx: &mut TestAppContext, o: &Opened, id: impl Into<ElementId>) -> bool {
    cx.update_window(o.handle, |_, w, _| w.try_find(id).is_some())
        .unwrap()
}

#[gpui_kit::test]
fn opens_selected_screen_and_group_switch_saves(cx: &mut TestAppContext) {
    let o = open(cx, Arc::new(Fake::new()), Some("screen-a"));
    assert_eq!(
        o.view.read_with(cx, |v, _| v.selected().map(str::to_owned)),
        Some("screen-a".into())
    );
    click(cx, &o, "screens-group-false");
    assert!(
        o.fake
            .snapshot
            .lock()
            .unwrap()
            .settings
            .overlay
            .outputs
            .iter()
            .all(|s| !s.enabled)
    );
}
#[gpui_kit::test]
fn show_panels_persist_and_reopen(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::new());
    let o = open(cx, fake.clone(), None);
    click(cx, &o, "screens-show");
    assert_eq!(
        fake.ui
            .lock()
            .unwrap()
            .get("show:selected")
            .map(String::as_str),
        Some("show:setup")
    );
    assert!(shows(cx, &o, "screens-transition"));
    let reopened = open(cx, fake, None);
    assert_eq!(
        reopened
            .view
            .read_with(cx, |v, _| v.selected().map(str::to_owned)),
        Some("show:setup".into())
    );
}
#[gpui_kit::test]
fn add_card_survives_another_output_edit_and_roundtrip(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::new());
    let o = open(cx, fake.clone(), Some("add:device"));
    let card = o
        .view
        .read_with(cx, |v, _| v.selected().unwrap().to_string());
    cx.update_window(o.handle,|_,w,cx|o.view.update(cx,|v,cx|v.edit(Edit::Output{id:card.clone(),patch:serde_json::json!({"devicePortId":"decklink:123:3","deviceModeId":"decklink:25"})},w,cx))).unwrap();
    draw(cx, &o);
    cx.update_window(o.handle, |_, w, cx| {
        o.view.update(cx, |v, cx| {
            v.edit(
                Edit::Output {
                    id: "screen-a".into(),
                    patch: serde_json::json!({"name":"Sunday edited"}),
                },
                w,
                cx,
            )
        })
    })
    .unwrap();
    draw(cx, &o);
    let raw = serde_json::to_value(&fake.snapshot.lock().unwrap().settings).unwrap();
    fake.snapshot.lock().unwrap().settings = serde_json::from_value(raw).unwrap();
    let reopened = open(cx, fake, Some("kind:device"));
    assert_eq!(
        reopened.view.read_with(cx, |v, _| v
            .settings()
            .unwrap()
            .overlay
            .outputs
            .iter()
            .find(|o| o.id == card)
            .unwrap()
            .device_mode_id
            .clone()),
        "decklink:25"
    );
}
#[gpui_kit::test]
fn save_and_test_failures_show_inline_and_keep_settings(cx: &mut TestAppContext) {
    let o = open(cx, Arc::new(Fake::new()), Some("screen-a"));
    *o.fake.error.lock().unwrap() = Some("Disk full".into());
    click(cx, &o, "screens-group-false");
    assert!(
        o.view
            .read_with(cx, |v, _| v.error().unwrap().contains("Disk full"))
    );
    assert!(o.fake.snapshot.lock().unwrap().settings.overlay.outputs[0].enabled);
    click(cx, &o, "screens-test");
    assert_eq!(
        o.view.read_with(cx, |v, _| v.error().map(str::to_owned)),
        Some("Disk full".into())
    );
}
#[gpui_kit::test]
fn stage_create_layout_and_identify(cx: &mut TestAppContext) {
    let o = open(cx, Arc::new(Fake::new()), None);
    cx.update_window(o.handle, |_, w, cx| {
        o.view.update(cx, |v, cx| v.add_stage(w, cx))
    })
    .unwrap();
    draw(cx, &o);
    let id = o
        .fake
        .snapshot
        .lock()
        .unwrap()
        .settings
        .presentation
        .stage_displays[0]
        .id
        .clone();
    cx.update_window(o.handle, |_, w, cx| {
        o.view.update(cx, |v, cx| {
            v.edit(
                Edit::Stage {
                    id,
                    patch: serde_json::json!({"showClock":false}),
                },
                w,
                cx,
            )
        })
    })
    .unwrap();
    draw(cx, &o);
    assert!(
        !o.fake
            .snapshot
            .lock()
            .unwrap()
            .settings
            .presentation
            .stage_displays[0]
            .show_clock
    );
    click(cx, &o, "screens-identify");
    assert!(o.fake.calls.lock().unwrap().contains(&"Identify".into()));
}

#[gpui_kit::test]
fn context_delete_targets_clicked_row_and_survives_restart(cx: &mut TestAppContext) {
    use gpui_kit::test::TestQueryExt;
    let fake = Arc::new(Fake::new());
    let o = open(cx, fake.clone(), Some("screen-a"));
    cx.update_window(o.handle, |_, w, cx| {
        w.right_click("screens-row-screen-b", cx)
    })
    .unwrap();
    draw(cx, &o);
    cx.update_window(o.handle, |_, w, cx| {
        let item = w.find_by_label("Delete").path().last().unwrap().clone();
        w.click(item, cx);
    })
    .unwrap();
    draw(cx, &o);
    // The real confirmation has the clicked row's name, while A stays selected.
    assert_eq!(
        o.view.read_with(cx, |v, _| v.selected().map(str::to_owned)),
        Some("screen-a".into())
    );
    cx.update_window(o.handle, |_, w, cx| {
        let items = w.elements();
        let confirm = items
            .iter()
            .find(|e| e.label() == Some("Delete") && e.role() == Some(gpui_kit::Role::Button))
            .expect("Delete confirmation");
        let id = confirm.path().last().unwrap().clone();
        w.click(id, cx);
    })
    .unwrap();
    draw(cx, &o);
    let saved = serde_json::to_value(&fake.snapshot.lock().unwrap().settings).unwrap();
    let roundtrip: AppSettings = serde_json::from_value(saved).unwrap();
    assert!(roundtrip.overlay.outputs.iter().any(|o| o.id == "screen-a"));
    assert!(!roundtrip.overlay.outputs.iter().any(|o| o.id == "screen-b"));
    fake.snapshot.lock().unwrap().settings = roundtrip;
    let reopened = open(cx, fake, None);
    assert!(!reopened.view.read_with(cx, |v, _| {
        v.settings()
            .unwrap()
            .overlay
            .outputs
            .iter()
            .any(|o| o.id == "screen-b")
    }));
}

#[gpui_kit::test]
fn a_topic_update_preserves_a_focused_name_draft(cx: &mut TestAppContext) {
    use gpui_kit::test::TestQueryExt;
    let o = open(cx, Arc::new(Fake::new()), Some("screen-a"));
    cx.update_window(o.handle, |_, w, cx| {
        let id = w.find_by_label("Name").path().last().unwrap().clone();
        w.click(id, cx);
        w.press("secondary-a", cx);
        w.input("Renaming", cx);
    })
    .unwrap();
    draw(cx, &o);
    let next = o.fake.snapshot.lock().unwrap().clone();
    o.fake.events.lock().unwrap()[0]
        .unbounded_send(Ok(next))
        .unwrap();
    draw(cx, &o);
    let value = cx
        .update_window(o.handle, |_, w, _| {
            w.find_by_label("Name").value().map(str::to_owned)
        })
        .unwrap();
    assert_eq!(value, Some("Renaming".into()));
}

#[gpui_kit::test]
fn numeric_fields_follow_external_settings_and_restore_failed_writes(cx: &mut TestAppContext) {
    use gpui_kit::test::TestQueryExt;
    let fake = Arc::new(Fake::new());
    fake.snapshot.lock().unwrap().settings.overlay.outputs[0].aspect =
        kairo_domain::overlay::OutputAspect::Custom;
    let o = open(cx, fake, Some("screen-a"));
    {
        let mut s = o.fake.snapshot.lock().unwrap();
        s.settings.overlay.outputs[0].custom_aspect.width = 42.;
        o.fake.events.lock().unwrap()[0]
            .unbounded_send(Ok(s.clone()))
            .unwrap();
    }
    draw(cx, &o);
    let value = cx
        .update_window(o.handle, |_, w, _| {
            w.find_by_label("Ratio width").value().map(str::to_owned)
        })
        .unwrap();
    assert_eq!(value, Some("42".into()));
    *o.fake.error.lock().unwrap() = Some("Disk full".into());
    cx.update_window(o.handle, |_, w, cx| {
        w.within("screens-ratio-width").click("input-state", cx);
        w.press("secondary-a", cx);
        w.input("77", cx);
    })
    .unwrap();
    draw(cx, &o);
    let value = cx
        .update_window(o.handle, |_, w, _| {
            w.find_by_label("Ratio width").value().map(str::to_owned)
        })
        .unwrap();
    assert_eq!(value, Some("42".into()));
}

#[gpui_kit::test]
fn failed_initial_read_can_retry(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::new());
    *fake.load_error.lock().unwrap() = Some("Could not read settings".into());
    let o = open(cx, fake, None);
    assert!(o.view.read_with(cx, |v, _| v.settings().is_none()));
    assert!(shows(cx, &o, "screens-retry"));
    *o.fake.load_error.lock().unwrap() = None;
    click(cx, &o, "screens-retry");
    assert!(o.view.read_with(cx, |v, _| v.settings().is_some()));
}

#[gpui_kit::test]
fn empty_audience_stays_empty_and_can_add_a_screen(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::new());
    fake.snapshot
        .lock()
        .unwrap()
        .settings
        .overlay
        .outputs
        .clear();
    let o = open(cx, fake, None);
    assert!(o.view.read_with(cx, |v, _| v.selected().is_none()));
    cx.update_window(o.handle, |_, w, cx| {
        o.view
            .update(cx, |v, cx| v.add_output(OverlayOutputKind::Screen, w, cx))
    })
    .unwrap();
    draw(cx, &o);
    assert_eq!(
        o.fake
            .snapshot
            .lock()
            .unwrap()
            .settings
            .overlay
            .outputs
            .len(),
        1
    );
}

#[gpui_kit::test]
fn ndi_audio_is_disabled_when_the_runtime_cannot_send_it(cx: &mut TestAppContext) {
    use gpui_kit::test::TestQueryExt;
    let fake = Arc::new(Fake::new());
    fake.snapshot.lock().unwrap().settings.overlay.outputs[0].kind = OverlayOutputKind::Ndi;
    let o = open(cx, fake, Some("screen-a"));
    let tab = ElementId::NamedChild(Arc::new("screens-tabs".into()), "sound".into());
    click(cx, &o, tab);
    let disabled = cx
        .update_window(o.handle, |_, w, _| {
            w.find_by_label("Send program sound with every NDI feed")
                .disabled()
        })
        .unwrap();
    assert_eq!(disabled, Some(true));
}

#[gpui_kit::test]
fn card_refresh_failure_releases_the_button(cx: &mut TestAppContext) {
    let o = open(cx, Arc::new(Fake::new()), Some("add:device"));
    *o.fake.error.lock().unwrap() = Some("Could not refresh video outputs".into());
    click(cx, &o, "screens-refresh");
    assert!(
        o.fake
            .calls
            .lock()
            .unwrap()
            .contains(&"RefreshCards".into())
    );
    assert!(o.view.read_with(cx, |v, _| v.error().is_some()));
    *o.fake.error.lock().unwrap() = None;
    click(cx, &o, "screens-refresh");
    assert_eq!(
        o.fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.as_str() == "RefreshCards")
            .count(),
        2
    );
}

#[gpui_kit::test]
fn keyboard_resize_persists_the_frame(cx: &mut TestAppContext) {
    let o = open(cx, Arc::new(Fake::new()), None);
    click(cx, &o, "screens-resize");
    cx.update_window(o.handle, |_, w, cx| w.press("left", cx))
        .unwrap();
    draw(cx, &o);
    let stored = o
        .fake
        .ui
        .lock()
        .unwrap()
        .get("kairo.frame.screens.v4")
        .cloned()
        .expect("frame persisted");
    let value: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(value["w"], 984.);
}

#[gpui_kit::test]
fn closing_while_a_save_is_pending_flushes_later_edits(cx: &mut TestAppContext) {
    let o = open(cx, Arc::new(Fake::new()), Some("screen-a"));
    let (_release, pause) = futures_channel::oneshot::channel();
    *o.fake.pause.lock().unwrap() = Some(pause);
    cx.update_window(o.handle, |_, w, cx| {
        o.view.update(cx, |v, cx| {
            v.edit(
                Edit::Output {
                    id: "screen-a".into(),
                    patch: serde_json::json!({"name":"First"}),
                },
                w,
                cx,
            );
            v.edit(
                Edit::Output {
                    id: "screen-b".into(),
                    patch: serde_json::json!({"name":"Second"}),
                },
                w,
                cx,
            );
            v.close(w, cx);
        })
    })
    .unwrap();
    assert_eq!(
        o.fake.snapshot.lock().unwrap().settings.overlay.outputs[1].name,
        "Second"
    );
}

struct PreviewLifetime {
    engine: Arc<kairo_output::fake::FakeEngine>,
    output: String,
    alive: Arc<std::sync::atomic::AtomicUsize>,
}
impl Drop for PreviewLifetime {
    fn drop(&mut self) {
        use kairo_core::engine::OutputEngine;
        self.engine
            .command(kairo_engine_proto::Command::CloseSurface {
                output: self.output.clone(),
                destroy: true,
            });
        self.alive.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[gpui_kit::test]
fn theme_tab_uses_the_complete_engine_theme_and_releases_its_surface(cx: &mut TestAppContext) {
    use std::sync::atomic::Ordering;
    let mut fake = Fake::new();
    fake.preview_engine = Some(Arc::new(kairo_output::fake::FakeEngine::new()));
    let fake = Arc::new(fake);
    let expected = fake.snapshot.lock().unwrap().settings.overlay.outputs[0]
        .theme
        .clone();
    let o = open(cx, fake.clone(), Some("screen-a"));
    let tab =
        |key: &str| ElementId::NamedChild(Arc::new("screens-tabs".into()), key.to_owned().into());
    assert_eq!(fake.preview_alive.load(Ordering::SeqCst), 0);
    click(cx, &o, tab("theme"));
    assert_eq!(fake.preview_alive.load(Ordering::SeqCst), 1);
    assert_eq!(fake.preview_themes.lock().unwrap().as_slice(), &[expected]);
    {
        let mut next = fake.snapshot.lock().unwrap();
        next.settings.overlay.outputs[0].theme.background.opacity = 0.45;
        fake.events.lock().unwrap()[0]
            .unbounded_send(Ok(next.clone()))
            .unwrap();
    }
    draw(cx, &o);
    assert_eq!(
        fake.preview_themes
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .background
            .opacity,
        0.45
    );
    click(cx, &o, tab("look"));
    assert_eq!(fake.preview_alive.load(Ordering::SeqCst), 0);
    assert!(
        fake.preview_engine
            .as_ref()
            .unwrap()
            .commands()
            .iter()
            .any(|c| matches!(
                c,
                kairo_engine_proto::Command::CloseSurface { destroy: true, .. }
            ))
    );
    click(cx, &o, tab("theme"));
    cx.update_window(o.handle, |_, w, cx| {
        o.view.update(cx, |v, cx| v.close(w, cx))
    })
    .unwrap();
    assert_eq!(fake.preview_alive.load(Ordering::SeqCst), 0);
}
