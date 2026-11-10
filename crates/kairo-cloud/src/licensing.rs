//! Offline licence verification, activation and managed feature access.

use std::{
    future::pending,
    path::PathBuf,
    sync::{Arc, Weak},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use kairo_core::{Publisher, Watch};
use kairo_domain::{
    cloud::{
        CloudResult, CloudSessionState, LicenseSnapshot, LicenseStatus, LicensedFeature,
        ManagedProvider, ManagedPurpose, ManagedRequest, SecretsConfigured, SessionSnapshot,
        SignedLicenseLease, TranscriptionGrant,
    },
    json::to_js_string,
};
use kairo_net::{ApiError, Method};
use kairo_store::{
    build_config::LICENSE_PUBLIC_KEY,
    secrets::{KeyProtector, SecureStore},
    settings::now_ms,
};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, Notify, watch},
    task::AbortHandle,
    time::{Instant, sleep, sleep_until},
};

use crate::CloudSession;

const SNAPSHOT: &str = "license-snapshot";
const TRUSTED_KEY: &str = "license-trusted-key";
const OBSERVED_TIME: &str = "license-observed-time";
const CLOCK_TOLERANCE: f64 = 60.0;
const MAX_GRACE: f64 = 14.0 * 86_400.0;
const REFRESH_INTERVAL: Duration = Duration::from_secs(86_400);
const ACTIVATE_FIRST: &str =
    "Activate this computer in Account → Licence & Devices before using this feature.";

fn now_seconds() -> f64 {
    (now_ms() / 1000.0).floor()
}

fn parse_public_key(key: &str) -> Option<[u8; 32]> {
    // RFC 8410 Ed25519 SubjectPublicKeyInfo. Parameters are absent and the
    // 32-byte public key is a BIT STRING with no unused bits.
    const SPKI_PREFIX: &[u8] = &[0x30, 0x2a, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 3, 0x21, 0];
    let body = key
        .trim()
        .strip_prefix("-----BEGIN PUBLIC KEY-----")?
        .strip_suffix("-----END PUBLIC KEY-----")?;
    let base64: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let der = STANDARD.decode(base64).ok()?;
    der.strip_prefix(SPKI_PREFIX)?.try_into().ok()
}

/// Verify the same UTF-8 `JSON.stringify(claims)` envelope the server signs.
/// The caller supplies the pinned key. A key embedded in a disk snapshot
/// never establishes trust.
pub fn valid_license_lease(
    lease: Option<&SignedLicenseLease>,
    trusted_key: Option<&str>,
    org_id: &str,
    device_id: &str,
    feature: Option<LicensedFeature>,
    now: f64,
) -> bool {
    let (Some(lease), Some(key)) = (lease, trusted_key.and_then(parse_public_key)) else {
        return false;
    };
    let claims = &lease.claims;
    if org_id.is_empty()
        || device_id.is_empty()
        || claims.org_id != org_id
        || claims.device_id != device_id
        || ![
            claims.issued_at,
            claims.refresh_after,
            claims.expires_at,
            now,
        ]
        .into_iter()
        .all(f64::is_finite)
        || claims.issued_at > now + CLOCK_TOLERANCE
        || claims.expires_at <= now
        || claims.expires_at <= claims.issued_at
        || claims.expires_at - claims.issued_at > MAX_GRACE
        || claims.license_expires_at.is_some_and(|expiry| {
            !expiry.is_finite() || expiry <= now || claims.expires_at > expiry
        })
        || feature.is_some_and(|feature| !claims.features.contains(&feature))
    {
        return false;
    }
    let (Ok(payload), Ok(signature)) = (to_js_string(claims), STANDARD.decode(&lease.signature))
    else {
        return false;
    };
    UnparsedPublicKey::new(&ED25519, key)
        .verify(payload.as_bytes(), &signature)
        .is_ok()
}

/// Feature gates for all providers. LLM calls share a gateway and use the
/// entitlement for the selected task.
pub fn provider_feature(request: &ManagedRequest) -> LicensedFeature {
    match request.provider {
        ManagedProvider::Bible => LicensedFeature::Scripture,
        ManagedProvider::Brave => LicensedFeature::WebSearch,
        ManagedProvider::GoogleTranslate => LicensedFeature::Translation,
        ManagedProvider::Anthropic | ManagedProvider::Deepseek => {
            match request.purpose.unwrap_or(ManagedPurpose::Scripture) {
                ManagedPurpose::Scripture => LicensedFeature::Scripture,
                ManagedPurpose::Summaries => LicensedFeature::Summaries,
                ManagedPurpose::Translation => LicensedFeature::Translation,
                ManagedPurpose::WebSearch => LicensedFeature::WebSearch,
            }
        }
    }
}

fn empty() -> LicenseSnapshot {
    LicenseSnapshot {
        license: None,
        activated: false,
        lease: None,
        providers: Default::default(),
    }
}

#[derive(Clone)]
struct Vault {
    profile: PathBuf,
    protector: Arc<dyn KeyProtector>,
}

impl Vault {
    fn store(&self) -> SecureStore<'_> {
        SecureStore::new(&self.profile, self.protector.as_ref(), "Kairo")
    }

    async fn read(&self) -> (LicenseSnapshot, Option<String>, f64) {
        let vault = self.clone();
        tokio::task::spawn_blocking(move || {
            let store = vault.store();
            let snapshot = store
                .read(SNAPSHOT)
                .and_then(|value| serde_json::from_str(&value).ok())
                .unwrap_or_else(empty);
            let key = store.read(TRUSTED_KEY).filter(|key| !key.is_empty());
            let time = store
                .read(OBSERVED_TIME)
                .and_then(|value| value.trim().parse::<f64>().ok())
                .filter(|time| time.is_finite() && *time >= 0.0)
                .unwrap_or(0.0);
            (snapshot, key, time)
        })
        .await
        .unwrap_or_else(|_| (empty(), None, 0.0))
    }

    async fn save(&self, values: Vec<(&'static str, String)>) {
        let vault = self.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let store = vault.store();
            for (name, value) in values {
                store.write(name, &value);
            }
        })
        .await;
    }

    async fn clear_snapshot(&self) {
        let vault = self.clone();
        let _ = tokio::task::spawn_blocking(move || vault.store().clear(SNAPSHOT)).await;
    }
}

type Refresh = watch::Receiver<Option<Result<LicenseSnapshot, ApiError>>>;

struct State {
    snapshot: LicenseSnapshot,
    trusted_key: Option<String>,
    highest_time: f64,
    persisted_time: f64,
    account_epoch: u64,
    org_id: Option<String>,
    in_flight: Option<(u64, Refresh)>,
    next_flight: u64,
}

struct Inner {
    session: CloudSession,
    vault: Vault,
    topics: Publisher,
    state: Mutex<State>,
    online: Mutex<()>,
    updated: Arc<Notify>,
    monitor: std::sync::OnceLock<AbortHandle>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(monitor) = self.monitor.get() {
            monitor.abort();
        }
    }
}

#[derive(Clone)]
pub struct LicensingService(Arc<Inner>);

#[derive(Clone)]
struct Context {
    epoch: u64,
    org_id: String,
}

impl LicensingService {
    /// Open on AppCore's runtime. Use the same topic publisher as the
    /// CloudSession so sign-out, church changes and rehydration propagate.
    pub async fn open(
        session: CloudSession,
        profile: PathBuf,
        protector: Arc<dyn KeyProtector>,
        topics: Publisher,
    ) -> Self {
        Self::open_with_trusted_key(session, profile, protector, topics, LICENSE_PUBLIC_KEY).await
    }

    /// Supply a public pin from trusted build or deployment configuration.
    pub async fn open_with_trusted_key(
        session: CloudSession,
        profile: PathBuf,
        protector: Arc<dyn KeyProtector>,
        topics: Publisher,
        trusted_key: Option<&str>,
    ) -> Self {
        let sessions = topics.watch::<SessionSnapshot>();
        let vault = Vault { profile, protector };
        let (mut snapshot, stored_key, highest_time) = vault.read().await;
        let (epoch, current) = session.account_identity().await;
        if current.state == CloudSessionState::SignedOut {
            snapshot = empty();
            vault.clear_snapshot().await;
        }
        let service = Self(Arc::new(Inner {
            session,
            vault,
            topics,
            state: Mutex::new(State {
                snapshot,
                trusted_key: trusted_key
                    .filter(|key| !key.is_empty())
                    .map(str::to_owned)
                    .or(stored_key),
                highest_time,
                persisted_time: highest_time,
                account_epoch: epoch,
                org_id: current.org.map(|org| org.id),
                in_flight: None,
                next_flight: 0,
            }),
            online: Mutex::new(()),
            updated: Arc::new(Notify::new()),
            monitor: std::sync::OnceLock::new(),
        }));
        service.emit().await;
        let task = tokio::spawn(monitor(Arc::downgrade(&service.0), sessions));
        let _ = service.0.monitor.set(task.abort_handle());
        service
    }

    /// Returns the snapshot with activation recomputed against the signed
    /// lease, current account and rollback-protected wall clock.
    pub async fn get_status(&self) -> LicenseSnapshot {
        let mut state = self.0.state.lock().await;
        let (epoch, current) = self.0.session.account_identity().await;
        self.reconcile(&mut state, epoch, &current).await;
        let now = now_seconds();
        let clock_valid = now >= state.highest_time - CLOCK_TOLERANCE;
        state.highest_time = state.highest_time.max(now);
        if state.highest_time - state.persisted_time >= CLOCK_TOLERANCE {
            self.0
                .vault
                .save(vec![(OBSERVED_TIME, state.highest_time.to_string())])
                .await;
            state.persisted_time = state.highest_time;
        }
        let org_id = current
            .org
            .as_ref()
            .map(|org| org.id.as_str())
            .or_else(|| {
                if current.state == CloudSessionState::Stale {
                    state
                        .snapshot
                        .lease
                        .as_ref()
                        .map(|lease| lease.claims.org_id.as_str())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let activated = current.state != CloudSessionState::SignedOut
            && clock_valid
            && state.snapshot.activated
            && state
                .snapshot
                .license
                .as_ref()
                .is_some_and(|license| license.status == LicenseStatus::Active)
            && valid_license_lease(
                state.snapshot.lease.as_ref(),
                state.trusted_key.as_deref(),
                org_id,
                self.0.session.device_id(),
                None,
                now,
            );
        LicenseSnapshot {
            activated,
            ..state.snapshot.clone()
        }
    }

    /// Server-side presence markers for the settings form.
    pub async fn secrets_configured(&self) -> SecretsConfigured {
        let flags = self.get_status().await.providers;
        SecretsConfigured {
            deepgram: flags.deepgram,
            anthropic: flags.anthropic,
            deepseek: flags.deepseek,
            bible: flags.bible,
            brave: flags.brave,
            google_translate: flags.google_translate,
        }
    }

    pub async fn assert_feature(&self, feature: LicensedFeature) -> Result<(), ApiError> {
        let snapshot = self.get_status().await;
        if snapshot.activated
            && snapshot
                .lease
                .is_some_and(|lease| lease.claims.features.contains(&feature))
        {
            Ok(())
        } else {
            Err(ApiError::new(ACTIVATE_FIRST))
        }
    }

    pub async fn activate_form(
        &self,
        key: &str,
        name: Option<&str>,
    ) -> CloudResult<LicenseSnapshot> {
        crate::session::form(self.activate(key, name).await)
    }

    pub async fn refresh_form(&self) -> CloudResult<LicenseSnapshot> {
        crate::session::form(self.refresh().await)
    }

    pub async fn activate(
        &self,
        key: &str,
        name: Option<&str>,
    ) -> Result<LicenseSnapshot, ApiError> {
        let trimmed = kairo_domain::js::trim(key);
        if trimmed.is_empty() || key.encode_utf16().count() > 512 {
            return Err(ApiError::new("Enter a valid activation key."));
        }
        let context = self.context().await?;
        let name = match name {
            Some(name) => {
                String::from_utf16_lossy(&name.encode_utf16().take(160).collect::<Vec<_>>())
            }
            None => tokio::task::spawn_blocking(sysinfo::System::host_name)
                .await
                .ok()
                .flatten()
                .unwrap_or_default(),
        };
        let body = json!({ "key": trimmed, "name": name });
        let service = self.clone();
        tokio::spawn(async move {
            service
                .request_lease("/v1/licensing/activate", Some(body), &context)
                .await
        })
        .await
        .map_err(|_| ApiError::new("Could not save the activation lease."))?
    }

    pub async fn refresh(&self) -> Result<LicenseSnapshot, ApiError> {
        let context = self.context().await?;
        let mut receiver = {
            let mut state = self.0.state.lock().await;
            let (epoch, current) = self.0.session.account_identity().await;
            self.reconcile(&mut state, epoch, &current).await;
            if !matches_context(&context, epoch, &current) {
                return Err(account_changed());
            }
            if let Some((_, receiver)) = &state.in_flight {
                receiver.clone()
            } else {
                let (sender, receiver) = watch::channel(None);
                let id = state.next_flight;
                state.next_flight = id.wrapping_add(1);
                state.in_flight = Some((id, receiver.clone()));
                let service = self.clone();
                tokio::spawn(async move {
                    let result = service
                        .request_lease("/v1/licensing/refresh", None, &context)
                        .await;
                    let mut state = service.0.state.lock().await;
                    if state
                        .in_flight
                        .as_ref()
                        .is_some_and(|(flight, _)| *flight == id)
                    {
                        state.in_flight = None;
                    }
                    sender.send_replace(Some(result));
                });
                receiver
            }
        };
        receiver
            .wait_for(Option::is_some)
            .await
            .ok()
            .and_then(|result| result.clone())
            .unwrap_or_else(|| Err(ApiError::new("Could not refresh the activation lease.")))
    }

    async fn request_lease(
        &self,
        path: &str,
        body: Option<Value>,
        context: &Context,
    ) -> Result<LicenseSnapshot, ApiError> {
        // An overlapping refresh must not replace a successful activation
        // with a snapshot taken before that activation reached the server.
        let _online = self.0.online.lock().await;
        let (epoch, current) = self.0.session.account_identity().await;
        if !matches_context(context, epoch, &current) {
            return Err(account_changed());
        }
        let snapshot = self
            .0
            .session
            .managed_request(Method::POST, path, body)
            .await?;
        self.adopt(snapshot, context).await
    }

    async fn context(&self) -> Result<Context, ApiError> {
        let (epoch, current) = self.0.session.account_identity().await;
        current
            .org
            .filter(|org| !org.id.is_empty() && current.state != CloudSessionState::SignedOut)
            .map(|org| Context {
                epoch,
                org_id: org.id,
            })
            .ok_or_else(|| ApiError::new("Sign in to activate this computer."))
    }

    async fn reconcile(&self, state: &mut State, epoch: u64, current: &SessionSnapshot) {
        let current_org = current.org.as_ref().map(|org| org.id.as_str());
        let changed = epoch != state.account_epoch
            || current_org
                .zip(state.org_id.as_deref())
                .is_some_and(|(current, prior)| current != prior);
        if changed {
            state.account_epoch = epoch;
            state.in_flight = None;
        }
        if let Some(org) = current_org {
            state.org_id = Some(org.into());
        }
        if (changed || current.state == CloudSessionState::SignedOut) && state.snapshot != empty() {
            state.snapshot = empty();
            self.0.vault.clear_snapshot().await;
            self.0.updated.notify_one();
        }
    }

    async fn adopt(
        &self,
        snapshot: LicenseSnapshot,
        context: &Context,
    ) -> Result<LicenseSnapshot, ApiError> {
        let mut state = self.0.state.lock().await;
        let (epoch, current) = self.0.session.account_identity().await;
        self.reconcile(&mut state, epoch, &current).await;
        if !matches_context(context, epoch, &current) {
            return Err(account_changed());
        }
        let now = now_seconds();
        let mut writes = Vec::new();
        if snapshot.activated {
            let key = state.trusted_key.as_deref().or_else(|| {
                snapshot
                    .lease
                    .as_ref()
                    .map(|lease| lease.public_key.as_str())
            });
            if !valid_license_lease(
                snapshot.lease.as_ref(),
                key,
                &context.org_id,
                self.0.session.device_id(),
                None,
                now,
            ) {
                return Err(ApiError::new("The activation lease could not be verified."));
            }
            if state.trusted_key.is_none()
                && let Some(key) = key
            {
                writes.push((TRUSTED_KEY, key.to_owned()));
                state.trusted_key = Some(key.to_owned());
            }
        }
        state.highest_time = now;
        state.persisted_time = now;
        writes.push((OBSERVED_TIME, now.to_string()));
        if let Ok(json) = to_js_string(&snapshot) {
            writes.push((SNAPSHOT, json));
        }
        self.0.vault.save(writes).await;
        state.snapshot = snapshot;
        drop(state);
        self.0.updated.notify_one();
        Ok(self.emit().await)
    }

    async fn emit(&self) -> LicenseSnapshot {
        let snapshot = self.get_status().await;
        self.0.topics.publish(snapshot.clone());
        snapshot
    }

    pub async fn request<T: DeserializeOwned>(
        &self,
        request: &ManagedRequest,
    ) -> Result<T, ApiError> {
        self.assert_feature(provider_feature(request)).await?;
        self.0.session.request_provider(request).await
    }

    /// The account and church API.Bible requests speak for, or `None` when
    /// signed out. A change means previously authorized Bible ids no longer
    /// apply, as an API key change does in Electron.
    pub async fn account_key(&self) -> Option<String> {
        let (epoch, current) = self.0.session.account_identity().await;
        if current.state == CloudSessionState::SignedOut {
            return None;
        }
        current
            .org
            .filter(|org| !org.id.is_empty())
            .map(|org| format!("{epoch}:{}", org.id))
    }

    pub async fn transcription(&self) -> Result<TranscriptionGrant, ApiError> {
        self.assert_feature(LicensedFeature::Transcription).await?;
        self.0
            .session
            .managed_request(Method::POST, "/v1/managed/transcription", None)
            .await
    }

    pub async fn renew_transcription(&self, id: &str) -> Result<TranscriptionGrant, ApiError> {
        let id = encode_grant_id(id)?;
        self.0
            .session
            .managed_request(
                Method::POST,
                &format!("/v1/managed/transcription/{id}/renew"),
                None,
            )
            .await
    }

    pub async fn release_transcription(&self, id: &str) -> Result<(), ApiError> {
        let id = encode_grant_id(id)?;
        self.0
            .session
            .managed_request::<Value>(
                Method::DELETE,
                &format!("/v1/managed/transcription/{id}"),
                None,
            )
            .await
            .map(|_| ())
    }
}

fn encode_grant_id(id: &str) -> Result<String, ApiError> {
    if matches!(id, "" | "." | "..") {
        return Err(ApiError::new("Invalid transcription session."));
    }
    Ok(url::form_urlencoded::byte_serialize(id.as_bytes())
        .collect::<String>()
        .replace('+', "%20"))
}

fn account_changed() -> ApiError {
    ApiError::new("Your account changed. Check activation again.")
}

fn matches_context(context: &Context, epoch: u64, session: &SessionSnapshot) -> bool {
    context.epoch == epoch
        && session.state != CloudSessionState::SignedOut
        && session
            .org
            .as_ref()
            .is_some_and(|org| org.id == context.org_id)
}

async fn expiry_timer(expiry: Option<f64>) {
    if let Some(expiry) = expiry {
        sleep(Duration::from_secs_f64(
            (expiry.ceil() - now_ms() / 1000.0 + 0.001).max(0.0),
        ))
        .await;
    } else {
        pending().await
    }
}

async fn monitor(weak: Weak<Inner>, mut sessions: Watch<SessionSnapshot>) {
    let mut daily = Instant::now() + REFRESH_INTERVAL;
    loop {
        let Some(inner) = weak.upgrade() else { return };
        let expiry = inner
            .state
            .lock()
            .await
            .snapshot
            .lease
            .as_ref()
            .map(|lease| lease.claims.expires_at)
            .filter(|expiry| {
                expiry.is_finite()
                    && *expiry > now_ms() / 1000.0
                    && *expiry <= now_seconds() + MAX_GRACE + CLOCK_TOLERANCE
            });
        // Notify's owned waiter does not keep the service alive between events.
        let changed = inner.updated.clone().notified_owned();
        drop(inner);
        tokio::pin!(changed);
        changed.as_mut().enable();
        tokio::select! {
            session = sessions.next() => {
                let (Some(session), Some(inner)) = (session, weak.upgrade()) else { return };
                let service = LicensingService(inner);
                let snapshot = service.emit().await;
                if let Some(org) = &session.org
                    && (!snapshot.activated || snapshot.lease.as_ref().is_none_or(|lease| {
                        lease.claims.org_id != org.id || lease.claims.refresh_after <= now_seconds()
                    }))
                {
                    refresh_background(service);
                }
            }
            _ = sleep_until(daily) => {
                daily = Instant::now() + REFRESH_INTERVAL;
                if let Some(inner) = weak.upgrade() {
                    refresh_background(LicensingService(inner));
                }
            }
            _ = expiry_timer(expiry) => {
                if let Some(inner) = weak.upgrade() {
                    LicensingService(inner).emit().await;
                }
            }
            _ = changed => {}
        }
    }
}

fn refresh_background(service: LicensingService) {
    tokio::spawn(async move {
        if service.refresh().await.is_err() {
            service.emit().await;
        }
    });
}
