//! Managed gateway request construction and Audience endpoint handling.

use std::{collections::BTreeMap, fmt};

use kairo_domain::{
    audience::audience_room_error,
    cloud::{HttpMethod, ManagedProvider, ManagedPurpose, ManagedRequest, QueryValue},
};
use serde_json::Value;
use url::Url;

pub const MANAGED_REQUEST_PATH: &str = "/v1/managed/request";

/// A presence marker. It carries no credentials and is never sent to a provider.
pub const MANAGED: &str = "kairo-managed";

/// Construct the gateway envelope. Only LLM calls carry a feature purpose,
/// and the gateway returns a single JSON response for those calls.
pub fn managed_payload(
    provider: ManagedProvider,
    path: impl Into<String>,
    mut body: Option<Value>,
    query: Option<BTreeMap<String, QueryValue>>,
    purpose: Option<ManagedPurpose>,
) -> ManagedRequest {
    let llm = matches!(
        provider,
        ManagedProvider::Anthropic | ManagedProvider::Deepseek
    );
    if llm && let Some(Value::Object(body)) = &mut body {
        body.remove("stream");
    }
    ManagedRequest {
        provider,
        path: path.into(),
        method: if body.is_some() {
            HttpMethod::Post
        } else {
            HttpMethod::Get
        },
        body,
        query,
        purpose: llm.then(|| purpose.unwrap_or(ManagedPurpose::Scripture)),
    }
}

/// Session transport only sends managed requests under the API's managed and
/// licensing prefixes. Reject fragments, foreign URLs and path normalization
/// that could move an apparent managed endpoint to an unrelated API route.
pub fn is_managed_endpoint(path: &str) -> bool {
    let Some(pathname) = endpoint_path(path) else {
        return false;
    };
    ["/v1/licensing", "/v1/managed"].iter().any(|prefix| {
        path == *prefix
            || pathname
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn endpoint_path(path: &str) -> Option<&str> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['\\', '#'])
        || path.chars().any(char::is_control)
    {
        return None;
    }
    let pathname = path.split_once('?').map_or(path, |(path, _)| path);
    // WHATWG URL parsing collapses both literal and percent-encoded dot segments.
    if pathname.split('/').any(|part| {
        let decoded = part.replace("%2e", ".").replace("%2E", ".");
        matches!(decoded.as_str(), "." | "..")
    }) {
        return None;
    }
    Some(pathname)
}

#[derive(Clone, Copy)]
pub enum AudienceRoute<'a> {
    Rooms,
    Engage(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub struct EndpointError;

impl fmt::Display for EndpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Invalid Audience endpoint")
    }
}

impl std::error::Error for EndpointError {}

/// Build the absolute Audience URL. An Engage suffix is always a path on that
/// service, even when a caller supplies an unexpected string.
pub fn audience_url(base: &str, route: AudienceRoute<'_>) -> Result<String, EndpointError> {
    let url = Url::parse(base).map_err(|_| EndpointError)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(EndpointError);
    }
    let base = base.trim_end_matches('/');
    match route {
        AudienceRoute::Rooms => Ok(format!("{base}/v1/audience/rooms")),
        AudienceRoute::Engage(path) if path.is_empty() || endpoint_path(path).is_some() => {
            Ok(format!("{base}/v1/engage{path}"))
        }
        AudienceRoute::Engage(_) => Err(EndpointError),
    }
}

pub fn audience_error(
    message: &str,
    status: Option<u16>,
    base: &str,
    route: AudienceRoute<'_>,
) -> String {
    let local = Url::parse(base).is_ok_and(|url| match url.host_str() {
        Some("localhost" | "127.0.0.1") => true,
        Some("[::1]") => matches!(route, AudienceRoute::Rooms),
        _ => false,
    });
    audience_room_error(message, Some(status), local).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_envelopes_match_gateway_shape() {
        let llm = managed_payload(
            ManagedProvider::Anthropic,
            "/v1/messages",
            Some(json!({"model": "claude-haiku-4-5", "stream": true})),
            None,
            None,
        );
        assert_eq!(
            serde_json::to_value(llm).unwrap(),
            json!({
                "provider": "anthropic",
                "path": "/v1/messages",
                "method": "POST",
                "body": {"model": "claude-haiku-4-5"},
                "purpose": "scripture"
            })
        );
        let bible = managed_payload(
            ManagedProvider::Bible,
            "/v1/bibles",
            None,
            Some(BTreeMap::from([(
                "language".into(),
                QueryValue::Text("eng".into()),
            )])),
            Some(ManagedPurpose::Summaries),
        );
        assert_eq!(
            serde_json::to_value(bible).unwrap(),
            json!({"provider": "bible", "path": "/v1/bibles", "method": "GET", "query": {"language": "eng"}})
        );
        let deepseek = managed_payload(
            ManagedProvider::Deepseek,
            "/chat/completions",
            Some(Value::Null),
            None,
            Some(ManagedPurpose::Translation),
        );
        assert_eq!(deepseek.method, HttpMethod::Post);
        assert_eq!(deepseek.body, Some(Value::Null));
        assert_eq!(deepseek.purpose, Some(ManagedPurpose::Translation));
    }

    #[test]
    fn managed_endpoints_cannot_escape_the_gateway() {
        assert!(is_managed_endpoint(MANAGED_REQUEST_PATH));
        assert!(is_managed_endpoint("/v1/licensing"));
        assert!(is_managed_endpoint("/v1/licensing/refresh?source=desktop"));
        for path in [
            "https://example.test/v1/managed/request",
            "/v1/management",
            "/v1/managed/../../v1/auth/session",
            "/v1/managed/%2E./auth",
            "/v1/managed\\..\\auth",
            "/v1/managed#ignored",
        ] {
            assert!(!is_managed_endpoint(path), "{path}");
        }
    }

    #[test]
    fn audience_urls_and_errors_preserve_service_specific_behavior() {
        let engage = AudienceRoute::Engage("/ROOM/resume");
        assert_eq!(
            audience_url("http://localhost:8787/", engage).unwrap(),
            "http://localhost:8787/v1/engage/ROOM/resume"
        );
        assert!(audience_url("http://localhost:8787", AudienceRoute::Engage("/../auth")).is_err());
        assert!(audience_url("http://localhost:8787?token=test", engage).is_err());
        assert_eq!(
            audience_error("Rejected", Some(401), "https://audience.example", engage),
            "Sign in to Kairo to start a Q&A."
        );
        assert!(
            audience_error("Offline", None, "http://localhost:8787", engage)
                .contains("local Audience service")
        );
        assert!(
            audience_error("Missing", Some(404), "https://audience.example", engage)
                .contains("update the Kairo server")
        );
        assert!(
            audience_error(
                "Missing",
                Some(404),
                "http://[::1]:8787",
                AudienceRoute::Rooms
            )
            .contains("local server")
        );
        assert_eq!(
            audience_error(
                "Error: Room is closed.",
                Some(409),
                "https://audience.example",
                engage
            ),
            "Room is closed."
        );
    }
}
