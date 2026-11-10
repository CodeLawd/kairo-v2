//! `api-bible-client.ts`: every API.Bible call goes through one transport.

use super::parse::{
    ApiBibleSummary, ParsedApiBibleVerse, encode_uri_component, parse_api_bible_passage_content,
    parse_api_bible_search_response,
};
use crate::api::{ApiSearchHit, BoxFuture};
use kairo_domain::cloud::QueryValue;
use kairo_domain::js::value_to_string;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// A normalized API.Bible failure, so callers branch on status alone.
#[derive(Clone, Debug, PartialEq)]
pub struct RequestError {
    pub message: String,
    pub status: Option<u16>,
    pub retry_after_ms: Option<f64>,
}

impl RequestError {
    pub fn new(message: impl Into<String>, status: Option<u16>) -> Self {
        Self {
            message: message.into(),
            status,
            retry_after_ms: None,
        }
    }

    /// The account no longer grants this translation.
    pub fn is_access_error(&self) -> bool {
        matches!(self.status, Some(401 | 403))
    }

    pub fn is_rate_limited(&self) -> bool {
        self.status == Some(429)
    }
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RequestError {}

pub type Query = BTreeMap<String, QueryValue>;

/// The managed gateway for API.Bible. `path` starts with `/v1/bibles`.
/// Production uses the licensed Kairo gateway (`kairo-cloud`); tests use a
/// mock. A `Retry-After` header, when the transport sees one, belongs in
/// `retry_after_ms`.
pub trait BibleTransport: Send + Sync {
    fn get(&self, path: String, query: Option<Query>)
    -> BoxFuture<'_, Result<Value, RequestError>>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApiBibleDetails {
    pub id: String,
    pub name: String,
    pub abbreviation: Option<String>,
    pub abbreviation_local: Option<String>,
    pub copyright: String,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiBibleBook {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiBibleChapterSummary {
    pub id: String,
    pub number: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApiBiblePassage {
    pub bible_id: String,
    pub passage_id: String,
    pub reference: String,
    pub copyright: String,
    pub verses: Vec<ParsedApiBibleVerse>,
}

#[derive(Clone)]
pub struct ApiBibleClient(Arc<dyn BibleTransport>);

fn data(value: &Value) -> Option<&Value> {
    value.get("data").filter(|v| !v.is_null())
}

fn rows(value: &Value) -> Vec<Value> {
    data(value)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .filter(|v| !v.is_null())
        .map(value_to_string)
        .unwrap_or_default()
}

fn content_params() -> Query {
    let mut q = Query::new();
    q.insert("content-type".into(), QueryValue::Text("json".into()));
    for key in ["include-notes", "include-titles", "include-chapter-numbers"] {
        q.insert(key.into(), QueryValue::Flag(false));
    }
    q
}

impl ApiBibleClient {
    pub fn new(transport: Arc<dyn BibleTransport>) -> Self {
        Self(transport)
    }

    async fn get(&self, pathname: String, query: Option<Query>) -> Result<Value, RequestError> {
        self.0.get(format!("/v1{pathname}"), query).await
    }

    pub async fn list_bibles(&self) -> Result<Vec<ApiBibleSummary>, RequestError> {
        let mut q = Query::new();
        q.insert("language".into(), QueryValue::Text("eng".into()));
        let value = self.get("/bibles".into(), Some(q)).await?;
        Ok(rows(&value)
            .iter()
            .filter_map(ApiBibleSummary::from_value)
            .collect())
    }

    pub async fn get_bible(&self, bible_id: &str) -> Result<ApiBibleDetails, RequestError> {
        let value = self
            .get(format!("/bibles/{}", encode_uri_component(bible_id)), None)
            .await?;
        let d = data(&value).cloned().unwrap_or(Value::Null);
        let opt = |k: &str| d.get(k).and_then(Value::as_str).map(str::to_owned);
        let id = text(&d, "id");
        Ok(ApiBibleDetails {
            id: if d.get("id").is_some_and(|v| !v.is_null()) {
                id
            } else {
                bible_id.to_owned()
            },
            name: text(&d, "name"),
            abbreviation: opt("abbreviation"),
            abbreviation_local: opt("abbreviationLocal"),
            copyright: text(&d, "copyright"),
            language: d
                .get("language")
                .and_then(|l| l.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    pub async fn list_books(&self, bible_id: &str) -> Result<Vec<ApiBibleBook>, RequestError> {
        let value = self
            .get(
                format!("/bibles/{}/books", encode_uri_component(bible_id)),
                None,
            )
            .await?;
        Ok(rows(&value)
            .iter()
            .map(|row| ApiBibleBook {
                id: text(row, "id"),
                name: text(row, "name"),
            })
            .collect())
    }

    pub async fn list_chapters(
        &self,
        bible_id: &str,
        book_id: &str,
    ) -> Result<Vec<ApiBibleChapterSummary>, RequestError> {
        let value = self
            .get(
                format!(
                    "/bibles/{}/books/{}/chapters",
                    encode_uri_component(bible_id),
                    encode_uri_component(book_id)
                ),
                None,
            )
            .await?;
        Ok(rows(&value)
            .iter()
            .map(|row| ApiBibleChapterSummary {
                id: text(row, "id"),
                number: text(row, "number"),
            })
            .collect())
    }

    pub async fn get_passage(
        &self,
        bible_id: &str,
        passage_id: &str,
        book_name: &str,
    ) -> Result<ApiBiblePassage, RequestError> {
        self.fetch_content(bible_id, "passages", passage_id, book_name)
            .await
    }

    pub async fn get_chapter(
        &self,
        bible_id: &str,
        chapter_id: &str,
        book_name: &str,
    ) -> Result<ApiBiblePassage, RequestError> {
        self.fetch_content(bible_id, "chapters", chapter_id, book_name)
            .await
    }

    /// Keyword or remembered-phrase search inside one authorized Bible.
    pub async fn search(
        &self,
        bible_id: &str,
        query: &str,
        limit: u32,
    ) -> Result<Vec<ApiSearchHit>, RequestError> {
        let mut q = Query::new();
        q.insert("query".into(), QueryValue::Text(query.into()));
        q.insert("limit".into(), QueryValue::Number(limit.into()));
        q.insert("sort".into(), QueryValue::Text("relevance".into()));
        let value = self
            .get(
                format!("/bibles/{}/search", encode_uri_component(bible_id)),
                Some(q),
            )
            .await?;
        Ok(parse_api_bible_search_response(&value))
    }

    async fn fetch_content(
        &self,
        bible_id: &str,
        resource: &str,
        id: &str,
        book_name: &str,
    ) -> Result<ApiBiblePassage, RequestError> {
        let value = self
            .get(
                format!("/bibles/{}/{resource}/{id}", encode_uri_component(bible_id)),
                Some(content_params()),
            )
            .await?;
        let d = data(&value).cloned().unwrap_or(Value::Null);
        Ok(ApiBiblePassage {
            bible_id: bible_id.to_owned(),
            passage_id: if d.get("id").is_some_and(|v| !v.is_null()) {
                text(&d, "id")
            } else {
                id.to_owned()
            },
            reference: text(&d, "reference"),
            copyright: text(&d, "copyright"),
            verses: parse_api_bible_passage_content(
                d.get("content").unwrap_or(&Value::Null),
                book_name,
            ),
        })
    }
}
