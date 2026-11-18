//! Kairo server contracts and other cloud types, from `src/lib/cloud/`.
//! The NestJS server in `server/src` compiles against the same contracts.

use crate::serde_util::{Extra, nullable, required, string_enum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

string_enum!(
    pub enum OrgRole {
        Viewer = "viewer",
        Operator = "operator",
        Admin = "admin",
        Owner = "owner",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudUser {
    pub id: String,
    pub email: String,
    pub name: String,
    pub email_verified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudOrg {
    pub id: String,
    pub name: String,
    pub role: OrgRole,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResultPayload {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: String,
    pub user: CloudUser,
    pub org: CloudOrg,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshPayload {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStartPayload {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: f64,
    pub expires_in: f64,
}

/// `DevicePollPayload`, tagged by `state`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DevicePollPayload {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Approved(Box<AuthResultPayload>),
}

string_enum!(
    pub enum DevicePollOutcome {
        Pending = "pending",
        SlowDown = "slow_down",
        Approved = "approved",
        Denied = "denied",
        Expired = "expired",
    }
);
string_enum!(
    pub enum CloudSessionState {
        SignedOut = "signed-out",
        Active = "active",
        Stale = "stale",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub state: CloudSessionState,
    #[serde(deserialize_with = "required")]
    pub user: Option<CloudUser>,
    #[serde(deserialize_with = "required")]
    pub org: Option<CloudOrg>,
    pub orgs: Vec<CloudOrg>,
    #[serde(deserialize_with = "required")]
    pub last_synced_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials_persisted: Option<bool>,
}

string_enum!(
    pub enum DevicePairingStatus {
        Idle = "idle",
        Waiting = "waiting",
        Approved = "approved",
        Denied = "denied",
        Expired = "expired",
        Error = "error",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePairingState {
    pub status: DevicePairingStatus,
    #[serde(deserialize_with = "required")]
    pub user_code: Option<String>,
    #[serde(deserialize_with = "required")]
    pub verification_uri: Option<String>,
    #[serde(deserialize_with = "required")]
    pub expires_at: Option<f64>,
    #[serde(deserialize_with = "required")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgDevice {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "required")]
    pub hostname: Option<String>,
    #[serde(deserialize_with = "required")]
    pub os: Option<String>,
    #[serde(deserialize_with = "required")]
    pub os_version: Option<String>,
    #[serde(deserialize_with = "required")]
    pub arch: Option<String>,
    #[serde(deserialize_with = "required")]
    pub app_version: Option<String>,
    #[serde(deserialize_with = "required")]
    pub electron_version: Option<String>,
    pub signed_in_as: String,
    #[serde(deserialize_with = "required")]
    pub signed_in_email: Option<String>,
    pub last_seen_at: String,
    pub last_login_at: String,
    #[serde(deserialize_with = "required")]
    pub ip: Option<String>,
    pub online: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignUpInput {
    pub email: String,
    pub password: String,
    pub name: String,
    pub org_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignInInput {
    pub email: String,
    pub password: String,
}

string_enum!(
    /// `CloudBlockReason` without its `null` member; use `Option<CloudBlockReason>`.
    pub enum CloudBlockReason {
        EmailUnverified = "email-unverified",
    }
);

/// `CloudResult<T>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudResult<T> {
    pub ok: bool,
    #[serde(
        deserialize_with = "required",
        bound(deserialize = "T: Deserialize<'de>")
    )]
    pub data: Option<T>,
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgSecretsPayload {
    pub deepgram_api_key: String,
    pub anthropic_api_key: String,
    pub deepseek_api_key: String,
    pub bible_api_key: String,
    pub brave_api_key: String,
    pub google_translate_api_key: String,
    #[serde(deserialize_with = "required")]
    pub updated_at: Option<String>,
}

/// Every key optional; `null` clears.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgSecretsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub deepgram_api_key: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub anthropic_api_key: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub deepseek_api_key: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub bible_api_key: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub brave_api_key: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub google_translate_api_key: Option<Option<String>>,
}

/// `ManagedProviderPatch` has the same keys and rules.
pub type ManagedProviderPatch = OrgSecretsPatch;

string_enum!(
    pub enum MemberStatus {
        Active = "active",
        Invited = "invited",
        Removed = "removed",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgMember {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub role: OrgRole,
    pub status: MemberStatus,
    pub joined_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonKeyPoint {
    pub title: String,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonKeyScripture {
    pub reference: String,
    pub connection: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonSummary {
    pub headline: String,
    pub big_idea: String,
    pub key_points: Vec<SermonKeyPoint>,
    pub memorable_quotes: Vec<String>,
    pub takeaways: Vec<String>,
    pub key_scriptures: Vec<SermonKeyScripture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_to_action: Option<String>,
}

string_enum!(
    pub enum SermonStatus {
        Pending = "pending",
        Ready = "ready",
        Failed = "failed",
    }
);
string_enum!(
    pub enum SummaryErrorCode {
        NoApiKey = "no-api-key",
        Provider = "provider",
        Timeout = "timeout",
        Format = "format",
        Refusal = "refusal",
        Stalled = "stalled",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonListItem {
    pub id: String,
    pub title: String,
    pub speaker: String,
    pub preached_at: String,
    pub duration_ms: f64,
    pub word_count: f64,
    pub status: SermonStatus,
    #[serde(deserialize_with = "required")]
    pub headline: Option<String>,
    pub share_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonTranscriptSegment {
    pub id: String,
    pub text: String,
    pub timestamp: f64,
    pub duration: f64,
}

/// `SermonDetail extends SermonListItem`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonDetail {
    #[serde(flatten)]
    pub item: SermonListItem,
    #[serde(deserialize_with = "required")]
    pub summary: Option<SermonSummary>,
    #[serde(deserialize_with = "required")]
    pub failure_reason: Option<String>,
    #[serde(deserialize_with = "required")]
    pub failure_code: Option<SummaryErrorCode>,
    #[serde(deserialize_with = "required")]
    pub share_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonTranscriptPayload {
    pub segments: Vec<SermonTranscriptSegment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonListPage {
    pub items: Vec<SermonListItem>,
    #[serde(deserialize_with = "required")]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonPeriodStats {
    pub services: f64,
    pub duration_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonWeekBucket {
    pub week_start: String,
    pub services: f64,
    pub duration_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonSpeakerStat {
    pub name: String,
    pub services: f64,
    pub duration_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonBookStat {
    pub book: String,
    pub count: f64,
}

string_enum!(
    pub enum SermonStatsRange {
        Today = "today",
        Days7 = "7d",
        Weeks4 = "4w",
        Weeks12 = "12w",
        Months6 = "6m",
        Ytd = "ytd",
        All = "all",
        Custom = "custom",
    }
);
string_enum!(
    pub enum SermonStatsGranularity {
        Day = "day",
        Week = "week",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonStats {
    pub services: f64,
    pub recaps_ready: f64,
    pub recaps_failed: f64,
    pub recaps_pending: f64,
    pub total_duration_ms: f64,
    pub total_words: f64,
    pub scripture_passages: f64,
    pub shared_links: f64,
    pub average_duration_ms: f64,
    pub previous: SermonPeriodStats,
    pub granularity: SermonStatsGranularity,
    pub weekly: Vec<SermonWeekBucket>,
    pub speakers: Vec<SermonSpeakerStat>,
    pub scripture_books: Vec<SermonBookStat>,
    pub speaker_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonUploadWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
    pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonUploadSegment {
    pub id: String,
    pub text: String,
    pub timestamp: f64,
    pub duration: f64,
    pub words: Vec<SermonUploadWord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonUploadScripture {
    pub reference: String,
    pub translation: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SermonUploadInput {
    pub local_id: String,
    pub title: String,
    pub speaker: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sermon_title: String,
    pub started_at: f64,
    pub ended_at: f64,
    pub transcript: Vec<SermonUploadSegment>,
    pub scriptures: Vec<SermonUploadScripture>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SermonUploadResult {
    pub id: String,
    pub status: SermonStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicSermonPayload {
    pub title: String,
    pub speaker: String,
    pub church_name: String,
    pub preached_at: String,
    pub summary: SermonSummary,
}

// ─── crashes.ts ───────────────────────────────────────────────────────────────

string_enum!(
    pub enum CrashKind {
        Renderer = "renderer",
        ChildProcess = "child_process",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashSystem {
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub app_version: String,
    pub electron_version: String,
    pub memory_gb: f64,
    pub gpu: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub id: String,
    pub kind: CrashKind,
    pub process: String,
    pub reason: String,
    pub exit_code: f64,
    pub first_at: String,
    pub last_at: String,
    pub count: f64,
    pub system: CrashSystem,
    pub errors: Vec<String>,
    pub log: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Persisted as `kairo-crash-reports.json`, the upload queue.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CrashReportQueue {
    pub reports: Vec<CrashReport>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashUpload {
    pub install_id: String,
    pub reports: Vec<CrashReport>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrashUploadResult {
    pub accepted: Vec<String>,
}

// ─── feedback.ts ──────────────────────────────────────────────────────────────

string_enum!(
    pub enum FeedbackKind {
        Idea = "idea",
        Problem = "problem",
        Praise = "praise",
        Other = "other",
    }
);
string_enum!(
    pub enum FeedbackStatus {
        New = "new",
        Read = "read",
        Done = "done",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackUpload {
    pub kind: FeedbackKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub app_version: String,
    pub os: String,
    pub os_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeedbackUploadResult {
    pub id: String,
}

// ─── licensing.ts ─────────────────────────────────────────────────────────────

string_enum!(
    pub enum LicensedFeature {
        Presentation = "presentation",
        Transcription = "transcription",
        Scripture = "scripture",
        Summaries = "summaries",
        Translation = "translation",
        WebSearch = "webSearch",
    }
);
string_enum!(
    pub enum LicenseStatus {
        Active = "active",
        Suspended = "suspended",
        Revoked = "revoked",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicensePolicy {
    pub free_issuance_enabled: bool,
    pub default_max_devices: f64,
    pub default_max_transcription_sessions: f64,
    pub default_transcription_minutes: f64,
    pub default_managed_requests: f64,
    pub offline_grace_days: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseDevice {
    pub device_id: String,
    pub name: String,
    pub activated_at: String,
    pub last_seen_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseUsage {
    pub period: String,
    pub transcription_minutes: f64,
    pub managed_requests: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChurchLicense {
    pub org_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_name: Option<String>,
    pub status: LicenseStatus,
    #[serde(deserialize_with = "required")]
    pub expires_at: Option<String>,
    pub features: Vec<LicensedFeature>,
    pub max_devices: f64,
    pub max_transcription_sessions: f64,
    pub transcription_minutes: f64,
    pub managed_requests: f64,
    pub devices: Vec<LicenseDevice>,
    pub usage: LicenseUsage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminChurchLicense {
    pub org_id: String,
    pub org_name: String,
    pub owner_email: String,
    #[serde(deserialize_with = "required")]
    pub created_at: Option<String>,
    pub license: ChurchLicense,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseLeaseClaims {
    pub org_id: String,
    pub device_id: String,
    /// Features this build does not know are dropped, so a server that grants
    /// a newer feature does not invalidate the whole lease.
    #[serde(deserialize_with = "known_features")]
    pub features: Vec<LicensedFeature>,
    pub issued_at: f64,
    pub refresh_after: f64,
    pub expires_at: f64,
    #[serde(deserialize_with = "required")]
    pub license_expires_at: Option<f64>,
}

fn known_features<'de, D>(deserializer: D) -> Result<Vec<LicensedFeature>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Vec::<String>::deserialize(deserializer)?
        .iter()
        .filter_map(|name| LicensedFeature::from_name(name))
        .collect())
}

/// A lease as the server sent it. The signature covers `JSON.stringify` of
/// the claims object as received, so the received text is kept alongside the
/// typed view: fields this build does not know still count towards it.
#[derive(Clone, Debug, PartialEq)]
pub struct SignedLicenseLease {
    pub claims: LicenseLeaseClaims,
    /// The claims JSON exactly as received. `None` for a lease built in code.
    pub raw_claims: Option<String>,
    pub signature: String,
    pub public_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedLicenseLeaseWire {
    claims: Box<serde_json::value::RawValue>,
    signature: String,
    public_key: String,
}

impl<'de> Deserialize<'de> for SignedLicenseLease {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SignedLicenseLeaseWire::deserialize(deserializer)?;
        let claims = serde_json::from_str(wire.claims.get()).map_err(serde::de::Error::custom)?;
        Ok(Self {
            claims,
            raw_claims: Some(wire.claims.get().to_owned()),
            signature: wire.signature,
            public_key: wire.public_key,
        })
    }
}

impl Serialize for SignedLicenseLease {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error as _, SerializeStruct as _};
        let mut lease = serializer.serialize_struct("SignedLicenseLease", 3)?;
        match &self.raw_claims {
            // Written back verbatim so a saved lease still verifies.
            Some(raw) => lease.serialize_field(
                "claims",
                &serde_json::value::RawValue::from_string(raw.clone()).map_err(S::Error::custom)?,
            )?,
            None => lease.serialize_field("claims", &self.claims)?,
        }
        lease.serialize_field("signature", &self.signature)?;
        lease.serialize_field("publicKey", &self.public_key)?;
        lease.end()
    }
}

/// `Record<'deepgram' | 'bible' | 'anthropic' | 'deepseek' | 'brave' | 'googleTranslate', T>`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMap<T> {
    pub deepgram: T,
    pub bible: T,
    pub anthropic: T,
    pub deepseek: T,
    pub brave: T,
    pub google_translate: T,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LicenseSnapshot {
    #[serde(deserialize_with = "required")]
    pub license: Option<ChurchLicense>,
    pub activated: bool,
    #[serde(deserialize_with = "required")]
    pub lease: Option<SignedLicenseLease>,
    pub providers: ProviderMap<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseActivationKey {
    pub id: String,
    pub key: String,
    #[serde(deserialize_with = "required")]
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<LicenseStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub expires_at: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<LicensedFeature>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_devices: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_transcription_sessions: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcription_minutes: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_requests: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProviderStatus {
    pub configured: ProviderMap<bool>,
    pub hints: ProviderMap<String>,
    #[serde(deserialize_with = "required")]
    pub updated_at: Option<String>,
}

string_enum!(
    pub enum ManagedProvider {
        Bible = "bible",
        Anthropic = "anthropic",
        Deepseek = "deepseek",
        Brave = "brave",
        GoogleTranslate = "googleTranslate",
    }
);
string_enum!(
    pub enum HttpMethod {
        Get = "GET",
        Post = "POST",
    }
);
string_enum!(
    pub enum ManagedPurpose {
        Scripture = "scripture",
        Summaries = "summaries",
        Translation = "translation",
        WebSearch = "webSearch",
    }
);

/// A query value: `string | number | boolean`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QueryValue {
    Text(String),
    Number(f64),
    Flag(bool),
}

/// A provider call relayed by the Kairo server.
///
/// `body` is `unknown` in TypeScript: it is the provider's own request body,
/// passed through untouched, so it stays a `serde_json::Value`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManagedRequest {
    pub provider: ManagedProvider,
    pub path: String,
    pub method: HttpMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<BTreeMap<String, QueryValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<ManagedPurpose>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionGrant {
    pub access_token: String,
    pub expires_in: f64,
    pub session_id: String,
    pub renew_after_seconds: f64,
}

/// A renewal extends the lease of the grant already held: the server answers
/// with when to renew next, not a new token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionRenewal {
    pub renew_after_seconds: f64,
}

// ─── org-secrets.ts and auth-state.ts ─────────────────────────────────────────

/// Which provider keys are set, without their values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretsConfigured {
    pub deepgram: bool,
    pub anthropic: bool,
    pub deepseek: bool,
    pub bible: bool,
    pub brave: bool,
    pub google_translate: bool,
}

string_enum!(
    pub enum SessionTone {
        Neutral = "neutral",
        Good = "good",
        Warn = "warn",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionDescription {
    pub label: String,
    pub detail: String,
    pub tone: SessionTone,
}

mod account_rules;
pub use account_rules::*;

mod setup;
pub use setup::*;

mod onboarding;
pub use onboarding::*;

mod org_secrets;
pub use org_secrets::*;

pub const CRASH_KINDS: &[CrashKind] = CrashKind::ALL;
pub const CRASH_MAX_REPORTS: usize = 10;
pub const CRASH_MAX_ERRORS: usize = 20;
pub const CRASH_MAX_ERROR_CHARS: usize = 1_000;
pub const CRASH_MAX_LOG_LINES: usize = 200;
pub const CRASH_MAX_LOG_CHARS: usize = 64_000;
pub const CRASH_REPEAT_MS: u64 = 60_000;
pub const FEEDBACK_KINDS: &[FeedbackKind] = FeedbackKind::ALL;
pub const FEEDBACK_STATUSES: &[FeedbackStatus] = FeedbackStatus::ALL;
pub const FEEDBACK_MAX_CHARS: usize = 5_000;

pub fn scrub_home(text: &str, home: &str, windows: bool) -> String {
    if home.is_empty() {
        return text.into();
    }
    let forms = [
        home.to_owned(),
        home.replace('\\', "\\\\"),
        home.replace('\\', "/"),
    ];
    let mut out = text.to_owned();
    for form in forms {
        if windows {
            let escaped = regex::escape(&form);
            if let Ok(pattern) = regex::RegexBuilder::new(&escaped)
                .case_insensitive(true)
                .build()
            {
                out = pattern.replace_all(&out, "~").into_owned();
            }
        } else {
            out = out.replace(&form, "~");
        }
    }
    out
}

pub fn log_tail(text: &str, max_lines: usize, max_chars: usize) -> String {
    let mut lines: Vec<_> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let first = if max_lines == 0 {
        0
    } else {
        lines.len().saturating_sub(max_lines)
    };
    let tail = lines[first..].join("\n");
    if tail.encode_utf16().count() <= max_chars || max_chars == 0 {
        return tail;
    }
    let mut units = 0;
    let start = tail
        .char_indices()
        .rev()
        .take_while(|(_, ch)| {
            units += ch.len_utf16();
            units <= max_chars
        })
        .last()
        .map_or(tail.len(), |(index, _)| index);
    let tail = &tail[start..];
    tail.split_once('\n').map_or(tail, |(_, rest)| rest).into()
}
