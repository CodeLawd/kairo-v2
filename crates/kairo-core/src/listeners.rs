//! The one synchronous listener registry services share.
//!
//! Listeners run in registration order on the notifying thread, outside the
//! registry's lock, so a listener may add or remove listeners. Use
//! [`crate::Publisher`] topics instead where coalesced or async delivery is
//! fine; this registry is for callers that must see every change in order.

use std::sync::{Arc, Mutex, MutexGuard};

type Listener<T> = Arc<dyn Fn(&T) + Send + Sync>;

struct Inner<T: ?Sized> {
    next: u64,
    listeners: Vec<(u64, Listener<T>)>,
}

/// A registry of `Fn(&T)` listeners. Clones share the registry.
pub struct Listeners<T: ?Sized> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T: ?Sized> Clone for Listeners<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: ?Sized> Default for Listeners<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next: 0,
                listeners: Vec::new(),
            })),
        }
    }
}

/// Identifies one registered listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ListenerId(pub u64);

/// Removes its listener when dropped.
#[must_use = "dropping the guard removes the listener"]
pub struct ListenerGuard<T: ?Sized> {
    listeners: Listeners<T>,
    id: ListenerId,
}

impl<T: ?Sized> Drop for ListenerGuard<T> {
    fn drop(&mut self) {
        self.listeners.remove(self.id);
    }
}

impl<T: ?Sized> Listeners<T> {
    fn lock(&self) -> MutexGuard<'_, Inner<T>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers `listener` for the registry's lifetime (or until removed).
    pub fn add(&self, listener: impl Fn(&T) + Send + Sync + 'static) -> ListenerId {
        let mut inner = self.lock();
        inner.next += 1;
        let id = inner.next;
        inner.listeners.push((id, Arc::new(listener)));
        ListenerId(id)
    }

    /// Registers `listener` until the returned guard is dropped.
    pub fn guard(&self, listener: impl Fn(&T) + Send + Sync + 'static) -> ListenerGuard<T> {
        ListenerGuard {
            id: self.add(listener),
            listeners: self.clone(),
        }
    }

    pub fn remove(&self, id: ListenerId) {
        self.lock().listeners.retain(|(i, _)| *i != id.0);
    }

    /// Calls every listener with `value`, in registration order. A listener
    /// that panics is logged and does not stop the others or the notifier.
    pub fn notify(&self, value: &T) {
        let listeners: Vec<Listener<T>> = self
            .lock()
            .listeners
            .iter()
            .map(|(_, l)| l.clone())
            .collect();
        for listener in listeners {
            let call = std::panic::AssertUnwindSafe(|| listener(value));
            if std::panic::catch_unwind(call).is_err() {
                tracing::error!("listener panicked");
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lock().listeners.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listeners_run_in_order_and_can_be_removed() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let listeners = Listeners::<u32>::default();
        let a = seen.clone();
        let first = listeners.add(move |v| a.lock().unwrap().push(("a", *v)));
        let b = seen.clone();
        let guard = listeners.guard(move |v| b.lock().unwrap().push(("b", *v)));
        listeners.notify(&1);
        listeners.remove(first);
        listeners.notify(&2);
        drop(guard);
        listeners.notify(&3);
        assert!(listeners.is_empty());
        assert_eq!(*seen.lock().unwrap(), [("a", 1), ("b", 1), ("b", 2)]);
    }

    #[test]
    fn a_listener_may_register_another() {
        let listeners = Listeners::<()>::default();
        let inner = listeners.clone();
        listeners.add(move |_| {
            inner.add(|_| {});
        });
        listeners.notify(&());
        assert_eq!(listeners.lock().listeners.len(), 2);
    }
}
