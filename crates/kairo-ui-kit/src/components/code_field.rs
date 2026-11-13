//! The emailed confirmation code (`components/ui/input-otp.tsx` as
//! `VerifyEmailNotice` uses it): one box per digit. Kit's `OtpInput` takes
//! digits typed one at a time; this adds pasting a whole code, which is how
//! a code from an email usually arrives.

use gpui_kit::component::Disableable;
use gpui_kit::component::input::{OtpEvent, OtpInput, OtpState};
use gpui_kit::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, Role, SharedString, StatefulInteractiveElement,
    Subscription, Window, actions, div,
};

actions!(kairo_code_field, [PasteCode]);

const CONTEXT: &str = "KairoCodeField";

/// Binds paste for code fields. Called by [`crate::init`].
pub fn init(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let paste = "cmd-v";
    #[cfg(not(target_os = "macos"))]
    let paste = "ctrl-v";
    cx.bind_keys([KeyBinding::new(paste, PasteCode, Some(CONTEXT))]);
}

/// The digits in pasted text, full-width digits included, up to `length`.
/// Spaces and dashes (`123 456`, `123-456`) are dropped.
pub fn digits_from_paste(text: &str, length: usize) -> String {
    text.chars()
        .filter_map(|c| {
            if c.is_ascii_digit() {
                Some(c)
            } else {
                let offset = (c as u32).checked_sub('０' as u32)?;
                char::from_digit(offset, 10)
            }
        })
        .take(length)
        .collect()
}

/// What the field reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeFieldEvent {
    /// The digits changed.
    Change(SharedString),
    /// Every box is filled.
    Complete(SharedString),
}

/// The code field.
pub struct CodeField {
    state: Entity<OtpState>,
    disabled: bool,
    label: SharedString,
    _subscription: Subscription,
}

impl EventEmitter<CodeFieldEvent> for CodeField {}

impl CodeField {
    /// A field for `length` digits.
    pub fn new(
        length: usize,
        label: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = cx.new(|cx| OtpState::new(length, window, cx));
        let subscription = cx.subscribe(&state, |this, state, event: &OtpEvent, cx| {
            let value = state.read(cx).value().clone();
            match event {
                OtpEvent::Change => cx.emit(CodeFieldEvent::Change(value)),
                OtpEvent::Complete => cx.emit(CodeFieldEvent::Complete(value)),
                _ => {}
            }
            let _ = this;
        });
        Self {
            state,
            disabled: false,
            label: label.into(),
            _subscription: subscription,
        }
    }

    /// The digits entered.
    pub fn value(&self, cx: &App) -> SharedString {
        self.state.read(cx).value().clone()
    }

    /// Clears the boxes (after a wrong code or a resend).
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    /// Disables the boxes while a code is being checked.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    fn paste(&mut self, _: &PasteCode, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let length = self.state.read(cx).len();
        let digits = digits_from_paste(&text, length);
        if digits.is_empty() {
            return;
        }
        let value: SharedString = digits.into();
        self.state
            .update(cx, |state, cx| state.set_value(value.clone(), window, cx));
        cx.emit(CodeFieldEvent::Change(value.clone()));
        if value.chars().count() == length {
            cx.emit(CodeFieldEvent::Complete(value));
        }
        cx.notify();
    }
}

impl Focusable for CodeField {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }
}

impl Render for CodeField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("code-field")
            .key_context(CONTEXT)
            .role(Role::Group)
            .aria_label(self.label.clone())
            .on_action(cx.listener(Self::paste))
            .child(OtpInput::new(&self.state).disabled(self.disabled))
    }
}

#[cfg(test)]
mod tests {
    use super::digits_from_paste;

    #[test]
    fn paste_keeps_digits_only_up_to_the_length() {
        assert_eq!(digits_from_paste("123 456", 6), "123456");
        assert_eq!(digits_from_paste("Your code: 987-654-3", 6), "987654");
        assert_eq!(digits_from_paste("１２３", 6), "123");
        assert_eq!(digits_from_paste("no digits", 6), "");
    }
}
