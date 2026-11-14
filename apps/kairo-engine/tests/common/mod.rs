//! Helpers shared by the engine's integration tests.

use std::time::{Duration, Instant};

/// Waits up to 15 s for `done`, failing with `what` if it never holds.
pub async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
