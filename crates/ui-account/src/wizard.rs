//! First-run setup (`OnboardingWizard.tsx`, `StepAccount.tsx`,
//! `StepOutput.tsx`, `WelcomeView.tsx`): the tour, the account, this
//! computer's screens, then the closing welcome. Shown signed out; the shell
//! keeps it open across sign-in so the flow carries on.

use crate::account_page::{AccountPage, Host};
use crate::backend::OnboardingWrite;
use crate::display_picker::{DisplayPicker, Picked};
use crate::fluid::{Flick, Tuning};
use crate::model::{Account, AccountChanged};
use crate::ob::Clock;
use crate::tour::TOUR_SLIDES;
use crate::tour_page::{SlideChanged, TourPage};
use crate::write_runner::Job;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{FocusTrapElement, Icon, h_flex, v_flex};
use gpui_kit::rems;
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder, px,
    relative,
};
pub use kairo_domain::cloud::{OutputTargets, base36, output_targets, seeded_church};
use kairo_domain::cloud::{SetupOutputChange as Save, bind_setup_output};
use kairo_domain::cloud::{
    can_enter_app, is_step_complete, needs_email_confirmation, onboarding_steps,
};
use kairo_domain::displays::DisplayBinding;
use kairo_domain::settings::OnboardingStepId;
use kairo_ui_kit::brand::{KairoMark, MarkSize};
use kairo_ui_kit::onboarding::{OnboardingColors, colors};
use kairo_ui_kit::tokens::TextStep;
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use std::time::Instant;

/// Where a quick start goes (`NavRoute`).
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
    /// Close setup (`onDismiss`): open `route` if given; `coach` runs the
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
    tour: Option<Entity<TourPage>>,
    tour_subscription: Option<Subscription>,
    page: Flick,
    page_key: Option<crate::setup_state::Page>,
    dir: f32,
    page_at: Instant,
    welcomed: bool,
    account_page: Option<Entity<AccountPage>>,
    audience: Option<Entity<DisplayPicker>>,
    stage: Option<Entity<DisplayPicker>>,
    output_error: Option<SharedString>,
    targets: Option<OutputTargets>,
    display_count: usize,
    reconcile_account: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WizardEvent> for OnboardingWizard {}

impl Focusable for OnboardingWizard {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl OnboardingWizard {
    /// The wizard over `account`, focused.
    pub fn new(account: Entity<Account>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let subscriptions = vec![cx.subscribe_in(
            &account,
            window,
            |this, _, event: &AccountChanged, window, cx| {
                if *event == AccountChanged::Settings {
                    this.targets = None;
                }
                this.sync_children(window, cx);
                this.carry_on_from_account(cx);
                cx.notify();
            },
        )];
        let now = cx.background_executor().now();
        let mut wizard = Self {
            account,
            focus,
            busy: false,
            pending: false,
            saving_screen: false,
            error: None,
            error_at: now,
            slide: 0,
            tour: None,
            tour_subscription: None,
            page: Flick::at(0., now),
            page_key: None,
            dir: 1.0,
            page_at: now,
            welcomed: false,
            account_page: None,
            audience: None,
            stage: None,
            output_error: None,
            targets: None,
            display_count: 0,
            reconcile_account: false,
            _subscriptions: subscriptions,
        };
        wizard.sync_children(window, cx);
        wizard.carry_on_from_account(cx);
        wizard
    }

    // ── State ───────────────────────────────────────────────────────────

    fn setup(&self, cx: &gpui_kit::App) -> crate::setup_state::SetupState {
        let account = self.account.read(cx);
        crate::setup_state::SetupState::derive(
            account.onboarding(),
            can_enter_app(account.session()),
            needs_email_confirmation(account.session()),
            account.license().is_some_and(|l| l.activated),
            self.welcomed,
        )
    }

    fn steps() -> &'static [OnboardingStepId] {
        onboarding_steps()
    }

    fn index(&self, cx: &gpui_kit::App) -> usize {
        self.setup(cx).index
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
        !self.touring(cx)
            && (self.current(cx) == OnboardingStepId::Account || !self.account_ready(cx))
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
        self.setup(cx).back
    }

    // ── Writes ──────────────────────────────────────────────────────────

    /// `apply`: one write at a time; `then` runs on success.
    fn apply(
        &mut self,
        jobs: Vec<Job>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pending {
            return false;
        }
        self.pending = true;
        self.busy = true;
        self.error = None;
        cx.notify();
        let account = self.account.clone();
        let epoch = account.read(cx).write_epoch();
        cx.spawn(async move |this, cx| {
            let result = crate::write_runner::run(&account, jobs, epoch, cx).await;
            let _ = this.update(cx, |this, cx| {
                this.pending = false;
                this.busy = false;
                match result {
                    Ok(_) if this.account.read(cx).write_epoch() != epoch => {}
                    Ok(state) => {
                        if let Some(state) = state {
                            this.account.update(cx, |a, cx| a.set_onboarding(state, cx));
                        }
                        then(this, cx);
                    }
                    Err(crate::write_runner::WriteError::Stale) => {}
                    Err(crate::write_runner::WriteError::Backend(error)) => {
                        tracing::warn!(%error, "[Onboarding] setup could not be saved");
                        this.error =
                            Some("Your setup could not be saved. Please try again.".into());
                        this.error_at = cx.background_executor().now();
                    }
                }
                if this.reconcile_account && this.error.is_none() {
                    this.reconcile_account = false;
                    this.carry_on_from_account(cx);
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
        if self.pending {
            self.reconcile_account = true;
            return;
        }
        self.reconcile_account = false;
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

    /// Ends setup; finishing lands on the welcome unless `welcome` is false.
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
                } else {
                    cx.emit(WizardEvent::Dismiss {
                        route: None,
                        coach: false,
                    });
                    cx.emit(WizardEvent::OpenScreens);
                }
            },
            cx,
        )
    }

    /// Moves to tour slide `next`, clamped.
    pub fn go_to_slide(&mut self, next: isize, cx: &mut Context<Self>) {
        let clamped = next.clamp(0, TOUR_SLIDES.len() as isize - 1) as usize;
        self.dir = if clamped > self.slide { 1.0 } else { -1.0 };
        if let Some(tour) = &self.tour {
            tour.update(cx, |tour, cx| tour.go(clamped, cx));
        }
        self.slide = clamped;
        cx.notify();
    }

    /// Leaves the tour: to the account, or straight to screens when ready.
    pub fn leave_tour(&mut self, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        self.dir = 1.0;
        self.apply(
            vec![
                Job::Write(OnboardingWrite::Complete(OnboardingStepId::Welcome)),
                Job::LeaveTour,
            ],
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
            jobs.push(Job::Write(OnboardingWrite::SetCurrent(
                Self::steps()[index + 1],
            )));
        }
        self.apply(
            jobs,
            move |this, _| {
                if last {
                    this.welcomed = true;
                }
            },
            cx,
        );
    }

    /// Back, skipping the account page once signed in.
    pub fn back(&mut self, cx: &mut Context<Self>) {
        if self.touring(cx) {
            self.go_to_slide(self.slide as isize - 1, cx);
            return;
        }
        let Some(target) = self.back_target(cx) else {
            return;
        };
        if self.locked() {
            return;
        }
        self.dir = -1.0;
        // Walking back into the tour lands on its last slide.
        if target == OnboardingStepId::Welcome {
            self.slide = TOUR_SLIDES.len() - 1;
        }
        self.apply(
            vec![Job::Write(OnboardingWrite::SetCurrent(target))],
            |_, _| {},
            cx,
        );
    }

    fn escape(&mut self, cx: &mut Context<Self>) {
        // The account cannot be skipped; once in, Escape is "set up later".
        if self.welcomed {
            cx.emit(WizardEvent::Dismiss {
                route: None,
                coach: true,
            });
        } else if self.account_ready(cx) {
            self.finish(true, cx);
        }
    }

    fn continue_primary(&mut self, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if self.welcomed {
            cx.emit(WizardEvent::Dismiss {
                route: None,
                coach: true,
            });
        } else if self.touring(cx) {
            if self.slide + 1 == TOUR_SLIDES.len() {
                self.leave_tour(cx);
            } else {
                self.go_to_slide(self.slide as isize + 1, cx);
            }
        } else if self.on_account(cx) && self.account_ready(cx) {
            self.carry_on_from_account(cx);
        } else if !self.on_account(cx) {
            self.advance(cx);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "enter" if window.focused(cx).is_none_or(|f| f == self.focus) => {
                cx.stop_propagation();
                self.continue_primary(cx);
            }
            "escape" => {
                cx.stop_propagation();
                self.escape(cx);
            }
            "right" | "left" if self.touring(cx) && !self.typing(window, cx) => {
                cx.stop_propagation();
                let step = if event.keystroke.key == "right" {
                    1
                } else {
                    -1
                };
                self.go_to_slide(self.slide as isize + step, cx);
            }
            _ => {}
        }
    }

    fn typing(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        // Arrow keys belong to a focused text field.
        window.focused(cx).is_some_and(|f| f != self.focus) && !self.touring(cx)
    }

    // ── Pages ───────────────────────────────────────────────────────────

    fn sync_children(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.setup(cx).account() && self.account_page.is_none() {
            let account = self.account.clone();
            self.account_page =
                Some(cx.new(|cx| AccountPage::new(account, Host::Setup, window, cx)));
        }
        let active = self.setup(cx).account();
        if let Some(page) = &self.account_page {
            page.update(cx, |page, cx| page.set_active(active, window, cx));
        }
        if self.setup(cx).page == crate::setup_state::Page::Tour {
            if self.tour.is_none() {
                let slide = self.slide;
                let tour = cx.new(|cx| TourPage::new(slide, cx));
                self.tour_subscription =
                    Some(cx.subscribe(&tour, |this, _, SlideChanged(slide), cx| {
                        if this.slide != *slide {
                            this.slide = *slide;
                            cx.notify();
                        }
                    }));
                self.tour = Some(tour);
            }
        } else {
            self.tour = None;
            self.tour_subscription = None;
        }
        self.refresh_pickers(window, cx);
    }

    fn output_targets(&self, cx: &gpui_kit::App) -> Option<OutputTargets> {
        let settings = self.account.read(cx).settings()?;
        Some(output_targets(settings))
    }

    fn refresh_pickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.targets.is_none() {
            self.targets = self.output_targets(cx);
        }
        let Some(targets) = self.targets.clone() else {
            return;
        };
        let displays = self.account.read(cx).displays().to_vec();
        self.display_count = displays.len();
        if self.setup(cx).page == crate::setup_state::Page::Output {
            if self.audience.is_none() {
                let t = targets.clone();
                let d = displays.clone();
                let picker = cx.new(|cx| {
                    DisplayPicker::new(
                        "ob-audience",
                        "Audience Screen",
                        &t.audience,
                        &d,
                        &t.audience_others,
                        window,
                        cx,
                    )
                });
                self._subscriptions
                    .push(cx.subscribe(&picker, |this, _, Picked(binding), cx| {
                        this.save_output(true, binding.clone(), cx)
                    }));
                self.audience = Some(picker);
            }
            if self.stage.is_none() {
                let t = targets.clone();
                let d = displays.clone();
                let picker = cx.new(|cx| {
                    DisplayPicker::new(
                        "ob-stage",
                        "Stage Screen",
                        &t.stage,
                        &d,
                        &t.stage_others,
                        window,
                        cx,
                    )
                });
                self._subscriptions
                    .push(cx.subscribe(&picker, |this, _, Picked(binding), cx| {
                        this.save_output(false, binding.clone(), cx)
                    }));
                self.stage = Some(picker);
            }
        }

        if let Some(audience) = &self.audience {
            audience.update(cx, |p, cx| {
                p.update(
                    &targets.audience,
                    &displays,
                    &targets.audience_others,
                    window,
                    cx,
                )
            });
        }
        if let Some(stage) = &self.stage {
            stage.update(cx, |p, cx| {
                p.update(&targets.stage, &displays, &targets.stage_others, window, cx)
            });
        }
    }

    fn save_output(&mut self, audience: bool, binding: DisplayBinding, cx: &mut Context<Self>) {
        if self.saving_screen {
            return;
        }
        let Some(settings) = self.account.read(cx).settings().cloned() else {
            return;
        };
        let backend = self.account.read(cx).backend();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let Some(save) = bind_setup_output(&settings, audience, &binding, now) else {
            return;
        };
        let epoch = self.account.read(cx).write_epoch();
        let settings_revision = self.account.read(cx).settings_revision();
        self.saving_screen = true;
        self.output_error = None;
        self.set_pickers_disabled(true, cx);
        cx.notify();
        let request = match &save {
            Save::Overlay(o) => backend.save_overlay((**o).clone()),
            Save::Presentation(p) => backend.save_presentation((**p).clone()),
        };
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.saving_screen = false;
                this.set_pickers_disabled(false, cx);
                if this.account.read(cx).write_epoch() != epoch
                    || this.account.read(cx).settings_revision() != settings_revision
                {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(()) => this.account.update(cx, |a, cx| {
                        a.update_settings(
                            |s| match save {
                                Save::Overlay(o) => s.overlay = *o,
                                Save::Presentation(p) => s.presentation = *p,
                            },
                            cx,
                        )
                    }),
                    Err(_) => {
                        this.output_error =
                            Some("The screen could not be saved. Please try again.".into());
                        this.error_at = cx.background_executor().now();
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

    fn render_output(
        &mut self,
        clock: Clock,
        ob: &OnboardingColors,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let row = |label: &'static str,
                   hint: &'static str,
                   picker: Option<Entity<DisplayPicker>>,
                   i: usize| {
            clock.ob_in(
                h_flex()
                    .w_full()
                    .gap_6()
                    .items_center()
                    .px_6()
                    .py_5()
                    .rounded(rems(1.))
                    .bg(ob.card[0])
                    .child(
                        v_flex()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(rems(1.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text(TextStep::T200))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_size(rems(0.8125))
                                    .text_color(k.text(TextStep::T500))
                                    .child(hint),
                            ),
                    )
                    .child(div().flex_basis(relative(0.565)).children(picker)),
                i,
                0.0,
                28.0,
            )
        };
        let many = self.display_count > 1;
        v_flex()
            .items_center()
            .max_w(rems(45.))
            .mx_auto()
            .pt(rems(2.75))
            .px(rems(2.))
            .pb(rems(2.))
            .child(clock.ob_in(
                div().id(ids::TITLE).test_support().role(Role::Heading).aria_label("Configure Screens").text_size(rems(2.)).font_weight(FontWeight::MEDIUM).text_color(ob.text).child("Configure Screens"),
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
            .when_some(self.output_error.clone(), |el, error| el.child(error_line(error, self.error_at, k.danger_text, cx.background_executor().now(), cx.reduced_motion()).mt_4()))
            .child(clock.rise(
                div().mt_6().text_center().text_size(rems(0.8125)).text_color(k.text(TextStep::T400)).child(if many {
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
                .text_size(rems(0.875))
                .font_weight(FontWeight::MEDIUM)
                .text_color(k.notice_text)),
                0,
                0.0,
            ))
            .into_any_element()
    }

    fn render_welcome(
        &mut self,
        clock: Clock,
        ob: &OnboardingColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let settings_name = self
            .account
            .read(cx)
            .settings()
            .map(|s| s.church.name.clone())
            .unwrap_or_default();
        let org = self
            .account
            .read(cx)
            .session()
            .org
            .as_ref()
            .map(|o| o.name.clone())
            .unwrap_or_default();
        let church = if settings_name.is_empty() {
            org
        } else {
            settings_name
        };
        let church = church.trim().to_owned();
        let ready_line = if church.is_empty() {
            "You’re ready for your next service.".to_owned()
        } else {
            format!("{church} is ready for your next service.")
        };
        let quick = [
            (
                QuickStart::Scripture,
                "Find a verse",
                "Search by reference or words",
                KairoIcon::BookOpen,
                "quickstart-scripture",
            ),
            (
                QuickStart::Lyrics,
                "Add a song",
                "Import or type in lyrics",
                KairoIcon::Music2,
                "quickstart-lyrics",
            ),
            (
                QuickStart::Documents,
                "Import slides",
                "PowerPoint or PDF",
                KairoIcon::Presentation,
                "quickstart-documents",
            ),
        ];
        let entrance = self.page.value().clamp(0., 1.);
        v_flex()
            .w_full()
            .min_h_full()
            .items_center()
            .justify_center()
            .py(rems(2.5))
            .px(rems(2.))
            .child(
                div()
                    .opacity(entrance)
                    .child(KairoMark::new(MarkSize::Large)),
            )
            .child(animated_title("Welcome to Kairo", clock, ob))
            .child(welcome_entry(
                div()
                    .mt_3()
                    .text_center()
                    .text_size(rems(1.0625))
                    .text_color(k.text(TextStep::T400))
                    .child(ready_line),
                0,
                entrance,
            ))
            .child(
                h_flex()
                    .mt(rems(2.5))
                    .w_full()
                    .max_w(rems(40.))
                    .gap_3()
                    .children(quick.into_iter().enumerate().map(
                        |(i, (route, title, detail, icon, id))| {
                            use gpui_kit::component::button::{
                                Button, ButtonCustomVariant, ButtonVariants,
                            };
                            welcome_entry(
                                div().flex_1().rounded(rems(1.)).bg(ob.card[0]).child(
                                    Button::new(id)
                                        .cursor_pointer()
                                        .w_full()
                                        .h_auto()
                                        .p(rems(1.125))
                                        .rounded(gpui_kit::component::button::ButtonRounded::Large)
                                        .accessibility_label(title)
                                        .custom(
                                            ButtonCustomVariant::new(cx)
                                                .color(ob.card[0])
                                                .foreground(ob.text)
                                                .hover(ob.card[1])
                                                .active(ob.card[2]),
                                        )
                                        .child(
                                            v_flex()
                                                .items_start()
                                                .w_full()
                                                .child(
                                                    Icon::new(icon)
                                                        .size(rems(1.125))
                                                        .text_color(k.text(TextStep::T300)),
                                                )
                                                .child(
                                                    div()
                                                        .mt_4()
                                                        .text_size(rems(0.9375))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .mt_1()
                                                        .text_size(rems(0.75))
                                                        .text_color(k.text(TextStep::T500))
                                                        .child(detail),
                                                ),
                                        )
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(WizardEvent::Dismiss {
                                                route: Some(route),
                                                coach: true,
                                            })
                                        })),
                                ),
                                i + 1,
                                entrance,
                            )
                        },
                    )),
            )
            .child(welcome_entry(
                cta(
                    ids::START,
                    "Start using Kairo",
                    false,
                    ob,
                    cx,
                    cx.listener(|_, _, _, cx| {
                        cx.emit(WizardEvent::Dismiss {
                            route: None,
                            coach: true,
                        })
                    }),
                )
                .mt(rems(2.5)),
                4,
                entrance,
            ))
            .into_any_element()
    }
}

/// Time since `since` as a [`Clock`].
fn clock_since(since: Instant, dir: f32, still: bool, now: Instant) -> Clock {
    if still {
        return Clock::still();
    }
    Clock {
        t: now.saturating_duration_since(since).as_secs_f32() * 1000.0,
        dir,
        still,
    }
}

pub use crate::account_page::account_copy;

fn error_line(
    error: SharedString,
    since: Instant,
    color: gpui_kit::Hsla,
    now: Instant,
    still: bool,
) -> impl gpui_kit::Styled + gpui_kit::IntoElement {
    // `.onboarding-error`: shakes once.
    let t = now.saturating_duration_since(since).as_secs_f32() * 1000.0;
    let x = if still {
        0.
    } else {
        kairo_ui_kit::motion::OB_SHAKE
            .sample((t / 350.0).clamp(0.0, 1.0))
            .x
    };
    div()
        .id(ids::ERROR)
        .test_support()
        .role(Role::Alert)
        .aria_label(error.clone())
        .relative()
        .left(px(x))
        .text_size(rems(0.75))
        .text_color(color)
        .child(error)
}

fn welcome_entry<E: Styled>(el: E, ix: usize, progress: f32) -> E {
    let progress = crate::fluid::staged(progress, ix, 5);
    el.relative()
        .top(px(16. * (1. - progress)))
        .opacity(progress)
}

fn text_link(
    id: &'static str,
    label: &'static str,
    disabled: bool,
    _icon: Option<KairoIcon>,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    _k: &kairo_ui_kit::KairoColors,
) -> gpui_kit::component::button::Button {
    crate::stage::quiet(id, label, disabled).on_click(on_click)
}
fn cta(
    id: &'static str,
    label: &'static str,
    disabled: bool,
    ob: &OnboardingColors,
    cx: &gpui_kit::App,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> gpui_kit::component::button::Button {
    crate::stage::primary(id, label, disabled, ob, cx).on_click(on_click)
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
            let e = if clock.still {
                1.0
            } else {
                kairo_ui_kit::motion::OUT_EXPO
                    .sample(((clock.t - (550.0 + index as f32 * 38.0)) / 900.0).clamp(0.0, 1.0))
            };
            let shade = index as f32 / (total - 1) as f32;
            let color = kairo_ui_kit::onboarding::mix(ob.text, ob.text.opacity(0.72), shade);
            div()
                .relative()
                .top(rems(size / 16. * 1.1 * 1.05 * (1.0 - e)))
                .opacity(e)
                .text_color(color)
                .child(c.to_string())
        });
        h_flex()
            .overflow_hidden()
            .when(w > 0, |el| el.ml(rems(size / 16. * 0.25)))
            .children(letters.collect::<Vec<_>>())
    });
    h_flex()
        .id(ids::TITLE)
        .test_support()
        .role(Role::Heading)
        .aria_label(source)
        .mt_8()
        .text_size(rems(size / 16.))
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
        let now = cx.background_executor().now();
        let setup = self.setup(cx);
        let touring = setup.page == crate::setup_state::Page::Tour;
        let on_account = setup.account();
        // The account page and the closing welcome centre their own content, which
        // needs a settled height below them to centre within.
        // Configure Screens sits in the middle of the window too (owner).
        let centred = on_account || self.welcomed || setup.page == crate::setup_state::Page::Output;
        let actions = setup.actions(self.slide, self.error.is_some());
        let locked = self.locked();
        let index = setup.index;
        if self.page_key != Some(setup.page) {
            self.page_key = Some(setup.page);
            self.page_at = now;
            self.page.pin(0., now);
            self.page.to(1., Tuning::PAGE, now);
        }
        let moving = self.page.tick(now, still);
        let clock = clock_since(self.page_at, self.dir, still, now);
        let body = if self.welcomed {
            self.render_welcome(clock, &ob, cx)
        } else if touring {
            div()
                .size_full()
                .children(self.tour.clone())
                .into_any_element()
        } else if on_account {
            div().children(self.account_page.clone()).into_any_element()
        } else {
            self.render_output(Clock::still(), &ob, window, cx)
        };
        let back = actions.back;
        let top = crate::stage::top_bar()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_start()
                    .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation()
                    })
                    .when(back, |el| {
                        el.child(
                            crate::stage::quiet(ids::BACK, "Back", locked)
                                .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                        )
                    }),
            )
            .child(h_flex().gap_2().when(!self.welcomed, |el| {
                el.children(Self::steps().iter().enumerate().map(|(i, _)| {
                    div()
                        .h(rems(0.1875))
                        .w(rems(1.75))
                        .rounded_full()
                        .bg(ob.text.opacity(if i <= index { 0.8 } else { 0.15 }))
                }))
            }))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation()
                    })
                    .map(|el| {
                        if actions.skip {
                            el.child(
                                crate::stage::quiet(ids::SKIP_TOUR, "Skip tour", locked)
                                    .on_click(cx.listener(|this, _, _, cx| this.leave_tour(cx))),
                            )
                        } else if actions.later {
                            el.child(
                                crate::stage::quiet(ids::LATER, "Set up later", locked).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.finish(true, cx);
                                    }),
                                ),
                            )
                        } else {
                            el
                        }
                    }),
            );
        let error_moving = (self.error.is_some() || self.output_error.is_some())
            && now.saturating_duration_since(self.error_at).as_millis() < 350;
        let letters_moving = self.welcomed && clock.t < 2100.;
        if moving || (!still && (letters_moving || error_moving)) {
            window.request_animation_frame();
        }
        let progress = self.page.value().clamp(0., 1.);
        crate::stage::surface(&ob)
            .id(ids::DIALOG)
            .test_support()
            .role(Role::Dialog)
            .aria_label("Welcome to Kairo")
            .track_focus(&self.focus)
            .key_context("OnboardingWizard")
            .on_key_down(cx.listener(Self::on_key))
            .occlude()
            .child(top)
            .child(
                div()
                    .id("onboarding-body")
                    .test_support()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .min_h_full()
                            .when(centred, |el| el.flex().flex_col().justify_center())
                            .when(touring, |el| el.h_full())
                            .top(px(if still {
                                0.
                            } else {
                                self.dir * 24. * (1. - progress)
                            }))
                            .opacity(progress)
                            .child(body),
                    ),
            )
            .when_some(self.error.clone(), |el, error| {
                el.child(error_line(
                    error,
                    self.error_at,
                    k.danger_text,
                    cx.background_executor().now(),
                    cx.reduced_motion(),
                ))
            })
            .when(actions.primary, |el| {
                el.child(
                    h_flex().justify_center().pb_6().pt_2().child(
                        crate::stage::primary(
                            ids::CONTINUE,
                            if actions.retry {
                                "Try again"
                            } else {
                                "Continue"
                            },
                            locked,
                            &ob,
                            cx,
                        )
                        .loading(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.continue_primary(cx))),
                    ),
                )
            })
            .focus_trap(ids::DIALOG, &self.focus)
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
        assert_eq!(base36(1_731_571_200_000), "m3h0s8w0");
    }

    #[test]
    fn account_page_copy() {
        assert_eq!(
            account_copy(true, false, true, None).0,
            "Activate this computer"
        );
        assert_eq!(
            account_copy(false, true, true, Some("a@b.c")).1,
            "Enter the 6-digit code we sent to a@b.c."
        );
        assert_eq!(
            account_copy(false, true, true, None).1,
            "Enter the 6-digit code we sent to your email."
        );
        assert_eq!(
            account_copy(false, false, true, None).0,
            "Create your account"
        );
        assert_eq!(
            account_copy(false, false, false, None).0,
            "Sign in to Kairo"
        );
    }
}
