//! The API.Bible half of Electron's `ScriptureService` (authorized ids,
//! `resolveBibleId`, `ensureAuthorizedBibleId`, `getClient`) and the IPC
//! handlers for the offline cache (`offline-bibles.ts`, `ipc/index.ts`).
//!
//! Native has no user API key: every request goes through the licensed Kairo
//! gateway. The gateway's `identity` (the account and church it speaks for)
//! plays the part of Electron's API key. When it changes, ids authorized for
//! the previous identity are dropped, exactly as a key change drops them.

use super::cache::ApiBibleCache;
use super::client::{ApiBibleClient, BibleTransport, RequestError};
use super::downloads::{ApiBibleDownloadManager, AuthorizedTranslation, DownloadDeps, tokio_sleep};
use super::lookup::{ApiBibleLookup, Clock};
use super::parse::build_api_bible_id_map;
use crate::api::{ApiBible, ApiChapter, ApiError, ApiPassage, ApiSearchHit, BoxFuture};
use kairo_domain::scripture::translations::get_translation_definition;
use kairo_domain::scripture::{
    ApiBibleDownloadProgress, ApiBibleOfflineTranslation, ScriptureResult, TranslationAccess,
};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

fn api_error(error: RequestError) -> ApiError {
    ApiError(error.message)
}

const NOT_AUTHORIZED: &str = "That Bible is not authorized for this API.Bible key.";
const NO_GATEWAY: &str = "Sign in to use API.Bible translations.";

#[derive(Default)]
struct State {
    gateway: Option<(String, ApiBibleClient)>,
    lookup: Option<ApiBibleLookup>,
    /// Catalogue id (upper case) to API.Bible id, in API order.
    ids: Vec<(String, String)>,
    names: Vec<(String, String)>,
    /// The identity the ids were loaded for.
    ids_key: Option<String>,
}

struct Inner {
    cache: Option<ApiBibleCache>,
    clock: Clock,
    session_started_at: i64,
    state: Mutex<State>,
}

#[derive(Clone)]
pub struct ApiBibleSource(Arc<Inner>);

impl ApiBibleSource {
    /// `cache` is `None` when OS encryption is unavailable. Online lookups
    /// still work then, and nothing is cached.
    pub fn new(cache: Option<ApiBibleCache>, clock: Clock) -> Self {
        let session_started_at = clock();
        Self(Arc::new(Inner {
            cache,
            clock,
            session_started_at,
            state: Mutex::new(State::default()),
        }))
    }

    pub fn cache(&self) -> Option<&ApiBibleCache> {
        self.0.cache.as_ref()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Set or clear the gateway. A new identity authorizes a different set
    /// of Bibles: dropping the old ids stops a request for a Bible this
    /// account cannot reach, which would come back 403 and purge valid text.
    pub fn set_gateway(&self, gateway: Option<(String, Arc<dyn BibleTransport>)>) {
        let mut state = self.state();
        let same = match (&state.gateway, &gateway) {
            (Some((a, _)), Some((b, _))) => a == b,
            (None, None) => true,
            _ => false,
        };
        state.gateway = gateway.map(|(key, transport)| (key, ApiBibleClient::new(transport)));
        if !same {
            state.ids.clear();
            state.names.clear();
            state.ids_key = None;
            state.lookup = None;
        }
    }

    /// The current client, or `None` with no gateway.
    pub fn client(&self) -> Option<ApiBibleClient> {
        self.state().gateway.as_ref().map(|(_, c)| c.clone())
    }

    fn lookup(&self) -> Result<ApiBibleLookup, ApiError> {
        let mut state = self.state();
        let client = state
            .gateway
            .as_ref()
            .map(|(_, c)| c.clone())
            .ok_or_else(|| ApiError(NO_GATEWAY.into()))?;
        Ok(state
            .lookup
            .get_or_insert_with(|| {
                ApiBibleLookup::new(
                    self.0.cache.clone(),
                    client,
                    self.0.clock.clone(),
                    self.0.session_started_at,
                )
            })
            .clone())
    }

    /// Background revalidation settled (tests, shutdown).
    pub async fn when_idle(&self) {
        let lookup = self.state().lookup.clone();
        if let Some(lookup) = lookup {
            lookup.when_idle().await;
        }
    }

    /// `loadApiBibleIds`.
    async fn load_ids(&self) -> Result<(), ApiError> {
        let (key, client) = {
            let state = self.state();
            match &state.gateway {
                Some((key, client)) => (key.clone(), client.clone()),
                None => {
                    drop(state);
                    self.forget();
                    return Err(ApiError(NO_GATEWAY.into()));
                }
            }
        };
        let bibles = client.list_bibles().await.map_err(api_error)?;
        let ids = build_api_bible_id_map(&bibles);
        let names = ids
            .iter()
            .map(|(catalog, bible_id)| {
                let bible = bibles.iter().find(|b| &b.id == bible_id);
                let name = bible
                    .and_then(|b| b.name_local.clone().or_else(|| b.name.clone()))
                    .or_else(|| get_translation_definition(catalog).map(|d| d.name.to_owned()))
                    .unwrap_or_else(|| catalog.clone());
                (catalog.to_uppercase(), name)
            })
            .collect();
        let mut state = self.state();
        // A gateway swapped mid-request must not adopt the old account's ids.
        if state.gateway.as_ref().map(|(k, _)| k) != Some(&key) {
            return Err(ApiError(NOT_AUTHORIZED.into()));
        }
        tracing::info!(
            count = ids.len(),
            "[Scripture] API.Bible translations authorized"
        );
        state.ids = ids;
        state.names = names;
        state.ids_key = Some(key);
        Ok(())
    }

    fn forget(&self) {
        let mut state = self.state();
        state.ids.clear();
        state.names.clear();
        state.ids_key = None;
    }

    fn current(&self) -> bool {
        let state = self.state();
        state.ids_key.is_some() && state.ids_key.as_ref() == state.gateway.as_ref().map(|(k, _)| k)
    }

    /// True when the id belongs to the list the current identity authorizes.
    pub fn is_authorized_bible_id(&self, bible_id: &str) -> bool {
        self.state().ids.iter().any(|(_, id)| id == bible_id)
    }

    /// Fails unless the id is authorized for the current identity.
    pub async fn ensure_authorized_bible_id(&self, bible_id: &str) -> Result<(), ApiError> {
        if self.current() && self.is_authorized_bible_id(bible_id) {
            return Ok(());
        }
        self.load_ids().await?;
        if self.is_authorized_bible_id(bible_id) {
            Ok(())
        } else {
            Err(ApiError(NOT_AUTHORIZED.into()))
        }
    }

    /// `resolveBibleId`.
    pub async fn resolve(&self, translation: &str) -> Result<String, ApiError> {
        let find = |s: &Self| {
            s.state()
                .ids
                .iter()
                .find(|(c, _)| c == translation)
                .map(|(_, id)| id.clone())
        };
        if !self.current() || find(self).is_none() {
            self.load_ids().await?;
        }
        find(self).ok_or_else(|| {
            ApiError(format!(
                "{translation} is not authorized for this API.Bible key."
            ))
        })
    }

    /// API translations the account reaches, for the offline cache list.
    /// Bundled locals already ship offline and unknown editions stay off.
    pub async fn list_authorized_translations(
        &self,
    ) -> Result<Vec<AuthorizedTranslation>, ApiError> {
        if self.state().gateway.is_none() {
            self.forget();
            return Ok(Vec::new());
        }
        self.load_ids().await?;
        let state = self.state();
        Ok(state
            .ids
            .iter()
            .filter_map(|(id, bible_id)| {
                let definition = get_translation_definition(id)?;
                (definition.access == TranslationAccess::Api).then(|| AuthorizedTranslation {
                    bible_id: bible_id.clone(),
                    translation: id.clone(),
                    name: state
                        .names
                        .iter()
                        .find(|(k, _)| *k == id.to_uppercase())
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| definition.name.to_owned()),
                })
            })
            .collect())
    }
}

impl ApiBible for ApiBibleSource {
    fn authorized_translations(&self) -> BoxFuture<'_, Result<BTreeSet<String>, ApiError>> {
        Box::pin(async move {
            self.load_ids().await?;
            Ok(self
                .state()
                .ids
                .iter()
                .map(|(id, _)| id.to_uppercase())
                .collect())
        })
    }

    fn resolve_bible_id<'a>(
        &'a self,
        translation: &'a str,
    ) -> BoxFuture<'a, Result<String, ApiError>> {
        Box::pin(self.resolve(translation))
    }

    fn search<'a>(
        &'a self,
        bible_id: &'a str,
        query: &'a str,
        limit: u32,
    ) -> BoxFuture<'a, Result<Vec<ApiSearchHit>, ApiError>> {
        Box::pin(async move {
            let client = self.client().ok_or_else(|| ApiError(NO_GATEWAY.into()))?;
            client
                .search(bible_id, query, limit)
                .await
                .map_err(api_error)
        })
    }

    fn lookup_passage(
        &self,
        passage: ApiPassage,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, ApiError>> {
        Box::pin(async move { self.lookup()?.lookup_passage(passage).await })
    }

    fn lookup_chapter(
        &self,
        chapter: ApiChapter,
    ) -> BoxFuture<'_, Result<Vec<ScriptureResult>, ApiError>> {
        Box::pin(async move { self.lookup()?.lookup_chapter(chapter).await })
    }
}

const STORAGE_UNAVAILABLE: &str = "Offline Bible storage is unavailable on this machine.";

/// The offline-cache IPC surface. Every id is checked against what the
/// account authorizes, except removal, which checks the cache so it still
/// works after access is revoked. The UI never sees cache paths or text.
#[derive(Clone)]
pub struct OfflineBibles {
    source: ApiBibleSource,
    manager: Option<ApiBibleDownloadManager>,
    on_download: Arc<dyn Fn() + Send + Sync>,
}

impl OfflineBibles {
    /// `allowlist` returns `resolveOfflineDownloadBibleIds` for the current
    /// settings. `on_download` records the `bible_download` usage event.
    pub fn new(
        source: ApiBibleSource,
        allowlist: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
        on_download: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self::with_options(
            source,
            allowlist,
            on_download,
            tokio_sleep(),
            super::downloads::DEFAULT_CONCURRENCY,
        )
    }

    pub fn with_options(
        source: ApiBibleSource,
        allowlist: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
        on_download: Arc<dyn Fn() + Send + Sync>,
        sleep: super::downloads::Sleep,
        concurrency: usize,
    ) -> Self {
        let manager = source.cache().cloned().map(|cache| {
            let (for_client, for_list) = (source.clone(), source.clone());
            ApiBibleDownloadManager::new(DownloadDeps {
                cache,
                client: Arc::new(move || for_client.client()),
                authorized: Arc::new(move || {
                    let source = for_list.clone();
                    Box::pin(async move { source.list_authorized_translations().await })
                }),
                allowlist,
                clock: source.0.clock.clone(),
                sleep,
                concurrency,
            })
        });
        Self {
            source,
            manager,
            on_download,
        }
    }

    /// Reopen downloads an app exit interrupted as paused.
    pub async fn start(&self) -> Result<(), ApiError> {
        match &self.manager {
            Some(manager) => manager.recover_interrupted_downloads().await,
            None => Ok(()),
        }
    }

    pub fn manager(&self) -> Result<&ApiBibleDownloadManager, ApiError> {
        self.manager
            .as_ref()
            .ok_or_else(|| ApiError(STORAGE_UNAVAILABLE.into()))
    }

    pub fn on_progress(
        &self,
        listener: impl Fn(&ApiBibleDownloadProgress) + Send + Sync + 'static,
    ) {
        if let Some(manager) = &self.manager {
            manager.on_progress(listener);
        }
    }

    /// `scripture:listOfflineTranslations`.
    pub async fn list_offline_translations(
        &self,
    ) -> Result<Vec<ApiBibleOfflineTranslation>, ApiError> {
        match &self.manager {
            Some(manager) => manager.list_offline_translations().await,
            None => Ok(Vec::new()),
        }
    }

    /// `scripture:downloadTranslation`: returns once the download started.
    pub async fn download_translation(&self, bible_id: &str) -> Result<(), ApiError> {
        (self.on_download)();
        self.source.ensure_authorized_bible_id(bible_id).await?;
        self.manager()?.start_download_in_background(bible_id).await
    }

    /// `scripture:pauseTranslationDownload`.
    pub fn pause_translation_download(&self, bible_id: &str) -> Result<(), ApiError> {
        self.manager()?.pause_download(bible_id);
        Ok(())
    }

    /// `scripture:refreshOfflineTranslation`.
    pub async fn refresh_offline_translation(&self, bible_id: &str) -> Result<(), ApiError> {
        self.source.ensure_authorized_bible_id(bible_id).await?;
        self.manager()?.refresh_in_background(bible_id).await
    }

    /// `scripture:removeOfflineTranslation`.
    pub async fn remove_offline_translation(&self, bible_id: &str) -> Result<(), ApiError> {
        let manager = self.manager()?;
        if !manager.has_cached_translation(bible_id).await {
            return Err(ApiError(
                "That Bible has no cached content to remove.".into(),
            ));
        }
        manager.remove_translation(bible_id).await
    }
}
