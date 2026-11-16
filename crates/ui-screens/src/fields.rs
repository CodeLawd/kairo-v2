use crate::{
    Command, ScreensWindow,
    model::{self, Edit},
};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kairo_domain::displays::{OUTPUT_ASPECTS, describe_display, resolve_display};
use kairo_domain::program::{OutputShowKey, output_show_label};
use kairo_domain::{overlay::*, program::StageDisplayConfig};
use kairo_ui_kit::components::PopoverTriggerExt;
use kairo_ui_kit::{
    KairoIcon, KairoThemeExt,
    components::{NumberField, NumberFieldChanged, NumberModel},
    tokens::{TextStep, radius, text},
};
use serde_json::{Value, json};

pub(crate) fn muted(text_: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .text_size(text::LABEL.size)
        .text_color(cx.kairo().text(TextStep::T500))
        .child(text_.into())
        .into_any_element()
}
pub(crate) fn toggle(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    disabled: bool,
    change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    Switch::new(id)
        .checked(on)
        .disabled(disabled)
        .accessibility_label(label)
        .on_change(move |v, w, cx| change(*v, w, cx))
        .into_any_element()
}
fn field(label: &str, control: impl IntoElement) -> AnyElement {
    v_flex()
        .gap_2()
        .child(div().text_size(text::SM.size).child(label.to_owned()))
        .child(control)
        .into_any_element()
}
fn row(label: &str, control: impl IntoElement) -> AnyElement {
    h_flex()
        .items_center()
        .justify_between()
        .gap_4()
        .py_1p5()
        .child(div().text_size(text::SM.size).child(label.to_owned()))
        .child(control)
        .into_any_element()
}

#[derive(Clone)]
struct Choice {
    value: String,
    label: String,
    disabled: bool,
}
impl Choice {
    fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
        }
    }
    fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }
}
fn choices(
    id: impl Into<ElementId>,
    label: &str,
    value: &str,
    options: Vec<Choice>,
    change: impl Fn(String, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let current = options
        .iter()
        .find(|o| o.value == value)
        .map(|o| o.label.clone())
        .unwrap_or_else(|| value.to_owned());
    let change = std::rc::Rc::new(change);
    let value = value.to_owned();
    // A select (`input`): full width, the choice on the left, a caret on the right.
    // Electron's `.input` select: filled, 36 px tall, 12 px a side.
    Button::new(id)
        .secondary()
        .with_size(gpui_kit::component::Size::Medium)
        .h(px(36.))
        .px(px(12.))
        .w_full()
        .dropdown_caret(true)
        .label(current)
        .accessibility_label(label.to_owned())
        .popover_trigger()
        .dropdown_menu(move |mut menu, _, _| {
            for option in &options {
                let change = change.clone();
                let next = option.value.clone();
                menu = menu.item(
                    PopupMenuItem::new(option.label.clone())
                        .checked(option.value == value)
                        .disabled(option.disabled)
                        .on_click(move |_, w, cx| change(next.clone(), w, cx)),
                );
            }
            menu
        })
        .into_any_element()
}
impl ScreensWindow {
    fn output_patch(
        &mut self,
        id: String,
        patch: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit(Edit::Output { id, patch }, window, cx);
    }
    fn choice(
        &self,
        id: &str,
        label: &str,
        value: &str,
        options: Vec<Choice>,
        edit: impl Fn(String) -> Edit + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let e = cx.entity();
        choices(
            SharedString::from(id.to_owned()),
            label,
            value,
            options,
            move |v, w, cx| e.update(cx, |t, cx| t.edit(edit(v), w, cx)),
        )
    }
    fn output_choice(
        &self,
        o: &OverlayOutput,
        key: &'static str,
        label: &str,
        value: &str,
        options: Vec<Choice>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = o.id.clone();
        self.choice(
            &format!("screens-{key}"),
            label,
            value,
            options,
            move |v| Edit::Output {
                id: id.clone(),
                patch: json!({key:v}),
            },
            cx,
        )
    }
    fn output_toggle(
        &self,
        o: &OverlayOutput,
        key: &'static str,
        label: &str,
        on: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = o.id.clone();
        let e = cx.entity();
        toggle(
            SharedString::from(format!("screens-{key}")),
            label,
            on,
            false,
            move |on, w, cx| {
                e.update(cx, |t, cx| {
                    t.output_patch(id.clone(), json!({key:on}), w, cx)
                })
            },
        )
    }
    fn number(
        &mut self,
        id: &str,
        label: &str,
        model: NumberModel,
        edit: impl Fn(f64) -> Edit + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = if let Some(field) = self.fields.get(id) {
            field.clone()
        } else {
            let owned = id.to_owned();
            let field = cx.new(|cx| {
                NumberField::new(
                    SharedString::from(owned),
                    label.to_owned(),
                    model,
                    window,
                    cx,
                )
            });
            self.field_subscriptions.push(cx.subscribe_in(
                &field,
                window,
                move |this, _, event: &NumberFieldChanged, w, cx| this.edit(edit(event.0), w, cx),
            ));
            self.fields.insert(id.into(), field.clone());
            field
        };
        field.into_any_element()
    }
    fn display_picker(
        &self,
        id: Option<f64>,
        label: &str,
        size: &Option<kairo_domain::overlay::Size>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = self.snapshot.as_ref().unwrap();
        let binding = model::binding(id, label, size);
        let resolved = resolve_display(&binding, &s.displays, &[]);
        let mut options = vec![Choice::new("", "None")];
        if id.is_some() && resolved.is_none() {
            options.push(
                Choice::new(
                    "missing",
                    format!(
                        "{} (not connected)",
                        if label.is_empty() {
                            "Previous display"
                        } else {
                            label
                        }
                    ),
                )
                .disabled(true),
            );
        }
        let key = self.selected.clone().unwrap_or_default();
        for d in &s.displays {
            let owner = model::display_owner(&s.settings, &key, d, &s.displays);
            let tag = owner
                .as_ref()
                .map(|n| format!(" — used by {n}"))
                .unwrap_or_else(|| {
                    if d.hosts_main_window {
                        " — Kairo’s controls".into()
                    } else {
                        String::new()
                    }
                });
            options.push(
                Choice::new(
                    d.id.to_string(),
                    format!(
                        "{}{tag}",
                        describe_display(&d.label, &d.size, d.scale_factor)
                    ),
                )
                .disabled(owner.is_some()),
            );
        }
        if !key.starts_with("stage:") {
            for p in &s.devices.ports {
                let reason = p.unavailable_reason.clone().or_else(|| {
                    s.settings
                        .overlay
                        .outputs
                        .iter()
                        .any(|o| {
                            o.id != key
                                && o.kind == OverlayOutputKind::Device
                                && o.device_port_id == p.id
                        })
                        .then_some("in use".into())
                });
                options.push(
                    Choice::new(
                        format!("card:{}", p.id),
                        format!(
                            "{}{}",
                            p.port_name,
                            reason
                                .as_ref()
                                .map(|r| format!(" — {r}"))
                                .unwrap_or_default()
                        ),
                    )
                    .disabled(reason.is_some()),
                );
            }
        }
        let value = resolved.map(|d| d.id.to_string()).unwrap_or_else(|| {
            if id.is_some() {
                "missing".into()
            } else {
                String::new()
            }
        });
        let e = cx.entity();
        let hint = resolved.is_some_and(|d| d.hosts_main_window);
        v_flex()
            .gap_2()
            .child(choices(
                "screens-output",
                "Output",
                &value,
                options,
                move |value, w, cx| {
                    e.update(cx, |t, cx| {
                        let s = t.snapshot.as_ref().unwrap();
                        if let Some(port) = value
                            .strip_prefix("card:")
                            .and_then(|id| s.devices.ports.iter().find(|p| p.id == id))
                            .cloned()
                        {
                            let next_id = crate::window::new_id("device");
                            let id = t.selected.clone().unwrap();
                            t.selected = Some(next_id.clone());
                            t.tab = "hardware".into();
                            t.clear_fields();
                            t.edit(Edit::MoveToCard { id, next_id, port }, w, cx);
                        } else {
                            let display = s
                                .displays
                                .iter()
                                .find(|d| d.id.to_string() == value)
                                .cloned();
                            t.bind(display, w, cx);
                        }
                    })
                },
            ))
            .when(hint, |d| {
                d.child(muted("Opens as a window — Kairo’s controls are here.", cx))
            })
            .into_any_element()
    }
    pub(crate) fn output_fields(
        &mut self,
        o: &OverlayOutput,
        tab: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = v_flex().gap_4();
        match tab {
            "display" => {
                // Electron's grid: Output | Fit, then Content in the left column.
                let half = |el: AnyElement| div().flex_1().min_w_0().child(el);
                let output = field(
                    "Output",
                    self.display_picker(o.display_id, &o.display_label, &o.display_size, cx),
                );
                let fit = (o.source != OutputSource::Playlist).then(|| self.fit(o, window, cx));
                body = body
                    .child(
                        h_flex()
                            .gap_4()
                            .items_start()
                            .child(half(output))
                            .child(half(fit.unwrap_or_else(|| div().into_any_element()))),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .items_start()
                            .child(half(self.source(o, window, cx)))
                            .child(half(div().into_any_element())),
                    );
            }
            "hardware" => body = body.child(self.hardware(o, window, cx)),
            "look" => {
                if o.kind == OverlayOutputKind::Screen && o.display_id.is_none() {
                    body = body
                        .child(muted(
                            "Nothing shows until this screen is on a display.",
                            cx,
                        ))
                        .child(
                            Button::new("screens-choose-display")
                                .small()
                                .label("Choose one")
                                .on_click(cx.listener(|t, _, _, cx| {
                                    t.tab = "display".into();
                                    cx.notify();
                                })),
                        );
                }
                if o.kind == OverlayOutputKind::Syphon {
                    body=body.child(muted(format!("In OBS, add a Syphon Client source and pick Kairo {}. Turn on its transparency option to put the words over your video input.",o.name),cx));
                }
                body = body.child(self.looks(o, cx));
            }
            "theme" => {
                body = body.child(self.theme_preview(o, cx));
                for (kind, label) in [
                    (OverlayContentKind::Scripture, "Scripture"),
                    (OverlayContentKind::Lyrics, "Lyrics"),
                ] {
                    let value = if kind == OverlayContentKind::Scripture {
                        o.theme_id.as_deref().unwrap_or("")
                    } else {
                        o.lyrics
                            .as_ref()
                            .map(|l| l.theme_id.as_deref().unwrap_or("custom"))
                            .unwrap_or("")
                    };
                    let mut options = vec![Choice::new(
                        "",
                        if kind == OverlayContentKind::Scripture {
                            "Custom"
                        } else {
                            "Same as scripture"
                        },
                    )];
                    if value == "custom" {
                        options.push(Choice::new("custom", "Custom").disabled(true));
                    }
                    for t in self
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .settings
                        .theme_library
                        .iter()
                        .filter(|t| t.kind == kind)
                    {
                        options.push(Choice::new(t.id.clone(), t.name.clone()));
                    }
                    let id = o.id.clone();
                    body = body.child(field(
                        label,
                        self.choice(
                            &format!("screens-theme-{}", kind.as_str()),
                            label,
                            value,
                            options,
                            move |value| Edit::Theme {
                                id: id.clone(),
                                kind,
                                theme: (!value.is_empty()).then_some(value),
                            },
                            cx,
                        ),
                    ));
                }
                body = body.child(muted("Design themes on the Theme page.", cx));
            }
            "sound" => {
                let s = self.snapshot.as_ref().unwrap();
                let e = cx.entity();
                body = body.child(row(
                    "Send program sound with every NDI feed",
                    toggle(
                        "screens-ndi-audio",
                        "Send program sound with every NDI feed",
                        s.settings.presentation.audio.ndi && s.ndi_audio,
                        !s.ndi_audio,
                        move |v, w, cx| {
                            e.update(cx, |t, cx| {
                                t.edit(Edit::Presentation(json!({"audio":{"ndi":v}})), w, cx)
                            })
                        },
                    ),
                ));
                if !s.ndi_audio {
                    body=body.child(muted("This machine’s NDI library can only send video. Sound needs the NDI 6 runtime Kairo ships for your platform.",cx));
                }
            }
            _ => {}
        }
        body.into_any_element()
    }
    fn fit(
        &mut self,
        o: &OverlayOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let options = OUTPUT_ASPECTS
            .iter()
            .map(|o| {
                Choice::new(
                    o.value.as_str(),
                    o.label.split(" — ").next().unwrap_or(o.label),
                )
            })
            .collect();
        let e = cx.entity();
        let id = o.id.clone();
        let fit = choices(
            "screens-fit",
            "Fit",
            o.aspect.as_str(),
            options,
            move |value, w, cx| {
                e.update(cx, |t, cx| {
                    t.output_patch(id.clone(), json!({"aspect":value}), w, cx);
                })
            },
        );
        let mut body = v_flex().gap_2().child(
            h_flex()
                .items_end()
                .gap_2()
                .child(div().flex_1().min_w_0().child(field("Fit", fit)))
                .child(
                    Button::new("screens-frame")
                        .secondary()
                        .with_size(gpui_kit::component::Size::Medium)
                        .size(px(36.))
                        .icon(KairoIcon::MonitorPlay)
                        .accessibility_label("Show the frame on this display")
                        .disabled(o.display_id.is_none() || self.busy)
                        .on_click({
                            let o = o.clone();
                            cx.listener(move |t, _, _, cx| {
                                if let Some(display_id) = o.display_id {
                                    t.run(
                                        Command::Frame(TestPatternInput {
                                            display_id,
                                            name: o.name.clone(),
                                            aspect: o.aspect,
                                            custom_aspect: o.custom_aspect.clone(),
                                        }),
                                        cx,
                                    )
                                }
                            })
                        }),
                ),
        );
        if o.aspect == OutputAspect::Custom {
            for (key, label, value) in [
                ("width", "Ratio width", o.custom_aspect.width),
                ("height", "Ratio height", o.custom_aspect.height),
            ] {
                let id = o.id.clone();
                body = body.child(self.number(
                    &format!("screens-ratio-{key}"),
                    label,
                    NumberModel::new(value, 1.).min(1.).max(100.),
                    move |v| Edit::Output {
                        id: id.clone(),
                        patch: json!({"customAspect":{key:v}}),
                    },
                    window,
                    cx,
                ));
            }
        }
        if let Some(hint) = fit_hint(o) {
            body = body.child(muted(hint, cx));
        }
        body.into_any_element()
    }
    fn source(
        &mut self,
        o: &OverlayOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = v_flex().gap_4().child(field(
            "Content",
            self.output_choice(
                o,
                "source",
                "Content",
                o.source.as_str(),
                vec![
                    Choice::new("program", "Follows the service"),
                    Choice::new("playlist", "Plays its own playlist"),
                ],
                cx,
            ),
        ));
        if o.source == OutputSource::Playlist {
            let playlists = &self.snapshot.as_ref().unwrap().playlists;
            let mut options = vec![Choice::new("", "Choose a playlist")];
            if !o.playlist_id.is_empty() && !playlists.iter().any(|p| p.id == o.playlist_id) {
                options.push(Choice::new(o.playlist_id.clone(), "Deleted playlist").disabled(true));
            }
            for p in playlists {
                options.push(Choice::new(
                    p.id.clone(),
                    format!("{} ({})", p.name, p.item_ids.len()),
                ));
            }
            let empty = playlists.is_empty();
            body = body.child(field(
                "Playlist",
                self.output_choice(o, "playlistId", "Playlist", &o.playlist_id, options, cx),
            ));
            if empty {
                body = body.child(muted("Make a playlist in the Media dock first.", cx));
            }
            let id = o.id.clone();
            body = body
                .child(self.number(
                    "screens-slide-sec",
                    "Image length",
                    NumberModel::new(o.slide_sec, 1.).min(3.).max(600.),
                    move |v| Edit::Output {
                        id: id.clone(),
                        patch: json!({"slideSec":v}),
                    },
                    window,
                    cx,
                ))
                .child(muted("Seconds. Videos play to their end.", cx));
        }
        body.into_any_element()
    }
    fn looks(&self, o: &OverlayOutput, cx: &mut Context<Self>) -> AnyElement {
        let mut body = v_flex().gap_3();
        let show = serde_json::to_value(&o.show).unwrap();
        for (title, keys) in LOOK_GROUPS {
            let mut group = v_flex().gap_1();
            let mut any = false;
            for key in *keys {
                if !model::look_applies(o, key.as_str()) {
                    continue;
                }
                any = true;
                let e = cx.entity();
                let id = o.id.clone();
                let key = *key;
                let label = output_show_label(key);
                let on = show[key.as_str()].as_bool().unwrap_or(false);
                group = group.child(row(
                    label,
                    toggle(
                        SharedString::from(format!("screens-look-{}", key.as_str())),
                        label,
                        on,
                        false,
                        move |on, w, cx| {
                            e.update(cx, |t, cx| {
                                t.output_patch(id.clone(), json!({"show":{key.as_str():on}}), w, cx)
                            })
                        },
                    ),
                ));
            }
            if any {
                body = body
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(*title),
                    )
                    .child(group);
            }
        }
        if o.show.countdown || o.show.clock || o.show.stage_message {
            for (key, label, value, options) in [
                (
                    "position",
                    "Position",
                    o.confidence.position.as_str(),
                    vec![Choice::new("top", "Top"), Choice::new("bottom", "Bottom")],
                ),
                (
                    "size",
                    "Size",
                    o.confidence.size.as_str(),
                    vec![
                        Choice::new("small", "Small"),
                        Choice::new("medium", "Medium"),
                        Choice::new("large", "Large"),
                    ],
                ),
            ] {
                let id = o.id.clone();
                body = body.child(row(
                    label,
                    self.choice(
                        &format!("screens-confidence-{key}"),
                        label,
                        value,
                        options,
                        move |v| Edit::Output {
                            id: id.clone(),
                            patch: json!({"confidence":{key:v}}),
                        },
                        cx,
                    ),
                ));
            }
        }
        body.child(muted(
            "Put the countdown, clock and stage message up from Operator → Timers.",
            cx,
        ))
        .into_any_element()
    }
    pub(crate) fn stage_fields(
        &self,
        s: &StageDisplayConfig,
        tab: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = v_flex().gap_4();
        if tab == "display" {
            body = body.child(field(
                "Output",
                self.display_picker(s.display_id, &s.display_label, &s.display_size, cx),
            ));
        } else {
            for (key, label, on) in [
                ("showNext", "Next slide", s.show_next),
                ("showClock", "Clock", s.show_clock),
                ("showTimer", "Countdown", s.show_timer),
            ] {
                let e = cx.entity();
                let id = s.id.clone();
                body = body.child(row(
                    label,
                    toggle(
                        SharedString::from(format!("screens-stage-{key}")),
                        label,
                        on,
                        false,
                        move |v, w, cx| {
                            e.update(cx, |t, cx| {
                                t.edit(
                                    Edit::Stage {
                                        id: id.clone(),
                                        patch: json!({key:v}),
                                    },
                                    w,
                                    cx,
                                )
                            })
                        },
                    ),
                ));
            }
            body = body.child(muted(
                "The current slide and stage messages always show.",
                cx,
            ));
        }
        body.into_any_element()
    }
    fn theme_preview(&mut self, o: &OverlayOutput, cx: &mut Context<Self>) -> AnyElement {
        if let Some(sample) = &self.sample {
            return div()
                .w_full()
                .h(px(250.))
                .overflow_hidden()
                .rounded(radius::md())
                .child(sample.clone())
                .into_any_element();
        }
        use kairo_core_gpui::thumbnails::{ThumbnailRequest, thumbnail_element};
        let theme =
            kairo_domain::overlay_outputs::output_theme_for(o, OverlayContentKind::Scripture);
        let request = ThumbnailRequest::new(
            std::sync::Arc::new(theme.clone()),
            "John 3:16 (KJV)",
            "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
            vec![],
        );
        let thumbnail = self.thumbnails.update(cx, |t, cx| t.get(&request, cx));
        let k = *cx.kairo();
        // These colours are slide data, not interface palette values.
        let bg = &theme.background;
        let color = Rgba::try_from(bg.color.as_str())
            .map(Hsla::from)
            .unwrap_or(k.surface_display)
            .opacity(bg.opacity as f32);
        let mut preview = div()
            .relative()
            .w_full()
            .h(px(250.))
            .overflow_hidden()
            .rounded(radius::md())
            .bg(k.surface_display);
        if bg.kind == OverlayBackgroundType::Gradient {
            let to = bg
                .color2
                .as_deref()
                .and_then(|v| Rgba::try_from(v).ok())
                .map(Hsla::from)
                .unwrap_or(color)
                .opacity(bg.opacity as f32);
            preview = preview.bg(linear_gradient(
                bg.angle_deg.unwrap_or(0.) as f32,
                linear_color_stop(color, 0.),
                linear_color_stop(to, 1.),
            ));
        } else if bg.kind != OverlayBackgroundType::Transparent {
            preview = preview.bg(color);
        }
        if bg.kind == OverlayBackgroundType::Image
            && let Some(path) = &bg.media_path
        {
            preview = preview.child(
                img(std::path::PathBuf::from(path))
                    .absolute()
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            );
        }
        preview
            .child(
                div()
                    .absolute()
                    .size_full()
                    .child(thumbnail_element(&thumbnail)),
            )
            .into_any_element()
    }
    fn hardware(
        &mut self,
        o: &OverlayOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot = self.snapshot.as_ref().unwrap().devices.clone();
        let port = snapshot.ports.iter().find(|p| p.id == o.device_port_id);
        let k = *cx.kairo();
        let mut body = v_flex().gap_5();
        // `DriverNotice`: the driver's own message, and the download page
        // when it is missing or too old.
        let driver = snapshot
            .providers
            .iter()
            .find(|p| p.name == "decklink")
            .and_then(|p| p.status.driver.clone());
        let driver_explains = driver.as_ref().is_some_and(|d| {
            matches!(
                d.state,
                kairo_domain::video_devices::DriverState::Missing
                    | kairo_domain::video_devices::DriverState::Unsupported
            )
        });
        if let Some(driver) = &driver {
            body = body.child(
                div()
                    .text_size(px(12.))
                    .text_color(if driver_explains {
                        k.warning_text
                    } else {
                        k.text(TextStep::T400)
                    })
                    .child(driver.message.clone()),
            );
            if driver_explains {
                body = body.child(
                    Button::new("screens-driver-download")
                        .small()
                        .label("Open download page")
                        .on_click(|_, _, cx| {
                            cx.open_url("https://www.blackmagicdesign.com/support/")
                        }),
                );
            }
        }
        let available = snapshot.providers.iter().any(|p| p.status.available);
        let provider_reason = if snapshot.providers.is_empty() {
            "This version of Kairo cannot drive video cards yet".to_owned()
        } else {
            snapshot
                .providers
                .iter()
                .find_map(|p| p.status.reason.clone().filter(|r| !r.is_empty()))
                .unwrap_or_else(|| "Install Blackmagic Desktop Video to use DeckLink cards".into())
        };
        let mut options = vec![Choice::new("", "Choose an output")];
        for p in &snapshot.ports {
            options.push(
                Choice::new(
                    p.id.clone(),
                    format!(
                        "{}{}",
                        p.port_name,
                        p.unavailable_reason
                            .as_ref()
                            .map(|r| format!(" — {r}"))
                            .unwrap_or_default()
                    ),
                )
                .disabled(p.unavailable_reason.is_some()),
            );
        }
        if !o.device_port_id.is_empty() && port.is_none() {
            options.push(
                Choice::new(
                    o.device_port_id.clone(),
                    format!(
                        "{} — disconnected",
                        if o.device_port_label.is_empty() {
                            "Saved output"
                        } else {
                            &o.device_port_label
                        }
                    ),
                )
                .disabled(true),
            );
        }
        let e = cx.entity();
        let id = o.id.clone();
        let ports = snapshot.ports.clone();
        let output_field = field(
            "Output",
            h_flex()
                .gap_2()
                .child(div().flex_1().min_w_0().child(choices(
                    "screens-port",
                    "Output",
                    &o.device_port_id,
                    options,
                    move |value, w, cx| {
                        let port = ports.iter().find(|p| p.id == value);
                        let patch = json!({
                            "devicePortId": value,
                            "devicePortLabel": port.map(|p| format!("{} · {}", p.device_name, p.port_name)).unwrap_or_default(),
                            "deviceModeId": port.and_then(|p| p.modes.first()).map(|m| m.id.clone()).unwrap_or_default(),
                            "deviceKeyFill": false,
                        });
                        e.update(cx, |t, cx| t.output_patch(id.clone(), patch, w, cx));
                    },
                )))
                .child(
                    Button::new("screens-refresh")
                        .small()
                        .icon(KairoIcon::RefreshCw)
                        .accessibility_label("Look for video cards again")
                        .tooltip("Look for video cards again")
                        .loading(self.refreshing && !cx.reduced_motion())
                        .loading_icon(KairoIcon::RefreshCw)
                        .disabled(self.refreshing)
                        .on_click(cx.listener(|t, _, _, cx| t.run(Command::RefreshCards, cx))),
                ),
        );
        let mode_field = port.map(|port| {
            let mut options = vec![Choice::new("", "Choose a mode")];
            for mode in &port.modes {
                options.push(Choice::new(mode.id.clone(), mode.label.clone()));
            }
            if !o.device_mode_id.is_empty() && !port.modes.iter().any(|m| m.id == o.device_mode_id)
            {
                options.push(
                    Choice::new(o.device_mode_id.clone(), "Saved mode — unavailable")
                        .disabled(true),
                );
            }
            field(
                "Mode",
                self.output_choice(o, "deviceModeId", "Mode", &o.device_mode_id, options, cx),
            )
        });
        let state = snapshot.outputs.get(&o.id);
        let ready = state.is_some_and(|s| s.ready);
        let current = state
            .and_then(|s| s.test_pattern.flatten())
            .map(|p| p.as_str())
            .unwrap_or("off");
        let e = cx.entity();
        let id = o.id.clone();
        let busy = self.busy;
        let pattern_field = field(
            "Test pattern",
            div().when(!ready || busy, |d| d.opacity(0.4)).child(
                kairo_ui_kit::components::SegmentedControl::new(
                    "screens-card-pattern",
                    "Test pattern",
                )
                .fit()
                .selected(current.to_owned())
                .options(
                    [
                        ("off", "Off"),
                        ("bars", "Color bars"),
                        ("text", "Fine text"),
                        ("motion", "Motion"),
                    ]
                    .into_iter()
                    .map(|(v, l)| kairo_ui_kit::components::SegmentOption::new(v, l)),
                )
                .on_change(move |value, _, cx| {
                    if !ready || busy {
                        return;
                    }
                    let pattern = kairo_domain::video_devices::VideoTestPattern::ALL
                        .iter()
                        .find(|p| p.as_str() == value.as_ref())
                        .copied();
                    e.update(cx, |t, cx| {
                        t.run(
                            Command::CardPattern {
                                id: id.clone(),
                                pattern,
                            },
                            cx,
                        )
                    });
                }),
            ),
        );
        let half = |el: AnyElement| div().flex_1().min_w_0().child(el);
        body = body
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(half(output_field))
                    .child(half(mode_field.unwrap_or_else(|| div().into_any_element()))),
            )
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(half(self.source(o, window, cx)))
                    .child(half(pattern_field)),
            );
        if let Some(port) = port
            && port.supports_key_fill
        {
            body = body.child(row(
                "Key/fill",
                self.output_toggle(o, "deviceKeyFill", "Key/fill", o.device_key_fill, cx),
            ));
        }
        // Only a problem earns a line; a healthy output says nothing.
        if !ready && !driver_explains {
            body = body.child(
                div()
                    .id("screens-card-problem")
                    .test_support()
                    .role(Role::Status)
                    .text_size(px(13.))
                    .text_color(k.warning_text)
                    .child(if !available {
                        provider_reason
                    } else {
                        state
                            .map(|s| s.reason.clone())
                            .filter(|r| !r.is_empty())
                            .unwrap_or_else(|| {
                                if snapshot.ports.is_empty() {
                                    "No video cards detected"
                                } else {
                                    "Choose an output"
                                }
                                .into()
                            })
                    }),
            );
        }
        // `SignalDetails`: one line folded, the counters unfolded.
        let diagnostics = state.and_then(|s| s.diagnostics.as_ref());
        let hardware = diagnostics.and_then(|s| s.hardware.as_ref());
        let missed = hardware.map_or(0., |h| h.late + h.dropped);
        let summary = match hardware {
            None => "Waiting for the card".to_owned(),
            Some(h) if missed > 0. => format!("{} late · {} dropped", h.late, h.dropped),
            Some(h) => format!("Smooth · {:.1} fps", h.output_fps),
        };
        let mode = port.and_then(|p| p.modes.iter().find(|m| m.id == o.device_mode_id));
        let fmt = |fps: f64| {
            let text = format!("{fps:.2}");
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        let open = self.signal_open;
        body = body.child(
            h_flex()
                .id("screens-signal")
                .test_support()
                .role(Role::Button)
                .aria_label(SharedString::from(format!("Signal: {summary}")))
                .gap_3()
                .items_center()
                .cursor_pointer()
                .on_click(cx.listener(|t, _, _, cx| {
                    t.signal_open = !t.signal_open;
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .text_size(px(13.))
                        .text_color(k.text(TextStep::T300))
                        .child(
                            gpui_kit::component::Icon::new(if open {
                                KairoIcon::ChevronDown
                            } else {
                                KairoIcon::ChevronRight
                            })
                            .size(px(12.)),
                        )
                        .child("Signal"),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if missed > 0. {
                            k.warning_text
                        } else {
                            k.text(TextStep::T300)
                        })
                        .child(summary),
                ),
        );
        if open {
            let dash = || "—".to_owned();
            let cells: [(&str, String); 6] = [
                (
                    "Rendered",
                    diagnostics.map_or_else(dash, |d| format!("{:.1} fps", d.capture_fps)),
                ),
                (
                    "Sent to card",
                    hardware.map_or_else(dash, |h| format!("{:.1} fps", h.output_fps)),
                ),
                (
                    "Target",
                    mode.map_or_else(dash, |m| {
                        let fps = m.frame_rate_n / m.frame_rate_d;
                        format!(
                            "{} fps{}",
                            fmt(fps),
                            if m.interlaced {
                                format!(" · {} fields/s", fmt(fps * 2.))
                            } else {
                                String::new()
                            }
                        )
                    }),
                ),
                ("Late", hardware.map_or_else(dash, |h| h.late.to_string())),
                (
                    "Dropped",
                    hardware.map_or_else(dash, |h| h.dropped.to_string()),
                ),
                (
                    "Repeated",
                    hardware.map_or_else(dash, |h| h.repeated.to_string()),
                ),
            ];
            let mut grid = v_flex()
                .gap_3()
                .rounded(radius::md())
                .bg(k.surface)
                .p_3()
                .text_size(px(12.));
            for pair in cells.chunks(3) {
                let mut line = h_flex().gap_4();
                for (label, value) in pair {
                    line = line.child(
                        v_flex()
                            .flex_1()
                            .child(
                                div()
                                    .text_color(k.text(TextStep::T500))
                                    .child(label.to_string()),
                            )
                            .child(
                                div()
                                    .text_color(k.text(TextStep::T200))
                                    .child(value.clone()),
                            ),
                    );
                }
                grid = grid.child(line);
            }
            body = body.child(grid);
        }
        body.into_any_element()
    }
    pub(crate) fn show_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = self.snapshot.clone().unwrap();
        let mut grid = v_flex().gap_1();
        let k = *cx.kairo();
        if snapshot.settings.overlay.outputs.is_empty() {
            grid = grid.child(muted("Add a screen or NDI feed first.", cx));
        } else {
            let mut head = h_flex().gap_2().child(div().w(px(136.)));
            for o in &snapshot.settings.overlay.outputs {
                let id = o.id.clone();
                head = head.child(
                    Button::new(SharedString::from(format!("looks-column-{}", o.id)))
                        .small()
                        .ghost()
                        .w(px(108.))
                        .label(o.name.clone())
                        .on_click(cx.listener(move |t, _, w, cx| t.select(id.clone(), w, cx))),
                );
            }
            grid = grid.child(head);
            for (group, keys) in LOOK_GROUPS {
                grid = grid.child(muted(*group, cx));
                for key in *keys {
                    let mut line = h_flex().gap_2().items_center().child(
                        div()
                            .w(px(136.))
                            .text_size(text::XS.size)
                            .child(output_show_label(*key)),
                    );
                    for o in &snapshot.settings.overlay.outputs {
                        let show = serde_json::to_value(&o.show).unwrap();
                        let on = show[key.as_str()].as_bool().unwrap_or(false);
                        let id = o.id.clone();
                        let key = *key;
                        let e = cx.entity();
                        let cell = if model::look_applies(o, key.as_str()) {
                            toggle(
                                SharedString::from(format!("looks-{}-{}", o.id, key.as_str())),
                                format!("{} on {}", output_show_label(key), o.name),
                                on,
                                false,
                                move |on, w, cx| {
                                    e.update(cx, |t, cx| {
                                        t.output_patch(
                                            id.clone(),
                                            json!({"show":{key.as_str():on}}),
                                            w,
                                            cx,
                                        )
                                    })
                                },
                            )
                        } else {
                            muted("—", cx)
                        };
                        line = line.child(div().w(px(108.)).flex().justify_center().child(cell));
                    }
                    grid = grid.child(line.py_1());
                }
            }
        }
        let transition = &snapshot.settings.presentation.transition;
        let e = cx.entity();
        let transition_choice = choices(
            "screens-transition",
            "Transition",
            transition.kind.as_str(),
            vec![Choice::new("cut", "Cut"), Choice::new("fade", "Fade")],
            move |value, w, cx| {
                e.update(cx, |t, cx| {
                    t.edit(
                        Edit::Presentation(json!({"transition":{"kind":value}})),
                        w,
                        cx,
                    )
                })
            },
        );
        let mut body=v_flex().gap_4().child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child("Looks & transition"))
            .child(muted("Which layers each screen shows, how slides change, and a check of everything a push depends on.",cx))
            .child(div().text_size(text::SM.size).font_weight(FontWeight::SEMIBOLD).child("Looks"))
            .child(div().id("screens-looks-scroll").test_support().overflow_x_scroll().bg(k.surface_secondary).rounded(radius::md()).p_3().child(grid))
            .child(div().text_size(text::SM.size).font_weight(FontWeight::SEMIBOLD).child("Transition")).child(transition_choice);
        if transition.kind == kairo_domain::program::TransitionKind::Fade {
            body = body.child(
                self.number(
                    "screens-fade-length",
                    "Fade length (ms)",
                    NumberModel::new(transition.duration_ms, 50.)
                        .min(100.)
                        .max(2000.),
                    |v| Edit::Presentation(json!({"transition":{"durationMs":v}})),
                    window,
                    cx,
                ),
            );
        }
        body=body.child(muted("Applies to slides, backgrounds, messages, props and the logo. Document page turns are always instant.",cx)).child(div().text_size(text::SM.size).font_weight(FontWeight::SEMIBOLD).child("Setup check"));
        let enabled: Vec<_> = snapshot
            .settings
            .overlay
            .outputs
            .iter()
            .filter(|o| o.enabled)
            .collect();
        let mut steps = vec![(
            "A screen is on".to_string(),
            enabled.iter().any(|o| {
                o.kind != OverlayOutputKind::Device
                    || snapshot.outputs.get(&o.id).is_some_and(|(ready, _)| *ready)
            }),
            "Add an external display, video card or NDI feed in the Screens list and turn it on."
                .to_string(),
        )];
        for o in &enabled {
            if o.kind == OverlayOutputKind::Screen {
                let status = snapshot.outputs.get(&o.id);
                steps.push((
                    format!("{} is on its display", o.name),
                    status.is_some_and(|s| s.0),
                    format!(
                        "{}. Pick a connected display for it.",
                        status.map(|s| s.1.as_str()).unwrap_or("No display chosen")
                    ),
                ));
            }
        }
        if enabled.iter().any(|o| o.kind == OverlayOutputKind::Ndi) {
            let ready = enabled
                .iter()
                .filter(|o| o.kind == OverlayOutputKind::Ndi)
                .all(|o| snapshot.outputs.get(&o.id).is_some_and(|s| s.0));
            steps.push(("NDI sender running".into(),ready,"The NDI runtime failed to load or the sender could not start — check the logs. Other screens still work.".into()));
        }
        let count = steps.iter().filter(|s| !s.1).count();
        body = body.child(muted(
            if count == 0 {
                "Everything is ready.".into()
            } else {
                format!("{count} things to fix.")
            },
            cx,
        ));
        for (label, ready, hint) in steps {
            body = body.child(
                v_flex()
                    .gap_1()
                    .child(format!("{} {label}", if ready { "✓" } else { "○" }))
                    .when(!ready, |d| d.child(muted(hint, cx))),
            );
        }
        body = body.child(
            Button::new("screens-setup-test")
                .small()
                .label("Send test verse")
                .disabled(self.busy)
                .on_click(cx.listener(|t, _, _, cx| t.run(Command::TestVerse, cx))),
        );
        div()
            .id("screens-show-scroll")
            .test_support()
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .child(body)
            .into_any_element()
    }
}
const LOOK_GROUPS: &[(&str, &[OutputShowKey])] = &[
    (
        "Slides",
        &[
            OutputShowKey::Scripture,
            OutputShowKey::Lyrics,
            OutputShowKey::Documents,
        ],
    ),
    ("Media", &[OutputShowKey::Backgrounds]),
    (
        "Layers",
        &[
            OutputShowKey::Poll,
            OutputShowKey::Question,
            OutputShowKey::Messages,
            OutputShowKey::Overlays,
        ],
    ),
    (
        "Confidence",
        &[
            OutputShowKey::Countdown,
            OutputShowKey::Clock,
            OutputShowKey::StageMessage,
        ],
    ),
];
fn fit_hint(o: &OverlayOutput) -> Option<String> {
    use kairo_domain::displays::{Dimensions, aspect_ratio_of};
    let size = o
        .display_size
        .as_ref()
        .filter(|s| s.width > 0. && s.height > 0.)?;
    let screen = size.width / size.height;
    let frame = aspect_ratio_of(
        o.aspect,
        Some(Dimensions {
            width: o.custom_aspect.width,
            height: o.custom_aspect.height,
        }),
        Some(Dimensions {
            width: size.width,
            height: size.height,
        }),
    )?;
    if (frame - screen).abs() / screen < 0.02 {
        return None;
    }
    Some(format!(
        "{screen:.2}:1 display — {}",
        if frame < screen {
            "bars at the sides"
        } else {
            "bars top and bottom"
        }
    ))
}
