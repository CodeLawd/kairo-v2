//! Rendering for [`Shell`].

use crate::chrome::{Tone, ToolbarButton, hued};
use crate::crash::panic_message;
use crate::keymap::{
    self, About, ActualSize, BringAllToFront, CheckForUpdates, CloseWindow, Hide, HideOthers,
    Import, Minimize, OpenSettings, Quit, ShowAll, SongSearch, ToggleFullScreen, ToggleScreens,
    WhatsNew, ZoomIn, ZoomOut, ZoomWindow,
};
use crate::route::{Route, WORKSPACES};
use crate::shell::{FeedbackPhase, Overlay, Shell};
use crate::state::{FeedbackKind, ScreensLevel, ShellRequest, UpdateState, screens_status};
use crate::whats_new::{self, CHANGELOG_URL};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::FocusTrapElement;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    Role, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
    px,
};
use kairo_ui_kit::brand::{KairoMark, MarkSize};
use kairo_ui_kit::components::PopoverTriggerExt;
use kairo_ui_kit::components::{Frost, FrostedPanel, RailItem};
use kairo_ui_kit::tokens::{Hue, TextStep, metrics, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use std::time::Instant;

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending_feedback_clear {
            self.pending_feedback_clear = false;
            self.feedback_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.set_placeholder(crate::shell::placeholder(FeedbackKind::Idea), window, cx);
            });
        }
        let k = *cx.kairo();
        let launching = !self.launch.ready();
        let (opacity, fading) = if launching {
            (1.0, false)
        } else {
            self.opening_opacity(cx)
        };
        if fading {
            window.request_animation_frame();
        }
        let whats_new_open = !launching && self.whats_new.is_some();
        self.whats_new_focus.sync(whats_new_open, window, cx);
        if self.quit_focus.sync(self.quit_open, window, cx) {
            self.keep_open_focus.focus(window, cx);
        }
        let mut content: Vec<AnyElement> = Vec::new();
        if launching {
            content.push(self.render_launch(window, cx));
        } else {
            let header = self.render_header(window, cx);
            let rail = self.render_rail(cx);
            let workspace = self.render_workspace(cx);
            content.push(header);
            content.push(
                h_flex()
                    .items_stretch()
                    .min_h_0()
                    .w_full()
                    .flex_1()
                    .overflow_hidden()
                    .child(rail)
                    .child(workspace)
                    .into_any_element(),
            );
            content.extend(self.render_startup_warning(cx));
            content.extend(self.render_overlay(window, cx));
            content.extend(self.render_notices(cx));
            content.extend(self.transfer.clone().map(|t| t.into_any_element()));
            content.extend(self.render_whats_new(cx));
            content.extend(self.front.clone().map(|front| front.into_any_element()));
        }
        content.extend(self.render_quit(cx));
        div()
            .id("kairo-shell")
            .key_context(keymap::CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(k.surface)
            .opacity(opacity)
            .text_color(k.text_primary)
            .font_family(text::FAMILY)
            .text_size(text::BASE.size)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(|this, _: &Quit, window, cx| {
                if this.request_quit(window, cx) {
                    cx.quit();
                }
            }))
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                // Closing the operator window quits, after the same question.
                if this.request_quit(window, cx) {
                    cx.quit();
                }
            }))
            .on_action(|_: &Hide, _, cx| cx.hide())
            .on_action(|_: &HideOthers, _, cx| cx.hide_other_apps())
            .on_action(|_: &ShowAll, _, cx| cx.unhide_other_apps())
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &ZoomWindow, window, _| window.zoom_window())
            .on_action(|_: &BringAllToFront, window, _| window.activate_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(
                cx.listener(|this, _: &ZoomIn, _, cx| this.set_scale(this.scale.zoom_in(), cx)),
            )
            .on_action(
                cx.listener(|this, _: &ZoomOut, _, cx| this.set_scale(this.scale.zoom_out(), cx)),
            )
            .on_action(
                cx.listener(|this, _: &ActualSize, _, cx| this.set_scale(Default::default(), cx)),
            )
            .on_action(cx.listener(|this, _: &WhatsNew, _, cx| this.open_whats_new(cx)))
            .on_action(cx.listener(|this, _: &About, _, cx| this.open_whats_new(cx)))
            .on_action(
                cx.listener(|_, _: &CheckForUpdates, _, cx| cx.emit(ShellRequest::CheckForUpdates)),
            )
            .on_action(
                cx.listener(|this, _: &SongSearch, _, cx| {
                    this.open_overlay(Overlay::SongSearch, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &OpenSettings, _, cx| {
                    this.open_overlay(Overlay::Settings, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleScreens, _, cx| {
                if this.overlay == Some(Overlay::Screens) {
                    this.close_overlay(cx);
                } else {
                    this.open_overlay(Overlay::Screens, cx);
                }
            }))
            .on_action(cx.listener(|this, action: &Import, window, cx| {
                let kind = action.0.0;
                if this.ready {
                    this.deliver_import(kind, window, cx);
                } else {
                    this.pending_import = Some(kind);
                }
            }))
            .children(content)
    }
}

/// Element ids for the fixed parts, for tests.
pub mod ids {
    /// The header.
    pub const HEADER: &str = "app-header";
    /// The rail.
    pub const RAIL: &str = "workspace-rail";
    /// The workspace.
    pub const WORKSPACE: &str = "app-workspace";
}

impl Shell {
    fn render_header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let full_screen = window.is_fullscreen();
        // The Mac window buttons sit in the corner outside full screen; elsewhere the logo.
        let show_logo = !self.mac || full_screen;
        let (screens_detail, level) = match &self.state.outputs {
            Some(outputs) => screens_status(outputs),
            None => ("checking".to_owned(), ScreensLevel::Offline),
        };
        let dot = match level {
            ScreensLevel::Ready => k.success_text,
            ScreensLevel::Warning => k.hue(Hue::Amber),
            ScreensLevel::Offline => k.text(TextStep::T500),
        };
        let (_, live_detail) = self.state.live_text();
        let live = self.state.is_live() || self.state.recording;
        let clearing = false;
        let live_rail_width = cx.try_global::<kairo_ui_kit::LiveRailWidth>().map(|w| w.0);

        h_flex()
            .id(ids::HEADER)
            .test_support()
            .role(Role::Toolbar)
            .aria_label("Presentation controls")
            .flex_shrink_0()
            .h(metrics::header_height())
            .relative()
            .bg(k.surface_header)
            .border_b(metrics::panel_border_width())
            .border_color(k.panel_border)
            // The empty header moves the window (macOS hidden title bar) and
            // a double-click zooms it, as the system title bar would. A layer
            // behind the controls, so pressing a control never drags.
            .when(self.mac, |this| {
                this.child(div().id("header-drag").absolute().inset_0().on_mouse_down(
                    MouseButton::Left,
                    |event, window, _| {
                        if event.click_count == 2 {
                            window.titlebar_double_click();
                        } else {
                            window.start_window_move();
                        }
                    },
                ))
            })
            // Aligned with the left workspace rail's right border.
            .child(
                h_flex()
                    .w(metrics::rail_width())
                    .h_full()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .border_r(metrics::panel_border_width())
                    .border_color(k.panel_border)
                    .when(show_logo, |this| {
                        this.child(KairoMark::new(MarkSize::Small))
                    }),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .pl_3()
                    .pr_2p5()
                    .gap_3()
                    .child(
                        ToolbarButton::new(
                            "search-lyrics",
                            "Search",
                            "Search lyrics, slides, libraries and playlists",
                            hued(KairoIcon::Search, Hue::Violet, cx),
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| {
                                this.open_overlay(Overlay::SongSearch, cx)
                            }),
                        ),
                    )
                    // The page's own toolbar (the Operator's, prompt 076) sits here.
                    .child(
                        div()
                            .id("header-toolbar")
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden(),
                    )
                    .child(
                        h_flex()
                            .relative()
                            .flex_shrink_0()
                            .items_center()
                            .gap_1()
                            .child(
                                ToolbarButton::new(
                                    "media",
                                    "Media",
                                    format!(
                                        "{} media{}",
                                        if self.media_open { "Close" } else { "Open" },
                                        self.media_live
                                            .as_ref()
                                            .map(|n| format!(" — {n} on screen"))
                                            .unwrap_or_default()
                                    ),
                                    Icon::new(KairoIcon::MediaLibrary.weight(self.media_open))
                                        .size(px(18.))
                                        .text_color(k.hue(Hue::Orange)),
                                )
                                .when(self.media_live.is_some(), |b| b.badge(k.live))
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(ShellRequest::ToggleMedia)),
                                ),
                            )
                            .child(self.render_screen_themes(cx))
                            .child(
                                ToolbarButton::new(
                                    "screens",
                                    "Screens",
                                    format!("Screens: {screens_detail}. Open Screens (⌥⌘1)"),
                                    Icon::new(KairoIcon::MonitorPlay.fill())
                                        .size(px(18.))
                                        .text_color(k.hue(Hue::Cyan)),
                                )
                                .badge(dot)
                                .coach_anchor(kairo_ui_kit::coach::anchors::HEADER_SCREENS)
                                .on_click(cx.listener(
                                    |this, _, _, cx| this.open_overlay(Overlay::Screens, cx),
                                )),
                            )
                            .child(kairo_ui_kit::coach::coach_anchor(
                                kairo_ui_kit::coach::anchors::HEADER_LIVE,
                            ))
                            .when_some(self.state.house_track.clone(), |this, track| {
                                let paused = track.paused;
                                this.child(
                                    ToolbarButton::new(
                                        "house-track",
                                        track.name.clone(),
                                        format!(
                                            "{} {}",
                                            if paused { "Play" } else { "Pause" },
                                            track.name
                                        ),
                                        Icon::new(if paused {
                                            KairoIcon::Play.fill()
                                        } else {
                                            KairoIcon::Pause.fill()
                                        })
                                        .size_4(),
                                    )
                                    .on_click(cx.listener(
                                        move |_, _, _, cx| {
                                            cx.emit(ShellRequest::SetHouseTrackPaused(!paused))
                                        },
                                    )),
                                )
                            })
                            .child(self.render_live(live_detail, live, cx)),
                    ),
            )
            // Aligned with the right-hand Live Rail's left border.
            .child(
                h_flex()
                    .flex_shrink_0()
                    .h_full()
                    .items_center()
                    .px_3()
                    .gap_1p5()
                    .border_l(metrics::panel_border_width())
                    .border_color(k.panel_border)
                    .when_some(live_rail_width, |d, w| d.w(px(w)))
                    .child(self.render_screen_toggle(
                        "audience-screen",
                        "Audience",
                        false,
                        self.state.audience_on,
                        cx,
                    ))
                    .child(self.render_screen_toggle(
                        "stage-screen",
                        "Stage",
                        true,
                        self.state.stage_on,
                        cx,
                    ))
                    .when(live_rail_width.is_some(), |d| d.child(div().flex_1()))
                    .child(
                        ToolbarButton::new(
                            "clear",
                            if clearing { "Clearing" } else { "Clear" },
                            "Clear text and the dock background",
                            Icon::new(KairoIcon::Eraser.fill())
                                .size(px(18.))
                                .text_color(k.hue(Hue::Pink)),
                        )
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::ClearLive))),
                    )
                    .children(self.render_activation(cx))
                    .children(
                        self.render_update_pill(cx)
                            .map(|pill| div().occlude().child(pill)),
                    ),
            )
            .into_any_element()
    }

    fn render_screen_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        stage: bool,
        state: Option<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let (ring_color, title) = match state {
            Some(true) => (
                k.success_text,
                format!("{label} screens are on — click to turn off"),
            ),
            Some(false) => (
                k.live,
                format!("{label} screens are off — click to turn on"),
            ),
            None => (
                k.text(TextStep::T500),
                format!("No {label} screens set up — click to add one"),
            ),
        };
        let dot = div().size(px(14.)).rounded_full().bg(ring_color);
        ToolbarButton::new(id, label, title, dot)
            .on_click(cx.listener(move |this, _, _, cx| match state {
                Some(on) => cx.emit(ShellRequest::SetScreenGroupEnabled {
                    stage,
                    enabled: !on,
                }),
                None => this.open_overlay(Overlay::Screens, cx),
            }))
            .into_any_element()
    }

    fn render_activation(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Only once signed in and the licence says not activated.
        if !self.state.account.signed_in || self.state.activated != Some(false) {
            return None;
        }
        Some(
            ToolbarButton::new(
                "activate",
                "Activate",
                "This computer isn't activated — live transcription, scripture lookup and lyric tools are off. Click to activate.",
                Icon::new(KairoIcon::Key).size_3p5(),
            )
            .on_click(cx.listener(|this, _, _, cx| this.open_overlay(Overlay::Settings, cx)))
            .into_any_element(),
        )
    }

    fn render_update_pill(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let solid = ButtonCustomVariant::new(cx)
            .color(k.live)
            .foreground(k.white)
            .hover(k.live.opacity(0.85))
            .active(k.live.opacity(0.85));
        match self.state.update.clone() {
            UpdateState::Idle | UpdateState::Checking | UpdateState::Error => None,
            UpdateState::Downloading { percent } => Some(
                h_flex()
                    .id("update-pill")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(match percent {
                        Some(p) => format!("Downloading update, {p} percent"),
                        None => "Downloading update".to_owned(),
                    })
                    .h_7()
                    .gap_1p5()
                    .px_2p5()
                    .rounded(radius::sm())
                    .bg(k.live.opacity(0.15))
                    .text_color(k.live)
                    .text_size(text::LABEL.size)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(Icon::new(KairoIcon::Download).size_3())
                    .child(percent.map_or("Updating".to_owned(), |p| format!("{p}%")))
                    .into_any_element(),
            ),
            UpdateState::Available { version } => Some(
                Button::new("update-pill")
                    .cursor_pointer()
                    .custom(solid)
                    // Solid fill: the custom variant paints its colour at 20%.
                    .bg(k.live)
                    .icon(Icon::new(KairoIcon::Download).size_3())
                    .label("Update")
                    .small()
                    .tooltip(format!(
                        "Kairo {version} is available — download it now, install whenever you like"
                    ))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::DownloadUpdate)))
                    .into_any_element(),
            ),
            UpdateState::Downloaded { version } => {
                let label = if self.update_confirming {
                    if self.state.is_live() {
                        "Restart? Service is live".to_owned()
                    } else {
                        "Restart now?".to_owned()
                    }
                } else {
                    format!("Update {version}")
                };
                let tooltip = if self.update_confirming {
                    "Click again to quit and install now".to_owned()
                } else {
                    format!(
                        "Kairo {version} is ready. It installs on the next quit — click to restart now."
                    )
                };
                Some(
                    Button::new("update-pill")
                        .cursor_pointer()
                        .custom(solid)
                        // Solid fill: the custom variant paints its colour at 20%.
                        .bg(k.live)
                        .icon(Icon::new(KairoIcon::Download).size_3())
                        .label(label)
                        .small()
                        .tooltip(tooltip)
                        .on_click(cx.listener(|this, _, _, cx| {
                            // Installing quits, so it always takes two clicks.
                            if this.update_confirming {
                                this.update_confirming = false;
                                this.allow_quit();
                                cx.emit(ShellRequest::InstallUpdate);
                            } else {
                                this.update_confirming = true;
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                )
            }
        }
    }

    fn rail_trigger(
        &self,
        id: &'static str,
        icon: KairoIcon,
        label: &'static str,
        hue: Option<Hue>,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let k = *cx.kairo();
        let active_fg = hue.map_or(k.text_primary, |h| k.hue(h));
        let variant = ButtonCustomVariant::new(cx)
            .color(if open {
                k.surface_elevated
            } else {
                k.surface.opacity(0.0)
            })
            .foreground(if open {
                active_fg
            } else {
                k.text(TextStep::T500)
            })
            .hover(k.surface_tertiary)
            .active(k.surface_elevated);
        Button::new(id)
            .cursor_pointer()
            .custom(variant)
            // Solid fill: the custom variant paints its colour at 20%.
            .when(open, |b| b.bg(k.surface_elevated))
            .w_full()
            .h_auto()
            .py_1()
            .rounded(radius::lg())
            .accessibility_label(label)
            .child(
                v_flex()
                    .items_center()
                    .gap(gpui_kit::px(3.))
                    .child(Icon::new(icon.weight(open)).size_5())
                    .child(
                        div()
                            .text_size(text::MICRO.size)
                            .line_height(text::MICRO.line_height)
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(label),
                    ),
            )
    }

    fn render_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let route = self.route;
        let feedback = self.render_feedback(cx);
        let account = self.render_account(cx);
        let audio = Popover::new("audio-popover")
            .anchor(gpui_kit::Anchor::LeftCenter)
            .offset(px(8.))
            .appearance(false)
            .trigger(
                self.rail_trigger(
                    "audio",
                    KairoIcon::Mic,
                    "Audio",
                    Some(Hue::Green),
                    false,
                    cx,
                )
                .popover_trigger(),
            )
            .when_some(self.audio.clone(), |popover, (view, opened)| {
                popover
                    .on_open_change(move |open, window, cx| opened(*open, window, cx))
                    .content(move |_, _, cx| {
                        let k = *cx.kairo();
                        div()
                            .rounded(radius::xl())
                            .overflow_hidden()
                            .bg(k.surface_secondary)
                            .border_1()
                            .border_color(k.ink(0.14))
                            .shadow_2xl()
                            .child(view.clone())
                    })
            });
        v_flex()
            .id(ids::RAIL)
            .test_support()
            .role(Role::Navigation)
            .aria_label("Workspaces")
            .flex_shrink_0()
            .w(metrics::rail_width())
            .h_full()
            .items_center()
            .bg(k.surface_rail)
            .child(
                v_flex()
                    .id("rail-workspaces")
                    .relative()
                    .child(kairo_ui_kit::coach::coach_anchor(
                        kairo_ui_kit::coach::anchors::WORKSPACES,
                    ))
                    .min_h_0()
                    .w_full()
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .relative()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .px_1()
                            .py_2()
                            .children(WORKSPACES.iter().map(|w| {
                                let target = w.route;
                                let item = RailItem::new(
                                    SharedString::from(format!("rail-{}", w.route.as_str())),
                                    w.icon,
                                    w.rail_label(),
                                    w.hue,
                                )
                                .active(route == w.route)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.navigate(target, cx)),
                                );
                                match w.tooltip() {
                                    Some(tip) => div()
                                        .id(SharedString::from(format!(
                                            "rail-tip-{}",
                                            w.route.as_str()
                                        )))
                                        .w_full()
                                        .tooltip(move |window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                                .build(window, cx)
                                        })
                                        .child(item)
                                        .into_any_element(),
                                    None => item.into_any_element(),
                                }
                            })),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .pb_1()
                    .child(feedback)
                    .child(audio)
                    .child(account),
            )
            .child(
                div().w_full().flex_shrink_0().px_1().pb_3().child(
                    self.rail_trigger("settings", KairoIcon::Settings, "Settings", None, false, cx)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.open_overlay(Overlay::Settings, cx)),
                        ),
                ),
            )
            .into_any_element()
    }

    fn render_account(&self, cx: &mut Context<Self>) -> AnyElement {
        let account = self.state.account.clone();
        let entity = cx.entity();
        Popover::new("account-popover")
            .anchor(gpui_kit::Anchor::LeftCenter)
            .offset(px(8.))
            .appearance(false)
            .trigger(
                self.rail_trigger(
                    "account",
                    KairoIcon::CircleUser,
                    "Account",
                    Some(Hue::Blue),
                    false,
                    cx,
                )
                .popover_trigger(),
            )
            .content(move |_, _, cx| {
                // `AccountRailButton`: who is signed in on a toned band, the
                // connection at a glance, then the actions.
                let k = *cx.kairo();
                let (settings, tips, out) = (entity.clone(), entity.clone(), entity.clone());
                let signed_out = !account.signed_in;
                let (status, dot) = if signed_out {
                    ("Signed out", k.neutral(kairo_ui_kit::tokens::Step::S600))
                } else if account.offline {
                    ("Offline", k.hue(Hue::Orange))
                } else {
                    ("Online", k.hue(Hue::Green))
                };
                let synced = if account.offline {
                    "Waiting".to_owned()
                } else {
                    account.last_synced_ms.map_or("—".to_owned(), synced_label)
                };
                let title = account
                    .name
                    .clone()
                    .or(account.email.clone())
                    .unwrap_or_else(|| "Not signed in".into());
                let initial: String = title
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().collect())
                    .unwrap_or_default();
                let chip = |content: AnyElement| {
                    h_flex()
                        .gap_1p5()
                        .max_w_full()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .border_1()
                        .border_color(k.panel_border)
                        .bg(k.surface)
                        .text_size(gpui_kit::px(11.))
                        .child(content)
                };
                let tile = |label: &'static str, value: AnyElement| {
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .px_2p5()
                        .py_2()
                        .rounded(radius::md())
                        .border_1()
                        .border_color(k.panel_border)
                        .bg(k.surface)
                        .child(
                            div()
                                .text_size(gpui_kit::px(10.))
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .text_color(k.text(TextStep::T500))
                                .child(label.to_uppercase()),
                        )
                        .child(
                            div()
                                .mt_0p5()
                                .text_size(text::LABEL.size)
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .text_color(k.text_primary)
                                .child(value),
                        )
                };
                let row = |id: &'static str,
                           icon: KairoIcon,
                           label: &'static str,
                           danger: bool,
                           click: Box<dyn Fn(&mut App)>| {
                    h_flex()
                        .id(id)
                        .test_support()
                        .role(Role::Button)
                        .aria_label(label)
                        .w_full()
                        .gap_2p5()
                        .px_2p5()
                        .py_2()
                        .rounded(radius::md())
                        .cursor_pointer()
                        .hover(|s| s.bg(k.ink(0.06)))
                        .text_size(text::XS.size)
                        .text_color(if danger {
                            k.danger_text
                        } else {
                            k.text(TextStep::T200)
                        })
                        .on_click(move |_, _, cx| click(cx))
                        .child(
                            Icon::new(icon)
                                .size_3p5()
                                .flex_none()
                                .text_color(if danger {
                                    k.danger_text
                                } else {
                                    k.text(TextStep::T400)
                                }),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(label))
                        .when(!danger, |r| {
                            r.child(
                                Icon::new(KairoIcon::ChevronRight)
                                    .size_3()
                                    .text_color(k.text(TextStep::T500)),
                            )
                        })
                };
                v_flex()
                    .w(px(276.))
                    .rounded(radius::xl())
                    .overflow_hidden()
                    .bg(k.surface_secondary)
                    .border_1()
                    .border_color(k.ink(0.14))
                    .shadow_2xl()
                    .child(
                        v_flex()
                            .p_3p5()
                            .gap_2p5()
                            .bg(k.surface_header)
                            .border_b_1()
                            .border_color(k.panel_border)
                            .child(
                                h_flex()
                                    .gap_2p5()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .size_9()
                                            .flex_none()
                                            .rounded_full()
                                            .bg(k.hue(Hue::Blue).opacity(0.22))
                                            .border_1()
                                            .border_color(k.hue(Hue::Blue).opacity(0.4))
                                            .text_color(k.text_primary)
                                            .text_size(text::SM.size)
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .child(initial),
                                    )
                                    .child(
                                        v_flex()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(text::SM.size)
                                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                    .text_color(k.text_primary)
                                                    .child(title),
                                            )
                                            .when_some(
                                                account
                                                    .email
                                                    .clone()
                                                    .filter(|_| account.name.is_some()),
                                                |this, email| {
                                                    this.child(
                                                        div()
                                                            .truncate()
                                                            .text_size(text::LABEL.size)
                                                            .text_color(k.text(TextStep::T400))
                                                            .child(email),
                                                    )
                                                },
                                            ),
                                    ),
                            )
                            .when_some(account.org.clone(), |this, (org, role)| {
                                this.child(
                                    h_flex()
                                        .flex_wrap()
                                        .gap_1p5()
                                        .child(chip(
                                            h_flex()
                                                .gap_1p5()
                                                .min_w_0()
                                                .text_color(k.text(TextStep::T200))
                                                .child(
                                                    Icon::new(KairoIcon::Church)
                                                        .size(gpui_kit::px(11.)),
                                                )
                                                .child(div().truncate().child(org))
                                                .into_any_element(),
                                        ))
                                        .child(chip(
                                            div()
                                                .text_color(k.text(TextStep::T400))
                                                .child(role)
                                                .into_any_element(),
                                        )),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .px_3()
                            .pt_3()
                            .child(tile(
                                "Status",
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(div().size(gpui_kit::px(6.)).rounded_full().bg(dot))
                                    .child(status)
                                    .into_any_element(),
                            ))
                            .child(tile(
                                "Last synced",
                                div().truncate().child(synced).into_any_element(),
                            )),
                    )
                    .child(
                        v_flex()
                            .p_2()
                            .gap_0p5()
                            .child(row(
                                "account-settings",
                                KairoIcon::Settings,
                                "Account settings",
                                false,
                                Box::new(move |cx| {
                                    settings
                                        .update(cx, |this, cx| this.open_settings("account", cx))
                                }),
                            ))
                            .child(row(
                                "account-tips",
                                KairoIcon::Lightbulb,
                                "Show tips again",
                                false,
                                Box::new(move |cx| {
                                    tips.update(cx, |_, cx| cx.emit(ShellRequest::ShowTips))
                                }),
                            ))
                            .when(!signed_out, |this| {
                                this.child(
                                    div()
                                        .mt_1()
                                        .pt_1()
                                        .border_t_1()
                                        .border_color(k.panel_border)
                                        .child(row(
                                            "account-sign-out",
                                            KairoIcon::LogOut,
                                            "Sign out",
                                            true,
                                            Box::new(move |cx| {
                                                out.update(cx, |_, cx| {
                                                    cx.emit(ShellRequest::SignOut)
                                                })
                                            }),
                                        )),
                                )
                            }),
                    )
            })
            .into_any_element()
    }

    /// The header's Live button: what is on screen, the recording clock,
    /// and its popover with capture and Syphon (`LiveMenu`).
    fn render_live(
        &mut self,
        detail: SharedString,
        live: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let recording = self.state.recording;
        let elapsed = recording.then(|| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.);
            kairo_domain::capture::format_capture_elapsed(
                self.state
                    .recording_started_at
                    .map_or(0., |at| (now - at).max(0.)),
            )
        });
        // Keep the header button compact so long live titles never push adjacent icons;
        // the full live title is shown in the hover tooltip.
        let base = if !live && self.state.transcribing {
            "Listening"
        } else {
            "Live"
        };
        let text: SharedString = match &elapsed {
            Some(e) => format!("{base} · {e}").into(),
            None => base.into(),
        };
        let title = if live {
            format!(
                "Live: {detail}{}",
                elapsed
                    .as_ref()
                    .map(|e| format!(" · recording {e}"))
                    .unwrap_or_default()
            )
        } else {
            format!(
                "{detail}{}. Click for live outputs and capture",
                elapsed
                    .as_ref()
                    .map(|e| format!(" · recording {e}"))
                    .unwrap_or_default()
            )
        };
        // Recording shows as a red dot on the icon, as on a camera.
        let icon = div()
            .relative()
            .child(Icon::new(KairoIcon::PhBroadcast.fill()).size(px(18.)))
            .when(recording, |this| {
                this.child(
                    div()
                        .absolute()
                        .right(gpui_kit::px(-4.))
                        .top(gpui_kit::px(-2.))
                        .size_2()
                        .rounded_full()
                        .bg(k.live)
                        .border_2()
                        .border_color(k.surface_tertiary),
                )
            });
        let button = ToolbarButton::new("live", text, title, icon).tone(if live || recording {
            Tone::Live
        } else {
            Tone::Muted
        });
        let Some(view) = self.live_menu.clone() else {
            return button.into_any_element();
        };
        let entity = cx.entity();
        let toggle = entity.clone();
        Popover::new("live-popover")
            .anchor(gpui_kit::Anchor::TopRight)
            .open(self.live_open)
            .on_open_change(move |next, _, cx| {
                let next = *next;
                entity.update(cx, |this, cx| this.set_live_open(next, cx))
            })
            .trigger(button.opens_popover(move |_, cx| {
                toggle.update(cx, |this, cx| this.set_live_open(!this.live_open, cx))
            }))
            .content(move |_, _, _| div().w_64().p_1().child(view.clone()))
            .into_any_element()
    }

    /// The header's Screen Themes button and its popover.
    fn render_screen_themes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let button = ToolbarButton::new(
            "screen-themes",
            "Screen Themes",
            "Choose themes for your screens",
            Icon::new(KairoIcon::PhPalette.fill())
                .size(px(18.))
                .text_color(k.hue(Hue::Pink)),
        );
        let Some((view, _)) = self.screen_themes.clone() else {
            return button.disabled(true).into_any_element();
        };
        let entity = cx.entity();
        let toggle = entity.clone();
        Popover::new("screen-themes-popover")
            .anchor(gpui_kit::Anchor::TopRight)
            .open(self.screen_themes_open)
            .on_open_change(move |next, window, cx| {
                let next = *next;
                entity.update(cx, |this, cx| this.set_screen_themes_open(next, window, cx))
            })
            .trigger(button.opens_popover(move |window, cx| {
                toggle.update(cx, |this, cx| {
                    this.set_screen_themes_open(!this.screen_themes_open, window, cx)
                })
            }))
            .content(move |_, _, _| div().p_3().child(view.clone()))
            .into_any_element()
    }

    fn render_feedback(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let open = self.feedback_open;
        Popover::new("feedback-popover")
            .anchor(gpui_kit::Anchor::LeftCenter)
            .offset(px(8.))
            .appearance(false)
            .open(open)
            .on_open_change({
                let entity = entity.clone();
                move |next, _, cx| {
                    entity.update(cx, |this, cx| {
                        if *next {
                            this.feedback_open = true;
                            cx.notify();
                        } else {
                            this.close_feedback(cx);
                        }
                    })
                }
            })
            .trigger(
                self.rail_trigger(
                    "feedback",
                    KairoIcon::PhChatCircleDots,
                    "Feedback",
                    Some(Hue::Coral),
                    open,
                    cx,
                )
                .popover_trigger(),
            )
            .content(move |_, window, cx| {
                let entity = entity.clone();
                entity.update(cx, |this, cx| this.feedback_content(window, cx))
            })
            .into_any_element()
    }

    fn feedback_content(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let account = &self.state.account;
        if self.feedback_phase == FeedbackPhase::Sent {
            let reply = account
                .email
                .as_ref()
                .map(|e| format!(" and may reply to {e}"))
                .unwrap_or_default();
            return v_flex()
                .id("feedback-sent")
                .test_support()
                .role(Role::Status)
                .aria_label("Thank you")
                .w(px(360.))
                .rounded(radius::xl())
                .overflow_hidden()
                .bg(k.surface_secondary)
                .border_1()
                .border_color(k.ink(0.14))
                .shadow_2xl()
                .items_center()
                .gap_2()
                .px_6()
                .py_8()
                .child(
                    Icon::new(KairoIcon::PhCheckCircle.fill())
                        .size_9()
                        .text_color(k.success_text),
                )
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(k.text_primary)
                        .child("Thank you"),
                )
                .child(
                    div()
                        .text_center()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T400))
                        .child(format!(
                            "It's with the Kairo team. We read every note{reply}."
                        )),
                )
                .into_any_element();
        }
        let signed_in = account.signed_in;
        let sending = self.feedback_phase == FeedbackPhase::Sending;
        let has_text = !self.feedback_input.read(cx).value().trim().is_empty();
        let sender = if signed_in {
            format!(
                "Sent as {}",
                account
                    .name
                    .clone()
                    .or(account.email.clone())
                    .unwrap_or_default()
            )
        } else {
            "Sign in to send feedback".to_owned()
        };
        let kinds = [
            (FeedbackKind::Idea, "Idea", KairoIcon::PhLightbulb),
            (FeedbackKind::Problem, "Problem", KairoIcon::PhWarningCircle),
            (FeedbackKind::Praise, "Praise", KairoIcon::PhHeart),
        ];
        v_flex()
            .id("feedback-form")
            .key_context("FeedbackForm")
            .w(px(360.))
            .rounded(radius::xl())
            .overflow_hidden()
            .bg(k.surface_secondary)
            .border_1()
            .border_color(k.ink(0.14))
            .shadow_2xl()
            .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                let m = event.keystroke.modifiers;
                if event.keystroke.key == "enter" && (m.platform || m.control) {
                    cx.stop_propagation();
                    this.send_feedback(cx);
                }
            }))
            .child(
                v_flex()
                    .px_4()
                    .py_3()
                    .gap_0p5()
                    .bg(k.surface_header)
                    .border_b_1()
                    .border_color(k.panel_border)
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child("Feedback"),
                    )
                    .child(
                        div()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T400))
                            .child("Share an idea, report a bug, or send a note."),
                    ),
            )
            .child(
                v_flex()
                    .p_3p5()
                    .gap_3()
                    .child(
                        h_flex()
                            .id("feedback-kind")
                            .role(Role::RadioGroup)
                            .aria_label("Kind of feedback")
                            .w_full()
                            .p_1()
                            .gap_1()
                            .rounded(radius::lg())
                            .bg(k.surface)
                            .border_1()
                            .border_color(k.panel_border)
                            .children(kinds.map(|(kind, label, icon)| {
                                let selected = self.feedback_kind == kind;
                                h_flex()
                                    .id(SharedString::from(format!("feedback-{}", kind.as_str())))
                                    .test_support()
                                    .role(Role::Button)
                                    .aria_label(label)
                                    .aria_selected(selected)
                                    .flex_1()
                                    .justify_center()
                                    .items_center()
                                    .gap_1p5()
                                    .py_1()
                                    .px_2()
                                    .rounded(radius::md())
                                    .cursor_pointer()
                                    .text_size(text::LABEL.size)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .when(selected, |d| {
                                        d.bg(k.surface_tertiary)
                                            .border_1()
                                            .border_color(k.ink(0.14))
                                            .text_color(k.text_primary)
                                    })
                                    .when(!selected, |d| {
                                        d.text_color(k.text(TextStep::T400))
                                            .hover(|s| s.text_color(k.text_primary))
                                    })
                                    .child(Icon::new(icon.weight(selected)).size_3p5())
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.feedback_kind = kind;
                                        this.feedback_input.update(cx, |input, cx| {
                                            input.set_placeholder(
                                                crate::shell::placeholder(kind),
                                                window,
                                                cx,
                                            )
                                        });
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .rounded(radius::md())
                            .bg(k.surface)
                            .border_1()
                            .border_color(k.ink(0.18))
                            .child(
                                Textarea::new(&self.feedback_input)
                                    .appearance(false)
                                    .aria_label("Your feedback")
                                    .disabled(!signed_in || sending),
                            ),
                    )
                    .when_some(self.feedback_error.clone(), |this, error| {
                        this.child(
                            div()
                                .id("feedback-error")
                                .test_support()
                                .role(Role::Alert)
                                .aria_label(error.clone())
                                .text_size(text::LABEL.size)
                                .text_color(k.danger_text)
                                .child(error),
                        )
                    }),
            )
            .child(
                h_flex()
                    .px_4()
                    .py_2p5()
                    .gap_2()
                    .items_center()
                    .bg(k.surface_header)
                    .border_t_1()
                    .border_color(k.panel_border)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T400))
                            .child(sender),
                    )
                    .child(
                        Button::new("feedback-send")
                            .cursor_pointer()
                            .primary()
                            .small()
                            .label(if sending { "Sending…" } else { "Send" })
                            .tooltip("Send (⌘↵)")
                            .disabled(!signed_in || !has_text || sending)
                            .on_click(cx.listener(|this, _, _, cx| this.send_feedback(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_workspace(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let route = self.route;
        let content = self.contained(route, cx);
        div()
            .min_h_0()
            .min_w_0()
            .flex_1()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(k.surface_rail)
            .child(
                div()
                    .id(ids::WORKSPACE)
                    .test_support()
                    .role(Role::Main)
                    .aria_label(route.workspace().label)
                    .relative()
                    .min_h_0()
                    .w_full()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .border_l(metrics::panel_border_width())
                    .border_color(k.panel_border)
                    .bg(k.surface)
                    .child(content),
            )
            .into_any_element()
    }

    /// Builds a workspace inside a panic boundary.
    fn contained(&mut self, route: Route, cx: &mut Context<Self>) -> AnyElement {
        if let Some((_, message, _)) = self.crashes.get(&route).filter(|(_, _, failed)| *failed) {
            return self.fallback(route, message.clone(), cx);
        }
        if let Some(view) = self.workspaces.get(&route) {
            return view.clone().into_any_element();
        }
        let build = std::panic::AssertUnwindSafe(|| workspace_placeholder(route, &*cx));
        match std::panic::catch_unwind(build) {
            Ok(element) => element,
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                tracing::error!(route = route.as_str(), %message, "[Shell] workspace panicked; contained");
                let entry = self.crashes.entry(route).or_default();
                let recoverable = entry.0.record(Instant::now());
                entry.1 = message.clone().into();
                entry.2 = true;
                let _ = recoverable;
                cx.emit(ShellRequest::FeaturePanicked {
                    route,
                    message: message.clone(),
                });
                self.fallback(route, message.into(), cx)
            }
        }
    }

    fn fallback(&self, route: Route, message: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let can_reload = self
            .crashes
            .get(&route)
            .is_some_and(|(guard, _, _)| guard.recent(Instant::now()) <= crate::crash::LIMIT);
        v_flex()
            .id("workspace-fallback")
                    .test_support()
            .role(Role::Alert)
            .aria_label("This page stopped")
            .size_full()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                v_flex()
                    .max_w(gpui_kit::rems(42.))
                    .gap_4()
                    .p_8()
                    .rounded(radius::md())
                    .bg(k.surface_secondary)
                    .child(
                        h_flex()
                            .gap_3()
                            .child(Icon::new(KairoIcon::AlertTriangle).size_6().text_color(k.danger_text))
                            .child(div().text_size(text::XL.size).font_weight(gpui_kit::FontWeight::SEMIBOLD).child("This page stopped")),
                    )
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .text_color(k.text(TextStep::T300))
                            .child("Something went wrong on this page. Live screens and capture keep running. Reload the page to continue."),
                    )
                    .child(
                        div()
                            .p_4()
                            .rounded(radius::base())
                            .bg(k.surface)
                            .font_family(text::MONO_FAMILY)
                            .text_size(text::XS.size)
                            .text_color(k.danger_text)
                            .child(message),
                    )
                    .child(
                        h_flex().justify_end().child(
                            Button::new("reload-page").cursor_pointer()
                                .secondary()
                                .label(if can_reload { "Reload page" } else { "Restart Kairo to recover" })
                                .disabled(!can_reload)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(entry) = this.crashes.get_mut(&route) {
                                        entry.2 = false;
                                    }
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let overlay = self.overlay?;
        if self.overlay_view.is_none()
            && let Some(build) = self.overlay_builders.get(&overlay)
        {
            let section = if matches!(overlay, Overlay::Settings | Overlay::Screens) {
                self.settings_section
            } else {
                None
            };
            self.overlay_view = Some(build(section, window, cx));
        }
        if let Some(view) = self.overlay_view.clone() {
            return Some(view.into_any_element());
        }
        let k = *cx.kairo();
        Some(kairo_ui_kit::motion::enter(
            div()
                .id("overlay-scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(k.scrim())
                .occlude()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.close_overlay(cx)))
                .child(
                    v_flex()
                        .id("overlay-window")
                        .test_support()
                        .role(Role::Dialog)
                        .aria_label(overlay.title())
                        .w(gpui_kit::rems(30.))
                        .rounded(radius::xl2())
                        .overflow_hidden()
                        .bg(k.surface)
                        .cursor_pointer()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            FrostedPanel::new(Frost::Panel).rounded(false).child(
                                h_flex()
                                    .h_10()
                                    .px_4()
                                    .text_size(text::SM.size)
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(overlay.title()),
                            ),
                        )
                        .child(
                            v_flex()
                                .gap_4()
                                .p_5()
                                .child(
                                    div()
                                        .text_size(text::SM.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(format!(
                                            "{} couldn’t open. Close this and try again.",
                                            overlay.title()
                                        )),
                                )
                                .child(
                                    h_flex().justify_end().child(
                                        Button::new("overlay-close")
                                            .cursor_pointer()
                                            .secondary()
                                            .label("Close")
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.close_overlay(cx)
                                                }),
                                            ),
                                    ),
                                ),
                        ),
                ),
            "overlay-enter",
            cx,
        ))
    }

    fn render_notices(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let k = *cx.kairo();
        let mut notices = Vec::new();
        if !self.migration_notes.is_empty() {
            notices.push(
                v_flex()
                    .id("migration-notes")
                    .test_support()
                    .role(Role::Alert)
                    .aria_label("Upgrade notes")
                    .absolute()
                    .bottom_4()
                    .left_4()
                    .max_w_112()
                    .gap_2()
                    .rounded(radius::lg())
                    .bg(k.surface_elevated)
                    .px_3p5()
                    .py_3()
                    .occlude()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Icon::new(KairoIcon::AlertTriangle)
                                    .size_4()
                                    .text_color(k.warning_text),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(text::SM.size)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child("Kairo was upgraded. A few things need you:"),
                            )
                            .child(
                                Button::new("migration-notes-dismiss")
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .label("Dismiss")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.migration_notes.clear();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children(self.migration_notes.iter().map(|note| {
                        div()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T300))
                            .child(note.clone())
                    }))
                    .into_any_element(),
            );
        }
        let live = self.state.is_live();
        if let Some(version) = self.toast_version.clone().filter(|_| !live) {
            notices.push(
                h_flex()
                    .id("update-toast")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(format!("Kairo {version} is ready"))
                    .absolute()
                    .bottom_4()
                    .right_4()
                    .max_w_96()
                    .items_start()
                    .gap_3()
                    .rounded(radius::lg())
                    .bg(k.surface_elevated)
                    .px_3p5()
                    .py_3()
                    .child(
                        Icon::new(KairoIcon::Download)
                            .size_4()
                            .text_color(k.accent(kairo_ui_kit::tokens::Step::S300)),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(text::SM.size)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child(format!("Kairo {version} is ready")),
                            )
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child("It installs the next time you quit — or restart now."),
                            )
                            .child(
                                h_flex()
                                    .mt_2()
                                    .gap_2()
                                    .child(
                                        Button::new("toast-restart")
                                            .cursor_pointer()
                                            .outline()
                                            .xsmall()
                                            .label("Restart now")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toast_version = None;
                                                this.allow_quit();
                                                cx.emit(ShellRequest::InstallUpdate);
                                            })),
                                    )
                                    .child(
                                        Button::new("toast-later")
                                            .cursor_pointer()
                                            .ghost()
                                            .xsmall()
                                            .label("Later")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toast_version = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .into_any_element(),
            );
        }
        let account = &self.state.account;
        if account.signed_in
            && account.credentials_persisted == Some(false)
            && !self.keychain_dismissed
            && !live
        {
            notices.push(
                h_flex()
                    .id("keychain-notice")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(CREDENTIALS_NOT_SAVED_NOTICE)
                    .absolute()
                    .bottom_4()
                    .right_4()
                    .max_w_96()
                    .items_start()
                    .gap_3()
                    .rounded(radius::lg())
                    .bg(k.surface_elevated)
                    .px_3p5()
                    .py_3()
                    .child(
                        Icon::new(KairoIcon::AlertTriangle)
                            .size_4()
                            .text_color(k.hue(Hue::Amber)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T200))
                            .child(CREDENTIALS_NOT_SAVED_NOTICE),
                    )
                    .child(
                        Button::new("dismiss-keychain")
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::X).size_3())
                            .accessibility_label("Dismiss keychain notice")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.keychain_dismissed = true;
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        notices
    }

    fn render_whats_new(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let entry = self.whats_new.clone()?;
        let k = *cx.kairo();
        let unreleased = self.dev && entry.version != self.version.as_ref();
        let body: AnyElement = if entry.highlights.is_empty() {
            v_flex()
                .gap_2p5()
                .child(
                    div()
                        .text_size(text::LG.size)
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child(entry.title.clone()),
                )
                .children(entry.notes.iter().map(|n| {
                    h_flex()
                        .items_start()
                        .gap_2()
                        .text_size(text::SM.size)
                        .text_color(k.text(TextStep::T300))
                        .child(div().text_color(k.text(TextStep::T500)).child("•"))
                        .child(div().min_w_0().flex_1().child(n.clone()))
                }))
                .into_any_element()
        } else {
            v_flex()
                .gap_7()
                .children(entry.highlights.iter().map(|h| {
                    let (icon, hue) = highlight_mark(&h.icon);
                    h_flex()
                        .items_start()
                        .gap_5()
                        .child(Icon::new(icon.fill()).size_9().text_color(k.hue(hue)))
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(text::LG.size)
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .child(h.title.clone()),
                                )
                                .child(
                                    div()
                                        .mt_1()
                                        .text_size(text::SM.size)
                                        .text_color(k.text(TextStep::T300))
                                        .child(h.body.clone()),
                                ),
                        )
                }))
                .into_any_element()
        };
        Some(kairo_ui_kit::motion::enter(
            div()
                .id("whats-new-scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .bg(k.scrim())
                .occlude()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.close_whats_new(cx)))
                .child(
                    v_flex()
                        .id("whats-new")
                        .test_support()
                        .role(Role::Dialog)
                        .aria_label("What's New in Kairo")
                        .w_full()
                        .max_w(gpui_kit::rems(40.))
                        .max_h(gpui_kit::relative(0.9))
                        .rounded(radius::xl2())
                        .overflow_hidden()
                        .bg(k.glass)
                        .cursor_pointer()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            v_flex()
                                .items_center()
                                .px_10()
                                .pt_10()
                                .pb_6()
                                .child(
                                    div()
                                        .text_size(text::XL3.size)
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .child("What's New in Kairo"),
                                )
                                .child(
                                    h_flex()
                                        .mt_1p5()
                                        .gap_2()
                                        .text_size(text::SM.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(format!(
                                            "Version {} · {}",
                                            entry.version,
                                            whats_new::format_date(&entry.date)
                                        ))
                                        .when(unreleased, |this| {
                                            this.child(
                                                div()
                                                    .rounded(radius::sm())
                                                    .bg(k.surface_tertiary)
                                                    .px_1p5()
                                                    .text_size(text::LABEL.size)
                                                    .child("Preview · not released"),
                                            )
                                        }),
                                ),
                        )
                        .child(
                            v_flex()
                                .id("whats-new-body")
                                .min_h_0()
                                .flex_1()
                                .overflow_y_scroll()
                                .px_12()
                                .pb_6()
                                .child(body)
                                .child(
                                    h_flex().mt_8().justify_center().child(
                                        Button::new("release-notes")
                                            .cursor_pointer()
                                            .link()
                                            .label("View full release notes")
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.emit(ShellRequest::OpenExternal(
                                                    CHANGELOG_URL.into(),
                                                ))
                                            })),
                                    ),
                                ),
                        )
                        .child(
                            h_flex()
                                .justify_center()
                                .bg(k.surface_secondary)
                                .border_t(metrics::panel_border_width())
                                .border_color(k.panel_border)
                                .px_6()
                                .py_4()
                                .child(
                                    Button::new("whats-new-continue")
                                        .cursor_pointer()
                                        .primary()
                                        .min_w(gpui_kit::rems(8.75))
                                        .label("Continue")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.close_whats_new(cx)),
                                        ),
                                ),
                        )
                        .focus_trap("whats-new", self.whats_new_focus.handle()),
                ),
            "whats-new-enter",
            cx,
        ))
    }

    fn render_quit(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.quit_open {
            return None;
        }
        let k = *cx.kairo();
        let recording = if self.state.recording {
            " The recording will be stopped and saved."
        } else {
            ""
        };
        Some(
            kairo_ui_kit::motion::enter(div()
                .id("quit-scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .bg(gpui_kit::black().opacity(0.35))
                .occlude()
                .child(
                    v_flex()
                        .id("quit-confirmation")
                        .test_support()
                        .role(Role::AlertDialog)
                        .aria_label("Quit Kairo?")
                        .w_full()
                        .max_w(gpui_kit::rems(26.25))
                        .rounded(radius::xl2())
                        .overflow_hidden()
                        .bg(k.glass)
                        .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.respond_to_quit(false, cx);
                            }
                        }))
                        .child(
                            v_flex()
                                .px_7()
                                .pt_8()
                                .pb_7()
                                .child(KairoMark::new(MarkSize::Large))
                                .child(div().mt_6().text_size(text::XL2.size).font_weight(gpui_kit::FontWeight::SEMIBOLD).child("Quit Kairo?"))
                                .child(
                                    div()
                                        .mt_3()
                                        .text_size(text::SM.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(format!(
                                            "Your screens will go offline and live transcription will stop.{recording} Ready to close Kairo?"
                                        )),
                                ),
                        )
                        .child(
                            h_flex()
                                .flex_wrap()
                                .justify_end()
                                .gap_2()
                                .bg(k.surface_tertiary)
                                .px_7()
                                .py_5()
                                .child(
                                    Button::new("keep-open").cursor_pointer()
                                        .track_focus(&self.keep_open_focus)
                                        .secondary()
                                        .label("Keep Kairo open")
                                        .on_click(cx.listener(|this, _, _, cx| this.respond_to_quit(false, cx))),
                                )
                                .child(
                                    Button::new("quit-kairo").cursor_pointer()
                                        .primary()
                                        .label("Quit Kairo")
                                        .on_click(cx.listener(|this, _, _, cx| this.respond_to_quit(true, cx))),
                                ),
                        )
                        .focus_trap("quit-confirmation", self.quit_focus.handle()),
                ), "quit-enter", cx),
        )
    }
}

/// `CREDENTIALS_NOT_SAVED_NOTICE` (`lib/credentials-notice.ts`): the OS
/// keychain cannot hold the sign-in, so it is lost on quit.
pub const CREDENTIALS_NOT_SAVED_NOTICE: &str =
    "This computer has no keychain, so you’ll need to sign in again next launch.";

/// The What's New marks (`WHATS_NEW_ICONS`); unknown names fall back to `tip`.
pub fn highlight_mark(name: &str) -> (KairoIcon, Hue) {
    match name {
        "operator" => (KairoIcon::PhBroadcast, Hue::Green),
        "scripture" => (KairoIcon::PhBookOpen, Hue::Blue),
        "lyrics" => (KairoIcon::PhMusicNotes, Hue::Violet),
        "documents" => (KairoIcon::PhFileText, Hue::Cyan),
        "theme" => (KairoIcon::PhPalette, Hue::Pink),
        "media" => (KairoIcon::Film, Hue::Orange),
        "screens" => (KairoIcon::PhMonitor, Hue::Indigo),
        "cloud" => (KairoIcon::PhCloud, Hue::Blue),
        "speed" => (KairoIcon::PhLightning, Hue::Coral),
        _ => (KairoIcon::PhLightbulb, Hue::Lime),
    }
}

/// A workspace's content until its view is installed.
fn workspace_placeholder(route: Route, cx: &App) -> AnyElement {
    let k = cx.kairo();
    let w = route.workspace();
    v_flex()
        .id(SharedString::from(format!("workspace-{}", route.as_str())))
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .child(Icon::new(w.icon.fill()).size_8().text_color(k.hue(w.hue)))
        .child(
            div()
                .text_size(text::XL.size)
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(w.label),
        )
        .child(
            div()
                .text_size(text::SM.size)
                .text_color(k.text(TextStep::T500))
                .child("Loading…"),
        )
        .into_any_element()
}

/// `relativeTime` for the Account popover's Last synced.
fn synced_label(at_ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let minutes = now.saturating_sub(at_ms) / 60_000;
    match minutes {
        0 => "Just now".into(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} hr ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}
