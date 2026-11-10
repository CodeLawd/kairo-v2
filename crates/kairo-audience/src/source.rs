//! `source.ts`: what every audience source offers the service.

use crate::Result;
use kairo_domain::audience::{
    AudienceMergeMode, AudienceQuestion, AudienceQuestionState, AudienceRoomSettings,
    AudienceSession, AudienceSourceStatus, EngageCommand, EngageSnapshot,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One change from a source. `session` is `None` when the update says
/// nothing about it and `Some(None)` when the room went away.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudienceUpdate {
    pub engage: Option<EngageSnapshot>,
    pub questions: Option<Vec<AudienceQuestion>>,
    /// New questions keep the state the source gives them instead of entering review.
    pub moderation: Option<AudienceMergeMode>,
    pub status: Option<AudienceSourceStatus>,
    pub session: Option<Option<AudienceSession>>,
}

pub type UpdateSink = Arc<dyn Fn(AudienceUpdate) + Send + Sync>;

/// The optional TypeScript methods return `None` here when a source lacks them.
pub trait AudienceSource: Send + Sync {
    fn id(&self) -> &str;
    fn start(&self) -> BoxFuture<'_, Result<()>>;
    fn stop(&self) -> BoxFuture<'_, Result<()>>;
    /// The service listens through this; it gates the sink itself to unsubscribe.
    fn subscribe(&self, sink: UpdateSink);
    fn open(&self) {}
    fn dispose(&self) {}
    fn command(&self, _command: EngageCommand) -> Option<BoxFuture<'_, Result<()>>> {
        None
    }
    fn manage(
        &self,
        _action: String,
        _body: serde_json::Value,
    ) -> Option<BoxFuture<'_, Result<()>>> {
        None
    }
    fn set_state(
        &self,
        _question_id: String,
        _state: AudienceQuestionState,
    ) -> Option<BoxFuture<'_, Result<()>>> {
        None
    }
    fn set_settings(&self, _settings: AudienceRoomSettings) -> Option<BoxFuture<'_, Result<()>>> {
        None
    }
}

/// Listener list shared by the sources.
#[derive(Clone, Default)]
pub(crate) struct Sinks(Arc<std::sync::Mutex<Vec<UpdateSink>>>);

impl Sinks {
    pub fn add(&self, sink: UpdateSink) {
        crate::lock(&self.0).push(sink);
    }

    pub fn emit(&self, update: AudienceUpdate) {
        // Copy out so a sink may subscribe or emit without deadlocking.
        let sinks = crate::lock(&self.0).clone();
        for sink in sinks {
            sink(update.clone());
        }
    }
}
