//! What replaces `ErrorBoundary.tsx` and the renderer reload.
//!
//! A panic while building one workspace's element is caught at that
//! workspace ([`crate::boundary`]); the rest of the window keeps working and
//! the workspace shows a fallback with Reload. Like the renderer reload in
//! `src/main/index.ts`, recovery stops after more than three failures within
//! 60 seconds, so a panic on every frame cannot loop.

use std::time::{Duration, Instant};

/// The window the retry limit counts over.
pub const WINDOW: Duration = Duration::from_secs(60);
/// Failures allowed inside the window before recovery stops.
pub const LIMIT: usize = 3;

/// Counts recent failures.
#[derive(Clone, Debug, Default)]
pub struct CrashGuard {
    recent: Vec<Instant>,
}

impl CrashGuard {
    /// Records a failure at `now`. Returns whether the feature may be
    /// rebuilt automatically.
    pub fn record(&mut self, now: Instant) -> bool {
        self.recent.retain(|at| now.duration_since(*at) <= WINDOW);
        self.recent.push(now);
        self.recent.len() <= LIMIT
    }

    /// Failures within the window ending at `now`.
    pub fn recent(&self, now: Instant) -> usize {
        self.recent
            .iter()
            .filter(|at| now.duration_since(**at) <= WINDOW)
            .count()
    }
}

/// The text of a caught panic payload.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "Unknown error".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_after_more_than_three_in_a_minute() {
        let mut guard = CrashGuard::default();
        let t = Instant::now();
        assert!(guard.record(t));
        assert!(guard.record(t + Duration::from_secs(1)));
        assert!(guard.record(t + Duration::from_secs(2)));
        assert!(!guard.record(t + Duration::from_secs(3)));
        // A minute later the old failures no longer count.
        assert!(guard.record(t + Duration::from_secs(70)));
        assert_eq!(guard.recent(t + Duration::from_secs(70)), 1);
    }

    #[test]
    fn reads_panic_payloads() {
        let caught = std::panic::catch_unwind(|| panic!("boom {}", 1)).unwrap_err();
        assert_eq!(panic_message(caught.as_ref()), "boom 1");
        let caught = std::panic::catch_unwind(|| std::panic::panic_any(7u8)).unwrap_err();
        assert_eq!(panic_message(caught.as_ref()), "Unknown error");
    }
}
