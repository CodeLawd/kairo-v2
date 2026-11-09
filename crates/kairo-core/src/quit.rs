//! Confirmation shared by operator-window close and application quit.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    Prompting,
    Allowed,
}

/// A quit request stays pending until the operator responds.
#[derive(Default)]
pub struct QuitConfirmation {
    state: State,
}

impl QuitConfirmation {
    /// Returns true when quit is already allowed. Starts a prompt at most once.
    pub fn confirm(&mut self, prompt: impl FnOnce()) -> bool {
        match self.state {
            State::Allowed => true,
            State::Prompting => false,
            State::Idle => {
                self.state = State::Prompting;
                prompt();
                false
            }
        }
    }

    pub fn pending(&self) -> bool {
        self.state == State::Prompting
    }

    /// Accept responses only for a pending prompt.
    pub fn respond(&mut self, confirmed: bool) -> bool {
        if !self.pending() {
            return false;
        }
        self.state = if confirmed {
            State::Allowed
        } else {
            State::Idle
        };
        confirmed
    }

    /// Used by the updater once installation is authorized.
    pub fn allow(&mut self) {
        self.state = State::Allowed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_requests_prompt_once_and_cancellation_permits_retry() {
        let mut confirmation = QuitConfirmation::default();
        let mut prompts = 0;
        assert!(!confirmation.confirm(|| prompts += 1));
        assert!(!confirmation.confirm(|| prompts += 1));
        assert_eq!(prompts, 1);
        assert!(confirmation.pending());
        assert!(!confirmation.respond(false));
        assert!(!confirmation.pending());
        assert!(!confirmation.confirm(|| prompts += 1));
        assert_eq!(prompts, 2);
        assert!(confirmation.respond(true));
        assert!(confirmation.confirm(|| panic!("already confirmed")));
    }

    #[test]
    fn unsolicited_approval_cannot_bypass_confirmation() {
        let mut confirmation = QuitConfirmation::default();
        assert!(!confirmation.respond(true));
        assert!(!confirmation.confirm(|| {}));
    }

    #[test]
    fn update_installation_can_bypass_confirmation() {
        let mut confirmation = QuitConfirmation::default();
        assert!(!confirmation.confirm(|| {}));
        confirmation.allow();
        assert!(!confirmation.pending());
        assert!(confirmation.confirm(|| panic!("updater already allowed quit")));
    }
}
