//! Web search through the managed gateway: DeepSeek, then Brave. From
//! `src/main/services/lyrics/web-search.ts`.
//!
//! Batches are lazy: the caller verifies each and stops on success. Slots are
//! reserved per provider (250 ms DeepSeek, 1.1 s Brave) so concurrent song and
//! gloss lookups cannot exceed the provider's rate.

use super::http::ManagedGateway;
use super::throttle::Throttle;
use kairo_domain::cloud::{
    HttpMethod, ManagedProvider, ManagedPurpose, ManagedRequest, QueryValue,
};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::Duration;

const COUNT: usize = 8;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct WebSearchResult {
    pub title: Option<String>,
    pub url: String,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchProvider {
    Deepseek,
    Brave,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebSearchBatch {
    pub provider: SearchProvider,
    pub results: Vec<WebSearchResult>,
}

static TRACKING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(utm_|__cf_chl_|relatedposts_)").unwrap());
static TRACKING_EXACT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(share|like_comment|_wpnonce|replytocom|amp|noamp|random-post)$").unwrap()
});

/// Drops search-index tracking parameters; refuses credentials and non-HTTP.
pub fn clean_url(value: &Value) -> Option<String> {
    let mut url = url::Url::parse(value.as_str()?).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    let all: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let kept: Vec<(String, String)> = all
        .iter()
        .filter(|(k, _)| !TRACKING.is_match(k) && !TRACKING_EXACT.is_match(k))
        .cloned()
        .collect();
    // Like URLSearchParams.delete, the query is reserialized only when a key goes.
    if kept.len() != all.len() {
        if kept.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(&kept);
        }
    }
    Some(url.to_string())
}

pub fn normalize_results(value: &Value) -> Vec<WebSearchResult> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        if !item.is_object() {
            continue;
        }
        let Some(url) = clean_url(&item["url"]) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        out.push(WebSearchResult {
            url,
            title: item["title"].as_str().map(str::to_owned),
            description: item["snippet"]
                .as_str()
                .or_else(|| item["description"].as_str())
                .map(str::to_owned),
        });
        if out.len() == COUNT {
            break;
        }
    }
    out
}

pub struct WebSearch {
    deepseek: Throttle,
    brave: Throttle,
}

impl Default for WebSearch {
    fn default() -> Self {
        Self {
            deepseek: Throttle::spaced(Duration::from_millis(250)),
            brave: Throttle::spaced(Duration::from_millis(1_100)),
        }
    }
}

impl WebSearch {
    /// Batches in order: DeepSeek for every query, then Brave. DeepSeek skips
    /// queries over 128 characters rather than lose words. The caller stops
    /// pulling once a batch verifies.
    pub fn batches<'a>(
        &'a self,
        gateway: &'a dyn ManagedGateway,
        queries: &[String],
    ) -> SearchCursor<'a> {
        let mut unique: Vec<String> = Vec::new();
        for q in queries
            .iter()
            .map(|q| super::normalize::trim(q).to_owned())
            .filter(|q| !q.is_empty())
        {
            if !unique.contains(&q) {
                unique.push(q);
            }
        }
        SearchCursor {
            search: self,
            gateway,
            queries: unique,
            step: 0,
        }
    }
}

pub struct SearchCursor<'a> {
    search: &'a WebSearch,
    gateway: &'a dyn ManagedGateway,
    queries: Vec<String>,
    step: usize,
}

impl SearchCursor<'_> {
    pub async fn next(&mut self) -> Option<WebSearchBatch> {
        while self.step < self.queries.len() * 2 {
            let provider = if self.step < self.queries.len() {
                SearchProvider::Deepseek
            } else {
                SearchProvider::Brave
            };
            let q = self.queries[self.step % self.queries.len()].clone();
            self.step += 1;
            if provider == SearchProvider::Deepseek && q.chars().count() > 128 {
                continue;
            }
            let request = match provider {
                SearchProvider::Deepseek => {
                    self.search.deepseek.wait().await;
                    ManagedRequest {
                        provider: ManagedProvider::Deepseek,
                        path: "/v1/tools/search".into(),
                        method: HttpMethod::Post,
                        query: None,
                        body: Some(json!({ "query": q, "count": COUNT, "need_content": false })),
                        purpose: Some(ManagedPurpose::WebSearch),
                    }
                }
                SearchProvider::Brave => {
                    self.search.brave.wait().await;
                    let mut query = BTreeMap::new();
                    query.insert("q".to_owned(), QueryValue::Text(q.clone()));
                    query.insert("count".to_owned(), QueryValue::Number(COUNT as f64));
                    ManagedRequest {
                        provider: ManagedProvider::Brave,
                        path: "/res/v1/web/search".into(),
                        method: HttpMethod::Get,
                        query: Some(query),
                        body: None,
                        purpose: Some(ManagedPurpose::WebSearch),
                    }
                }
            };
            match self.gateway.request(request).await {
                Ok(data) => {
                    let raw = match provider {
                        SearchProvider::Deepseek => &data["results"],
                        SearchProvider::Brave => &data["web"]["results"],
                    };
                    let results = normalize_results(raw);
                    if !results.is_empty() {
                        return Some(WebSearchBatch { provider, results });
                    }
                }
                // The error may carry provider detail; only the status is logged.
                Err(error) => {
                    tracing::warn!(?provider, status = ?error.status, "web search provider failed; trying fallback")
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_parameters_and_fragments_go() {
        assert_eq!(
            clean_url(&json!("https://site.com/song/?utm_source=x&id=4&amp=1#top")).as_deref(),
            Some("https://site.com/song/?id=4")
        );
        assert_eq!(clean_url(&json!("https://user:pw@site.com/")), None);
        assert_eq!(clean_url(&json!("ftp://site.com/")), None);
        assert_eq!(clean_url(&json!(4)), None);
    }

    #[test]
    fn results_are_deduplicated_and_capped() {
        let raw = json!([
            { "url": "https://a.com/1", "title": "One", "snippet": "s" },
            { "url": "https://a.com/1#x", "title": "Dup" },
            "junk",
            { "url": "https://a.com/2", "description": "d" },
        ]);
        let results = normalize_results(&raw);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].description.as_deref(), Some("s"));
        assert_eq!(results[1].description.as_deref(), Some("d"));
    }
}
