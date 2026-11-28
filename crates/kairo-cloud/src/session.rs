use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Weak},
    time::Duration,
};

use kairo_core::{PowerEvent, Publisher};
use kairo_domain::{
    audience::audience_api_base_url,
    cloud::{
        AuthResultPayload, CloudOrg, CloudResult, CloudSessionState, CloudUser, CrashUpload,
        CrashUploadResult, DevicePairingState, DevicePairingStatus, DevicePollOutcome,
        DevicePollPayload, DeviceStartPayload, FeedbackKind, FeedbackUpload, FeedbackUploadResult,
        ManagedRequest, RefreshPayload, SermonUploadInput, SermonUploadResult, SessionSnapshot,
        SignInInput, SignUpInput, next_poll_delay_sec, safe_return_path,
    },
    usage::{UsageReport, UsageReportResult},
};
use kairo_net::{ApiError, CloudApiClient, Method, RequestOptions};
use kairo_store::{build_config, secrets::KeyProtector, settings::now_ms};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, watch},
    task::AbortHandle,
    time::{Instant, sleep, sleep_until, timeout_at},
};
use url::Url;
use zeroize::Zeroizing;

use crate::{
    managed::{
        AudienceRoute, MANAGED_REQUEST_PATH, audience_error, audience_url, is_managed_endpoint,
    },
    profile::{Device, Profile, Settings},
};

/// Waits after each failed refresh while offline.
const RETRY: kairo_domain::time::Backoff = kairo_domain::time::Backoff::Steps(&[
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(300),
]);
const COLD_REQUEST: Duration = Duration::from_secs(90);
const ANONYMOUS: RequestOptions = RequestOptions {
    authenticated: false,
    allow_refresh: false,
    timeout: None,
};

pub struct CloudConfig {
    api: String,
    web: Url,
    audience: String,
    fallback: Option<String>,
    system_browser: bool,
}

impl CloudConfig {
    pub fn production() -> Result<Self, ApiError> {
        let mut config = Self::new(
            build_config::API_URL,
            build_config::WEB_URL,
            build_config::AUDIENCE_API_URL_OVERRIDE,
        )?;
        config.system_browser = true;
        Ok(config)
    }

    pub fn new(api: &str, web: &str, audience: Option<&str>) -> Result<Self, ApiError> {
        let web = browser_url(web).ok_or_else(|| ApiError::new("Invalid Kairo web address."))?;
        Ok(Self {
            api: api.into(),
            web,
            audience: audience_api_base_url(api, audience),
            fallback: build_config::api_fallback_for(api).map(str::to_owned),
            system_browser: false,
        })
    }
}

fn browser_url(raw: &str) -> Option<Url> {
    Url::parse(raw).ok().filter(|url| {
        matches!(url.scheme(), "https" | "http")
            && url.host().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

#[derive(Clone, Debug)]
pub struct ChurchNameSeeded;

/// The shell subscribes to these ordered events and opens the system browser.
#[derive(Clone, Debug)]
pub struct OpenBrowser(pub Url);

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OrgProfilePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_times: Option<Vec<kairo_domain::settings::ChurchServiceTime>>,
}

fn signed_out(persisted: bool) -> SessionSnapshot {
    SessionSnapshot {
        state: CloudSessionState::SignedOut,
        user: None,
        org: None,
        orgs: vec![],
        last_synced_at: None,
        credentials_persisted: Some(persisted),
    }
}

fn pairing(status: DevicePairingStatus, message: Option<String>) -> DevicePairingState {
    DevicePairingState {
        status,
        user_code: None,
        verification_uri: None,
        expires_at: None,
        message,
    }
}

struct State {
    session: SessionSnapshot,
    pairing: DevicePairingState,
    token: Option<Zeroizing<String>>,
    epoch: u64,
    intent: u64,
    pairing_generation: u64,
    refresh: Option<watch::Receiver<Option<Option<String>>>>,
    retry_at: Option<Instant>,
    failures: usize,
    retry: Option<AbortHandle>,
    periodic: Option<AbortHandle>,
    poll: Option<AbortHandle>,
    running: bool,
}

fn abort(task: &mut Option<AbortHandle>) {
    if let Some(task) = task.take() {
        task.abort();
    }
}

impl State {
    fn reset_retry(&mut self) {
        abort(&mut self.retry);
        self.retry_at = None;
        self.failures = 0;
    }

    fn cancel_pairing(&mut self) {
        self.pairing_generation = self.pairing_generation.wrapping_add(1);
        abort(&mut self.poll);
        self.pairing = pairing(DevicePairingStatus::Idle, None);
    }
}

impl Drop for State {
    fn drop(&mut self) {
        abort(&mut self.retry);
        abort(&mut self.periodic);
        abort(&mut self.poll);
    }
}

struct Inner {
    state: Mutex<State>,
    config: CloudConfig,
    api: CloudApiClient,
    audience: CloudApiClient,
    profile: Profile,
    device_id: String,
    device: Device,
    topics: Publisher,
}

/// All network and keychain work is asynchronous. Run account operations on
/// AppCore's runtime. Views can subscribe to SessionSnapshot and DevicePairingState
/// independently of outstanding cloud requests.
#[derive(Clone)]
pub struct CloudSession(Arc<Inner>);

const IMAGINE_KEY: &str = "/v1/account/imagine-key";

impl CloudSession {
    pub async fn open(
        config: CloudConfig,
        user_data: PathBuf,
        protector: Arc<dyn KeyProtector>,
        settings: Option<Settings>,
        topics: Publisher,
    ) -> Result<Self, ApiError> {
        let session = Self::open_local(config, user_data, protector, settings, topics).await?;
        session.start().await;
        Ok(session)
    }

    /// Restore local account state without starting network hydration. The
    /// application calls `start` after live-plan and onboarding migration.
    pub async fn open_local(
        config: CloudConfig,
        user_data: PathBuf,
        protector: Arc<dyn KeyProtector>,
        settings: Option<Settings>,
        topics: Publisher,
    ) -> Result<Self, ApiError> {
        let api = CloudApiClient::new(&config.api, config.fallback.as_deref())?;
        let audience = if config.audience == config.api {
            api.clone()
        } else {
            CloudApiClient::new(&config.audience, None)?
        };
        let profile = Profile {
            root: user_data,
            protector,
            settings,
        };
        let (device_id, device, persisted, token) = tokio::task::spawn_blocking({
            let profile = profile.clone();
            move || profile.load()
        })
        .await
        .map_err(|_| ApiError::new("Could not open the Kairo account profile."))?;
        let mut session = signed_out(persisted);
        if token.is_some() {
            session.state = CloudSessionState::Stale;
            tracing::info!("[Cloud] restored saved refresh token");
        } else {
            tracing::info!(persisted, "[Cloud] no saved refresh token found");
        }
        let pairing = pairing(DevicePairingStatus::Idle, None);
        topics.publish(session.clone());
        topics.publish(pairing.clone());
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                session,
                pairing,
                token: token.map(Zeroizing::new),
                epoch: 0,
                intent: 0,
                pairing_generation: 0,
                refresh: None,
                retry_at: None,
                failures: 0,
                retry: None,
                periodic: None,
                poll: None,
                running: true,
            }),
            config,
            api,
            audience,
            profile,
            device_id,
            device,
            topics,
        });
        Ok(Self(inner))
    }

    /// Begin initial refresh and the quiet refresh loop, once. Never await
    /// the network: only installation of the task is awaited.
    pub async fn start(&self) {
        let mut state = self.0.state.lock().await;
        if state.periodic.is_some() || !state.running {
            return;
        }
        let weak = Arc::downgrade(&self.0);
        let jitter = u64::from(u16::from_le_bytes(
            uuid::Uuid::new_v4().as_bytes()[..2]
                .try_into()
                .expect("two bytes"),
        )) % 60_000;
        state.periodic = Some(
            tokio::spawn(async move {
                loop {
                    let Some(session) = weak.upgrade().map(CloudSession) else {
                        break;
                    };
                    session.refresh().await;
                    drop(session);
                    sleep(Duration::from_millis(600_000 + jitter)).await;
                }
            })
            .abort_handle(),
        );
    }

    pub fn device_id(&self) -> &str {
        &self.0.device_id
    }

    pub async fn session(&self) -> SessionSnapshot {
        self.0.state.lock().await.session.clone()
    }

    pub(crate) async fn account_identity(&self) -> (u64, SessionSnapshot) {
        let state = self.0.state.lock().await;
        (state.epoch, state.session.clone())
    }

    pub(crate) fn topics(&self) -> &Publisher {
        &self.0.topics
    }

    pub(crate) fn device(&self) -> &Device {
        &self.0.device
    }

    pub async fn can_upload(&self) -> bool {
        self.upload_org().await.is_some()
    }

    async fn upload_org(&self) -> Option<String> {
        let state = self.0.state.lock().await;
        if state.session.state == CloudSessionState::SignedOut
            || !state
                .session
                .user
                .as_ref()
                .is_some_and(|user| user.email_verified)
        {
            return None;
        }
        state
            .session
            .org
            .as_ref()
            .map(|org| org.id.clone())
            .filter(|id| !id.is_empty())
    }

    pub async fn upload_sermon(
        &self,
        payload: SermonUploadInput,
    ) -> Result<SermonUploadResult, ApiError> {
        let org = self.upload_org().await.ok_or_else(|| {
            ApiError::new("Sign in and confirm your email address to publish recaps.")
        })?;
        let org: String = url::form_urlencoded::byte_serialize(org.as_bytes()).collect();
        self.post(&format!("/v1/orgs/{org}/sermons"), payload, 120)
            .await
    }

    pub(crate) async fn report_usage(
        &self,
        report: UsageReport,
    ) -> Result<UsageReportResult, ApiError> {
        if !self.can_upload().await {
            return Err(ApiError::new("Not signed in"));
        }
        self.post("/v1/usage/report", report, 15).await
    }

    pub(crate) async fn report_crashes(
        &self,
        report: CrashUpload,
    ) -> Result<CrashUploadResult, ApiError> {
        if !self.can_upload().await {
            return Err(ApiError::new("Not signed in"));
        }
        self.post("/v1/crashes", report, 20).await
    }

    pub async fn send_feedback(
        &self,
        kind: FeedbackKind,
        message: String,
        context: Option<String>,
    ) -> CloudResult<FeedbackUploadResult> {
        if !self.can_upload().await {
            return form(Err(ApiError::new(
                "Sign in and confirm your email address to send feedback.",
            )));
        }
        let device = self.device();
        form(
            self.post(
                "/v1/feedback",
                FeedbackUpload {
                    kind,
                    message,
                    context,
                    app_version: device.app_version.into(),
                    os: device.os.into(),
                    os_version: device.os_version.clone(),
                },
                15,
            )
            .await,
        )
    }

    /// The person's own OpenAI key for Imagine, kept (encrypted) on their
    /// account so it follows them to every device. `Ok(None)`: none saved,
    /// or nobody signed in with a confirmed email.
    pub async fn imagine_key(&self) -> Result<Option<String>, ApiError> {
        #[derive(Deserialize)]
        struct Saved {
            key: Option<String>,
        }
        if !self.can_upload().await {
            return Ok(None);
        }
        let saved: Saved = self
            .request(
                &self.0.api,
                Method::GET,
                IMAGINE_KEY,
                None,
                Some(Duration::from_secs(15)),
            )
            .await?;
        Ok(saved.key.filter(|k| !k.is_empty()))
    }

    /// Saves (`Some`) or forgets (`None`) the account's Imagine key. Does
    /// nothing for someone not signed in with a confirmed email.
    pub async fn set_imagine_key(&self, key: Option<&str>) -> Result<(), ApiError> {
        if !self.can_upload().await {
            return Ok(());
        }
        let timeout = Some(Duration::from_secs(15));
        let _: Value = match key {
            Some(key) => {
                let body = json!({ "key": key });
                self.request(&self.0.api, Method::PUT, IMAGINE_KEY, Some(body), timeout)
                    .await?
            }
            None => {
                self.request(&self.0.api, Method::DELETE, IMAGINE_KEY, None, timeout)
                    .await?
            }
        };
        Ok(())
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        payload: impl Serialize,
        timeout_seconds: u64,
    ) -> Result<T, ApiError> {
        let payload =
            serde_json::to_value(payload).map_err(|_| ApiError::new("Invalid Kairo report."))?;
        self.request(
            &self.0.api,
            Method::POST,
            path,
            Some(payload),
            Some(Duration::from_secs(timeout_seconds)),
        )
        .await
    }

    pub async fn pairing(&self) -> DevicePairingState {
        self.0.state.lock().await.pairing.clone()
    }

    /// Forward platform wake or window focus here to bypass offline backoff.
    pub async fn reconnect(&self) {
        let mut state = self.0.state.lock().await;
        if !state.running || state.session.state != CloudSessionState::Stale {
            return;
        }
        abort(&mut state.retry);
        state.retry_at = None;
        self.start_refresh(&mut state);
    }

    pub async fn stop(&self) {
        let inflight = {
            let mut state = self.0.state.lock().await;
            state.running = false;
            abort(&mut state.periodic);
            abort(&mut state.retry);
            state.cancel_pairing();
            self.0.topics.publish(state.pairing.clone());
            state.refresh.clone()
        };
        // Let an in-flight rotation finish persisting its new refresh token so
        // the next launch does not replay a revoked token.
        if let Some(mut rx) = inflight {
            let _ =
                tokio::time::timeout(Duration::from_secs(5), rx.wait_for(Option::is_some)).await;
        }
    }

    pub async fn sign_up(&self, input: SignUpInput) -> CloudResult<SessionSnapshot> {
        form(self.authenticate("/v1/auth/signup", json!(input)).await)
    }

    pub async fn sign_in(&self, input: SignInInput) -> CloudResult<SessionSnapshot> {
        form(self.authenticate("/v1/auth/login", json!(input)).await)
    }

    async fn authenticate(
        &self,
        endpoint: &str,
        input: Value,
    ) -> Result<SessionSnapshot, ApiError> {
        let intent = {
            let mut state = self.0.state.lock().await;
            state.intent = state.intent.wrapping_add(1);
            state.cancel_pairing();
            self.0.topics.publish(state.pairing.clone());
            state.intent
        };
        let body = self.0.device.payload(Some(&self.0.device_id), input);
        let result = self
            .0
            .api
            .request(Method::POST, endpoint, Some(body), ANONYMOUS)
            .await?;
        let session = self.clone();
        tokio::spawn(async move { session.adopt(result, Adoption::Intent(intent)).await })
            .await
            .map_err(|_| ApiError::new("Could not save the account session."))?
    }

    async fn adopt(
        &self,
        result: AuthResultPayload,
        source: Adoption,
    ) -> Result<SessionSnapshot, ApiError> {
        if result.access_token.is_empty() || result.refresh_token.is_empty() {
            return Err(ApiError::new("Kairo returned an invalid account session."));
        }
        let mut state = self.0.state.lock().await;
        match source {
            Adoption::Intent(intent) if intent == state.intent => {}
            Adoption::Pairing(generation) if generation == state.pairing_generation => {}
            _ => return Err(ApiError::new("This sign-in was cancelled.")),
        }
        state.epoch = state.epoch.wrapping_add(1);
        state.refresh = None;
        state.reset_retry();
        let token = Zeroizing::new(result.refresh_token);
        let persisted = self.persist(Some(token.clone())).await;
        if !persisted {
            tracing::warn!("[Cloud] signed in, but refresh token could not be persisted");
        }
        state.token = Some(token);
        self.set_access(Some(result.access_token), false);
        let org = result.org;
        state.session = SessionSnapshot {
            state: CloudSessionState::Active,
            user: Some(result.user),
            org: Some(org.clone()),
            orgs: vec![org.clone()],
            last_synced_at: Some(now_ms()),
            credentials_persisted: Some(persisted),
        };
        if let Adoption::Pairing(_) = source {
            state.pairing = pairing(DevicePairingStatus::Approved, None);
            state.poll = None;
            self.0.topics.publish(state.pairing.clone());
        }
        self.0.topics.publish(state.session.clone());
        self.seed_church(org.name).await;
        Ok(state.session.clone())
    }

    pub async fn sign_out(&self) -> SessionSnapshot {
        let session = self.clone();
        // Once requested, local cleanup must survive a closed account view.
        tokio::spawn(async move {
            let (token, snapshot) = {
                let mut state = session.0.state.lock().await;
                let token = session.0.api.access_token();
                state.intent = state.intent.wrapping_add(1);
                state.epoch = state.epoch.wrapping_add(1);
                state.cancel_pairing();
                state.refresh = None;
                state.reset_retry();
                state.token = None;
                session.set_access(None, false);
                state.session = signed_out(session.persist(None).await);
                session.0.topics.publish(state.pairing.clone());
                session.0.topics.publish(state.session.clone());
                (token, state.session.clone())
            };
            let _ = session
                .0
                .api
                .request_with_token::<Value>(
                    Method::POST,
                    "/v1/auth/logout",
                    None,
                    RequestOptions {
                        allow_refresh: false,
                        ..Default::default()
                    },
                    token.as_deref(),
                )
                .await;
            snapshot
        })
        .await
        .unwrap_or_else(|_| signed_out(false))
    }

    pub async fn request_password_reset(&self, email: String) -> CloudResult<()> {
        form(
            self.email_request("/v1/auth/forgot-password", json!({ "email": email }))
                .await,
        )
    }

    pub async fn resend_verification(&self) -> CloudResult<()> {
        let email = self.session().await.user.map(|user| user.email);
        form(match email {
            Some(email) => {
                self.email_request("/v1/auth/resend-verification", json!({ "email": email }))
                    .await
            }
            None => Ok(()),
        })
    }

    pub async fn verify_email_code(&self, code: String) -> CloudResult<SessionSnapshot> {
        form(self.verify(code).await)
    }

    async fn verify(&self, code: String) -> Result<SessionSnapshot, ApiError> {
        let (epoch, email) = {
            let state = self.0.state.lock().await;
            (
                state.epoch,
                state
                    .session
                    .user
                    .as_ref()
                    .map(|user| user.email.clone())
                    .ok_or_else(|| ApiError::new("Sign in first."))?,
            )
        };
        self.email_request(
            "/v1/auth/verify-email-code",
            json!({ "email": email, "code": code }),
        )
        .await?;
        if self.0.state.lock().await.epoch == epoch {
            self.refresh().await;
        }
        let mut state = self.0.state.lock().await;
        if state.epoch == epoch
            && let Some(user) = &mut state.session.user
        {
            user.email_verified = true;
            self.0.topics.publish(state.session.clone());
        }
        Ok(state.session.clone())
    }

    async fn email_request(&self, endpoint: &str, body: Value) -> Result<(), ApiError> {
        self.0
            .api
            .request::<Value>(Method::POST, endpoint, Some(body), ANONYMOUS)
            .await
            .map(|_| ())
    }

    pub async fn refresh(&self) -> bool {
        self.refresh_token().await.is_some()
    }

    fn refresh_token(&self) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        Box::pin(async move {
            let mut result = {
                let mut state = self.0.state.lock().await;
                self.start_refresh(&mut state)?
            };
            result
                .wait_for(Option::is_some)
                .await
                .ok()?
                .clone()
                .flatten()
        })
    }

    fn start_refresh(&self, state: &mut State) -> Option<watch::Receiver<Option<Option<String>>>> {
        if let Some(inflight) = &state.refresh {
            return Some(inflight.clone());
        }
        if !state.running
            || state
                .retry_at
                .is_some_and(|deadline| Instant::now() < deadline)
        {
            return None;
        }
        let token = state.token.clone()?;
        let epoch = state.epoch;
        let (sender, receiver) = watch::channel(None);
        let session = self.clone();
        tokio::spawn(async move {
            session.exchange(epoch, token, sender).await;
        });
        state.refresh = Some(receiver.clone());
        Some(receiver)
    }

    async fn exchange(
        &self,
        epoch: u64,
        token: Zeroizing<String>,
        reply: watch::Sender<Option<Option<String>>>,
    ) {
        let payload = self
            .0
            .device
            .payload(None, json!({ "refreshToken": &*token }));
        let response: Result<RefreshPayload, ApiError> = self
            .0
            .api
            .request::<RefreshPayload>(
                Method::POST,
                "/v1/auth/refresh",
                Some(payload),
                RequestOptions {
                    timeout: Some(COLD_REQUEST),
                    ..ANONYMOUS
                },
            )
            .await
            .and_then(|tokens| {
                if tokens.access_token.is_empty() || tokens.refresh_token.is_empty() {
                    Err(ApiError::new("Kairo returned an invalid account session."))
                } else {
                    Ok(tokens)
                }
            });

        let mut state = self.0.state.lock().await;
        if state.epoch != epoch {
            reply.send_replace(Some(None));
            return;
        }
        state.refresh = None;
        let mut hydrate = false;
        let outcome = match response {
            Ok(result) => {
                state.reset_retry();
                let token = Zeroizing::new(result.refresh_token);
                let persisted = self.persist(Some(token.clone())).await;
                if !persisted {
                    tracing::warn!(
                        "[Cloud] rotated refresh token could not be persisted; next launch may require sign-in"
                    );
                }
                state.session.credentials_persisted = Some(persisted);
                state.token = Some(token);
                self.set_access(Some(result.access_token.clone()), true);
                hydrate =
                    state.session.state == CloudSessionState::Stale || state.session.user.is_none();
                state.session.state = CloudSessionState::Active;
                state.session.last_synced_at = Some(now_ms());
                Some(result.access_token)
            }
            Err(error) if error.status == Some(401) => {
                tracing::warn!(
                    message = %error.message,
                    "[Cloud] refresh rejected with 401; clearing saved session"
                );
                state.epoch = state.epoch.wrapping_add(1);
                state.token = None;
                state.reset_retry();
                self.set_access(None, false);
                state.session = signed_out(self.persist(None).await);
                None
            }
            Err(error) => {
                let wait = RETRY.delay(u32::try_from(state.failures).unwrap_or(u32::MAX));
                tracing::debug!(
                    status = ?error.status,
                    message = %error.message,
                    wait_secs = wait.as_secs(),
                    "[Cloud] refresh failed; keeping saved session stale"
                );
                state.failures = state.failures.saturating_add(1);
                let at = Instant::now() + wait;
                state.retry_at = Some(at);
                abort(&mut state.retry);
                if state.running {
                    state.retry = Some(
                        tokio::spawn(retry(Arc::downgrade(&self.0), epoch, at)).abort_handle(),
                    );
                }
                state.session.state = CloudSessionState::Stale;
                None
            }
        };
        self.0.topics.publish(state.session.clone());
        reply.send_replace(Some(outcome.clone()));
        drop(state);
        if hydrate && let Some(token) = outcome {
            self.hydrate(epoch, &token).await;
        }
    }

    async fn hydrate(&self, epoch: u64, token: &str) {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            user: CloudUser,
            org_id: String,
            orgs: Vec<CloudOrg>,
        }
        let Ok(response) = self
            .0
            .api
            .request_with_token::<Response>(
                Method::GET,
                "/v1/auth/session",
                None,
                RequestOptions {
                    allow_refresh: false,
                    ..Default::default()
                },
                Some(token),
            )
            .await
        else {
            return;
        };
        let mut state = self.0.state.lock().await;
        if state.epoch != epoch {
            return;
        }
        let org = response
            .orgs
            .iter()
            .find(|org| org.id == response.org_id)
            .or_else(|| response.orgs.first())
            .cloned();
        state.session.user = Some(response.user);
        state.session.org = org.clone();
        state.session.orgs = response.orgs;
        self.0.topics.publish(state.session.clone());
        if let Some(org) = org {
            self.seed_church(org.name).await;
        }
    }

    pub async fn start_pairing(&self) -> DevicePairingState {
        let generation = {
            let mut state = self.0.state.lock().await;
            state.cancel_pairing();
            state.intent = state.intent.wrapping_add(1);
            state.pairing_generation
        };
        let body = self.0.device.payload(Some(&self.0.device_id), json!({}));
        let result = self
            .0
            .api
            .request::<DeviceStartPayload>(
                Method::POST,
                "/v1/auth/device/start",
                Some(body),
                ANONYMOUS,
            )
            .await;
        let mut state = self.0.state.lock().await;
        if state.pairing_generation != generation || !state.running {
            return state.pairing.clone();
        }
        match result {
            Ok(start)
                if pair_seconds(start.expires_in).is_some()
                    && pair_seconds(start.interval).is_some()
                    && !start.device_code.is_empty()
                    && !start.user_code.is_empty()
                    && let Some(verification) = browser_url(&start.verification_uri) =>
            {
                state.pairing = DevicePairingState {
                    status: DevicePairingStatus::Waiting,
                    user_code: Some(start.user_code.clone()),
                    verification_uri: Some(verification.to_string()),
                    expires_at: Some(now_ms() + start.expires_in * 1000.0),
                    message: None,
                };
                let session = self.clone();
                tokio::spawn(async move {
                    let _ = session.launch_browser(verification).await;
                });
                state.poll = Some(
                    tokio::spawn(poll(Arc::downgrade(&self.0), start, generation)).abort_handle(),
                );
            }
            Ok(_) => {
                state.pairing = pairing(
                    DevicePairingStatus::Error,
                    Some("Kairo returned an invalid pairing code.".into()),
                )
            }
            Err(error) => state.pairing = pairing(DevicePairingStatus::Error, Some(error.message)),
        }
        self.0.topics.publish(state.pairing.clone());
        state.pairing.clone()
    }

    pub async fn cancel_pairing(&self) -> DevicePairingState {
        let mut state = self.0.state.lock().await;
        state.cancel_pairing();
        self.0.topics.publish(state.pairing.clone());
        state.pairing.clone()
    }

    async fn finish_pairing(&self, generation: u64, status: DevicePairingStatus, message: &str) {
        let mut state = self.0.state.lock().await;
        if state.pairing_generation == generation {
            state.poll = None;
            state.pairing = pairing(status, Some(message.into()));
            self.0.topics.publish(state.pairing.clone());
        }
    }

    pub async fn open_web(&self, path: Option<&str>) -> Result<(), ApiError> {
        let path = safe_return_path(path, "/");
        let url = self
            .0
            .config
            .web
            .join(&path)
            .ok()
            .filter(|url| url.origin() == self.0.config.web.origin())
            .ok_or_else(|| ApiError::new("Invalid Kairo web address."))?;
        self.launch_browser(url).await
    }

    async fn launch_browser(&self, url: Url) -> Result<(), ApiError> {
        if self.0.config.system_browser {
            return tokio::task::spawn_blocking(move || open::that_detached(url.as_str()))
                .await
                .ok()
                .and_then(Result::ok)
                .ok_or_else(|| ApiError::new("Could not open the system browser."));
        }
        self.0.topics.emit(OpenBrowser(url));
        Ok(())
    }

    /// Provider credentials were retired upstream. This compatibility call
    /// makes no network request, even when callers request throttling.
    pub async fn pull_org_secrets(&self, _throttle: bool) {}

    /// Focus handler for the legacy sync action. Licence refresh is owned by
    /// the licence service and can run independently.
    pub async fn sync_org_secrets(&self) {
        self.reconnect().await;
        self.pull_org_secrets(true).await;
    }

    pub async fn sync_org_profile(&self, mut patch: OrgProfilePatch) {
        let (epoch, org_id) = {
            let state = self.0.state.lock().await;
            if state.session.state == CloudSessionState::SignedOut
                || !state
                    .session
                    .user
                    .as_ref()
                    .is_some_and(|user| user.email_verified)
            {
                return;
            }
            let Some(org) = &state.session.org else {
                return;
            };
            (state.epoch, org.id.clone())
        };
        patch.name = patch
            .name
            .as_deref()
            .map(kairo_domain::js::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        if patch.name.is_none() && patch.timezone.is_none() && patch.service_times.is_none() {
            return;
        }
        let Ok(body) = serde_json::to_value(patch) else {
            return;
        };
        let escaped: String = url::form_urlencoded::byte_serialize(org_id.as_bytes()).collect();
        let Ok(updated) = self
            .request::<Value>(
                &self.0.api,
                Method::PATCH,
                &format!("/v1/orgs/{escaped}"),
                Some(body),
                None,
            )
            .await
        else {
            return;
        };
        let Some(name) = updated
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        let mut state = self.0.state.lock().await;
        if state.epoch == epoch {
            if let Some(org) = &mut state.session.org
                && org.id == org_id
            {
                org.name = name.into();
            }
            for org in &mut state.session.orgs {
                if org.id == org_id {
                    org.name = name.into();
                }
            }
            self.0.topics.publish(state.session.clone());
        }
    }

    pub async fn managed_request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, ApiError> {
        if !matches!(method, Method::GET | Method::POST | Method::DELETE)
            || !is_managed_endpoint(path)
        {
            return Err(ApiError::new("Unsupported managed endpoint"));
        }
        self.request(
            &self.0.api,
            method,
            path,
            body,
            Some(Duration::from_secs(30)),
        )
        .await
    }

    pub async fn request_provider<T: DeserializeOwned>(
        &self,
        request: &ManagedRequest,
    ) -> Result<T, ApiError> {
        let body =
            serde_json::to_value(request).map_err(|_| ApiError::new("Invalid managed request."))?;
        self.managed_request(Method::POST, MANAGED_REQUEST_PATH, Some(body))
            .await
    }

    pub async fn engage_request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, ApiError> {
        if !matches!(method, Method::GET | Method::POST) {
            return Err(ApiError::new("Unsupported Engage method."));
        }
        self.reconnect().await;
        let timeout = if method == Method::GET {
            Duration::from_secs(10)
        } else {
            COLD_REQUEST
        };
        self.audience_request(method, AudienceRoute::Engage(path), body, timeout)
            .await
    }

    pub async fn create_audience_room<T: DeserializeOwned>(
        &self,
        title: &str,
        settings: &kairo_domain::audience::AudienceRoomSettings,
    ) -> Result<T, ApiError> {
        self.audience_request(
            Method::POST,
            AudienceRoute::Rooms,
            Some(json!({ "title": title, "settings": settings })),
            COLD_REQUEST,
        )
        .await
    }

    async fn audience_request<T: DeserializeOwned>(
        &self,
        method: Method,
        route: AudienceRoute<'_>,
        body: Option<Value>,
        timeout: Duration,
    ) -> Result<T, ApiError> {
        let base = &self.0.config.audience;
        let url = audience_url(base, route).map_err(|error| ApiError::new(error.to_string()))?;
        self.request(&self.0.audience, method, &url, body, Some(timeout))
            .await
            .map_err(|error| ApiError {
                message: audience_error(&error.message, error.status, base, route),
                ..error
            })
    }

    async fn request<T: DeserializeOwned>(
        &self,
        client: &CloudApiClient,
        method: Method,
        path: &str,
        body: Option<Value>,
        timeout: Option<Duration>,
    ) -> Result<T, ApiError> {
        client
            .request_with_refresh(
                method,
                path,
                body,
                RequestOptions {
                    timeout,
                    ..Default::default()
                },
                || self.refresh_token(),
            )
            .await
    }

    fn set_access(&self, token: Option<String>, rotation: bool) {
        for client in [&self.0.api, &self.0.audience] {
            if rotation && let Some(token) = &token {
                client.rotate_access_token(token.clone());
            } else {
                client.set_access_token(token.clone());
            }
        }
    }

    async fn persist(&self, token: Option<Zeroizing<String>>) -> bool {
        let profile = self.0.profile.clone();
        tokio::task::spawn_blocking(move || {
            profile.persist(token.as_ref().map(|token| token.as_str()))
        })
        .await
        .unwrap_or(false)
    }

    async fn seed_church(&self, name: String) {
        let profile = self.0.profile.clone();
        if tokio::task::spawn_blocking(move || profile.seed_church(&name))
            .await
            .unwrap_or(false)
        {
            self.0.topics.emit(ChurchNameSeeded);
        }
    }
}

enum Adoption {
    Intent(u64),
    Pairing(u64),
}

async fn retry(weak: Weak<Inner>, epoch: u64, at: Instant) {
    // A suspended monotonic clock can delay a Tokio timer after wake, so a
    // platform resume or window focus retries at once.
    let Some(mut power) = weak
        .upgrade()
        .map(|inner| inner.topics.stream::<PowerEvent>())
    else {
        return;
    };
    tokio::select! {
        () = sleep_until(at) => {}
        _ = power.next() => {}
    }
    if let Some(session) = weak.upgrade().map(CloudSession) {
        let mut state = session.0.state.lock().await;
        if state.epoch == epoch && state.retry_at == Some(at) {
            state.retry = None;
            state.retry_at = None;
            session.start_refresh(&mut state);
        }
    }
}

fn pair_seconds(value: f64) -> Option<Duration> {
    (value.is_finite() && value > 0.0 && value <= 86_400.0).then(|| Duration::from_secs_f64(value))
}

async fn poll(weak: Weak<Inner>, started: DeviceStartPayload, generation: u64) {
    let deadline = Instant::now() + Duration::from_secs_f64(started.expires_in);
    let mut delay = started.interval;
    loop {
        if timeout_at(deadline, sleep(Duration::from_secs_f64(delay)))
            .await
            .is_err()
            || Instant::now() >= deadline
        {
            break;
        }
        let Some(session) = weak.upgrade().map(CloudSession) else {
            return;
        };
        let result = timeout_at(
            deadline,
            session.0.api.request::<DevicePollPayload>(
                Method::POST,
                "/v1/auth/device/token",
                Some(json!({ "deviceCode": &started.device_code })),
                ANONYMOUS,
            ),
        )
        .await;
        match result {
            Ok(Ok(DevicePollPayload::Approved(result))) => {
                // Adoption must finish persisting even if the polling task is
                // cancelled after the pairing has already committed.
                let adopter = session.clone();
                if let Ok(Err(error)) = tokio::spawn(async move {
                    adopter.adopt(*result, Adoption::Pairing(generation)).await
                })
                .await
                {
                    session
                        .finish_pairing(generation, DevicePairingStatus::Error, &error.message)
                        .await;
                }
                return;
            }
            Ok(Ok(DevicePollPayload::Denied)) => {
                session
                    .finish_pairing(
                        generation,
                        DevicePairingStatus::Denied,
                        "That request was declined.",
                    )
                    .await;
                return;
            }
            Ok(Ok(DevicePollPayload::Expired)) | Err(_) => break,
            Ok(Ok(DevicePollPayload::SlowDown)) => {
                delay = next_poll_delay_sec(DevicePollOutcome::SlowDown, Some(delay))
            }
            Ok(Ok(DevicePollPayload::Pending) | Err(_)) => {}
        }
    }
    if let Some(session) = weak.upgrade().map(CloudSession) {
        session
            .finish_pairing(
                generation,
                DevicePairingStatus::Expired,
                "The code expired. Start again.",
            )
            .await;
    }
}

pub(crate) fn form<T>(result: Result<T, ApiError>) -> CloudResult<T> {
    match result {
        Ok(data) => CloudResult {
            ok: true,
            data: Some(data),
            error: None,
        },
        Err(error) => CloudResult {
            ok: false,
            data: None,
            error: Some(error.message),
        },
    }
}
