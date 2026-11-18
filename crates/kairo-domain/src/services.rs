//! Service records (one per church service), from `src/lib/service-records.ts`.

use crate::scripture::{ScriptureSuggestion, SermonPlan};
use crate::serde_util::{Extra, nullable, present, required, string_enum};
use crate::transcript::TranscriptResult;
use serde::{Deserialize, Serialize};

string_enum!(
    pub enum NuggetOrigin {
        Automatic = "automatic",
        Manual = "manual",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceNugget {
    pub id: String,
    pub text: String,
    pub captured_at: f64,
    pub source_ids: Vec<String>,
    pub origin: NuggetOrigin,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum UploadStatus {
        Idle = "idle",
        Queued = "queued",
        Uploading = "uploading",
        Uploaded = "uploaded",
        Failed = "failed",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceUploadState {
    pub status: UploadStatus,
    #[serde(deserialize_with = "required")]
    pub sermon_id: Option<String>,
    pub attempts: f64,
    #[serde(deserialize_with = "required")]
    pub last_attempt_at: Option<f64>,
    #[serde(deserialize_with = "required")]
    pub error: Option<String>,
    pub retryable: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `idleUpload()`.
impl Default for ServiceUploadState {
    fn default() -> Self {
        Self {
            status: UploadStatus::Idle,
            sermon_id: None,
            attempts: 0.0,
            last_attempt_at: None,
            error: None,
            retryable: true,
            extra: Extra::new(),
        }
    }
}

string_enum!(
    pub enum ServiceStatus {
        Live = "live",
        Ended = "ended",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRecord {
    pub id: String,
    pub title: String,
    pub speaker: String,
    /// Named when the service was saved; written only when given.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sermon_title: String,
    pub created_at: f64,
    #[serde(deserialize_with = "required")]
    pub ended_at: Option<f64>,
    pub status: ServiceStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub paused_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    pub paused_at: Option<Option<f64>>,
    pub transcript: Vec<TranscriptResult>,
    pub scriptures: Vec<ScriptureSuggestion>,
    pub notes: Vec<SermonPlan>,
    pub nuggets: Vec<ServiceNugget>,
    #[serde(deserialize_with = "required")]
    pub analysis_error: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub analyzed_source_ids: Option<Vec<String>>,
    pub upload: ServiceUploadState,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Persisted as `kairo-services.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSnapshot {
    #[serde(deserialize_with = "required")]
    pub active_id: Option<String>,
    pub services: Vec<ServiceRecord>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `ServiceCommand`, tagged by `action`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum ServiceCommand {
    List,
    Start,
    #[serde(rename_all = "camelCase")]
    End {
        title: String,
        speaker: String,
        /// The sermon's own title; the recap uses it as its headline.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        sermon_title: String,
        #[serde(deserialize_with = "required")]
        plan_id: Option<String>,
    },
    Discard,
    #[serde(rename_all = "camelCase")]
    Analyze {
        service_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Nugget {
        text: String,
        source_ids: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveNugget {
        service_id: String,
        nugget_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Upload {
        service_id: String,
    },
}

pub const MAX_TRANSCRIPT_WORDS: usize = 60_000;
const MAX_SEGMENT_CHARS: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TranscriptTooLongError;

impl std::fmt::Display for TranscriptTooLongError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("That service is too long to publish. Record long services in separate parts.")
    }
}

impl std::error::Error for TranscriptTooLongError {}

pub fn to_sermon_upload(
    record: &ServiceRecord,
    now_ms: f64,
) -> Result<crate::cloud::SermonUploadInput, TranscriptTooLongError> {
    use crate::cloud::{
        SermonUploadInput, SermonUploadScripture, SermonUploadSegment, SermonUploadWord,
    };
    let mut seen = std::collections::HashSet::new();
    let mut transcript = Vec::new();
    let mut count = 0;
    for segment in &record.transcript {
        let text = crate::js::trim(&segment.text);
        if text.is_empty() || !seen.insert(&segment.id) {
            continue;
        }
        count += text
            .split(crate::js::is_whitespace)
            .filter(|part| !part.is_empty())
            .count();
        if count > MAX_TRANSCRIPT_WORDS {
            return Err(TranscriptTooLongError);
        }
        let mut units = 0;
        let text = text
            .chars()
            .take_while(|c| {
                units += c.len_utf16();
                units <= MAX_SEGMENT_CHARS
            })
            .collect();
        transcript.push(SermonUploadSegment {
            id: segment.id.clone(),
            text,
            timestamp: segment.timestamp,
            duration: segment.duration,
            words: segment
                .words
                .iter()
                .map(|word| SermonUploadWord {
                    word: word.word.clone(),
                    start: word.start,
                    end: word.end,
                    confidence: word.confidence,
                })
                .collect(),
        });
    }
    Ok(SermonUploadInput {
        local_id: record.id.clone(),
        title: record.title.clone(),
        speaker: record.speaker.clone(),
        sermon_title: record.sermon_title.clone(),
        started_at: record.created_at,
        ended_at: record.ended_at.unwrap_or(now_ms),
        transcript,
        scriptures: record
            .scriptures
            .iter()
            .map(|item| SermonUploadScripture {
                reference: item.reference.clone(),
                translation: item.translation.clone(),
            })
            .collect(),
    })
}

pub fn should_retry_upload(status: Option<u16>) -> bool {
    status.is_none_or(|status| matches!(status, 408 | 429) || status >= 500)
}

/// Waits after the 1st, 2nd, … failed upload: 30 s, 2 min, 10 min, 30 min, then hourly.
pub const UPLOAD_BACKOFF: crate::time::Backoff = crate::time::Backoff::Steps(&[
    std::time::Duration::from_secs(30),
    std::time::Duration::from_secs(120),
    std::time::Duration::from_secs(600),
    std::time::Duration::from_secs(1_800),
    std::time::Duration::from_secs(3_600),
]);

pub fn next_upload_backoff_ms(attempts: usize) -> u64 {
    let retry = u32::try_from(attempts.saturating_sub(1)).unwrap_or(u32::MAX);
    UPLOAD_BACKOFF.delay(retry).as_millis() as u64
}

pub fn service_talk_time_ms(record: &ServiceRecord, now_ms: f64) -> f64 {
    let open_pause = record
        .paused_at
        .flatten()
        .filter(|_| record.ended_at.is_none())
        .map_or(0.0, |start| now_ms - start);
    crate::js::math_max(
        0.0,
        record.ended_at.unwrap_or(now_ms)
            - record.created_at
            - record.paused_ms.unwrap_or(0.0)
            - open_pause,
    )
}

pub fn extract_nugget_quotes(
    raw: &str,
    transcript: &[crate::transcript::TranscriptResult],
) -> Result<Vec<String>, &'static str> {
    fn collapse(text: &str) -> String {
        text.split(crate::js::is_whitespace)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
    let cleaned = crate::js::trim(raw);
    let cleaned = if let Some(rest) = cleaned.strip_prefix("```") {
        let offset = usize::from(
            rest.get(..4)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json")),
        ) * 4;
        &rest[offset..]
    } else {
        cleaned
    };
    let cleaned = crate::js::trim(cleaned.strip_suffix("```").unwrap_or(cleaned));
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    let range = cleaned
        .find('[')
        .zip(cleaned.rfind(']'))
        .filter(|(first, last)| first <= last)
        .ok_or("Nugget selection returned incomplete JSON. Retry selection.")?;
    let parsed: serde_json::Value = serde_json::from_str(&cleaned[range.0..=range.1])
        .map_err(|_| "Nugget selection returned invalid JSON. Retry selection.")?;
    let values = parsed.as_array().ok_or("Invalid nugget response")?;
    let source = collapse(
        &transcript
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );
    let mut seen = std::collections::HashSet::new();
    Ok(values
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(collapse)
        .filter(|item| {
            (35..=1200).contains(&item.encode_utf16().count())
                && source.contains(item)
                && seen.insert(item.clone())
        })
        .take(3)
        .collect())
}

/// The caller supplies the locale's date and time formatter.
pub fn default_service_title(
    created_at: f64,
    format_date_time: impl FnOnce(f64) -> String,
) -> String {
    format!("Service — {}", format_date_time(created_at))
}

/// Render a text export using the operator's local date and time formatters.
pub fn service_text_export(
    service: &ServiceRecord,
    format_date_time: impl FnOnce(f64) -> String,
    format_time: impl Fn(f64) -> String,
) -> String {
    let nuggets = service
        .nuggets
        .iter()
        .map(|n| format!("{}\n{}", n.text, format_time(n.captured_at)))
        .collect::<Vec<_>>()
        .join("\n\n");
    let notes = service
        .notes
        .iter()
        .map(|note| {
            let body = note.source_text.clone().unwrap_or_else(|| {
                note.items
                    .iter()
                    .map(|item| item.reference.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            format!("{}\n{body}", note.title)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let scriptures = service
        .scriptures
        .iter()
        .map(|s| format!("{} ({})", s.reference, s.translation))
        .collect::<Vec<_>>()
        .join("\n");
    let transcript = service
        .transcript
        .iter()
        .map(|s| format!("{}  {}", format_time(s.timestamp), s.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "{}\n{}\n{}\n\nNUGGETS\n{nuggets}\n\nSERMON NOTES\n{notes}\n\nDETECTED SCRIPTURES\n{scriptures}\n\nTRANSCRIPT\n{transcript}",
        service.title,
        service.speaker,
        format_date_time(service.created_at)
    )
}
