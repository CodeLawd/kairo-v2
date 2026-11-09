//! Audience questions, Engage rooms and their wire protocol, from
//! `src/lib/audience.ts`, `engage.ts` and `audience-protocol.ts`.

use crate::serde_util::{nullable, required, string_enum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

string_enum!(
    pub enum AudienceQuestionState {
        Review = "review",
        Approved = "approved",
        Answered = "answered",
        Hidden = "hidden",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudienceQuestion {
    pub id: String,
    pub source_id: String,
    pub text: String,
    #[serde(deserialize_with = "required")]
    pub author: Option<String>,
    pub votes: f64,
    pub state: AudienceQuestionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<f64>,
    pub created_at: f64,
}

string_enum!(
    pub enum AudienceSourceKind {
        Kairo = "kairo",
        Slido = "slido",
        Sample = "sample",
    }
);
string_enum!(
    pub enum AudienceStatusKind {
        Kairo = "kairo",
        Slido = "slido",
    }
);
string_enum!(
    pub enum AudienceSourceState {
        Connecting = "connecting",
        Live = "live",
        Error = "error",
        Stopped = "stopped",
    }
);
string_enum!(
    pub enum AudienceSourceStep {
        Reserving = "reserving",
        Connecting = "connecting",
        Finding = "finding",
        SignIn = "sign-in",
        OpenEvent = "open-event",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudienceSourceStatus {
    pub id: String,
    pub kind: AudienceStatusKind,
    pub label: String,
    pub state: AudienceSourceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<AudienceSourceStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moderator: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_review: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<f64>,
}

string_enum!(
    pub enum AudienceQuestionSort {
        Votes = "votes",
        Newest = "newest",
    }
);
string_enum!(
    pub enum AudienceMergeMode {
        Review = "review",
        Trusted = "trusted",
        Authoritative = "authoritative",
    }
);
string_enum!(
    pub enum AudienceNames {
        Optional = "optional",
        Off = "off",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudienceRoomSettings {
    pub review: bool,
    pub show_questions: bool,
    pub names: AudienceNames,
}

/// `DEFAULT_AUDIENCE_ROOM_SETTINGS`.
impl Default for AudienceRoomSettings {
    fn default() -> Self {
        Self {
            review: false,
            show_questions: true,
            names: AudienceNames::Optional,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudienceSession {
    pub room_id: String,
    pub code: String,
    pub url: String,
    pub title: String,
    pub qr_data_url: String,
    pub settings: AudienceRoomSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudienceState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engage: Option<EngageSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub live_poll_id: Option<Option<String>>,
    pub sample_available: bool,
    pub questions: Vec<AudienceQuestion>,
    pub sources: Vec<AudienceSourceStatus>,
    #[serde(deserialize_with = "required")]
    pub live_id: Option<String>,
    #[serde(deserialize_with = "required")]
    pub session: Option<AudienceSession>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudienceSourceOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub starts_at: Option<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub ends_at: Option<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<AudienceRoomSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moderator: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_review: Option<bool>,
}

string_enum!(
    pub enum EngagementStatus {
        Draft = "draft",
        Open = "open",
        Closed = "closed",
    }
);

/// `AudienceRoomSettings & { enabled }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QaActivity {
    pub enabled: bool,
    #[serde(flatten)]
    pub settings: AudienceRoomSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PollsActivity {
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngagementActivities {
    pub qa: QaActivity,
    pub polls: PollsActivity,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Engagement {
    pub id: String,
    pub code: String,
    pub name: String,
    pub org_name: String,
    #[serde(deserialize_with = "required")]
    pub starts_at: Option<f64>,
    #[serde(deserialize_with = "required")]
    pub ends_at: Option<f64>,
    pub status: EngagementStatus,
    pub activities: EngagementActivities,
    #[serde(deserialize_with = "required")]
    pub active_poll_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moderator {
    pub id: String,
    pub email: String,
    #[serde(deserialize_with = "required")]
    pub name: Option<String>,
    pub invited_at: f64,
    #[serde(deserialize_with = "required")]
    pub last_seen_at: Option<f64>,
    pub revoked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

string_enum!(
    pub enum ReplyAudience {
        Everyone = "everyone",
        Asker = "asker",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub id: String,
    pub question_id: String,
    pub text: String,
    pub by: String,
    pub audience: ReplyAudience,
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PollOption {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Poll {
    pub id: String,
    pub title: String,
    pub options: Vec<PollOption>,
    pub multiple: bool,
    pub state: EngagementStatus,
    pub show_results: bool,
}

/// `PollView extends Poll`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PollView {
    #[serde(flatten)]
    pub poll: Poll,
    pub totals: BTreeMap<String, f64>,
    pub voters: f64,
    pub voted: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngageSnapshot {
    pub engagement: Engagement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moderators: Option<Vec<Moderator>>,
    pub replies: Vec<Reply>,
    pub polls: Vec<PollView>,
}

string_enum!(
    pub enum DraftOrOpen {
        Draft = "draft",
        Open = "open",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngagementInput {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub starts_at: Option<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub ends_at: Option<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DraftOrOpen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<AudienceRoomSettings>,
}

string_enum!(
    pub enum OpenOrClosed {
        Open = "open",
        Closed = "closed",
    }
);

/// `EngageCommand`, tagged by `type`.
///
/// `poll` in `poll-save` is `unknown` in TypeScript and validated by
/// `normalizePoll`, so it stays a `serde_json::Value` here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EngageCommand {
    Edit {
        id: String,
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    Reply {
        question_id: String,
        text: String,
        audience: ReplyAudience,
    },
    Present {
        id: String,
    },
    PollSave {
        poll: serde_json::Value,
    },
    #[serde(rename_all = "camelCase")]
    PollState {
        id: String,
        state: OpenOrClosed,
        show_results: bool,
    },
    PollVote {
        id: String,
        options: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngagementIndex {
    pub id: String,
    pub code: String,
    pub name: String,
    pub status: EngagementStatus,
    #[serde(deserialize_with = "required")]
    pub starts_at: Option<f64>,
    #[serde(deserialize_with = "required")]
    pub ends_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudienceRoomCreated {
    pub room_id: String,
    pub code: String,
    pub operator_token: String,
    pub title: String,
    pub url: String,
    pub settings: AudienceRoomSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engagement: Option<Engagement>,
}

/// Sent by the operator or a member of the audience. Tagged by `type`.
/// The first six variants are `EngageCommand & { requestId }`; `poll` is a
/// `serde_json::Value` for the reason given on [`EngageCommand`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AudienceClientEvent {
    #[serde(rename_all = "camelCase")]
    Edit {
        id: String,
        text: String,
        request_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Reply {
        question_id: String,
        text: String,
        audience: ReplyAudience,
        request_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Present {
        id: String,
        request_id: String,
    },
    #[serde(rename_all = "camelCase")]
    PollSave {
        poll: serde_json::Value,
        request_id: String,
    },
    #[serde(rename_all = "camelCase")]
    PollState {
        id: String,
        state: OpenOrClosed,
        show_results: bool,
        request_id: String,
    },
    #[serde(rename_all = "camelCase")]
    PollVote {
        id: String,
        options: Vec<String>,
        request_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Ask {
        request_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Vote {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        voted: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    State {
        id: String,
        state: AudienceQuestionState,
        request_id: String,
    },
    #[serde(rename_all = "camelCase")]
    End {
        request_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Settings {
        settings: AudienceRoomSettings,
        request_id: String,
    },
    Snapshot,
}

/// Sent by the audience server. Tagged by `type`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AudienceServerEvent {
    Engage(EngageSnapshot),
    #[serde(rename_all = "camelCase")]
    Snapshot {
        questions: Vec<AudienceQuestion>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        own_questions: Option<Vec<AudienceQuestion>>,
        title: String,
        org_name: String,
        joined: f64,
        ended: bool,
        voted: Vec<String>,
        settings: AudienceRoomSettings,
    },
    Question {
        question: AudienceQuestion,
    },
    State {
        id: String,
        state: AudienceQuestionState,
    },
    Joined {
        joined: f64,
    },
    #[serde(rename_all = "camelCase")]
    Ack {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        question: Option<AudienceQuestion>,
    },
    Ended,
    #[serde(rename_all = "camelCase")]
    Error {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

/// `AUDIENCE_MAX_SNAPSHOT_BYTES`.
pub const AUDIENCE_MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

pub const AUDIENCE_STATES: &[AudienceQuestionState] = AudienceQuestionState::ALL;

pub fn is_audience_code(code: &str) -> bool {
    code.len() == 6
        && code
            .bytes()
            .all(|c| matches!(c, b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z' | b'2'..=b'9'))
}

pub fn normalize_audience_room_settings(value: &serde_json::Value) -> AudienceRoomSettings {
    AudienceRoomSettings {
        review: crate::normalize::safe_bool(value.get("review"), false),
        show_questions: crate::normalize::safe_bool(value.get("showQuestions"), true),
        names: crate::normalize::safe_enum(value.get("names"), AudienceNames::Optional),
    }
}

fn valid_request_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

pub fn parse_audience_event(message: &str) -> Option<AudienceClientEvent> {
    use serde_json::Value;
    if message.encode_utf16().count() > 8192 {
        return None;
    }
    let mut value: Value = serde_json::from_str(message).ok()?;
    let kind = value["type"].as_str()?;
    if kind == "snapshot" {
        return Some(AudienceClientEvent::Snapshot);
    }
    let request_id = value["requestId"]
        .as_str()
        .filter(|id| valid_request_id(id));
    if kind != "vote" && request_id.is_none() {
        return None;
    }
    let id = |key| {
        value[key]
            .as_str()
            .filter(|id| id.encode_utf16().count() <= 128)
    };
    match kind {
        "edit" | "present" | "poll-state" | "poll-vote" if id("id").is_none_or(str::is_empty) => {
            return None;
        }
        "vote" | "state" if id("id").is_none() => return None,
        "reply" if id("questionId").is_none() => return None,
        "ask"
            if value
                .get("author")
                .is_some_and(|author| !author.is_string()) =>
        {
            return None;
        }
        "poll-save" if !value["poll"].is_object() && !value["poll"].is_array() => return None,
        "poll-vote"
            if value["options"].as_array().is_none_or(|options| {
                options.len() > 12
                    || options
                        .iter()
                        .any(|opt| opt.as_str().is_none_or(|s| s.encode_utf16().count() > 64))
            }) =>
        {
            return None;
        }
        "vote" if value.get("voted").is_some_and(|voted| !voted.is_boolean()) => return None,
        "vote" if request_id.is_none() => {
            value.as_object_mut()?.remove("requestId");
        }
        "settings" => {
            let settings = &value["settings"];
            if !settings.is_object() && !settings.is_array() {
                return None;
            }
            value["settings"] =
                serde_json::to_value(normalize_audience_room_settings(settings)).ok()?;
        }
        _ => {}
    }
    serde_json::from_value(value).ok()
}

pub fn normalize_audience_text(value: &serde_json::Value) -> Option<String> {
    let text: String = crate::js::trim(value.as_str()?).chars().take(280).collect();
    let text = crate::js::trim(&text);
    (!text.is_empty()).then(|| text.into())
}

pub fn sort_audience_questions(
    questions: &[AudienceQuestion],
    sort: AudienceQuestionSort,
) -> Vec<AudienceQuestion> {
    let mut questions = questions.to_vec();
    let order = |a: f64, b: f64| b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal);
    questions.sort_by(|a, b| {
        order(a.sent_at.unwrap_or_default(), b.sent_at.unwrap_or_default())
            .then_with(|| {
                if sort == AudienceQuestionSort::Votes {
                    order(a.votes, b.votes)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| order(a.created_at, b.created_at))
            .then_with(|| a.id.encode_utf16().cmp(b.id.encode_utf16()))
    });
    questions
}

pub fn merge_audience_questions(
    questions: &[AudienceQuestion],
    updates: &[AudienceQuestion],
    mode: AudienceMergeMode,
) -> Vec<AudienceQuestion> {
    let mut merged: Vec<AudienceQuestion> = Vec::new();
    let mut indices = std::collections::HashMap::new();
    for question in questions {
        let index = *indices.entry(question.id.as_str()).or_insert(merged.len());
        if index == merged.len() {
            merged.push(question.clone());
        } else {
            merged[index] = question.clone();
        }
    }
    for update in updates {
        let index = *indices.entry(update.id.as_str()).or_insert(merged.len());
        let mut question = update.clone();
        if mode != AudienceMergeMode::Authoritative {
            question.state = merged.get(index).map_or_else(
                || {
                    if mode == AudienceMergeMode::Trusted {
                        update.state
                    } else {
                        AudienceQuestionState::Review
                    }
                },
                |question| question.state,
            );
        }
        if index == merged.len() {
            merged.push(question);
        } else {
            merged[index] = question;
        }
    }
    merged
}

pub fn set_audience_question_state(
    mut questions: Vec<AudienceQuestion>,
    id: &str,
    state: AudienceQuestionState,
) -> Vec<AudienceQuestion> {
    for question in questions.iter_mut().filter(|q| q.id == id) {
        question.state = state;
    }
    questions
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SlidoEvent {
    Code {
        code: String,
    },
    Event {
        #[serde(rename = "eventId")]
        event_id: String,
    },
}

pub fn parse_slido_event(input: &str) -> Option<SlidoEvent> {
    use regex::Regex;
    use std::sync::LazyLock;
    static CODE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^#?([a-zA-Z0-9-]{1,32})$").unwrap());
    static EVENT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^/event/([a-zA-Z0-9]{6,64})").unwrap());
    let value = crate::js::trim(input);
    if let Some(code) = CODE.captures(value) {
        return Some(SlidoEvent::Code {
            code: code[1].into(),
        });
    }
    let url = if value
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
        || value
            .get(..8)
            .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
    {
        url::Url::parse(value).ok()?
    } else {
        url::Url::parse(&format!("https://{value}")).ok()?
    };
    let host = url.host_str()?;
    if !["sli.do", "slido.com"].iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    }) {
        return None;
    }
    if let Some(event) = EVENT.captures(url.path()) {
        return Some(SlidoEvent::Event {
            event_id: event[1].into(),
        });
    }
    let path = match url.fragment() {
        Some(fragment) if !fragment.is_empty() => format!("{}#{fragment}", url.path()),
        _ => url.path().to_owned(),
    };
    let path = percent_encoding::percent_decode_str(&path)
        .decode_utf8()
        .ok()?;
    let code = path
        .strip_prefix('/')?
        .strip_suffix('/')
        .unwrap_or(&path[1..]);
    let code = CODE.captures(code)?;
    Some(SlidoEvent::Code {
        code: code[1].into(),
    })
}

pub fn audience_api_base_url(api: &str, override_url: Option<&str>) -> String {
    if let Some(url) = override_url
        .map(crate::js::trim)
        .filter(|url| !url.is_empty())
    {
        return url.strip_suffix('/').unwrap_or(url).into();
    }
    if let Ok(url) = url::Url::parse(api)
        && url.port() == Some(3000)
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    {
        return "http://localhost:8787".into();
    }
    api.strip_suffix('/').unwrap_or(api).into()
}

pub fn audience_room_error(raw: &str, status: Option<Option<u16>>, local: bool) -> &str {
    match status {
        Some(Some(404)) if local => {
            "Audience Q&A is not available on the local server. Start the server with bun run start:dev, then try again."
        }
        Some(Some(404)) => {
            "Audience Q&A is not available on this server yet. Please update the Kairo server."
        }
        Some(Some(401)) => "Sign in to Kairo to start a Q&A.",
        Some(None) if local => {
            "Could not reach the local Audience service. Start the server with bun run start:dev, then try again."
        }
        _ => crate::ipc_error::clean_ipc_error(raw),
    }
}

pub fn engagement_deadline(last_activity_at: f64, ends_at: Option<f64>) -> f64 {
    const DAY_MS: f64 = 86_400_000.0;
    crate::js::math_min(
        last_activity_at + 30.0 * DAY_MS,
        ends_at.map_or(f64::INFINITY, |end| end + DAY_MS),
    )
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedEngagementInput {
    pub name: String,
    pub starts_at: Option<f64>,
    pub ends_at: Option<f64>,
    pub status: DraftOrOpen,
}

pub fn normalize_engagement_input(
    value: &serde_json::Value,
) -> Result<NormalizedEngagementInput, &'static str> {
    let name = crate::workspace::text(&value["title"]);
    if name.is_empty() || name.encode_utf16().count() > 100 {
        return Err("Give the engagement a title of up to 100 characters.");
    }
    let date = |key| {
        let raw = &value[key];
        if raw.is_null() {
            return Ok(None);
        }
        raw.as_f64()
            .filter(|&n| n >= 0.0 && crate::js::is_safe_integer(n))
            .map(Some)
            .ok_or("Choose a valid date.")
    };
    let starts_at = date("startsAt")?;
    let ends_at = date("endsAt")?;
    if starts_at
        .zip(ends_at)
        .is_some_and(|(start, end)| end < start)
    {
        return Err("The end must follow the start.");
    }
    let status = match value.get("status") {
        None => DraftOrOpen::Open,
        Some(value) => DraftOrOpen::from_name(value.as_str().unwrap_or_default())
            .ok_or("Choose draft or open.")?,
    };
    Ok(NormalizedEngagementInput {
        name: name.into(),
        starts_at,
        ends_at,
        status,
    })
}

pub fn normalize_poll(
    value: &serde_json::Value,
    mut new_id: impl FnMut() -> String,
) -> Result<Poll, &'static str> {
    let title = crate::workspace::text(&value["title"]);
    let options = value["options"]
        .as_array()
        .filter(|options| (2..=12).contains(&options.len()));
    let Some(options) =
        options.filter(|_| !title.is_empty() && title.encode_utf16().count() <= 200)
    else {
        return Err("Add a title and between 2 and 12 options.");
    };
    let mut labels = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    let mut normalized = Vec::with_capacity(options.len());
    for option in options {
        let text = if option.is_string() {
            crate::workspace::text(option)
        } else {
            crate::workspace::text(&option["text"])
        };
        if text.is_empty() || text.encode_utf16().count() > 120 {
            return Err("Each option needs up to 120 characters.");
        }
        let id = option["id"]
            .as_str()
            .filter(|id| valid_request_id(id))
            .map_or_else(&mut new_id, str::to_owned);
        labels.insert(text.to_lowercase());
        ids.insert(id.clone());
        normalized.push(PollOption {
            id,
            text: text.into(),
        });
    }
    if labels.len() != normalized.len() || ids.len() != normalized.len() {
        return Err("Use different options.");
    }
    let id = value["id"]
        .as_str()
        .filter(|id| valid_request_id(id))
        .map_or_else(new_id, str::to_owned);
    Ok(Poll {
        id,
        title: title.into(),
        options: normalized,
        multiple: value["multiple"].as_bool().unwrap_or(false),
        state: EngagementStatus::Draft,
        show_results: false,
    })
}
