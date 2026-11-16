//! Pure presentation policy; persisted progress remains owned by kairo-domain.
use kairo_domain::cloud::onboarding_steps;
use kairo_domain::settings::{OnboardingState, OnboardingStepId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Tour,
    Account,
    Verify,
    Activate,
    Output,
    Welcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupState {
    pub page: Page,
    pub index: usize,
    pub back: Option<OnboardingStepId>,
    pub ready: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Actions {
    pub back: bool,
    pub skip: bool,
    pub later: bool,
    pub primary: bool,
    pub retry: bool,
}
impl SetupState {
    pub fn derive(
        state: &OnboardingState,
        signed_in: bool,
        verifying: bool,
        activated: bool,
        welcomed: bool,
    ) -> Self {
        let steps = onboarding_steps();
        let index = steps
            .iter()
            .position(|s| *s == state.current_step)
            .unwrap_or(0);
        let ready = signed_in && activated;
        let page = if welcomed {
            Page::Welcome
        } else if index == 0 {
            Page::Tour
        } else if steps[index] == OnboardingStepId::Account || !ready {
            if verifying {
                Page::Verify
            } else if signed_in {
                Page::Activate
            } else {
                Page::Account
            }
        } else {
            Page::Output
        };
        let back = steps[..index]
            .iter()
            .rev()
            .copied()
            .find(|s| *s != OnboardingStepId::Account || !ready);
        Self {
            page,
            index,
            back,
            ready,
        }
    }
    pub fn actions(self, slide: usize, error: bool) -> Actions {
        let retry = self.account() && self.ready && error;
        Actions {
            back: self.page != Page::Welcome
                && if self.page == Page::Tour {
                    slide > 0
                } else {
                    self.back.is_some()
                },
            skip: self.page == Page::Tour,
            later: self.page == Page::Output && self.ready,
            primary: matches!(self.page, Page::Tour | Page::Output) || retry,
            retry,
        }
    }
    pub fn account(self) -> bool {
        matches!(self.page, Page::Account | Page::Verify | Page::Activate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_is_guarded_by_confirmation_and_activation() {
        let state = OnboardingState {
            current_step: OnboardingStepId::Output,
            ..Default::default()
        };
        assert_eq!(
            SetupState::derive(&state, false, false, false, false).page,
            Page::Account
        );
        assert_eq!(
            SetupState::derive(&state, false, true, false, false).page,
            Page::Verify
        );
        assert_eq!(
            SetupState::derive(&state, true, false, false, false).page,
            Page::Activate
        );
        let ready = SetupState::derive(&state, true, false, true, false);
        assert_eq!(ready.page, Page::Output);
        assert_eq!(ready.back, Some(OnboardingStepId::Welcome));
        assert_eq!(
            SetupState::derive(&state, true, false, true, true).page,
            Page::Welcome
        );
    }
    #[test]
    fn account_actions_do_not_duplicate_submit_but_offer_write_retry() {
        let state = OnboardingState {
            current_step: OnboardingStepId::Account,
            ..Default::default()
        };
        let auth = SetupState::derive(&state, false, false, false, false);
        assert!(!auth.actions(0, false).primary);
        let ready = SetupState::derive(&state, true, false, true, false);
        assert!(ready.actions(0, true).retry && ready.actions(0, true).primary);
        let welcome = SetupState::derive(&state, true, false, true, true).actions(5, false);
        assert!(!welcome.back && !welcome.skip && !welcome.later && !welcome.primary);
    }
}
