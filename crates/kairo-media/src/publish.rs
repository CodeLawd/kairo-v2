//! Coalesced library snapshot pushes.
//!
//! Electron calls every listener on each change. Here a burst of changes
//! keeps only the newest snapshot, and a subscriber that wakes takes that one.

use std::sync::{Arc, Mutex};

type Slot<T> = Arc<Mutex<Option<T>>>;

/// Only the publisher holds the wake senders, so every subscription (clones
/// included) sees the channel close when the publisher is dropped.
pub struct Publisher<T> {
    subscribers: Vec<(Slot<T>, async_channel::Sender<()>)>,
}

/// One receiving end. `next` waits for a change and returns the newest
/// snapshot. Clones share one pending snapshot.
pub struct Subscription<T> {
    latest: Slot<T>,
    wake: async_channel::Receiver<()>,
}

impl<T> Clone for Subscription<T> {
    fn clone(&self) -> Self {
        Self {
            latest: self.latest.clone(),
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
        let latest = Slot::default();
        self.subscribers.push((latest.clone(), signal));
        Subscription { latest, wake }
    }

    /// Replace each subscriber's pending snapshot. Subscriptions whose every
    /// clone was dropped are pruned.
    pub fn publish(&mut self, snapshot: &T) {
        self.subscribers
            .retain(|(_, signal)| signal.receiver_count() > 0);
        for (latest, signal) in &self.subscribers {
            *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot.clone());
            // Full means a wake is already pending; that wake will see this snapshot.
            let _ = signal.try_send(());
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
            // Fails once the publisher (the only sender) is dropped.
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
    fn cloned_subscriptions_close_with_the_publisher() {
        let mut publisher = Publisher::<u32>::default();
        let sub = publisher.subscribe();
        let clone = sub.clone();
        publisher.publish(&1);
        drop(publisher);
        let next = futures_lite::future::block_on(sub.next());
        assert_eq!(next, Some(1), "a pending snapshot is still delivered");
        assert_eq!(futures_lite::future::block_on(clone.next()), None);
        assert_eq!(futures_lite::future::block_on(sub.next()), None);
    }

    #[test]
    fn dropped_subscribers_are_pruned() {
        let mut publisher = Publisher::default();
        drop(publisher.subscribe());
        publisher.publish(&1);
        assert!(publisher.subscribers.is_empty());
    }
}
