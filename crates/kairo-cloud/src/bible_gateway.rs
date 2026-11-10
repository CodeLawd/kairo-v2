//! API.Bible through the licensed Kairo gateway (`managedJson('bible', …)`).
//! The `scripture` licence feature gates every request, and the server holds
//! the provider key.

use crate::licensing::LicensingService;
use kairo_domain::cloud::{HttpMethod, ManagedProvider, ManagedRequest};
use kairo_scripture::api::BoxFuture;
use kairo_scripture::api_bible::ApiBibleSource;
use kairo_scripture::api_bible::client::{BibleTransport, Query, RequestError};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct BibleGateway(LicensingService);

impl BibleGateway {
    pub fn new(licensing: LicensingService) -> Self {
        Self(licensing)
    }

    /// Point `source` at the current account, or detach it when signed out.
    /// Call at startup and on every session change.
    pub async fn sync(&self, source: &ApiBibleSource) {
        let gateway = self
            .0
            .account_key()
            .await
            .map(|key| (key, Arc::new(self.clone()) as Arc<dyn BibleTransport>));
        source.set_gateway(gateway);
    }
}

/// Dedicated providers carry no `purpose`; only LLM requests do.
pub fn bible_request(path: String, query: Option<Query>) -> ManagedRequest {
    ManagedRequest {
        provider: ManagedProvider::Bible,
        path,
        method: HttpMethod::Get,
        query,
        body: None,
        purpose: None,
    }
}

impl BibleTransport for BibleGateway {
    fn get(
        &self,
        path: String,
        query: Option<Query>,
    ) -> BoxFuture<'_, Result<Value, RequestError>> {
        Box::pin(async move {
            self.0
                .request::<Value>(&bible_request(path, query))
                .await
                .map_err(|error| RequestError::new(error.message, error.status))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::cloud::QueryValue;

    #[test]
    fn bible_requests_match_managed_payload() {
        let mut query = Query::new();
        query.insert("query".into(), QueryValue::Text("love is patient".into()));
        query.insert("limit".into(), QueryValue::Number(8.0));
        query.insert("sort".into(), QueryValue::Text("relevance".into()));
        let request = bible_request("/v1/bibles/nkjv-01/search".into(), Some(query));
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({ "provider": "bible", "path": "/v1/bibles/nkjv-01/search", "method": "GET",
                "query": { "query": "love is patient", "limit": 8.0, "sort": "relevance" } })
        );
        let plain = serde_json::to_value(bible_request("/v1/bibles".into(), None)).unwrap();
        assert!(plain.get("query").is_none() && plain.get("purpose").is_none());
    }
}
