//! Online lyric search, preview, import and translation (prompt 041).
//!
//! Genius and LRCLIB first, an allowlisted web tier when both miss, AI review
//! of fetched pages and bilingual glosses. No sources beyond Electron's.

pub mod cache;
pub mod genius;
pub mod glosses;
pub mod http;
pub mod lrclib;
pub mod normalize;
pub mod rank;
pub mod resolver;
pub mod review;
pub mod throttle;
pub mod translate;
pub mod web_lyrics;
pub mod web_search;

use cache::OnlineLyricsTextCache;
use http::{BoxFuture, Http, ManagedGateway};
use kairo_domain::lyrics::query::{parse_song_query, query_variants};
use kairo_domain::lyrics::{LyricsOnlineResult, LyricsProvider, LyricsSongSection};
use review::{OnlineLyricsPreparation, ReviewedLyrics};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// A provider hit before ranking. `lyrics` and `popularity` are ranking
/// inputs and never leave this module.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderResult {
    pub id: String,
    pub provider: LyricsProvider,
    pub title: String,
    pub artist: String,
    pub url: String,
    pub album: Option<String>,
    pub release_year: Option<String>,
    pub snippet: Option<String>,
    pub lyrics_available: Option<bool>,
    pub lyrics: Option<String>,
    pub popularity: Option<f64>,
}

impl Default for ProviderResult {
    fn default() -> Self {
        Self {
            id: String::new(),
            provider: LyricsProvider::Web,
            title: String::new(),
            artist: String::new(),
            url: String::new(),
            album: None,
            release_year: None,
            snippet: None,
            lyrics_available: None,
            lyrics: None,
            popularity: None,
        }
    }
}

impl ProviderResult {
    pub fn to_ipc(&self) -> LyricsOnlineResult {
        LyricsOnlineResult {
            lyrics_available: self.lyrics_available,
            id: self.id.clone(),
            provider: self.provider,
            title: self.title.clone(),
            artist: self.artist.clone(),
            url: self.url.clone(),
            album: self.album.clone(),
            release_year: self.release_year.clone(),
            snippet: self.snippet.clone(),
            existing_song_id: None,
        }
    }
}

/// Where preview and import fetch from.
#[derive(Clone, Debug, PartialEq)]
pub struct OnlineSource {
    pub provider: LyricsProvider,
    pub url: String,
    pub title: String,
    pub artist: String,
}

pub struct OnlineLyrics {
    http: Arc<dyn Http>,
    gateway: Arc<dyn ManagedGateway>,
    pub lrclib: Arc<lrclib::Lrclib>,
    pub genius: Arc<genius::Genius>,
    resolver: resolver::SnippetResolver,
    glosses: glosses::GlossFinder,
    text_cache: OnlineLyricsTextCache,
    preparation: OnlineLyricsPreparation,
    /// No queue: obsolete searches cannot build a backlog of paid reviews.
    prefetching: Arc<Mutex<HashSet<String>>>,
}

impl OnlineLyrics {
    pub fn new(http: Arc<dyn Http>, gateway: Arc<dyn ManagedGateway>) -> Arc<Self> {
        let lrclib = Arc::new(lrclib::Lrclib::new(http.clone()));
        let genius = Arc::new(genius::Genius::new(http.clone()));
        let search = Arc::new(web_search::WebSearch::default());
        Arc::new(Self {
            resolver: resolver::SnippetResolver::new(
                http.clone(),
                gateway.clone(),
                genius.clone(),
                lrclib.clone(),
                search.clone(),
            ),
            glosses: glosses::GlossFinder {
                http: http.clone(),
                gateway: gateway.clone(),
                search,
            },
            http,
            gateway,
            lrclib,
            genius,
            text_cache: OnlineLyricsTextCache::default(),
            preparation: OnlineLyricsPreparation::default(),
            prefetching: Arc::default(),
        })
    }

    /// Every catalogue in parallel, then the web tier when they miss; one
    /// ranked list. Fails only when every catalogue call failed.
    pub async fn search_all(&self, query: &str) -> Result<Vec<LyricsOnlineResult>, String> {
        let q = normalize::trim(query);
        if q.encode_utf16().count() < rank::MIN_QUERY_LENGTH {
            return Ok(Vec::new());
        }
        let parsed = parse_song_query(q);
        let variants = query_variants(q);
        let mut calls: Vec<BoxFuture<'_, Result<Vec<ProviderResult>, String>>> = Vec::new();
        for variant in &variants {
            calls.push(Box::pin(self.genius.search(variant)));
            calls.push(Box::pin(self.lrclib.search(variant)));
        }
        for pair in &parsed.pairs {
            calls.push(Box::pin(
                self.lrclib.search_structured(&pair.title, &pair.artist),
            ));
        }
        let total = calls.len();
        let settled = futures_util::future::join_all(calls).await;
        let mut gathered: Vec<ProviderResult> = Vec::new();
        let mut failures: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for outcome in settled {
            match outcome {
                Ok(results) => {
                    for r in results {
                        if seen.insert(r.id.clone()) {
                            gathered.push(r);
                        }
                    }
                }
                Err(e) => failures.push(e),
            }
        }
        if failures.len() == total && total > 0 {
            return Err(failures.swap_remove(0));
        }
        if !failures.is_empty() {
            tracing::warn!(query = q, failures = ?&failures[..failures.len().min(3)], "lyrics provider failed");
        }
        if rank::needs_web_resolution(q, &gathered) {
            gathered.extend(self.resolver.resolve(q).await);
        }
        Ok(rank::rank_results(q, &gathered)
            .into_iter()
            .map(|r| {
                if let Some(lyrics) = r.lyrics.as_deref().filter(|l| !l.is_empty()) {
                    self.text_cache.remember_result(&r, lyrics);
                    // Search-inline lyrics stay available without /api/get.
                    if r.provider == LyricsProvider::Lrclib {
                        self.lrclib.remember_plain_lyrics(&r.url, lyrics);
                    }
                }
                r.to_ipc()
            })
            .collect())
    }

    async fn fetch_text(&self, source: &OnlineSource) -> Result<String, String> {
        let probe = LyricsOnlineResult {
            lyrics_available: None,
            id: String::new(),
            provider: source.provider,
            title: source.title.clone(),
            artist: String::new(),
            url: source.url.clone(),
            album: None,
            release_year: None,
            snippet: None,
            existing_song_id: None,
        };
        if let Some(cached) = self.text_cache.get(&probe) {
            return Ok(cached);
        }
        match source.provider {
            LyricsProvider::Lrclib => self.lrclib.fetch_lyrics(&source.url).await,
            LyricsProvider::Web => Ok(web_lyrics::scrape_lyrics_page(&self.http, &source.url)
                .await?
                .lyrics),
            LyricsProvider::Genius => {
                self.genius
                    .fetch_lyrics(&source.url, Some(&source.title))
                    .await
            }
        }
    }

    /// Fetched and reviewed text; the same snapshot serves preview and import.
    pub async fn prepare(
        self: &Arc<Self>,
        source: &OnlineSource,
        credential: &str,
    ) -> Result<ReviewedLyrics, String> {
        let this = self.clone();
        let owned = source.clone();
        let work: BoxFuture<'static, Result<ReviewedLyrics, String>> = Box::pin(async move {
            let text = this.fetch_text(&owned).await?;
            review::review_online_lyrics(this.gateway.as_ref(), &text, &owned.title, &owned.artist)
                .await
        });
        let provider = source.provider.as_str();
        self.preparation
            .prepare(
                [provider, &source.url, &source.title, &source.artist],
                credential,
                work,
            )
            .await
    }

    /// Warms the first two results not already in the library, at most two
    /// at a time; results never wait for these.
    pub fn warm(self: &Arc<Self>, results: &[LyricsOnlineResult], credential: &str) {
        for result in results
            .iter()
            .filter(|r| r.existing_song_id.is_none())
            .take(2)
        {
            let key =
                serde_json::json!([result.provider.as_str(), result.url, result.title]).to_string();
            {
                let mut active = self.prefetching.lock().unwrap_or_else(|e| e.into_inner());
                if active.contains(&key) || active.len() >= 2 {
                    continue;
                }
                active.insert(key.clone());
            }
            let this = self.clone();
            let source = OnlineSource {
                provider: result.provider,
                url: result.url.clone(),
                title: result.title.clone(),
                artist: result.artist.clone(),
            };
            let credential = credential.to_owned();
            tokio::spawn(async move {
                // A selected preview retries normally.
                let _ = this.prepare(&source, &credential).await;
                this.prefetching
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&key);
            });
        }
    }

    pub async fn translate_sections(
        &self,
        sections: &[LyricsSongSection],
        options: &translate::TranslateOptions,
    ) -> Result<Vec<LyricsSongSection>, String> {
        translate::translate_sections(self.gateway.as_ref(), &self.glosses, sections, options).await
    }
}
