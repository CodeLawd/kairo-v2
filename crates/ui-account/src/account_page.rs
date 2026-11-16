//! Shared auth → verification → activation page. Children follow state transitions.
use crate::fluid::{Flick, Tuning};
use crate::{
    auth::{AuthEvent, AuthMode, AuthPanel},
    license::LicensePanel,
    model::Account,
    verify::VerifyEmail,
};
use gpui_kit::component::v_flex;
use gpui_kit::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
};
use kairo_domain::cloud::{can_enter_app, needs_email_confirmation};
use kairo_ui_kit::{KairoThemeExt, brand::PRODUCT_NAME};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    Gate,
    Setup,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Auth,
    Verify,
    Activate,
}

pub struct AccountPage {
    account: Entity<Account>,
    host: Host,
    active: bool,
    phase: Phase,
    mode: AuthMode,
    auth: Option<Entity<AuthPanel>>,
    verify: Option<Entity<VerifyEmail>>,
    license: Option<Entity<LicensePanel>>,
    signing_out: bool,
    entry: Flick,
    _account: Subscription,
    auth_subscription: Option<Subscription>,
}
impl AccountPage {
    pub fn new(
        account: Entity<Account>,
        host: Host,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe_in(&account, window, |this, _, _, window, cx| {
            this.sync(window, cx);
            cx.notify();
        });
        let mut page = Self {
            account,
            host,
            active: true,
            phase: Phase::Auth,
            mode: if host == Host::Gate {
                AuthMode::SignIn
            } else {
                AuthMode::SignUp
            },
            auth: None,
            verify: None,
            license: None,
            signing_out: false,
            entry: Flick::at(0., cx.background_executor().now()),
            _account: subscription,
            auth_subscription: None,
        };
        page.sync(window, cx);
        page.enter(cx);
        page
    }
    fn enter(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.entry.pin(0., now);
        self.entry.to(1., Tuning::PAGE, now);
    }
    pub fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.sync(window, cx);
            self.enter(cx);
            cx.notify();
        }
    }
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        let session = self.account.read(cx).session();
        let phase = if can_enter_app(session) {
            Phase::Activate
        } else if needs_email_confirmation(session) {
            Phase::Verify
        } else {
            Phase::Auth
        };
        if phase != self.phase {
            self.phase = phase;
            self.enter(cx);
        }
        if phase != Phase::Auth {
            self.auth = None;
            self.auth_subscription = None;
        }
        if phase != Phase::Verify {
            self.verify = None;
        }
        if phase != Phase::Activate {
            self.license = None;
        }
        if phase == Phase::Auth && self.auth.is_none() {
            let account = self.account.clone();
            let mode = self.mode;
            let auth = cx.new(|cx| AuthPanel::new(account, mode, window, cx));
            self.auth_subscription = Some(cx.subscribe(&auth, |this, _, event: &AuthEvent, cx| {
                if let AuthEvent::ModeChanged(mode) = event {
                    this.mode = *mode;
                    this.enter(cx);
                    cx.notify();
                }
            }));
            self.auth = Some(auth);
        }
        if phase == Phase::Verify && self.verify.is_none() {
            let account = self.account.clone();
            let gate = self.host == Host::Gate;
            self.verify = Some(cx.new(|cx| VerifyEmail::new(account, gate, !gate, window, cx)));
        }
        if phase == Phase::Activate && self.license.is_none() && self.host == Host::Setup {
            let account = self.account.clone();
            self.license = Some(cx.new(|cx| LicensePanel::new(account, true, window, cx)));
        }
    }
    fn different_email(&mut self, cx: &mut Context<Self>) {
        if self.signing_out {
            return;
        }
        self.signing_out = true;
        cx.notify();
        let revision = self.account.read(cx).session_revision();
        let request = self.account.read(cx).backend().sign_out();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.signing_out = false;
                if this.account.read(cx).session_revision() == revision
                    && let Ok(session) = result
                {
                    this.mode = if this.host == Host::Gate {
                        AuthMode::SignIn
                    } else {
                        AuthMode::SignUp
                    };
                    this.account.update(cx, |a, cx| a.set_session(session, cx));
                }
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for AccountPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let ob = kairo_ui_kit::onboarding::colors(k.appearance);
        if self
            .entry
            .tick(cx.background_executor().now(), cx.reduced_motion())
        {
            window.request_animation_frame();
        }
        let session = self.account.read(cx).session();
        let (title, blurb) = account_copy(
            self.phase == Phase::Activate,
            self.phase == Phase::Verify,
            self.mode == AuthMode::SignUp,
            session.user.as_ref().map(|u| u.email.as_str()),
        );
        let body = match self.phase {
            Phase::Auth => gpui_kit::div()
                .children(self.auth.clone())
                .into_any_element(),
            Phase::Activate => gpui_kit::div()
                .children(self.license.clone())
                .into_any_element(),
            Phase::Verify => v_flex()
                .gap_4()
                .children(self.verify.clone())
                .child(
                    crate::stage::quiet(
                        if self.host == Host::Gate {
                            crate::gate::ids::DIFFERENT_EMAIL
                        } else {
                            crate::wizard::ids::DIFFERENT_EMAIL
                        },
                        "Use a different email",
                        self.signing_out,
                    )
                    .loading(self.signing_out)
                    .on_click(cx.listener(|this, _, _, cx| this.different_email(cx))),
                )
                .into_any_element(),
        };
        crate::stage::form_page(
            if self.host == Host::Gate {
                "account-gate-title"
            } else {
                crate::wizard::ids::TITLE
            },
            title,
            blurb,
            body,
            &ob,
            self.entry.value(),
        )
    }
}

/// The account page's title and blurb.
pub fn account_copy(
    ready: bool,
    verifying: bool,
    sign_up: bool,
    email: Option<&str>,
) -> (String, String) {
    if ready {
        return (
            "Activate this computer".into(),
            "Turn on live transcription, scripture lookup and lyric tools for your church.".into(),
        );
    }
    if verifying {
        return (
            "Confirm your email".into(),
            format!(
                "Enter the 6-digit code we sent to {}.",
                email.unwrap_or("your email")
            ),
        );
    }
    if sign_up {
        (
            "Create your account".into(),
            "Your church’s songs, themes and settings follow you to every Kairo computer.".into(),
        )
    } else {
        (
            format!("Sign in to {PRODUCT_NAME}"),
            "Welcome back. Your church’s library comes with you.".into(),
        )
    }
}
