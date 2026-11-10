//! `sources/sample.ts`: demo questions. Started only in development with
//! `KAIRO_AUDIENCE_SAMPLE=1` (see [`crate::sample_allowed`]).

use crate::source::{AudienceSource, AudienceUpdate, BoxFuture, Sinks, UpdateSink};
use crate::{Result, now_ms};
use kairo_domain::audience::{
    AudienceQuestion, AudienceQuestionState, AudienceSourceState, AudienceSourceStatus,
    AudienceStatusKind,
};

#[derive(Default)]
pub struct SampleAudienceSource {
    sinks: Sinks,
}

impl SampleAudienceSource {
    pub fn new() -> Self {
        Self::default()
    }
}

fn status(state: AudienceSourceState, joined: Option<f64>) -> AudienceSourceStatus {
    AudienceSourceStatus {
        id: "sample".into(),
        kind: AudienceStatusKind::Kairo,
        label: "Sample questions".into(),
        state,
        step: None,
        error: None,
        joined,
        can_open: None,
        code: None,
        event_id: None,
        event_name: None,
        moderator: None,
        skip_review: None,
        updated_at: None,
    }
}

impl AudienceSource for SampleAudienceSource {
    fn id(&self) -> &str {
        "sample"
    }

    fn subscribe(&self, sink: UpdateSink) {
        self.sinks.add(sink);
    }

    fn start(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let now = now_ms();
            let questions = [
                (
                    "How can we put what we learned today into practice this week?",
                    Some("Ruth"),
                    8.0,
                ),
                ("Where can I find out more about volunteering?", None, 5.0),
                ("Will there be another Q&A next week?", Some("James"), 2.0),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (text, author, votes))| AudienceQuestion {
                id: format!("sample:{index}"),
                source_id: "sample".into(),
                text: text.into(),
                author: author.map(str::to_owned),
                votes,
                state: AudienceQuestionState::Review,
                original_text: None,
                edited_by: None,
                sent_by: None,
                sent_at: None,
                created_at: now + index as f64,
            })
            .collect();
            self.sinks.emit(AudienceUpdate {
                status: Some(status(AudienceSourceState::Live, Some(3.0))),
                questions: Some(questions),
                ..Default::default()
            });
            Ok(())
        })
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.sinks.emit(AudienceUpdate {
                status: Some(status(AudienceSourceState::Stopped, None)),
                ..Default::default()
            });
            Ok(())
        })
    }
}
