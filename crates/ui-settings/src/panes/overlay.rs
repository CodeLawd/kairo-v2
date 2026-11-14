//! Verses on screen: translation label, verse numbers, the verse cap and a
//! test verse.

use crate::model::Section;
use crate::widgets;
use crate::window::{SettingsWindow, TestStatus};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Role, SharedString,
    StatefulInteractiveElement, Styled, div,
};
use kairo_ui_kit::tokens::{radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

/// Element ids, for tests.
pub mod ids {
    /// Show translation.
    pub const SHOW_TRANSLATION: &str = "settings-overlay-translation";
    /// Verse numbers.
    pub const VERSE_NUMBERS: &str = "settings-overlay-numbers";
    /// Max verses.
    pub const MAX_VERSES: &str = "settings-overlay-max";
    /// Send.
    pub const SEND: &str = "settings-overlay-send";
    /// Clear.
    pub const CLEAR: &str = "settings-overlay-clear";
    /// The test result.
    pub const RESULT: &str = "settings-overlay-result";
}

impl SettingsWindow {
    /// Send (`clear` false) or Clear.
    pub fn run_test(&mut self, clear: bool, cx: &mut Context<Self>) {
        if self.test.0 == TestStatus::Testing {
            return;
        }
        self.test = (
            TestStatus::Testing,
            if clear { "Clearing…" } else { "Sending…" }.into(),
        );
        let request = if clear {
            self.backend.clear_all()
        } else {
            self.backend.send_test()
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.test = match result {
                    Ok(true) => (
                        TestStatus::Ok,
                        if clear { "Cleared" } else { "Sent" }.into(),
                    ),
                    Ok(false) => (
                        TestStatus::Fail,
                        if clear {
                            "Clear failed — check logs"
                        } else {
                            "Nothing took the test verse — check Screens"
                        }
                        .into(),
                    ),
                    Err(error) => (
                        TestStatus::Fail,
                        if error.is_empty() {
                            "Unknown error".into()
                        } else {
                            error.into()
                        },
                    ),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn render_overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let ov = self.settings.overlay.clone();
        let entity = cx.entity();
        let testing = self.test.0 == TestStatus::Testing;
        let hint: SharedString = if self.test.1.is_empty() {
            "Sends John 3:16 to every screen that is on".into()
        } else {
            self.test.1.clone()
        };
        let group = widgets::group(
            Some("Verse text"),
            None,
            vec![
                widgets::row(
                    "Show translation",
                    Some("John 3:16 (KJV)"),
                    widgets::toggle(
                        ids::SHOW_TRANSLATION,
                        "Show translation",
                        ov.show_translation,
                        {
                            let entity = entity.clone();
                            move |on, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.settings.overlay.show_translation = on;
                                    cx.notify();
                                })
                            }
                        },
                    ),
                    cx,
                ),
                widgets::row(
                    "Verse numbers",
                    Some("On multi-verse pushes"),
                    widgets::toggle(
                        ids::VERSE_NUMBERS,
                        "Verse numbers",
                        ov.show_verse_numbers,
                        {
                            let entity = entity.clone();
                            move |on, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.settings.overlay.show_verse_numbers = on;
                                    cx.notify();
                                })
                            }
                        },
                    ),
                    cx,
                ),
                widgets::row(
                    "Max verses",
                    Some("0 sends the whole passage"),
                    div()
                        .w_16()
                        .h_7()
                        .rounded(radius::base())
                        .bg(k.surface_tertiary)
                        .px_2()
                        .font_family(text::MONO_FAMILY)
                        .child(
                            Input::new(&self.max_verses)
                                .id(ids::MAX_VERSES)
                                .appearance(false)
                                .small()
                                .aria_label("Maximum verses per overlay push"),
                        ),
                    cx,
                ),
                widgets::row(
                    "Test",
                    Some(hint.clone()),
                    v_flex()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .child(
                                    Button::new(ids::SEND)
                                        .small()
                                        .icon(Icon::new(KairoIcon::Send))
                                        .label("Send")
                                        .disabled(testing)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.run_test(false, cx)),
                                        ),
                                )
                                .child(
                                    Button::new(ids::CLEAR)
                                        .small()
                                        .icon(Icon::new(KairoIcon::Trash2))
                                        .label("Clear")
                                        .disabled(testing)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.run_test(true, cx)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .id(ids::RESULT)
                                .test_support()
                                .role(Role::Status)
                                .aria_label(hint)
                                .size_0(),
                        ),
                    cx,
                ),
            ],
            cx,
        );
        v_flex()
            .gap_5()
            .child(group)
            .children(
                self.save_error
                    .clone()
                    .map(|e| widgets::notice("settings-save-error", e, cx)),
            )
            .child(widgets::save_bar(
                Section::Overlay,
                self.saved == Some(Section::Overlay),
                cx.listener(|this, _, _, cx| this.save(Section::Overlay, cx)),
                cx,
            ))
            .into_any_element()
    }
}
