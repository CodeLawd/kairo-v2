//! The account wall (`AccountGate.tsx`): full screen until the operator has
//! a confirmed session. Sign-in first; a new account stays here for the
//! email code; only then do setup and the booth open.

use crate::model::Account;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::FocusTrapElement;
use gpui_kit::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Role,
    StatefulInteractiveElement, Styled, Window, div,
};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::brand::PRODUCT_NAME;

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

/// The sign-in wall. AccountPage owns the form journey.
pub struct AccountGate {
    page: Entity<crate::account_page::AccountPage>,
    modal: kairo_ui_kit::components::ModalFocus,
}
impl AccountGate {
    pub fn new(account: Entity<Account>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut modal = kairo_ui_kit::components::ModalFocus::new(cx);
        modal.open(window, cx);
        let page = cx.new(|cx| {
            crate::account_page::AccountPage::new(
                account,
                crate::account_page::Host::Gate,
                window,
                cx,
            )
        });
        Self { page, modal }
    }
}
impl Render for AccountGate {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stage::surface(&kairo_ui_kit::onboarding::colors(cx.kairo().appearance))
            .id(ids::GATE)
            .test_support()
            .role(Role::Dialog)
            .aria_label("Account")
            .child(crate::stage::top_bar())
            .child(
                div()
                    .id("gate-body")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.page.clone()),
            )
            .focus_trap(ids::GATE, self.modal.handle())
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
