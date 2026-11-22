//! Latency traces from detection to screen, from `src/lib/scripture-trace.ts`.

use crate::serde_util::string_enum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

string_enum!(
    pub enum ScriptureResolver {
        Explicit = "explicit",
        QuotationLocal = "quotation-local",
        SermonPlan = "sermon-plan",
        Ai = "ai",
        /// A bare verse or a resume, resolved against the service's readings.
        Reading = "reading",
    }
);
string_enum!(
    pub enum TranscriptSource {
        Interim = "interim",
        Final = "final",
    }
);
string_enum!(
    pub enum TraceStatus {
        Active = "active",
        Presented = "presented",
        Cancelled = "cancelled",
        Superseded = "superseded",
        Failed = "failed",
    }
);
string_enum!(
    pub enum TraceFailureReason {
        ReferenceNotResolved = "reference-not-resolved",
        VerseNotFound = "verse-not-found",
        TranslationUnavailable = "translation-unavailable",
        PresenterDisconnected = "presenter-disconnected",
        PresenterTimeout = "presenter-timeout",
        PresenterRejected = "presenter-rejected",
        ConfirmationTimeout = "confirmation-timeout",
        Unknown = "unknown",
    }
);
string_enum!(
    pub enum ConfirmationLevel {
        VisibleState = "visible-state",
        ActiveDocument = "active-document",
        ContentMatch = "content-match",
        RequestAccepted = "request-accepted",
        None = "none",
    }
);
string_enum!(
    pub enum PresenterConnectionMode {
        Connected = "connected",
        Degraded = "degraded",
        Disconnected = "disconnected",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureLatencyTrace {
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    pub source_text: String,
    pub transcript_source: TranscriptSource,
    pub resolver: ScriptureResolver,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_present_delay_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_connection_mode: Option<PresenterConnectionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_output_mode: Option<String>,
    pub status: TraceStatus,
    pub stt_received_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection_started_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_recognized_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bible_lookup_started_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bible_lookup_completed_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion_published_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion_rendered_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub countdown_started_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub countdown_completed_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_request_started_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_request_completed_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_state_check_started_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_state_confirmed_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<ConfirmationLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<TraceFailureReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlowestStage {
    pub name: String,
    pub duration_ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureLatencyMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bible_lookup_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion_publish_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion_render_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_visible_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparation_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub countdown_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_request_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_confirmation_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_to_presenter_request_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slowest_stage: Option<SlowestStage>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceStartInput {
    pub correlation_id: String,
    pub source_text: String,
    pub transcript_source: TranscriptSource,
    pub resolver: ScriptureResolver,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_present_delay_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_connection_mode: Option<PresenterConnectionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    pub stt_received_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection_started_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_recognized_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StageSummary {
    pub count: f64,
    pub min: f64,
    pub mean: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolverSummary {
    pub resolver: ScriptureResolver,
    pub transcript_source: TranscriptSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_present_delay_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presenter_connection_mode: Option<PresenterConnectionMode>,
    pub presented: f64,
    pub cancelled: f64,
    pub superseded: f64,
    pub failed: f64,
    /// Keyed by `ScriptureLatencyMetrics` field name (`detectionMs`, …).
    pub stages: BTreeMap<String, StageSummary>,
}

/// `ipc.ts` `ScriptureTraceRecord`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptureTraceRecord {
    pub trace: ScriptureLatencyTrace,
    pub metrics: ScriptureLatencyMetrics,
}
