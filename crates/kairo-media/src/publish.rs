//! Coalesced library snapshot pushes.
//!
//! Electron calls every listener on each change. Here a burst of changes
//! keeps only the newest snapshot, and a subscriber that wakes takes that one.

use std::sync::{Arc, Mutex};

pub struct Publisher<T> {
    subscribers: Vec<Subscription<T>>,
}

/// One receiving end. `next` waits for a change and returns the newest snapshot.
pub struct Subscription<T> {
    latest: Arc<Mutex<Option<T>>>,
    signal: async_channel::Sender<()>,
    wake: async_channel::Receiver<()>,
}

impl<T> Clone for Subscription<T> {
    fn clone(&self) -> Self {
        Self {
            latest: self.latest.clone(),
            signal: self.signal.clone(),
            wake: self.wake.clone(),
        }
    }
}

impl<T> Default for Publisher<T> {
    fn default() -> Self {
        Self {
            subscribers: Vec::new(),
        }
    }
}

impl<T: Clone> Publisher<T> {
    pub fn subscribe(&mut self) -> Subscription<T> {
        let (signal, wake) = async_channel::bounded(1);
        let subscription = Subscription {
            latest: Arc::new(Mutex::new(None)),
            signal,
            wake,
        };
        self.subscribers.push(subscription.clone());
        subscription
    }

    /// Replace each subscriber's pending snapshot. Dropped subscribers are pruned.
    pub fn publish(&mut self, snapshot: &T) {
        self.subscribers.retain(|s| s.wake.receiver_count() > 1);
        for subscriber in &self.subscribers {
            *subscriber.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot.clone());
            // Full means a wake is already pending; that wake will see this snapshot.
            let _ = subscriber.signal.try_send(());
        }
    }
}

impl<T> Subscription<T> {
    /// Wait for the next change. `None` once the publisher is gone.
    pub async fn next(&self) -> Option<T> {
        loop {
            if let Some(value) = self.take() {
                return Some(value);
            }
            if self.wake.sender_count() <= 1 && self.wake.is_empty() {
                return None;
            }
            self.wake.recv().await.ok()?;
        }
    }

    /// The pending snapshot, if one arrived since the last take.
    pub fn take(&self) -> Option<T> {
        let _ = self.wake.try_recv();
        self.latest.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_coalesces_to_the_newest() {
        let mut publisher = Publisher::default();
        let sub = publisher.subscribe();
        for n in 0..100 {
            publisher.publish(&n);
        }
        assert_eq!(sub.take(), Some(99));
        assert_eq!(sub.take(), None);
    }

    #[test]
    fn dropped_subscribers_are_pruned() {
        let mut publisher = Publisher::default();
        drop(publisher.subscribe());
        publisher.publish(&1);
        assert!(publisher.subscribers.is_empty());
    }
}
