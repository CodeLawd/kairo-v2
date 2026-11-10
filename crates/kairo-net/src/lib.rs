//! HTTP transport for the Kairo cloud API.

pub mod transport;

use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

pub use reqwest::Method;
use reqwest::{Client, Url, header, redirect::Policy};
use serde::de::DeserializeOwned;
use serde_json::Value;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
const OFFLINE: &str = "Could not reach Kairo. Check your internet connection.";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    pub status: Option<u16>,
    pub message: String,
}

impl ApiError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            status: None,
            message: message.into(),
        }
    }

    fn http(status: u16, body: &[u8]) -> Self {
        let json = serde_json::from_slice::<Value>(body).ok();
        let message = json
            .as_ref()
            .and_then(|json| json.get("message"))
            .and_then(|msg| {
                msg.as_array()
                    .and_then(|msgs| msgs.first())
                    .unwrap_or(msg)
                    .as_str()
            })
            .filter(|msg| !msg.is_empty())
            .unwrap_or(match status {
                401 => "Please sign in again.",
                403 => "You do not have access to that.",
                500..=599 => "Kairo is having trouble. Try again shortly.",
                _ => "Something went wrong.",
            });
        Self {
            status: Some(status),
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct RequestOptions {
    pub authenticated: bool,
    pub allow_refresh: bool,
    pub timeout: Option<Duration>,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            authenticated: true,
            allow_refresh: true,
            timeout: None,
        }
    }
}

#[derive(Default, Clone)]
struct Credential {
    epoch: u64,
    token: Option<String>,
}

struct Inner {
    http: Client,
    primary: Url,
    backup: Option<Url>,
    selected: tokio::sync::Mutex<Option<Url>>,
    credential: Mutex<Credential>,
    refreshing: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct CloudApiClient(Arc<Inner>);

impl CloudApiClient {
    pub fn new(primary: &str, backup: Option<&str>) -> Result<Self, ApiError> {
        fn parse(base: &str) -> Result<Url, ApiError> {
            Url::parse(base)
                .ok()
                .filter(|url| {
                    matches!(url.scheme(), "http" | "https")
                        && url.host().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.query().is_none()
                        && url.fragment().is_none()
                })
                .ok_or_else(|| ApiError::new("Invalid Kairo API address."))
        }

        let http = transport::client_builder()
            .timeout(REQUEST_TIMEOUT)
            // A redirected POST can be replayed by the HTTP stack. API routes
            // have no redirects, and credentials must stay on configured hosts.
            .redirect(Policy::none())
            .default_headers(header::HeaderMap::from_iter([
                (
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("application/json"),
                ),
                (
                    header::HeaderName::from_static("x-pa-client"),
                    header::HeaderValue::from_static("desktop"),
                ),
            ]))
            .build()
            .map_err(|_| ApiError::new("Could not initialize Kairo networking."))?;
        Ok(Self(Arc::new(Inner {
            http,
            primary: parse(primary)?,
            backup: backup.map(parse).transpose()?,
            selected: tokio::sync::Mutex::new(None),
            credential: Mutex::default(),
            refreshing: tokio::sync::Mutex::new(()),
        })))
    }

    /// Replace the account and invalidate pending retries for the old account.
    pub fn set_access_token(&self, token: Option<String>) {
        let mut credential = self.0.credential.lock().expect("credential lock");
        credential.epoch = credential.epoch.wrapping_add(1);
        credential.token = token;
    }

    /// Replace a rotated token without invalidating requests for this account.
    pub fn rotate_access_token(&self, token: String) {
        self.0.credential.lock().expect("credential lock").token = Some(token);
    }

    pub fn access_token(&self) -> Option<String> {
        self.credential().token
    }

    fn credential(&self) -> Credential {
        self.0.credential.lock().expect("credential lock").clone()
    }

    pub async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        options: RequestOptions,
    ) -> Result<T, ApiError> {
        self.request_with_token(method, path, body, options, self.access_token().as_deref())
            .await
    }

    pub async fn request_with_token<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        options: RequestOptions,
        token: Option<&str>,
    ) -> Result<T, ApiError> {
        self.send(&method, path, body.as_ref(), options, token)
            .await
    }

    pub async fn request_with_refresh<T, F, Fut>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        options: RequestOptions,
        refresh: F,
    ) -> Result<T, ApiError>
    where
        T: DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        let mut credential = self.credential();
        let mut refresh = (options.authenticated && options.allow_refresh).then_some(refresh);
        if credential.token.is_none()
            && let Some(run) = refresh.take()
        {
            credential.token = self.refresh(&credential, run).await;
        }
        let result = self
            .send(
                &method,
                path,
                body.as_ref(),
                options,
                credential.token.as_deref(),
            )
            .await;
        if result
            .as_ref()
            .is_err_and(|error| error.status == Some(401))
            && let Some(run) = refresh
            && let Some(token) = self.refresh(&credential, run).await
        {
            return self
                .send(&method, path, body.as_ref(), options, Some(&token))
                .await;
        }
        result
    }

    async fn refresh<F, Fut>(&self, observed: &Credential, run: F) -> Option<String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Option<String>>,
    {
        let (_guard, joined) = match self.0.refreshing.try_lock() {
            Ok(guard) => (guard, false),
            Err(_) => (self.0.refreshing.lock().await, true),
        };
        let current = self.credential();
        if current.epoch != observed.epoch {
            return None;
        }
        if current.token != observed.token {
            return current.token;
        }
        if joined {
            return None;
        }
        let token = run().await?;
        let mut current = self.0.credential.lock().expect("credential lock");
        if current.epoch != observed.epoch {
            return None;
        }
        current.token = Some(token.clone());
        Some(token)
    }

    async fn endpoint(&self) -> Url {
        let Some(backup) = &self.0.backup else {
            return self.0.primary.clone();
        };
        let mut selected = self.0.selected.lock().await;
        if let Some(url) = &*selected {
            return url.clone();
        }
        for candidate in [&self.0.primary, backup] {
            if let Ok(url) = candidate.join("/v1/health")
                && let Ok(response) = self.0.http.get(url).timeout(HEALTH_TIMEOUT).send().await
                && response.status().is_success()
            {
                *selected = Some(candidate.clone());
                return candidate.clone();
            }
        }
        self.0.primary.clone()
    }

    fn destination(&self, endpoint: &Url, path: &str) -> Result<Url, ApiError> {
        if let Ok(absolute) = Url::parse(path) {
            if absolute.origin() == self.0.primary.origin()
                && absolute.username().is_empty()
                && absolute.password().is_none()
            {
                let mut url = endpoint.clone();
                url.set_path(absolute.path());
                url.set_query(absolute.query());
                return Ok(url);
            }
        } else if path.starts_with('/')
            && !path.starts_with("//")
            && !path.contains('\\')
            && let Ok(url) = endpoint.join(path)
            && url.origin() == endpoint.origin()
        {
            return Ok(url);
        }
        Err(ApiError::new("Unsupported Kairo endpoint."))
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: &Method,
        path: &str,
        body: Option<&Value>,
        options: RequestOptions,
        token: Option<&str>,
    ) -> Result<T, ApiError> {
        let endpoint = self.endpoint().await;
        let result = self
            .send_to(&endpoint, method, path, body, options, token)
            .await;
        if matches!(&result, Err(Failure::Network)) && self.0.backup.is_some() {
            {
                let mut selected = self.0.selected.lock().await;
                if selected.as_ref() == Some(&endpoint) {
                    *selected = None;
                }
            }
            if method == Method::GET {
                let retry = self.endpoint().await;
                if retry != endpoint {
                    return self
                        .send_to(&retry, method, path, body, options, token)
                        .await
                        .map_err(Failure::api);
                }
            }
        }
        result.map_err(Failure::api)
    }

    async fn send_to<T: DeserializeOwned>(
        &self,
        endpoint: &Url,
        method: &Method,
        path: &str,
        body: Option<&Value>,
        options: RequestOptions,
        token: Option<&str>,
    ) -> Result<T, Failure> {
        let url = self.destination(endpoint, path).map_err(Failure::Api)?;
        let mut request = self
            .0
            .http
            .request(method.clone(), url)
            .timeout(options.timeout.unwrap_or(REQUEST_TIMEOUT));
        if options.authenticated
            && let Some(token) = token
        {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|_| Failure::Network)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|_| Failure::Network)?;
        if !status.is_success() {
            return Err(Failure::Api(ApiError::http(status.as_u16(), &bytes)));
        }
        let payload = if bytes.is_empty() {
            b"null".as_slice()
        } else {
            &bytes
        };
        serde_json::from_slice(payload)
            .map_err(|_| Failure::Api(ApiError::new("Kairo returned an invalid response.")))
    }
}

enum Failure {
    Network,
    Api(ApiError),
}

impl Failure {
    fn api(self) -> ApiError {
        match self {
            Self::Network => ApiError::new(OFFLINE),
            Self::Api(error) => error,
        }
    }
}
