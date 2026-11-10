//! Ported from `src/main/services/scripture/__tests__`: api-bible,
//! api-bible-client, api-bible-cache, scripture-cache-lookup,
//! api-bible-download-manager, offline-download-policy,
//! scripture-api-key-change, api-cache-policy and the detection fallbacks.
//! A mock API.Bible gateway serves catalogue, books, chapters, passages,
//! search, 401/403 and 429.

use kairo_domain::scripture::offline_state::api_cache_messages as msg;
use kairo_domain::scripture::{ApiBibleCacheStatus, ApiBibleDownloadProgress, ScriptureVerse};
use kairo_scripture::api::{ApiBible, ApiChapter, ApiError, ApiPassage, BoxFuture};
use kairo_scripture::api_bible::cache::{ApiBibleCachePassage, ApiBibleTranslationMeta, Coverage};
use kairo_scripture::api_bible::client::{BibleTransport, Query, RequestError};
use kairo_scripture::api_bible::detection::lookup_with_translation_fallback;
use kairo_scripture::api_bible::downloads::{
    ApiBibleDownloadManager, AuthorizedTranslation, DownloadDeps,
};
use kairo_scripture::api_bible::lookup::{Clock, build_passage_id};
use kairo_scripture::api_bible::parse::{
    ApiBibleSummary, build_api_bible_id_map, parse_api_bible_passage_content,
    parse_api_bible_search_response,
};
use kairo_scripture::api_bible::policy::*;
use kairo_scripture::api_bible::{ApiBibleCache, ApiBibleClient, ApiBibleSource, OfflineBibles};
use kairo_store::secrets::{ApiCacheCrypto, KeyProtector, SecretError};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

const BIBLE_ID: &str = "nkjv-01";
const NOW: i64 = 1_787_616_000_000;

// ─── Mock gateway ──────────────────────────────────────────────────────────

type Fail = Box<dyn Fn(&str, u32) -> Option<RequestError> + Send + Sync>;

#[derive(Default)]
struct Mock {
    /// Catalogue for `/v1/bibles`.
    bibles: Vec<Value>,
    books: Vec<(&'static str, &'static str)>,
    chapters: Vec<&'static str>,
    passage: Option<Value>,
    search: Option<Value>,
    details: Option<Result<Value, RequestError>>,
    fail: Option<Fail>,
    calls: Mutex<Vec<(String, Option<Query>)>>,
    attempts: Mutex<HashMap<String, u32>>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl Mock {
    fn new() -> Self {
        Self {
            books: vec![("JHN", "John")],
            chapters: vec!["intro", "1", "2", "3"],
            ..Default::default()
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _)| p.clone())
            .collect()
    }

    fn chapter_calls(&self) -> Vec<String> {
        let mut calls: Vec<String> = self
            .calls()
            .into_iter()
            .filter(|p| p.contains("/chapters/"))
            .map(|p| p.rsplit('/').next().unwrap().to_owned())
            .collect();
        calls.sort();
        calls
    }

    async fn answer(&self, path: &str) -> Result<Value, RequestError> {
        let attempt = {
            let mut a = self.attempts.lock().unwrap();
            let n = a.entry(path.to_owned()).or_insert(0);
            *n += 1;
            *n
        };
        if let Some(error) = self.fail.as_ref().and_then(|f| f(path, attempt)) {
            return Err(error);
        }
        let parts: Vec<&str> = path.trim_start_matches("/v1/bibles").split('/').collect();
        match parts.as_slice() {
            [""] => Ok(json!({ "data": self.bibles })),
            ["", id] => match &self.details {
                Some(d) => d.clone(),
                None => Ok(
                    json!({ "data": { "id": id, "name": "New King James Version",
                    "abbreviation": "engNKJV", "abbreviationLocal": "NKJV", "copyright": "© Thomas Nelson" } }),
                ),
            },
            ["", _, "books"] => Ok(json!({ "data": self.books.iter()
                .map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>() })),
            ["", _, "books", book, "chapters"] => Ok(json!({ "data": self.chapters.iter()
                .map(|n| json!({ "id": format!("{book}.{n}"), "number": n, "bookId": book })).collect::<Vec<_>>() })),
            ["", _, "chapters", id] => {
                let (book, n) = id.split_once('.').unwrap();
                let n: u32 = n.parse().unwrap();
                Ok(
                    json!({ "data": { "id": id, "copyright": "© Thomas Nelson", "content": [
                    { "type": "text", "text": format!("{book} {n}:1"), "attrs": { "verseId": format!("{book}.{n}.1") } }
                ] } }),
                )
            }
            ["", _, "passages", _] => Ok(self
                .passage
                .clone()
                .unwrap_or(json!({ "data": { "content": [] } }))),
            ["", _, "search"] => Ok(self
                .search
                .clone()
                .unwrap_or(json!({ "data": { "verses": [] } }))),
            _ => Err(RequestError::new("not found", Some(404))),
        }
    }
}

impl BibleTransport for Mock {
    fn get(
        &self,
        path: String,
        query: Option<Query>,
    ) -> BoxFuture<'_, Result<Value, RequestError>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.clone(), query));
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            tokio::task::yield_now().await;
            let result = self.answer(&path).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            result
        })
    }
}

fn error(status: u16) -> RequestError {
    RequestError::new(format!("HTTP {status}"), Some(status))
}

struct Keyring(Mutex<HashMap<String, [u8; 32]>>);

impl KeyProtector for Keyring {
    fn load(&self, slot: &str) -> Result<Option<[u8; 32]>, SecretError> {
        Ok(self.0.lock().unwrap().get(slot).copied())
    }
    fn save(&self, slot: &str, key: &[u8; 32]) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(slot.into(), *key);
        Ok(())
    }
}

async fn open_cache(dir: &TempDir) -> ApiBibleCache {
    let keyring = Keyring(Mutex::new(HashMap::new()));
    let (crypto, _) = ApiCacheCrypto::open(dir.path(), &keyring, "Kairo").unwrap();
    ApiBibleCache::open(&dir.path().join("api-bible-cache.db"), crypto)
        .await
        .unwrap()
}

fn fixed(now: i64) -> Clock {
    Arc::new(move || now)
}

fn verse(chapter: f64, n: f64, text: &str) -> ScriptureVerse {
    ScriptureVerse {
        book: "John".into(),
        chapter,
        verse: n,
        text: text.into(),
        extra: Default::default(),
    }
}

fn meta(id: &str, translation: &str, name: &str) -> ApiBibleTranslationMeta {
    ApiBibleTranslationMeta {
        bible_id: id.into(),
        translation: translation.into(),
        name: name.into(),
        copyright: "©".into(),
        total_chapters: None,
    }
}

async fn status(cache: &ApiBibleCache, now: i64) -> ApiBibleCacheStatus {
    cache
        .translation_state(BIBLE_ID, now)
        .await
        .unwrap()
        .unwrap()
        .status
}

// ─── Download manager ──────────────────────────────────────────────────────

struct Setup {
    _dir: TempDir,
    cache: ApiBibleCache,
    manager: ApiBibleDownloadManager,
    progress: Arc<Mutex<Vec<ApiBibleDownloadProgress>>>,
    sleeps: Arc<Mutex<Vec<Duration>>>,
}

fn deps(
    cache: ApiBibleCache,
    mock: Arc<Mock>,
    enabled: bool,
    clock: Clock,
    sleeps: Arc<Mutex<Vec<Duration>>>,
) -> DownloadDeps {
    let client = ApiBibleClient::new(mock);
    DownloadDeps {
        cache,
        client: Arc::new(move || Some(client.clone())),
        authorized: Arc::new(|| {
            Box::pin(async {
                Ok(vec![AuthorizedTranslation {
                    bible_id: BIBLE_ID.into(),
                    translation: "NKJV".into(),
                    name: "New King James Version".into(),
                }])
            })
        }),
        allowlist: Arc::new(move || {
            if enabled {
                vec![BIBLE_ID.into()]
            } else {
                Vec::new()
            }
        }),
        clock,
        sleep: Arc::new(move |d| {
            sleeps.lock().unwrap().push(d);
            Box::pin(async {})
        }),
        concurrency: 2,
    }
}

async fn setup(mock: Arc<Mock>, enabled: bool, clock: Clock) -> Setup {
    let dir = TempDir::new().unwrap();
    let cache = open_cache(&dir).await;
    let sleeps = Arc::new(Mutex::new(Vec::new()));
    let manager =
        ApiBibleDownloadManager::new(deps(cache.clone(), mock, enabled, clock, sleeps.clone()));
    let progress = Arc::new(Mutex::new(Vec::new()));
    let sink = progress.clone();
    manager.on_progress(move |p| sink.lock().unwrap().push(p.clone()));
    Setup {
        _dir: dir,
        cache,
        manager,
        progress,
        sleeps,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_to_download_without_confirmed_offline_rights() {
    let mock = Arc::new(Mock::new());
    let s = setup(mock.clone(), false, fixed(NOW)).await;
    let err = s.manager.start_download(BIBLE_ID).await.unwrap_err();
    assert_eq!(err.0, msg::OFFLINE_DOWNLOAD_NOT_PERMITTED);
    assert!(mock.calls().is_empty());
    let err = s
        .manager
        .start_download_in_background(BIBLE_ID)
        .await
        .unwrap_err();
    assert_eq!(err.0, msg::OFFLINE_DOWNLOAD_NOT_PERMITTED);
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_every_chapter_except_intro_with_two_in_flight() {
    let mock = Arc::new(Mock::new());
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    s.manager.start_download(BIBLE_ID).await.unwrap();
    assert_eq!(mock.chapter_calls(), ["JHN.1", "JHN.2", "JHN.3"]);
    assert!(mock.max_in_flight.load(Ordering::SeqCst) <= 2);
    let state = s
        .cache
        .translation_state(BIBLE_ID, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.status, ApiBibleCacheStatus::Downloaded);
    assert_eq!((state.total_chapters, state.cached_chapters), (3, 3));
    assert_eq!(state.copyright, "© Thomas Nelson");
    assert_eq!(
        s.progress.lock().unwrap().last().unwrap(),
        &ApiBibleDownloadProgress {
            bible_id: BIBLE_ID.into(),
            status: ApiBibleCacheStatus::Downloaded,
            completed_chapters: 3.0,
            total_chapters: 3.0,
            cached_verses: 3.0,
            error: None,
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn discovery_walks_every_book_and_skips_non_canonical_books() {
    let mock = Arc::new(Mock {
        books: vec![("GEN", "Genesis"), ("TOB", "Tobit"), ("REV", "Revelation")],
        chapters: vec!["intro", "1", "2"],
        ..Mock::new()
    });
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    s.manager.start_download(BIBLE_ID).await.unwrap();
    assert_eq!(mock.chapter_calls(), ["GEN.1", "GEN.2", "REV.1", "REV.2"]);
    assert!(!mock.calls().iter().any(|p| p.contains("TOB/chapters")));
    assert_eq!(
        s.cache
            .translation_state(BIBLE_ID, NOW)
            .await
            .unwrap()
            .unwrap()
            .total_chapters,
        4
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_persists_progress_and_resume_does_not_refetch() {
    let mock = Arc::new(Mock::new());
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    s.manager.pause_after_next_chapter();
    s.manager.start_download(BIBLE_ID).await.unwrap();
    let paused = s
        .cache
        .translation_state(BIBLE_ID, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, ApiBibleCacheStatus::Paused);
    assert!(paused.cached_chapters >= 1 && paused.cached_chapters < 3);
    let before = mock.chapter_calls().len() as i64;
    s.manager.start_download(BIBLE_ID).await.unwrap();
    assert_eq!(status(&s.cache, NOW).await, ApiBibleCacheStatus::Downloaded);
    assert_eq!(
        mock.chapter_calls().len() as i64,
        before + (3 - paused.cached_chapters)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_download_survives_restart_as_paused_and_resumes() {
    let mock = Arc::new(Mock::new());
    let dir = TempDir::new().unwrap();
    let keyring = Keyring(Mutex::new(HashMap::new()));
    {
        let (crypto, _) = ApiCacheCrypto::open(dir.path(), &keyring, "Kairo").unwrap();
        let cache = ApiBibleCache::open(&dir.path().join("api-bible-cache.db"), crypto)
            .await
            .unwrap();
        cache
            .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
            .await
            .unwrap();
        cache
            .set_status(BIBLE_ID, ApiBibleCacheStatus::Downloading, None)
            .await
            .unwrap();
    }
    // A new process: reopen the key and database.
    let (crypto, discarded) = ApiCacheCrypto::open(dir.path(), &keyring, "Kairo").unwrap();
    assert!(!discarded);
    let cache = ApiBibleCache::open(&dir.path().join("api-bible-cache.db"), crypto)
        .await
        .unwrap();
    let manager = ApiBibleDownloadManager::new(deps(
        cache.clone(),
        mock,
        true,
        fixed(NOW),
        Default::default(),
    ));
    manager.recover_interrupted_downloads().await.unwrap();
    assert_eq!(status(&cache, NOW).await, ApiBibleCacheStatus::Paused);
    manager.start_download(BIBLE_ID).await.unwrap();
    assert_eq!(status(&cache, NOW).await, ApiBibleCacheStatus::Downloaded);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rate_limited_chapter_waits_for_retry_after_then_retries() {
    let mock = Arc::new(Mock {
        fail: Some(Box::new(|path, attempt| {
            (path.ends_with("JHN.2") && attempt == 1).then(|| RequestError {
                retry_after_ms: Some(1_000.0),
                ..error(429)
            })
        })),
        ..Mock::new()
    });
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    s.manager.start_download(BIBLE_ID).await.unwrap();
    assert_eq!(
        mock.chapter_calls()
            .iter()
            .filter(|c| *c == "JHN.2")
            .count(),
        2
    );
    assert_eq!(*s.sleeps.lock().unwrap(), [Duration::from_secs(1)]);
    assert_eq!(status(&s.cache, NOW).await, ApiBibleCacheStatus::Downloaded);
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_without_retry_after_uses_two_seconds_and_gives_up_after_three_retries() {
    let mock = Arc::new(Mock {
        fail: Some(Box::new(|path, _| {
            path.ends_with("JHN.2").then(|| error(429))
        })),
        ..Mock::new()
    });
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    let err = s.manager.start_download(BIBLE_ID).await.unwrap_err();
    assert_eq!(err.0, "HTTP 429");
    assert_eq!(
        mock.chapter_calls()
            .iter()
            .filter(|c| *c == "JHN.2")
            .count(),
        4
    );
    assert_eq!(*s.sleeps.lock().unwrap(), [Duration::from_secs(2); 3]);
    assert_eq!(status(&s.cache, NOW).await, ApiBibleCacheStatus::Failed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_translation_is_marked_unavailable_and_purged() {
    let mock = Arc::new(Mock::new());
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    s.manager.start_download(BIBLE_ID).await.unwrap();
    assert!(
        s.cache
            .translation_state(BIBLE_ID, NOW)
            .await
            .unwrap()
            .unwrap()
            .cached_verses
            > 0
    );

    for code in [401, 403] {
        let revoked = Arc::new(Mock {
            details: Some(Err(error(code))),
            ..Mock::new()
        });
        let manager = ApiBibleDownloadManager::new(deps(
            s.cache.clone(),
            revoked.clone(),
            true,
            fixed(NOW),
            Default::default(),
        ));
        let progress = Arc::new(Mutex::new(Vec::new()));
        let sink = progress.clone();
        manager.on_progress(move |p| sink.lock().unwrap().push(p.status));
        let err = manager.start_download(BIBLE_ID).await.unwrap_err();
        assert_eq!(err.0, msg::ACCESS_REVOKED);
        let state = s
            .cache
            .translation_state(BIBLE_ID, NOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.status, ApiBibleCacheStatus::Unavailable);
        assert_eq!(state.cached_verses, 0);
        assert_eq!(
            progress.lock().unwrap().last(),
            Some(&ApiBibleCacheStatus::Unavailable)
        );
        assert!(revoked.chapter_calls().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_chapter_leaves_a_failed_state_with_its_error() {
    let mock = Arc::new(Mock {
        fail: Some(Box::new(|path, _| {
            path.ends_with("JHN.2")
                .then(|| RequestError::new("socket hang up", None))
        })),
        ..Mock::new()
    });
    let s = setup(mock, true, fixed(NOW)).await;
    assert!(s.manager.start_download(BIBLE_ID).await.is_err());
    let state = s
        .cache
        .translation_state(BIBLE_ID, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.status, ApiBibleCacheStatus::Failed);
    assert!(state.error.unwrap().contains("socket hang up"));
}

#[tokio::test(flavor = "multi_thread")]
async fn refresh_refetches_only_stale_chapters_and_keeps_old_times_on_failure() {
    let mock = Arc::new(Mock::new());
    let now = Arc::new(AtomicI64::new(NOW));
    let t = now.clone();
    let s = setup(
        mock.clone(),
        true,
        Arc::new(move || t.load(Ordering::SeqCst)),
    )
    .await;
    s.manager.start_download(BIBLE_ID).await.unwrap();
    let initial = mock.chapter_calls().len();
    let later = NOW + API_CACHE_MAX_AGE_MS;

    // A failing refresh leaves the old fetch time.
    let offline = Arc::new(Mock {
        fail: Some(Box::new(|p, _| {
            p.contains("/chapters/")
                .then(|| RequestError::new("offline", None))
        })),
        ..Mock::new()
    });
    let manager = ApiBibleDownloadManager::new(deps(
        s.cache.clone(),
        offline,
        true,
        fixed(later),
        Default::default(),
    ));
    assert!(manager.refresh_translation(BIBLE_ID).await.is_err());
    assert_eq!(
        s.cache
            .translation_state(BIBLE_ID, later)
            .await
            .unwrap()
            .unwrap()
            .fetched_at,
        Some(NOW)
    );

    now.store(later, Ordering::SeqCst);
    assert_eq!(status(&s.cache, later).await, ApiBibleCacheStatus::Failed);
    s.manager.refresh_translation(BIBLE_ID).await.unwrap();
    assert_eq!(mock.chapter_calls().len(), initial + 3);
    assert_eq!(
        status(&s.cache, later).await,
        ApiBibleCacheStatus::Downloaded
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refresh_needs_a_licence_and_cached_content() {
    let mock = Arc::new(Mock::new());
    let s = setup(mock.clone(), false, fixed(NOW)).await;
    s.cache
        .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
        .await
        .unwrap();
    assert_eq!(
        s.manager.refresh_translation(BIBLE_ID).await.unwrap_err().0,
        msg::OFFLINE_DOWNLOAD_NOT_PERMITTED
    );
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    assert_eq!(
        s.manager
            .refresh_in_background(BIBLE_ID)
            .await
            .unwrap_err()
            .0,
        format!("{BIBLE_ID} has no cached content to refresh.")
    );
    assert!(mock.chapter_calls().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn listing_merges_cache_state_authorization_and_the_licence_gate() {
    let mock = Arc::new(Mock::new());
    let s = setup(mock.clone(), false, fixed(NOW)).await;
    s.cache
        .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
        .await
        .unwrap();
    let rows = s.manager.list_offline_translations().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].offline_download_enabled);
    assert_eq!(rows[0].status, ApiBibleCacheStatus::NotDownloaded);

    // Authorized but never cached.
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    let rows = s.manager.list_offline_translations().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].name.as_str(), rows[0].offline_download_enabled),
        ("New King James Version", true)
    );

    // Revoked text still on disk stays listed so it can be purged.
    s.cache
        .upsert_translation(meta("gone-01", "NLT", "New Living Translation"))
        .await
        .unwrap();
    s.cache
        .mark_unavailable("gone-01", "Access revoked")
        .await
        .unwrap();
    let rows = s.manager.list_offline_translations().await.unwrap();
    let mut ids: Vec<_> = rows.iter().map(|r| r.bible_id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["gone-01", BIBLE_ID]);

    // Public-domain text needs no allowlist.
    s.cache
        .upsert_translation(ApiBibleTranslationMeta {
            copyright: "PUBLIC DOMAIN".into(),
            ..meta("kjv-01", "KJV", "KJV")
        })
        .await
        .unwrap();
    let s2 = ApiBibleDownloadManager::new(deps(
        s.cache.clone(),
        mock,
        false,
        fixed(NOW),
        Default::default(),
    ));
    assert!(s2.is_offline_download_enabled("kjv-01").await);
    assert!(!s2.is_offline_download_enabled(BIBLE_ID).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_api_still_lists_what_is_cached() {
    let mock = Arc::new(Mock::new());
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    s.cache
        .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
        .await
        .unwrap();
    let mut d = deps(s.cache.clone(), mock, true, fixed(NOW), Default::default());
    d.authorized = Arc::new(|| Box::pin(async { Err(ApiError("offline".into())) }));
    let rows = ApiBibleDownloadManager::new(d)
        .list_offline_translations()
        .await
        .unwrap();
    assert_eq!(
        rows.iter().map(|r| r.bible_id.as_str()).collect::<Vec<_>>(),
        [BIBLE_ID]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn remove_purges_every_row() {
    let s = setup(Arc::new(Mock::new()), true, fixed(NOW)).await;
    assert!(!s.manager.has_cached_translation(BIBLE_ID).await);
    s.manager.start_download(BIBLE_ID).await.unwrap();
    assert!(s.manager.has_cached_translation(BIBLE_ID).await);
    s.manager.remove_translation(BIBLE_ID).await.unwrap();
    assert!(
        s.cache
            .translation_state(BIBLE_ID, NOW)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(s.cache.count_rows(BIBLE_ID).await.unwrap(), [0; 4]);
    assert_eq!(
        s.progress.lock().unwrap().last().unwrap().status,
        ApiBibleCacheStatus::NotDownloaded
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn background_downloads_return_at_once_and_record_failures() {
    let s = setup(Arc::new(Mock::new()), true, fixed(NOW)).await;
    s.manager
        .start_download_in_background(BIBLE_ID)
        .await
        .unwrap();
    s.manager.when_idle().await;
    assert_eq!(status(&s.cache, NOW).await, ApiBibleCacheStatus::Downloaded);

    let failing = Arc::new(Mock {
        fail: Some(Box::new(|p, _| {
            p.contains("/chapters/JHN")
                .then(|| RequestError::new("socket hang up", None))
        })),
        ..Mock::new()
    });
    let s = setup(failing, true, fixed(NOW)).await;
    s.manager
        .start_download_in_background(BIBLE_ID)
        .await
        .unwrap();
    s.manager.when_idle().await;
    assert_eq!(status(&s.cache, NOW).await, ApiBibleCacheStatus::Failed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_background_download_can_be_paused() {
    let mock = Arc::new(Mock {
        chapters: (1..=40)
            .map(|n| &*Box::leak(n.to_string().into_boxed_str()))
            .collect(),
        ..Mock::new()
    });
    let s = setup(mock.clone(), true, fixed(NOW)).await;
    let manager = s.manager.clone();
    let pause = Arc::new(Mutex::new(false));
    let flag = pause.clone();
    s.manager.on_progress(move |p| {
        if p.completed_chapters >= 2.0 && !*flag.lock().unwrap() {
            *flag.lock().unwrap() = true;
            manager.pause_download(BIBLE_ID);
        }
    });
    s.manager
        .start_download_in_background(BIBLE_ID)
        .await
        .unwrap();
    s.manager.when_idle().await;
    let state = s
        .cache
        .translation_state(BIBLE_ID, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.status, ApiBibleCacheStatus::Paused);
    assert!(state.cached_chapters < 40);
}

// ─── Cache ─────────────────────────────────────────────────────────────────

fn passage(
    chapter: i64,
    verses: &[(f64, &str)],
    fetched_at: i64,
    coverage: Option<Coverage>,
) -> ApiBibleCachePassage {
    ApiBibleCachePassage {
        book_id: 43,
        book_name: "John".into(),
        verses: verses
            .iter()
            .map(|(n, t)| verse(chapter as f64, *n, t))
            .collect(),
        fetched_at,
        coverage,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cache_never_stores_plaintext_and_rejects_tampered_rows() {
    let dir = TempDir::new().unwrap();
    let cache = open_cache(&dir).await;
    cache
        .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
        .await
        .unwrap();
    cache
        .put_passage(
            BIBLE_ID,
            passage(3, &[(16.0, "For God so loved the world")], NOW, None),
        )
        .await
        .unwrap();
    assert_eq!(
        cache
            .get_range(BIBLE_ID, 43, 3, 16, None, NOW)
            .await
            .unwrap()
            .unwrap()[0]
            .text,
        "For God so loved the world"
    );
    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            bytes.extend(std::fs::read(path).unwrap());
        }
    }
    assert!(
        !bytes
            .windows(26)
            .any(|w| w == b"For God so loved the world")
    );

    // Fail closed: without its key the cache is discarded, never read.
    let other = TempDir::new().unwrap();
    std::fs::copy(
        dir.path().join("api-bible-cache.db"),
        other.path().join("api-bible-cache.db"),
    )
    .unwrap();
    let keyring = Keyring(Mutex::new(HashMap::new()));
    let (crypto, discarded) = ApiCacheCrypto::open(other.path(), &keyring, "Kairo").unwrap();
    assert!(discarded);
    let foreign = ApiBibleCache::open(&other.path().join("api-bible-cache.db"), crypto)
        .await
        .unwrap();
    assert!(
        foreign
            .get_range(BIBLE_ID, 43, 3, 16, None, NOW)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ranges_respect_gaps_coverage_freshness_and_chapter_replacement() {
    let dir = TempDir::new().unwrap();
    let cache = open_cache(&dir).await;
    cache
        .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
        .await
        .unwrap();
    // A translation that omits 5:6 still serves 5:5-7.
    let cov = Coverage {
        chapter: 5,
        verse_start: Some(5),
        verse_end: Some(7),
    };
    cache
        .put_passage(
            BIBLE_ID,
            passage(5, &[(5.0, "five"), (7.0, "seven")], NOW, Some(cov)),
        )
        .await
        .unwrap();
    let got = cache
        .get_range(BIBLE_ID, 43, 5, 5, Some(7), NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.iter().map(|v| v.verse).collect::<Vec<_>>(), [5.0, 7.0]);
    // Stale: present but not served.
    let later = NOW + API_CACHE_MAX_AGE_MS;
    assert!(
        cache
            .get_range(BIBLE_ID, 43, 5, 5, Some(7), later)
            .await
            .unwrap()
            .is_none()
    );
    assert!(cache.has_range(BIBLE_ID, 43, 5, 5, Some(7)).await.unwrap());
    assert_eq!(status(&cache, later).await, ApiBibleCacheStatus::Stale);
    // An uncovered gap is a miss.
    cache
        .put_passage(BIBLE_ID, passage(1, &[(1.0, "one")], NOW, None))
        .await
        .unwrap();
    assert!(
        cache
            .get_range(BIBLE_ID, 43, 1, 1, Some(3), NOW)
            .await
            .unwrap()
            .is_none()
    );
    // Refreshing a chapter drops verses the publisher removed.
    let whole = Some(Coverage {
        chapter: 2,
        verse_start: None,
        verse_end: None,
    });
    cache
        .put_chapter(
            BIBLE_ID,
            "JHN.2",
            passage(2, &[(1.0, "a"), (2.0, "b"), (3.0, "c")], NOW, whole),
        )
        .await
        .unwrap();
    cache
        .put_chapter(
            BIBLE_ID,
            "JHN.2",
            passage(2, &[(1.0, "a"), (3.0, "c")], NOW, whole),
        )
        .await
        .unwrap();
    assert!(
        cache
            .get_range(BIBLE_ID, 43, 2, 2, None, NOW)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        cache
            .get_range(BIBLE_ID, 43, 2, 1, Some(3), NOW)
            .await
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    // Revocation purges text and progress.
    cache
        .mark_unavailable(BIBLE_ID, msg::ACCESS_REVOKED)
        .await
        .unwrap();
    assert_eq!(cache.count_rows(BIBLE_ID).await.unwrap(), [1, 0, 0, 0]);
    assert!(
        cache
            .get_range(BIBLE_ID, 43, 2, 1, None, NOW)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn quote_batches_skip_stale_and_revoked_text() {
    let dir = TempDir::new().unwrap();
    let cache = open_cache(&dir).await;
    cache
        .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
        .await
        .unwrap();
    cache
        .put_passage(BIBLE_ID, passage(3, &[(16.0, "fresh")], NOW, None))
        .await
        .unwrap();
    cache
        .put_passage(
            BIBLE_ID,
            passage(4, &[(1.0, "old")], NOW - API_CACHE_MAX_AGE_MS, None),
        )
        .await
        .unwrap();
    let batch = cache.read_quote_batch(0, 500, NOW).await.unwrap();
    assert!(batch.complete);
    assert_eq!(
        batch
            .rows
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["fresh"]
    );
    let source = batch.rows[0].cache_source.clone();
    assert!(cache.is_quote_source_eligible(&source, NOW).await);
    cache.mark_unavailable(BIBLE_ID, "x").await.unwrap();
    assert!(!cache.is_quote_source_eligible(&source, NOW).await);
    assert!(
        cache
            .read_quote_batch(0, 10, NOW)
            .await
            .unwrap()
            .rows
            .is_empty()
    );
}

// ─── Lookup ────────────────────────────────────────────────────────────────

fn john_passage() -> Value {
    json!({ "data": { "id": "JHN.3.16-JHN.3.17", "reference": "John 3:16-17", "copyright": "© Thomas Nelson",
        "content": [
            { "type": "text", "text": "verse 16", "attrs": { "verseId": "JHN.3.16" } },
            { "type": "text", "text": "verse 17", "attrs": { "verseId": "JHN.3.17" } }
        ] } })
}

fn request(end: Option<f64>) -> ApiPassage {
    ApiPassage {
        bible_id: BIBLE_ID.into(),
        translation: "NKJV".into(),
        book_id: 43,
        book_name: "John".into(),
        chapter: 3.0,
        verse_start: 16.0,
        verse_end: end,
    }
}

fn catalog() -> Vec<Value> {
    vec![json!({ "id": BIBLE_ID, "abbreviationLocal": "NKJV" })]
}

async fn source_with(
    mock: Arc<Mock>,
    cache: Option<ApiBibleCache>,
    clock: Clock,
) -> ApiBibleSource {
    let source = ApiBibleSource::new(cache, clock);
    source.set_gateway(Some(("account-1".into(), mock)));
    source
}

#[tokio::test(flavor = "multi_thread")]
async fn a_miss_fetches_stores_and_a_fresh_hit_skips_the_api() {
    let mock = Arc::new(Mock {
        passage: Some(john_passage()),
        ..Mock::new()
    });
    let dir = TempDir::new().unwrap();
    let cache = open_cache(&dir).await;
    let source = source_with(mock.clone(), Some(cache.clone()), fixed(NOW)).await;
    let result = source.lookup_passage(request(Some(17.0))).await.unwrap();
    assert_eq!(result[0].reference, "John 3:16–17");
    assert_eq!(result[0].verses.len(), 2);
    assert!(
        mock.calls()
            .iter()
            .any(|p| p.ends_with("/passages/JHN.3.16-JHN.3.17"))
    );
    let state = cache
        .translation_state(BIBLE_ID, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (state.copyright.as_str(), state.cached_verses),
        ("© Thomas Nelson", 2)
    );

    let calls = mock.calls().len();
    let again = source.lookup_passage(request(Some(17.0))).await.unwrap();
    source.when_idle().await;
    assert_eq!(again[0].verses[1].text, "verse 17");
    // Access was confirmed this session, so no revalidation either.
    assert_eq!(mock.calls().len(), calls);
    assert_eq!(build_passage_id(43, 3.0, 16.0, None), "JHN.3.16");
}

#[tokio::test(flavor = "multi_thread")]
async fn failures_explain_offline_refresh_and_revocation() {
    let dir = TempDir::new().unwrap();
    let cache = open_cache(&dir).await;
    let offline = Arc::new(Mock {
        fail: Some(Box::new(|p, _| {
            p.contains("/passages/")
                .then(|| RequestError::new("offline", None))
        })),
        ..Mock::new()
    });
    let source = source_with(offline, Some(cache.clone()), fixed(NOW)).await;
    assert_eq!(
        source.lookup_passage(request(None)).await.unwrap_err().0,
        msg::NOT_AVAILABLE_OFFLINE
    );

    // Stale text is never served when the refresh fails.
    cache
        .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
        .await
        .unwrap();
    cache
        .put_passage(
            BIBLE_ID,
            passage(3, &[(16.0, "old")], NOW - API_CACHE_MAX_AGE_MS, None),
        )
        .await
        .unwrap();
    assert_eq!(
        source.lookup_passage(request(None)).await.unwrap_err().0,
        msg::REFRESH_REQUIRED
    );

    for code in [401, 403] {
        let refused = Arc::new(Mock {
            fail: Some(Box::new(move |p, _| {
                p.contains("/passages/").then(|| error(code))
            })),
            ..Mock::new()
        });
        let source = source_with(refused.clone(), Some(cache.clone()), fixed(NOW)).await;
        assert_eq!(
            source.lookup_passage(request(None)).await.unwrap_err().0,
            msg::ACCESS_REVOKED
        );
        assert_eq!(status(&cache, NOW).await, ApiBibleCacheStatus::Unavailable);
        // Already unavailable: not requested again.
        let before = refused.calls().len();
        assert_eq!(
            source.lookup_passage(request(None)).await.unwrap_err().0,
            msg::ACCESS_REVOKED
        );
        assert_eq!(refused.calls().len(), before);
        cache
            .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
            .await
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_responses_and_missing_cache_still_work() {
    let mock = Arc::new(Mock::new());
    let source = source_with(mock.clone(), None, fixed(NOW)).await;
    assert!(
        source
            .lookup_passage(request(None))
            .await
            .unwrap()
            .is_empty()
    );
    let mock = Arc::new(Mock {
        passage: Some(john_passage()),
        ..Mock::new()
    });
    let source = source_with(mock, None, fixed(NOW)).await;
    assert_eq!(
        source.lookup_passage(request(Some(17.0))).await.unwrap()[0]
            .verses
            .len(),
        2
    );
    let chapter = ApiChapter {
        bible_id: BIBLE_ID.into(),
        translation: "NKJV".into(),
        book_id: 43,
        book_name: "John".into(),
        chapter: 3.0,
    };
    assert_eq!(
        source.lookup_chapter(chapter).await.unwrap()[0].reference,
        "John 3"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cached_text_revalidates_once_per_session() {
    let dir = TempDir::new().unwrap();
    let cache = open_cache(&dir).await;
    cache
        .upsert_translation(meta(BIBLE_ID, "NKJV", "NKJV"))
        .await
        .unwrap();
    cache
        .put_passage(BIBLE_ID, passage(3, &[(16.0, "cached")], NOW, None))
        .await
        .unwrap();
    cache.mark_access_checked(BIBLE_ID, NOW - 1).await.unwrap();

    // Offline revalidation keeps the text usable.
    let offline = Arc::new(Mock {
        details: Some(Err(RequestError::new("offline", None))),
        ..Mock::new()
    });
    let source = source_with(offline.clone(), Some(cache.clone()), fixed(NOW)).await;
    assert_eq!(
        source.lookup_passage(request(None)).await.unwrap()[0].verses[0].text,
        "cached"
    );
    source.when_idle().await;
    assert_eq!(offline.calls().len(), 1);
    assert_eq!(status(&cache, NOW).await, ApiBibleCacheStatus::Partial);

    // A successful check is not repeated in the same session.
    let ok = Arc::new(Mock::new());
    let source = source_with(ok.clone(), Some(cache.clone()), fixed(NOW)).await;
    source.lookup_passage(request(None)).await.unwrap();
    source.when_idle().await;
    source.lookup_passage(request(None)).await.unwrap();
    source.when_idle().await;
    assert_eq!(ok.calls().len(), 1);

    // A refusal purges the translation.
    let refused = Arc::new(Mock {
        details: Some(Err(error(403))),
        ..Mock::new()
    });
    let source = source_with(refused, Some(cache.clone()), fixed(NOW + 1)).await;
    cache.mark_access_checked(BIBLE_ID, NOW - 1).await.unwrap();
    source.lookup_passage(request(None)).await.unwrap();
    source.when_idle().await;
    assert_eq!(status(&cache, NOW).await, ApiBibleCacheStatus::Unavailable);
    assert_eq!(cache.count_rows(BIBLE_ID).await.unwrap()[1], 0);
}

// ─── Authorization and identity changes (scripture-api-key-change) ─────────

fn keyed(catalogs: &[(&str, Vec<Value>)]) -> Vec<(String, Arc<Mock>)> {
    catalogs
        .iter()
        .map(|(k, bibles)| {
            (
                (*k).to_owned(),
                Arc::new(Mock {
                    bibles: bibles.clone(),
                    ..Mock::new()
                }),
            )
        })
        .collect()
}

fn key_one() -> Vec<Value> {
    vec![
        json!({ "id": "nkjv-under-key-one", "abbreviationLocal": "NKJV" }),
        json!({ "id": "nlt-under-key-one", "abbreviationLocal": "NLT" }),
    ]
}

fn key_two() -> Vec<Value> {
    vec![json!({ "id": "nlt-under-key-two", "abbreviationLocal": "NLT" })]
}

#[tokio::test(flavor = "multi_thread")]
async fn ids_from_a_previous_account_are_never_reused() {
    let keys = keyed(&[("key-one", key_one()), ("key-two", key_two())]);
    let source = ApiBibleSource::new(None, fixed(NOW));
    let set = |i: usize| {
        source.set_gateway(Some((
            keys[i].0.clone(),
            keys[i].1.clone() as Arc<dyn BibleTransport>,
        )))
    };

    set(0);
    assert_eq!(source.resolve("NLT").await.unwrap(), "nlt-under-key-one");
    assert_eq!(source.resolve("NLT").await.unwrap(), "nlt-under-key-one");
    assert_eq!(
        keys[0].1.calls().len(),
        1,
        "current ids resolve without a reload"
    );
    assert!(source.is_authorized_bible_id("nkjv-under-key-one"));

    set(1);
    assert_eq!(source.resolve("NLT").await.unwrap(), "nlt-under-key-two");
    assert_eq!(
        source.resolve("NKJV").await.unwrap_err().0,
        "NKJV is not authorized for this API.Bible key."
    );
    assert_eq!(
        source
            .ensure_authorized_bible_id("nkjv-under-key-one")
            .await
            .unwrap_err()
            .0,
        "That Bible is not authorized for this API.Bible key."
    );
    assert!(!source.is_authorized_bible_id("nkjv-under-key-one"));

    // Signing out clears everything the previous account authorized.
    set(0);
    source.resolve("NKJV").await.unwrap();
    source.set_gateway(None);
    assert!(source.resolve("NKJV").await.is_err());
    assert!(!source.is_authorized_bible_id("nkjv-under-key-one"));
    assert!(
        source
            .list_authorized_translations()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_cache_list_skips_bundled_locals_and_unknown_editions() {
    let mock = Arc::new(Mock {
        bibles: vec![
            json!({ "id": "kjv-bundled-text", "abbreviationLocal": "KJV", "name": "King James Version" }),
            json!({ "id": "niv-text", "abbreviationLocal": "NIV", "name": "New International Version" }),
            json!({ "id": "xyz-text", "abbreviationLocal": "XYZ", "name": "Xyz Simple Translation" }),
        ],
        ..Mock::new()
    });
    let source = source_with(mock, None, fixed(NOW)).await;
    let list = source.list_authorized_translations().await.unwrap();
    let ids: Vec<_> = list.iter().map(|t| t.translation.as_str()).collect();
    assert_eq!(ids, ["NIV"]);
    assert_eq!(list[0].name, "New International Version");
    let authorized = source.authorized_translations().await.unwrap();
    assert!(
        authorized.contains("NIV") && authorized.contains("KJV") && !authorized.contains("XYZ")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ipc_surface_checks_authorization_tracks_usage_and_removes_revoked() {
    let dir = TempDir::new().unwrap();
    let cache = open_cache(&dir).await;
    let mock = Arc::new(Mock {
        bibles: catalog(),
        ..Mock::new()
    });
    let source = source_with(mock.clone(), Some(cache.clone()), fixed(NOW)).await;
    let downloads = Arc::new(AtomicUsize::new(0));
    let counter = downloads.clone();
    let offline = OfflineBibles::new(
        source.clone(),
        Arc::new(|| vec![BIBLE_ID.to_owned()]),
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    offline.start().await.unwrap();

    assert_eq!(
        offline
            .download_translation("someone-elses")
            .await
            .unwrap_err()
            .0,
        "That Bible is not authorized for this API.Bible key."
    );
    offline.download_translation(BIBLE_ID).await.unwrap();
    offline.manager().unwrap().when_idle().await;
    assert_eq!(
        downloads.load(Ordering::SeqCst),
        2,
        "bible_download is tracked per request"
    );
    assert_eq!(status(&cache, NOW).await, ApiBibleCacheStatus::Downloaded);

    // After access is gone, removal still works from what is cached.
    source.set_gateway(None);
    assert!(offline.refresh_offline_translation(BIBLE_ID).await.is_err());
    offline.remove_offline_translation(BIBLE_ID).await.unwrap();
    assert_eq!(
        offline
            .remove_offline_translation(BIBLE_ID)
            .await
            .unwrap_err()
            .0,
        "That Bible has no cached content to remove."
    );

    // Without OS encryption there is no storage, and listing is empty.
    let none = OfflineBibles::new(
        ApiBibleSource::new(None, fixed(NOW)),
        Arc::new(Vec::new),
        Arc::new(|| {}),
    );
    assert!(none.list_offline_translations().await.unwrap().is_empty());
    assert_eq!(
        none.pause_translation_download(BIBLE_ID).unwrap_err().0,
        "Offline Bible storage is unavailable on this machine."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_uses_the_managed_query_shape() {
    let mock = Arc::new(Mock {
        search: Some(
            json!({ "data": { "verses": [{ "id": "PSA.23.1", "orgId": "PSA.23.1", "bookId": "PSA",
            "reference": "Psalm 23:1", "text": "<p class=\"p\">The LORD is my shepherd; I shall not want.</p>" }] } }),
        ),
        ..Mock::new()
    });
    let source = source_with(mock.clone(), None, fixed(NOW)).await;
    let hits = source
        .search(BIBLE_ID, "the lord is my shepherd", 5)
        .await
        .unwrap();
    let (path, query) = mock.calls.lock().unwrap()[0].clone();
    assert_eq!(path, format!("/v1/bibles/{BIBLE_ID}/search"));
    assert_eq!(
        serde_json::to_value(query).unwrap(),
        json!({ "query": "the lord is my shepherd", "limit": 5.0, "sort": "relevance" })
    );
    assert_eq!(
        (hits[0].reference.as_str(), hits[0].chapter, hits[0].verse),
        ("Psalm 23:1", Some(23.0), Some(1.0))
    );
    assert_eq!(hits[0].text, "The LORD is my shepherd; I shall not want.");
}

// ─── Pure rules ────────────────────────────────────────────────────────────

fn summary(
    id: &str,
    abbreviation: Option<&str>,
    local: Option<&str>,
    name: Option<&str>,
    kind: Option<&str>,
) -> ApiBibleSummary {
    ApiBibleSummary {
        id: id.into(),
        abbreviation: abbreviation.map(Into::into),
        abbreviation_local: local.map(Into::into),
        name: name.map(Into::into),
        name_local: name.map(Into::into),
        kind: kind.map(Into::into),
    }
}

fn get(map: &[(String, String)], id: &str) -> Option<String> {
    map.iter().find(|(k, _)| k == id).map(|(_, v)| v.clone())
}

#[test]
fn id_map_matches_the_electron_cases() {
    let m = build_api_bible_id_map(&[
        summary(
            "de4e12af7f28f599-01",
            Some("engKJV"),
            Some("KJV"),
            None,
            None,
        ),
        summary("abcd-01", Some("engNKJV"), Some("NKJV"), None, None),
        summary("web-01", Some("WEB"), None, None, None),
        summary("niv-uk", Some("engNIVUK"), Some("NIVUK"), None, None),
        summary("niv-us", Some("engNIV"), Some("NIV"), None, None),
        summary("nlt-01", Some("engNLT"), None, None, None),
        summary("esv-01", Some("engESV"), None, None, None),
        summary(
            "msg-01",
            Some("engMSG"),
            Some("The Message"),
            Some("The Message"),
            None,
        ),
        summary("amp-01", Some("AMP"), Some("AMP"), None, None),
        summary(
            "nasb-01",
            Some("engNASB2020"),
            Some("NASB2020"),
            Some("New American Standard Bible"),
            None,
        ),
        summary("tpt-01", None, None, Some("The Passion Translation"), None),
        summary("csb-01", None, Some("CSB"), None, None),
        summary(
            "xyz-01",
            None,
            Some("XYZ"),
            Some("Xyz Simple Translation"),
            None,
        ),
    ]);
    for (id, bible) in [
        ("KJV", "de4e12af7f28f599-01"),
        ("NKJV", "abcd-01"),
        ("WEB", "web-01"),
        ("NIV", "niv-us"),
        ("NLT", "nlt-01"),
        ("ESV", "esv-01"),
        ("MSG", "msg-01"),
        ("AMPC", "amp-01"),
        ("NASB", "nasb-01"),
        ("TPT", "tpt-01"),
        ("CSB", "csb-01"),
    ] {
        assert_eq!(get(&m, id).as_deref(), Some(bible), "{id}");
    }
    assert!(get(&m, "ASV").is_none() && get(&m, "XYZ").is_none());

    let audio = build_api_bible_id_map(&[
        summary("niv-audio", None, Some("NIV"), None, Some("audio")),
        summary("niv-text", None, Some("NIV"), None, Some("text")),
    ]);
    assert_eq!(get(&audio, "NIV").as_deref(), Some("niv-text"));
    let official = build_api_bible_id_map(&[summary(
        "78a9f6124f344018-01",
        Some("NIV11"),
        None,
        None,
        None,
    )]);
    assert_eq!(
        get(&official, "NIV").as_deref(),
        Some("78a9f6124f344018-01")
    );
    let named = build_api_bible_id_map(&[summary(
        "engkjv-01",
        None,
        Some("ENGKJV"),
        Some("King James Version"),
        None,
    )]);
    assert_eq!(get(&named, "KJV").as_deref(), Some("engkjv-01"));
    assert!(get(&named, "ENGKJV").is_none());
}

#[test]
fn passage_content_splits_into_verses() {
    let content = json!([{ "name": "para", "type": "tag", "items": [
        { "name": "verse", "type": "tag", "attrs": { "number": "1", "sid": "JOS 1:1" }, "items": [{ "type": "text", "text": "1" }] },
        { "type": "text", "text": " After the death of Moses ", "attrs": { "verseId": "JOS.1.1" } },
        { "name": "verse", "type": "tag", "attrs": { "number": "2", "sid": "JOS 1:2" }, "items": [{ "type": "text", "text": "2" }] },
        { "type": "text", "text": " Moses my servant is dead. ", "attrs": { "verseId": "JOS.1.2" } }
    ] }]);
    let verses = parse_api_bible_passage_content(&content, "Joshua");
    let got: Vec<_> = verses
        .iter()
        .map(|v| (v.chapter, v.verse, v.text.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (1.0, 1.0, "After the death of Moses"),
            (1.0, 2.0, "Moses my servant is dead.")
        ]
    );
    assert!(parse_api_bible_passage_content(&json!({}), "x").is_empty());
}

#[test]
fn search_response_shapes() {
    let flat = parse_api_bible_search_response(
        &json!({ "data": [{ "id": "JHN.3.16", "text": "For God &amp; <b>world</b>" }, { "id": "", "text": "x" }, { "reference": "John 1:1", "text": "" }] }),
    );
    assert_eq!(flat.len(), 1);
    assert_eq!(
        (
            flat[0].reference.as_str(),
            flat[0].book_id.as_deref(),
            flat[0].text.as_str()
        ),
        ("JHN 3:16", Some("JHN"), "For God & world")
    );
    let passages = parse_api_bible_search_response(
        &json!({ "data": { "passages": [{ "reference": "Romans 8:28", "content": "And we know" }] } }),
    );
    assert_eq!(
        (passages[0].chapter, passages[0].verse),
        (Some(8.0), Some(28.0))
    );
    assert!(parse_api_bible_search_response(&json!(null)).is_empty());
}

#[test]
fn freshness_is_exactly_thirty_days() {
    assert_eq!(API_CACHE_MAX_AGE_MS, 30 * 24 * 60 * 60 * 1000);
    assert!(is_fresh(NOW, NOW + API_CACHE_MAX_AGE_MS - 1));
    assert!(!is_fresh(NOW, NOW + API_CACHE_MAX_AGE_MS));
    assert!(!is_fresh(NOW, NOW + API_CACHE_MAX_AGE_MS + 1));
    assert_eq!(api_cache_expiry(NOW), NOW + API_CACHE_MAX_AGE_MS);
}

#[test]
fn offline_download_policy() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    };
    let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        resolve_offline_download_bible_ids(&s(&["nkjv-01"]), env(&[])),
        ["nkjv-01"]
    );
    assert_eq!(
        resolve_offline_download_bible_ids(
            &[],
            env(&[(OFFLINE_BIBLE_IDS_ENV_VAR, "nlt-01, niv-01")])
        ),
        ["nlt-01", "niv-01"]
    );
    assert_eq!(
        resolve_offline_download_bible_ids(
            &s(&["nkjv-01", "nlt-01"]),
            env(&[(OFFLINE_BIBLE_IDS_ENV_VAR, "nlt-01,niv-01")])
        ),
        ["nkjv-01", "nlt-01", "niv-01"]
    );
    assert_eq!(
        resolve_offline_download_bible_ids(
            &[],
            env(&[(OFFLINE_BIBLE_IDS_ENV_VAR, " , ,\tnkjv-01 ,")])
        ),
        ["nkjv-01"]
    );
    assert!(resolve_offline_download_bible_ids(&[], env(&[])).is_empty());
    assert_eq!(
        resolve_offline_download_bible_ids(
            &s(&["kjv-01"]),
            env(&[
                (OFFLINE_BIBLE_IDS_ENV_VAR, "nlt-01"),
                (LEGACY_OFFLINE_BIBLE_IDS_ENV_VAR, "niv-01, nlt-01")
            ])
        ),
        ["kjv-01", "nlt-01", "niv-01"]
    );
    assert!(is_offline_download_permitted(
        "nkjv-01",
        &s(&["nkjv-01"]),
        "© Thomas Nelson"
    ));
    assert!(!is_offline_download_permitted(
        "nkjv-01",
        &[],
        "© Thomas Nelson"
    ));
    assert!(is_offline_download_permitted(
        "kjv-01",
        &[],
        "PUBLIC DOMAIN"
    ));
    assert!(is_offline_download_permitted(
        "kjv-01",
        &[],
        "This work is in the public domain."
    ));
    assert!(!is_offline_download_permitted("unknown-01", &[], ""));
    assert!(!is_offline_download_permitted(
        "unknown-01",
        &[],
        "All rights reserved"
    ));
    assert!(!is_offline_download_permitted("x", &[], "publicdomain"));
}

#[tokio::test]
async fn detection_falls_back_to_the_default_translation_and_says_so() {
    let lookup = |t: String| async move {
        Ok::<_, ApiError>(if t == "KJV" {
            vec![verse(3.0, 16.0, "kjv")]
        } else {
            Vec::new()
        })
    };
    let found = lookup_with_translation_fallback(lookup, "KJV", Some("KJV"), "NKJV")
        .await
        .unwrap();
    assert_eq!(
        (found.translation.as_str(), found.requested_translation),
        ("KJV", None)
    );
    let found = lookup_with_translation_fallback(lookup, "NIV", Some("NIV"), "KJV")
        .await
        .unwrap();
    assert_eq!(
        (
            found.translation.as_str(),
            found.requested_translation.as_deref()
        ),
        ("KJV", Some("NIV"))
    );
    assert_eq!(found.verses.len(), 1);
    let found = lookup_with_translation_fallback(lookup, "NIV", None, "KJV")
        .await
        .unwrap();
    assert!(found.verses.is_empty() && found.requested_translation.is_none());
}
