//! Email and password, and browser sign-in (\`AuthPanel.tsx\`). Used by the
//! account gate and the account page of setup.

use crate::model::Account;
use crate::style::{self, encode_uri_component};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable, h_flex, v_flex};
use gpui_kit::{
    AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder, px,
};
use kairo_domain::cloud::{
    DevicePairingStatus, MIN_PASSWORD_LENGTH, SignInInput, SignUpInput, validate_sign_up,
};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, text};

/// Creating an account or signing in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// \`signIn\`.
    SignIn,
    /// \`signUp\`.
    SignUp,
}

/// What the panel reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthEvent {
    /// Signed in or signed up (\`onDone\`).
    Done,
    /// The operator switched between creating an account and signing in.
    ModeChanged(AuthMode),
}

/// Element ids, for tests and the coach tour.
pub mod ids {
    /// Continue with Google.
    pub const GOOGLE: &str = "auth-google";
    /// Name field.
    pub const NAME: &str = "account-name";
    /// Church name field.
    pub const CHURCH: &str = "account-church";
    /// Email field.
    pub const EMAIL: &str = "account-email";
    /// Password field.
    pub const PASSWORD: &str = "account-password";
    /// Forgot password.
    pub const FORGOT: &str = "auth-forgot";
    /// Submit.
    pub const SUBMIT: &str = "auth-submit";
    /// Switch between sign-in and sign-up.
    pub const SWITCH: &str = "auth-switch";
    /// The error or notice line.
    pub const MESSAGE: &str = "auth-message";
    /// The pairing code.
    pub const PAIRING_CODE: &str = "auth-pairing-code";
    /// Open browser again.
    pub const PAIRING_REOPEN: &str = "auth-pairing-reopen";
    /// Cancel pairing.
    pub const PAIRING_CANCEL: &str = "auth-pairing-cancel";
}

/// Placeholder for the name field. Electron's names a person; this one is
/// a made-up name (DECISIONS.md, 073).
const NAME_PLACEHOLDER: &str = "Jordan Lee";

/// The panel.
pub struct AuthPanel {
    account: Entity<Account>,
    mode: AuthMode,
    name: Entity<InputState>,
    church: Entity<InputState>,
    email: Entity<InputState>,
    password: Entity<InputState>,
    busy: bool,
    /// The synchronous single-flight gate (\`createSingleFlight\`).
    in_flight: bool,
    error: Option<SharedString>,
    notice: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AuthEvent> for AuthPanel {}

impl AuthPanel {
    /// A panel starting in \`mode\`.
    pub fn new(
        account: Entity<Account>,
        mode: AuthMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(NAME_PLACEHOLDER));
        let church = cx.new(|cx| InputState::new(window, cx).placeholder("Grace Chapel"));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("you@church.org"));
        let password = cx.new(|cx| InputState::new(window, cx).masked(true));
        let subscriptions = vec![
            cx.subscribe_in(&password, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit(cx);
                }
            }),
            cx.observe(&account, |_, _, cx| cx.notify()),
        ];
        let mut panel = Self {
            account,
            mode,
            name,
            church,
            email,
            password,
            busy: false,
            in_flight: false,
            error: None,
            notice: None,
            _subscriptions: subscriptions,
        };
        panel.apply_placeholder(window, cx);
        panel
    }

    /// The mode showing.
    pub fn mode(&self) -> AuthMode {
        self.mode
    }

    /// The error or notice showing.
    pub fn message(&self) -> Option<&SharedString> {
        self.error.as_ref().or(self.notice.as_ref())
    }

    fn apply_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = match self.mode {
            AuthMode::SignUp => format!("At least {MIN_PASSWORD_LENGTH} characters"),
            AuthMode::SignIn => String::new(),
        };
        self.password
            .update(cx, |state, cx| state.set_placeholder(placeholder, window, cx));
    }

    fn value(&self, input: &Entity<InputState>, cx: &Context<Self>) -> String {
        input.read(cx).value().to_string()
    }

    /// Submits the form. A second submit while one is running shares it.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.in_flight {
            return;
        }
        self.error = None;
        self.notice = None;
        let email = self.value(&self.email, cx).trim().to_owned();
        let password = self.value(&self.password, cx);
        let name = self.value(&self.name, cx);
        let church = self.value(&self.church, cx);
        if self.mode == AuthMode::SignUp {
            // Checked here as well as on the server so the answer is instant.
            let input = SignUpInput {
                email: email.clone(),
                password: password.clone(),
                name: name.clone(),
                org_name: church.clone(),
            };
            if let Some(invalid) = validate_sign_up(&input) {
                self.error = Some(invalid.into());
                cx.notify();
                return;
            }
        }
        self.in_flight = true;
        self.busy = true;
        cx.notify();
        let backend = self.account.read(cx).backend();
        let request = match self.mode {
            AuthMode::SignUp => backend.sign_up(SignUpInput {
                email,
                password,
                name: name.trim().to_owned(),
                org_name: church.trim().to_owned(),
            }),
            AuthMode::SignIn => backend.sign_in(SignInInput { email, password }),
        };
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.in_flight = false;
                this.busy = false;
                match result {
                    Ok(result) if result.ok && result.data.is_some() => {
                        let session = result.data.expect("checked");
                        this.account
                            .update(cx, |account, cx| account.set_session(session, cx));
                        cx.emit(AuthEvent::Done);
                    }
                    Ok(result) => {
                        this.error = Some(
                            result
                                .error
                                .unwrap_or_else(|| "Something went wrong.".into())
                                .into(),
                        );
                    }
                    Err(error) => this.error = Some(error.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reset_password(&mut self, cx: &mut Context<Self>) {
        let email = self.value(&self.email, cx).trim().to_owned();
        if email.is_empty() {
            self.error = Some("Enter your email address first".into());
            self.notice = None;
            cx.notify();
            return;
        }
        let request = self.account.read(cx).backend().request_password_reset(email);
        cx.spawn(async move |this, cx| {
            let _ = request.await;
            // Deliberately unconditional: the API will not say whether the
            // address is registered, and neither does this.
            let _ = this.update(cx, |this, cx| {
                this.error = None;
                this.notice =
                    Some("If that address has an account, a reset link is on its way.".into());
                cx.notify();
            });
        })
        .detach();
    }

    fn pair_in_browser(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.account.read(cx).pairing().status == DevicePairingStatus::Waiting {
            return;
        }
        self.busy = true;
        self.error = None;
        self.notice = None;
        cx.notify();
        let request = self.account.read(cx).backend().start_pairing();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(pairing) => this
                        .account
                        .update(cx, |account, cx| account.set_pairing(pairing, cx)),
                    Err(_) => {
                        this.error =
                            Some("Could not open browser sign-in. Please try again.".into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn switch_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            AuthMode::SignUp => AuthMode::SignIn,
            AuthMode::SignIn => AuthMode::SignUp,
        };
        self.error = None;
        self.notice = None;
        self.apply_placeholder(window, cx);
        cx.emit(AuthEvent::ModeChanged(self.mode));
        cx.notify();
    }

    fn field(
        &self,
        id: &'static str,
        label: &'static str,
        input: &Entity<InputState>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        v_flex().child(style::label(label, cx)).child(
            Input::new(input)
                .id(id)
                .aria_label(label)
                .disabled(self.busy),
        )
    }

    fn render_pairing(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let k = *cx.kairo();
        let code = self
            .account
            .read(cx)
            .pairing()
            .user_code
            .clone()
            .unwrap_or_default();
        let reopen = code.clone();
        v_flex()
            .id("auth-pairing")
            .test_support()
            .role(Role::Status)
            .gap_4()
            .child(
                div()
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T300))
                    .child("Finish signing in with Google in your browser, then approve this computer."),
            )
            .child(
                div()
                    .id(ids::PAIRING_CODE)
                    .test_support()
                    .aria_label(SharedString::from(code.clone()))
                    .w_full()
                    .flex()
                    .justify_center()
                    .font_family(text::MONO_FAMILY)
                    .text_size(text::XL2.size)
                    .text_color(k.text_primary)
                    .child(code),
            )
            .child(
                div()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child("Check that the browser shows this same code. Kairo will finish signing in automatically."),
            )
            .child(
                Button::new(ids::PAIRING_REOPEN)
                    .label("Open browser again")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let path = format!("/activate?userCode={}", encode_uri_component(&reopen));
                        let request = this.account.read(cx).backend().open_web(path);
                        cx.background_spawn(async move {
                            let _ = request.await;
                        })
                        .detach();
                    })),
            )
            .child(
                div().flex().justify_center().text_size(text::XS.size).child(
                    style::text_button(
                        ids::PAIRING_CANCEL,
                        "Cancel",
                        k.text(TextStep::T400),
                        k.text_primary,
                        false,
                        cx.listener(|this, _, _, cx| {
                            let request = this.account.read(cx).backend().cancel_pairing();
                            cx.spawn(async move |this, cx| {
                                if let Ok(pairing) = request.await {
                                    let _ = this.update(cx, |this, cx| {
                                        this.account
                                            .update(cx, |a, cx| a.set_pairing(pairing, cx))
                                    });
                                }
                            })
                            .detach();
                        }),
                    ),
                ),
            )
            .into_any_element()
    }
}

impl Render for AuthPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let k = *cx.kairo();
        let pairing = self.account.read(cx).pairing().clone();
        if pairing.status == DevicePairingStatus::Waiting {
            return self.render_pairing(cx);
        }
        let sign_up = self.mode == AuthMode::SignUp;
        let busy = self.busy;
        let message = match (&self.error, &self.notice) {
            (Some(error), _) => Some((error.clone(), k.danger_text)),
            (None, Some(notice)) => Some((notice.clone(), k.notice_text)),
            _ => None,
        };
        v_flex()
            .id("auth-panel")
            .gap_4()
            .child(
                Button::new(ids::GOOGLE)
                    .label("Continue with Google")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.pair_in_browser(cx))),
            )
            .when_some(pairing.message.clone(), |el, message| {
                el.child(
                    div()
                        .id("auth-pairing-message")
                        .test_support()
                        .role(Role::Alert)
                        .text_size(text::XS.size)
                        .text_color(k.danger_text)
                        .child(message),
                )
            })
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child(div().h(px(1.)).flex_1().bg(k.surface_border))
                    .child("or")
                    .child(div().h(px(1.)).flex_1().bg(k.surface_border)),
            )
            .when(sign_up, |el| {
                el.child(self.field(ids::NAME, "Your name", &self.name, cx))
                    .child(self.field(ids::CHURCH, "Church name", &self.church, cx))
            })
            .child(self.field(ids::EMAIL, "Email", &self.email, cx))
            .child(
                v_flex()
                    .child(
                        h_flex()
                            .mb(px(6.))
                            .items_baseline()
                            .justify_between()
                            .gap_3()
                            .child(style::label("Password", cx).mb_0())
                            .when(!sign_up, |el| {
                                el.child(div().text_size(text::XS.size).child(style::text_button(
                                    ids::FORGOT,
                                    "Forgot password",
                                    k.text(TextStep::T500),
                                    k.text(TextStep::T300),
                                    busy,
                                    cx.listener(|this, _, _, cx| this.reset_password(cx)),
                                )))
                            }),
                    )
                    .child(
                        Input::new(&self.password)
                            .id(ids::PASSWORD)
                            .aria_label("Password")
                            .mask_toggle()
                            .disabled(busy),
                    ),
            )
            .child(
                div()
                    .id(ids::MESSAGE)
                    .test_support()
                    .role(Role::Status)
                    .min_h(px(16.))
                    .text_size(text::XS.size)
                    .when_some(message, |el, (message, color)| {
                        el.aria_label(message.clone()).text_color(color).child(message)
                    }),
            )
            .child(
                Button::new(ids::SUBMIT)
                    .primary()
                    .loading(busy)
                    .disabled(busy)
                    .label(if sign_up { "Create account" } else { "Sign in" })
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
            )
            .child(
                h_flex()
                    .gap_1()
                    .pt(px(2.))
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child(if sign_up {
                        "Already have an account?"
                    } else {
                        "Don’t have an account?"
                    })
                    .child(style::text_button(
                        ids::SWITCH,
                        if sign_up { "Sign in" } else { "Create one" },
                        k.text(TextStep::T300),
                        k.text_primary,
                        busy,
                        cx.listener(|this, _, window, cx| this.switch_mode(window, cx)),
                    )),
            )
            .into_any_element()
    }
}
