//! Keyboard Shortcuts (`KeyboardShortcutsSection.tsx`): click a binding,
//! press a combination. A combination another command uses is refused with
//! that command's name; Escape cancels; leaving the button cancels.

use crate::model::{record_shortcut, with_binding};
use crate::window::SettingsWindow;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Disableable, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Role, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder, px,
};
use kairo_domain::keyboard_shortcuts::{SHORTCUT_COMMANDS, bindings_for, shortcut_label};
use kairo_domain::settings::{AppSettings, ShortcutBindings, ShortcutCommand};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, radius, text};

/// Element ids, for tests.
pub mod ids {
    use kairo_domain::settings::ShortcutCommand;

    /// A command's binding button: `settings-shortcut-<command>`.
    pub fn binding(command: ShortcutCommand) -> String {
        format!("settings-shortcut-{}", command.as_str())
    }
    /// A command's Clear: `settings-shortcut-clear-<command>`.
    pub fn clear(command: ShortcutCommand) -> String {
        format!("settings-shortcut-clear-{}", command.as_str())
    }
    /// The conflict or save error.
    pub const ERROR: &str = "settings-shortcut-error";
    /// Restore defaults.
    pub const RESTORE: &str = "settings-shortcut-restore";
    /// Save shortcuts.
    pub const SAVE: &str = "settings-shortcut-save";
}

/// The pane's state.
pub struct ShortcutsPane {
    /// The bindings being edited.
    pub draft: ShortcutBindings,
    /// The command listening for keys.
    pub recording: Option<ShortcutCommand>,
    /// The conflict or save error.
    pub error: Option<String>,
    /// Saving.
    pub saving: bool,
    /// Saved since the last change.
    pub saved: bool,
    /// The key that just finished a recording also activates the button;
    /// that keyboard click is swallowed.
    swallow_key_click: bool,
    focus: Vec<(ShortcutCommand, FocusHandle)>,
}

impl ShortcutsPane {
    pub(crate) fn new(settings: &AppSettings, cx: &mut Context<SettingsWindow>) -> Self {
        Self {
            draft: settings.display.shortcuts.clone().unwrap_or_default(),
            recording: None,
            error: None,
            saving: false,
            saved: false,
            swallow_key_click: false,
            focus: SHORTCUT_COMMANDS
                .iter()
                .map(|c| (c.id, cx.focus_handle()))
                .collect(),
        }
    }

    /// Replaces the draft with the stored bindings.
    pub fn reset(&mut self, bindings: ShortcutBindings) {
        self.draft = bindings;
        self.recording = None;
        self.error = None;
        self.saved = false;
    }

    fn handle(&self, command: ShortcutCommand) -> Option<&FocusHandle> {
        self.focus
            .iter()
            .find(|(c, _)| *c == command)
            .map(|(_, h)| h)
    }

    fn change(&mut self, next: ShortcutBindings) {
        self.draft = next;
        self.saved = false;
        self.error = None;
    }
}

impl SettingsWindow {
    /// Leaving a binding button stops recording (`onBlur`).
    pub(crate) fn shortcut_blurs(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        self.shortcuts
            .focus
            .iter()
            .map(|(command, handle)| {
                let command = *command;
                cx.on_blur(handle, window, move |this, _, cx| {
                    if this.shortcuts.recording == Some(command) {
                        this.shortcuts.recording = None;
                        cx.notify();
                    }
                })
            })
            .collect()
    }

    /// Starts listening for `command`'s keys.
    pub fn record(
        &mut self,
        command: ShortcutCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Space and Enter on the focused button also activate it; the press
        // is the binding being recorded (or refused), not a new start.
        if self.shortcuts.recording == Some(command) {
            return;
        }
        self.shortcuts.recording = Some(command);
        self.shortcuts.error = None;
        if let Some(handle) = self.shortcuts.handle(command) {
            handle.focus(window, cx);
        }
        cx.notify();
    }

    /// A key pressed while `command` is recording.
    pub fn record_key(
        &mut self,
        command: ShortcutCommand,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        self.shortcuts.swallow_key_click = false;
        if self.shortcuts.recording != Some(command) {
            return false;
        }
        if event.keystroke.key == "escape" {
            self.shortcuts.recording = None;
            cx.notify();
            return true;
        }
        // A lone modifier, or a key with no stored name, keeps listening.
        let Some(shortcut) = ui_shell::keymap::shortcut_for_keystroke(&event.keystroke) else {
            return true;
        };
        match record_shortcut(&self.shortcuts.draft, command, &shortcut) {
            Ok(next) => {
                self.shortcuts.change(next);
                self.shortcuts.recording = None;
                self.shortcuts.swallow_key_click =
                    matches!(event.keystroke.key.as_str(), "space" | "enter");
            }
            Err(error) => self.shortcuts.error = Some(error),
        }
        cx.notify();
        true
    }

    /// Clears one command's binding.
    pub fn clear_binding(&mut self, command: ShortcutCommand, cx: &mut Context<Self>) {
        let next = with_binding(&self.shortcuts.draft, command, Vec::new());
        self.shortcuts.change(next);
        cx.notify();
    }

    /// Restore defaults.
    pub fn restore_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.shortcuts.change(ShortcutBindings::default());
        self.shortcuts.recording = None;
        cx.notify();
    }

    /// Save shortcuts: the stored display section with these bindings.
    pub fn save_shortcuts(&mut self, cx: &mut Context<Self>) {
        if self.shortcuts.saving || self.shortcuts.recording.is_some() {
            return;
        }
        self.shortcuts.saving = true;
        self.shortcuts.error = None;
        let mut display = self.settings.display.clone();
        display.shortcuts = Some(self.shortcuts.draft.clone());
        let value = serde_json::to_value(&display);
        cx.notify();
        let request = value
            .map_err(|e| e.to_string())
            .map(|value| self.backend.set_section("display", value));
        cx.spawn(async move |this, cx| {
            let result = match request {
                Ok(request) => request.await,
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |this, cx| {
                this.shortcuts.saving = false;
                match result {
                    Ok(()) => {
                        this.settings.display.shortcuts = Some(this.shortcuts.draft.clone());
                        this.shortcuts.saved = true;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "[Settings] shortcuts not saved");
                        this.shortcuts.error =
                            Some("Could not save shortcuts. Please try again.".into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn render_shortcuts(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let pane = &self.shortcuts;
        let groups = ["Navigation", "Live commands"].map(|group| {
            let rows: Vec<AnyElement> = SHORTCUT_COMMANDS
                .iter()
                .filter(|c| c.group == group)
                .map(|c| {
                    let command = c.id;
                    let recording = pane.recording == Some(command);
                    let label = if recording {
                        "Press keys… Esc cancels".to_owned()
                    } else {
                        let keys: Vec<String> = bindings_for(command, &pane.draft)
                            .into_iter()
                            .map(shortcut_label)
                            .collect();
                        if keys.is_empty() {
                            "Not assigned".to_owned()
                        } else {
                            keys.join(" or ")
                        }
                    };
                    let handle = pane.handle(command).cloned();
                    h_flex()
                        .justify_between()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_3()
                        .child(
                            div()
                                .text_size(text::SM.size)
                                .text_color(k.text(TextStep::T200))
                                .child(c.label),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .id(SharedString::from(ids::binding(command)))
                                        .test_support()
                                        .role(Role::Button)
                                        .aria_label(format!("Set shortcut for {}", c.label))
                                        .tab_index(0)
                                        .when_some(handle, |this, h| this.track_focus(&h))
                                        .min_w(px(130.))
                                        .rounded(radius::md())
                                        .border_1()
                                        .border_color(if recording {
                                            k.focus_ring()
                                        } else {
                                            gpui_kit::transparent_black()
                                        })
                                        .bg(k.surface_tertiary)
                                        .px_3()
                                        .py_1p5()
                                        .text_size(text::XS.size)
                                        .text_color(k.text(TextStep::T300))
                                        .cursor_pointer()
                                        .hover(|s| s.border_color(k.focus_ring()))
                                        .focus_visible(|s| s.border_color(k.focus_ring()))
                                        .child(label)
                                        .on_click(cx.listener(
                                            move |this,
                                                  event: &gpui_kit::ClickEvent,
                                                  window,
                                                  cx| {
                                                if event.is_keyboard()
                                                    && std::mem::take(
                                                        &mut this.shortcuts.swallow_key_click,
                                                    )
                                                {
                                                    return;
                                                }
                                                this.record(command, window, cx)
                                            },
                                        ))
                                        .on_key_down(cx.listener(
                                            move |this, event: &KeyDownEvent, window, cx| {
                                                if this.record_key(command, event, cx) {
                                                    // The press is the binding, not a command.
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                }
                                            },
                                        )),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(ids::clear(command)))
                                        .test_support()
                                        .role(Role::Button)
                                        .aria_label(format!("Clear shortcut for {}", c.label))
                                        .tab_index(0)
                                        .text_size(text::XS.size)
                                        .text_color(k.text(TextStep::T500))
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(k.text_primary))
                                        .child("Clear")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.clear_binding(command, cx)
                                        })),
                                ),
                        )
                        .into_any_element()
                })
                .collect();
            v_flex()
                .gap_2()
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "settings-shortcut-group-{group}"
                        )))
                        .role(Role::Heading)
                        .text_size(text::XS.size)
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(k.text(TextStep::T400))
                        .child(group),
                )
                .child(
                    v_flex()
                        .rounded(radius::lg())
                        .bg(k.surface_tertiary)
                        .children(rows),
                )
        });
        let saving = pane.saving;
        let saved = pane.saved;
        let recording = pane.recording.is_some();
        v_flex()
            .gap_5()
            .child(
                div()
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T400))
                    .child("Shortcuts work while Kairo is focused. Click a binding and press a key combination. Live commands pause while typing or using a dialog."),
            )
            .children(groups)
            .when_some(pane.error.clone(), |this, error| {
                this.child(
                    div()
                        .id(ids::ERROR)
                        .test_support()
                        .role(Role::Alert)
                        .aria_label(error.clone())
                        .text_size(text::XS.size)
                        .text_color(k.danger_text)
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .id(ids::RESTORE)
                            .test_support()
                            .role(Role::Button)
                            .aria_label("Restore defaults")
                            .tab_index(0)
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .cursor_pointer()
                            .hover(|s| s.text_color(k.text_primary))
                            .child("Restore defaults")
                            .on_click(cx.listener(|this, _, _, cx| this.restore_shortcuts(cx))),
                    )
                    .child(
                        Button::new(ids::SAVE).cursor_pointer()
                            .primary()
                            .small()
                            .disabled(saving || recording)
                            .label(if saving {
                                "Saving…"
                            } else if saved {
                                "Saved"
                            } else {
                                "Save shortcuts"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.save_shortcuts(cx))),
                    ),
            )
            .into_any_element()
    }
}
