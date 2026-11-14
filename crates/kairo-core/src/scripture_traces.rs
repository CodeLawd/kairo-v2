//! The main process's view of scripture latency
//! (`src/main/services/scripture/trace.ts`).
//!
//! Thin on purpose: [`ScriptureTraceStore`] holds the data and does the
//! arithmetic; this adds what a service needs and a data structure should
//! not have: id generation, the clock and structured logging.

use kairo_domain::scripture::trace::{
    CompleteInput, ScriptureTraceRecord, ScriptureTraceStage, ScriptureTraceStore, TraceAnnotation,
    TraceStartInput, derive_metrics,
};
use std::sync::Mutex;

/// `getRecent`'s default limit.
pub const DEFAULT_RECENT: usize = 50;

/// Recent scripture traces, shared across services.
#[derive(Default)]
pub struct ScriptureTraces {
    store: Mutex<ScriptureTraceStore>,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// The log event a stage reports, if any.
fn stage_event(stage: ScriptureTraceStage) -> Option<&'static str> {
    use ScriptureTraceStage as S;
    Some(match stage {
        S::BibleLookupStartedAt => "scripture.lookup.started",
        S::BibleLookupCompletedAt => "scripture.lookup.completed",
        S::SuggestionPublishedAt => "scripture.suggestion.published",
        S::SuggestionRenderedAt => "scripture.suggestion.rendered",
        S::CountdownStartedAt => "scripture.countdown.started",
        S::CountdownCompletedAt => "scripture.countdown.completed",
        S::PresenterRequestStartedAt => "scripture.presenter.started",
        S::PresenterRequestCompletedAt => "scripture.presenter.completed",
        S::PresenterStateCheckStartedAt => "scripture.presenter.confirmation-started",
        S::PresenterStateConfirmedAt => "scripture.presenter.confirmed",
        _ => return None,
    })
}

impl ScriptureTraces {
    fn store(&self) -> std::sync::MutexGuard<'_, ScriptureTraceStore> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Begins tracing a reference that was actually recognised. A new id
    /// replaces the input's, and the citation is stamped now.
    pub fn start(&self, mut input: TraceStartInput) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        input.correlation_id = id.clone();
        input.citation_recognized_at = Some(now_ms());
        let reference = input.reference.clone();
        let resolver = input.resolver;
        self.store().start(input);
        tracing::info!(
            target: "scripture",
            event = "scripture.detected",
            correlation_id = %id,
            reference = reference.as_deref().unwrap_or(""),
            resolver = resolver.as_str(),
        );
        id
    }

    /// Records configuration on a trace; no id does nothing.
    pub fn annotate(&self, correlation_id: Option<&str>, fields: TraceAnnotation) {
        if let Some(id) = correlation_id {
            self.store().annotate(id, fields);
        }
    }

    /// Stamps a stage now. No id, or an unknown one, does nothing to the
    /// store, as the source's `Map` lookup does.
    pub fn mark(&self, correlation_id: Option<&str>, stage: ScriptureTraceStage) {
        let Some(id) = correlation_id else { return };
        self.store().mark_now(id, stage);
        if let Some(event) = stage_event(stage) {
            tracing::info!(target: "scripture", event, correlation_id = %id);
        }
    }

    /// Ends a trace and logs its totals.
    pub fn complete(&self, correlation_id: Option<&str>, input: CompleteInput) {
        let Some(id) = correlation_id else { return };
        let status = input.status;
        let mut store = self.store();
        store.complete(id, input);
        let Some(trace) = store.get(id) else { return };
        let metrics = derive_metrics(trace);
        tracing::info!(
            target: "scripture",
            event = format!("scripture.{}", status.as_str()),
            correlation_id = %id,
            reference = trace.reference.as_deref().unwrap_or(""),
            total_ms = ?metrics.total_ms.or(metrics.total_to_presenter_request_ms),
            preparation_ms = ?metrics.preparation_ms,
        );
    }

    /// Newest first with derived metrics, at most `limit` (default 50).
    pub fn recent_with_metrics(&self, limit: Option<usize>) -> Vec<ScriptureTraceRecord> {
        self.store()
            .get_recent(limit.unwrap_or(DEFAULT_RECENT))
            .into_iter()
            .map(|trace| ScriptureTraceRecord {
                trace: trace.clone(),
                metrics: derive_metrics(trace),
            })
            .collect()
    }
}
