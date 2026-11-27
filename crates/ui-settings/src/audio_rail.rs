//! The rail's Audio popover (`AudioRailButton` in `RailPopovers.tsx`): the
//! input in use with a level meter while Kairo listens, the inputs to pick
//! from, and a way into Audio settings. It reads and writes the same
//! `audio` settings section as Settings → Audio.

use crate::backend::{InputDevice, MeterEvent, MeterHandle, SettingsBackend};
use crate::model::{meter_fill, meter_percent, resolve_input};
use futures_util::StreamExt;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::{
    Context, EventEmitter, FontWeight, InteractiveElement, IntoElement, ParentElement, Render,
    Role, SharedString, StatefulInteractiveElement, Styled, Task, Window, div,
    prelude::FluentBuilder, px,
};
use kairo_domain::status::AudioLevel;
use kairo_ui_kit::tokens::{Hue, Step, TextStep, radius, text};
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
    /// While the popover is open and Kairo isn't listening, the selected
    /// input is metered on its own so the operator can see it is live.
    preview: Option<Preview>,
}

struct Preview {
    device: String,
    level: Option<AudioLevel>,
    error: Option<String>,
    _meter: Option<MeterHandle>,
    _task: Task<()>,
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
            preview: None,
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
                if !this.listening() {
                    this.start_preview(cx);
                }
                cx.notify();
            });
        }));
    }

    /// The previewed input's latest RMS, while the popover meters it.
    pub fn preview_level(&self) -> Option<f64> {
        self.preview.as_ref()?.level.as_ref().map(|l| l.rms)
    }

    /// The popover closed: stop metering on its behalf.
    pub fn closed(&mut self, cx: &mut Context<Self>) {
        if self.preview.take().is_some() {
            cx.notify();
        }
    }

    /// The input the capture uses: the saved one while present, else the default.
    fn selected(&self) -> String {
        let saved = self.saved();
        match &self.devices {
            Some(devices) => {
                let devices: Vec<_> = devices.iter().map(|d| d.device.clone()).collect();
                resolve_input(&devices, &saved)
            }
            None => saved,
        }
    }

    /// Meters the selected input while the popover is open.
    fn start_preview(&mut self, cx: &mut Context<Self>) {
        let device = self.selected();
        if self.preview.as_ref().is_some_and(|p| p.device == device) {
            return;
        }
        let request = self.backend.start_meter(device.clone());
        let task = cx.spawn(async move |this, cx| {
            let (handle, mut events) = match request.await {
                Ok(started) => started,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(p) = &mut this.preview {
                            p.error = Some(error);
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            let kept = this.update(cx, |this, _| match &mut this.preview {
                Some(p) => {
                    p._meter = Some(handle);
                    true
                }
                None => false,
            });
            if !matches!(kept, Ok(true)) {
                return;
            }
            while let Some(event) = events.next().await {
                let alive = this.update(cx, |this, cx| {
                    if let Some(p) = &mut this.preview {
                        match event {
                            MeterEvent::Level(level) => p.level = Some(level),
                            MeterEvent::Error(error) => p.error = Some(error),
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        self.preview = Some(Preview {
            device,
            level: None,
            error: None,
            _meter: None,
            _task: task,
        });
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
        if self.preview.is_some() {
            self.start_preview(cx);
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
        // Kairo's own capture while listening; else the popover's preview.
        let previewing = !listening && self.preview.is_some();
        let preview_error = self.preview.as_ref().and_then(|p| p.error.clone());
        let level = if listening {
            self.level.as_ref().map(|(l, _)| l)
        } else {
            self.preview.as_ref().and_then(|p| p.level.as_ref())
        };
        let metered = listening || previewing;
        let rms = if metered {
            level.map_or(0.0, |l| meter_fill(l.rms))
        } else {
            0.0
        };
        let clipping = metered && level.is_some_and(|l| l.clipping);
        let percent = meter_percent(level, metered);
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
        let active_kind = self
            .devices
            .as_ref()
            .and_then(|d| d.iter().find(|d| d.device.id == selected))
            .map(|d| d.kind.clone());
        let header = h_flex()
            .px_4()
            .pt_4()
            .pb_3()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_size(text::BASE.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child("Microphone"),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .text_size(text::MICRO.size)
                    .font_weight(FontWeight::MEDIUM)
                    .when(listening, |d| d.bg(green.opacity(0.16)).text_color(green))
                    .when(!listening, |d| {
                        d.bg(k.ink(0.06)).text_color(k.text(TextStep::T400))
                    })
                    .child(div().size_1p5().rounded_full().bg(if listening {
                        green
                    } else {
                        k.text(TextStep::T500)
                    }))
                    .child(if listening { "Listening" } else { "Idle" }),
            );
        // The input in use, and how loud it is.
        let current = v_flex()
            .mx_3()
            .p_3()
            .rounded(radius::lg())
            .bg(k.ink(0.04))
            .border_1()
            .border_color(k.ink(0.08))
            .child(
                h_flex()
                    .gap_2p5()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size_8()
                            .rounded(radius::md())
                            .when(listening, |d| d.bg(green).text_color(k.surface))
                            .when(!listening, |d| {
                                d.bg(k.ink(0.08)).text_color(k.text(TextStep::T300))
                            })
                            .child(Icon::new(KairoIcon::Mic).size_4()),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text_primary)
                                    .child(active),
                            )
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(match active_kind {
                                        Some(kind) => format!("In use · {kind}"),
                                        None => "In use".to_owned(),
                                    }),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .id(ids::METER)
                    .test_support()
                    .role(Role::Meter)
                    .aria_label(SharedString::from(format!("Input level {percent}%")))
                    .mt_3()
                    .h_1p5()
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
                        div().flex_1().min_w_0().h_full().rounded_full().bg(if lit {
                            tone
                        } else {
                            k.ink(0.08)
                        })
                    })),
            )
            .when(!listening, |d| {
                let (note, warn) = match (&preview_error, previewing) {
                    (Some(error), _) => (error.clone(), true),
                    (None, true) => (
                        "Not listening yet. This is the input as Kairo hears it.".to_owned(),
                        false,
                    ),
                    (None, false) => (
                        "The level shows while Kairo is listening.".to_owned(),
                        false,
                    ),
                };
                d.child(
                    div()
                        .mt_2()
                        .text_size(text::LABEL.size)
                        .line_height(text::LABEL.line_height)
                        .text_color(if warn {
                            k.warning_text
                        } else {
                            k.text(TextStep::T500)
                        })
                        .child(note),
                )
            });
        let quiet = |message: &'static str| {
            div()
                .px_2p5()
                .py_3()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T500))
                .child(message)
                .into_any_element()
        };
        let list = match &self.devices {
            None => quiet("Finding microphones…"),
            Some(devices) if devices.is_empty() => {
                quiet("No microphones found. Allow microphone access in System Settings.")
            }
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
                        .gap_2p5()
                        .px_2p5()
                        .py_1p5()
                        .rounded(radius::md())
                        .cursor_pointer()
                        .hover(|s| s.bg(k.ink(0.06)))
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(id.clone(), cx)))
                        .child(
                            Icon::new(kind_icon(&d.kind))
                                .size_4()
                                .flex_none()
                                .text_color(if chosen {
                                    k.text_primary
                                } else {
                                    k.text(TextStep::T400)
                                }),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(text::SM.size)
                                        .text_color(if chosen {
                                            k.text_primary
                                        } else {
                                            k.text(TextStep::T200)
                                        })
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T500))
                                        .child(d.kind.clone()),
                                ),
                        )
                        .when(chosen, |r| {
                            r.child(
                                Icon::new(KairoIcon::Check)
                                    .size_4()
                                    .text_color(k.accent(Step::S500)),
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
            .child(current)
            .child(
                div()
                    .px_4()
                    .pt_3p5()
                    .pb_1()
                    .text_size(text::LABEL.size)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(k.text(TextStep::T500))
                    .child("Inputs"),
            )
            .child(
                div()
                    .id("audio-rail-list")
                    .max_h(px(224.))
                    .overflow_y_scroll()
                    .px_1p5()
                    .role(Role::ListBox)
                    .aria_label("Audio input devices")
                    .child(list),
            )
            .child(div().mt_1p5().mx_4().h(px(1.)).bg(k.panel_border))
            .child(
                div().p_1p5().child(
                    h_flex()
                        .id(ids::SETTINGS)
                        .test_support()
                        .role(Role::Button)
                        .aria_label("Audio settings")
                        .w_full()
                        .gap_2p5()
                        .px_2p5()
                        .py_2()
                        .rounded(radius::md())
                        .cursor_pointer()
                        .hover(|s| s.bg(k.ink(0.06)))
                        .text_size(text::SM.size)
                        .text_color(k.text_primary)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenAudioSettings)))
                        .child(
                            Icon::new(KairoIcon::Settings)
                                .size_4()
                                .flex_none()
                                .text_color(k.text(TextStep::T300)),
                        )
                        .child(div().flex_1().child("Audio settings"))
                        .child(
                            Icon::new(KairoIcon::ChevronRight)
                                .size_3p5()
                                .text_color(k.text(TextStep::T500)),
                        ),
                ),
            )
    }
}
