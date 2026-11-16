//! Outbound HTTP for the online lyric sources, and the managed gateway.
//!
//! Both sit behind traits so the sources run against recorded fixtures in
//! tests. The production client is `reqwest` with rustls; the gateway is
//! implemented by `kairo-cloud`'s licensing service.

use kairo_domain::cloud::ManagedRequest;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub url: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            url: url.into(),
            query: Vec::new(),
            headers: Vec::new(),
            timeout,
        }
    }

    pub fn query(mut self, key: &str, value: impl Into<String>) -> Self {
        self.query.push((key.into(), value.into()));
        self
    }

    pub fn header(mut self, key: &str, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// The full URL with the query string, for logs and fakes.
    pub fn full_url(&self) -> String {
        match url::Url::parse_with_params(&self.url, &self.query) {
            Ok(u) if !self.query.is_empty() => u.to_string(),
            _ => self.url.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// How a request failed, in the shapes the Electron code tells apart
/// (`ECONNABORTED`, `ENOTFOUND`/`EAI_AGAIN`, an HTTP status).
#[derive(Clone, Debug, PartialEq)]
pub enum HttpError {
    Timeout,
    Offline,
    Status { status: u16, body: String },
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => f.write_str("timeout exceeded"),
            Self::Offline => f.write_str("getaddrinfo ENOTFOUND"),
            Self::Status { status, .. } => write!(f, "Request failed with status code {status}"),
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl HttpError {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// GET returning the body. `accept` decides which statuses count as success
/// (Axios' `validateStatus`; by default 2xx).
pub trait Http: Send + Sync {
    fn get(
        &self,
        request: HttpRequest,
        accept: fn(u16) -> bool,
    ) -> BoxFuture<'_, Result<HttpResponse, HttpError>>;
}

pub fn success(status: u16) -> bool {
    (200..300).contains(&status)
}

/// The production client.
#[derive(Clone)]
/// The client is built on first use, not at construction: building it loads
/// the system root certificates, which took over 100 ms on the UI thread
/// during launch on macOS (prompt 107).
#[derive(Default)]
pub struct ReqwestHttp {
    client: std::sync::OnceLock<reqwest::Client>,
}

impl ReqwestHttp {
    fn client(&self) -> &reqwest::Client {
        self.client.get_or_init(|| {
            kairo_net::transport::client_builder()
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()
                .unwrap_or_default()
        })
    }
}

impl Http for ReqwestHttp {
    fn get(
        &self,
        request: HttpRequest,
        accept: fn(u16) -> bool,
    ) -> BoxFuture<'_, Result<HttpResponse, HttpError>> {
        Box::pin(async move {
            let mut builder = self
                .client()
                .get(&request.url)
                .query(&request.query)
                .timeout(request.timeout);
            for (k, v) in &request.headers {
                builder = builder.header(k, v);
            }
            let response = builder.send().await.map_err(classify)?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(classify)?;
            if accept(status) {
                Ok(HttpResponse { status, body })
            } else {
                Err(HttpError::Status { status, body })
            }
        })
    }
}

fn classify(error: reqwest::Error) -> HttpError {
    use kairo_net::transport::TransportError;
    match TransportError::from(&error) {
        TransportError::Timeout(_) => HttpError::Timeout,
        TransportError::Offline(_) => HttpError::Offline,
        TransportError::Status { .. } | TransportError::Other(_) => {
            HttpError::Other(error.to_string())
        }
    }
}

/// A managed gateway failure: the server's status (when it answered) and words.
#[derive(Clone, Debug, PartialEq)]
pub struct GatewayError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// `managedJson`: a provider call through Kairo's server.
pub trait ManagedGateway: Send + Sync {
    fn request(&self, request: ManagedRequest) -> BoxFuture<'_, Result<Value, GatewayError>>;
}
