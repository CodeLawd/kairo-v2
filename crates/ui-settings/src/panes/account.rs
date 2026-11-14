//! Account (`AccountSection.tsx`), laid out like macOS's Apple Account pane:
//! who you are up top, a note only when something needs attention, then
//! grouped rows. Signed out, it is the sign-in form.

use crate::model::{initials, relative_time};
use crate::widgets;
use crate::window::{SettingsEvent, SettingsWindow};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Role, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use kairo_domain::cloud::{CloudSessionState, OrgRole, SessionSnapshot, needs_email_confirmation};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use ui_account::Account;
use ui_account::auth::{AuthMode, AuthPanel};
use ui_account::license::LicensePanel;
use ui_account::verify::VerifyEmail;

/// Element ids, for tests.
pub mod ids {
    /// The offline or keychain notice.
    pub const NOTICE: &str = "settings-account-notice";
    /// The name heading.
    pub const NAME: &str = "settings-account-name";
    /// Last synced.
    pub const LAST_SYNCED: &str = "settings-account-synced";
    /// Run setup again.
    pub const RUN_SETUP: &str = "settings-account-setup";
    /// Show tips again.
    pub const SHOW_TIPS: &str = "settings-account-tips";
    /// Sign out.
    pub const SIGN_OUT: &str = "settings-account-sign-out";
    /// The reset error.
    pub const ERROR: &str = "settings-account-error";
}

type RowClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui_kit::App)>;

/// `ROLE_LABEL`.
pub fn role_label(role: OrgRole) -> &'static str {
    match role {
        OrgRole::Owner => "Owner",
        OrgRole::Admin => "Admin",
        OrgRole::Operator => "Operator",
        OrgRole::Viewer => "Viewer",
    }
}

/// The pane's state.
pub struct AccountPane {
    /// 073's licence panel (also the Licence & Devices pane).
    pub license: Entity<LicensePanel>,
    auth: Option<Entity<AuthPanel>>,
    verify: Option<Entity<VerifyEmail>>,
    account: Entity<Account>,
    /// Signing out.
    pub signing_out: bool,
    /// Resetting setup.
    pub resetting: bool,
    /// Why sign-out or reset failed.
    pub error: Option<String>,
}

impl AccountPane {
    pub(crate) fn new(
        account: Entity<Account>,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> Self {
        let license = cx.new(|cx| LicensePanel::new(account.clone(), false, window, cx));
        let session = account.read(cx).session().clone();
        let mut pane = Self {
            license,
            auth: None,
            verify: None,
            account,
            signing_out: false,
            resetting: false,
            error: None,
        };
        pane.ensure(&session, window, cx);
        pane
    }

    fn ensure(
        &mut self,
        session: &SessionSnapshot,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) {
        if session.state == CloudSessionState::SignedOut {
            if self.auth.is_none() {
                let account = self.account.clone();
                self.auth =
                    Some(cx.new(|cx| AuthPanel::new(account, AuthMode::SignIn, window, cx)));
            }
        } else {
            self.auth = None;
        }
        if needs_email_confirmation(session) {
            if self.verify.is_none() {
                let account = self.account.clone();
                self.verify =
                    Some(cx.new(|cx| VerifyEmail::new(account, false, false, window, cx)));
            }
        } else {
            self.verify = None;
        }
    }

    /// Follows the session: the sign-in form while signed out, the
    /// confirmation form while the email is unconfirmed.
    pub(crate) fn sync(&mut self, session: &SessionSnapshot, cx: &mut Context<SettingsWindow>) {
        if session.state != CloudSessionState::SignedOut {
            self.auth = None;
        }
        if !needs_email_confirmation(session) {
            self.verify = None;
        }
        cx.notify();
    }
}

impl SettingsWindow {
    /// Sign out.
    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        if self.account_pane.signing_out {
            return;
        }
        self.account_pane.signing_out = true;
        self.account_pane.error = None;
        let request = self.account.read(cx).backend().sign_out();
        let account = self.account.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.account_pane.signing_out = false;
                match result {
                    Ok(session) => account.update(cx, |a, cx| a.set_session(session, cx)),
                    Err(error) => this.account_pane.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Run setup again: resets setup, then closes so it shows.
    pub fn run_setup(&mut self, cx: &mut Context<Self>) {
        if self.account_pane.resetting {
            return;
        }
        self.account_pane.resetting = true;
        self.account_pane.error = None;
        let request = self.backend.reset_onboarding();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.account_pane.resetting = false;
                match result {
                    Ok(()) => cx.emit(SettingsEvent::RunSetup),
                    Err(error) => this.account_pane.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn render_account(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let session = self.account.read(cx).session().clone();
        // Make sure the forms that follow the session exist.
        self.account_pane.ensure(&session, window, cx);
        if let Some(auth) = &self.account_pane.auth {
            return auth.clone().into_any_element();
        }
        let user = session.user.clone();
        let name = user
            .as_ref()
            .map(|u| u.name.trim().to_owned())
            .filter(|n| !n.is_empty());
        let email = user.as_ref().map(|u| u.email.clone());
        let offline = session.state == CloudSessionState::Stale;
        let notice = if offline {
            Some(
                "Working offline. Kairo can’t reach the cloud right now — everything on this computer keeps working.",
            )
        } else if session.credentials_persisted == Some(false) {
            Some(ui_shell::view::CREDENTIALS_NOT_SAVED_NOTICE)
        } else {
            None
        };
        let row = |id: &'static str,
                   icon: KairoIcon,
                   label: String,
                   detail: Option<&'static str>,
                   value: Option<String>,
                   danger: bool,
                   on_click: Option<RowClick>,
                   disabled: bool| {
            let tone = if danger {
                k.danger_text
            } else {
                k.text_primary
            };
            let clickable = on_click.is_some();
            h_flex()
                .id(id)
                .test_support()
                .when(clickable, |this| this.role(Role::Button).tab_index(0))
                .aria_label(label.clone())
                .min_h(px(48.))
                .w_full()
                .gap_3()
                .items_center()
                .px_3()
                .py(px(10.))
                .child(
                    div()
                        .size_7()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius::md())
                        .bg(k.surface_elevated)
                        .text_color(if danger {
                            k.danger_text
                        } else {
                            k.text(TextStep::T300)
                        })
                        .child(Icon::new(icon).size(px(15.))),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .child(div().text_size(text::SM.size).text_color(tone).child(label))
                        .when_some(detail, |this, d| {
                            this.child(
                                div()
                                    .mt_0p5()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(d),
                            )
                        }),
                )
                .when_some(value, |this, v| {
                    this.child(
                        div()
                            .flex_shrink_0()
                            .truncate()
                            .text_size(text::SM.size)
                            .text_color(k.text(TextStep::T400))
                            .child(v),
                    )
                })
                .when(clickable && !danger, |this| {
                    this.child(
                        Icon::new(KairoIcon::ChevronRight)
                            .size(px(13.))
                            .text_color(k.text(TextStep::T500)),
                    )
                })
                .map(|this| match on_click {
                    Some(handler) if !disabled => this
                        .cursor_pointer()
                        .hover(|s| s.bg(k.surface_elevated))
                        .on_click(move |e, w, cx| handler(e, w, cx)),
                    Some(_) => this.opacity(0.5),
                    None => this,
                })
                .into_any_element()
        };
        let entity = cx.entity();
        let listener =
            |f: fn(&mut SettingsWindow, &mut Context<SettingsWindow>)| -> Option<RowClick> {
                let entity = entity.clone();
                Some(Box::new(move |_, _, cx| entity.update(cx, f)))
            };
        let card = |rows: Vec<AnyElement>| {
            gpui_kit::component::v_flex()
                .rounded(radius::lg())
                .overflow_hidden()
                .bg(k.surface_tertiary)
                .children(rows.into_iter().enumerate().map(|(ix, r)| {
                    div()
                        .relative()
                        .when(ix > 0, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left(px(48.))
                                    .right_0()
                                    .h_px()
                                    .bg(k.surface_border),
                            )
                        })
                        .child(r)
                }))
        };
        let mut info_rows = Vec::new();
        if let Some(org) = &session.org {
            info_rows.push(row(
                "settings-account-org",
                KairoIcon::Church,
                "Organization".into(),
                None,
                Some(org.name.clone()),
                false,
                None,
                false,
            ));
        }
        let synced = if offline {
            "Offline".to_owned()
        } else {
            session.last_synced_at.map_or_else(
                || "Not yet".to_owned(),
                |at| relative_time(at, kairo_domain::js::now_ms()),
            )
        };
        info_rows.push(row(
            ids::LAST_SYNCED,
            KairoIcon::Clock,
            "Last synced".into(),
            None,
            Some(synced),
            false,
            None,
            false,
        ));
        let setup_rows = vec![
            row(
                ids::RUN_SETUP,
                KairoIcon::RefreshCw,
                "Run setup again".into(),
                Some("Screens and your church profile. Nothing is reset."),
                None,
                false,
                listener(|this, cx| this.run_setup(cx)),
                self.account_pane.resetting,
            ),
            row(
                ids::SHOW_TIPS,
                KairoIcon::Lightbulb,
                "Show tips again".into(),
                Some("A quick walk round the Operator."),
                None,
                false,
                listener(|_, cx| cx.emit(SettingsEvent::ShowTips)),
                false,
            ),
        ];
        let sign_out = row(
            ids::SIGN_OUT,
            KairoIcon::LogOut,
            if self.account_pane.signing_out {
                "Signing out…".into()
            } else {
                "Sign out".into()
            },
            None,
            None,
            true,
            listener(|this, cx| this.sign_out(cx)),
            self.account_pane.signing_out,
        );
        v_flex()
            .gap_5()
            .when_some(notice, |this, notice| {
                this.child(widgets::notice(ids::NOTICE, notice, cx))
            })
            .child(
                v_flex()
                    .items_center()
                    .pb_1()
                    .pt_3()
                    .child(widgets::avatar(
                        initials(name.as_deref(), email.as_deref()),
                        84.0,
                        cx,
                    ))
                    .child(
                        div()
                            .id(ids::NAME)
                            .test_support()
                            .role(Role::Heading)
                            .mt_4()
                            .text_size(px(20.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child(
                                name.clone()
                                    .or_else(|| email.clone())
                                    .unwrap_or_else(|| "Signed in".into()),
                            ),
                    )
                    .when(name.is_some() && email.is_some(), |this| {
                        this.child(
                            div()
                                .mt_0p5()
                                .text_size(text::SM.size)
                                .text_color(k.text(TextStep::T400))
                                .child(email.clone().unwrap_or_default()),
                        )
                    })
                    .when_some(session.org.clone(), |this, org| {
                        this.child(
                            div()
                                .mt_1()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T500))
                                .child(format!("{} · {}", org.name, role_label(org.role))),
                        )
                    }),
            )
            .when_some(self.account_pane.verify.clone(), |this, verify| {
                this.child(
                    div()
                        .rounded(radius::lg())
                        .bg(k.surface_tertiary)
                        .px(px(14.))
                        .py_3()
                        .child(verify),
                )
            })
            .child(self.account_pane.license.clone())
            .child(card(info_rows))
            .child(card(setup_rows))
            .child(card(vec![sign_out]))
            .when_some(self.account_pane.error.clone(), |this, error| {
                this.child(
                    div()
                        .id(ids::ERROR)
                        .test_support()
                        .role(Role::Alert)
                        .text_size(text::XS.size)
                        .text_color(k.danger_text)
                        .child(SharedString::from(error)),
                )
            })
            .into_any_element()
    }
}
