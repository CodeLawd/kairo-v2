//! `api-bible-download-manager.ts`: resumable whole-translation downloads.
//! Each chapter commits in its own transaction, so an interrupted run resumes
//! without refetching completed work.

use super::cache::{ApiBibleCache, ApiBibleCachePassage, ApiBibleTranslationMeta, Coverage};
use super::client::{ApiBibleClient, RequestError};
use super::lookup::Clock;
use super::parse::canonical_book_id_from_usfm;
use super::policy::is_offline_download_permitted;
use crate::api::{ApiError, BoxFuture};
use kairo_domain::scripture::offline_state::api_cache_messages as msg;
use kairo_domain::scripture::{
    ApiBibleCacheStatus, ApiBibleDownloadProgress, ApiBibleOfflineTranslation, ScriptureVerse,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

pub const DEFAULT_CONCURRENCY: usize = 2;
const MAX_RATE_LIMIT_RETRIES: u32 = 3;
const DEFAULT_RETRY_DELAY_MS: f64 = 2_000.0;

/// A translation the account can reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedTranslation {
    pub bible_id: String,
    pub translation: String,
    pub name: String,
}

pub type Sleep = Arc<dyn Fn(Duration) -> BoxFuture<'static, ()> + Send + Sync>;

/// What the manager needs from the rest of the app. Each is read on use, so
/// the current account and settings always apply.
#[derive(Clone)]
pub struct DownloadDeps {
    pub cache: ApiBibleCache,
    /// `None` when no account can reach API.Bible.
    pub client: Arc<dyn Fn() -> Option<ApiBibleClient> + Send + Sync>,
    pub authorized: Arc<
        dyn Fn() -> BoxFuture<'static, Result<Vec<AuthorizedTranslation>, ApiError>> + Send + Sync,
    >,
    /// The resolved licence allowlist (`resolveOfflineDownloadBibleIds`).
    pub allowlist: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    pub clock: Clock,
    pub sleep: Sleep,
    pub concurrency: usize,
}

pub fn tokio_sleep() -> Sleep {
    Arc::new(|d| Box::pin(tokio::time::sleep(d)))
}

#[derive(Clone)]
struct ChapterTask {
    chapter_id: String,
    book_id: i64,
    book_name: String,
}

type Listener = Arc<dyn Fn(&ApiBibleDownloadProgress) + Send + Sync>;

#[derive(Default)]
struct State {
    paused: HashSet<String>,
    active: HashSet<String>,
    running: HashMap<String, watch::Receiver<bool>>,
    listeners: Vec<Listener>,
}

struct Inner {
    deps: DownloadDeps,
    state: Mutex<State>,
    pause_after_next: AtomicBool,
}

#[derive(Clone)]
pub struct ApiBibleDownloadManager(Arc<Inner>);

impl ApiBibleDownloadManager {
    pub fn new(deps: DownloadDeps) -> Self {
        Self(Arc::new(Inner {
            deps,
            state: Mutex::new(State::default()),
            pause_after_next: AtomicBool::new(false),
        }))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self) -> i64 {
        (self.0.deps.clock)()
    }

    /// Test and UX seam: stop after the chapter being committed.
    pub fn pause_after_next_chapter(&self) {
        self.0.pause_after_next.store(true, Ordering::SeqCst);
    }

    pub fn on_progress(
        &self,
        listener: impl Fn(&ApiBibleDownloadProgress) + Send + Sync + 'static,
    ) {
        self.state().listeners.push(Arc::new(listener));
    }

    /// The licence gate: an allowlisted id or public-domain text. A key alone
    /// never permits a whole-translation download.
    pub async fn is_offline_download_enabled(&self, bible_id: &str) -> bool {
        let copyright = self
            .0
            .deps
            .cache
            .translation_state(bible_id, self.now())
            .await
            .ok()
            .flatten()
            .map(|s| s.copyright)
            .unwrap_or_default();
        is_offline_download_permitted(bible_id, &(self.0.deps.allowlist)(), &copyright)
    }

    /// What the account authorizes, merged with what is cached, so a revoked
    /// translation can still be purged.
    pub async fn list_offline_translations(
        &self,
    ) -> Result<Vec<ApiBibleOfflineTranslation>, ApiError> {
        let now = self.now();
        let mut rows: Vec<ApiBibleOfflineTranslation> = Vec::new();
        for state in self.0.deps.cache.list_translation_states(now).await? {
            let enabled = self.is_offline_download_enabled(&state.bible_id).await;
            rows.push(state.with_licence(enabled));
        }
        // A listing failure must not hide what is already on disk.
        let authorized = match (self.0.deps.authorized)().await {
            Ok(list) => list,
            Err(error) => {
                tracing::warn!(%error, "[Scripture] Could not list authorized Bibles");
                Vec::new()
            }
        };
        for item in authorized {
            if rows.iter().any(|r| r.bible_id == item.bible_id) {
                continue;
            }
            let enabled = self.is_offline_download_enabled(&item.bible_id).await;
            rows.push(ApiBibleOfflineTranslation {
                bible_id: item.bible_id,
                translation: item.translation,
                name: item.name,
                copyright: String::new(),
                status: ApiBibleCacheStatus::NotDownloaded,
                cached_chapters: 0.0,
                total_chapters: 0.0,
                cached_verses: 0.0,
                fetched_at: None,
                expires_at: None,
                offline_download_enabled: enabled,
                error: None,
            });
        }
        // `localeCompare`; an ordinal compare of lower-cased names is close
        // enough for the English edition names this list holds.
        rows.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(rows)
    }

    /// Downloads do not survive an app exit. Reopen them as paused.
    pub async fn recover_interrupted_downloads(&self) -> Result<(), ApiError> {
        for state in self
            .0
            .deps
            .cache
            .list_translation_states(self.now())
            .await?
        {
            if state.status == ApiBibleCacheStatus::Downloading {
                self.0
                    .deps
                    .cache
                    .set_status(&state.bible_id, ApiBibleCacheStatus::Paused, None)
                    .await?;
                tracing::info!(bible_id = %state.bible_id, "[Scripture] Reopened interrupted download as paused");
            }
        }
        Ok(())
    }

    async fn require_licence(&self, bible_id: &str) -> Result<(), ApiError> {
        if self.is_offline_download_enabled(bible_id).await {
            Ok(())
        } else {
            Err(ApiError(msg::OFFLINE_DOWNLOAD_NOT_PERMITTED.into()))
        }
    }

    async fn require_cached(&self, bible_id: &str) -> Result<(), ApiError> {
        if self.has_cached_translation(bible_id).await {
            Ok(())
        } else {
            Err(ApiError(format!(
                "{bible_id} has no cached content to refresh."
            )))
        }
    }

    pub async fn start_download(&self, bible_id: &str) -> Result<(), ApiError> {
        self.require_licence(bible_id).await?;
        self.run(bible_id).await
    }

    /// Re-fetches chapters at or past the 30-day boundary. A refresh fetches
    /// the whole translation, so it needs the same licence as a download.
    pub async fn refresh_translation(&self, bible_id: &str) -> Result<(), ApiError> {
        self.require_licence(bible_id).await?;
        self.require_cached(bible_id).await?;
        self.run(bible_id).await
    }

    /// Returns once the download is running, so the caller can still pause it.
    pub async fn start_download_in_background(&self, bible_id: &str) -> Result<(), ApiError> {
        self.require_licence(bible_id).await?;
        self.detach(bible_id);
        Ok(())
    }

    pub async fn refresh_in_background(&self, bible_id: &str) -> Result<(), ApiError> {
        self.require_licence(bible_id).await?;
        self.require_cached(bible_id).await?;
        self.detach(bible_id);
        Ok(())
    }

    /// Resolves once background downloads have settled.
    pub async fn when_idle(&self) {
        let pending: Vec<_> = self.state().running.values().cloned().collect();
        for mut rx in pending {
            let _ = rx.wait_for(|done| *done).await;
        }
    }

    pub async fn has_cached_translation(&self, bible_id: &str) -> bool {
        matches!(
            self.0
                .deps
                .cache
                .translation_state(bible_id, self.now())
                .await,
            Ok(Some(_))
        )
    }

    pub fn pause_download(&self, bible_id: &str) {
        self.state().paused.insert(bible_id.to_owned());
    }

    pub async fn remove_translation(&self, bible_id: &str) -> Result<(), ApiError> {
        self.pause_download(bible_id);
        self.0.deps.cache.remove_translation(bible_id).await?;
        self.emit(&ApiBibleDownloadProgress {
            bible_id: bible_id.to_owned(),
            status: ApiBibleCacheStatus::NotDownloaded,
            completed_chapters: 0.0,
            total_chapters: 0.0,
            cached_verses: 0.0,
            error: None,
        });
        Ok(())
    }

    /// Failures land in the cache state and a progress event.
    fn detach(&self, bible_id: &str) {
        let (tx, rx) = watch::channel(false);
        {
            let mut state = self.state();
            if state.running.contains_key(bible_id) {
                return;
            }
            state.running.insert(bible_id.to_owned(), rx);
        }
        let this = self.clone();
        let id = bible_id.to_owned();
        tokio::spawn(async move {
            let _ = this.run(&id).await;
            this.state().running.remove(&id);
            let _ = tx.send(true);
        });
    }

    async fn run(&self, bible_id: &str) -> Result<(), ApiError> {
        {
            let mut state = self.state();
            if !state.active.insert(bible_id.to_owned()) {
                return Ok(());
            }
            state.paused.remove(bible_id);
        }
        let result = self.pipeline(bible_id).await;
        if let Err(error) = &result {
            let cache = &self.0.deps.cache;
            let unavailable = matches!(
                cache.translation_state(bible_id, self.now()).await,
                Ok(Some(s)) if s.status == ApiBibleCacheStatus::Unavailable
            );
            if !unavailable {
                let _ = cache
                    .set_status(bible_id, ApiBibleCacheStatus::Failed, Some(error.0.clone()))
                    .await;
            }
            self.emit_state(bible_id).await;
            tracing::warn!(bible_id, message = %error, "[Scripture] Offline Bible download failed");
        }
        {
            let mut state = self.state();
            state.active.remove(bible_id);
            state.paused.remove(bible_id);
        }
        self.0.pause_after_next.store(false, Ordering::SeqCst);
        result
    }

    async fn pipeline(&self, bible_id: &str) -> Result<(), ApiError> {
        let cache = &self.0.deps.cache;
        let client = (self.0.deps.client)()
            .ok_or_else(|| ApiError("Sign in to download Bibles for offline use.".into()))?;
        let details = self
            .guard(bible_id, client.get_bible(bible_id).await)
            .await?;
        cache
            .upsert_translation(ApiBibleTranslationMeta {
                bible_id: bible_id.into(),
                translation: details
                    .abbreviation_local
                    .or(details.abbreviation)
                    .unwrap_or_else(|| bible_id.into()),
                name: details.name,
                copyright: details.copyright,
                total_chapters: None,
            })
            .await?;
        cache.mark_access_checked(bible_id, self.now()).await?;
        cache
            .set_status(bible_id, ApiBibleCacheStatus::Downloading, None)
            .await?;
        self.emit_state(bible_id).await;

        let tasks = self.discover(bible_id, &client).await?;
        cache
            .set_total_chapters(bible_id, tasks.len() as i64)
            .await?;
        let pending: HashSet<String> = cache
            .incomplete_chapters(
                bible_id,
                tasks.iter().map(|t| t.chapter_id.clone()).collect(),
                self.now(),
            )
            .await?
            .into_iter()
            .collect();
        let queue: Vec<ChapterTask> = tasks
            .into_iter()
            .filter(|t| pending.contains(&t.chapter_id))
            .collect();
        self.drain(bible_id, &client, queue).await?;

        let stopped = self.state().paused.contains(bible_id);
        let state = cache.translation_state(bible_id, self.now()).await?;
        let complete =
            state.is_some_and(|s| s.total_chapters > 0 && s.cached_chapters >= s.total_chapters);
        let status = if complete {
            ApiBibleCacheStatus::Downloaded
        } else if stopped {
            ApiBibleCacheStatus::Paused
        } else {
            ApiBibleCacheStatus::Partial
        };
        cache.set_status(bible_id, status, None).await?;
        self.emit_state(bible_id).await;
        Ok(())
    }

    async fn discover(
        &self,
        bible_id: &str,
        client: &ApiBibleClient,
    ) -> Result<Vec<ChapterTask>, ApiError> {
        let books = self
            .guard(bible_id, client.list_books(bible_id).await)
            .await?;
        let mut tasks = Vec::new();
        for book in books {
            // Apocrypha and other books outside the canonical map.
            let Some(book_id) = canonical_book_id_from_usfm(&book.id) else {
                continue;
            };
            let chapters = self
                .guard(bible_id, client.list_chapters(bible_id, &book.id).await)
                .await?;
            for chapter in chapters {
                if chapter.number.to_lowercase() == "intro" {
                    continue;
                }
                tasks.push(ChapterTask {
                    chapter_id: chapter.id,
                    book_id: book_id as i64,
                    book_name: book.name.clone(),
                });
            }
        }
        Ok(tasks)
    }

    async fn drain(
        &self,
        bible_id: &str,
        client: &ApiBibleClient,
        queue: Vec<ChapterTask>,
    ) -> Result<(), ApiError> {
        let queue = Arc::new(queue);
        let index = Arc::new(AtomicUsize::new(0));
        let workers = (0..self.0.deps.concurrency.max(1)).map(|_| {
            let (queue, index) = (queue.clone(), index.clone());
            async move {
                loop {
                    if self.state().paused.contains(bible_id) {
                        return Ok(());
                    }
                    let i = index.fetch_add(1, Ordering::SeqCst);
                    let Some(task) = queue.get(i) else {
                        return Ok(());
                    };
                    self.download_chapter(bible_id, client, task).await?;
                    if self.0.pause_after_next.swap(false, Ordering::SeqCst) {
                        self.pause_download(bible_id);
                    }
                }
            }
        });
        // `Promise.all`: the first failure rejects; others finish their
        // current chapter and stop because nothing awaits them further.
        futures_util::future::try_join_all(workers)
            .await
            .map(|_| ())
    }

    async fn download_chapter(
        &self,
        bible_id: &str,
        client: &ApiBibleClient,
        task: &ChapterTask,
    ) -> Result<(), ApiError> {
        let mut attempt = 1;
        loop {
            match client
                .get_chapter(bible_id, &task.chapter_id, &task.book_name)
                .await
            {
                Ok(passage) => {
                    // Commit only after the response parses, so fetched_at
                    // never advances without current content.
                    let verses = passage
                        .verses
                        .into_iter()
                        .map(|v| ScriptureVerse {
                            book: v.book,
                            chapter: v.chapter,
                            verse: v.verse,
                            text: v.text,
                            extra: Default::default(),
                        })
                        .collect();
                    self.0
                        .deps
                        .cache
                        .put_chapter(
                            bible_id,
                            &task.chapter_id,
                            ApiBibleCachePassage {
                                book_id: task.book_id,
                                book_name: task.book_name.clone(),
                                verses,
                                fetched_at: self.now(),
                                coverage: Some(Coverage {
                                    chapter: chapter_number_of(&task.chapter_id),
                                    verse_start: None,
                                    verse_end: None,
                                }),
                            },
                        )
                        .await?;
                    self.emit_state(bible_id).await;
                    return Ok(());
                }
                Err(error) if error.is_rate_limited() && attempt <= MAX_RATE_LIMIT_RETRIES => {
                    let ms = error.retry_after_ms.unwrap_or(DEFAULT_RETRY_DELAY_MS);
                    (self.0.deps.sleep)(Duration::from_secs_f64(ms / 1000.0)).await;
                    attempt += 1;
                }
                Err(error) => return self.guard(bible_id, Err(error)).await,
            }
        }
    }

    /// Turns a refused request into a purged `unavailable` translation.
    async fn guard<T>(
        &self,
        bible_id: &str,
        result: Result<T, RequestError>,
    ) -> Result<T, ApiError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) if error.is_access_error() => {
                self.0
                    .deps
                    .cache
                    .mark_unavailable(bible_id, msg::ACCESS_REVOKED)
                    .await?;
                self.emit_state(bible_id).await;
                Err(ApiError(msg::ACCESS_REVOKED.into()))
            }
            Err(error) => Err(ApiError(error.message)),
        }
    }

    async fn emit_state(&self, bible_id: &str) {
        let Ok(Some(state)) = self
            .0
            .deps
            .cache
            .translation_state(bible_id, self.now())
            .await
        else {
            return;
        };
        self.emit(&ApiBibleDownloadProgress {
            bible_id: bible_id.to_owned(),
            status: state.status,
            completed_chapters: state.cached_chapters as f64,
            total_chapters: state.total_chapters as f64,
            cached_verses: state.cached_verses as f64,
            error: state.error,
        });
    }

    fn emit(&self, progress: &ApiBibleDownloadProgress) {
        let listeners = self.state().listeners.clone();
        for listener in listeners {
            listener(progress);
        }
    }
}

/// `JHN.3` is chapter 3.
fn chapter_number_of(chapter_id: &str) -> i64 {
    chapter_id
        .rsplit('.')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}
