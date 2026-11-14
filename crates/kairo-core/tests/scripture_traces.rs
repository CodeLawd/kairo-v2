use kairo_core::scripture_traces::ScriptureTraces;
use kairo_domain::scripture::trace::{ScriptureTraceStage, TraceStartInput};
#[test]
fn rendered_marks_are_scoped_to_known_traces_and_recent_metrics_are_real() {
    let traces = ScriptureTraces::default();
    let input: TraceStartInput = serde_json::from_value(serde_json::json!({"correlationId":"ignored", "sourceText":"John 3:16", "transcriptSource":"final", "resolver":"explicit", "reference":"John 3:16", "sttReceivedAt":1})).unwrap();
    let id = traces.start(input);
    traces.mark(Some(&id), ScriptureTraceStage::SuggestionRenderedAt);
    traces.mark(Some("missing"), ScriptureTraceStage::SuggestionRenderedAt);
    let recent = traces.recent_with_metrics(None);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].trace.correlation_id, id);
    assert!(recent[0].trace.suggestion_rendered_at.is_some());
    assert_eq!(recent[0].trace.reference.as_deref(), Some("John 3:16"));
    assert!(traces.recent_with_metrics(Some(0)).is_empty());
}
