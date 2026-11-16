//! Service records in the transcript's foot bar (`ServicePanel.tsx`): the
//! recording light, talk time and name (click to review), End, and the Save,
//! Discard and Review dialogs.

use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, div, prelude::FluentBuilder, px, size,
};
use kairo_domain::services::{
    ServiceCommand, ServiceRecord, ServiceStatus, UploadStatus, service_talk_time_ms,
    service_text_export,
};
use kairo_ui_kit::components::modal::{MovableDialog, MovableDialogEvent};
use kairo_ui_kit::tokens::{Step, TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::widgets::{primary_button, secondary_button, window_layer};
use super::{Booth, BoothAnswer, BoothCall, format_local_time, now_ms};

/// Element ids, for tests.
pub mod ids {
    pub const REVIEW: &str = "service-review";
    pub const END: &str = "service-end";
    pub const SAVE: &str = "service-save";
    pub const KEEP: &str = "service-keep";
    pub const DISCARD_LINK: &str = "service-discard-link";
    pub const DISCARD: &str = "service-discard";
    pub const ERROR: &str = "service-error";
}

/// `formatServiceDuration`: `HH:MM:SS`, never negative.
pub fn format_duration(ms: f64) -> String {
    let total = (ms / 1000.0).floor().max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// The review dialog's sections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReviewTab {
    #[default]
    Nuggets,
    Transcript,
    Notes,
    Scriptures,
}

impl ReviewTab {
    const ALL: [ReviewTab; 4] = [
        ReviewTab::Nuggets,
        ReviewTab::Transcript,
        ReviewTab::Notes,
        ReviewTab::Scriptures,
    ];
    fn label(self) -> &'static str {
        match self {
            ReviewTab::Nuggets => "Nuggets",
            ReviewTab::Transcript => "Transcript",
            ReviewTab::Notes => "Notes",
            ReviewTab::Scriptures => "Scriptures",
        }
    }
    fn count(self, record: &ServiceRecord) -> usize {
        match self {
            ReviewTab::Nuggets => record.nuggets.len(),
            ReviewTab::Transcript => record.transcript.len(),
            ReviewTab::Notes => record.notes.len(),
            ReviewTab::Scriptures => record.scriptures.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    End,
    Discard,
}

pub struct ServicePanel {
    booth: Entity<Booth>,
    title: Entity<InputState>,
    speaker: Entity<InputState>,
    plan_id: Option<String>,
    view_id: Option<String>,
    tab: ReviewTab,
    busy: bool,
    error: String,
    open: Option<Open>,
    frame: Option<Entity<MovableDialog>>,
    _tick: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl ServicePanel {
    pub fn new(booth: Entity<Booth>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Sunday morning service"));
        let speaker = cx.new(|cx| InputState::new(window, cx));
        // One clock for the talk time; it repaints only while a service runs.
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let running = this.update(cx, |this, cx| {
                    let running =
                        this.booth.read(cx).active_service().is_some_and(|s| {
                            s.ended_at.is_none() && s.paused_at.flatten().is_none()
                        });
                    if running {
                        cx.notify();
                    }
                });
                if running.is_err() {
                    break;
                }
            }
        });
        let subscriptions = vec![cx.observe(&booth, |_, _, cx| cx.notify())];
        Self {
            booth,
            title,
            speaker,
            plan_id: None,
            view_id: None,
            tab: ReviewTab::default(),
            busy: false,
            error: String::new(),
            open: None,
            frame: None,
            _tick: tick,
            _subscriptions: subscriptions,
        }
    }

    pub fn error(&self) -> &str {
        &self.error
    }
    pub fn is_ending(&self) -> bool {
        self.open == Some(Open::End)
    }
    pub fn is_discarding(&self) -> bool {
        self.open == Some(Open::Discard)
    }
    pub fn reviewing(&self) -> Option<&str> {
        self.view_id.as_deref()
    }

    fn frame(
        &mut self,
        label: &str,
        width: f32,
        height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let frame = cx.new(|cx| {
            MovableDialog::new(
                "service-dialog",
                label.to_owned(),
                size(px(width), px(height)),
                window,
                cx,
            )
        });
        self._subscriptions.push(cx.subscribe(
            &frame,
            |this, _, event: &MovableDialogEvent, cx| {
                if matches!(event, MovableDialogEvent::Dismiss) {
                    this.close(cx);
                }
            },
        ));
        self.frame = Some(frame);
    }

    /// End: the save form, filled from an earlier attempt.
    pub fn open_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.booth.read(cx).active_service().cloned() else {
            return;
        };
        self.error.clear();
        self.title.update(cx, |input, cx| {
            input.set_value(active.title.clone(), window, cx)
        });
        self.speaker.update(cx, |input, cx| {
            input.set_value(active.speaker.clone(), window, cx)
        });
        self.plan_id = active.notes.first().map(|n| n.id.clone());
        self.open = Some(Open::End);
        self.frame("Save service", 448., 392., window, cx);
        cx.notify();
    }

    pub fn open_review(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.view_id = Some(id);
        self.open = None;
        self.frame("Service review", 672., 620., window, cx);
        cx.notify();
    }

    /// Closing is refused while a request is under way.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.busy && self.open.is_some() {
            return;
        }
        self.open = None;
        self.view_id = None;
        self.frame = None;
        cx.notify();
    }

    /// `run`: one request, busy while it runs, its error shown.
    fn run(
        &mut self,
        command: ServiceCommand,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        self.busy = true;
        self.error.clear();
        cx.notify();
        let panel = cx.entity().downgrade();
        self.booth.update(cx, |booth, cx| {
            booth.call(BoothCall::Service(command), cx, move |booth, result, cx| {
                if let Ok(BoothAnswer::Services(snapshot)) = &result {
                    booth.set_services(snapshot.clone());
                }
                let _ = panel.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(_) => then(this, cx),
                        Err(error) => this.error = error,
                    }
                    cx.notify();
                });
            });
        });
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        let command = ServiceCommand::End {
            title: self.title.read(cx).value().to_string(),
            speaker: self.speaker.read(cx).value().to_string(),
            plan_id: self.plan_id.clone(),
        };
        self.run(command, cx, |this, cx| {
            this.open = None;
            this.frame = None;
            cx.notify();
        });
    }

    pub fn ask_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = Some(Open::Discard);
        self.frame("Discard this service?", 384., 216., window, cx);
        cx.notify();
    }

    pub fn discard(&mut self, cx: &mut Context<Self>) {
        self.run(ServiceCommand::Discard, cx, |this, cx| {
            this.open = None;
            this.frame = None;
            cx.notify();
        });
    }

    fn export(&mut self, record: &ServiceRecord, json: bool, cx: &mut Context<Self>) {
        let (file_name, body) = if json {
            (
                format!("service-{}.json", record.id),
                serde_json::to_string_pretty(record).unwrap_or_default(),
            )
        } else {
            let slug: String = {
                let mut out = String::new();
                let mut dash = false;
                for c in record.title.chars() {
                    if c.is_ascii_alphanumeric() {
                        out.push(c);
                        dash = false;
                    } else if !dash {
                        out.push('-');
                        dash = true;
                    }
                }
                out
            };
            (
                format!("{slug}.txt"),
                service_text_export(record, super::format_local_date_time, |ms| {
                    format_local_time(ms, false)
                }),
            )
        };
        self.booth.update(cx, |booth, cx| {
            booth.program_call(BoothCall::Export { file_name, body }, cx)
        });
    }

    fn bar(&self, active: &ServiceRecord, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let paused = active.paused_at.flatten().is_some();
        let label = if active.title.is_empty() {
            "Untitled service".to_owned()
        } else {
            active.title.clone()
        };
        let elapsed = format_duration(service_talk_time_ms(active, now_ms()));
        let id = active.id.clone();
        let review = if active.speaker.is_empty() {
            format!("Review — {label}")
        } else {
            format!("Review — {label} · {}", active.speaker)
        };
        h_flex()
            .min_w_0()
            .flex_1()
            .items_center()
            .gap_1()
            .child(
                h_flex()
                    .id(ids::REVIEW)
                    .test_support()
                    .aria_label(format!(
                        "{} — review",
                        if paused { "Paused" } else { "Recording" }
                    ))
                    .tooltip(super::widgets::tip(review))
                    .h_8()
                    .min_w_0()
                    .flex_1()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(k.surface_tertiary))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_review(id.clone(), window, cx)
                    }))
                    .child(
                        div()
                            .size(px(6.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(if paused {
                                k.text(TextStep::T500)
                            } else {
                                k.destructive
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(text::MONO_FAMILY)
                            .text_size(text::XS.size)
                            .text_color(if paused {
                                k.text(TextStep::T500)
                            } else {
                                k.text_primary
                            })
                            .child(elapsed),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(text::XS.size)
                            .text_color(if active.title.is_empty() {
                                k.text(TextStep::T500)
                            } else {
                                k.text(TextStep::T300)
                            })
                            .child(label),
                    ),
            )
            .child(
                Button::new(ids::END)
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(k.tints.red)
                            .foreground(k.danger_text)
                            .hover(k.tints.red.opacity(2.0))
                            .active(k.tints.red.opacity(2.0)),
                    )
                    // Solid fill: the custom variant paints its colour at 20%.
                    .bg(k.tints.red)
                    .small()
                    .icon(Icon::new(KairoIcon::Square).size(px(9.)))
                    .label("End")
                    .accessibility_label("End service")
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.open_end(window, cx))),
            )
            .into_any_element()
    }

    fn end_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let frame = self.frame.clone()?;
        let k = *cx.kairo();
        let plans = self.booth.read(cx).plans.clone();
        let plan_label = self
            .plan_id
            .as_ref()
            .and_then(|id| plans.iter().find(|p| &p.id == id))
            .map(|p| p.title.clone())
            .unwrap_or_else(|| "No notes".into());
        let handle = frame.update(cx, |f, cx| f.drag_handle("service-end-drag", cx));
        let busy = self.busy;
        let field = |label: &str, child: AnyElement| {
            v_flex()
                .gap_1()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T400))
                .child(label.to_owned())
                .child(child)
        };
        let panel = cx.entity().downgrade();
        let content = v_flex()
            .size_full()
            .p_5()
            .child(
                handle
                    .child(
                        div()
                            .text_size(text::BASE.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child("Save service"),
                    )
                    .child(
                        div()
                            .mt_1p5()
                            .text_size(text::SM.size)
                            .text_color(k.text(TextStep::T400))
                            .child("Transcription stops and this message is saved. Name it now — leave the name blank and it is filed by date."),
                    ),
            )
            .child(
                v_flex()
                    .mt_5()
                    .gap_4()
                    .child(field(
                        "Service name",
                        Input::new(&self.title).kairo_size()
                            .small()
                            .disabled(busy)
                            .aria_label("Service name")
                            .into_any_element(),
                    ))
                    .child(field(
                        "Speaker (optional)",
                        Input::new(&self.speaker).kairo_size()
                            .small()
                            .disabled(busy)
                            .aria_label("Speaker (optional)")
                            .into_any_element(),
                    ))
                    .child(field(
                        "Sermon notes",
                        Button::new("service-plan")
                            .outline()
                            .small()
                            .w_full()
                            .label(plan_label)
                            .dropdown_caret(true)
                            .disabled(busy)
                            .popover_trigger().dropdown_menu(move |mut menu, _, _| {
                                let pick = panel.clone();
                                menu = menu.item(PopupMenuItem::new("No notes").on_click(
                                    move |_, _, cx| {
                                        let _ = pick.update(cx, |this, cx| {
                                            this.plan_id = None;
                                            cx.notify();
                                        });
                                    },
                                ));
                                for plan in &plans {
                                    let pick = panel.clone();
                                    let id = plan.id.clone();
                                    menu = menu.item(PopupMenuItem::new(plan.title.clone()).on_click(
                                        move |_, _, cx| {
                                            let _ = pick.update(cx, |this, cx| {
                                                this.plan_id = Some(id.clone());
                                                cx.notify();
                                            });
                                        },
                                    ));
                                }
                                menu
                            })
                            .into_any_element(),
                    )),
            )
            .when(!self.error.is_empty(), |this| {
                this.child(
                    div()
                        .id(ids::ERROR)
                        .test_support()
                        .mt_3()
                        .text_size(text::LABEL.size)
                        .text_color(k.danger_text)
                        .child(self.error.clone()),
                )
            })
            .child(
                h_flex()
                    .mt_5()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        Button::new(ids::DISCARD_LINK)
                            .ghost()
                            .xsmall()
                            .label("Discard service")
                            .text_color(k.text(TextStep::T600))
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.ask_discard(window, cx))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                secondary_button(ids::KEEP, "Keep recording", cx)
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                            )
                            .child(
                                primary_button(ids::SAVE, if busy { "Saving…" } else { "End and save" }, cx)
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                            ),
                    ),
            );
        let element = frame.update(cx, |f, cx| f.frame(content, window, cx));
        Some(window_layer(element, window))
    }

    fn discard_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let frame = self.frame.clone()?;
        let k = *cx.kairo();
        let handle = frame.update(cx, |f, cx| f.drag_handle("service-discard-drag", cx));
        let busy = self.busy;
        let content = v_flex()
            .size_full()
            .p_5()
            .child(
                handle
                    .child(
                        div()
                            .text_size(text::BASE.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child("Discard this service?"),
                    )
                    .child(
                        div()
                            .mt_1p5()
                            .text_size(text::SM.size)
                            .text_color(k.text(TextStep::T400))
                            .child("The transcript, nuggets and detected scriptures from this service are deleted. This cannot be undone."),
                    ),
            )
            .when(!self.error.is_empty(), |this| {
                this.child(
                    div()
                        .mt_3()
                        .text_size(text::LABEL.size)
                        .text_color(k.danger_text)
                        .child(self.error.clone()),
                )
            })
            .child(
                h_flex()
                    .mt_5()
                    .justify_end()
                    .gap_2()
                    .child(
                        secondary_button("service-discard-cancel", "Cancel", cx)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(
                        Button::new(ids::DISCARD)
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .color(k.destructive)
                                    .foreground(k.white)
                                    .hover(k.destructive.opacity(0.85))
                                    .active(k.destructive.opacity(0.85)),
                            )
        // Solid fill: the custom variant paints its colour at 20%.
        .bg(k.destructive)
                            .small()
                            .label(if busy { "Discarding…" } else { "Discard" })
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.discard(cx))),
                    ),
            );
        let element = frame.update(cx, |f, cx| f.frame(content, window, cx));
        Some(window_layer(element, window))
    }

    fn review_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let frame = self.frame.clone()?;
        let id = self.view_id.clone()?;
        let Some(record) = self
            .booth
            .read(cx)
            .services
            .services
            .iter()
            .find(|s| s.id == id)
            .cloned()
        else {
            // The record is gone (discarded): the dialog closes with it.
            self.view_id = None;
            self.frame = None;
            return None;
        };
        let k = *cx.kairo();
        let handle = frame.update(cx, |f, cx| f.drag_handle("service-review-drag", cx));
        let meta = {
            let mut parts = Vec::new();
            if !record.speaker.is_empty() {
                parts.push(record.speaker.clone());
            }
            parts.push(super::format_local_date_time(record.created_at));
            parts.push(format_duration(service_talk_time_ms(&record, now_ms())));
            parts.push(
                if record.status == ServiceStatus::Ended {
                    "Ended"
                } else {
                    "In progress"
                }
                .to_owned(),
            );
            parts.join(" · ")
        };
        let tabs = h_flex()
            .gap_0p5()
            .p_0p5()
            .rounded_lg()
            .bg(k.surface_tertiary)
            .children(ReviewTab::ALL.into_iter().map(|tab| {
                let selected = tab == self.tab;
                div()
                    .id(SharedString::from(format!("service-tab-{}", tab.label())))
                    .test_support()
                    .px_2p5()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .text_size(text::LABEL.size)
                    .font_weight(FontWeight::MEDIUM)
                    .when(selected, |s| {
                        s.bg(k.surface_elevated).text_color(k.text_primary)
                    })
                    .when(!selected, |s| s.text_color(k.text(TextStep::T500)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
                    .child(format!("{} {}", tab.label(), tab.count(&record)))
            }));
        let recap = self.recap(&record, cx);
        let export_record = record.clone();
        let json_record = record.clone();
        let header = v_flex()
            .px_5()
            .pt_5()
            .pb_4()
            .child(
                handle
                    .child(
                        div()
                            .truncate()
                            .text_size(text::BASE.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child(if record.title.is_empty() {
                                "Unnamed service".to_owned()
                            } else {
                                record.title.clone()
                            }),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(meta),
                    ),
            )
            .child(
                h_flex()
                    .mt_4()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(tabs)
                    .child(
                        h_flex()
                            .gap_1()
                            .children(recap)
                            .child(
                                Button::new("service-export")
                                    .ghost()
                                    .xsmall()
                                    .label("Export")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.export(&export_record, false, cx)
                                    })),
                            )
                            .child(
                                Button::new("service-json")
                                    .ghost()
                                    .xsmall()
                                    .label("JSON")
                                    .tooltip("Download full JSON archive")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.export(&json_record, true, cx)
                                    })),
                            ),
                    ),
            );
        let body = self.review_body(&record, cx);
        let content = v_flex()
            .size_full()
            .relative()
            .child(header)
            .child(
                div()
                    .id("service-review-body")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px_5()
                    .py_4()
                    .when(!self.error.is_empty(), |this| {
                        this.child(
                            div()
                                .mb_3()
                                .text_size(text::LABEL.size)
                                .text_color(k.danger_text)
                                .child(self.error.clone()),
                        )
                    })
                    .child(body),
            )
            .child(
                div().absolute().top_3().right_3().child(
                    Button::new("service-review-close")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(KairoIcon::X).size(px(14.)))
                        .accessibility_label("Close")
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                ),
            );
        let element = frame.update(cx, |f, cx| f.frame(content, window, cx));
        Some(window_layer(element, window))
    }

    /// `RecapStatus`: where an ended service is on its way to the website.
    fn recap(&self, record: &ServiceRecord, cx: &mut Context<Self>) -> Option<AnyElement> {
        let upload = &record.upload;
        if record.status != ServiceStatus::Ended || upload.status == UploadStatus::Idle {
            return None;
        }
        let k = *cx.kairo();
        Some(match upload.status {
            UploadStatus::Uploaded => {
                let path = upload
                    .sermon_id
                    .as_ref()
                    .map_or("/dashboard/sermons".to_owned(), |id| {
                        format!("/dashboard/sermons/{id}")
                    });
                Button::new("service-recap")
                    .ghost()
                    .xsmall()
                    .label("Recap published")
                    .text_color(k.accent(Step::S400))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let path = path.clone();
                        this.booth
                            .update(cx, |b, cx| b.program_call(BoothCall::OpenWeb(path), cx));
                    }))
                    .into_any_element()
            }
            UploadStatus::Failed => {
                let id = record.id.clone();
                let mut button = Button::new("service-recap")
                    .ghost()
                    .xsmall()
                    .label("Recap not sent — Retry")
                    .text_color(k.warning_text)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.run(
                            ServiceCommand::Upload {
                                service_id: id.clone(),
                            },
                            cx,
                            |_, _| {},
                        )
                    }));
                if let Some(error) = upload.error.clone() {
                    button = button.tooltip(error);
                }
                button.into_any_element()
            }
            _ => div()
                .px_2()
                .py_1p5()
                .text_size(text::LABEL.size)
                .text_color(k.text(TextStep::T500))
                .child(if upload.status == UploadStatus::Uploading {
                    "Publishing recap…"
                } else {
                    "Recap queued"
                })
                .into_any_element(),
        })
    }

    fn review_body(&self, record: &ServiceRecord, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let empty = |copy: &str| {
            div()
                .py_10()
                .text_center()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T600))
                .child(copy.to_owned())
                .into_any_element()
        };
        let card = || {
            v_flex()
                .rounded_lg()
                .bg(k.surface_secondary)
                .px(px(14.))
                .py_3()
        };
        match self.tab {
            ReviewTab::Nuggets => {
                let id = record.id.clone();
                let mut list =
                    v_flex().gap_3().child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(record.analysis_error.clone().unwrap_or_else(|| {
                                        "Quotes selected from the message.".into()
                                    })),
                            )
                            .child(
                                Button::new("service-scan")
                                    .ghost()
                                    .xsmall()
                                    .label("Scan speech")
                                    .text_color(k.accent(Step::S400))
                                    .disabled(self.busy || record.transcript.is_empty())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run(
                                            ServiceCommand::Analyze {
                                                service_id: id.clone(),
                                            },
                                            cx,
                                            |_, _| {},
                                        )
                                    })),
                            ),
                    );
                if record.nuggets.is_empty() {
                    return list.child(empty("No nuggets yet.")).into_any_element();
                }
                for nugget in &record.nuggets {
                    let copy = nugget.text.clone();
                    let (service_id, nugget_id) = (record.id.clone(), nugget.id.clone());
                    list =
                        list.child(
                            card()
                                .child(
                                    div()
                                        .text_size(text::SM.size)
                                        .text_color(k.text(TextStep::T200))
                                        .child(nugget.text.clone()),
                                )
                                .child(
                                    h_flex()
                                        .mt_2p5()
                                        .gap_3()
                                        .items_center()
                                        .text_size(text::MICRO.size)
                                        .text_color(k.text(TextStep::T500))
                                        .child(capitalize(nugget.origin.as_str()))
                                        .child("·")
                                        .child(format_local_time(nugget.captured_at, false))
                                        .child(div().flex_1())
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "nugget-copy-{}",
                                                nugget.id
                                            )))
                                            .ghost()
                                            .xsmall()
                                            .label("Copy")
                                            .on_click(move |_, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    copy.clone(),
                                                ))
                                            }),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "nugget-remove-{}",
                                                nugget.id
                                            )))
                                            .ghost()
                                            .xsmall()
                                            .label("Remove")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.run(
                                                    ServiceCommand::RemoveNugget {
                                                        service_id: service_id.clone(),
                                                        nugget_id: nugget_id.clone(),
                                                    },
                                                    cx,
                                                    |_, _| {},
                                                )
                                            })),
                                        ),
                                ),
                        );
                }
                list.into_any_element()
            }
            ReviewTab::Transcript if record.transcript.is_empty() => {
                empty("No transcript recorded.")
            }
            ReviewTab::Transcript => v_flex()
                .gap_3()
                .children(record.transcript.iter().map(|s| {
                    div()
                        .text_size(text::SM.size)
                        .text_color(k.text(TextStep::T300))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .font_family(text::MONO_FAMILY)
                                        .text_size(text::MICRO.size)
                                        .text_color(k.text(TextStep::T600))
                                        .child(format_local_time(s.timestamp, true)),
                                )
                                .child(div().child(s.text.clone())),
                        )
                }))
                .into_any_element(),
            ReviewTab::Notes if record.notes.is_empty() => empty("No sermon notes attached."),
            ReviewTab::Notes => v_flex()
                .gap_5()
                .children(record.notes.iter().map(|n| {
                    v_flex()
                        .child(
                            div()
                                .text_size(text::SM.size)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(k.text_primary)
                                .child(n.title.clone()),
                        )
                        .when_some(
                            n.source_text.clone().filter(|t| !t.is_empty()),
                            |this, source| {
                                this.child(
                                    div()
                                        .mt_2()
                                        .text_size(text::SM.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(source),
                                )
                            },
                        )
                        .when(!n.items.is_empty(), |this| {
                            this.child(v_flex().mt_3().gap_2().children(n.items.iter().map(
                                |item| {
                                    v_flex()
                                        .text_size(text::XS.size)
                                        .child(
                                            h_flex()
                                                .child(
                                                    div()
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(k.text(TextStep::T200))
                                                        .child(item.reference.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(k.text(TextStep::T600))
                                                        .child(format!(" · {}", item.translation)),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .mt_0p5()
                                                .text_size(text::LABEL.size)
                                                .text_color(k.text(TextStep::T500))
                                                .child(
                                                    item.verses
                                                        .iter()
                                                        .map(|v| v.text.as_str())
                                                        .collect::<Vec<_>>()
                                                        .join(" "),
                                                ),
                                        )
                                },
                            )))
                        })
                }))
                .into_any_element(),
            ReviewTab::Scriptures if record.scriptures.is_empty() => {
                empty("No scriptures detected.")
            }
            ReviewTab::Scriptures => v_flex()
                .gap_3()
                .children(record.scriptures.iter().map(|s| {
                    card()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(text::SM.size)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(k.text_primary)
                                        .child(s.reference.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T500))
                                        .child(s.translation.clone()),
                                ),
                        )
                        .child(
                            div()
                                .mt_1p5()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T400))
                                .child(
                                    s.verses
                                        .iter()
                                        .map(|v| v.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                ),
                        )
                }))
                .into_any_element(),
        }
    }
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

impl Render for ServicePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let active = self.booth.read(cx).active_service().cloned();
        // The end and discard dialogs close if the service ends elsewhere.
        if active.is_none() && self.open.is_some() {
            self.open = None;
            self.frame = None;
        }
        let dialog = match (self.open, self.view_id.is_some()) {
            (Some(Open::End), _) => self.end_dialog(window, cx),
            (Some(Open::Discard), _) => self.discard_dialog(window, cx),
            (None, true) => self.review_dialog(window, cx),
            _ => None,
        };
        h_flex()
            .id("service-panel")
            .min_w_0()
            .flex_1()
            .items_center()
            .when_some(active.as_ref(), |this, active| {
                this.child(self.bar(active, cx))
            })
            .when(!self.error.is_empty() && self.open.is_none(), |this| {
                this.child(
                    div()
                        .id("service-bar-error")
                        .test_support()
                        .min_w_0()
                        .truncate()
                        .px_2()
                        .text_size(text::LABEL.size)
                        .text_color(k.danger_text)
                        .child(self.error.clone()),
                )
            })
            .children(dialog)
    }
}

/// Lets an `App` read the panel in tests.
pub fn reviewing(panel: &Entity<ServicePanel>, cx: &App) -> Option<String> {
    panel.read(cx).view_id.clone()
}
