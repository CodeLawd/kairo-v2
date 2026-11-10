//! Request spacing, as the Electron services keep it.

use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

/// Reserves send slots synchronously so concurrent callers cannot all wake
/// at once and burst past the gap.
pub struct Throttle {
    gap: Duration,
    next: Mutex<Option<Instant>>,
}

impl Throttle {
    pub fn spaced(gap: Duration) -> Self {
        Self {
            gap,
            next: Mutex::new(None),
        }
    }

    pub async fn wait(&self) {
        let slot = {
            let mut next = self.next.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let slot = next.map_or(now, |n| n.max(now));
            *next = Some(slot + self.gap);
            slot
        };
        tokio::time::sleep_until(slot).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn bursts_are_spaced() {
        let throttle = std::sync::Arc::new(Throttle::spaced(Duration::from_millis(350)));
        let start = Instant::now();
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let t = throttle.clone();
                tokio::spawn(async move {
                    t.wait().await;
                    Instant::now()
                })
            })
            .collect();
        let mut times = Vec::new();
        for h in handles {
            times.push(h.await.unwrap() - start);
        }
        times.sort();
        assert_eq!(
            times,
            [
                Duration::ZERO,
                Duration::from_millis(350),
                Duration::from_millis(700)
            ]
        );
    }
}
