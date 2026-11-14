//! General: appearance, lyrics gloss colour, the Kairo folder, the
//! PowerPoint converter, privacy switches and software updates.

use crate::backend::UpdateAction;
use crate::model::{
    Section, converter_busy, converter_hint, gloss_or_default, hex_to_rgb, migration_hint,
    resync_note, update_label,
};
use crate::widgets;
use crate::window::SettingsWindow;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Role, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px, rgb,
};
use kairo_domain::documents::SlideConverterState;
use kairo_domain::settings::DisplayTheme;
use kairo_domain::status::UpdateStatus;
use kairo_domain::workspace::{MediaFolderMigration, WorkspaceInfo};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

/// Element ids, for tests.
pub mod ids {
    /// Dark mode.
    pub const DARK: &str = "settings-dark-mode";
    /// Interface size.
    pub const FONT_SIZE: &str = "settings-font-size";
    /// Transcription size.
    pub const TRANSCRIPT_SIZE: &str = "settings-transcript-size";
    /// Gloss hex.
    pub const GLOSS: &str = "settings-gloss";
    /// The folder path.
    pub const FOLDER: &str = "settings-workspace-folder";
    /// The workspace error.
    pub const FOLDER_ERROR: &str = "settings-workspace-error";
    /// Show folder.
    pub const REVEAL: &str = "settings-workspace-reveal";
    /// Open Songs.
    pub const REVEAL_SONGS: &str = "settings-workspace-songs";
    /// Move….
    pub const MOVE: &str = "settings-workspace-move";
    /// Point elsewhere….
    pub const POINT: &str = "settings-workspace-point";
    /// Rescan.
    pub const RESCAN: &str = "settings-workspace-rescan";
    /// Move them in.
    pub const ADOPT_MOVE: &str = "settings-media-move";
    /// Use the Kairo folder anyway.
    pub const ADOPT_SWITCH: &str = "settings-media-switch";
    /// Keep as is.
    pub const ADOPT_KEEP: &str = "settings-media-keep";
    /// The note under Storage.
    pub const NOTE: &str = "settings-workspace-note";
    /// The converter's action.
    pub const CONVERTER: &str = "settings-converter-action";
    /// The converter's hint.
    pub const CONVERTER_HINT: &str = "settings-converter-hint";
    /// Share usage statistics.
    pub const SHARE: &str = "settings-share-stats";
    /// Send crash reports.
    pub const CRASH: &str = "settings-crash-reports";
    /// The update action.
    pub const UPDATE: &str = "settings-update-action";
    /// The update hint.
    pub const UPDATE_HINT: &str = "settings-update-hint";
}

/// What the Storage group is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceBusy {
    /// Moving or pointing elsewhere.
    Change,
    /// Re-reading Songs.
    Resync,
    /// Adopting backgrounds.
    Adopt,
}

/// The Storage group's state (`WorkspaceGroup`).
#[derive(Clone, Debug, Default)]
pub struct WorkspacePane {
    /// `workspace.get`.
    pub info: Option<WorkspaceInfo>,
    /// `workspace.mediaMigration`; `None` after "Keep as is".
    pub migration: Option<MediaFolderMigration>,
    /// The running action.
    pub busy: Option<WorkspaceBusy>,
    /// The last result or error.
    pub note: Option<String>,
}

impl SettingsWindow {
    pub(crate) fn load_workspace(&mut self, cx: &mut Context<Self>) {
        let request = self.backend.workspace();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((info, migration)) => {
                        this.workspace.info = Some(info);
                        this.workspace.migration = Some(migration);
                    }
                    Err(error) => this.workspace.note = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn load_converter(&mut self, cx: &mut Context<Self>) {
        let request = self.backend.converter_status();
        cx.spawn(async move |this, cx| {
            let status = request.await;
            let _ = this.update(cx, |this, cx| {
                // A pushed status may already have arrived.
                if this.converter.is_none() {
                    this.converter = Some(status);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Move… (`move` true) or Point elsewhere….
    pub fn change_workspace(&mut self, move_contents: bool, cx: &mut Context<Self>) {
        if self.workspace.busy.is_some() {
            return;
        }
        self.workspace.busy = Some(WorkspaceBusy::Change);
        self.workspace.note = None;
        let request = self.backend.choose_workspace(move_contents);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.workspace.busy = None;
                match result {
                    // The picker was cancelled: nothing changed.
                    Ok(None) => {}
                    Ok(Some(info)) => {
                        this.workspace.info = Some(info);
                        this.workspace.note = Some(if move_contents {
                            "Workspace moved.".into()
                        } else {
                            "Workspace folder changed.".into()
                        });
                        this.load_workspace(cx);
                    }
                    Err(error) => this.workspace.note = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Rescan.
    pub fn resync_songs(&mut self, cx: &mut Context<Self>) {
        if self.workspace.busy.is_some() {
            return;
        }
        self.workspace.busy = Some(WorkspaceBusy::Resync);
        let request = self.backend.resync_songs();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.workspace.busy = None;
                this.workspace.note = Some(match result {
                    Ok(sync) => resync_note(&sync),
                    Err(error) => error,
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Move them in (`move` true) or Use the Kairo folder anyway.
    pub fn adopt_media(&mut self, move_contents: bool, cx: &mut Context<Self>) {
        if self.workspace.busy.is_some() {
            return;
        }
        self.workspace.busy = Some(WorkspaceBusy::Adopt);
        let request = self.backend.adopt_media(move_contents);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.workspace.busy = None;
                match result {
                    Ok(()) => {
                        this.workspace.note = Some(if move_contents {
                            "Backgrounds moved into the workspace.".into()
                        } else {
                            "Backgrounds folder switched to the workspace.".into()
                        });
                        this.load_workspace(cx);
                    }
                    Err(error) => this.workspace.note = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reveal(&mut self, songs: bool, cx: &mut Context<Self>) {
        let request = self.backend.reveal_workspace(songs);
        cx.spawn(async move |_, _| {
            if let Err(error) = request.await {
                tracing::warn!(%error, "[Settings] could not reveal the workspace");
            }
        })
        .detach();
    }

    /// An update action. A busy check, or one already running, is ignored.
    pub fn run_update(&mut self, action: UpdateAction, cx: &mut Context<Self>) {
        if self.update_busy {
            return;
        }
        self.update_busy = true;
        self.backend.update(action);
        cx.notify();
    }

    pub(crate) fn render_general(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let dark = self.settings.display.theme == DisplayTheme::Dark;
        let entity = cx.entity();
        let size_row = |id: &'static str,
                        label: &'static str,
                        hint: Option<&'static str>,
                        state,
                        value: f64,
                        cx: &mut Context<Self>| {
            widgets::stacked(
                label,
                hint,
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .child(Slider::new(state).accessibility_label(label)),
                    )
                    .child(
                        div()
                            .id(id)
                            .w_9()
                            .text_right()
                            .font_family(text::MONO_FAMILY)
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child(crate::model::number_text(value)),
                    ),
                cx,
            )
        };
        let appearance = widgets::group(
            Some("Appearance"),
            None,
            vec![
                widgets::row(
                    "Dark mode",
                    Some(if dark {
                        "Dim room lighting"
                    } else {
                        "Bright rooms — easier to see during a service"
                    }),
                    widgets::toggle(ids::DARK, "Dark mode", dark, {
                        let entity = entity.clone();
                        move |on, window, cx| {
                            entity.update(cx, |this, cx| this.set_dark(on, window, cx))
                        }
                    }),
                    cx,
                ),
                size_row(
                    ids::FONT_SIZE,
                    "Interface size",
                    None,
                    &self.font_size.clone(),
                    self.settings.display.font_size,
                    cx,
                ),
                size_row(
                    ids::TRANSCRIPT_SIZE,
                    "Transcription size",
                    Some("Live transcript during a service"),
                    &self.transcript_size.clone(),
                    self.settings.display.transcription_font_size,
                    cx,
                ),
            ],
            cx,
        );

        // The swatch and preview draw the operator's colour, not a token.
        let gloss = gloss_or_default(&self.settings.lyrics.gloss_color).to_owned();
        // Always valid: `gloss_or_default` falls back to the default colour.
        let gloss_rgb = hex_to_rgb(&gloss).unwrap_or_default();
        let lyrics = widgets::group(
            Some("Lyrics"),
            None,
            vec![widgets::stacked(
                "Gloss color",
                Some("Parenthetical translation lines"),
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .h_7()
                                    .w_9()
                                    .rounded(radius::base())
                                    .border_1()
                                    .border_color(k.surface_border)
                                    .bg(rgb(gloss_rgb)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h_7()
                                    .rounded(radius::base())
                                    .bg(k.surface_tertiary)
                                    .px_2()
                                    .font_family(text::MONO_FAMILY)
                                    .child(
                                        Input::new(&self.gloss)
                                            .id(ids::GLOSS)
                                            .appearance(false)
                                            .small()
                                            .aria_label("Gloss color hex"),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .items_center()
                            .rounded(radius::md())
                            .bg(k.monitor())
                            .px_3()
                            .py_3()
                            .child(
                                div()
                                    .text_size(text::SM.size)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .text_color(k.white)
                                    .child("Onye nke di ike n’aka Ya"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_size(text::XS.size)
                                    .italic()
                                    .text_color(rgb(gloss_rgb))
                                    .child("(The arm of the Lord does great things)"),
                            ),
                    ),
                cx,
            )],
            cx,
        );

        let storage = self.render_storage(cx);
        let converter = self.render_converter(cx);

        let share = self.settings.usage.share_stats;
        let crash = self.settings.usage.send_crash_reports != Some(false);
        let privacy = widgets::group(
            Some("Privacy"),
            None,
            vec![
                widgets::row(
                    "Share usage statistics",
                    Some(
                        "Counts of the features you use and basic system info, so we can improve Kairo. Never your songs, sermons, verses or files.",
                    ),
                    widgets::toggle(ids::SHARE, "Share usage statistics", share, {
                        let entity = entity.clone();
                        move |on, _, cx| {
                            entity.update(cx, |this, cx| this.set_usage(Some(on), None, cx))
                        }
                    }),
                    cx,
                ),
                widgets::row(
                    "Send crash reports",
                    Some(
                        "If Kairo crashes, send what failed and the end of its log so we can fix it. The log can name songs and files.",
                    ),
                    widgets::toggle(ids::CRASH, "Send crash reports", crash, {
                        let entity = entity.clone();
                        move |on, _, cx| {
                            entity.update(cx, |this, cx| this.set_usage(None, Some(on), cx))
                        }
                    }),
                    cx,
                ),
            ],
            cx,
        );

        let updates = self.render_updates(cx);
        let saved = self.saved == Some(Section::General);
        v_flex()
            .gap_5()
            .child(appearance)
            .child(lyrics)
            .child(storage)
            .child(converter)
            .child(privacy)
            .child(updates)
            .children(
                self.save_error
                    .clone()
                    .map(|e| widgets::notice("settings-save-error", e, cx)),
            )
            .child(widgets::save_bar(
                Section::General,
                saved,
                cx.listener(|this, _, _, cx| this.save(Section::General, cx)),
                cx,
            ))
            .into_any_element()
    }

    fn render_storage(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let busy = self.workspace.busy;
        let info = self.workspace.info.clone();
        let mut rows = vec![widgets::stacked(
            "Kairo folder",
            Some("Songs and backgrounds live here"),
            v_flex()
                .gap_2()
                .child(
                    div()
                        .id(ids::FOLDER)
                        .test_support()
                        .rounded(radius::md())
                        .bg(k.surface)
                        .px_3()
                        .py_2()
                        .font_family(text::MONO_FAMILY)
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T300))
                        .child(info.as_ref().map_or_else(
                            || "Locating…".to_owned(),
                            |i| i.root.clone(),
                        )),
                )
                .when_some(
                    info.as_ref()
                        .filter(|i| !i.ready)
                        .map(|i| i.error.clone().unwrap_or_default()),
                    |this, error| {
                        this.child(
                            div()
                                .id(ids::FOLDER_ERROR)
                                .test_support()
                                .role(Role::Alert)
                                .text_size(text::LABEL.size)
                                .text_color(k.danger_text)
                                .child(format!("This folder could not be created — {error}")),
                        )
                    },
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new(ids::REVEAL)
                                .small()
                                .icon(Icon::new(KairoIcon::FolderOpen))
                                .label("Show folder")
                                .on_click(cx.listener(|this, _, _, cx| this.reveal(false, cx))),
                        )
                        .child(widgets::button(
                            ids::REVEAL_SONGS,
                            "Open Songs",
                            false,
                            cx.listener(|this, _, _, cx| this.reveal(true, cx)),
                        ))
                        .child(widgets::button(
                            ids::MOVE,
                            if busy == Some(WorkspaceBusy::Change) {
                                "Moving…"
                            } else {
                                "Move…"
                            },
                            busy.is_some(),
                            cx.listener(|this, _, _, cx| this.change_workspace(true, cx)),
                        ))
                        .child(widgets::button(
                            ids::POINT,
                            "Point elsewhere…",
                            busy.is_some(),
                            cx.listener(|this, _, _, cx| this.change_workspace(false, cx)),
                        )),
                )
                .child(
                    div()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Move takes the songs and backgrounds with it. Point elsewhere leaves the files behind and reads whatever is already in the new folder."),
                ),
            cx,
        )];
        rows.push(widgets::row(
            "Re-read Songs folder",
            Some("After adding, editing or deleting song files outside Kairo"),
            widgets::button(
                ids::RESCAN,
                if busy == Some(WorkspaceBusy::Resync) {
                    "Reading…"
                } else {
                    "Rescan"
                },
                busy.is_some(),
                cx.listener(|this, _, _, cx| this.resync_songs(cx)),
            ),
            cx,
        ));
        if let Some(migration) = self.workspace.migration.clone().filter(|m| m.needed) {
            rows.push(widgets::stacked(
                "Backgrounds are outside the Kairo folder",
                Some(migration_hint(&migration)),
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new(ids::ADOPT_MOVE)
                            .small()
                            .primary()
                            .label(if busy == Some(WorkspaceBusy::Adopt) {
                                "Moving…"
                            } else {
                                "Move them in"
                            })
                            .disabled(busy.is_some())
                            .on_click(cx.listener(|this, _, _, cx| this.adopt_media(true, cx))),
                    )
                    .child(widgets::button(
                        ids::ADOPT_SWITCH,
                        "Use the Kairo folder anyway",
                        busy.is_some(),
                        cx.listener(|this, _, _, cx| this.adopt_media(false, cx)),
                    ))
                    .child(widgets::button(
                        ids::ADOPT_KEEP,
                        "Keep as is",
                        false,
                        cx.listener(|this, _, _, cx| {
                            this.workspace.migration = None;
                            cx.notify();
                        }),
                    )),
                cx,
            ));
        }
        if let Some(note) = self.workspace.note.clone() {
            rows.push(
                div()
                    .id(ids::NOTE)
                    .test_support()
                    .role(Role::Status)
                    .px(px(14.))
                    .pb_2()
                    .text_size(text::LABEL.size)
                    .text_color(k.notice_text)
                    .child(note)
                    .into_any_element(),
            );
        }
        widgets::group(Some("Storage"), None, rows, cx)
    }

    fn render_converter(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let status = self.converter.clone();
        let busy = converter_busy(status.as_ref());
        let hint: SharedString = converter_hint(status.as_ref()).into();
        let control: AnyElement = match &status {
            Some(s) if busy => {
                let percent = if s.size_bytes > 0.0 {
                    (s.received_bytes / s.size_bytes * 100.0).clamp(0.0, 100.0)
                } else {
                    0.0
                };
                let label = if s.state == SlideConverterState::Installing {
                    "Installing…".to_owned()
                } else {
                    format!(
                        "Downloading · {} of {}",
                        crate::model::format_mb(s.received_bytes),
                        crate::model::format_mb(s.size_bytes)
                    )
                };
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .id("settings-converter-progress")
                                    .test_support()
                                    .role(Role::ProgressIndicator)
                                    .aria_label(label.clone())
                                    .h_1()
                                    .rounded_full()
                                    .bg(k.surface_border)
                                    .child(
                                        div()
                                            .h_full()
                                            .rounded_full()
                                            .bg(k.accent(kairo_ui_kit::tokens::Step::S400))
                                            .w(gpui_kit::relative(percent as f32 / 100.0)),
                                    ),
                            )
                            .child(widgets::muted(label, cx)),
                    )
                    .child(widgets::button(
                        ids::CONVERTER,
                        "Stop",
                        false,
                        cx.listener(|this, _, _, _| this.backend.converter_cancel()),
                    ))
                    .into_any_element()
            }
            Some(s) if s.state == SlideConverterState::Ready => widgets::button(
                ids::CONVERTER,
                "Remove",
                false,
                cx.listener(|this, _, _, _| this.backend.converter_remove()),
            )
            .into_any_element(),
            Some(s) if s.state != SlideConverterState::Unsupported => Button::new(ids::CONVERTER)
                .small()
                .primary()
                .icon(Icon::new(KairoIcon::Download))
                .label(if s.state == SlideConverterState::Error {
                    "Try again"
                } else {
                    "Download"
                })
                .on_click(cx.listener(|this, _, _, _| this.backend.converter_install()))
                .into_any_element(),
            _ => div().into_any_element(),
        };
        let row = if busy {
            widgets::stacked("Slide converter", None::<SharedString>, control, cx)
        } else {
            widgets::row("Slide converter", None::<SharedString>, control, cx)
        };
        let hint_row = (!busy).then(|| {
            div()
                .id(ids::CONVERTER_HINT)
                .test_support()
                .px(px(14.))
                .pb_2()
                .text_size(text::LABEL.size)
                .text_color(
                    if status
                        .as_ref()
                        .is_some_and(|s| s.state == SlideConverterState::Error)
                    {
                        k.danger_text
                    } else {
                        k.text(TextStep::T500)
                    },
                )
                .child(hint)
                .into_any_element()
        });
        let mut rows = vec![row];
        rows.extend(hint_row);
        widgets::group(Some("PowerPoint"), None, rows, cx)
    }

    fn render_updates(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let label = update_label(&self.update);
        let button = match &self.update {
            UpdateStatus::Available { .. } => widgets::button(
                ids::UPDATE,
                "Download",
                false,
                cx.listener(|this, _, _, cx| this.run_update(UpdateAction::Download, cx)),
            ),
            UpdateStatus::Downloaded { .. } => widgets::button(
                ids::UPDATE,
                "Restart & install",
                false,
                cx.listener(|this, _, _, cx| this.run_update(UpdateAction::Install, cx)),
            ),
            status => widgets::button(
                ids::UPDATE,
                "Check now",
                self.update_busy
                    || matches!(
                        status,
                        UpdateStatus::Checking | UpdateStatus::Downloading { .. }
                    ),
                cx.listener(|this, _, _, cx| this.run_update(UpdateAction::Check, cx)),
            ),
        };
        widgets::group(
            Some("Software update"),
            None,
            vec![widgets::row(
                "Updates",
                Some(label),
                div().id(ids::UPDATE_HINT).child(button),
                cx,
            )],
            cx,
        )
    }
}
