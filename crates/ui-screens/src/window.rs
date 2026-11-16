use crate::{
    Command, ScreensBackend, Snapshot,
    model::{self, Edit},
};
use futures_util::StreamExt;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::Selectable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Disableable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kairo_domain::{overlay::*, settings::AppSettings};
use kairo_ui_kit::components::PopoverTriggerExt;
use kairo_ui_kit::components::{
    MovableDialog, MovableDialogEvent, TrafficLights, confirm, messages,
};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use serde_json::json;
use std::{collections::VecDeque, sync::Arc, time::Instant};

pub const FRAME: (f32, f32) = (800., 620.);
pub const FRAME_MIN: (f32, f32) = (760., 480.);
#[derive(Clone, Copy, Debug)]
pub enum ScreensEvent {
    Close,
}

pub struct ScreensWindow {
    pub(crate) backend: Arc<dyn ScreensBackend>,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) selected: Option<String>,
    pub(crate) tab: String,
    pub(crate) error: Option<String>,
    pub(crate) message: String,
    pub(crate) busy: bool,
    pub(crate) refreshing: bool,
    pub(crate) fields:
        std::collections::HashMap<String, Entity<kairo_ui_kit::components::NumberField>>,
    _subscriptions: Vec<Subscription>,
    pub(crate) field_subscriptions: Vec<Subscription>,
    dialog: Entity<MovableDialog>,
    name: Entity<InputState>,
    queue: VecDeque<Edit>,
    saving: bool,
    initial: Option<String>,
    opened: Option<Instant>,
    events: Task<()>,
    pushed: u64,
    frame: (f32, f32),
    resize_press: Option<(Point<Pixels>, (f32, f32))>,
    resize_focus: FocusHandle,
    fit_preview: Option<Task<()>>,
    pub(crate) thumbnails: Entity<kairo_core_gpui::thumbnails::Thumbnails>,
    pub(crate) sample: Option<Entity<kairo_core_gpui::preview::LivePreview>>,
    sample_session: Option<crate::backend::ThemePreview>,
    sample_theme: Option<OverlayTheme>,
}
impl EventEmitter<ScreensEvent> for ScreensWindow {}
impl ScreensWindow {
    pub fn new(
        backend: Arc<dyn ScreensBackend>,
        initial: Option<&str>,
        frame: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let dialog = cx.new(|cx| {
            MovableDialog::new(
                "screens-window",
                "Screens",
                size(px(frame.0), px(frame.1)),
                window,
                cx,
            )
        });
        let name = cx.new(|cx| InputState::new(window, cx));
        let thumbnails =
            cx.new(|cx| kairo_core_gpui::thumbnails::Thumbnails::new((960, 540), 4, 1, cx));
        let subscriptions = vec![
            cx.observe(&thumbnails, |_, _, cx| cx.notify()),
            cx.subscribe(&dialog, |_, _, _: &MovableDialogEvent, cx| {
                cx.emit(ScreensEvent::Close)
            }),
            cx.subscribe_in(
                &name,
                window,
                |this, field, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        let name = field.read(cx).value().trim().to_string();
                        if !name.is_empty() {
                            this.patch_selected(json!({"name":name}), window, cx);
                        }
                    }
                },
            ),
        ];
        let mut feed = backend.events();
        let events = cx.spawn_in(window, async move |this, cx| {
            let _lifetime = feed.lifetime;
            while let Some(next) = feed.receiver.next().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.pushed += 1;
                        this.receive(next, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut this = Self {
            backend,
            snapshot: None,
            selected: None,
            tab: "display".into(),
            error: None,
            message: String::new(),
            busy: false,
            refreshing: false,
            fields: Default::default(),
            _subscriptions: subscriptions,
            field_subscriptions: Vec::new(),
            dialog,
            name,
            queue: VecDeque::new(),
            saving: false,
            initial: initial.map(str::to_owned),
            opened: Some(Instant::now()),
            events,
            pushed: 0,
            frame,
            resize_press: None,
            resize_focus: cx.focus_handle(),
            fit_preview: None,
            thumbnails,
            sample: None,
            sample_session: None,
            sample_theme: None,
        };
        this.load(window, cx);
        this
    }
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_edits();
        self.sample = None;
        self.sample_session = None;
        self.dialog
            .update(cx, |dialog, cx| dialog.restore(window, cx));
    }
    fn flush_edits(&mut self) {
        for edit in self.queue.drain(..) {
            drop(self.backend.edit(edit));
        }
    }
    pub fn settings(&self) -> Option<&AppSettings> {
        self.snapshot.as_ref().map(|s| &s.settings)
    }
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        let request = self.backend.load();
        let pushed = self.pushed;
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |this, w, cx| {
                if this.pushed == pushed {
                    this.receive(result, w, cx)
                }
            });
        })
        .detach();
    }
    fn receive(
        &mut self,
        result: Result<Snapshot, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(next) => {
                let first = self.snapshot.is_none();
                if first {
                    self.selected = model::initial_selection(
                        &next.settings,
                        self.initial.as_deref(),
                        self.backend.read_ui(model::SELECTION_KEY).as_deref(),
                    );
                }
                self.snapshot = Some(next);
                if first {
                    let add = self
                        .initial
                        .as_deref()
                        .and_then(|s| s.strip_prefix("add:"))
                        .and_then(|k| {
                            OverlayOutputKind::ALL
                                .iter()
                                .find(|v| v.as_str() == k)
                                .copied()
                        });
                    if let Some(kind) = add {
                        self.add_output(kind, window, cx);
                    } else {
                        self.sync_name(false, window, cx);
                    }
                } else if !self.saving {
                    self.sync_name(false, window, cx);
                }
            }
            Err(error) => self.error = Some(error),
        }
        if !self.saving {
            self.sync_numbers(false, window, cx);
            self.sync_preview(window, cx);
        }
        cx.notify();
    }
    fn sync_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let theme = (self.tab == "theme")
            .then(|| {
                self.settings()
                    .and_then(|settings| {
                        settings
                            .overlay
                            .outputs
                            .iter()
                            .find(|output| Some(output.id.as_str()) == self.selected.as_deref())
                    })
                    .map(|output| {
                        kairo_domain::overlay_outputs::output_theme_for(
                            output,
                            OverlayContentKind::Scripture,
                        )
                    })
            })
            .flatten();
        let Some(theme) = theme else {
            // Dropping the view releases its frame map before the session
            // destroys the private surface and its media players.
            self.sample = None;
            self.sample_session = None;
            self.sample_theme = None;
            return;
        };
        if self.sample_session.is_some() && self.sample_theme.as_ref() == Some(&theme) {
            return;
        }
        if self.sample_session.is_none() {
            self.sample_session = self.backend.open_preview();
            if let Some(session) = &self.sample_session {
                let hub = session.hub();
                let output = session.output().to_owned();
                let view = cx.new(|cx| {
                    let mut preview = kairo_core_gpui::preview::LivePreview::new(hub, cx);
                    preview.set_output(Some(output), window, cx);
                    preview
                });
                self.sample = Some(view);
            }
        }
        if let Some(session) = &self.sample_session {
            session.set_theme(theme.clone());
        }
        self.sample_theme = Some(theme);
    }
    fn sync_name(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !force && self.name.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let name = self
            .settings()
            .and_then(|s| {
                self.selected.as_deref().and_then(|key| {
                    s.overlay
                        .outputs
                        .iter()
                        .find(|o| o.id == key)
                        .map(|o| o.name.clone())
                        .or_else(|| {
                            s.presentation
                                .stage_displays
                                .iter()
                                .find(|o| format!("stage:{}", o.id) == key)
                                .map(|o| o.name.clone())
                        })
                })
            })
            .unwrap_or_default();
        if self.name.read(cx).value().as_ref() != name {
            self.name
                .update(cx, |input, cx| input.set_value(name, window, cx));
        }
    }
    pub(crate) fn clear_fields(&mut self) {
        self.fields.clear();
        self.field_subscriptions.clear();
    }
    fn sync_numbers(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings() else {
            return;
        };
        let output = self
            .selected
            .as_deref()
            .and_then(|id| settings.overlay.outputs.iter().find(|o| o.id == id));
        let values = [
            ("screens-ratio-width", output.map(|o| o.custom_aspect.width)),
            (
                "screens-ratio-height",
                output.map(|o| o.custom_aspect.height),
            ),
            ("screens-slide-sec", output.map(|o| o.slide_sec)),
            (
                "screens-fade-length",
                Some(settings.presentation.transition.duration_ms),
            ),
        ];
        for (id, value) in values {
            if let (Some(field), Some(value)) = (self.fields.get(id), value)
                && (force || !field.read(cx).focus_handle(cx).is_focused(window))
                && field.read(cx).value() != value
            {
                field.update(cx, |f, cx| f.set_value(value, window, cx));
            }
        }
    }
    pub fn select(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        if model::SHOW_KEYS.contains(&key.as_str()) {
            self.backend.write_ui(model::SELECTION_KEY, key.clone());
            self.backend.write_ui(&key, "true".into());
        }
        self.selected = Some(key);
        self.clear_fields();
        self.sync_name(true, window, cx);
        self.sync_preview(window, cx);
        cx.notify();
    }
    pub fn edit(&mut self, edit: Edit, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.queue.push_back(edit);
        self.save_next(window, cx);
    }
    fn save_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(edit) = self.queue.pop_front() else {
            return;
        };
        let fit_id = match &edit {
            Edit::Output { id, patch }
                if patch.get("customAspect").is_some() || patch.get("aspect").is_some() =>
            {
                Some((id.clone(), patch.get("customAspect").is_some()))
            }
            _ => None,
        };
        self.saving = true;
        let request = self.backend.edit(edit);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                match result {
                    Ok(settings) => {
                        if let Some(s) = &mut this.snapshot {
                            s.settings = settings;
                        }
                    }
                    Err(e) => {
                        this.error = Some(format!(
                            "Screen changes could not be saved. Please try again. {e}"
                        ))
                    }
                }
                if this.error.is_none()
                    && let Some((id, custom)) = fit_id
                {
                    this.preview_fit(id, custom, cx);
                }
                this.sync_name(false, window, cx);
                if this.queue.is_empty() {
                    this.sync_numbers(this.error.is_some(), window, cx);
                }
                this.sync_preview(window, cx);
                this.save_next(window, cx);
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn patch_selected(
        &mut self,
        patch: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Rename commits on blur or Enter through the same serialized writer.
        if let Some(key) = &self.selected {
            self.queue
                .push_back(if let Some(id) = key.strip_prefix("stage:") {
                    Edit::Stage {
                        id: id.into(),
                        patch,
                    }
                } else {
                    Edit::Output {
                        id: key.clone(),
                        patch,
                    }
                });
            self.save_next(window, cx);
            cx.notify();
        }
    }
    pub fn add_output(
        &mut self,
        kind: OverlayOutputKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = new_id(kind.as_str());
        self.selected = Some(id.clone());
        self.tab = if kind == OverlayOutputKind::Device {
            "hardware"
        } else {
            "display"
        }
        .into();
        self.clear_fields();
        self.edit(Edit::AddOutput { id, kind }, window, cx);
    }
    pub fn add_stage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = new_id("stage");
        self.selected = Some(format!("stage:{id}"));
        self.tab = "display".into();
        self.clear_fields();
        self.edit(Edit::AddStage(id), window, cx);
    }
    pub fn ask_remove(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings() else {
            return;
        };
        let name = settings
            .overlay
            .outputs
            .iter()
            .find(|o| o.id == key)
            .map(|o| o.name.clone())
            .or_else(|| {
                settings
                    .presentation
                    .stage_displays
                    .iter()
                    .find(|s| format!("stage:{}", s.id) == key)
                    .map(|s| s.name.clone())
            });
        let Some(name) = name else {
            return;
        };
        if !key.starts_with("stage:") && settings.overlay.outputs.len() <= 1 {
            self.error = Some(
                "Audience needs at least one screen. Add another before deleting this one.".into(),
            );
            cx.notify();
            return;
        }
        let entity = cx.entity();
        confirm(
            messages::screens_delete(&name),
            window,
            cx,
            move |answer, w, cx| {
                if answer {
                    entity.update(cx, |this, cx| {
                        if this.selected.as_deref() == Some(&key) {
                            this.selected = None;
                        }
                        this.edit(Edit::Remove(key), w, cx);
                    });
                }
            },
        );
    }
    fn preview_fit(&mut self, id: String, debounce: bool, cx: &mut Context<Self>) {
        self.fit_preview = Some(cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
            }
            let _ = this.update(cx, |this, cx| {
                let output = this
                    .settings()
                    .and_then(|s| s.overlay.outputs.iter().find(|o| o.id == id))
                    .cloned();
                if let Some(o) = output
                    && let Some(display_id) = o.display_id
                {
                    this.run(
                        Command::Frame(TestPatternInput {
                            display_id,
                            name: o.name,
                            aspect: o.aspect,
                            custom_aspect: o.custom_aspect,
                        }),
                        cx,
                    );
                }
            });
        }));
    }
    fn resize(&mut self, size: (f32, f32), window: &Window, cx: &mut Context<Self>) {
        let viewport = window.viewport_size();
        self.frame = (
            size.0.max(FRAME_MIN.0).min(f32::from(viewport.width) - 32.),
            size.1
                .max(FRAME_MIN.1)
                .min(f32::from(viewport.height) - 32.),
        );
        self.backend.write_ui(
            "kairo.frame.screens.v4",
            json!({"w":self.frame.0,"h":self.frame.1}).to_string(),
        );
        let size = self.frame;
        self.dialog.update(cx, |d, cx| {
            d.set_size(gpui_kit::size(px(size.0), px(size.1)), cx)
        });
        cx.notify();
    }
    pub fn run(&mut self, command: Command, cx: &mut Context<Self>) {
        let refresh = matches!(command, Command::RefreshCards);
        if (refresh && self.refreshing) || (!refresh && self.busy) {
            return;
        }
        if refresh {
            self.refreshing = true;
        } else {
            self.busy = true;
        }
        self.error = None;
        self.message = if refresh {
            "Looking for video outputs…"
        } else {
            "Working…"
        }
        .into();
        let request = self.backend.run(command);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                if refresh {
                    this.refreshing = false;
                } else {
                    this.busy = false;
                }
                match result {
                    Ok(message) => this.message = message,
                    Err(error) => {
                        this.message.clear();
                        this.error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn bind(
        &mut self,
        display: Option<DisplayInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.selected.clone() else {
            return;
        };
        if let (Some(display), Some(snapshot)) = (&display, &self.snapshot)
            && let Some(owner) =
                model::display_owner(&snapshot.settings, &key, display, &snapshot.displays)
        {
            self.error = Some(messages::screens_display_taken(&owner));
            cx.notify();
            return;
        }
        self.edit(Edit::Bind { key, display }, window, cx);
    }
    fn sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let entity = cx.entity();
        let snapshot = self.snapshot.clone();
        let lights = self
            .dialog
            .update(cx, |d, cx| d.drag_handle("screens-sidebar-drag", cx))
            .h(px(44.))
            .px_4()
            .py_4()
            .child(
                TrafficLights::new("screens-lights")
                    .on_close(cx.listener(|_, _, _, cx| cx.emit(ScreensEvent::Close))),
            );
        let mut groups = v_flex()
            .id("screens-nav")
            .test_support()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_3();
        if let Some(snapshot) = snapshot {
            for stage in [false, true] {
                let settings = &snapshot.settings;
                let any = if stage {
                    settings
                        .presentation
                        .stage_displays
                        .iter()
                        .any(|s| s.enabled)
                } else {
                    settings.overlay.outputs.iter().any(|o| o.enabled)
                };
                let e = entity.clone();
                let toggle = crate::fields::toggle(
                    format!("screens-group-{stage}"),
                    if stage {
                        "Turn all Stage screens on or off"
                    } else {
                        "Turn all Audience screens on or off"
                    },
                    any,
                    false,
                    move |on, w, cx| {
                        e.update(cx, |this, cx| {
                            this.edit(Edit::Group { stage, enabled: on }, w, cx)
                        })
                    },
                );
                let e = entity.clone();
                let additions: Vec<_> = OverlayOutputKind::ALL
                    .iter()
                    .copied()
                    .filter(|kind| {
                        let count = settings
                            .overlay
                            .outputs
                            .iter()
                            .filter(|o| o.kind == *kind)
                            .count();
                        match kind {
                            OverlayOutputKind::Ndi => {
                                count < kairo_domain::overlay_outputs::MAX_NDI_OUTPUTS
                            }
                            OverlayOutputKind::Syphon => {
                                cfg!(target_os = "macos")
                                    && count < kairo_domain::overlay_outputs::MAX_SYPHON_OUTPUTS
                            }
                            OverlayOutputKind::Device => !snapshot.devices.providers.is_empty(),
                            _ => true,
                        }
                    })
                    .collect();
                let add = Button::new(SharedString::from(format!("screens-add-{stage}")))
                    .small()
                    .ghost()
                    .icon(KairoIcon::Plus)
                    .accessibility_label(if stage {
                        "Add stage display"
                    } else {
                        "Add to Audience"
                    })
                    .popover_trigger()
                    .dropdown_menu(move |mut menu, _, _| {
                        if stage {
                            let e = e.clone();
                            return menu.item(PopupMenuItem::new("Add stage display").on_click(
                                move |_, w, cx| e.update(cx, |t, cx| t.add_stage(w, cx)),
                            ));
                        }
                        for kind in &additions {
                            let e = e.clone();
                            let kind = *kind;
                            menu = menu.item(PopupMenuItem::new(model::kind_title(kind)).on_click(
                                move |_, w, cx| e.update(cx, |t, cx| t.add_output(kind, w, cx)),
                            ));
                        }
                        menu
                    });
                let mut group = v_flex().gap_1().child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .bg(k.surface_tertiary)
                        .child(toggle)
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if stage { "Stage" } else { "Audience" }),
                        )
                        .child(add),
                );
                let rows: Vec<(String, String, String, bool)> = if stage {
                    settings
                        .presentation
                        .stage_displays
                        .iter()
                        .map(|s| {
                            (
                                format!("stage:{}", s.id),
                                s.name.clone(),
                                self.stage_status(&s.id),
                                s.enabled,
                            )
                        })
                        .collect()
                } else {
                    settings
                        .overlay
                        .outputs
                        .iter()
                        .map(|o| {
                            (
                                o.id.clone(),
                                o.name.clone(),
                                self.output_status(o),
                                o.enabled,
                            )
                        })
                        .collect()
                };
                if rows.is_empty() {
                    group = group.child(
                        div()
                            .px_3()
                            .py_4()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(if stage {
                                "No stage displays yet"
                            } else {
                                "No audience screens yet"
                            }),
                    );
                }
                for (key, name, detail, on) in rows {
                    let e = entity.clone();
                    let target = key.clone();
                    let selected = self.selected.as_ref() == Some(&key);
                    let remove_entity = entity.clone();
                    let remove_key = key.clone();
                    let row = Button::new(SharedString::from(format!("screens-row-{key}")))
                        .ghost()
                        .w_full()
                        .h_auto()
                        .px_2()
                        .py_2()
                        .selected(selected)
                        .label(name)
                        .accessibility_label(format!("{key}: {detail}"))
                        .on_click(move |_, w, cx| {
                            e.update(cx, |t, cx| t.select(target.clone(), w, cx))
                        });
                    group = group.child(
                        v_flex()
                            .id(SharedString::from(format!("screens-item-{key}")))
                            .px_2()
                            .opacity(if on { 1. } else { 0.5 })
                            .child(row)
                            .child(
                                div()
                                    .px_2()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(detail),
                            )
                            .context_menu(move |menu, _, _| {
                                let e = remove_entity.clone();
                                let key = remove_key.clone();
                                menu.item(PopupMenuItem::new("Delete").on_click(move |_, w, cx| {
                                    e.update(cx, |t, cx| t.ask_remove(key.clone(), w, cx))
                                }))
                            }),
                    );
                }
                groups = groups.child(group);
            }
        }
        v_flex()
            .w(px(232.))
            .flex_shrink_0()
            .h_full()
            .bg(k.glass_panel())
            .border_r_1()
            .border_color(k.panel_border)
            .child(lights)
            .child(groups)
            .child(
                v_flex()
                    .border_t_1()
                    .border_color(k.panel_border)
                    .p_2()
                    .gap_1()
                    .child(
                        Button::new("screens-show")
                            .ghost()
                            .label("Looks & transition")
                            .on_click(
                                cx.listener(|t, _, w, cx| t.select("show:setup".into(), w, cx)),
                            ),
                    )
                    .child(
                        Button::new("screens-identify")
                            .ghost()
                            .label("Identify screens")
                            .disabled(self.busy)
                            .on_click(cx.listener(|t, _, _, cx| t.run(Command::Identify, cx))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("screens-test")
                                    .small()
                                    .label("Test verse")
                                    .disabled(self.busy)
                                    .on_click(
                                        cx.listener(|t, _, _, cx| t.run(Command::TestVerse, cx)),
                                    ),
                            )
                            .child(
                                Button::new("screens-clear")
                                    .small()
                                    .label("Clear")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|t, _, _, cx| t.run(Command::Clear, cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
    pub(crate) fn output_status(&self, o: &OverlayOutput) -> String {
        if !o.enabled {
            return "Off".into();
        }
        let Some((ready, reason)) = self.snapshot.as_ref().and_then(|s| s.outputs.get(&o.id))
        else {
            return "Checking…".into();
        };
        if !ready {
            return if reason.is_empty() {
                "Not ready".into()
            } else {
                reason.clone()
            };
        }
        if kairo_domain::overlay_outputs::is_local_kind(o.kind)
            && o.source == OutputSource::Playlist
        {
            return "Playing its own playlist".into();
        }
        if !reason.is_empty() && reason != "Live" {
            return reason.clone();
        }
        if o.kind == OverlayOutputKind::Screen {
            "Live"
        } else {
            "Ready"
        }
        .into()
    }
    fn stage_status(&self, id: &str) -> String {
        self.snapshot
            .as_ref()
            .and_then(|s| s.stages.iter().find(|s| s.id == id))
            .map(|s| s.reason.clone())
            .unwrap_or("Checking…".into())
    }
    fn content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(snapshot) = self.snapshot.clone() else {
            return v_flex()
                .p_6()
                .gap_3()
                .child("Loading screens…")
                .child(
                    Button::new("screens-retry")
                        .label("Retry")
                        .on_click(cx.listener(|t, _, w, cx| t.load(w, cx))),
                )
                .into_any_element();
        };
        let key = self.selected.clone().unwrap_or_default();
        if model::SHOW_KEYS.contains(&key.as_str()) {
            return self.show_page(window, cx);
        }
        let output = snapshot
            .settings
            .overlay
            .outputs
            .iter()
            .find(|o| o.id == key)
            .cloned();
        let stage = snapshot
            .settings
            .presentation
            .stage_displays
            .iter()
            .find(|s| format!("stage:{}", s.id) == key)
            .cloned();
        let tabs: &[&str] = if let Some(o) = &output {
            match o.kind {
                OverlayOutputKind::Screen => &["display", "look", "theme"],
                OverlayOutputKind::Device => &["hardware", "look", "theme"],
                OverlayOutputKind::Ndi => &["look", "theme", "sound"],
                OverlayOutputKind::Syphon => &["look", "theme"],
            }
        } else if stage.is_some() {
            &["display", "layout"]
        } else {
            return v_flex()
                .p_8()
                .gap_3()
                .child("Select a screen, or add one to get started.")
                .into_any_element();
        };
        let active = if tabs.contains(&self.tab.as_str()) {
            self.tab.clone()
        } else {
            tabs[0].into()
        };
        let status = output
            .as_ref()
            .map(|o| format!("{} · {}", model::kind_title(o.kind), self.output_status(o)))
            .unwrap_or_else(|| {
                stage
                    .as_ref()
                    .map(|s| {
                        if s.enabled {
                            self.stage_status(&s.id)
                        } else {
                            "Off".into()
                        }
                    })
                    .unwrap_or_default()
            });
        let k = *cx.kairo();
        let header = self
            .dialog
            .update(cx, |d, cx| d.drag_handle("screens-header", cx))
            .px_6()
            .py_4()
            .border_b_1()
            .border_color(k.panel_border)
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div().flex_1().child(
                            Input::new(&self.name)
                                .appearance(false)
                                .text_size(px(17.))
                                .aria_label("Name"),
                        ),
                    )
                    .child(
                        Button::new("screens-delete")
                            .ghost()
                            .small()
                            .icon(KairoIcon::Trash2)
                            .accessibility_label("Delete screen")
                            .on_click(cx.listener(|t, _, w, cx| {
                                if let Some(key) = t.selected.clone() {
                                    t.ask_remove(key, w, cx)
                                }
                            })),
                    ),
            )
            .child(crate::fields::muted(status, cx));
        let entity = cx.entity();
        let tabs = kairo_ui_kit::components::SegmentedControl::new("screens-tabs", "Settings")
            .tabs()
            .fit()
            .selected(active.clone())
            .options(
                tabs.iter()
                    .map(|t| kairo_ui_kit::components::SegmentOption::new(*t, capitalize(t))),
            )
            .on_change(move |key, window, cx| {
                entity.update(cx, |t, cx| {
                    t.tab = key.to_string();
                    t.clear_fields();
                    t.sync_preview(window, cx);
                    cx.notify();
                })
            });
        let canvas = if stage.is_some()
            || output
                .as_ref()
                .is_some_and(|o| o.kind == OverlayOutputKind::Screen)
        {
            Some(self.arrangement(cx))
        } else if let Some(o) = output
            .as_ref()
            .filter(|o| o.kind == OverlayOutputKind::Device)
        {
            let port = snapshot
                .devices
                .ports
                .iter()
                .find(|p| p.id == o.device_port_id);
            let sending = o.enabled && snapshot.devices.outputs.get(&o.id).is_some_and(|s| s.ready);
            Some(
                div()
                    .h(px(185.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(k.surface_stage)
                    .child(
                        div()
                            .w(px(240.))
                            .h(px(135.))
                            .rounded(radius::md())
                            .bg(k.surface_display)
                            .border_2()
                            .border_color(if sending { k.live } else { k.surface_display })
                            .p_3()
                            .text_size(text::SM.size)
                            .child(port.map(|p| p.port_name.clone()).unwrap_or_else(|| {
                                if o.device_port_label.is_empty() {
                                    "No output chosen".into()
                                } else {
                                    o.device_port_label.clone()
                                }
                            })),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };
        let fields = if let Some(o) = output {
            self.output_fields(&o, &active, window, cx)
        } else {
            self.stage_fields(&stage.unwrap(), &active, cx)
        };
        v_flex()
            .h_full()
            .child(header)
            .when_some(canvas, |d, c| d.child(c))
            .child(
                div()
                    .id("screens-detail-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .child(
                        v_flex().gap_4().child(tabs).child(
                            div()
                                .bg(k.surface_secondary)
                                .rounded(radius::lg())
                                .p_5()
                                .child(fields),
                        ),
                    ),
            )
            .into_any_element()
    }
}
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}
pub(crate) fn new_id(prefix: &str) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{prefix}-{:x}-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
impl Focusable for ScreensWindow {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.dialog.read(cx).focus_handle(cx)
    }
}
impl Render for ScreensWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.events;
        let k = *cx.kairo();
        let sidebar = self.sidebar(cx);
        let content = self.content(window, cx);
        let grip = div()
            .id("screens-resize")
            .test_support()
            .absolute()
            .right_0()
            .bottom_0()
            .size_4()
            .tab_index(0)
            .track_focus(&self.resize_focus)
            .role(Role::Slider)
            .aria_label("Resize Screens")
            .cursor_crosshair()
            .on_drag(FrameResize, |drag, _, _, cx| cx.new(|_| *drag))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, event: &MouseDownEvent, w, cx| {
                    t.resize_focus.focus(w, cx);
                    t.resize_press = Some((event.position, t.frame));
                }),
            )
            .on_drag_move(cx.listener(|t, event: &DragMoveEvent<FrameResize>, w, cx| {
                if let Some((origin, start)) = t.resize_press {
                    t.resize(
                        (
                            start.0 + f32::from(event.event.position.x - origin.x),
                            start.1 + f32::from(event.event.position.y - origin.y),
                        ),
                        w,
                        cx,
                    );
                }
            }))
            .on_key_down(cx.listener(|t, event: &KeyDownEvent, w, cx| {
                let (dx, dy) = match event.keystroke.key.as_str() {
                    "left" => (-16., 0.),
                    "right" => (16., 0.),
                    "up" => (0., -16.),
                    "down" => (0., 16.),
                    _ => return,
                };
                t.resize((t.frame.0 + dx, t.frame.1 + dy), w, cx);
                cx.stop_propagation();
            }));
        let body = h_flex()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(k.surface)
            .text_color(k.text_primary)
            .child(sidebar)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when_some(self.error.clone(), |d, e| {
                        d.child(
                            div()
                                .id("screens-error")
                                .test_support()
                                .role(Role::Alert)
                                .p_3()
                                .text_size(text::XS.size)
                                .text_color(k.destructive)
                                .child(e),
                        )
                    })
                    .when(!self.message.is_empty(), |d| {
                        d.child(
                            div()
                                .id("screens-result")
                                .test_support()
                                .role(Role::Status)
                                .px_6()
                                .py_2()
                                .text_size(text::XS.size)
                                .child(self.message.clone()),
                        )
                    })
                    .child(content),
            )
            .child(grip)
            .when_some(self.opened.take(), |d, start| {
                d.child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, _, _| {
                            tracing::info!(
                                ms = start.elapsed().as_secs_f64() * 1000.,
                                "[Screens] opened"
                            )
                        },
                    )
                    .absolute()
                    .size_0(),
                )
            });
        self.dialog.update(cx, |d, cx| d.frame(body, window, cx))
    }
}

#[derive(Clone, Copy)]
struct FrameResize;
impl Render for FrameResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Drop for ScreensWindow {
    fn drop(&mut self) {
        self.flush_edits();
    }
}
