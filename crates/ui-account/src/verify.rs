//! Confirming the address with the emailed code (`VerifyEmailNotice.tsx`).
//! Submits on the sixth digit; one request at a time.

use crate::model::Account;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::v_flex;
use gpui_kit::rems;
use gpui_kit::{
    AppContext, Context, Entity, Focusable, InteractiveElement, IntoElement, ParentElement, Render,
    Role, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder,
};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::components::code_field::{CodeField, CodeFieldEvent};
use kairo_ui_kit::tokens::{TextStep, text};

/// Digits in the code.
pub const CODE_LENGTH: usize = 6;

/// Element ids, for tests.
pub mod ids {
    /// The status line.
    pub const STATUS: &str = "verify-status";
    /// Send a new code.
    pub const RESEND: &str = "verify-resend";
}

/// The code boxes and their status line.
pub struct VerifyEmail {
    account: Entity<Account>,
    field: Entity<CodeField>,
    show_email: bool,
    centered: bool,
    busy: bool,
    in_flight: bool,
    error: Option<SharedString>,
    resent: bool,
    focus_next_render: bool,
    _subscriptions: Vec<Subscription>,
}

impl VerifyEmail {
    /// `showEmail` names the address; `centered` is setup's layout.
    pub fn new(
        account: Entity<Account>,
        show_email: bool,
        centered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(|cx| CodeField::new(CODE_LENGTH, "Confirmation code", window, cx));
        let subscriptions = vec![
            cx.subscribe_in(
                &field,
                window,
                |this, _, event: &CodeFieldEvent, window, cx| {
                    if let CodeFieldEvent::Complete(code) = event {
                        this.submit(code.to_string(), window, cx);
                    }
                },
            ),
            cx.observe(&account, |_, _, cx| cx.notify()),
        ];
        Self {
            account,
            field,
            show_email,
            centered,
            busy: false,
            in_flight: false,
            error: None,
            resent: false,
            // `autoFocus`.
            focus_next_render: true,
            _subscriptions: subscriptions,
        }
    }

    /// The status line's text.
    pub fn status(&self) -> String {
        if self.busy {
            return "Checking…".into();
        }
        match (&self.error, self.resent) {
            (Some(error), _) => error.to_string(),
            (None, true) => "New code sent — use the newest one.".into(),
            (None, false) => "Didn’t get it?".into(),
        }
    }

    /// Checks `code`.
    pub fn submit(&mut self, code: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.in_flight {
            return;
        }
        self.in_flight = true;
        self.busy = true;
        self.error = None;
        self.resent = false;
        self.field
            .update(cx, |field, cx| field.set_disabled(true, cx));
        cx.notify();
        let request = self.account.read(cx).backend().verify_email_code(code);
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.in_flight = false;
                    this.busy = false;
                    this.field
                        .update(cx, |field, cx| field.set_disabled(false, cx));
                    match result {
                        Ok(result) if result.ok && result.data.is_some() => {
                            let session = result.data.expect("checked");
                            this.account
                                .update(cx, |account, cx| account.set_session(session, cx));
                        }
                        Ok(result) => {
                            this.error = Some(
                                result
                                    .error
                                    .unwrap_or_else(|| {
                                        "That code didn’t work. Check it and try again.".into()
                                    })
                                    .into(),
                            );
                            this.field.update(cx, |field, cx| field.clear(window, cx));
                            this.focus_next_render = true;
                        }
                        Err(_) => {
                            this.error = Some(
                                "Couldn’t reach Kairo. Check your connection and try again.".into(),
                            );
                            this.field.update(cx, |field, cx| field.clear(window, cx));
                            this.focus_next_render = true;
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn resend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.field.update(cx, |field, cx| field.clear(window, cx));
        let request = self.account.read(cx).backend().resend_verification();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(result) => {
                        this.resent = result.ok;
                        if !result.ok {
                            this.error = Some(
                                result
                                    .error
                                    .unwrap_or_else(|| {
                                        "Could not send it. Try again shortly.".into()
                                    })
                                    .into(),
                            );
                        }
                    }
                    Err(_) => {
                        this.resent = false;
                        this.error = Some("Could not send it. Try again shortly.".into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for VerifyEmail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_next_render) && !self.busy {
            let handle = self.field.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        let k = *cx.kairo();
        let email = self
            .account
            .read(cx)
            .session()
            .user
            .as_ref()
            .map(|u| u.email.clone())
            .unwrap_or_default();
        let status = self.status();
        let error = self.error.is_some() && !self.busy;
        v_flex()
            .gap_4()
            .when(self.centered, |el| el.items_center())
            .when(self.show_email, |el| {
                el.child(
                    div()
                        .flex()
                        .gap_1()
                        .text_size(text::SM.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Sent to")
                        .child(div().text_color(k.text(TextStep::T300)).child(email)),
                )
            })
            .child(self.field.clone())
            .child(
                div()
                    .id(ids::STATUS)
                    .test_support()
                    .role(Role::Status)
                    .aria_label(SharedString::from(status.clone()))
                    .min_h(rems(1.125))
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child(
                        div()
                            .when(error, |el| el.text_color(k.danger_text))
                            .child(status),
                    )
                    // Always offered: an error such as "expired" is only fixed
                    // by a new code.
                    .when(!self.busy && !self.resent, |el| {
                        el.child(
                            crate::style::text_button(
                                ids::RESEND,
                                "Send a new code",
                                k.text(TextStep::T300),
                                k.text_primary,
                                false,
                                cx.listener(|this, _, window, cx| this.resend(window, cx)),
                            )
                            .font_weight(gpui_kit::FontWeight::MEDIUM),
                        )
                    }),
            )
    }
}
