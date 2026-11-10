use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio::sync::{broadcast, watch};

const EVENT_CAPACITY: usize = 64;

type Topics = HashMap<TypeId, Box<dyn Any + Send + Sync>>;

/// Topic producers can hold this independently of the runtime. The last
/// producer dropping closes its subscriptions.
#[derive(Clone, Default)]
pub struct Publisher {
    state: Arc<Mutex<Topics>>,
    events: Arc<Mutex<Topics>>,
}

fn topic<T: 'static, S: Clone + Send + Sync + 'static>(
    topics: &Mutex<Topics>,
    create: impl FnOnce() -> S,
) -> S {
    topics
        .lock()
        .expect("topic registry lock")
        .entry(TypeId::of::<T>())
        .or_insert_with(|| Box::new(create()))
        .downcast_ref::<S>()
        .expect("one sender type per topic")
        .clone()
}

/// The platform woke from sleep or the app window gained focus. The shell
/// emits this once; services that back off while offline or idle listen for
/// it instead of each exposing a wake call or polling the wall clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerEvent {
    Resume,
    Focus,
}

impl Publisher {
    fn state_topic<T: Send + Sync + 'static>(&self) -> watch::Sender<Option<Arc<T>>> {
        topic::<T, _>(&self.state, || watch::channel(None).0)
    }

    fn event_topic<T: Send + Sync + 'static>(&self) -> broadcast::Sender<Arc<T>> {
        topic::<T, _>(&self.events, || broadcast::channel(EVENT_CAPACITY).0)
    }

    /// Replace the last snapshot, even with no subscribers.
    pub fn publish<T: Send + Sync + 'static>(&self, value: T) {
        self.state_topic::<T>().send_replace(Some(Arc::new(value)));
    }

    /// Deliver in order to active subscribers. After 64 unread events, the
    /// oldest event is overwritten and the lagging subscriber sees a gap.
    /// Events published without subscribers are discarded.
    pub fn emit<T: Send + Sync + 'static>(&self, event: T) {
        let _ = self.event_topic::<T>().send(Arc::new(event));
    }

    pub fn watch<T: Send + Sync + 'static>(&self) -> Watch<T> {
        Watch {
            receiver: self.state_topic().subscribe(),
            initial: true,
        }
    }

    pub fn stream<T: Send + Sync + 'static>(&self) -> EventStream<T> {
        EventStream(self.event_topic().subscribe())
    }
}

/// One latest snapshot, followed by updates. A slow subscriber skips
/// superseded snapshots. Closing returns None after the last update.
pub struct Watch<T> {
    receiver: watch::Receiver<Option<Arc<T>>>,
    initial: bool,
}

impl<T> Watch<T> {
    pub async fn next(&mut self) -> Option<Arc<T>> {
        if std::mem::take(&mut self.initial)
            && let Some(value) = self.receiver.borrow_and_update().clone()
        {
            return Some(value);
        }
        self.receiver.changed().await.ok()?;
        self.latest()
    }

    /// Read the freshest snapshot and mark it seen, useful at frame boundaries.
    pub fn latest(&mut self) -> Option<Arc<T>> {
        self.initial = false;
        self.receiver.borrow_and_update().clone()
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum StreamError {
    #[error("Missed {0} events.")]
    Lagged(u64),
}

pub struct EventStream<T>(broadcast::Receiver<Arc<T>>);

impl<T> EventStream<T> {
    pub async fn next(&mut self) -> Option<Result<Arc<T>, StreamError>> {
        match self.0.recv().await {
            Ok(event) => Some(Ok(event)),
            Err(broadcast::error::RecvError::Lagged(count)) => {
                Some(Err(StreamError::Lagged(count)))
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}
