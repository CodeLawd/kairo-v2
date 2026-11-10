//! The quotation half of Electron's `ScriptureService`: corpus versions,
//! licensed-index rebuilds from the API.Bible cache, expiry, crash recovery
//! and eligibility at publication.

use super::matching::{GlobalQuoteCandidate, GlobalQuoteMatch};
use super::worker::{QuoteResult, QuoteWorkerClient, Spawner, thread_spawner};
use crate::api_bible::cache::{ApiBibleCache, QuoteCorpusChange};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::task::AbortHandle;

/// Delay before rebuilding after text was removed or the corpus reopened.
const INVALIDATE_DELAY: Duration = Duration::from_millis(150);
/// Quiet period before rebuilding after text was added.
const REFRESH_DEBOUNCE: Duration = Duration::from_secs(2);
const BATCH: i64 = 128;
/// Crash rebuilds per worker, so a failing worker is not rebuilt per query.
const MAX_CACHE_RESTARTS: u32 = 2;

type Listener = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct State {
    worker: Option<QuoteWorkerClient>,
    db_path: Option<PathBuf>,
    version: u64,
    build: i64,
    api_cache: Option<ApiBibleCache>,
    cache_generation: u64,
    cache_timer: Option<AbortHandle>,
    expiry_timer: Option<AbortHandle>,
    listeners: Vec<Listener>,
    stopped: bool,
}

struct Inner {
    handle: Handle,
    spawner: Spawner,
    state: Mutex<State>,
}

#[derive(Clone)]
pub struct QuoteCorpus(Arc<Inner>);

fn now_ms() -> i64 {
    kairo_store::settings::now_ms() as i64
}

impl QuoteCorpus {
    /// Call inside the Tokio runtime that will run rebuilds.
    pub fn new() -> Self {
        Self::with_spawner(thread_spawner())
    }

    pub fn with_spawner(spawner: Spawner) -> Self {
        Self(Arc::new(Inner {
            handle: Handle::current(),
            spawner,
            state: Mutex::new(State::default()),
        }))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start matching against the profile's `bible.db`.
    pub fn open(&self, db_path: PathBuf) {
        let old = {
            let mut state = self.state();
            state.db_path = Some(db_path.clone());
            state.stopped = false;
            let worker = self.create_worker(db_path);
            state.worker.replace(worker)
        };
        if let Some(old) = old {
            old.close();
        }
        self.invalidate_api();
    }

    /// Attach or detach the encrypted API.Bible cache.
    pub fn attach_api_cache(&self, cache: Option<ApiBibleCache>) {
        let generation = {
            let mut state = self.state();
            state.cache_generation += 1;
            state.api_cache = cache.clone();
            state.cache_generation
        };
        if let Some(cache) = cache {
            let weak = Arc::downgrade(&self.0);
            cache.on_quote_corpus_change(move |change| {
                let Some(inner) = Weak::upgrade(&weak) else {
                    return;
                };
                let this = QuoteCorpus(inner);
                if this.state().cache_generation != generation {
                    return;
                }
                match change {
                    QuoteCorpusChange::Removed => this.invalidate_api(),
                    QuoteCorpusChange::Added => this.schedule_refresh(),
                }
            });
        }
        self.invalidate_api();
    }

    pub fn version(&self) -> u64 {
        self.state().version
    }

    /// Read-only FTS and scoring run on the worker thread, never in the UI.
    pub async fn search_general_quote(
        &self,
        text: &str,
        possible: bool,
    ) -> QuoteResult<Vec<GlobalQuoteMatch>> {
        let (version, worker) = {
            let state = self.state();
            (state.version, state.worker.clone())
        };
        let Some(worker) = worker else {
            return Ok(Vec::new());
        };
        let matches = worker.search(text, possible).await?;
        if version != self.version() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for mut m in matches {
            m.candidate.quote_corpus_version = Some(version);
            if self.is_general_quote_eligible(&m.candidate).await {
                out.push(m);
            }
        }
        Ok(out)
    }

    /// Call again at the final publication boundary, after output lookup.
    pub async fn is_general_quote_eligible(&self, source: &GlobalQuoteCandidate) -> bool {
        let (version, cache) = {
            let state = self.state();
            (state.version, state.api_cache.clone())
        };
        if source.quote_corpus_version.is_some_and(|v| v != version) {
            return false;
        }
        match (&source.cache_source, cache) {
            (None, _) => true,
            (Some(s), Some(cache)) => {
                cache
                    .is_quote_source_eligible(&s.clone().into(), now_ms())
                    .await
            }
            (Some(_), None) => false,
        }
    }

    pub fn on_general_quote_corpus_change(&self, listener: impl Fn() + Send + Sync + 'static) {
        self.state().listeners.push(Arc::new(listener));
    }

    /// Local packs changed: restart the worker and rebuild.
    pub fn invalidate_quote_corpus(&self) {
        let old = {
            let mut state = self.state();
            let worker = state.db_path.clone().map(|p| self.create_worker(p));
            std::mem::replace(&mut state.worker, worker)
        };
        if let Some(old) = old {
            old.close();
        }
        self.invalidate_api();
    }

    pub fn close(&self) {
        let worker = {
            let mut state = self.state();
            state.stopped = true;
            state.version += 1;
            state.cache_generation += 1;
            clear_timers(&mut state);
            state.worker.take()
        };
        self.notify();
        if let Some(worker) = worker {
            worker.close();
        }
    }

    fn notify(&self) {
        let listeners = self.state().listeners.clone();
        for listener in listeners {
            // Diagnostics never interrupt a pack or cache change.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener()));
        }
    }

    /// Removed, revoked or expired text: old results stop being eligible.
    fn invalidate_api(&self) {
        self.state().version += 1;
        self.notify();
        self.rebuild(INVALIDATE_DELAY);
    }

    /// Added text: nothing became ineligible, so rebuild once it goes quiet.
    fn schedule_refresh(&self) {
        let mut state = self.state();
        if state.stopped || state.worker.is_none() || state.api_cache.is_none() {
            return;
        }
        if let Some(t) = state.cache_timer.take() {
            t.abort();
        }
        let this = self.clone();
        state.cache_timer = Some(
            self.0
                .handle
                .spawn(async move {
                    tokio::time::sleep(REFRESH_DEBOUNCE).await;
                    this.state().cache_timer = None;
                    this.rebuild(Duration::ZERO);
                })
                .abort_handle(),
        );
    }

    fn rebuild(&self, delay: Duration) {
        let mut state = self.state();
        state.build += 1;
        let build = state.build;
        clear_timers(&mut state);
        if state.stopped {
            return;
        }
        let Some(worker) = state.worker.clone() else {
            return;
        };
        // Clear the previous view at once; partial indexes are never queried.
        if let Err(error) = worker.begin_cache(build) {
            tracing::warn!(%error, "[Scripture] Cache recognition unavailable");
            return;
        }
        if state.api_cache.is_none() {
            worker.commit_cache(build);
            return;
        }
        let this = self.clone();
        let task = self.0.handle.spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
                this.state().cache_timer = None;
            }
            this.build_corpus(build).await;
        });
        if !delay.is_zero() {
            state.cache_timer = Some(task.abort_handle());
        }
    }

    fn current(&self, build: i64) -> bool {
        let state = self.state();
        !state.stopped && state.build == build
    }

    async fn build_corpus(&self, build: i64) {
        let (cache, worker) = {
            let state = self.state();
            (state.api_cache.clone(), state.worker.clone())
        };
        let (Some(cache), Some(worker)) = (cache, worker) else {
            return;
        };
        let mut cursor = 0;
        let mut expiry = i64::MAX;
        let result: QuoteResult<()> = async {
            while self.current(build) {
                let batch = cache
                    .read_quote_batch(cursor, BATCH, now_ms())
                    .await
                    .map_err(|e| e.0)?;
                if !batch.rows.is_empty() {
                    expiry = batch
                        .rows
                        .iter()
                        .map(|r| r.cache_source.expires_at)
                        .fold(expiry, i64::min);
                    worker.append_cache(build, batch.rows).await?;
                }
                if !self.current(build) {
                    return Ok(());
                }
                if batch.complete {
                    worker.commit_cache(build);
                    if expiry != i64::MAX {
                        let delay = (expiry - now_ms()).clamp(1, 2_147_483_647) as u64;
                        let this = self.clone();
                        let timer = self.0.handle.spawn(async move {
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                            this.state().expiry_timer = None;
                            this.invalidate_api();
                        });
                        let mut state = self.state();
                        if let Some(old) = state.expiry_timer.replace(timer.abort_handle()) {
                            old.abort();
                        }
                    }
                    return Ok(());
                }
                cursor = batch.next_cursor;
                // Decryption stays bounded and yields between batches.
                tokio::task::yield_now().await;
            }
            Ok(())
        }
        .await;
        if let Err(error) = result
            && self.current(build)
        {
            tracing::warn!(%error, "[Scripture] Cache recognition index failed");
            // Public and local matching still work.
            let _ = worker.begin_cache(build);
        }
    }

    fn create_worker(&self, db_path: PathBuf) -> QuoteWorkerClient {
        let worker = QuoteWorkerClient::with_spawner(db_path, self.0.spawner.clone());
        let weak = Arc::downgrade(&self.0);
        let restarts = Arc::new(Mutex::new(0u32));
        let id = worker.id();
        worker.on_failure(move || {
            let Some(inner) = Weak::upgrade(&weak) else {
                return;
            };
            let this = QuoteCorpus(inner);
            let rebuild = {
                let state = this.state();
                let current = state.worker.as_ref().is_some_and(|w| w.id() == id);
                let mut count = restarts.lock().unwrap_or_else(|e| e.into_inner());
                let ok = !state.stopped
                    && current
                    && state.api_cache.is_some()
                    && *count < MAX_CACHE_RESTARTS;
                if ok {
                    *count += 1;
                }
                ok
            };
            // A crash loses only the worker's copy; the cache is unchanged, so
            // cards already shown stay eligible.
            if rebuild {
                this.rebuild(INVALIDATE_DELAY);
            }
        });
        worker
    }
}

impl Default for QuoteCorpus {
    fn default() -> Self {
        Self::new()
    }
}

fn clear_timers(state: &mut State) {
    if let Some(t) = state.cache_timer.take() {
        t.abort();
    }
    if let Some(t) = state.expiry_timer.take() {
        t.abort();
    }
}
