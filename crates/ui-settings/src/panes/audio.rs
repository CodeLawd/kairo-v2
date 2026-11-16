//! Audio: the input list with a live level meter while the pane is open, and
//! the playback device for music, video and live input sound.

use crate::backend::{InputDevice, MeterEvent, MeterHandle};
use crate::model::{
    METER_SEGMENTS, Section, Segment, meter_percent, meter_segments, output_choices, resolve_input,
};
use crate::widgets;
use crate::window::SettingsWindow;
use futures_util::StreamExt;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Role, SharedString,
    StatefulInteractiveElement, Styled, Task, Window, div, prelude::FluentBuilder, px,
};
use kairo_domain::status::AudioLevel;
use kairo_ui_kit::components::PopoverTriggerExt;
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use std::time::Duration;

/// How often the device lists are re-read while the pane is open, standing
/// in for the browser's `devicechange` event.
pub const DEVICE_POLL: Duration = Duration::from_secs(3);

/// Element ids, for tests.
pub mod ids {
    /// The input list.
    pub const INPUTS: &str = "settings-audio-inputs";
    /// "Looking for microphones…".
    pub const LOOKING: &str = "settings-audio-looking";
    /// "No microphones found…".
    pub const NO_INPUTS: &str = "settings-audio-none";
    /// The meter.
    pub const METER: &str = "settings-audio-meter";
    /// "Live".
    pub const LIVE: &str = "settings-audio-live";
    /// The capture error.
    pub const ERROR: &str = "settings-audio-error";
    /// The playback device picker.
    pub const OUTPUT: &str = "settings-audio-output";
    /// An input row: `settings-audio-input-<index>`.
    pub fn input(index: usize) -> String {
        format!("settings-audio-input-{index}")
    }
}

/// The Audio pane's state.
#[derive(Default)]
pub struct AudioPane {
    /// The inputs, once listed.
    pub devices: Option<Vec<InputDevice>>,
    /// Output labels, the system default left out.
    pub outputs: Vec<String>,
    /// The meter is capturing.
    pub listening: bool,
    /// The last reading.
    pub level: Option<AudioLevel>,
    /// Why capture failed.
    pub error: Option<String>,
    /// The device being metered.
    pub metering: Option<String>,
    meter: Option<MeterHandle>,
    meter_task: Option<Task<()>>,
    poll: Option<Task<()>>,
}

impl AudioPane {
    /// The input the meter and capture use.
    pub fn selected(&self, saved: &str) -> Option<String> {
        let devices: Vec<_> = self
            .devices
            .as_ref()?
            .iter()
            .map(|d| d.device.clone())
            .collect();
        Some(resolve_input(&devices, saved))
    }
}

impl SettingsWindow {
    pub(crate) fn open_audio(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.read_devices(cx);
        if self.audio.poll.is_none() {
            self.audio.poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(DEVICE_POLL).await;
                    if this.update(cx, |this, cx| this.read_devices(cx)).is_err() {
                        break;
                    }
                }
            }));
        }
    }

    pub(crate) fn close_audio(&mut self, cx: &mut Context<Self>) {
        self.audio.poll = None;
        self.stop_meter(cx);
    }

    fn read_devices(&mut self, cx: &mut Context<Self>) {
        let inputs = self.backend.input_devices();
        let outputs = self.backend.output_devices();
        cx.spawn(async move |this, cx| {
            let inputs = inputs.await;
            let outputs = outputs.await;
            let _ = this.update(cx, |this, cx| {
                if this.audio.devices.as_ref() != Some(&inputs) {
                    this.audio.devices = Some(inputs);
                }
                this.audio.outputs = outputs;
                this.sync_meter(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Starts, restarts or keeps the meter for the selected input.
    fn sync_meter(&mut self, cx: &mut Context<Self>) {
        if self.section() != Section::Audio {
            return;
        }
        let Some(selected) = self.audio.selected(&self.settings.audio.device_id) else {
            return;
        };
        if self.audio.metering.as_deref() == Some(selected.as_str()) {
            return;
        }
        self.stop_meter(cx);
        self.audio.metering = Some(selected.clone());
        self.audio.error = None;
        let request = self.backend.start_meter(selected.clone());
        self.audio.meter_task = Some(cx.spawn(async move |this, cx| {
            let (handle, mut events) = match request.await {
                Ok(started) => started,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.audio.metering.as_deref() == Some(selected.as_str()) {
                            this.audio.error = Some(error);
                            this.audio.listening = false;
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            let kept = this.update(cx, |this, cx| {
                // Another device was picked while this one was opening.
                if this.audio.metering.as_deref() != Some(selected.as_str()) {
                    return false;
                }
                this.audio.meter = Some(handle);
                this.audio.listening = true;
                cx.notify();
                true
            });
            if !matches!(kept, Ok(true)) {
                return;
            }
            while let Some(event) = events.next().await {
                let alive = this.update(cx, |this, cx| {
                    match event {
                        MeterEvent::Level(level) => this.audio.level = Some(level),
                        MeterEvent::Error(error) => {
                            this.audio.error = Some(error);
                            this.audio.listening = false;
                            this.audio.meter = None;
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    fn stop_meter(&mut self, cx: &mut Context<Self>) {
        self.audio.meter_task = None;
        self.audio.meter = None;
        self.audio.metering = None;
        self.audio.listening = false;
        self.audio.level = None;
        cx.notify();
    }

    /// Picks an input.
    pub fn pick_input(&mut self, id: String, cx: &mut Context<Self>) {
        self.settings.audio.device_id = id;
        self.sync_meter(cx);
        cx.notify();
    }

    /// Picks the playback device (`""` for System Default).
    pub fn pick_output(&mut self, label: String, cx: &mut Context<Self>) {
        self.settings.presentation.audio.output_label = label;
        cx.notify();
    }

    /// Whether the meter is capturing.
    pub fn metering(&self) -> Option<&str> {
        self.audio
            .listening
            .then_some(())
            .and(self.audio.metering.as_deref())
    }

    pub(crate) fn render_audio(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let selected = self.audio.selected(&self.settings.audio.device_id);
        let list: AnyElement = match &self.audio.devices {
            None => h_flex()
                .id(ids::LOOKING)
                .test_support()
                .role(Role::Status)
                .gap_2()
                .items_center()
                .px(px(14.))
                .py_6()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T500))
                .child(Icon::new(KairoIcon::Loader).size(px(13.)))
                .child("Looking for microphones…")
                .into_any_element(),
            Some(devices) if devices.is_empty() => div()
                .id(ids::NO_INPUTS)
                .test_support()
                .px(px(14.))
                .py_6()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T500))
                .child("No microphones found. Allow microphone access and reopen Settings.")
                .into_any_element(),
            Some(devices) => v_flex()
                .id(ids::INPUTS)
                .test_support()
                .role(Role::ListBox)
                .aria_label("Audio input devices")
                .px_1p5()
                .pb_1p5()
                .children(devices.iter().enumerate().map(|(ix, d)| {
                    let active = selected.as_deref() == Some(d.device.id.as_str());
                    let id = d.device.id.clone();
                    h_flex()
                        .id(SharedString::from(ids::input(ix)))
                        .test_support()
                        .role(Role::ListBoxOption)
                        .aria_label(d.device.label.clone())
                        .aria_selected(active)
                        .tab_index(0)
                        .w_full()
                        .gap_3()
                        .items_center()
                        .rounded(radius::md())
                        .px_2()
                        .py(px(7.))
                        .text_size(text::SM.size)
                        .cursor_pointer()
                        .map(|this| {
                            if active {
                                this.bg(k.surface_elevated).text_color(k.text_primary)
                            } else {
                                this.text_color(k.text(TextStep::T200))
                                    .hover(|s| s.bg(k.surface_tertiary))
                            }
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(d.device.label.clone()),
                        )
                        .child(
                            div()
                                .w(px(88.))
                                .truncate()
                                .text_color(if active {
                                    k.text(TextStep::T400)
                                } else {
                                    k.text(TextStep::T500)
                                })
                                .child(d.kind.clone()),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.pick_input(id.clone(), cx)),
                        )
                }))
                .into_any_element(),
        };
        let listening = self.audio.listening;
        let segments = meter_segments(self.audio.level.as_ref(), listening);
        let percent = meter_percent(self.audio.level.as_ref(), listening);
        let meter = h_flex()
            .id(ids::METER)
            .test_support()
            .role(Role::Meter)
            .aria_label(format!("Input level {percent}%"))
            .h_2()
            .gap(px(2.))
            .children((0..METER_SEGMENTS).map(|i| {
                let color = match segments[i] {
                    Segment::Off => k.surface_elevated,
                    Segment::Ok => k.hue(kairo_ui_kit::Hue::Green),
                    Segment::Warn => k.hue(kairo_ui_kit::Hue::Amber),
                    Segment::Clip => k.destructive,
                };
                div().min_w_0().flex_1().rounded_sm().bg(color)
            }));
        let input = v_flex()
            .gap_2()
            .child(
                div()
                    .px_0p5()
                    .text_size(text::XS.size)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(k.text(TextStep::T400))
                    .child("Input"),
            )
            .child(
                v_flex()
                    .rounded(radius::lg())
                    .overflow_hidden()
                    .bg(k.surface_tertiary)
                    .child(
                        h_flex()
                            .gap_3()
                            .px(px(14.))
                            .pb_1()
                            .pt(px(10.))
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T600))
                            .child(div().flex_1().child("Name"))
                            .child(div().w(px(88.)).child("Type")),
                    )
                    .child(list)
                    .child(
                        v_flex()
                            .px(px(14.))
                            .py_3()
                            .child(
                                h_flex()
                                    .mb_2()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(text::SM.size)
                                            .text_color(k.text_primary)
                                            .child("Input level"),
                                    )
                                    .when(listening, |this| {
                                        this.child(
                                            div()
                                                .id(ids::LIVE)
                                                .test_support()
                                                .text_size(text::LABEL.size)
                                                .text_color(k.text(TextStep::T600))
                                                .child("Live"),
                                        )
                                    }),
                            )
                            .child(meter)
                            .when_some(self.audio.error.clone(), |this, error| {
                                this.child(
                                    div()
                                        .id(ids::ERROR)
                                        .test_support()
                                        .role(Role::Alert)
                                        .mt_2()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.danger_text)
                                        .child(error),
                                )
                            }),
                    ),
            );

        let saved = self.settings.presentation.audio.output_label.clone();
        let choices = output_choices(&saved, &self.audio.outputs);
        let current = choices
            .iter()
            .find(|(value, _)| *value == saved)
            .map_or_else(|| "System Default".to_owned(), |(_, label)| label.clone());
        let entity = cx.entity();
        let output = v_flex()
            .gap_2()
            .child(
                div()
                    .px_0p5()
                    .text_size(text::XS.size)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(k.text(TextStep::T400))
                    .child("Output"),
            )
            .child(
                v_flex()
                    .gap_2()
                    .rounded(radius::lg())
                    .bg(k.surface_tertiary)
                    .p(px(14.))
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T300))
                            .child("Playback device"),
                    )
                    .child(
                        Button::new(ids::OUTPUT)
                            .outline()
                            .small()
                            .w_full()
                            .label(current)
                            .dropdown_caret(true)
                            .accessibility_label("Playback device")
                            .popover_trigger()
                            .dropdown_menu(move |mut menu, _, _| {
                                for (value, label) in &choices {
                                    let entity = entity.clone();
                                    let value = value.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(label.clone())
                                            .checked(value == saved)
                                            .on_click(move |_, _, cx| {
                                                let value = value.clone();
                                                entity.update(cx, |this, cx| {
                                                    this.pick_output(value, cx)
                                                })
                                            }),
                                    );
                                }
                                menu
                            }),
                    )
                    .child(widgets::muted(
                        "Routes music, video and live input sound to this device.",
                        cx,
                    )),
            );

        v_flex()
            .gap_3()
            .child(input)
            .child(output)
            .children(
                self.save_error
                    .clone()
                    .map(|e| widgets::notice("settings-save-error", e, cx)),
            )
            .child(widgets::save_bar(
                Section::Audio,
                self.saved == Some(Section::Audio),
                cx.listener(|this, _, _, cx| this.save(Section::Audio, cx)),
                cx,
            ))
            .into_any_element()
    }
}
