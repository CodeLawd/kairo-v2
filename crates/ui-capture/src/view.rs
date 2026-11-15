//! The Live popover body and the Capture Settings window.

use std::rc::Rc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Context, ElementId, Entity, EventEmitter, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, size,
};
use kairo_domain::capture::{
    CAPTURE_BITRATES, CAPTURE_CODECS, CAPTURE_FRAME_RATES, CAPTURE_RESOLUTIONS, capture_source_for,
    capture_sources,
};
use kairo_ui_kit::components::modal::{MovableDialog, MovableDialogEvent};
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::Capture;
use crate::model::{
    audio_hint, audio_missing, capture_disabled, capture_label, capture_plan, capture_title,
    compressed, kind_label, short_folder, size_hint, source_hint, syphon_action, syphon_title,
};

/// Element ids, for tests.
pub mod ids {
    pub const CAPTURE: &str = "live-capture";
    pub const CAPTURE_SETTINGS: &str = "live-capture-settings";
    pub const SYPHON_SETUP: &str = "live-syphon-settings";
    pub const WINDOW_CLOSE: &str = "capture-settings-close";
    pub const WINDOW_SAVE: &str = "capture-settings-save";
    pub const WINDOW_REVEAL: &str = "capture-settings-reveal";
    pub const WINDOW_FOLDER: &str = "capture-settings-folder";
    pub const WINDOW_ERROR: &str = "capture-settings-error";
}

/// What the popover asks of the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveMenuEvent {
    /// Close the popover and open Capture Settings.
    OpenSettings,
    /// Close the popover and open Screens at a target.
    OpenScreens(&'static str),
}

/// The Live popover's body (`LiveMenu`).
pub struct LiveMenu {
    capture: Entity<Capture>,
    /// Off the Mac only the detail line shows.
    mac: bool,
    detail: SharedString,
    _subscription: Subscription,
}

impl EventEmitter<LiveMenuEvent> for LiveMenu {}

const MENU_ITEM_H: f32 = 32.;

fn menu_item(id: impl Into<ElementId>, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let k = cx.kairo();
    let hover = k.surface_menu_hover;
    h_flex()
        .id(id)
        .role(Role::MenuItem)
        .min_h(px(MENU_ITEM_H))
        .w_full()
        .items_center()
        .gap_2()
        .rounded_md()
        .px_3()
        .py_2()
        .text_size(text::SM.size)
        .text_color(k.text(TextStep::T200))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

fn hairline(cx: &App) -> AnyElement {
    div()
        .mx_3()
        .my_1()
        .h(px(0.5))
        .bg(cx.kairo().surface_menu_line)
        .into_any_element()
}

impl LiveMenu {
    pub fn new(capture: Entity<Capture>, mac: bool, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&capture, |_, _, cx| cx.notify());
        Self {
            capture,
            mac,
            detail: SharedString::default(),
            _subscription: subscription,
        }
    }

    /// The Live button's detail line, shown off the Mac.
    pub fn set_detail(&mut self, detail: SharedString, cx: &mut Context<Self>) {
        if self.detail != detail {
            self.detail = detail;
            cx.notify();
        }
    }
}

impl Render for LiveMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        if !self.mac {
            return v_flex()
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .text_size(text::SM.size)
                        .text_color(k.text(TextStep::T200))
                        .child(self.detail.clone()),
                )
                .into_any_element();
        }
        let capture = self.capture.read(cx);
        let status = capture.status.clone();
        let busy = capture.busy;
        let has_source = capture_source_for(&capture.settings, &capture.outputs).is_some();
        let plan = capture_plan(&capture.settings, &capture.outputs);
        let disabled = capture_disabled(&status, busy, has_source);
        let title = capture_title(&status, &plan);
        let elapsed = status.recording.then(|| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.);
            kairo_domain::capture::format_capture_elapsed(
                status.started_at.map_or(0., |at| (now - at).max(0.)),
            )
        });
        let syphons: Vec<_> = capture
            .syphons()
            .into_iter()
            .map(|o| {
                let ready = capture.readiness(&o.id);
                (
                    o.id.clone(),
                    o.name.clone(),
                    o.enabled,
                    syphon_title(
                        o.enabled,
                        ready.and_then(|r| r.ready),
                        ready.and_then(|r| r.reason.as_deref()),
                    ),
                )
            })
            .collect();
        let (setup_label, setup_target) = syphon_action(syphons.len());
        let entity = self.capture.clone();

        let mut start = menu_item(ids::CAPTURE, cx)
            .aria_label(capture_label(&status, busy))
            .child(div().flex_1().child(capture_label(&status, busy)))
            .when_some(elapsed, |this, e| {
                this.child(
                    div()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T400))
                        .child(e),
                )
            });
        if disabled {
            start = start.opacity(0.5).cursor_default();
        } else {
            let entity = entity.clone();
            start = start.on_click(move |_, _, cx| entity.update(cx, |c, cx| c.toggle(cx)));
        }
        if let Some(title) = title {
            start = start.tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx)
            });
        }
        let mut body = v_flex().child(start);
        if status.recording
            && let Some(warning) = &status.warning
        {
            body = body.child(
                div()
                    .px_3()
                    .py_1()
                    .text_size(text::XS.size)
                    .text_color(k.warning_text)
                    .child(warning.clone()),
            );
        }
        if !status.recording
            && let Some(error) = &status.error
        {
            body = body.child(
                div()
                    .id("live-capture-error")
                    .role(Role::Alert)
                    .px_3()
                    .py_1()
                    .text_size(text::XS.size)
                    .text_color(k.danger_text)
                    .child(error.clone()),
            );
        }
        body = body
            .child(hairline(cx))
            .child(
                menu_item(ids::CAPTURE_SETTINGS, cx)
                    .child("Capture Settings…")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(LiveMenuEvent::OpenSettings))),
            )
            .child(hairline(cx));
        for (index, (id, name, enabled, tip)) in syphons.into_iter().enumerate() {
            let entity = entity.clone();
            body = body.child(
                h_flex()
                    .id(ElementId::NamedInteger("live-syphon".into(), index as u64))
                    .min_h(px(MENU_ITEM_H))
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .px_3()
                    .py_2()
                    .hover(move |s| s.bg(k.surface_menu_hover))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(text::SM.size)
                            .text_color(k.text(TextStep::T200))
                            .child(name.clone()),
                    )
                    .child(
                        Switch::new(ElementId::NamedInteger(
                            "live-syphon-switch".into(),
                            index as u64,
                        ))
                        .small()
                        .checked(enabled)
                        .accessibility_label(format!("Send {name} over Syphon"))
                        .on_change(move |on, _, cx| {
                            let on = *on;
                            let id = id.clone();
                            entity.update(cx, |c, cx| c.set_output_enabled(&id, on, cx))
                        }),
                    ),
            );
        }
        body.child(
            menu_item(ids::SYPHON_SETUP, cx)
                .child(setup_label)
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(LiveMenuEvent::OpenScreens(setup_target))
                })),
        )
        .into_any_element()
    }
}

/// The settings window closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureWindowEvent;

/// Capture Settings (`CaptureSettings.tsx`).
pub struct CaptureSettingsWindow {
    capture: Entity<Capture>,
    frame: Entity<MovableDialog>,
    inputs: Vec<(String, String)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CaptureWindowEvent> for CaptureSettingsWindow {}

/// One `Row`: label, optional hint, control.
fn row(label: &str, hint: Option<String>, control: AnyElement, cx: &App) -> AnyElement {
    let k = cx.kairo();
    h_flex()
        .min_h(px(44.))
        .items_center()
        .justify_between()
        .gap_3()
        .px_3p5()
        .py_2()
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .text_size(text::SM.size)
                        .text_color(k.text_primary)
                        .child(label.to_owned()),
                )
                .when_some(hint, |this, hint| {
                    this.child(
                        div()
                            .mt_1()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(hint),
                    )
                }),
        )
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}

/// One `Group`: a title over rounded rows.
fn group(title: &str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let k = cx.kairo();
    v_flex()
        .gap_2()
        .child(
            div()
                .px_0p5()
                .text_size(text::SM.size)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(k.text_primary)
                .child(title.to_owned()),
        )
        .child(
            v_flex()
                .rounded(radius::xl())
                .overflow_hidden()
                .bg(k.surface_tertiary)
                .children(rows),
        )
        .into_any_element()
}

/// A select (`SELECT`): the current label, a menu of options.
fn select(
    id: &'static str,
    label: &'static str,
    current: String,
    options: Vec<(String, String, bool)>,
    change: impl Fn(String, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let change = Rc::new(change);
    Button::new(id)
        .small()
        .w(px(220.))
        .label(current)
        .accessibility_label(label)
        .dropdown_menu(move |mut menu, _, _| {
            for (value, text, checked) in &options {
                let change = change.clone();
                let value = value.clone();
                menu = menu.item(
                    PopupMenuItem::new(text.clone())
                        .checked(*checked)
                        .on_click(move |_, w, cx| change(value.clone(), w, cx)),
                );
            }
            menu
        })
        .into_any_element()
}

impl CaptureSettingsWindow {
    pub fn new(capture: Entity<Capture>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let viewport = window.viewport_size();
        let frame = cx.new(|cx| {
            MovableDialog::new(
                "capture-settings",
                "Capture Settings",
                // `max-w-[520px] max-h-[min(720px,92vh)]`.
                size(
                    px(520f32.min(f32::from(viewport.width) - 32.)),
                    px(640f32.min(f32::from(viewport.height) * 0.92)),
                ),
                window,
                cx,
            )
        });
        let subscriptions = vec![
            cx.subscribe(&frame, |_, _, event: &MovableDialogEvent, cx| {
                if matches!(event, MovableDialogEvent::Dismiss) {
                    cx.emit(CaptureWindowEvent);
                }
            }),
            cx.observe(&capture, |_, _, cx| cx.notify()),
        ];
        // `capture.audioInputs()`; none on failure.
        let (tx, rx) = futures_channel::oneshot::channel();
        capture.read(cx).backend().audio_inputs(Box::new(move |r| {
            let _ = tx.send(r);
        }));
        cx.spawn(async move |this, cx| {
            let inputs = rx.await.ok().and_then(Result::ok).unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.inputs = inputs.into_iter().map(|i| (i.id, i.name)).collect();
                cx.notify();
            });
        })
        .detach();
        Self {
            capture,
            frame,
            inputs: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Returns focus to what had it before.
    pub fn restore_focus(&self, window: &mut Window, cx: &mut App) {
        let frame = self.frame.clone();
        frame.update(cx, |f, cx| f.restore(window, cx));
    }
}

impl Render for CaptureSettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let c = self.capture.read(cx);
        let settings = c.settings.clone();
        let outputs = c.outputs.clone();
        let status = c.status.clone();
        let sources: Vec<_> = capture_sources(&outputs).cloned().collect();
        let source = capture_source_for(&settings, &outputs).cloned();
        let codec = CAPTURE_CODECS
            .iter()
            .find(|c| c.0 == settings.codec)
            .unwrap_or(&CAPTURE_CODECS[0]);
        let missing = audio_missing(&settings, &self.inputs);
        let recording = status.recording;
        let entity = self.capture.clone();
        let save = move |edit: Box<dyn FnOnce(&mut kairo_domain::capture::CaptureSettings)>,
                         cx: &mut App| {
            entity.update(cx, |c, cx| c.save(edit, cx));
        };
        let save = Rc::new(save);

        let source_control = if sources.is_empty() {
            div()
                .text_size(text::LABEL.size)
                .text_color(k.text(TextStep::T500))
                .child("No screens yet")
                .into_any_element()
        } else {
            let label = |o: &kairo_domain::overlay::OverlayOutput| {
                format!(
                    "{} · {}{}",
                    o.name,
                    kind_label(o.kind),
                    if o.enabled { "" } else { " (off)" }
                )
            };
            let save = save.clone();
            select(
                "capture-source",
                "Source",
                source.as_ref().map(label).unwrap_or_default(),
                sources
                    .iter()
                    .map(|o| {
                        (
                            o.id.clone(),
                            label(o),
                            source.as_ref().is_some_and(|s| s.id == o.id),
                        )
                    })
                    .collect(),
                move |id, _, cx| save(Box::new(move |s| s.source_output_id = id), cx),
            )
        };
        let picture = vec![
            row(
                "Source",
                source_hint(&settings, &outputs),
                source_control,
                cx,
            ),
            row(
                "Size",
                None,
                {
                    let save = save.clone();
                    select(
                        "capture-size",
                        "Size",
                        CAPTURE_RESOLUTIONS
                            .iter()
                            .find(|r| r.0 == settings.resolution)
                            .map(|r| r.1.to_owned())
                            .unwrap_or_default(),
                        CAPTURE_RESOLUTIONS
                            .iter()
                            .map(|r| {
                                (
                                    r.0.as_str().to_owned(),
                                    r.1.to_owned(),
                                    r.0 == settings.resolution,
                                )
                            })
                            .collect(),
                        move |v, _, cx| {
                            if let Some(r) = kairo_domain::capture::CaptureResolution::from_name(&v)
                            {
                                save(Box::new(move |s| s.resolution = r), cx)
                            }
                        },
                    )
                },
                cx,
            ),
            row(
                "Frame rate",
                None,
                {
                    let save = save.clone();
                    select(
                        "capture-rate",
                        "Frame rate",
                        format!("{} fps", settings.frame_rate.value()),
                        CAPTURE_FRAME_RATES
                            .iter()
                            .map(|f| {
                                (
                                    f.value().to_string(),
                                    format!("{} fps", f.value()),
                                    *f == settings.frame_rate,
                                )
                            })
                            .collect(),
                        move |v, _, cx| {
                            if let Some(f) = CAPTURE_FRAME_RATES
                                .iter()
                                .copied()
                                .find(|f| f.value().to_string() == v)
                            {
                                save(Box::new(move |s| s.frame_rate = f), cx)
                            }
                        },
                    )
                },
                cx,
            ),
        ];
        let mut file = vec![row(
            "Format",
            Some(codec.2.to_owned()),
            {
                let save = save.clone();
                select(
                    "capture-format",
                    "Format",
                    codec.1.to_owned(),
                    CAPTURE_CODECS
                        .iter()
                        .map(|c| {
                            (
                                c.0.as_str().to_owned(),
                                c.1.to_owned(),
                                c.0 == settings.codec,
                            )
                        })
                        .collect(),
                    move |v, _, cx| {
                        if let Some(c) = kairo_domain::capture::CaptureCodec::from_name(&v) {
                            save(Box::new(move |s| s.codec = c), cx)
                        }
                    },
                )
            },
            cx,
        )];
        if compressed(&settings) {
            let mbps = settings.bitrate_mbps;
            let mut options: Vec<(String, String, bool)> = Vec::new();
            // A stored rate off the list stays choosable.
            if !CAPTURE_BITRATES.iter().any(|b| f64::from(b.0) == mbps) {
                options.push((mbps.to_string(), format!("{mbps} Mbps"), true));
            }
            options.extend(
                CAPTURE_BITRATES
                    .iter()
                    .map(|b| (b.0.to_string(), b.1.to_owned(), f64::from(b.0) == mbps)),
            );
            let current = options
                .iter()
                .find(|o| o.2)
                .map(|o| o.1.clone())
                .unwrap_or_default();
            let save = save.clone();
            file.push(row(
                "Quality",
                None,
                select(
                    "capture-quality",
                    "Quality",
                    current,
                    options,
                    move |v, _, cx| {
                        if let Ok(rate) = v.parse::<f64>() {
                            save(Box::new(move |s| s.bitrate_mbps = rate), cx)
                        }
                    },
                ),
                cx,
            ));
        }
        let folder_title = if settings.folder.is_empty() {
            "Movies/Kairo in your home folder".to_owned()
        } else {
            settings.folder.clone()
        };
        let chooser = self.capture.clone();
        file.push(row(
            "Save to",
            Some(size_hint(&settings)),
            h_flex()
                .gap_2()
                .child(
                    div()
                        .id("capture-folder-name")
                        .max_w(px(150.))
                        .truncate()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T300))
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(folder_title.clone())
                                .build(window, cx)
                        })
                        .child(short_folder(&settings.folder)),
                )
                .child(
                    secondary_button(ids::WINDOW_FOLDER, "Choose…", cx).on_click(
                        move |_, _, cx| {
                            let capture = chooser.clone();
                            let (tx, rx) = futures_channel::oneshot::channel();
                            capture.read(cx).backend().choose_folder(Box::new(move |r| {
                                let _ = tx.send(r);
                            }));
                            cx.spawn(async move |cx| {
                                if let Ok(Ok(Some(folder))) = rx.await {
                                    capture.update(cx, |c, cx| {
                                        c.settings.folder = folder;
                                        cx.notify();
                                    });
                                }
                            })
                            .detach();
                        },
                    ),
                )
                .into_any_element(),
            cx,
        ));
        let mut audio_options = vec![(
            String::new(),
            "No sound".to_owned(),
            settings.audio_device_id.is_empty(),
        )];
        if missing {
            audio_options.push((
                settings.audio_device_id.clone(),
                if settings.audio_device_name.is_empty() {
                    "Disconnected input".to_owned()
                } else {
                    settings.audio_device_name.clone()
                },
                true,
            ));
        }
        audio_options.extend(
            self.inputs
                .iter()
                .map(|(id, name)| (id.clone(), name.clone(), *id == settings.audio_device_id)),
        );
        let audio_current = audio_options
            .iter()
            .find(|o| o.2)
            .map(|o| o.1.clone())
            .unwrap_or_else(|| settings.audio_device_name.clone());
        let inputs = self.inputs.clone();
        let sound = vec![row(
            "Audio input",
            Some(audio_hint(&settings, missing)),
            {
                let save = save.clone();
                select(
                    "capture-audio",
                    "Audio input",
                    audio_current,
                    audio_options,
                    move |id, _, cx| {
                        let name = inputs
                            .iter()
                            .find(|(i, _)| *i == id)
                            .map(|(_, n)| n.clone())
                            .unwrap_or_default();
                        save(
                            Box::new(move |s| {
                                s.audio_device_id = id;
                                s.audio_device_name = name;
                            }),
                            cx,
                        )
                    },
                )
            },
            cx,
        )];

        let handle = self
            .frame
            .update(cx, |f, cx| f.drag_handle("capture-settings-drag", cx));
        let header = handle
            .flex()
            .items_center()
            .gap_3()
            .px_5()
            .pb_2()
            .pt_4()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child("Capture Settings"),
            )
            .child(
                div()
                    .id(ids::WINDOW_CLOSE)
                    .test_support()
                    .role(Role::Button)
                    .aria_label("Close")
                    .size_7()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(k.text(TextStep::T400))
                    .hover(move |s| s.bg(k.surface_tertiary).text_color(k.text_primary))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CaptureWindowEvent)))
                    .child(Icon::new(KairoIcon::X).size(px(14.))),
            );
        let mut body = v_flex().gap_5();
        if recording {
            body = body.child(
                div()
                    .rounded(radius::lg())
                    .bg(k.surface_tertiary)
                    .px_3p5()
                    .py_2p5()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T300))
                    .child(format!(
                        "Recording {} — changes here apply to the next recording.",
                        status.source_name.clone().unwrap_or_default()
                    )),
            );
        }
        body = body
            .child(group("Picture", picture, cx))
            .child(group("File", file, cx))
            .child(group("Sound", sound, cx));
        if !recording && let Some(error) = &status.error {
            body = body.child(
                div()
                    .id(ids::WINDOW_ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .rounded(radius::lg())
                    .bg(k.tints.red)
                    .px_3p5()
                    .py_2p5()
                    .text_size(text::LABEL.size)
                    .text_color(k.danger_text_soft)
                    .child(error.clone()),
            );
        }
        let reveal = self.capture.clone();
        let footer = h_flex()
            .gap_2()
            .bg(k.surface_tertiary)
            .px_5()
            .py_3p5()
            .child(
                secondary_button(ids::WINDOW_REVEAL, "Show in Finder", cx)
                    .on_click(move |_, _, cx| reveal.read(cx).backend().reveal()),
            )
            .child(div().flex_1())
            .child(
                primary_button(ids::WINDOW_SAVE, "Save", cx)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CaptureWindowEvent))),
            );
        let content = v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("capture-settings-body")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px_5()
                    .pb_5()
                    .pt_2()
                    .child(body),
            )
            .child(footer);
        self.frame.update(cx, |f, cx| f.frame(content, window, cx))
    }
}

fn primary_button(id: &'static str, label: &'static str, cx: &App) -> Button {
    let k = cx.kairo();
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(k.accent(Step::S500))
                .foreground(k.on_accent)
                .hover(k.accent(Step::S600))
                .active(k.accent(Step::S600)),
        )
        .small()
        .label(label)
}

fn secondary_button(id: &'static str, label: &'static str, cx: &App) -> Button {
    let k = cx.kairo();
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(k.surface_elevated)
                .foreground(k.text(TextStep::T200))
                .hover(k.surface_border)
                .active(k.surface_border),
        )
        .small()
        .label(label)
}
