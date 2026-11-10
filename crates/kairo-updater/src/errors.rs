//! `errors.ts`: keep updater diagnostics in the log, not in front of the operator.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Check,
    Download,
}

/// `updateErrorMessage`.
pub fn update_error_message(message: &str, operation: Operation) -> String {
    static NETWORK: OnceLock<Regex> = OnceLock::new();
    let network = NETWORK
        .get_or_init(|| {
            // JavaScript's `\b` and `\d` are ASCII-only.
            Regex::new(r"(?i)(?-u:\b)(?:5[0-9][0-9]|408|429)(?-u:\b)|network|timeout|timed out|econn|enotfound|offline|net::")
                .expect("valid")
        })
        .is_match(message);
    let action = match operation {
        Operation::Download => "download the update",
        Operation::Check => "check for updates",
    };
    let advice = if network {
        "Please check your connection and try again."
    } else {
        "Please try again."
    };
    format!("Could not {action}. {advice}")
}
