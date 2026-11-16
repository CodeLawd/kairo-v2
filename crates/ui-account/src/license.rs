//! Activation for this computer (`LicensePanel.tsx`): paste a key, press
//! Activate. Keys come from the church's Licence & Devices page.

use crate::model::Account;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable, h_flex, v_flex};
use gpui_kit::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Role,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder, px,
};
use kairo_domain::cloud::is_signed_in;
use kairo_ui_kit::ControlSize;
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, radius, text};

/// Element ids, for tests.
pub mod ids {
    /// The panel.
    pub const PANEL: &str = "license-panel";
    /// The key field.
    pub const KEY: &str = "license-key";
    /// Activate.
    pub const ACTIVATE: &str = "license-activate";
    /// Get a key.
    pub const GET_KEY: &str = "license-get-key";
    /// The error.
    pub const ERROR: &str = "license-error";
}

/// The message for a failed activation.
pub fn activation_error(error: Option<&str>) -> String {
    let error = error.unwrap_or("");
    if error.to_lowercase().contains("device limit") {
        "That key didn’t work — it may have been revoked, or every computer slot is taken.".into()
    } else if error.is_empty() {
        "Couldn’t activate. Try again.".into()
    } else {
        error.into()
    }
}

/// The panel.
pub struct LicensePanel {
    account: Entity<Account>,
    key: Entity<InputState>,
    bare: bool,
    busy: bool,
    error: Option<SharedString>,
    focus_once: bool,
    _subscriptions: Vec<Subscription>,
}

impl LicensePanel {
    /// `bare` drops the card and heading (the activation dialog).
    pub fn new(
        account: Entity<Account>,
        bare: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let key = cx.new(|cx| InputState::new(window, cx).placeholder("Activation key"));
        let subscriptions = vec![
            cx.subscribe_in(
                &key,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.activate(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
            cx.observe(&account, |_, _, cx| cx.notify()),
        ];
        Self {
            account,
            key,
            bare,
            busy: false,
            error: None,
            focus_once: bare,
            _subscriptions: subscriptions,
        }
    }

    /// Activates with the key typed.
    pub fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key.read(cx).value().to_string();
        if self.busy || key.trim().is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let request = self.account.read(cx).backend().activate_license(key);
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(result) if result.ok && result.data.is_some() => {
                            let snapshot = result.data.expect("checked");
                            this.account
                                .update(cx, |account, cx| account.set_license(snapshot, cx));
                            this.key.update(cx, |key, cx| key.set_value("", window, cx));
                        }
                        Ok(result) => {
                            this.error = Some(activation_error(result.error.as_deref()).into())
                        }
                        Err(_) => {
                            this.error = Some(
                                "Couldn’t reach Kairo. Check your connection and try again.".into(),
                            )
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }
}

impl Render for LicensePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let account = self.account.read(cx);
        let signed_in = is_signed_in(account.session());
        let activated = account.license().is_some_and(|l| l.activated);
        if std::mem::take(&mut self.focus_once) && signed_in && !activated {
            self.key.update(cx, |key, cx| key.focus(window, cx));
        }
        let body = if !signed_in {
            div()
                .text_size(text::SM.size)
                .text_color(k.text(TextStep::T400))
                .child("Sign in to activate this computer.")
                .into_any_element()
        } else if activated {
            h_flex()
                .gap_2()
                .items_center()
                .text_size(text::SM.size)
                .text_color(k.text(TextStep::T300))
                .child(div().size(px(6.)).rounded_full().bg(k.success_text))
                .child("This computer is activated.")
                .into_any_element()
        } else {
            let empty = self.key.read(cx).value().trim().is_empty();
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div().flex_1().font_family(text::MONO_FAMILY).child(
                                Input::new(&self.key)
                                    .kairo_size()
                                    .id(ids::KEY)
                                    .aria_label("Activation key"),
                            ),
                        )
                        .child(
                            Button::new(ids::ACTIVATE)
                                .primary()
                                .disabled(self.busy || empty)
                                .label(if self.busy {
                                    "Activating…"
                                } else {
                                    "Activate"
                                })
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.activate(window, cx)),
                                ),
                        ),
                )
                .child(
                    div()
                        .text_size(text::XS.size)
                        .child(crate::style::text_button(
                            ids::GET_KEY,
                            "Get a key ↗",
                            k.notice_text,
                            k.notice_text,
                            false,
                            cx.listener(|this, _, _, cx| {
                                let request = this
                                    .account
                                    .read(cx)
                                    .backend()
                                    .open_web("/dashboard/licence".into());
                                cx.background_spawn(async move {
                                    let _ = request.await;
                                })
                                .detach();
                            }),
                        )),
                )
                .into_any_element()
        };
        v_flex()
            .id(ids::PANEL)
            .test_support()
            .role(Role::Group)
            .aria_label("Activation")
            .gap(if self.bare { px(8.) } else { px(12.) })
            .when(!self.bare, |el| {
                el.rounded(radius::lg()).bg(k.surface_tertiary).p_4().child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(k.text_primary)
                        .child("Activation"),
                )
            })
            .child(body)
            .when_some(self.error.clone(), |el, error| {
                el.child(
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
    }
}

#[cfg(test)]
mod tests {
    use super::activation_error;

    #[test]
    fn device_limit_reads_as_revoked_or_full() {
        assert!(activation_error(Some("Device limit reached")).starts_with("That key didn’t work"));
        assert_eq!(activation_error(None), "Couldn’t activate. Try again.");
        assert_eq!(activation_error(Some("")), "Couldn’t activate. Try again.");
        assert_eq!(activation_error(Some("Key not found")), "Key not found");
    }
}
