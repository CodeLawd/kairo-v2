//! First-run setup (\`OnboardingWizard.tsx\`, \`StepAccount.tsx\`,
//! \`StepOutput.tsx\`, \`WelcomeView.tsx\`): the tour, the account, this
//! computer's screens, then the closing welcome. Shown signed out; the shell
//! keeps it open across sign-in so the flow carries on.

use crate::auth::{AuthEvent, AuthMode, AuthPanel};
use crate::backend::OnboardingWrite;
use crate::display_picker::{DisplayBound, DisplayPicker, Picked};
use crate::license::LicensePanel;
use crate::model::{Account, AccountChanged};
use crate::ob::{self, Clock};
use crate::tour::{self, TOUR_SLIDES};
use crate::verify::VerifyEmail;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{FocusTrapElement, Icon, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder, px,
    relative,
};
use kairo_domain::cloud::{can_enter_app, is_step_complete, needs_email_confirmation, onboarding_steps};
use kairo_domain::displays::DisplayBinding;
use kairo_domain::overlay::OverlayOutputKind;
use kairo_domain::overlay_defaults::make_projector_output;
use kairo_domain::program::StageDisplayConfig;
use kairo_domain::settings::{AppSettings, OnboardingState, OnboardingStepId};
use kairo_ui_kit::brand::{KairoMark, MarkSize, PRODUCT_NAME};
use kairo_ui_kit::onboarding::{OnboardingColors, StageGlow, colors};
use kairo_ui_kit::tokens::{TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use std::time::Instant;

/// Where a quick start goes (\`NavRoute\`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickStart {
    /// Find a verse.
    Scripture,
    /// Add a song.
    Lyrics,
    /// Import slides.
    Documents,
}

/// What the wizard asks of the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardEvent {
    /// Close setup (\`onDismiss\`): open \`route\` if given; \`coach\` runs the
    /// tips that follow.
    Dismiss {
        /// The workspace to open.
        route: Option<QuickStart>,
        /// Whether the coach tour follows.
        coach: bool,
    },
    /// Open the full Screens window (after closing setup, no tips).
    OpenScreens,
}

/// Element ids, for tests.
pub mod ids {
    /// The dialog.
    pub const DIALOG: &str = "onboarding-wizard";
    /// Continue.
    pub const CONTINUE: &str = "onboarding-continue";
    /// Skip Tour.
    pub const SKIP_TOUR: &str = "onboarding-skip-tour";
    /// Set Up Later.
    pub const LATER: &str = "onboarding-later";
    /// Back.
    pub const BACK: &str = "onboarding-back";
    /// The error.
    pub const ERROR: &str = "onboarding-error";
    /// The page title.
    pub const TITLE: &str = "onboarding-slide-title";
    /// Start using Kairo.
    pub const START: &str = "onboarding-start";
    /// Open full Screen Configuration.
    pub const OPEN_SCREENS: &str = "onboarding-open-screens";
    /// Use a different email.
    pub const DIFFERENT_EMAIL: &str = "onboarding-different-email";
}

/// Footer back-link names.
fn step_name(step: OnboardingStepId) -> &'static str {
    match step {
        OnboardingStepId::Welcome => "Welcome",
        OnboardingStepId::Account => "Account",
        _ => "Screen Configuration",
    }
}

/// What a write to setup does, run in order.
#[derive(Clone, Debug)]
enum Job {
    Seed,
    Write(OnboardingWrite),
    /// Write the next step only when not signed in and ready.
    LeaveTour { ready: bool },
}

/// The wizard.
pub struct OnboardingWizard {
    account: Entity<Account>,
    focus: FocusHandle,
    busy: bool,
    pending: bool,
    saving_screen: bool,
    error: Option<SharedString>,
    error_at: Instant,
    slide: usize,
    leaving: Option<(usize, Instant)>,
    dir: f32,
    page_at: Instant,
    welcomed: bool,
    auth_mode: AuthMode,
    auth: Option<Entity<AuthPanel>>,
    verify: Option<Entity<VerifyEmail>>,
    license: Option<Entity<LicensePanel>>,
    audience: Option<Entity<DisplayPicker>>,
    stage: Option<Entity<DisplayPicker>>,
    output_error: Option<SharedString>,
    signing_out: bool,
    opened_at: Instant,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WizardEvent> for OnboardingWizard {}

impl Focusable for OnboardingWizard {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl OnboardingWizard {
    /// The wizard over \`account\`, focused.
    pub fn new(account: Entity<Account>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let subscriptions = vec![cx.subscribe_in(&account, window, |this, _, event: &AccountChanged, window, cx| {
            match event {
                AccountChanged::Displays | AccountChanged::Settings => this.refresh_pickers(window, cx),
                _ => {}
            }
            this.carry_on_from_account(cx);
            cx.notify();
        })];
        let now = Instant::now();
        let mut wizard = Self {
            account,
            focus,
            busy: false,
            pending: false,
            saving_screen: false,
            error: None,
            error_at: now,
            slide: 0,
            leaving: None,
            dir: 1.0,
            page_at: now,
            welcomed: false,
            // A first launch most likely needs a new account.
            auth_mode: AuthMode::SignUp,
            auth: None,
            verify: None,
            license: None,
            audience: None,
            stage: None,
            output_error: None,
            signing_out: false,
            opened_at: now,
            _subscriptions: subscriptions,
        };
        wizard.carry_on_from_account(cx);
        wizard
    }

    // ── State ───────────────────────────────────────────────────────────

    fn state(&self, cx: &gpui_kit::App) -> OnboardingState {
        self.account.read(cx).onboarding().clone()
    }

    fn steps() -> &'static [OnboardingStepId] {
        onboarding_steps()
    }

    fn index(&self, cx: &gpui_kit::App) -> usize {
        let current = self.state(cx).current_step;
        Self::steps().iter().position(|s| *s == current).unwrap_or(0)
    }

    fn current(&self, cx: &gpui_kit::App) -> OnboardingStepId {
        Self::steps()[self.index(cx)]
    }

    /// Signed in, confirmed and activated.
    pub fn account_ready(&self, cx: &gpui_kit::App) -> bool {
        let account = self.account.read(cx);
        can_enter_app(account.session()) && account.license().is_some_and(|l| l.activated)
    }

    fn touring(&self, cx: &gpui_kit::App) -> bool {
        self.current(cx) == OnboardingStepId::Welcome
    }

    /// Nothing past the account opens until the session can enter the app.
    fn on_account(&self, cx: &gpui_kit::App) -> bool {
        !self.touring(cx) && (self.current(cx) == OnboardingStepId::Account || !self.account_ready(cx))
    }

    fn locked(&self) -> bool {
        self.busy || self.saving_screen
    }

    /// Whether the closing welcome is showing.
    pub fn welcomed(&self) -> bool {
        self.welcomed
    }

    /// The tour slide showing.
    pub fn slide(&self) -> usize {
        self.slide
    }

    /// The error showing.
    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    fn back_target(&self, cx: &gpui_kit::App) -> Option<OnboardingStepId> {
        let ready = self.account_ready(cx);
        Self::steps()[..self.index(cx)]
            .iter()
            .rev()
            .find(|s| **s != OnboardingStepId::Account || !ready)
            .copied()
    }

    // ── Writes ──────────────────────────────────────────────────────────

    /// \`apply\`: one write at a time; \`then\` runs on success.
    fn apply(&mut self, jobs: Vec<Job>, then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> bool {
        if self.pending {
            return false;
        }
        self.pending = true;
        self.busy = true;
        self.error = None;
        cx.notify();
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let mut result: Result<Option<OnboardingState>, String> = Ok(None);
            for job in jobs {
                let step = match job {
                    Job::Seed => {
                        let request = cx.update(|cx| seed_church(&account, cx));
                        match request {
                            Some((request, church)) => match request.await {
                                Ok(()) => {
                                    let _ = cx.update(|cx| {
                                        account.update(cx, |a, cx| a.update_settings(|s| s.church = church, cx))
                                    });
                                    Ok(None)
                                }
                                Err(e) => Err(e),
                            },
                            None => Ok(None),
                        }
                    }
                    Job::Write(write) => {
                        let request = cx.update(|cx| account.read(cx).backend().onboarding(write));
                        request.await.map(Some)
                    }
                    Job::LeaveTour { ready } => {
                        let writes: &[OnboardingWrite] = if ready {
                            &[OnboardingWrite::Complete(OnboardingStepId::Account), OnboardingWrite::SetCurrent(OnboardingStepId::Output)]
                        } else {
                            &[OnboardingWrite::SetCurrent(OnboardingStepId::Account)]
                        };
                        let mut last = Ok(None);
                        for write in writes {
                            let request = cx.update(|cx| account.read(cx).backend().onboarding(*write));
                            last = request.await.map(Some);
                            if last.is_err() {
                                break;
                            }
                        }
                        last
                    }
                };
                match step {
                    Ok(Some(state)) => result = Ok(Some(state)),
                    Ok(None) => {}
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.pending = false;
                this.busy = false;
                match result {
                    Ok(state) => {
                        if let Some(state) = state {
                            this.account.update(cx, |a, cx| a.set_onboarding(state, cx));
                        }
                        this.page_at = Instant::now();
                        then(this, cx);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "[Onboarding] setup could not be saved");
                        this.error = Some("Your setup could not be saved. Please try again.".into());
                        this.error_at = Instant::now();
                    }
                }
                cx.notify();
            });
        })
        .detach();
        true
    }

    /// Signed in and confirmed on the account page: carry on to screens.
    fn carry_on_from_account(&mut self, cx: &mut Context<Self>) {
        if !self.account_ready(cx) || self.current(cx) != OnboardingStepId::Account {
            return;
        }
        self.dir = 1.0;
        self.apply(
            vec![
                Job::Seed,
                Job::Write(OnboardingWrite::Complete(OnboardingStepId::Account)),
                Job::Write(OnboardingWrite::SetCurrent(OnboardingStepId::Output)),
            ],
            |_, _| {},
            cx,
        );
    }

    /// Ends setup; finishing lands on the welcome unless \`welcome\` is false.
    pub fn finish(&mut self, welcome: bool, cx: &mut Context<Self>) -> bool {
        if !self.account_ready(cx) || self.saving_screen {
            return false;
        }
        self.apply(
            vec![Job::Seed, Job::Write(OnboardingWrite::Finish)],
            move |this, cx| {
                if welcome {
                    this.dir = 1.0;
                    this.welcomed = true;
                    this.page_at = Instant::now();
                } else {
                    cx.emit(WizardEvent::Dismiss { route: None, coach: false });
                    cx.emit(WizardEvent::OpenScreens);
                }
            },
            cx,
        )
    }

    /// Moves to tour slide \`next\`, clamped.
    pub fn go_to_slide(&mut self, next: isize, cx: &mut Context<Self>) {
        let clamped = next.clamp(0, TOUR_SLIDES.len() as isize - 1) as usize;
        if clamped == self.slide {
            return;
        }
        self.dir = if clamped > self.slide { 1.0 } else { -1.0 };
        self.leaving = Some((self.slide, Instant::now()));
        self.slide = clamped;
        self.page_at = Instant::now();
        cx.notify();
    }

    /// Leaves the tour: to the account, or straight to screens when ready.
    pub fn leave_tour(&mut self, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        self.dir = 1.0;
        let ready = self.account_ready(cx);
        self.apply(
            vec![Job::Write(OnboardingWrite::Complete(OnboardingStepId::Welcome)), Job::LeaveTour { ready }],
            |_, _| {},
            cx,
        );
    }

    /// Continue on a setup page.
    pub fn advance(&mut self, cx: &mut Context<Self>) {
        if self.locked() || !self.account_ready(cx) {
            return;
        }
        self.dir = 1.0;
        let index = self.index(cx);
        let current = Self::steps()[index];
        let last = index == Self::steps().len() - 1;
        let complete = self
            .account
            .read(cx)
            .settings()
            .is_some_and(|s| is_step_complete(current, s));
        let mut jobs = vec![Job::Write(if complete {
            OnboardingWrite::Complete(current)
        } else {
            OnboardingWrite::Skip(current)
        })];
        if last {
            jobs.push(Job::Seed);
            jobs.push(Job::Write(OnboardingWrite::Finish));
        } else {
            jobs.push(Job::Write(OnboardingWrite::SetCurrent(Self::steps()[index + 1])));
        }
        self.apply(
            jobs,
            move |this, _| {
                if last {
                    this.welcomed = true;
                    this.page_at = Instant::now();
                }
            },
            cx,
        );
    }

    /// Back, skipping the account page once signed in.
    pub fn back(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.back_target(cx) else { return };
        if self.locked() {
            return;
        }
        self.dir = -1.0;
        // Walking back into the tour lands on its last slide.
        if target == OnboardingStepId::Welcome {
            self.slide = TOUR_SLIDES.len() - 1;
            self.leaving = None;
        }
        self.apply(vec![Job::Write(OnboardingWrite::SetCurrent(target))], |_, _| {}, cx);
    }

    fn escape(&mut self, cx: &mut Context<Self>) {
        // The account cannot be skipped; once in, Escape is "set up later".
        if self.welcomed {
            cx.emit(WizardEvent::Dismiss { route: None, coach: true });
        } else if self.account_ready(cx) {
            self.finish(true, cx);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                self.escape(cx);
            }
            "right" | "left" if self.touring(cx) && !self.typing(window, cx) => {
                cx.stop_propagation();
                let step = if event.keystroke.key == "right" { 1 } else { -1 };
                self.go_to_slide(self.slide as isize + step, cx);
            }
            _ => {}
        }
    }

    fn typing(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        // Arrow keys belong to a focused text field.
        window.focused(cx).is_some_and(|f| f != self.focus)
            && !self.touring(cx)
    }

    // ── Pages ───────────────────────────────────────────────────────────

    fn auth(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<AuthPanel> {
        if let Some(auth) = &self.auth
            && auth.read(cx).mode() == self.auth_mode
        {
            return auth.clone();
        }
        let account = self.account.clone();
        let mode = self.auth_mode;
        let auth = cx.new(|cx| AuthPanel::new(account, mode, window, cx));
        self._subscriptions.push(cx.subscribe(&auth, |this, _, event: &AuthEvent, cx| {
            if let AuthEvent::ModeChanged(mode) = event {
                this.auth_mode = *mode;
                cx.notify();
            }
        }));
        self.auth = Some(auth.clone());
        auth
    }

    fn use_different_email(&mut self, cx: &mut Context<Self>) {
        self.signing_out = true;
        cx.notify();
        let request = self.account.read(cx).backend().sign_out();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.signing_out = false;
                if let Ok(session) = result {
                    this.account.update(cx, |a, cx| a.set_session(session, cx));
                    this.auth_mode = AuthMode::SignUp;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_account(&mut self, clock: Clock, ob: &OnboardingColors, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let session = self.account.read(cx).session().clone();
        let ready = can_enter_app(&session);
        let verifying = needs_email_confirmation(&session);
        let sign_up = self.auth_mode == AuthMode::SignUp;
        let (title, blurb) = account_copy(ready, verifying, sign_up, session.user.as_ref().map(|u| u.email.as_str()));
        let body = if ready {
            let account = self.account.clone();
            let license = self
                .license
                .get_or_insert_with(|| cx.new(|cx| LicensePanel::new(account, false, window, cx)))
                .clone();
            license.into_any_element()
        } else if verifying {
            let account = self.account.clone();
            let verify = self
                .verify
                .get_or_insert_with(|| cx.new(|cx| VerifyEmail::new(account, false, true, window, cx)))
                .clone();
            v_flex()
                .items_center()
                .gap_6()
                .child(verify)
                .child(text_link(ids::DIFFERENT_EMAIL, if self.signing_out { "Signing out…" } else { "Use a different email" }, self.signing_out, None, cx.listener(|this, _, _, cx| this.use_different_email(cx)), &k))
                .into_any_element()
        } else {
            self.auth(window, cx).into_any_element()
        };
        v_flex()
            .items_center()
            .max_w(px(720.))
            .mx_auto()
            .pt(px(44.))
            .px(px(32.))
            .pb(px(32.))
            .child(clock.ob_in(
                div().id(ids::TITLE).test_support().role(Role::Heading).aria_label(SharedString::from(title.clone()))
                    .text_size(px(32.)).font_weight(FontWeight::MEDIUM).text_color(ob.text).text_center().child(title),
                0, 0.0, 28.0,
            ))
            .child(clock.rise(div().mt_2().max_w(px(44.0 * 7.5)).text_center().text_size(px(14.)).text_color(k.text(TextStep::T400)).child(blurb), 1, 0.0))
            .child(clock.ob_in(div().mt_8().w_full().max_w(px(380.)).child(body), 2, 0.0, 28.0))
            .into_any_element()
    }

    fn output_targets(&self, cx: &gpui_kit::App) -> Option<OutputTargets> {
        let settings = self.account.read(cx).settings()?;
        Some(output_targets(settings))
    }

    fn refresh_pickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(targets) = self.output_targets(cx) else { return };
        let displays = self.account.read(cx).displays().to_vec();
        if let Some(audience) = &self.audience {
            audience.update(cx, |p, cx| p.update(&targets.audience, &displays, &targets.audience_others, window, cx));
        }
        if let Some(stage) = &self.stage {
            stage.update(cx, |p, cx| p.update(&targets.stage, &displays, &targets.stage_others, window, cx));
        }
    }

    fn save_output(&mut self, audience: bool, binding: DisplayBinding, cx: &mut Context<Self>) {
        let Some(settings) = self.account.read(cx).settings().cloned() else { return };
        let backend = self.account.read(cx).backend();
        let enabled = binding.display_id.is_some();
        enum Save {
            Overlay(kairo_domain::settings::OverlaySettings),
            Presentation(kairo_domain::program::PresentationSettings),
        }
        let save = if audience {
            let mut overlay = settings.overlay.clone();
            let screen = overlay.outputs.iter().find(|o| o.kind == OverlayOutputKind::Screen).cloned().unwrap_or_else(|| make_projector_output(0.0));
            let mut next = screen.clone();
            next.display_id = binding.display_id;
            next.display_label = binding.display_label.clone();
            next.display_size = binding.display_size.clone();
            next.enabled = enabled;
            if overlay.outputs.iter().any(|o| o.id == screen.id) {
                for output in overlay.outputs.iter_mut() {
                    if output.id == screen.id {
                        *output = next.clone();
                    }
                }
            } else {
                overlay.outputs.push(next);
            }
            Save::Overlay(overlay)
        } else {
            let mut presentation = settings.presentation.clone();
            if let Some(first) = presentation.stage_displays.first_mut() {
                first.display_id = binding.display_id;
                first.display_label = binding.display_label.clone();
                first.display_size = binding.display_size.clone();
                first.enabled = enabled;
            } else if enabled {
                presentation.stage_displays = vec![new_stage_display(&binding)];
            } else {
                return;
            }
            Save::Presentation(presentation)
        };
        self.saving_screen = true;
        self.output_error = None;
        self.set_pickers_disabled(true, cx);
        cx.notify();
        let request = match &save {
            Save::Overlay(o) => backend.save_overlay(o.clone()),
            Save::Presentation(p) => backend.save_presentation(p.clone()),
        };
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.saving_screen = false;
                this.set_pickers_disabled(false, cx);
                match result {
                    Ok(()) => this.account.update(cx, |a, cx| {
                        a.update_settings(
                            |s| match save {
                                Save::Overlay(o) => s.overlay = o,
                                Save::Presentation(p) => s.presentation = p,
                            },
                            cx,
                        )
                    }),
                    Err(_) => {
                        this.output_error = Some("The screen could not be saved. Please try again.".into());
                        this.error_at = Instant::now();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_pickers_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        for picker in [&self.audience, &self.stage].into_iter().flatten() {
            picker.update(cx, |p, cx| p.set_disabled(disabled, cx));
        }
    }

    fn render_output(&mut self, clock: Clock, ob: &OnboardingColors, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let displays = self.account.read(cx).displays().to_vec();
        let targets = self.output_targets(cx);
        if let Some(targets) = &targets {
            if self.audience.is_none() {
                let t = targets.clone();
                let d = displays.clone();
                let picker = cx.new(|cx| DisplayPicker::new("ob-audience", "Audience Screen", &t.audience, &d, &t.audience_others, window, cx));
                self._subscriptions.push(cx.subscribe(&picker, |this, _, Picked(binding), cx| this.save_output(true, binding.clone(), cx)));
                self.audience = Some(picker);
            }
            if self.stage.is_none() {
                let t = targets.clone();
                let d = displays.clone();
                let picker = cx.new(|cx| DisplayPicker::new("ob-stage", "Stage Screen", &t.stage, &d, &t.stage_others, window, cx));
                self._subscriptions.push(cx.subscribe(&picker, |this, _, Picked(binding), cx| this.save_output(false, binding.clone(), cx)));
                self.stage = Some(picker);
            }
        }
        let row = |label: &'static str, hint: &'static str, picker: Option<Entity<DisplayPicker>>, i: usize| {
            clock.ob_in(
                h_flex()
                    .w_full()
                    .gap_6()
                    .items_center()
                    .px_6()
                    .py_5()
                    .rounded(px(16.))
                    .bg(ob.card[0])
                    .child(
                        v_flex()
                            .flex_1()
                            .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(k.text(TextStep::T200)).child(label))
                            .child(div().mt_1().text_size(px(13.)).text_color(k.text(TextStep::T500)).child(hint)),
                    )
                    .child(div().flex_basis(relative(0.565)).children(picker)),
                i,
                0.0,
                28.0,
            )
        };
        let many = displays.len() > 1;
        v_flex()
            .items_center()
            .max_w(px(720.))
            .mx_auto()
            .pt(px(44.))
            .px(px(32.))
            .pb(px(32.))
            .child(clock.ob_in(
                div().id(ids::TITLE).test_support().role(Role::Heading).aria_label("Configure Screens").text_size(px(32.)).font_weight(FontWeight::MEDIUM).text_color(ob.text).child("Configure Screens"),
                0, 0.0, 28.0,
            ))
            .child(
                v_flex()
                    .w_full()
                    .mt_8()
                    .gap_2()
                    .child(row("Audience Screen", "What the room sees", self.audience.clone(), 1))
                    .child(row("Stage Screen", "What the pastor and band see", self.stage.clone(), 2)),
            )
            .when_some(self.output_error.clone(), |el, error| el.child(error_line(error, self.error_at, k.danger_text).mt_4()))
            .child(clock.rise(
                div().mt_6().text_center().text_size(px(13.)).text_color(k.text(TextStep::T400)).child(if many {
                    "More screens and stage layouts can be set up in Screens."
                } else {
                    "No projector or TV connected yet? Skip this and connect one before your service."
                }),
                0,
                0.0,
            ))
            .child(clock.rise(
                div().mt_4().child(text_link(
                    ids::OPEN_SCREENS,
                    "Open full Screen Configuration",
                    self.saving_screen,
                    Some(KairoIcon::ChevronRight),
                    cx.listener(|this, _, _, cx| {
                        this.finish(false, cx);
                    }),
                    &k,
                )
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(k.notice_text)),
                0,
                0.0,
            ))
            .into_any_element()
    }

    fn render_welcome(&mut self, clock: Clock, ob: &OnboardingColors, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let settings_name = self.account.read(cx).settings().map(|s| s.church.name.clone()).unwrap_or_default();
        let org = self.account.read(cx).session().org.as_ref().map(|o| o.name.clone()).unwrap_or_default();
        let church = if settings_name.is_empty() { org } else { settings_name };
        let church = church.trim().to_owned();
        let ready_line = if church.is_empty() {
            "You’re ready for your next service.".to_owned()
        } else {
            format!("{church} is ready for your next service.")
        };
        let quick = [
            (QuickStart::Scripture, "Find a verse", "Search by reference or words", KairoIcon::BookOpen, "quickstart-scripture"),
            (QuickStart::Lyrics, "Add a song", "Import or type in lyrics", KairoIcon::Music2, "quickstart-lyrics"),
            (QuickStart::Documents, "Import slides", "PowerPoint or PDF", KairoIcon::Presentation, "quickstart-documents"),
        ];
        let focus_progress = if clock.still { 1.0 } else { kairo_ui_kit::motion::OB_FOCUS.sample((clock.t / 1100.0).clamp(0.0, 1.0)).opacity };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .py(px(40.))
            .px(px(32.))
            .child(div().opacity(focus_progress).child(KairoMark::new(MarkSize::Large)))
            .child(animated_title("Welcome to Kairo", clock, ob))
            .child(clock.rise(div().mt_3().text_center().text_size(px(17.)).text_color(k.text(TextStep::T400)).child(ready_line), 0, 1350.0))
            .child(
                h_flex().mt(px(40.)).w_full().max_w(px(640.)).gap_3().children(quick.into_iter().enumerate().map(|(i, (route, title, detail, icon, id))| {
                    clock.rise(
                        v_flex()
                            .id(id)
                            .test_support()
                            .role(Role::Button)
                            .aria_label(title)
                            .flex_1()
                            .items_start()
                            .p(px(18.))
                            .rounded(px(16.))
                            .bg(ob.card[0])
                            .cursor_pointer()
                            .hover(|s| s.bg(ob.card[1]))
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(WizardEvent::Dismiss { route: Some(route), coach: true })))
                            .child(Icon::new(icon).size(px(18.)).text_color(k.text(TextStep::T300)))
                            .child(div().mt_4().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(k.text(TextStep::T200)).child(title))
                            .child(div().mt_1().text_size(px(12.)).text_color(k.text(TextStep::T500)).child(detail)),
                        i,
                        1550.0,
                    )
                })),
            )
            .child(clock.rise(cta(ids::START, "Start using Kairo", false, ob, cx.listener(|_, _, _, cx| cx.emit(WizardEvent::Dismiss { route: None, coach: true }))).mt(px(40.)), 0, 1850.0))
            .into_any_element()
    }

    fn render_tour(&mut self, ob: &OnboardingColors, width: f32, still: bool, cx: &mut Context<Self>) -> AnyElement {
        let now = Instant::now();
        let clock = clock_since(self.page_at, self.dir, still, now);
        let mut stage = div().relative().flex_1().min_h_0().child(
            div().absolute().inset_0().child(tour::render_slide(&TOUR_SLIDES[self.slide], clock, width, ob, cx)),
        );
        if let Some((leaving, since)) = self.leaving {
            let t = now.saturating_duration_since(since).as_secs_f32() * 1000.0;
            if still || !tour::leaving_visible(t) {
                self.leaving = None;
            } else {
                let rest = Clock::still();
                stage = stage.child(ob::out(div().absolute().inset_0(), t, self.dir).child(tour::render_slide(&TOUR_SLIDES[leaving], rest, width, ob, cx)));
            }
        }
        let k = *cx.kairo();
        let dots = h_flex()
            .id("onboarding-dots")
            .role(Role::Group)
            .aria_label("Tour slides")
            .justify_center()
            .items_center()
            .gap_2()
            .pt(px(6.))
            .pb(px(20.))
            .children(TOUR_SLIDES.iter().enumerate().map(|(i, slide)| {
                let current = i == self.slide;
                div()
                    .id(SharedString::from(format!("onboarding-dot-{i}")))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!("{}, slide {} of {}", slide.headline, i + 1, TOUR_SLIDES.len())))
                    .h(px(8.))
                    .w(px(if current { 24.0 } else { 8.0 }))
                    .rounded_full()
                    .bg(if current { k.text_primary } else { ob.text.opacity(0.2) })
                    .cursor_pointer()
                    .hover(move |s| if current { s } else { s.bg(ob_text_hover(ob.text)) })
                    .on_click(cx.listener(move |this, _, _, cx| this.go_to_slide(i as isize, cx)))
            }));
        v_flex().size_full().child(stage).child(dots).into_any_element()
    }
}

fn ob_text_hover(text: gpui_kit::Hsla) -> gpui_kit::Hsla {
    text.opacity(0.45)
}

/// Time since \`since\` as a [\`Clock\`].
fn clock_since(since: Instant, dir: f32, still: bool, now: Instant) -> Clock {
    if still {
        return Clock::still();
    }
    Clock { t: now.saturating_duration_since(since).as_secs_f32() * 1000.0, dir, still }
}

/// The account page's title and blurb.
pub fn account_copy(ready: bool, verifying: bool, sign_up: bool, email: Option<&str>) -> (String, String) {
    if ready {
        return ("Activate this computer".into(), "Turn on live transcription, scripture lookup and lyric tools for your church.".into());
    }
    if verifying {
        return ("Confirm your email".into(), format!("Enter the 6-digit code we sent to {}.", email.unwrap_or("your email")));
    }
    if sign_up {
        ("Create your account".into(), "Your church’s songs, themes and settings follow you to every Kairo computer.".into())
    } else {
        (format!("Sign in to {PRODUCT_NAME}"), "Welcome back. Your church’s library comes with you.".into())
    }
}

/// The church profile to save before setup moves on: the org's name and
/// this computer's zone, filling only what is empty. \`None\` when nothing
/// changes.
pub fn seeded_church(settings: &AppSettings, org: Option<&str>, local_zone: &str) -> Option<kairo_domain::settings::ChurchProfile> {
    let church = &settings.church;
    let mut next = church.clone();
    if church.name.trim().is_empty() {
        next.name = org.map(str::trim).unwrap_or("").to_owned();
    } else {
        next.name = church.name.trim().to_owned();
    }
    if church.timezone.is_empty() {
        next.timezone = local_zone.to_owned();
    }
    (next.name != church.name || next.timezone != church.timezone).then_some(next)
}

type SeedRequest = (crate::Reply<Result<(), String>>, kairo_domain::settings::ChurchProfile);

fn seed_church(account: &Entity<Account>, cx: &gpui_kit::App) -> Option<SeedRequest> {
    let account = account.read(cx);
    let settings = account.settings()?;
    let org = account.session().org.as_ref().map(|o| o.name.as_str());
    let next = seeded_church(settings, org, account.local_zone())?;
    Some((account.backend().save_church(next.clone()), next))
}

/// The two rows' targets and what else is on displays.
#[derive(Clone, Debug)]
pub struct OutputTargets {
    /// The audience screen.
    pub audience: DisplayBound,
    /// Displays taken for the audience row.
    pub audience_others: Vec<DisplayBound>,
    /// The first stage display.
    pub stage: DisplayBound,
    /// Displays taken for the stage row.
    pub stage_others: Vec<DisplayBound>,
}

fn bound_of(name: &str, id: Option<f64>, label: &str, size: Option<kairo_domain::overlay::Size>) -> DisplayBound {
    DisplayBound { name: name.into(), binding: DisplayBinding { display_id: id, display_label: label.into(), display_size: size } }
}

/// \`StepOutput\`'s targets from settings.
pub fn output_targets(settings: &AppSettings) -> OutputTargets {
    let outputs = &settings.overlay.outputs;
    let screen = outputs.iter().find(|o| o.kind == OverlayOutputKind::Screen).cloned().unwrap_or_else(|| make_projector_output(0.0));
    let stages = &settings.presentation.stage_displays;
    let enabled_screens: Vec<_> = outputs.iter().filter(|o| o.kind == OverlayOutputKind::Screen && o.enabled).collect();
    let enabled_stages: Vec<_> = stages.iter().filter(|s| s.enabled).collect();
    let screen_bound = |o: &kairo_domain::overlay::OverlayOutput| bound_of(&o.name, o.display_id, &o.display_label, o.display_size.clone());
    let stage_bound = |s: &StageDisplayConfig| bound_of(&s.name, s.display_id, &s.display_label, s.display_size.clone());
    let audience_others = enabled_screens.iter().filter(|o| o.id != screen.id).map(|o| screen_bound(o)).chain(enabled_stages.iter().map(|s| stage_bound(s))).collect();
    let first_stage = stages.first();
    let stage_others = enabled_screens.iter().map(|o| screen_bound(o)).chain(enabled_stages.iter().filter(|s| Some(s.id.as_str()) != first_stage.map(|f| f.id.as_str())).map(|s| stage_bound(s))).collect();
    OutputTargets {
        audience: bound_of("Audience screen", screen.display_id, &screen.display_label, screen.display_size.clone()),
        audience_others,
        stage: match first_stage {
            Some(s) => stage_bound(s),
            None => bound_of("Stage display", None, "", None),
        },
        stage_others,
    }
}

/// \`stage-<Date.now() base 36>\`.
fn new_stage_display(binding: &DisplayBinding) -> StageDisplayConfig {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let mut config: StageDisplayConfig = serde_default_stage();
    config.id = format!("stage-{}", base36(now));
    config.name = "Stage display".into();
    config.enabled = true;
    config.display_id = binding.display_id;
    config.display_label = binding.display_label.clone();
    config.display_size = binding.display_size.clone();
    config.show_next = true;
    config.show_clock = true;
    config.show_timer = true;
    config
}

fn serde_default_stage() -> StageDisplayConfig {
    StageDisplayConfig {
        id: String::new(),
        name: String::new(),
        enabled: false,
        display_id: None,
        display_label: String::new(),
        display_size: None,
        show_next: true,
        show_clock: true,
        show_timer: true,
        extra: Default::default(),
    }
}

/// \`Number.prototype.toString(36)\`.
pub fn base36(mut value: u64) -> String {
    if value == 0 {
        return "0".into();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while value > 0 {
        out.push(digits[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn error_line(error: SharedString, since: Instant, color: gpui_kit::Hsla) -> gpui_kit::Stateful<gpui_kit::Div> {
    // \`.onboarding-error\`: shakes once.
    let t = Instant::now().saturating_duration_since(since).as_secs_f32() * 1000.0;
    let x = kairo_ui_kit::motion::OB_SHAKE.sample((t / 350.0).clamp(0.0, 1.0)).x;
    div()
        .id(ids::ERROR)
        .test_support()
        .role(Role::Alert)
        .aria_label(error.clone())
        .relative()
        .left(px(x))
        .text_size(px(12.))
        .text_color(color)
        .child(error)
}

fn text_link(
    id: &'static str,
    label: &'static str,
    disabled: bool,
    icon: Option<KairoIcon>,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    k: &kairo_ui_kit::KairoColors,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let hover = k.text_primary;
    let el = h_flex()
        .id(id)
        .test_support()
        .role(Role::Button)
        .aria_label(label)
        .items_center()
        .gap(px(6.))
        .text_size(px(13.))
        .text_color(k.text(TextStep::T400))
        .when_some(icon.filter(|i| *i == KairoIcon::ChevronLeft), |el, icon| el.child(Icon::new(icon).size(px(14.))))
        .child(label)
        .when_some(icon.filter(|i| *i != KairoIcon::ChevronLeft), |el, icon| el.child(Icon::new(icon).size(px(14.))));
    if disabled {
        el.opacity(0.4)
    } else {
        el.cursor_pointer().hover(move |s| s.text_color(hover)).on_click(on_click)
    }
}

fn cta(
    id: &'static str,
    label: &'static str,
    disabled: bool,
    ob: &OnboardingColors,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let (fill, text_color, hover) = ob.cta;
    let el = div()
        .id(id)
        .test_support()
        .role(Role::Button)
        .aria_label(label)
        .min_w(px(220.))
        .flex()
        .justify_center()
        .px_4()
        .py(px(9.))
        .rounded(px(10.))
        .bg(fill)
        .text_color(text_color)
        .text_size(text::SM.size)
        .font_weight(FontWeight::MEDIUM)
        .child(label);
    if disabled {
        el.opacity(0.5)
    } else {
        el.cursor_pointer().hover(move |s| s.bg(hover)).on_click(on_click)
    }
}

fn secondary(
    id: &'static str,
    label: &'static str,
    disabled: bool,
    ob: &OnboardingColors,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let text_color = ob.text;
    let el = div()
        .id(id)
        .test_support()
        .role(Role::Button)
        .aria_label(label)
        .min_w(px(140.))
        .flex()
        .justify_center()
        .px_4()
        .py(px(9.))
        .rounded(px(10.))
        .bg(text_color.opacity(0.1))
        .text_color(text_color)
        .text_size(text::SM.size)
        .child(label);
    if disabled {
        el.opacity(0.5)
    } else {
        el.cursor_pointer().hover(move |s| s.bg(text_color.opacity(0.16))).on_click(on_click)
    }
}

/// "Welcome to Kairo", letter by letter: each word clips its letters so
/// they rise into view, each letter shaded from one gradient across the line.
fn animated_title(source: &'static str, clock: Clock, ob: &OnboardingColors) -> AnyElement {
    let total = source.chars().filter(|c| !c.is_whitespace()).count().max(2);
    let mut k = 0usize;
    let size = 64.0;
    let words = source.split(' ').enumerate().map(|(w, word)| {
        let letters = word.chars().map(|c| {
            let index = k;
            k += 1;
            let e = if clock.still { 1.0 } else {
                kairo_ui_kit::motion::OUT_EXPO.sample(((clock.t - (550.0 + index as f32 * 38.0)) / 900.0).clamp(0.0, 1.0))
            };
            let shade = index as f32 / (total - 1) as f32;
            let color = kairo_ui_kit::onboarding::mix(ob.text, ob.text.opacity(0.72), shade);
            div().relative().top(px(size * 1.1 * 1.05 * (1.0 - e))).opacity(e).text_color(color).child(c.to_string())
        });
        h_flex()
            .overflow_hidden()
            .when(w > 0, |el| el.ml(px(size * 0.25)))
            .children(letters.collect::<Vec<_>>())
    });
    h_flex()
        .id(ids::TITLE)
        .test_support()
        .role(Role::Heading)
        .aria_label(source)
        .mt_8()
        .text_size(px(size))
        .line_height(relative(1.1))
        .font_weight(FontWeight::SEMIBOLD)
        .children(words.collect::<Vec<_>>())
        .into_any_element()
}

impl Render for OnboardingWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let ob = colors(k.appearance);
        let still = cx.reduced_motion();
        let now = Instant::now();
        let width = f32::from(window.viewport_size().width);
        let height = f32::from(window.viewport_size().height);
        let touring = self.touring(cx);
        let on_account = self.on_account(cx);
        let ready = self.account_ready(cx);
        let locked = self.locked();
        let index = self.index(cx);
        let steps = Self::steps();
        let clock = clock_since(self.page_at, self.dir, still, now);
        let opened = clock_since(self.opened_at, 1.0, still, now);

        let body: AnyElement = if self.welcomed {
            self.render_welcome(clock, &ob, cx)
        } else if touring {
            self.render_tour(&ob, width, still, cx)
        } else if on_account {
            self.render_account(clock, &ob, window, cx)
        } else {
            self.render_output(clock, &ob, window, cx)
        };

        let back_target = if touring { None } else { self.back_target(cx) };
        let last_slide = self.slide == TOUR_SLIDES.len() - 1;
        let setup_count = steps.len() - 1;
        let progress = if setup_count > 0 { index as f32 / setup_count as f32 } else { 1.0 };
        let footer = (!self.welcomed).then(|| {
            let left = v_flex()
                .min_w_0()
                .items_start()
                .gap_1()
                .when_some(back_target, |el, target| {
                    el.child(text_link(ids::BACK, step_name(target), locked, Some(KairoIcon::ChevronLeft), cx.listener(|this, _, _, cx| this.back(cx)), &k))
                })
                .when_some(self.error.clone(), |el, error| el.child(error_line(error, self.error_at, k.danger_text)));
            let center: AnyElement = if on_account {
                div().into_any_element()
            } else {
                cta(ids::CONTINUE, if self.busy { "Saving…" } else { "Continue" }, locked, &ob, cx.listener(move |this, _, _, cx| {
                    if this.touring(cx) {
                        if this.slide == TOUR_SLIDES.len() - 1 {
                            this.leave_tour(cx);
                        } else {
                            this.go_to_slide(this.slide as isize + 1, cx);
                        }
                    } else {
                        this.advance(cx);
                    }
                }))
                .into_any_element()
            };
            let right = h_flex().justify_end().map(|el| {
                if touring {
                    el.child(secondary(ids::SKIP_TOUR, "Skip Tour", locked, &ob, cx.listener(|this, _, _, cx| this.leave_tour(cx))))
                } else if ready && !on_account {
                    el.child(secondary(ids::LATER, "Set Up Later", locked, &ob, cx.listener(|this, _, _, cx| {
                        this.finish(true, cx);
                    })))
                } else {
                    el
                }
            });
            let _ = last_slide;
            let fill = if still { progress } else {
                kairo_ui_kit::motion::OUT_EXPO.sample((opened.t / 800.0).clamp(0.0, 1.0)) * progress
            };
            div()
                .relative()
                .flex()
                .items_center()
                .gap_4()
                .flex_shrink_0()
                .pt_4()
                .px(px(22.))
                .pb(px(20.))
                .when(!touring, |el| el.child(div().absolute().left_0().top_0().h(px(2.)).w(relative(fill)).bg(ob.blue)))
                .child(div().flex_1().child(left))
                .child(center)
                .child(div().flex_1().child(right))
        });

        // Keep drawing while anything settles or loops.
        if !still && (clock.settling() || self.leaving.is_some() || (touring && TOUR_SLIDES[self.slide].id == "screens") || (touring && TOUR_SLIDES[self.slide].id == "scripture")) {
            window.request_animation_frame();
        }

        let dialog_h = (height - 40.0).min(680.0);
        let dialog_in = if still { 1.0 } else { kairo_ui_kit::motion::OUT_EXPO.sample((opened.t / 450.0).clamp(0.0, 1.0)) };
        let scrim_in = if still { 1.0 } else { (opened.t / 250.0).clamp(0.0, 1.0) };
        div()
            .id("onboarding-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p(px(20.))
            .bg(ob.scrim.opacity(scrim_in))
            .occlude()
            .child(
                v_flex()
                    .id(ids::DIALOG)
                    .test_support()
                    .role(Role::Dialog)
                    .aria_label("Welcome to Kairo")
                    .track_focus(&self.focus)
                    .key_context("OnboardingWizard")
                    .on_key_down(cx.listener(Self::on_key))
                    .relative()
                    .top(px(10.0 * (1.0 - dialog_in)))
                    .opacity(dialog_in)
                    .w(px(1000.))
                    .max_w_full()
                    .h(px(dialog_h))
                    .overflow_hidden()
                    .rounded(px(16.))
                    .border(px(0.5))
                    .border_color(ob.edge)
                    .bg(ob.bg)
                    .text_color(ob.text)
                    .child(StageGlow::new(&ob))
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(10.))
                            .px(px(22.))
                            .py(px(14.))
                            .child(KairoMark::new(MarkSize::Small))
                            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(k.text(TextStep::T300)).child("Welcome to Kairo")),
                    )
                    .child(div().id("onboarding-body").relative().flex_1().min_h_0().overflow_y_scroll().child(body))
                    .children(footer)
                    .focus_trap("onboarding-trap", &self.focus),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base36_matches_number_to_string() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(1_731_571_200_000), "m3hf8ttc");
    }

    #[test]
    fn account_page_copy() {
        assert_eq!(account_copy(true, false, true, None).0, "Activate this computer");
        assert_eq!(account_copy(false, true, true, Some("a@b.c")).1, "Enter the 6-digit code we sent to a@b.c.");
        assert_eq!(account_copy(false, true, true, None).1, "Enter the 6-digit code we sent to your email.");
        assert_eq!(account_copy(false, false, true, None).0, "Create your account");
        assert_eq!(account_copy(false, false, false, None).0, "Sign in to Kairo");
    }
}
