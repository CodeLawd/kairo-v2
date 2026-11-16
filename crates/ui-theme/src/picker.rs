//! The header's Screen Themes popover (`ThemePicker.tsx`, prompt 087): a
//! saved-theme switcher for one screen or feed at a time. Authoring stays
//! in the Theme editor. Picking a theme changes only that screen's theme
//! for the kind shown; defaults and other screens are left alone.

use kairo_ui_kit::components::PopoverTriggerExt;
use std::cell::RefCell;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Render, Role,
    SharedString, StatefulInteractiveElement, Styled, Task, Window, div, px,
};
use kairo_domain::overlay::{CustomOverlayTheme, OverlayContentKind, OverlayOutput};
use kairo_domain::overlay_outputs::is_rendered_kind;
use kairo_domain::settings::AppSettings;
use kairo_domain::theme_library::{output_uses_theme, themes_for_kind};
use kairo_ui_kit::components::{SegmentOption, SegmentedControl};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::{FeedSink, Listening, ThemeBackend, ThemeFeed};
use crate::editor::ThumbCache;
use crate::ops::Op;

pub mod ids {
    pub const ROOT: &str = "theme-picker";
    pub const SCREEN: &str = "theme-picker-screen";
    pub const KIND: &str = "theme-picker-kind";
    pub const STATUS: &str = "theme-picker-status";
    pub const ALERT: &str = "theme-picker-alert";
    pub const EMPTY: &str = "theme-picker-empty";
    pub fn apply(id: &str) -> String {
        format!("theme-picker-apply-{id}")
    }
}

/// The failure message (`ThemePicker`'s catch).
pub const APPLY_FAILED: &str = "Theme could not be applied. Try again.";

pub struct ThemePicker {
    backend: Arc<dyn ThemeBackend>,
    settings: AppSettings,
    kind: OverlayContentKind,
    screen_id: Option<String>,
    saving: bool,
    error: Option<String>,
    applied: Option<String>,
    thumbs: RefCell<ThumbCache>,
    _feed: Task<()>,
    _listening: Listening,
}

impl ThemePicker {
    pub fn new(backend: Arc<dyn ThemeBackend>, cx: &mut Context<Self>) -> Self {
        let settings = backend.snapshot();
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<ThemeFeed>();
        let sink: FeedSink = Arc::new(move |feed| {
            let _ = tx.unbounded_send(feed);
        });
        let listening = backend.listen(sink);
        let feed = cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(ThemeFeed::Settings(settings)) = rx.next().await {
                if this
                    .update(cx, |this, cx| {
                        if !this.saving {
                            this.settings = *settings;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            backend,
            settings,
            kind: OverlayContentKind::Scripture,
            screen_id: None,
            saving: false,
            error: None,
            applied: None,
            thumbs: RefCell::default(),
            _feed: feed,
            _listening: listening,
        }
    }

    /// The popover opened: Lyrics on the Lyrics page, otherwise Scripture,
    /// and no message from last time.
    pub fn opened(&mut self, lyrics: bool, cx: &mut Context<Self>) {
        self.kind = if lyrics {
            OverlayContentKind::Lyrics
        } else {
            OverlayContentKind::Scripture
        };
        self.error = None;
        self.applied = None;
        if !self.saving {
            self.settings = self.backend.snapshot();
        }
        cx.notify();
    }

    /// The screens and feeds that draw a picture.
    pub fn outputs(&self) -> Vec<&OverlayOutput> {
        self.settings
            .overlay
            .outputs
            .iter()
            .filter(|o| is_rendered_kind(o.kind))
            .collect()
    }

    /// The chosen screen, or the first one.
    pub fn screen(&self) -> Option<&OverlayOutput> {
        let outputs = self.outputs();
        self.screen_id
            .as_deref()
            .and_then(|id| outputs.iter().find(|o| o.id == id).copied())
            .or_else(|| outputs.first().copied())
    }

    pub fn kind(&self) -> OverlayContentKind {
        self.kind
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn applied(&self) -> Option<&str> {
        self.applied.as_deref()
    }

    pub fn saving(&self) -> bool {
        self.saving
    }

    pub fn set_kind(&mut self, kind: OverlayContentKind, cx: &mut Context<Self>) {
        self.kind = kind;
        self.error = None;
        self.applied = None;
        cx.notify();
    }

    pub fn set_screen(&mut self, id: &str, cx: &mut Context<Self>) {
        self.screen_id = Some(id.to_owned());
        self.error = None;
        self.applied = None;
        cx.notify();
    }

    /// Gives `entry` to the chosen screen. One at a time.
    pub fn apply(&mut self, theme_id: &str, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let Some(screen) = self.screen() else {
            return;
        };
        let Some(entry) = self
            .settings
            .theme_library
            .iter()
            .find(|t| t.id == theme_id)
            .cloned()
        else {
            return;
        };
        let (screen_id, screen_name) = (screen.id.clone(), screen.name.clone());
        self.saving = true;
        self.error = None;
        self.applied = None;
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.edit(
            Op::Assign {
                theme_id: entry.id.clone(),
                output_id: screen_id,
            },
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The write was dropped.".into()));
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(()) => {
                        this.applied = Some(format!("{} applied to {}", entry.name, screen_name))
                    }
                    Err(error) => {
                        tracing::error!(%error, "[Theme] could not apply a theme");
                        this.error = Some(APPLY_FAILED.into());
                    }
                }
                this.settings = this.backend.snapshot();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn render_tile(
        &self,
        entry: &CustomOverlayTheme,
        selected: bool,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let id = entry.id.clone();
        let theme = crate::model::normalized(&entry.theme);
        div()
            .id(SharedString::from(ids::apply(&entry.id)))
            .test_support()
            .role(Role::Button)
            .aria_label(format!("Apply {}", entry.name))
            .aria_selected(selected)
            .w(px(162.))
            .overflow_hidden()
            .rounded(radius::md())
            .border_1()
            .border_color(if selected { k.white } else { k.surface_border })
            // Disabled: a press from assistive technology does nothing, as the pointer does.
            .when(disabled, |d| d.opacity(0.5).on_click(|_, _, _| {}))
            .when(!disabled, |d| {
                d.cursor_pointer()
                    .hover(|s| s.border_color(k.text(TextStep::T400)))
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        this.update(cx, |t, cx| t.apply(&id, cx))
                    })
            })
            .child(crate::view::slide_picture(
                &theme,
                entry.kind,
                160.0,
                &k,
                &self.thumbs,
            ))
            .child(
                h_flex()
                    .gap_1()
                    .px_2()
                    .py_1p5()
                    .bg(k.surface_tertiary)
                    .text_size(text::XS.size)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(entry.name.clone()),
                    )
                    .when(selected, |d| d.child(Icon::new(KairoIcon::Check).size_3())),
            )
            .into_any_element()
    }
}

impl Render for ThemePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let kind_word = match self.kind {
            OverlayContentKind::Scripture => "scripture",
            OverlayContentKind::Lyrics => "lyrics",
        };
        let screen = self.screen().cloned();
        let outputs: Vec<(String, String)> = self
            .outputs()
            .into_iter()
            .map(|o| (o.id.clone(), o.name.clone()))
            .collect();
        let entries: Vec<CustomOverlayTheme> =
            themes_for_kind(&self.settings.theme_library, self.kind)
                .into_iter()
                .cloned()
                .collect();
        let saving = self.saving;

        let screen_select = {
            let this = this.clone();
            let current = screen.as_ref().map(|s| s.id.clone());
            let options = outputs.clone();
            Button::new(ids::SCREEN)
                .cursor_pointer()
                .small()
                .w_full()
                .dropdown_caret(true)
                .accessibility_label("Theme screen")
                .label(
                    screen
                        .as_ref()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "No screens configured".into()),
                )
                .disabled(saving || outputs.is_empty())
                .popover_trigger()
                .dropdown_menu(move |mut menu, _, _| {
                    for (id, name) in &options {
                        let (this, id) = (this.clone(), id.clone());
                        menu = menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(current.as_deref() == Some(id.as_str()))
                                .on_click(move |_, _, cx| {
                                    let id = id.clone();
                                    this.update(cx, |t, cx| t.set_screen(&id, cx))
                                }),
                        );
                    }
                    menu
                })
        };
        let kinds = {
            let this = this.clone();
            SegmentedControl::new(ids::KIND, "Theme content")
                .fit()
                .options([
                    SegmentOption::new("scripture", "Scripture"),
                    SegmentOption::new("lyrics", "Lyrics"),
                ])
                .selected(kind_word)
                .on_change(move |v, _, cx| {
                    let kind = if v.as_ref() == "lyrics" {
                        OverlayContentKind::Lyrics
                    } else {
                        OverlayContentKind::Scripture
                    };
                    this.update(cx, |t, cx| t.set_kind(kind, cx))
                })
        };
        let note = match &screen {
            Some(s) => format!("Changes only {}’s {kind_word} theme.", s.name),
            None => "Add a screen or feed in Screens to choose its theme.".into(),
        };
        let tiles: Vec<AnyElement> = entries
            .iter()
            .map(|e| {
                let selected = screen
                    .as_ref()
                    .is_some_and(|s| output_uses_theme(s, &e.id, e.kind));
                self.render_tile(e, selected, saving || screen.is_none(), cx)
            })
            .collect();
        let muted = k.text(TextStep::T400);
        v_flex()
            .id(ids::ROOT)
            .test_support()
            .role(Role::Dialog)
            .aria_label("Screen Themes")
            .w(px(360.))
            .gap_2()
            .child(
                div()
                    .text_size(text::SM.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Screen Themes"),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .text_color(muted)
                            .child("Screen or feed"),
                    )
                    .child(screen_select),
            )
            .child(kinds)
            .child(div().text_size(text::XS.size).text_color(muted).child(note))
            .child(
                h_flex()
                    .id("theme-picker-grid")
                    .flex_wrap()
                    .gap_2()
                    .p_0p5()
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .children(tiles),
            )
            .when(entries.is_empty(), |d| {
                d.child(
                    div()
                        .id(ids::EMPTY)
                        .test_support()
                        .py_6()
                        .text_center()
                        .text_size(text::XS.size)
                        .text_color(muted)
                        .child(format!(
                            "No saved {kind_word} themes yet. Create one in the Theme Editor."
                        )),
                )
            })
            .child(
                div()
                    .id(ids::STATUS)
                    .test_support()
                    .role(Role::Status)
                    .text_size(text::XS.size)
                    .text_color(muted)
                    .child(if saving {
                        "Applying theme…".to_owned()
                    } else {
                        self.applied.clone().unwrap_or_default()
                    }),
            )
            .children(self.error.clone().map(|e| {
                div()
                    .id(ids::ALERT)
                    .test_support()
                    .role(Role::Alert)
                    .text_size(text::XS.size)
                    .text_color(k.danger_text)
                    .child(e)
            }))
    }
}
