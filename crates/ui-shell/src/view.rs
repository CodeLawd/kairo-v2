//! Rendering for [`Shell`].

use crate::chrome::{Tone, ToolbarButton, hued};
use crate::crash::panic_message;
use crate::keymap::{
    self, About, ActualSize, BringAllToFront, CheckForUpdates, CloseWindow, Hide, HideOthers,
    Import, Minimize, OpenCaptureSettings, OpenScreenThemes, OpenSettings, OpenWorkspace, Quit,
    ShowAll, SongSearch, ToggleAudienceScreens, ToggleCapture, ToggleFullScreen, ToggleMediaBin,
    ToggleScreens, ToggleStageScreens, WhatsNew, ZoomIn, ZoomOut, ZoomWindow,
};
use crate::route::{APPS, Route, WORKSPACES};
use crate::shell::{AccountPage, FeedbackPhase, Overlay, Shell};
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
            .on_action(
                cx.listener(|this, action: &OpenWorkspace, _, cx| this.navigate(action.0.0, cx)),
            )
            .on_action(
                cx.listener(|_, _: &ToggleMediaBin, _, cx| cx.emit(ShellRequest::ToggleMedia)),
            )
            .on_action(cx.listener(|this, _: &ToggleAudienceScreens, _, cx| {
                this.toggle_screen_group(false, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleStageScreens, _, cx| {
                    this.toggle_screen_group(true, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenScreenThemes, window, cx| {
                this.set_screen_themes_open(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleCapture, _, cx| {
                cx.emit(ShellRequest::SetCapture(!this.state.recording))
            }))
            .on_action(
                cx.listener(|this, _: &OpenCaptureSettings, _, cx| this.set_live_open(true, cx)),
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
                    .child(
                        self.render_screen_toggle(
                            "audience-screen",
                            "Audience",
                            false,
                            self.state.audience_on,
                            (level == ScreensLevel::Warning).then(|| screens_detail.clone()),
                            cx,
                        )
                        // Screens setup lives in the Screens menu now; the tour
                        // points here, at what the audience sees.
                        .coach_anchor(kairo_ui_kit::coach::anchors::HEADER_SCREENS),
                    )
                    .child(self.render_screen_toggle(
                        "stage-screen",
                        "Stage",
                        true,
                        self.state.stage_on,
                        None,
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

    /// An Audience or Stage button: one click turns the group on or off (⌘1,
    /// ⌘2). `problem` is a screen that isn't showing, flagged with a dot.
    fn render_screen_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        stage: bool,
        state: Option<bool>,
        problem: Option<String>,
        cx: &mut Context<Self>,
    ) -> ToolbarButton {
        let k = *cx.kairo();
        let key = if stage { "⌘2" } else { "⌘1" };
        let (ring_color, title) = match state {
            Some(true) => (
                k.success_text,
                format!("{label} screens are on — click to turn off ({key})"),
            ),
            Some(false) => (
                k.live,
                format!("{label} screens are off — click to turn on ({key})"),
            ),
            None => (
                k.text(TextStep::T500),
                format!("No {label} screens set up — click to add one"),
            ),
        };
        let title = match &problem {
            Some(problem) => format!("{title}. {problem}"),
            None => title,
        };
        let dot = div().size(px(14.)).rounded_full().bg(ring_color);
        ToolbarButton::new(id, label, title, dot)
            .when(problem.is_some(), |b| b.badge(k.hue(Hue::Amber)))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_screen_group(stage, cx)))
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

    fn render_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let route = self.route;
        let account = self.render_account(cx);
        let apps = self.render_apps(cx);
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
                            .children(WORKSPACES.iter().filter(|w| !w.route.in_apps()).map(|w| {
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
                            }))
                            .child(apps),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .pb_3()
                    .child(account),
            )
            .into_any_element()
    }

    /// The rail's one footer button: who is signed in, opening a menu to its
    /// right with Audio, Feedback, Settings and Sign out. Audio and Feedback
    /// fly out beside the menu on hover, as a submenu would.
    fn render_account(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let account = &self.state.account;
        let title = account
            .name
            .clone()
            .or(account.email.clone())
            .unwrap_or_else(|| "Not signed in".into());
        let open = self.account_open;
        let entity = cx.entity();
        let variant = ButtonCustomVariant::new(cx)
            .color(if open {
                k.surface_elevated
            } else {
                k.surface.opacity(0.0)
            })
            .foreground(k.text_primary)
            .hover(k.surface_tertiary)
            .active(k.surface_elevated);
        let trigger = Button::new("account")
            .cursor_pointer()
            .custom(variant)
            // Solid fill: the custom variant paints its colour at 20%.
            .when(open, |b| b.bg(k.surface_elevated))
            .w_full()
            .h_auto()
            .py_1p5()
            .rounded(radius::lg())
            .accessibility_label("Account")
            .tooltip(title.clone())
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .child(avatar(&title, false, cx)),
            );
        Popover::new("account-popover")
            .anchor(gpui_kit::Anchor::LeftCenter)
            .offset(px(8.))
            .appearance(false)
            .open(open)
            .on_open_change({
                let entity = entity.clone();
                move |next, window, cx| {
                    let next = *next;
                    entity.update(cx, |this, cx| {
                        if next {
                            this.account_open = true;
                            this.account_page = AccountPage::Menu;
                            this.account_pinned = false;
                            cx.notify();
                        } else {
                            this.close_account(window, cx);
                        }
                    })
                }
            })
            .trigger(trigger.popover_trigger())
            .content(move |_, window, cx| {
                entity.update(cx, |this, cx| {
                    let menu = this.account_menu(cx);
                    let flyout = match this.account_page {
                        AccountPage::Menu => None,
                        AccountPage::Feedback => Some(this.feedback_content(window, cx)),
                        AccountPage::Audio => this.audio.clone().map(|(view, _)| {
                            let k = *cx.kairo();
                            div()
                                .rounded(radius::xl())
                                .overflow_hidden()
                                .bg(k.surface_secondary)
                                .border_1()
                                .border_color(k.ink(0.14))
                                .shadow_2xl()
                                .child(view)
                                .into_any_element()
                        }),
                    };
                    // One element holds both, so a click in the flyout is
                    // inside the popover and doesn't dismiss it.
                    let page = this.account_page;
                    let element = h_flex().items_end().gap_2().child(menu).when_some(
                        flyout,
                        |this, flyout| {
                            this.child(kairo_ui_kit::motion::pop_in(
                                div()
                                    .id("account-flyout")
                                    // Working in a panel keeps it open while the
                                    // pointer passes over the menu again.
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, _| this.account_pinned = true),
                                    )
                                    .child(flyout),
                                SharedString::from(format!("account-flyout-in-{page:?}")),
                                kairo_ui_kit::motion::From::Leading,
                                cx,
                            ))
                        },
                    );
                    kairo_ui_kit::motion::pop_in(
                        element,
                        "account-menu-in",
                        kairo_ui_kit::motion::From::Leading,
                        cx,
                    )
                })
            })
            .into_any_element()
    }

    /// Closes the account menu and whichever panel it was showing.
    pub(crate) fn close_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_account_page(AccountPage::Menu, window, cx);
        self.account_open = false;
        self.account_pinned = false;
        cx.notify();
    }

    /// Shows one of the menu's panels beside it, or none.
    fn set_account_page(&mut self, page: AccountPage, window: &mut Window, cx: &mut Context<Self>) {
        let was = self.account_page;
        if was == page {
            return;
        }
        if let Some((_, opened)) = self.audio.clone() {
            if was == AccountPage::Audio {
                opened(false, window, cx);
            }
            if page == AccountPage::Audio {
                opened(true, window, cx);
            }
        }
        if was == AccountPage::Feedback {
            // A draft survives; a thank-you resets (see close_feedback).
            self.account_page = page;
            self.close_feedback(cx);
        }
        self.feedback_open = page == AccountPage::Feedback;
        self.account_page = page;
        self.account_pinned = false;
        cx.notify();
    }

    /// The pointer reached a menu row: a submenu row opens its panel, any
    /// other row closes the open one unless the operator is working in it.
    fn hover_account_row(
        &mut self,
        page: Option<AccountPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match page {
            Some(page) => self.set_account_page(page, window, cx),
            None if !self.account_pinned => self.set_account_page(AccountPage::Menu, window, cx),
            None => {}
        }
    }

    fn account_menu(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let entity = cx.entity();
        let account = self.state.account.clone();
        let signed_out = !account.signed_in;
        let has_audio = self.audio.is_some();
        let (status, dot) = if signed_out {
            (
                "Signed out".to_owned(),
                k.neutral(kairo_ui_kit::tokens::Step::S600),
            )
        } else if account.offline {
            ("Offline".to_owned(), k.hue(Hue::Orange))
        } else {
            (
                match account.last_synced_ms {
                    Some(ms) => format!("Online · synced {}", synced_label(ms)),
                    None => "Online".to_owned(),
                },
                k.hue(Hue::Green),
            )
        };
        let title = account
            .name
            .clone()
            .or(account.email.clone())
            .unwrap_or_else(|| "Not signed in".into());
        let subtitle: Option<SharedString> = match (&account.org, &account.email) {
            (Some((org, role)), _) => Some(format!("{org} · {role}").into()),
            (None, Some(email)) if account.name.is_some() => Some(email.clone()),
            _ => None,
        };
        let showing = self.account_page;
        let hover = |sub: Option<AccountPage>| {
            let entity = entity.clone();
            move |hovered: &bool, window: &mut Window, cx: &mut App| {
                if *hovered {
                    entity.update(cx, |this, cx| this.hover_account_row(sub, window, cx));
                }
            }
        };
        let row = |id: &'static str,
                   icon: KairoIcon,
                   label: &'static str,
                   sub: Option<AccountPage>,
                   danger: bool,
                   click: MenuClick| {
            let more = sub.is_some();
            h_flex()
                .id(id)
                .test_support()
                .role(Role::Button)
                .aria_label(label)
                .when(more, |r| r.aria_expanded(sub == Some(showing)))
                .w_full()
                .gap_3()
                .px_2p5()
                .py_2()
                .rounded(radius::md())
                .cursor_pointer()
                .when(more && sub == Some(showing), |r| r.bg(k.ink(0.06)))
                .hover(|s| s.bg(k.ink(0.06)))
                .on_hover(hover(sub))
                .text_size(text::SM.size)
                .text_color(if danger {
                    k.danger_text
                } else {
                    k.text_primary
                })
                .on_click(move |_, window, cx| click(window, cx))
                .child(Icon::new(icon).size_4().flex_none().text_color(if danger {
                    k.danger_text
                } else {
                    k.text(TextStep::T300)
                }))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .when(more, |r| {
                    r.child(
                        Icon::new(KairoIcon::ChevronRight)
                            .size_3p5()
                            .text_color(k.text(TextStep::T500)),
                    )
                })
        };
        let divider = || {
            div()
                .mx_2p5()
                .my_1()
                .h(metrics::panel_border_width())
                .bg(k.panel_border)
        };
        let on = |f: fn(&mut Shell, &mut Window, &mut Context<Shell>)| -> MenuClick {
            let entity = entity.clone();
            Box::new(move |window, cx| entity.update(cx, |this, cx| f(this, window, cx)))
        };

        v_flex()
            .w(px(272.))
            .p_1p5()
            .rounded(radius::xl())
            .bg(k.surface_secondary)
            .border_1()
            .border_color(k.ink(0.14))
            .shadow_2xl()
            // Who is signed in; opens the Account pane.
            .child(
                h_flex()
                    .id("account-settings")
                    .test_support()
                    .role(Role::Button)
                    .aria_label("Account settings")
                    .w_full()
                    .gap_3()
                    .px_2p5()
                    .py_2()
                    .rounded(radius::md())
                    .cursor_pointer()
                    .hover(|s| s.bg(k.ink(0.06)))
                    .on_hover(hover(None))
                    .on_click({
                        let f = on(|this, window, cx| {
                            this.close_account(window, cx);
                            this.open_settings("account", cx);
                        });
                        move |_, window, cx| f(window, cx)
                    })
                    .child(avatar(&title, true, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(k.text_primary)
                                    .child(title),
                            )
                            .when_some(subtitle, |this, sub| {
                                this.child(
                                    div()
                                        .truncate()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(sub),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(div().size(px(6.)).rounded_full().bg(dot))
                                    .child(status),
                            ),
                    )
                    .child(
                        Icon::new(KairoIcon::ChevronRight)
                            .size_3p5()
                            .text_color(k.text(TextStep::T500)),
                    ),
            )
            .child(divider())
            .when(has_audio, |this| {
                this.child(row(
                    "account-audio",
                    KairoIcon::Mic,
                    "Audio",
                    Some(AccountPage::Audio),
                    false,
                    on(|this, window, cx| {
                        this.set_account_page(AccountPage::Audio, window, cx);
                        this.account_pinned = true;
                    }),
                ))
            })
            .child(row(
                "account-feedback",
                KairoIcon::PhChatCircleDots,
                "Send feedback",
                Some(AccountPage::Feedback),
                false,
                on(|this, window, cx| {
                    this.set_account_page(AccountPage::Feedback, window, cx);
                    this.account_pinned = true;
                }),
            ))
            .child(row(
                "account-tips",
                KairoIcon::Lightbulb,
                "Show tips again",
                None,
                false,
                on(|this, window, cx| {
                    this.close_account(window, cx);
                    cx.emit(ShellRequest::ShowTips);
                }),
            ))
            .child(row(
                "settings",
                KairoIcon::Settings,
                "Settings",
                None,
                false,
                on(|this, window, cx| {
                    this.close_account(window, cx);
                    this.open_overlay(Overlay::Settings, cx);
                }),
            ))
            .when(!signed_out, |this| {
                this.child(divider()).child(row(
                    "account-sign-out",
                    KairoIcon::LogOut,
                    "Sign out",
                    None,
                    true,
                    on(|this, window, cx| {
                        this.close_account(window, cx);
                        cx.emit(ShellRequest::SignOut);
                    }),
                ))
            })
            .into_any_element()
    }

    /// The rail's Apps button: workspaces used now and then, kept off the
    /// rail so the daily ones stand out.
    fn render_apps(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let route = self.route;
        let current = APPS.iter().find(|r| **r == route).map(|r| r.workspace());
        let open = self.apps_open;
        let entity = cx.entity();
        // The rail's own item, so More looks like every workspace above it;
        // an app from its menu lights it in that app's colour.
        let trigger = RailItem::new(
            "rail-apps",
            KairoIcon::MoreHorizontal,
            "More",
            current.map_or(Hue::Indigo, |w| w.hue),
        )
        .active(current.is_some())
        .open(open)
        // Pressable from VoiceOver; the popover opens it for the pointer.
        .on_click(|_, _, _| {});
        Popover::new("apps-popover")
            // The popover wraps its trigger: stretch the wrapper so More is
            // as wide as every rail item, not just its label.
            .trigger_style({
                let mut style = gpui_kit::StyleRefinement::default();
                style.size.width = Some(gpui_kit::relative(1.).into());
                style
            })
            .anchor(gpui_kit::Anchor::LeftCenter)
            .offset(px(8.))
            .appearance(false)
            .open(open)
            .on_open_change({
                let entity = entity.clone();
                move |next, _, cx| {
                    let next = *next;
                    entity.update(cx, |this, cx| {
                        this.apps_open = next;
                        cx.notify();
                    })
                }
            })
            .trigger(trigger)
            .content(move |_, _, cx| {
                let k = *cx.kairo();
                let menu = v_flex()
                    .w(px(264.))
                    .p_1p5()
                    .rounded(radius::xl())
                    .bg(k.surface_secondary)
                    .border_1()
                    .border_color(k.ink(0.14))
                    .shadow_2xl()
                    .child(
                        div()
                            .px_2p5()
                            .pt_1p5()
                            .pb_1()
                            .text_size(text::LABEL.size)
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T500))
                            .child("More"),
                    )
                    .children(APPS.iter().map(|r| {
                        let w = r.workspace();
                        let target = w.route;
                        let active = target == route;
                        let entity = entity.clone();
                        h_flex()
                            .id(SharedString::from(format!("apps-{}", target.as_str())))
                            .test_support()
                            .role(Role::Button)
                            .aria_label(w.label)
                            .aria_selected(active)
                            .w_full()
                            .gap_3()
                            .px_2()
                            .py_1p5()
                            .rounded(radius::md())
                            .cursor_pointer()
                            .when(active, |s| s.bg(k.ink(0.06)))
                            .hover(|s| s.bg(k.ink(0.06)))
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.apps_open = false;
                                    this.navigate(target, cx);
                                })
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .size_8()
                                    .rounded(radius::md())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(k.hue(w.hue).opacity(0.16))
                                    .child(
                                        Icon::new(w.icon.weight(active))
                                            .size_4()
                                            .text_color(k.hue(w.hue)),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::SM.size)
                                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                                            .text_color(k.text_primary)
                                            .child(w.label),
                                    )
                                    .when_some(w.hint, |this, hint| {
                                        this.child(
                                            div()
                                                .truncate()
                                                .text_size(text::LABEL.size)
                                                .text_color(k.text(TextStep::T400))
                                                .child(hint),
                                        )
                                    }),
                            )
                            .when(active, |s| {
                                s.child(
                                    Icon::new(KairoIcon::Check)
                                        .size_3p5()
                                        .text_color(k.text(TextStep::T400)),
                                )
                            })
                    }))
                    // Screen setup lives here too, beside the apps.
                    .child(
                        div()
                            .mx_2()
                            .my_1()
                            .h(metrics::panel_border_width())
                            .bg(k.panel_border),
                    )
                    .child({
                        let entity = entity.clone();
                        let hue = k.hue(Hue::Indigo);
                        h_flex()
                            .id("apps-screens")
                            .test_support()
                            .role(Role::Button)
                            .aria_label("Screens")
                            .w_full()
                            .gap_3()
                            .px_2()
                            .py_1p5()
                            .rounded(radius::md())
                            .cursor_pointer()
                            .hover(|s| s.bg(k.ink(0.06)))
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.apps_open = false;
                                    this.open_overlay(Overlay::Screens, cx);
                                })
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .size_8()
                                    .rounded(radius::md())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(hue.opacity(0.16))
                                    .child(
                                        Icon::new(KairoIcon::PhMonitor).size_4().text_color(hue),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::SM.size)
                                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                                            .text_color(k.text_primary)
                                            .child("Screens"),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T400))
                                            .child("Set up displays, NDI and feeds"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(if cfg!(target_os = "macos") {
                                        "⌥⌘1"
                                    } else {
                                        "Ctrl+Alt+1"
                                    }),
                            )
                    });
                kairo_ui_kit::motion::pop_in(
                    menu,
                    "apps-menu-in",
                    kairo_ui_kit::motion::From::Leading,
                    cx,
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
            .appearance(false)
            .content(move |_, _, cx| {
                kairo_ui_kit::motion::pop_in(
                    header_popover_surface(cx).w_64().p_1().child(view.clone()),
                    "live-popover-in",
                    kairo_ui_kit::motion::From::Above,
                    cx,
                )
            })
            .into_any_element()
    }

    /// The header's Looks button (which theme each screen shows) and its popover.
    fn render_screen_themes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let button = ToolbarButton::new(
            "screen-themes",
            "Looks",
            "Choose which theme each screen shows",
            Icon::new(KairoIcon::Looks.fill())
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
            .appearance(false)
            .content(move |_, _, cx| {
                kairo_ui_kit::motion::pop_in(
                    header_popover_surface(cx).p_3().child(view.clone()),
                    "screen-themes-popover-in",
                    kairo_ui_kit::motion::From::Above,
                    cx,
                )
            })
            .into_any_element()
    }

    /// The Feedback panel beside the account menu: what kind, the note, send.
    fn feedback_content(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let account = &self.state.account;
        let panel = || {
            v_flex()
                .w(px(340.))
                .rounded(radius::xl())
                .overflow_hidden()
                .bg(k.surface_secondary)
                .border_1()
                .border_color(k.ink(0.14))
                .shadow_2xl()
        };
        if self.feedback_phase == FeedbackPhase::Sent {
            let reply = account
                .email
                .as_ref()
                .map(|e| format!(" and may reply to {e}"))
                .unwrap_or_default();
            return panel()
                .id("feedback-sent")
                .test_support()
                .role(Role::Status)
                .aria_label("Thank you")
                .items_center()
                .gap_1p5()
                .px_6()
                .py_7()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size_12()
                        .mb_1p5()
                        .rounded_full()
                        .bg(k.success_text.opacity(0.14))
                        .child(
                            Icon::new(KairoIcon::PhCheckCircle.fill())
                                .size_6()
                                .text_color(k.success_text),
                        ),
                )
                .child(
                    div()
                        .text_size(text::BASE.size)
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(k.text_primary)
                        .child("Thank you"),
                )
                .child(
                    div()
                        .text_center()
                        .text_size(text::XS.size)
                        .line_height(text::XS.line_height)
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
        let who = account.name.clone().or(account.email.clone());
        let kinds = [
            (
                FeedbackKind::Idea,
                "Idea",
                KairoIcon::PhLightbulb,
                Hue::Amber,
            ),
            (
                FeedbackKind::Problem,
                "Problem",
                KairoIcon::PhWarningCircle,
                Hue::Coral,
            ),
            (
                FeedbackKind::Praise,
                "Praise",
                KairoIcon::PhHeart,
                Hue::Pink,
            ),
        ];
        panel()
            .id("feedback-form")
            .key_context("FeedbackForm")
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
                    .pt_4()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(text::BASE.size)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child("Send feedback"),
                    )
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child("The Kairo team reads every note."),
                    ),
            )
            .child(
                v_flex()
                    .px_4()
                    .pt_3p5()
                    .pb_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .id("feedback-kind")
                            .role(Role::RadioGroup)
                            .aria_label("Kind of feedback")
                            .w_full()
                            .gap_2()
                            .children(kinds.map(|(kind, label, icon, hue)| {
                                let selected = self.feedback_kind == kind;
                                let tone = k.hue(hue);
                                v_flex()
                                    .id(SharedString::from(format!("feedback-{}", kind.as_str())))
                                    .test_support()
                                    .role(Role::Button)
                                    .aria_label(label)
                                    .aria_selected(selected)
                                    .flex_1()
                                    .items_center()
                                    .gap_1()
                                    .py_2p5()
                                    .rounded(radius::lg())
                                    .cursor_pointer()
                                    .border_1()
                                    .text_size(text::LABEL.size)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .when(selected, |d| {
                                        d.bg(tone.opacity(0.12))
                                            .border_color(tone.opacity(0.55))
                                            .text_color(k.text_primary)
                                    })
                                    .when(!selected, |d| {
                                        d.bg(k.ink(0.03))
                                            .border_color(k.ink(0.08))
                                            .text_color(k.text(TextStep::T400))
                                            .hover(|s| s.bg(k.ink(0.06)).text_color(k.text_primary))
                                    })
                                    .child(Icon::new(icon.weight(selected)).size_4().text_color(
                                        if selected {
                                            tone
                                        } else {
                                            k.text(TextStep::T400)
                                        },
                                    ))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.feedback_kind = kind;
                                        this.account_pinned = true;
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
                            .rounded(radius::lg())
                            .bg(k.surface)
                            .border_1()
                            .border_color(k.ink(0.12))
                            .child(
                                Textarea::new(&self.feedback_input)
                                    .appearance(false)
                                    .aria_label("Your feedback")
                                    .disabled(!signed_in || sending),
                            ),
                    )
                    .child(
                        div()
                            .mt_neg_1()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(crate::shell::hint(self.feedback_kind)),
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
                            .items_center()
                            .child(match (&who, signed_in) {
                                (Some(who), true) => h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_2()
                                    .items_center()
                                    .child(avatar(who, false, cx))
                                    .child(
                                        v_flex()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_size(text::MICRO.size)
                                                    .text_color(k.text(TextStep::T500))
                                                    .child("Sent as"),
                                            )
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(text::XS.size)
                                                    .text_color(k.text(TextStep::T200))
                                                    .child(who.clone()),
                                            ),
                                    )
                                    .into_any_element(),
                                _ => div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(text::XS.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child("Sign in to send feedback")
                                    .into_any_element(),
                            })
                            .child(
                                Button::new("feedback-send")
                                    .cursor_pointer()
                                    .primary()
                                    .icon(Icon::new(KairoIcon::Send))
                                    .label(if sending { "Sending…" } else { "Send" })
                                    .tooltip("Send (⌘↵)")
                                    .disabled(!signed_in || !has_text || sending)
                                    .on_click(cx.listener(|this, _, _, cx| this.send_feedback(cx))),
                            ),
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
                    // The page settles in on each switch: the animation is keyed by
                    // the route, so coming back to a page replays it.
                    .child(kairo_ui_kit::motion::travel_in(
                        div()
                            .min_h_0()
                            .w_full()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(content),
                        SharedString::from(format!("workspace-in-{}", route.as_str())),
                        kairo_ui_kit::motion::From::Below,
                        4.0,
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// Builds a workspace inside a panic boundary.
    fn contained(&mut self, route: Route, cx: &mut Context<Self>) -> AnyElement {
        if let Some((_, message, _)) = self.crashes.get(&route).filter(|(_, _, failed)| *failed) {
            return self.fallback(route, message.clone(), cx);
        }
        if let Some(view) = self.workspaces.get(&route) {
            let hosts_dock = matches!(
                route,
                Route::Operator | Route::Scripture | Route::Lyrics | Route::Documents
            );
            return match self.media_dock.clone().filter(|_| !hosts_dock) {
                Some(dock) => v_flex()
                    .size_full()
                    .min_h_0()
                    .child(v_flex().flex_1().min_h_0().child(view.clone()))
                    .child(dock)
                    .into_any_element(),
                None => view.clone().into_any_element(),
            };
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
        let Some(overlay) = self.overlay else {
            let (view, _) = self.overlay_leaving.as_ref()?;
            return Some(kairo_ui_kit::motion::fade_out(
                div().absolute().inset_0().occlude().child(view.clone()),
                "overlay-leaving",
                cx,
            ));
        };
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
        let recording = self.state.recording;
        let detail = if recording {
            "Your screens go offline, live transcription stops, and the recording is stopped and saved."
        } else {
            "Your screens go offline and live transcription stops."
        };
        // A key hint inside a button: `esc`, `↩`.
        let hint = |text_: &'static str, ink: gpui_kit::Hsla| {
            div()
                .flex_none()
                .px_1()
                .rounded(radius::sm())
                .border_1()
                .border_color(ink.opacity(0.35))
                .text_size(text::MICRO.size)
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(ink.opacity(0.75))
                .child(text_)
        };
        let quit_ink = k.white;
        Some(kairo_ui_kit::motion::enter(
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
                .child(kairo_ui_kit::motion::pop_in(
                    v_flex()
                        .id("quit-confirmation")
                        .test_support()
                        .role(Role::AlertDialog)
                        .aria_label("Quit Kairo?")
                        .w_full()
                        .max_w(gpui_kit::rems(28.))
                        .p_6()
                        .gap_5()
                        .rounded(radius::xl2())
                        .bg(k.surface_secondary)
                        .border_1()
                        .border_color(k.ink(0.12))
                        .shadow_2xl()
                        .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.respond_to_quit(false, cx);
                            }
                        }))
                        .child(
                            v_flex()
                                .gap_2()
                                .child(div().mb_2().child(KairoMark::new(MarkSize::Small)))
                                .child(
                                    div()
                                        .text_size(text::LG.size)
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(k.text_primary)
                                        .child("Quit Kairo?"),
                                )
                                .child(
                                    div()
                                        .text_size(text::SM.size)
                                        .line_height(text::SM.line_height)
                                        .text_color(k.text(TextStep::T400))
                                        .child(detail),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                // Not offered while recording: that always asks.
                                .when(!recording, |this| {
                                    this.child(
                                        Button::new("always-quit")
                                            .cursor_pointer()
                                            .secondary()
                                            .label("Always quit")
                                            .tooltip("Quit without asking from now on (it still asks while recording)")
                                            .on_click(cx.listener(|this, _, _, cx| this.always_quit(cx))),
                                    )
                                })
                                .child(div().flex_1())
                                .child(
                                    Button::new("keep-open")
                                        .cursor_pointer()
                                        // Focus starts here, not on Quit: a stray
                                        // Return mid-service must not take the
                                        // screens offline.
                                        .track_focus(&self.keep_open_focus)
                                        .secondary()
                                        .accessibility_label("Cancel")
                                        .on_click(cx.listener(|this, _, _, cx| this.respond_to_quit(false, cx)))
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child("Cancel")
                                                .child(hint("esc", k.text(TextStep::T300))),
                                        ),
                                )
                                .child(
                                    Button::new("quit-kairo")
                                        .cursor_pointer()
                                        .custom(
                                            ButtonCustomVariant::new(cx)
                                                .color(k.destructive)
                                                .foreground(quit_ink)
                                                .hover(k.destructive.opacity(0.88))
                                                .active(k.destructive.opacity(0.88)),
                                        )
                                        // Solid fill: the custom variant paints its colour at 20%.
                                        .bg(k.destructive)
                                        .accessibility_label("Quit")
                                        .on_click(cx.listener(|this, _, _, cx| this.respond_to_quit(true, cx)))
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child("Quit"),
                                        ),
                                ),
                        )
                        .focus_trap("quit-confirmation", self.quit_focus.handle()),
                    "quit-dialog-in",
                    kairo_ui_kit::motion::From::Below,
                    cx,
                )),
            "quit-enter",
            cx,
        ))
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

/// The surface of a header popover drawn by the shell, so it can animate
/// as one piece: the same as the rail's menus.
fn header_popover_surface(cx: &App) -> gpui_kit::Div {
    let k = *cx.kairo();
    div()
        .rounded(radius::xl())
        .bg(k.surface_secondary)
        .border_1()
        .border_color(k.ink(0.14))
        .shadow_2xl()
}

/// What a row in the account menu does when clicked.
type MenuClick = Box<dyn Fn(&mut Window, &mut App)>;

/// A round initial for the signed-in person; `large` in the menu header.
fn avatar(title: &str, large: bool, cx: &App) -> AnyElement {
    let k = *cx.kairo();
    let initials: String = title
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .when(large, |d| d.size_9())
        .when(!large, |d| d.size_8())
        .rounded_full()
        .bg(k.hue(Hue::Blue).opacity(0.28))
        .border_1()
        .border_color(k.hue(Hue::Blue).opacity(0.45))
        .text_color(k.text_primary)
        .text_size(if large { text::SM.size } else { text::XS.size })
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .child(initials)
        .into_any_element()
}
