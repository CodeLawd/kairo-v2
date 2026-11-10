//! The one wall-clock seam and the one retry backoff type.

use std::sync::Arc;
use std::time::Duration;

/// Wall-clock milliseconds since the Unix epoch, injectable for tests.
pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// The real clock: `Date.now()`.
pub fn system_clock() -> Clock {
    Arc::new(crate::js::now_ms)
}

/// How long to wait before retry number `attempt` (0 for the first retry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backoff {
    /// A fixed schedule; attempts past its end repeat the last step.
    Steps(&'static [Duration]),
    /// `base * 2^attempt`, capped at `max`.
    Exponential { base: Duration, max: Duration },
}

impl Backoff {
    pub fn delay(&self, attempt: u32) -> Duration {
        match *self {
            Self::Steps(steps) => steps
                .get(attempt as usize)
                .or(steps.last())
                .copied()
                .unwrap_or_default(),
            Self::Exponential { base, max } => {
                base.saturating_mul(2u32.saturating_pow(attempt)).min(max)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_repeat_the_last_and_exponential_caps() {
        const STEPS: &[Duration] = &[Duration::from_secs(1), Duration::from_secs(5)];
        let steps = Backoff::Steps(STEPS);
        assert_eq!(steps.delay(0), Duration::from_secs(1));
        assert_eq!(steps.delay(9), Duration::from_secs(5));
        let exponential = Backoff::Exponential {
            base: Duration::from_secs(1),
            max: Duration::from_secs(30),
        };
        let delays: Vec<u64> = (0..7).map(|a| exponential.delay(a).as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30]);
        assert_eq!(exponential.delay(u32::MAX), Duration::from_secs(30));
    }
}
