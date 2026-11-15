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
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable, Icon, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    Role, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use kairo_ui_kit::brand::{KairoMark, MarkSize};
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
        let (live_label, live_detail) = self.state.live_text();
        let live = self.state.is_live() || self.state.recording;
        let clearing = false;

        h_flex()
            .id(ids::HEADER)
            .test_support()
            .role(Role::Toolbar)
            .aria_label("Presentation controls")
            .flex_shrink_0()
            .h(metrics::header_height())
            .px_3()
            .gap_3()
            .relative()
            .bg(k.surface_header)
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
            .child(
                h_flex()
                    .w(metrics::traffic_light_corner())
                    .flex_shrink_0()
                    .pl_1p5()
                    .when(show_logo, |this| {
                        this.child(KairoMark::new(MarkSize::Small))
                    }),
            )
            .child(
                ToolbarButton::new(
                    "search-lyrics",
                    "Lyrics Search",
                    "Search lyrics",
                    hued(KairoIcon::Search, Hue::Violet, cx),
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_overlay(Overlay::SongSearch, cx))),
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
                                .size_4()
                                .text_color(k.hue(Hue::Orange)),
                        )
                        .when(self.media_live.is_some(), |b| b.badge(k.live))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::ToggleMedia))),
                    )
                    .child(self.render_screen_themes(cx))
                    .child(
                        ToolbarButton::new(
                            "screens",
                            "Screens",
                            format!("Screens: {screens_detail}. Open Screens (⌥⌘1)"),
                            Icon::new(KairoIcon::MonitorPlay.fill())
                                .size_4()
                                .text_color(k.hue(Hue::Cyan)),
                        )
                        .badge(dot)
                        .coach_anchor(kairo_ui_kit::coach::anchors::HEADER_SCREENS)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.open_overlay(Overlay::Screens, cx)),
                        ),
                    ),
            )
            .child(
                h_flex()
                    .relative()
                    .flex_shrink_0()
                    .gap_1()
                    .pl_1()
                    .child(kairo_ui_kit::coach::coach_anchor(
                        kairo_ui_kit::coach::anchors::HEADER_LIVE,
                    ))
                    .when_some(self.state.house_track.clone(), |this, track| {
                        let paused = track.paused;
                        this.child(
                            ToolbarButton::new(
                                "house-track",
                                track.name.clone(),
                                format!("{} {}", if paused { "Play" } else { "Pause" }, track.name),
                                Icon::new(if paused {
                                    KairoIcon::Play.fill()
                                } else {
                                    KairoIcon::Pause.fill()
                                })
                                .size_3p5(),
                            )
                            .on_click(cx.listener(
                                move |_, _, _, cx| {
                                    cx.emit(ShellRequest::SetHouseTrackPaused(!paused))
                                },
                            )),
                        )
                    })
                    .child(self.render_live(live_label, live_detail, live, cx))
                    .child(
                        ToolbarButton::new(
                            "clear",
                            if clearing { "Clearing" } else { "Clear" },
                            "Clear text and the dock background",
                            hued(KairoIcon::Eraser, Hue::Pink, cx),
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
                    .custom(solid)
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
                        .custom(solid)
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
            .custom(variant)
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
            .anchor(gpui_kit::Anchor::BottomLeft)
            .trigger(self.rail_trigger(
                "audio",
                KairoIcon::Mic,
                "Audio",
                Some(Hue::Green),
                false,
                cx,
            ))
            .content(|_, _, cx| {
                let k = *cx.kairo();
                div()
                    .w_64()
                    .p_3()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child("Audio input and level arrive with prompt 091.")
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
                    .items_center()
                    .gap_2p5()
                    .overflow_y_scroll()
                    .px_1()
                    .py_3()
                    .children(WORKSPACES.iter().map(|w| {
                        let target = w.route;
                        let item = RailItem::new(
                            SharedString::from(format!("rail-{}", w.route.as_str())),
                            w.icon,
                            w.rail_label(),
                            w.hue,
                        )
                        .active(route == w.route)
                        .on_click(cx.listener(move |this, _, _, cx| this.navigate(target, cx)));
                        match w.tooltip() {
                            Some(tip) => div()
                                .id(SharedString::from(format!("rail-tip-{}", w.route.as_str())))
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
            .anchor(gpui_kit::Anchor::BottomLeft)
            .trigger(self.rail_trigger(
                "account",
                KairoIcon::CircleUser,
                "Account",
                Some(Hue::Blue),
                false,
                cx,
            ))
            .content(move |_, _, cx| {
                let k = *cx.kairo();
                let entity = entity.clone();
                v_flex()
                    .w_64()
                    .p_1()
                    .child(
                        v_flex()
                            .px_3()
                            .py_2()
                            .child(
                                div()
                                    .text_size(text::SM.size)
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(k.text_primary)
                                    .child(
                                        account
                                            .name
                                            .clone()
                                            .or(account.email.clone())
                                            .unwrap_or_else(|| "Signed out".into()),
                                    ),
                            )
                            .when_some(
                                account.email.clone().filter(|_| account.name.is_some()),
                                |this, email| {
                                    this.child(
                                        div()
                                            .text_size(text::XS.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(email),
                                    )
                                },
                            ),
                    )
                    .child(
                        Button::new("account-settings")
                            .ghost()
                            .small()
                            .w_full()
                            .icon(Icon::new(KairoIcon::Settings).size_3())
                            .label("Account settings")
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| this.open_settings("account", cx))
                            }),
                    )
            })
            .into_any_element()
    }

    /// The header's Live button: what is on screen, the recording clock,
    /// and its popover with capture and Syphon (`LiveMenu`).
    fn render_live(
        &mut self,
        label: SharedString,
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
        let text: SharedString = match &elapsed {
            Some(e) => format!("{label} · {e}").into(),
            None => label,
        };
        let title = format!(
            "{detail}{}. Click for live outputs and capture",
            elapsed
                .as_ref()
                .map(|e| format!(" · recording {e}"))
                .unwrap_or_default()
        );
        // Recording shows as a red dot on the icon, as on a camera.
        let icon = div()
            .relative()
            .child(Icon::new(KairoIcon::Radio).size_4())
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
        Popover::new("live-popover")
            .anchor(gpui_kit::Anchor::TopRight)
            .open(self.live_open)
            .on_open_change(move |next, _, cx| {
                let next = *next;
                entity.update(cx, |this, cx| this.set_live_open(next, cx))
            })
            .trigger(button.opens_popover())
            .content(move |_, _, _| div().w_64().p_1().child(view.clone()))
            .into_any_element()
    }

    /// The header's Screen Themes button and its popover.
    fn render_screen_themes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let button = ToolbarButton::new(
            "screen-themes",
            "Screen Themes",
            "Choose themes for your screens",
            hued(KairoIcon::Palette, Hue::Pink, cx),
        );
        let Some((view, _)) = self.screen_themes.clone() else {
            return button.disabled(true).into_any_element();
        };
        let entity = cx.entity();
        Popover::new("screen-themes-popover")
            .anchor(gpui_kit::Anchor::TopRight)
            .open(self.screen_themes_open)
            .on_open_change(move |next, window, cx| {
                let next = *next;
                entity.update(cx, |this, cx| this.set_screen_themes_open(next, window, cx))
            })
            .trigger(button.opens_popover())
            .content(move |_, _, _| div().p_3().child(view.clone()))
            .into_any_element()
    }

    fn render_feedback(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let open = self.feedback_open;
        Popover::new("feedback-popover")
            .anchor(gpui_kit::Anchor::BottomLeft)
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
            .trigger(self.rail_trigger(
                "feedback",
                KairoIcon::PhChatCircleDots,
                "Feedback",
                Some(Hue::Coral),
                open,
                cx,
            ))
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
                .w_80()
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
                        .child("Thank you"),
                )
                .child(
                    div()
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
            .w_80()
            .gap_3()
            .p_3()
            .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                let m = event.keystroke.modifiers;
                if event.keystroke.key == "enter" && (m.platform || m.control) {
                    cx.stop_propagation();
                    this.send_feedback(cx);
                }
            }))
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Feedback"),
                    )
                    .child(
                        h_flex()
                            .id("feedback-kind")
                            .role(Role::RadioGroup)
                            .aria_label("Kind of feedback")
                            .gap_0p5()
                            .children(kinds.map(|(kind, label, icon)| {
                                let selected = self.feedback_kind == kind;
                                Button::new(SharedString::from(format!(
                                    "feedback-{}",
                                    kind.as_str()
                                )))
                                .ghost()
                                .xsmall()
                                .selected(selected)
                                .icon(Icon::new(icon.weight(selected)).size_3())
                                .label(label)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.feedback_kind = kind;
                                        this.feedback_input.update(cx, |input, cx| {
                                            input.set_placeholder(
                                                crate::shell::placeholder(kind),
                                                window,
                                                cx,
                                            )
                                        });
                                        cx.notify();
                                    },
                                ))
                            })),
                    ),
            )
            .child(
                Textarea::new(&self.feedback_input)
                    .aria_label("Your feedback")
                    .disabled(!signed_in || sending),
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
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(sender),
                    )
                    .child(
                        Button::new("feedback-send")
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
                    .rounded_tl(radius::xl2())
                    .border_t(metrics::panel_border_width())
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
                            Button::new("reload-page")
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
        Some(
            div()
                .id("overlay-scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(k.scrim())
                .occlude()
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
                                            "{} arrives with prompt {}.",
                                            overlay.title(),
                                            overlay.owner_prompt()
                                        )),
                                )
                                .child(
                                    h_flex().justify_end().child(
                                        Button::new("overlay-close")
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
                )
                .into_any_element(),
        )
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
        Some(
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
                                        .primary()
                                        .min_w(gpui_kit::rems(8.75))
                                        .label("Continue")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.close_whats_new(cx)),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
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
            div()
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
                                    Button::new("keep-open")
                                        .secondary()
                                        .label("Keep Kairo open")
                                        .on_click(cx.listener(|this, _, _, cx| this.respond_to_quit(false, cx))),
                                )
                                .child(
                                    Button::new("quit-kairo")
                                        .primary()
                                        .label("Quit Kairo")
                                        .on_click(cx.listener(|this, _, _, cx| this.respond_to_quit(true, cx))),
                                ),
                        ),
                )
                .into_any_element(),
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

/// A workspace's content until its prompt builds it.
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
                .child(format!("Built in prompt {}.", route.owner_prompt())),
        )
        .into_any_element()
}
