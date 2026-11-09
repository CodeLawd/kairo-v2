use std::{
    any::{Any, TypeId},
    collections::HashMap,
    future::Future,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

use tokio::{
    runtime::{Builder, Runtime},
    sync::{Semaphore, mpsc, oneshot, watch},
    task::JoinHandle,
};

use crate::Publisher;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CoreError {
    #[error("This service is unavailable.")]
    Unavailable,
    #[error("This service is already registered.")]
    AlreadyRegistered,
    #[error("The mailbox capacity is too large.")]
    InvalidCapacity,
    #[error("This service is busy. Try again.")]
    Busy,
    #[error("The application is shutting down.")]
    Stopped,
    #[error("The operation could not be completed.")]
    TaskFailed,
}

/// Implement once for each message handled by an actor. The actor owns its
/// state and processes messages serially, including across await points.
pub trait Handle<Message>: Send + Sized + 'static {
    type Reply: Send + 'static;
    fn handle(&mut self, message: Message) -> impl Future<Output = Self::Reply> + Send;
}

/// Routes a typed message to the single registered actor of this type.
pub trait Message: Send + Sized + 'static {
    type Actor: Handle<Self>;
}

type Job<A> = Box<
    dyn for<'a> FnOnce(&'a mut A) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'a>> + Send,
>;

struct Actor {
    mailbox: Box<dyn Any + Send + Sync>,
    task: JoinHandle<()>,
}

#[derive(Default)]
struct Actors {
    stopped: bool,
    entries: HashMap<TypeId, Actor>,
}

struct Inner {
    runtime: Option<Runtime>,
    actors: Mutex<Actors>,
    topics: Publisher,
    blocking: Arc<Semaphore>,
    stopping: watch::Sender<bool>,
    shutdown: crate::shutdown::Shutdown,
    stop_lock: tokio::sync::Mutex<()>,
}

impl Inner {
    fn runtime(&self) -> &Runtime {
        self.runtime.as_ref().expect("runtime exists until drop")
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

#[derive(Clone)]
pub struct AppCore(Arc<Inner>);

impl AppCore {
    pub fn new() -> std::io::Result<Self> {
        const BLOCKING_JOBS: usize = 8;
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("kairo-core")
            .max_blocking_threads(BLOCKING_JOBS)
            .build()?;
        Ok(Self(Arc::new(Inner {
            runtime: Some(runtime),
            actors: Mutex::default(),
            topics: Publisher::default(),
            blocking: Arc::new(Semaphore::new(BLOCKING_JOBS)),
            stopping: watch::channel(false).0,
            shutdown: crate::shutdown::Shutdown::default(),
            stop_lock: tokio::sync::Mutex::default(),
        })))
    }

    /// Register one instance per actor type. Pass dependencies such as the
    /// publisher to the actor, so it does not own the application runtime.
    pub fn register<A: Send + 'static>(
        &self,
        mut actor: A,
        capacity: NonZeroUsize,
    ) -> Result<(), CoreError> {
        let mut actors = self.0.actors.lock().expect("actor registry lock");
        if actors.stopped {
            return Err(CoreError::Stopped);
        }
        if capacity.get() > Semaphore::MAX_PERMITS {
            return Err(CoreError::InvalidCapacity);
        }
        let key = TypeId::of::<A>();
        if actors.entries.contains_key(&key) {
            return Err(CoreError::AlreadyRegistered);
        }
        let (sender, mut receiver) = mpsc::channel::<Job<A>>(capacity.get());
        let task = self.0.runtime().spawn(async move {
            while let Some(job) = receiver.recv().await {
                job(&mut actor).await;
            }
        });
        actors.entries.insert(
            key,
            Actor {
                mailbox: Box::new(sender),
                task,
            },
        );
        Ok(())
    }

    /// Wait for space in the mailbox and then for the typed reply. Dropping
    /// the call skips queued work. A handler that has started runs to completion
    /// unless the actor itself is stopped.
    pub async fn call<M: Message>(
        &self,
        message: M,
    ) -> Result<<M::Actor as Handle<M>>::Reply, CoreError> {
        let sender = self.mailbox::<M::Actor>()?;
        let (reply, result) = oneshot::channel();
        sender
            .send(Box::new(move |actor| {
                Box::pin(async move {
                    if !reply.is_closed() {
                        let value = actor.handle(message).await;
                        let _ = reply.send(value);
                    }
                })
            }))
            .await
            .map_err(|_| self.closed_mailbox())?;
        result.await.map_err(|_| self.closed_mailbox())
    }

    /// Fire and forget. A full mailbox rejects the new message immediately.
    pub fn send<M: Message>(&self, message: M) -> Result<(), CoreError>
    where
        M::Actor: Handle<M, Reply = ()>,
    {
        self.mailbox::<M::Actor>()?
            .try_send(Box::new(move |actor| Box::pin(actor.handle(message))))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => CoreError::Busy,
                mpsc::error::TrySendError::Closed(_) => self.closed_mailbox(),
            })
    }

    fn closed_mailbox(&self) -> CoreError {
        if self.0.actors.lock().expect("actor registry lock").stopped {
            CoreError::Stopped
        } else {
            CoreError::Unavailable
        }
    }

    fn mailbox<A: 'static>(&self) -> Result<mpsc::Sender<Job<A>>, CoreError> {
        let actors = self.0.actors.lock().expect("actor registry lock");
        if actors.stopped {
            return Err(CoreError::Stopped);
        }
        actors
            .entries
            .get(&TypeId::of::<A>())
            .and_then(|actor| actor.mailbox.downcast_ref::<mpsc::Sender<Job<A>>>())
            .cloned()
            .ok_or(CoreError::Unavailable)
    }

    pub fn publisher(&self) -> Publisher {
        self.0.topics.clone()
    }

    pub fn watch<T: Send + Sync + 'static>(&self) -> crate::Watch<T> {
        self.0.topics.watch()
    }

    pub fn stream<T: Send + Sync + 'static>(&self) -> crate::EventStream<T> {
        self.0.topics.stream()
    }

    /// Execute orchestration on the core runtime, even when awaited from GPUI.
    /// Dropping the awaiting future cancels the spawned work.
    pub async fn run<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
    ) -> Result<T, CoreError> {
        let mut stopping = self.0.stopping.subscribe();
        if *stopping.borrow() {
            return Err(CoreError::Stopped);
        }
        let mut task = CancelOnDrop(self.0.runtime().spawn(work));
        tokio::select! {
            biased;
            _ = stopping.changed() => Err(CoreError::Stopped),
            result = &mut task.0 => result.map_err(|_| CoreError::TaskFailed),
        }
    }

    /// Load startup resources on the core runtime. The progress callback is
    /// specific to this request, so it can publish to a window-local topic.
    pub async fn bootstrap(
        &self,
        loaders: crate::bootstrap::BootstrapLoaders,
        on_progress: impl FnMut(kairo_domain::status::BootstrapProgress) + Send + 'static,
    ) -> Result<kairo_domain::status::AppBootstrapSnapshot, CoreError> {
        self.run(crate::bootstrap::run_bootstrap(loaders, on_progress))
            .await
    }

    /// Run blocking filesystem or parsing work outside the actor workers.
    /// When all slots are occupied, callers await a permit before spawning.
    pub async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, CoreError> {
        let permit = self
            .0
            .blocking
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| CoreError::Stopped)?;
        let mut task = CancelOnDrop(self.0.runtime().spawn_blocking(move || {
            let _permit = permit;
            work()
        }));
        (&mut task.0).await.map_err(|_| CoreError::TaskFailed)
    }

    /// Run the single teardown after confirmation, then stop actor work.
    /// Dropping the waiter does not interrupt service cleanup.
    pub async fn shutdown(
        &self,
        services: Arc<impl crate::shutdown::ShutdownServices>,
    ) -> Result<Arc<crate::shutdown::ShutdownReport>, CoreError> {
        let core = self.clone();
        self.0
            .runtime()
            .spawn(async move {
                let report = core.0.shutdown.run(services).await;
                core.stop_actors().await;
                report.map_err(|_| CoreError::TaskFailed)
            })
            .await
            .map_err(|_| CoreError::TaskFailed)?
    }

    /// Close admission and cancel both queued and in-flight actor work.
    /// Invoke after the ordered service teardown. Started blocking work must
    /// cooperate separately and can finish after actor cancellation.
    pub async fn stop_actors(&self) {
        let _stopping = self.0.stop_lock.lock().await;
        let entries = {
            let mut actors = self.0.actors.lock().expect("actor registry lock");
            actors.stopped = true;
            std::mem::take(&mut actors.entries)
        };
        self.0.blocking.close();
        self.0.stopping.send_replace(true);
        for actor in entries.values() {
            actor.task.abort();
        }
        for actor in entries.into_values() {
            let _ = actor.task.await;
        }
    }
}

struct CancelOnDrop<T>(JoinHandle<T>);

impl<T> Drop for CancelOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
