//! What sits in front of the booth (`App.tsx`): first-run setup while it is
//! offered, the sign-in wall while a confirmed session is missing, and the
//! coach tips once setup closes.
//!
//! Setup opens before the account wall, so the account is created inside
//! it. A finished or dismissed wizard does not come back by itself; "Run
//! setup again" (a reset onboarding state) clears this launch's dismissal.

use crate::coach::{CoachDone, CoachTour, CoachTourId, coach_tour};
use crate::fluid::{Flick, Tuning};
use crate::gate::AccountGate;
use crate::model::{Account, AccountChanged};
use crate::wizard::{OnboardingWizard, QuickStart, WizardEvent};
use gpui_kit::{
    AppContext, Context, Entity, EventEmitter, Focusable, IntoElement, ParentElement, Render,
    Styled, Subscription, Window, div, prelude::FluentBuilder,
};
use kairo_domain::cloud::should_offer_account_gate;
use kairo_domain::cloud::should_offer_onboarding;
use kairo_ui_kit::KairoThemeExt;

/// What the flow asks of the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowEvent {
    /// Open a workspace (`setRoute`); `None` is the Operator.
    Navigate(Option<QuickStart>),
    /// Open the Screens window.
    OpenScreens,
}

pub struct AccountFlow {
    account: Entity<Account>,
    /// Setup closed this launch (`onboardingDismissed`).
    dismissed: bool,
    /// Setup stays up until it dismisses itself, even once finished
    /// (`onboardingHeld`): finishing marks it complete before the welcome.
    held: bool,
    wizard: Option<(Entity<OnboardingWizard>, Subscription)>,
    gate: Option<Entity<AccountGate>>,
    coach: Option<(Entity<CoachTour>, Subscription)>,
    closing: Option<(gpui_kit::AnyView, Flick)>,
    _account: Subscription,
}

impl EventEmitter<FlowEvent> for AccountFlow {}

impl AccountFlow {
    pub fn new(account: Entity<Account>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe_in(
            &account,
            window,
            |this, _, changed: &AccountChanged, window, cx| {
                if *changed == AccountChanged::Onboarding
                    && this.account.read(cx).onboarding().completed_at.is_none()
                {
                    this.dismissed = false;
                }
                this.sync(window, cx);
            },
        );
        let mut flow = Self {
            account,
            dismissed: false,
            held: false,
            wizard: None,
            gate: None,
            coach: None,
            closing: None,
            _account: subscription,
        };
        flow.sync(window, cx);
        flow
    }

    /// Setup is showing.
    pub fn wizard_open(&self) -> bool {
        self.wizard.is_some()
    }

    /// The sign-in wall is showing.
    pub fn gate_open(&self) -> bool {
        self.gate.is_some() && self.wizard.is_none()
    }

    /// A coach tour is running.
    pub fn coaching(&self) -> bool {
        self.coach.is_some()
    }

    /// Something covers the booth: setup or the sign-in wall.
    pub fn blocking(&self) -> bool {
        self.wizard.is_some() || self.gate.is_some()
    }

    /// "Show tips again" in Settings → Account: the Operator walk-round.
    pub fn replay_tips(&mut self, cx: &mut Context<Self>) {
        cx.emit(FlowEvent::Navigate(None));
        self.start_coach(CoachTourId::Operator, cx);
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (offer, gate) = {
            let account = self.account.read(cx);
            (
                // Not before the saved progress loads: the default state
                // looks unfinished and would hold setup open every launch.
                account.onboarding_known()
                    && should_offer_onboarding(account.onboarding(), self.dismissed),
                should_offer_account_gate(account.session()),
            )
        };
        if offer {
            self.held = true;
        }
        let wizard_open = offer || (self.held && !self.dismissed);
        if wizard_open && self.wizard.is_none() {
            let account = self.account.clone();
            let wizard = cx.new(|cx| OnboardingWizard::new(account, window, cx));
            let subscription = cx.subscribe_in(&wizard, window, Self::on_wizard);
            wizard.read(cx).focus_handle(cx).focus(window, cx);
            self.wizard = Some((wizard, subscription));
        } else if !wizard_open && let Some((wizard, _)) = self.wizard.take() {
            self.close_stage(wizard.into(), cx);
        }
        if gate && !wizard_open && self.gate.is_none() {
            let account = self.account.clone();
            self.gate = Some(cx.new(|cx| AccountGate::new(account, window, cx)));
            // The wall replaces the booth; tips over it would point at nothing.
            self.coach = None;
        } else if !gate || wizard_open {
            self.gate = None;
        }
        cx.notify();
    }

    fn on_wizard(
        &mut self,
        _: &Entity<OnboardingWizard>,
        event: &WizardEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match *event {
            WizardEvent::Dismiss { route, coach } => {
                self.dismissed = true;
                self.held = false;
                cx.emit(FlowEvent::Navigate(route));
                if coach {
                    self.start_coach(coach_tour_for(route), cx);
                }
            }
            WizardEvent::OpenScreens => {
                self.dismissed = true;
                self.held = false;
                cx.emit(FlowEvent::OpenScreens);
            }
        }
        self.sync(window, cx);
    }

    fn close_stage(&mut self, view: gpui_kit::AnyView, cx: &gpui_kit::App) {
        if cx.reduced_motion() {
            return;
        }
        let mut fade = Flick::at(1., cx.background_executor().now());
        fade.to(0., Tuning::LEAVE, cx.background_executor().now());
        self.closing = Some((view, fade));
    }

    fn start_coach(&mut self, id: CoachTourId, cx: &mut Context<Self>) {
        let tour = cx.new(|cx| CoachTour::at(coach_tour(id), cx.background_executor().now()));
        let subscription = cx.subscribe(&tour, |this, _, _: &CoachDone, cx| {
            this.coach = None;
            cx.notify();
        });
        self.coach = Some((tour, subscription));
        cx.notify();
    }
}

/// `coachTourFor`: the tour for where setup sent the operator.
pub fn coach_tour_for(route: Option<QuickStart>) -> CoachTourId {
    match route {
        None => CoachTourId::Operator,
        Some(QuickStart::Scripture) => CoachTourId::Scripture,
        Some(QuickStart::Lyrics) => CoachTourId::Lyrics,
        Some(QuickStart::Documents) => CoachTourId::Documents,
    }
}

impl Render for AccountFlow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let closing = if let Some((view, fade)) = &mut self.closing {
            if fade.tick(cx.background_executor().now(), cx.reduced_motion()) {
                window.request_animation_frame();
                Some(
                    div()
                        .absolute()
                        .inset_0()
                        .opacity(fade.value().clamp(0., 1.))
                        .child(view.clone()),
                )
            } else {
                None
            }
        } else {
            None
        };
        if closing.is_none() {
            self.closing = None;
        }
        let front = if let Some((wizard, _)) = &self.wizard {
            Some(wizard.clone().into_any_element())
        } else {
            self.gate.clone().map(|gate| gate.into_any_element())
        };
        let coach = self
            .coach
            .as_ref()
            .filter(|_| front.is_none() && closing.is_none())
            .map(|(tour, _)| tour.clone().into_any_element());
        // Nothing to show takes no space and no input.
        div()
            .absolute()
            .top_0()
            .left_0()
            .when(
                front.is_some() || coach.is_some() || closing.is_some(),
                |this| this.size_full(),
            )
            .when(front.is_none(), |el| el.children(closing))
            .children(front)
            .children(coach)
    }
}
