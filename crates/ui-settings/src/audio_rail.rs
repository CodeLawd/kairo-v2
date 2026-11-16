//! The rail's Audio popover (`AudioRailButton` in `RailPopovers.tsx`): the
//! input in use with a level meter while Kairo listens, the inputs to pick
//! from, and a way into Audio settings. It reads and writes the same
//! `audio` settings section as Settings → Audio.

use crate::backend::{InputDevice, SettingsBackend};
use crate::model::{meter_percent, resolve_input};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::{
    Context, EventEmitter, FontWeight, InteractiveElement, IntoElement, ParentElement, Render,
    Role, SharedString, StatefulInteractiveElement, Styled, Task, Window, div,
    prelude::FluentBuilder, px,
};
use kairo_domain::status::AudioLevel;
use kairo_ui_kit::tokens::{Hue, Step, TextStep, radius};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The rail meter's segments (`METER_SEGMENTS` in `RailPopovers.tsx`).
const SEGMENTS: usize = 24;

/// Levels older than this mean the capture stopped.
const LISTENING_GAP: Duration = Duration::from_millis(1500);

/// Element ids, for tests.
pub mod ids {
    /// The popover body.
    pub const PANEL: &str = "audio-rail";
    /// The meter.
    pub const METER: &str = "audio-rail-meter";
    /// Audio settings.
    pub const SETTINGS: &str = "audio-rail-settings";
    /// One input row.
    pub fn input(index: usize) -> String {
        format!("audio-rail-input-{index}")
    }
}

/// The popover asked for Audio settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenAudioSettings;

/// The popover's state.
pub struct AudioRail {
    backend: Arc<dyn SettingsBackend>,
    devices: Option<Vec<InputDevice>>,
    /// The stored `audio` section.
    section: Value,
    level: Option<(AudioLevel, Instant)>,
    _load: Option<Task<()>>,
}

impl EventEmitter<OpenAudioSettings> for AudioRail {}

impl AudioRail {
    /// A popover over `backend`'s audio section.
    pub fn new(backend: Arc<dyn SettingsBackend>) -> Self {
        Self {
            backend,
            devices: None,
            section: Value::Object(Default::default()),
            level: None,
            _load: None,
        }
    }

    /// Reads the inputs and the saved choice; a mic may have been plugged in.
    pub fn opened(&mut self, cx: &mut Context<Self>) {
        let devices = self.backend.input_devices();
        let section = self.backend.get_section("audio");
        self._load = Some(cx.spawn(async move |this, cx| {
            let devices = devices.await;
            let section = section.await;
            let _ = this.update(cx, |this, cx| {
                this.devices = Some(devices);
                if let Ok(section) = section {
                    this.section = section;
                }
                cx.notify();
            });
        }));
    }

    /// The capture's latest level (published only while Kairo listens).
    pub fn set_level(&mut self, level: AudioLevel, cx: &mut Context<Self>) {
        self.level = Some((level, Instant::now()));
        cx.notify();
    }

    fn saved(&self) -> String {
        self.section
            .get("deviceId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }

    /// Whether levels are arriving.
    pub fn listening(&self) -> bool {
        self.level
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < LISTENING_GAP)
    }

    /// Picks an input; the capture follows the setting.
    pub fn pick(&mut self, id: String, cx: &mut Context<Self>) {
        if let Value::Object(map) = &mut self.section {
            map.insert("deviceId".into(), Value::String(id));
        }
        let save = self.backend.set_section("audio", self.section.clone());
        cx.spawn(async move |_, _| {
            if let Err(error) = save.await {
                tracing::warn!(%error, "[Audio] could not save the input");
            }
        })
        .detach();
        cx.notify();
    }
}

fn kind_icon(kind: &str) -> KairoIcon {
    match kind {
        "Built-in" => KairoIcon::Laptop,
        "External" => KairoIcon::Usb,
        "Continuity" => KairoIcon::DeviceMobile,
        _ => KairoIcon::Waveform,
    }
}

impl Render for AudioRail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let listening = self.listening();
        if listening {
            // Notice when the levels stop.
            window.request_animation_frame();
        }
        let level = self.level.as_ref().map(|(l, _)| l);
        let rms = if listening {
            level.map_or(0.0, |l| l.rms.clamp(0.0, 1.0))
        } else {
            0.0
        };
        let clipping = listening && level.is_some_and(|l| l.clipping);
        let percent = meter_percent(level, listening);
        let saved = self.saved();
        let devices: Vec<_> = self
            .devices
            .as_ref()
            .map(|d| d.iter().map(|d| d.device.clone()).collect())
            .unwrap_or_default();
        let selected = if self.devices.is_some() {
            resolve_input(&devices, &saved)
        } else {
            saved
        };
        let active = self
            .devices
            .as_ref()
            .and_then(|d| d.iter().find(|d| d.device.id == selected))
            .map(|d| SharedString::from(d.device.label.clone()))
            .unwrap_or_else(|| "System default".into());
        let green = k.hue(Hue::Green);
        let header = v_flex()
            .px_4()
            .pt_4()
            .pb_3p5()
            .border_b_1()
            .border_color(k.white.opacity(0.06))
            .child(
                h_flex()
                    .gap_2p5()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size_9()
                            .rounded_full()
                            .when(listening, |d| d.bg(green).text_color(k.surface))
                            .when(!listening, |d| {
                                d.bg(k.white.opacity(0.08))
                                    .text_color(k.text(TextStep::T300))
                            })
                            .child(Icon::new(KairoIcon::Mic).size(px(16.))),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text(TextStep::T400))
                                    .child("AUDIO INPUT"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text_primary)
                                    .child(active),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .text_size(px(10.))
                            .font_weight(FontWeight::MEDIUM)
                            .when(listening, |d| d.bg(green).text_color(k.surface))
                            .when(!listening, |d| {
                                d.bg(k.white.opacity(0.08))
                                    .text_color(k.text(TextStep::T400))
                            })
                            .child(if listening { "Listening" } else { "Idle" }),
                    ),
            )
            .child(
                h_flex()
                    .id(ids::METER)
                    .test_support()
                    .role(Role::Meter)
                    .aria_label(SharedString::from(format!("Input level {percent}%")))
                    .mt_3p5()
                    .h(px(6.))
                    .gap(px(2.))
                    .children((0..SEGMENTS).map(|i| {
                        // Green, amber for the last seven, red for the last
                        // three or whenever the input clips.
                        let lit = rms >= (i + 1) as f64 / SEGMENTS as f64;
                        let tone = if i >= SEGMENTS - 3 || clipping {
                            k.danger_text
                        } else if i >= SEGMENTS - 7 {
                            k.hue(Hue::Orange)
                        } else {
                            green
                        };
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .rounded(px(2.))
                            .bg(if lit { tone } else { k.white.opacity(0.08) })
                    })),
            )
            .when(!listening, |d| {
                d.child(
                    div()
                        .mt_2()
                        .text_size(px(11.))
                        .text_color(k.text(TextStep::T500))
                        .child("The level shows while Kairo is listening."),
                )
            });
        let list = match &self.devices {
            None => div()
                .px_2()
                .py_3()
                .text_size(px(12.))
                .text_color(k.text(TextStep::T500))
                .child("Finding microphones…")
                .into_any_element(),
            Some(devices) if devices.is_empty() => div()
                .px_2()
                .py_3()
                .text_size(px(12.))
                .text_color(k.text(TextStep::T500))
                .child("No microphones found. Allow microphone access in System Settings.")
                .into_any_element(),
            Some(devices) => v_flex()
                .gap_0p5()
                .children(devices.iter().enumerate().map(|(i, d)| {
                    let chosen = d.device.id == selected;
                    let id = d.device.id.clone();
                    let label = SharedString::from(d.device.label.clone());
                    h_flex()
                        .id(SharedString::from(ids::input(i)))
                        .test_support()
                        .role(Role::ListBoxOption)
                        .aria_label(label.clone())
                        .aria_selected(chosen)
                        .w_full()
                        .gap_3()
                        .px_2()
                        .py_2()
                        .rounded(radius::lg())
                        .cursor_pointer()
                        .when(chosen, |r| r.bg(k.white.opacity(0.08)))
                        .when(!chosen, |r| r.hover(|s| s.bg(k.white.opacity(0.05))))
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(id.clone(), cx)))
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_center()
                                .size_8()
                                .rounded(radius::md())
                                .when(chosen, |d| {
                                    d.bg(k.accent(Step::S500)).text_color(k.on_accent)
                                })
                                .when(!chosen, |d| {
                                    d.bg(k.white.opacity(0.06))
                                        .text_color(k.text(TextStep::T300))
                                })
                                .child(Icon::new(kind_icon(&d.kind)).size(px(13.))),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .when(chosen, |d| {
                                            d.font_weight(FontWeight::MEDIUM)
                                                .text_color(k.text_primary)
                                        })
                                        .when(!chosen, |d| d.text_color(k.text(TextStep::T200)))
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(k.text(TextStep::T500))
                                        .child(d.kind.clone()),
                                ),
                        )
                        .when(chosen, |r| {
                            r.child(
                                Icon::new(KairoIcon::Check)
                                    .size(px(14.))
                                    .text_color(k.text_primary),
                            )
                        })
                }))
                .into_any_element(),
        };
        v_flex()
            .id(ids::PANEL)
            .test_support()
            .w_72()
            .child(header)
            .child(
                div()
                    .id("audio-rail-list")
                    .max_h(px(224.))
                    .overflow_y_scroll()
                    .p_2()
                    .role(Role::ListBox)
                    .aria_label("Audio input devices")
                    .child(list),
            )
            .child(
                div()
                    .p_2()
                    .border_t_1()
                    .border_color(k.white.opacity(0.06))
                    .child(
                        h_flex()
                            .id(ids::SETTINGS)
                            .test_support()
                            .role(Role::Button)
                            .aria_label("Audio settings")
                            .w_full()
                            .gap_3()
                            .px_2()
                            .py_2()
                            .rounded(radius::lg())
                            .cursor_pointer()
                            .hover(|s| s.bg(k.white.opacity(0.05)))
                            .text_size(px(12.))
                            .text_color(k.text(TextStep::T300))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenAudioSettings)))
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .size_8()
                                    .rounded(radius::md())
                                    .bg(k.white.opacity(0.06))
                                    .text_color(k.text(TextStep::T400))
                                    .child(Icon::new(KairoIcon::Settings).size(px(12.))),
                            )
                            .child(div().flex_1().child("Audio settings"))
                            .child(
                                Icon::new(KairoIcon::ChevronRight)
                                    .size(px(12.))
                                    .text_color(k.text(TextStep::T500)),
                            ),
                    ),
            )
    }
}
