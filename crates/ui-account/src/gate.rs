//! The account wall (`AccountGate.tsx`): full screen until the operator has
//! a confirmed session. Sign-in first; a new account stays here for the
//! email code; only then do setup and the booth open.

use crate::auth::{AuthMode, AuthPanel};
use crate::model::Account;
use crate::verify::VerifyEmail;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::v_flex;
use gpui_kit::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Role,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use kairo_domain::cloud::needs_email_confirmation;
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::brand::{KairoMark, MarkSize, PRODUCT_NAME};
use kairo_ui_kit::motion;
use kairo_ui_kit::tokens::{TextStep, radius, text};

/// Element ids, for tests.
pub mod ids {
    /// The gate.
    pub const GATE: &str = "account-gate";
    /// Use a different email.
    pub const DIFFERENT_EMAIL: &str = "gate-different-email";
}

/// The title and blurb for the gate.
pub fn gate_copy(verifying: bool) -> (String, String) {
    if verifying {
        (
            "Confirm your email".into(),
            "We sent a 6-digit code. Enter it here — no browser needed.".into(),
        )
    } else {
        (
            format!("Sign in to {PRODUCT_NAME}"),
            format!(
                "An account is required to use {PRODUCT_NAME}. Sign in, or create one if you don’t have one yet."
            ),
        )
    }
}

/// The gate.
pub struct AccountGate {
    account: Entity<Account>,
    auth: Entity<AuthPanel>,
    verify: Option<Entity<VerifyEmail>>,
    signing_out: bool,
    _subscription: Subscription,
}

impl AccountGate {
    /// A gate over `account`.
    pub fn new(account: Entity<Account>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let auth = cx.new(|cx| AuthPanel::new(account.clone(), AuthMode::SignIn, window, cx));
        let subscription = cx.observe(&account, |_, _, cx| cx.notify());
        Self {
            account,
            auth,
            verify: None,
            signing_out: false,
            _subscription: subscription,
        }
    }

    fn sign_out(&mut self, cx: &mut Context<Self>) {
        self.signing_out = true;
        cx.notify();
        let request = self.account.read(cx).backend().sign_out();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.signing_out = false;
                if let Ok(session) = result {
                    this.account
                        .update(cx, |account, cx| account.set_session(session, cx));
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for AccountGate {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let verifying = needs_email_confirmation(self.account.read(cx).session());
        let (title, blurb) = gate_copy(verifying);
        let content = if verifying {
            let verify = match &self.verify {
                Some(verify) => verify.clone(),
                None => {
                    let account = self.account.clone();
                    let verify = cx.new(|cx| VerifyEmail::new(account, true, false, window, cx));
                    self.verify = Some(verify.clone());
                    verify
                }
            };
            v_flex()
                .gap_4()
                .child(verify)
                .child(
                    div()
                        .text_size(text::XS.size)
                        .child(crate::style::text_button(
                            ids::DIFFERENT_EMAIL,
                            if self.signing_out {
                                "Signing out…"
                            } else {
                                "Use a different email"
                            },
                            k.text(TextStep::T500),
                            k.text(TextStep::T300),
                            self.signing_out,
                            cx.listener(|this, _, _, cx| this.sign_out(cx)),
                        )),
                )
                .into_any_element()
        } else {
            self.verify = None;
            self.auth.clone().into_any_element()
        };
        let card = v_flex()
            .w(px(392.))
            .max_w_full()
            .overflow_hidden()
            .rounded(radius::xl2())
            .bg(k.surface_secondary)
            .shadow_2xl()
            .px(px(28.))
            .pb(px(28.))
            .pt(px(32.))
            .child(
                div()
                    .mb(px(24.))
                    .flex()
                    .justify_center()
                    .child(KairoMark::new(MarkSize::Medium)),
            )
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child(title.clone()),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T500))
                    .child(blurb),
            )
            .child(div().mt(px(20.)).child(content));
        div()
            .id(ids::GATE)
            .test_support()
            .role(Role::Dialog)
            .aria_label(SharedString::from(title))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(k.surface)
            .child(motion::animate(
                div().child(card),
                "account-gate-card",
                motion::SPRING_IN,
                cx,
            ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn copy_follows_the_session() {
        assert_eq!(super::gate_copy(true).0, "Confirm your email");
        assert_eq!(super::gate_copy(false).0, "Sign in to Kairo");
    }
}
