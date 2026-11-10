//! `quote-worker.ts` and `quote-worker-client.ts`: a dedicated OS thread owns
//! a read-only `bible.db` connection and the in-memory licensed index. The
//! client keeps one active and one newest queued search, so load cannot grow
//! memory, and restarts the thread lazily after a failure.

use super::index::CacheQuoteIndex;
use super::matching::{GlobalQuoteMatch, match_global_quote};
use super::retrieval::{WordRarityTable, retrieve_global_quote};
use crate::api_bible::cache::CachedQuoteVerse;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::{mpsc as tmpsc, oneshot};
use tokio::task::AbortHandle;

/// Search and cache-batch deadline, as in Electron.
pub const QUOTE_TIMEOUT: Duration = Duration::from_secs(5);

pub type QuoteResult<T> = Result<T, String>;

/// The worker's command set: versioned begin, append and commit, and search.
pub enum Command {
    Search {
        id: u64,
        text: String,
        possible: bool,
    },
    CacheBegin {
        version: i64,
    },
    CacheAppend {
        version: i64,
        rows: Vec<CachedQuoteVerse>,
        chunk: u64,
    },
    CacheCommit {
        version: i64,
    },
}

pub enum Event {
    Ready,
    /// The thread could not start (missing database, unreadable schema).
    Failed(String),
    Search {
        id: u64,
        result: QuoteResult<Vec<GlobalQuoteMatch>>,
    },
    CacheAck {
        chunk: u64,
        error: Option<String>,
    },
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Quotation worker failed".into())
}

fn now_ms() -> i64 {
    kairo_domain::js::now_ms() as i64
}

/// The worker thread body. Returns when the command channel closes.
pub fn run_worker(
    db_path: &Path,
    commands: mpsc::Receiver<Command>,
    events: tmpsc::UnboundedSender<Event>,
) {
    let opened = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .and_then(|db| {
        // better-sqlite3's build default: a 16 MB page cache keeps the FTS
        // index warm between searches.
        db.execute_batch("PRAGMA cache_size = -16000;")?;
        WordRarityTable::new(&db).map(|rarity| (db, rarity))
    });
    let (db, mut rarity) = match opened {
        Ok(pair) => pair,
        Err(error) => {
            let _ = events.send(Event::Failed(error.to_string()));
            return;
        }
    };
    let mut cache = CacheQuoteIndex::new();
    let _ = events.send(Event::Ready);
    for command in commands {
        match command {
            Command::Search { id, text, possible } => {
                let result = catch_unwind(AssertUnwindSafe(
                    || -> QuoteResult<Vec<GlobalQuoteMatch>> {
                        let mut candidates =
                            retrieve_global_quote(&db, &text).map_err(|e| e.to_string())?;
                        candidates
                            .extend(cache.search(&text, now_ms()).map_err(|e| e.to_string())?);
                        if !possible {
                            return Ok(match_global_quote(&text, &candidates, None));
                        }
                        let mut failure = None;
                        let mut lookup = |word: &str| match rarity.rarity(&db, word) {
                            Ok(v) => v,
                            Err(e) => {
                                failure.get_or_insert(e.to_string());
                                f64::INFINITY
                            }
                        };
                        let matches = match_global_quote(&text, &candidates, Some(&mut lookup));
                        failure.map_or(Ok(matches), Err)
                    },
                ))
                .unwrap_or_else(|p| Err(panic_message(p)));
                let _ = events.send(Event::Search { id, result });
            }
            cache_command => {
                let chunk = match &cache_command {
                    Command::CacheAppend { chunk, .. } => Some(*chunk),
                    _ => None,
                };
                let outcome = catch_unwind(AssertUnwindSafe(|| match cache_command {
                    Command::CacheBegin { version } => {
                        cache.begin(version).map_err(|e| e.to_string())
                    }
                    Command::CacheAppend { version, rows, .. } => {
                        cache.append(version, &rows).map_err(|e| e.to_string())
                    }
                    Command::CacheCommit { version } => {
                        cache.commit(version);
                        Ok(())
                    }
                    Command::Search { .. } => Ok(()),
                }))
                .unwrap_or_else(|p| Err(panic_message(p)));
                // Fail closed for the licensed corpus, keeping the public index.
                if outcome.is_err() {
                    cache.close();
                }
                if let Some(chunk) = chunk {
                    let _ = events.send(Event::CacheAck {
                        chunk,
                        error: outcome.err(),
                    });
                }
            }
        }
    }
}

/// Starts a worker. Tests replace it to simulate crashes.
pub type Spawner =
    Arc<dyn Fn(&Path, mpsc::Receiver<Command>, tmpsc::UnboundedSender<Event>) + Send + Sync>;

pub fn thread_spawner() -> Spawner {
    Arc::new(|path, commands, events| {
        let path = path.to_owned();
        let spawned = std::thread::Builder::new()
            .name("scripture-quotes".into())
            .spawn(move || run_worker(&path, commands, events.clone()));
        if let Err(error) = spawned {
            tracing::warn!(%error, "[Scripture] Could not start the quotation worker");
        }
    })
}

struct Job {
    id: u64,
    text: String,
    possible: bool,
    reply: oneshot::Sender<QuoteResult<Vec<GlobalQuoteMatch>>>,
}

struct Worker {
    generation: u64,
    commands: mpsc::Sender<Command>,
}

#[derive(Default)]
struct State {
    worker: Option<Worker>,
    generation: u64,
    ready: bool,
    active: Option<Job>,
    queued: Option<Job>,
    next_id: u64,
    chunk_id: u64,
    chunks: HashMap<u64, oneshot::Sender<QuoteResult<()>>>,
    timer: Option<AbortHandle>,
    closed: bool,
    failure_listeners: Vec<Arc<dyn Fn() + Send + Sync>>,
}

struct Inner {
    db_path: PathBuf,
    spawner: Spawner,
    handle: Handle,
    state: Mutex<State>,
}

/// The persistent quotation worker client.
#[derive(Clone)]
pub struct QuoteWorkerClient(Arc<Inner>);

impl QuoteWorkerClient {
    /// Call inside the Tokio runtime that will serve replies.
    pub fn new(db_path: PathBuf) -> Self {
        Self::with_spawner(db_path, thread_spawner())
    }

    pub fn with_spawner(db_path: PathBuf, spawner: Spawner) -> Self {
        Self(Arc::new(Inner {
            db_path,
            spawner,
            handle: Handle::current(),
            state: Mutex::new(State::default()),
        }))
    }

    /// Identity without keeping the client alive.
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn on_failure(&self, listener: impl Fn() + Send + Sync + 'static) {
        self.state().failure_listeners.push(Arc::new(listener));
    }

    pub fn begin_cache(&self, version: i64) -> QuoteResult<()> {
        let mut state = self.state();
        self.ensure_worker(&mut state)?;
        self.send(&state, Command::CacheBegin { version });
        Ok(())
    }

    /// Resolves when the worker acknowledges the batch, so index construction
    /// cannot fill an unbounded queue.
    pub async fn append_cache(&self, version: i64, rows: Vec<CachedQuoteVerse>) -> QuoteResult<()> {
        let (chunk, rx) = {
            let mut state = self.state();
            if state.closed || state.worker.is_none() {
                return Err("Quotation worker unavailable".into());
            }
            state.chunk_id += 1;
            let chunk = state.chunk_id;
            let (tx, rx) = oneshot::channel();
            state.chunks.insert(chunk, tx);
            self.send(
                &state,
                Command::CacheAppend {
                    version,
                    rows,
                    chunk,
                },
            );
            (chunk, rx)
        };
        match tokio::time::timeout(QUOTE_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("Quotation worker unavailable".into()),
            Err(_) => {
                self.state().chunks.remove(&chunk);
                Err("Quotation cache batch timed out".into())
            }
        }
    }

    pub fn commit_cache(&self, version: i64) {
        let state = self.state();
        if !state.closed && state.worker.is_some() {
            self.send(&state, Command::CacheCommit { version });
        }
    }

    /// `possible` also asks for paraphrase candidates: finished speech only.
    /// A newer search replaces a queued one, which resolves empty.
    pub async fn search(&self, text: &str, possible: bool) -> QuoteResult<Vec<GlobalQuoteMatch>> {
        let rx = {
            let mut state = self.state();
            if state.closed {
                return Err("Quotation worker is closed".into());
            }
            state.next_id += 1;
            let (reply, rx) = oneshot::channel();
            let job = Job {
                id: state.next_id,
                text: text.to_owned(),
                possible,
                reply,
            };
            if state.active.is_some() {
                if let Some(old) = state.queued.replace(job) {
                    let _ = old.reply.send(Ok(Vec::new()));
                }
                drop(state);
            } else {
                let listeners = self.dispatch(&mut state, job);
                drop(state);
                call(listeners);
            }
            rx
        };
        rx.await
            .unwrap_or_else(|_| Err("Quotation worker closed".into()))
    }

    pub fn close(&self) {
        let listeners = {
            let mut state = self.state();
            state.closed = true;
            self.fail(&mut state, "Quotation worker closed")
        };
        call(listeners);
    }

    fn send(&self, state: &State, command: Command) {
        if let Some(worker) = &state.worker {
            let _ = worker.commands.send(command);
        }
    }

    fn ensure_worker(&self, state: &mut State) -> QuoteResult<()> {
        if state.closed {
            return Err("Quotation worker is closed".into());
        }
        if state.worker.is_some() {
            return Ok(());
        }
        state.ready = false;
        state.generation += 1;
        let generation = state.generation;
        let (commands, receiver) = mpsc::channel();
        let (events, mut incoming) = tmpsc::unbounded_channel();
        (self.0.spawner)(&self.0.db_path, receiver, events);
        state.worker = Some(Worker {
            generation,
            commands,
        });
        let this = self.clone();
        self.0.handle.spawn(async move {
            while let Some(event) = incoming.recv().await {
                if !this.handle(generation, event) {
                    return;
                }
            }
            // The thread ended: an exit while still current is a failure.
            let listeners = {
                let mut state = this.state();
                if state
                    .worker
                    .as_ref()
                    .is_some_and(|w| w.generation == generation)
                {
                    this.fail(&mut state, "Quotation worker exited")
                } else {
                    Vec::new()
                }
            };
            call(listeners);
        });
        Ok(())
    }

    /// Returns false once this generation is no longer current.
    fn handle(&self, generation: u64, event: Event) -> bool {
        let mut listeners = Vec::new();
        {
            let mut state = self.state();
            if state
                .worker
                .as_ref()
                .is_none_or(|w| w.generation != generation)
            {
                return false;
            }
            match event {
                Event::Ready => {
                    state.ready = true;
                    if let Some(job) = &state.active {
                        let command = Command::Search {
                            id: job.id,
                            text: job.text.clone(),
                            possible: job.possible,
                        };
                        self.send(&state, command);
                    }
                }
                Event::Failed(message) => listeners = self.fail(&mut state, &message),
                Event::CacheAck { chunk, error } => {
                    if let Some(tx) = state.chunks.remove(&chunk) {
                        let _ = tx.send(error.map_or(Ok(()), Err));
                    }
                }
                Event::Search { id, result } => {
                    if state.active.as_ref().is_some_and(|job| job.id == id) {
                        let job = state.active.take().expect("checked");
                        if let Some(timer) = state.timer.take() {
                            timer.abort();
                        }
                        let _ = job.reply.send(result);
                        if let Some(next) = state.queued.take() {
                            listeners = self.dispatch(&mut state, next);
                        }
                    }
                }
            }
        }
        call(listeners);
        true
    }

    fn dispatch(&self, state: &mut State, job: Job) -> Fallout {
        let id = job.id;
        state.active = Some(job);
        if let Err(error) = self.ensure_worker(state) {
            return self.fail(state, &error);
        }
        if state.ready
            && let Some(job) = &state.active
        {
            let command = Command::Search {
                id: job.id,
                text: job.text.clone(),
                possible: job.possible,
            };
            self.send(state, command);
        }
        let this = self.clone();
        let timer = self.0.handle.spawn(async move {
            tokio::time::sleep(QUOTE_TIMEOUT).await;
            let listeners = {
                let mut state = this.state();
                if state.active.as_ref().is_some_and(|j| j.id == id) {
                    this.fail(&mut state, "Quotation search timed out")
                } else {
                    Vec::new()
                }
            };
            call(listeners);
        });
        if let Some(old) = state.timer.replace(timer.abort_handle()) {
            old.abort();
        }
        Vec::new()
    }

    /// Rejects everything in flight and drops the thread's channel. Returns
    /// what to run once the lock is released: failure listeners first, then
    /// the rejections, the order Electron's synchronous emit gives.
    fn fail(&self, state: &mut State, error: &str) -> Fallout {
        if let Some(timer) = state.timer.take() {
            timer.abort();
        }
        let mut out: Fallout = Vec::new();
        if !state.closed {
            for listener in state.failure_listeners.clone() {
                out.push(Box::new(move || listener()));
            }
        }
        let error = error.to_owned();
        for (_, chunk) in state.chunks.drain() {
            let error = error.clone();
            out.push(Box::new(move || {
                let _ = chunk.send(Err(error));
            }));
        }
        for job in [state.active.take(), state.queued.take()]
            .into_iter()
            .flatten()
        {
            let error = error.clone();
            out.push(Box::new(move || {
                let _ = job.reply.send(Err(error));
            }));
        }
        state.ready = false;
        state.worker = None;
        out
    }
}

type Fallout = Vec<Box<dyn FnOnce() + Send>>;

fn call(fallout: Fallout) {
    for action in fallout {
        action();
    }
}
