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
