//! Latency trace logic, from `src/lib/scripture-trace.ts` and
//! `src/lib/scripture-trace-store.ts`. The trace types are in
//! [`crate::scripture_trace`] (prompt 012) and keep the Electron JSON shape,
//! so the scripture latency analysis reads native traces unchanged.
//!
//! Every timestamp is wall-clock milliseconds (`Date.now()`): a trace spans
//! processes, and only wall clock is comparable across them.

use super::js;
pub use crate::scripture_trace::*;
use std::collections::{BTreeMap, HashMap};

/// Every point on the path, named for the field it sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptureTraceStage {
    DetectionStartedAt,
    CitationRecognizedAt,
    BibleLookupStartedAt,
    BibleLookupCompletedAt,
    SuggestionPublishedAt,
    SuggestionRenderedAt,
    CountdownStartedAt,
    CountdownCompletedAt,
    PresenterRequestStartedAt,
    PresenterRequestCompletedAt,
    PresenterStateCheckStartedAt,
    PresenterStateConfirmedAt,
}

impl ScriptureTraceStage {
    /// In the order they happen.
    pub const ALL: [Self; 12] = [
        Self::DetectionStartedAt,
        Self::CitationRecognizedAt,
        Self::BibleLookupStartedAt,
        Self::BibleLookupCompletedAt,
        Self::SuggestionPublishedAt,
        Self::SuggestionRenderedAt,
        Self::CountdownStartedAt,
        Self::CountdownCompletedAt,
        Self::PresenterRequestStartedAt,
        Self::PresenterRequestCompletedAt,
        Self::PresenterStateCheckStartedAt,
        Self::PresenterStateConfirmedAt,
    ];

    /// The trace field name, as the JSON writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DetectionStartedAt => "detectionStartedAt",
            Self::CitationRecognizedAt => "citationRecognizedAt",
            Self::BibleLookupStartedAt => "bibleLookupStartedAt",
            Self::BibleLookupCompletedAt => "bibleLookupCompletedAt",
            Self::SuggestionPublishedAt => "suggestionPublishedAt",
            Self::SuggestionRenderedAt => "suggestionRenderedAt",
            Self::CountdownStartedAt => "countdownStartedAt",
            Self::CountdownCompletedAt => "countdownCompletedAt",
            Self::PresenterRequestStartedAt => "presenterRequestStartedAt",
            Self::PresenterRequestCompletedAt => "presenterRequestCompletedAt",
            Self::PresenterStateCheckStartedAt => "presenterStateCheckStartedAt",
            Self::PresenterStateConfirmedAt => "presenterStateConfirmedAt",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stage| stage.as_str() == name)
    }

    fn field(self, trace: &mut ScriptureLatencyTrace) -> &mut Option<f64> {
        match self {
            Self::DetectionStartedAt => &mut trace.detection_started_at,
            Self::CitationRecognizedAt => &mut trace.citation_recognized_at,
            Self::BibleLookupStartedAt => &mut trace.bible_lookup_started_at,
            Self::BibleLookupCompletedAt => &mut trace.bible_lookup_completed_at,
            Self::SuggestionPublishedAt => &mut trace.suggestion_published_at,
            Self::SuggestionRenderedAt => &mut trace.suggestion_rendered_at,
            Self::CountdownStartedAt => &mut trace.countdown_started_at,
            Self::CountdownCompletedAt => &mut trace.countdown_completed_at,
            Self::PresenterRequestStartedAt => &mut trace.presenter_request_started_at,
            Self::PresenterRequestCompletedAt => &mut trace.presenter_request_completed_at,
            Self::PresenterStateCheckStartedAt => &mut trace.presenter_state_check_started_at,
            Self::PresenterStateConfirmedAt => &mut trace.presenter_state_confirmed_at,
        }
    }

    pub fn get(self, trace: &ScriptureLatencyTrace) -> Option<f64> {
        let t = trace;
        match self {
            Self::DetectionStartedAt => t.detection_started_at,
            Self::CitationRecognizedAt => t.citation_recognized_at,
            Self::BibleLookupStartedAt => t.bible_lookup_started_at,
            Self::BibleLookupCompletedAt => t.bible_lookup_completed_at,
            Self::SuggestionPublishedAt => t.suggestion_published_at,
            Self::SuggestionRenderedAt => t.suggestion_rendered_at,
            Self::CountdownStartedAt => t.countdown_started_at,
            Self::CountdownCompletedAt => t.countdown_completed_at,
            Self::PresenterRequestStartedAt => t.presenter_request_started_at,
            Self::PresenterRequestCompletedAt => t.presenter_request_completed_at,
            Self::PresenterStateCheckStartedAt => t.presenter_state_check_started_at,
            Self::PresenterStateConfirmedAt => t.presenter_state_confirmed_at,
        }
    }
}

fn since(from: Option<f64>, to: Option<f64>) -> Option<f64> {
    let delta = to? - from?;
    // A negative delta is clock skew; NaN is not a duration either.
    (delta >= 0.0).then_some(delta)
}

/// Durations are derived, never stored, so they cannot drift from the timestamps.
pub fn derive_metrics(trace: &ScriptureLatencyTrace) -> ScriptureLatencyMetrics {
    let t = trace;
    let received = Some(t.stt_received_at);
    let mut metrics = ScriptureLatencyMetrics {
        detection_ms: since(received, t.citation_recognized_at),
        bible_lookup_ms: since(t.bible_lookup_started_at, t.bible_lookup_completed_at),
        suggestion_publish_ms: since(t.bible_lookup_completed_at, t.suggestion_published_at),
        suggestion_render_ms: since(t.suggestion_published_at, t.suggestion_rendered_at),
        operator_visible_ms: since(received, t.suggestion_rendered_at),
        preparation_ms: since(received, t.countdown_started_at),
        countdown_ms: since(t.countdown_started_at, t.countdown_completed_at),
        presenter_request_ms: since(
            t.presenter_request_started_at,
            t.presenter_request_completed_at,
        ),
        presenter_confirmation_ms: since(
            t.presenter_state_check_started_at,
            t.presenter_state_confirmed_at,
        ),
        total_ms: since(received, t.presenter_state_confirmed_at),
        total_to_presenter_request_ms: since(received, t.presenter_request_completed_at),
        slowest_stage: None,
    };
    // The countdown is deliberate and nearly always largest; naming it slowest
    // would bury whatever actually went wrong.
    let candidates = [
        ("Detection", metrics.detection_ms),
        ("Bible lookup", metrics.bible_lookup_ms),
        ("Suggestion publish", metrics.suggestion_publish_ms),
        ("On-screen render", metrics.suggestion_render_ms),
        ("Output request", metrics.presenter_request_ms),
        ("Output confirmation", metrics.presenter_confirmation_ms),
    ];
    for (name, duration) in candidates {
        let Some(duration) = duration else { continue };
        if metrics
            .slowest_stage
            .as_ref()
            .is_none_or(|slowest| duration > slowest.duration_ms)
        {
            metrics.slowest_stage = Some(SlowestStage {
                name: name.into(),
                duration_ms: duration,
            });
        }
    }
    metrics
}

/// Stages that have been recorded, in pipeline order.
pub fn recorded_stages(trace: &ScriptureLatencyTrace) -> Vec<(ScriptureTraceStage, f64)> {
    ScriptureTraceStage::ALL
        .into_iter()
        .filter_map(|stage| stage.get(trace).map(|at| (stage, at)))
        .collect()
}

/// A trace stops advancing once it has ended.
pub fn is_terminal(status: TraceStatus) -> bool {
    status != TraceStatus::Active
}

/// Nearest-rank percentile of an ascending sample; 0 for an empty one.
/// `None` where the source reads past the array (a `NaN` fraction).
pub fn percentile(sorted: &[f64], fraction: f64) -> Option<f64> {
    if sorted.is_empty() {
        return Some(0.0);
    }
    let rank = js::min(
        js::max((fraction * sorted.len() as f64).ceil(), 1.0),
        sorted.len() as f64,
    );
    if rank.is_nan() {
        return None;
    }
    Some(sorted[rank as usize - 1])
}

type MetricField = fn(&ScriptureLatencyMetrics) -> Option<f64>;

const SUMMARISED_STAGES: [(&str, MetricField); 9] = [
    ("detectionMs", |m| m.detection_ms),
    ("bibleLookupMs", |m| m.bible_lookup_ms),
    ("suggestionPublishMs", |m| m.suggestion_publish_ms),
    ("suggestionRenderMs", |m| m.suggestion_render_ms),
    ("operatorVisibleMs", |m| m.operator_visible_ms),
    ("preparationMs", |m| m.preparation_ms),
    ("presenterRequestMs", |m| m.presenter_request_ms),
    ("presenterConfirmationMs", |m| m.presenter_confirmation_ms),
    ("totalToPresenterRequestMs", |m| {
        m.total_to_presenter_request_ms
    }),
];

/// `JSON.stringify` of an optional number: non-finite numbers and absence both
/// write `null`, and `-0` writes `0`.
fn number_key(value: Option<f64>) -> Option<u64> {
    value
        .filter(|v| v.is_finite())
        .map(|v| if v == 0.0 { 0 } else { v.to_bits() })
}

/// Rolls traces up per resolver and configuration, busiest first.
///
/// Only presented traces, and traces that reached the operator's screen, are
/// timed: a cancelled suggestion is the operator doing their job, and how
/// fast it appeared is still what is measured.
pub fn summarize_by_resolver(traces: &[ScriptureLatencyTrace]) -> Vec<ResolverSummary> {
    type Key = (
        ScriptureResolver,
        TranscriptSource,
        Option<String>,
        Option<u64>,
        Option<PresenterConnectionMode>,
    );
    let mut order: Vec<Key> = Vec::new();
    let mut buckets: HashMap<Key, Vec<&ScriptureLatencyTrace>> = HashMap::new();
    for trace in traces {
        let key = (
            trace.resolver,
            trace.transcript_source,
            trace.translation.clone(),
            number_key(trace.auto_present_delay_ms),
            trace.presenter_connection_mode,
        );
        buckets
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push(trace);
    }

    let mut summaries: Vec<ResolverSummary> = order
        .iter()
        .map(|key| {
            let bucket = &buckets[key];
            let first = bucket[0];
            let count = |status| bucket.iter().filter(|t| t.status == status).count() as f64;
            let timed: Vec<ScriptureLatencyMetrics> = bucket
                .iter()
                .filter(|t| {
                    t.status == TraceStatus::Presented || t.suggestion_rendered_at.is_some()
                })
                .map(|t| derive_metrics(t))
                .collect();
            let mut stages = BTreeMap::new();
            for (name, field) in SUMMARISED_STAGES {
                let mut values: Vec<f64> = timed.iter().filter_map(field).collect();
                if values.is_empty() {
                    continue;
                }
                js::sort_by_number(&mut values, |a, b| a - b);
                let sum = values.iter().fold(0.0, |sum, v| sum + v);
                let pct = |f| percentile(&values, f).unwrap_or(f64::NAN);
                stages.insert(
                    name.to_string(),
                    StageSummary {
                        count: values.len() as f64,
                        min: values[0],
                        mean: sum / values.len() as f64,
                        p50: pct(0.5),
                        p90: pct(0.9),
                        p95: pct(0.95),
                        p99: pct(0.99),
                        max: values[values.len() - 1],
                    },
                );
            }
            ResolverSummary {
                resolver: first.resolver,
                transcript_source: first.transcript_source,
                translation: first.translation.clone(),
                auto_present_delay_ms: first.auto_present_delay_ms,
                presenter_connection_mode: first.presenter_connection_mode,
                presented: count(TraceStatus::Presented),
                cancelled: count(TraceStatus::Cancelled),
                superseded: count(TraceStatus::Superseded),
                failed: count(TraceStatus::Failed),
                stages,
            }
        })
        .collect();
    js::sort_by_number(&mut summaries, |a, b| b.presented - a.presented);
    summaries
}

/// A service runs for hours; traces are for what just happened.
pub const MAX_RECENT_TRACES: usize = 500;

/// How a trace ended.
#[derive(Clone, Debug, PartialEq)]
pub struct CompleteInput {
    pub status: TraceStatus,
    /// How sure we are it reached the congregation; nothing claimed by default.
    pub confirmation: Option<ConfirmationLevel>,
    pub failure_reason: Option<TraceFailureReason>,
    pub error: Option<String>,
    pub superseded_by: Option<String>,
}

impl From<TraceStatus> for CompleteInput {
    fn from(status: TraceStatus) -> Self {
        Self {
            status,
            confirmation: None,
            failure_reason: None,
            error: None,
            superseded_by: None,
        }
    }
}

/// Configuration recorded on a trace after it starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraceAnnotation {
    pub translation: Option<String>,
    pub auto_present_delay_ms: Option<f64>,
    pub presenter_connection_mode: Option<PresenterConnectionMode>,
    pub presenter_output_mode: Option<String>,
}

/// Recent scripture traces in a fixed-size buffer. A plain data structure: no
/// timers, IPC or logging, which belong to the service wrapper (prompt 033).
#[derive(Clone, Debug)]
pub struct ScriptureTraceStore {
    traces: HashMap<String, ScriptureLatencyTrace>,
    /// Start order, oldest first. A restarted id appears twice, as in the source.
    order: Vec<String>,
    limit: usize,
}

impl Default for ScriptureTraceStore {
    fn default() -> Self {
        Self::new(MAX_RECENT_TRACES)
    }
}

impl ScriptureTraceStore {
    pub fn new(limit: usize) -> Self {
        Self {
            traces: HashMap::new(),
            order: Vec::new(),
            limit,
        }
    }

    pub fn start(&mut self, input: TraceStartInput) -> ScriptureLatencyTrace {
        let trace = ScriptureLatencyTrace {
            correlation_id: input.correlation_id,
            reference: input.reference,
            source_text: input.source_text,
            transcript_source: input.transcript_source,
            resolver: input.resolver,
            translation: input.translation,
            auto_present_delay_ms: input.auto_present_delay_ms,
            presenter_connection_mode: input.presenter_connection_mode,
            presenter_output_mode: None,
            status: TraceStatus::Active,
            stt_received_at: input.stt_received_at,
            detection_started_at: input.detection_started_at,
            citation_recognized_at: input.citation_recognized_at,
            bible_lookup_started_at: None,
            bible_lookup_completed_at: None,
            suggestion_published_at: None,
            suggestion_rendered_at: None,
            countdown_started_at: None,
            countdown_completed_at: None,
            presenter_request_started_at: None,
            presenter_request_completed_at: None,
            presenter_state_check_started_at: None,
            presenter_state_confirmed_at: None,
            confirmation: None,
            failure_reason: None,
            error: None,
            superseded_by: None,
        };
        self.traces
            .insert(trace.correlation_id.clone(), trace.clone());
        self.order.push(trace.correlation_id.clone());
        while self.order.len() > self.limit {
            let oldest = self.order.remove(0);
            self.traces.remove(&oldest);
        }
        trace
    }

    /// Sets the given configuration fields; `None` leaves a field as it is.
    pub fn annotate(&mut self, correlation_id: &str, fields: TraceAnnotation) {
        let Some(trace) = self.traces.get_mut(correlation_id) else {
            return;
        };
        if let Some(v) = fields.translation {
            trace.translation = Some(v);
        }
        if let Some(v) = fields.auto_present_delay_ms {
            trace.auto_present_delay_ms = Some(v);
        }
        if let Some(v) = fields.presenter_connection_mode {
            trace.presenter_connection_mode = Some(v);
        }
        if let Some(v) = fields.presenter_output_mode {
            trace.presenter_output_mode = Some(v);
        }
    }

    /// Records that a stage happened at `at`; the first observation wins.
    /// Ignored once the trace ended, except a late on-screen paint of a
    /// presented trace, which closes an independent stage.
    pub fn mark(&mut self, correlation_id: &str, stage: ScriptureTraceStage, at: f64) {
        let Some(trace) = self.traces.get_mut(correlation_id) else {
            return;
        };
        let late_paint = stage == ScriptureTraceStage::SuggestionRenderedAt
            && trace.status == TraceStatus::Presented;
        if is_terminal(trace.status) && !late_paint {
            return;
        }
        let field = stage.field(trace);
        if field.is_none() {
            *field = Some(at);
        }
    }

    /// [`Self::mark`] at the current wall-clock time.
    pub fn mark_now(&mut self, correlation_id: &str, stage: ScriptureTraceStage) {
        self.mark(correlation_id, stage, now_ms());
    }

    /// Ends a trace; the first completion wins.
    pub fn complete(&mut self, correlation_id: &str, input: impl Into<CompleteInput>) {
        let Some(trace) = self.traces.get_mut(correlation_id) else {
            return;
        };
        if is_terminal(trace.status) {
            return;
        }
        let next = input.into();
        trace.status = next.status;
        if next.confirmation.is_some() {
            trace.confirmation = next.confirmation;
        }
        if next.failure_reason.is_some() {
            trace.failure_reason = next.failure_reason;
        }
        // Empty strings are falsy in the source and leave the field alone.
        if let Some(error) = next.error.filter(|e| !e.is_empty()) {
            trace.error = Some(error);
        }
        if let Some(by) = next.superseded_by.filter(|e| !e.is_empty()) {
            trace.superseded_by = Some(by);
        }
    }

    pub fn get(&self, correlation_id: &str) -> Option<&ScriptureLatencyTrace> {
        self.traces.get(correlation_id)
    }

    /// Newest first, at most `limit` (the source defaults to 50).
    pub fn get_recent(&self, limit: usize) -> Vec<&ScriptureLatencyTrace> {
        self.order
            .iter()
            .rev()
            .filter_map(|id| self.traces.get(id))
            .take(limit)
            .collect()
    }

    pub fn metrics(&self, correlation_id: &str) -> Option<ScriptureLatencyMetrics> {
        self.traces.get(correlation_id).map(derive_metrics)
    }

    pub fn len(&self) -> usize {
        self.traces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.traces.is_empty()
    }

    pub fn clear(&mut self) {
        self.traces.clear();
        self.order.clear();
    }
}

/// `getRecent`'s default limit.
pub const DEFAULT_RECENT_LIMIT: usize = 50;

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}
