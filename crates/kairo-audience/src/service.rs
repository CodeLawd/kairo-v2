//! `service.ts`: queue ownership, independent of the window and of each
//! remote source.
//!
//! Output calls (showing or clearing the question and poll layers) run after
//! the state lock is released, because the program service reports a clear
//! straight back through [`AudienceService::sync_cleared`].

use crate::source::{AudienceSource, AudienceUpdate, UpdateSink};
use crate::{AudienceError, Result, lock, slice_utf16};
use kairo_domain::audience::{
    AudienceMergeMode, AudienceQuestionState, AudienceRoomSettings, AudienceSourceState,
    AudienceState, EngageCommand, PollView, merge_audience_questions, normalize_audience_text,
    set_audience_question_state,
};
use kairo_domain::program::{JoinTag, ProgramQuestion};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The program layer the service drives.
pub trait QuestionOutput: Send + Sync {
    fn show_question(&self, question: ProgramQuestion);
    fn clear_question(&self);
    fn show_poll(&self, _poll: &PollView) {}
    fn clear_poll(&self) {}
}

enum Action {
    ShowQuestion(ProgramQuestion),
    ClearQuestion,
    ShowPoll(PollView),
    ClearPoll,
}

struct Entry {
    source: Arc<dyn AudienceSource>,
    /// Cleared to unsubscribe: the sink ignores updates from then on.
    active: Arc<AtomicBool>,
}

impl Entry {
    fn unsubscribe(&self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

struct Inner {
    state: AudienceState,
    sources: HashMap<String, Entry>,
    listeners: kairo_core::Listeners<AudienceState>,
}

/// What a locked step leaves to do once the lock is gone.
#[derive(Default)]
struct After {
    actions: Vec<Action>,
    emit: bool,
}

impl After {
    fn clear(&mut self, state: &mut AudienceState) {
        self.actions.push(Action::ClearQuestion);
        state.live_id = None;
        self.emit = true;
    }

    fn clear_poll(&mut self, state: &mut AudienceState) {
        self.actions.push(Action::ClearPoll);
        state.live_poll_id = Some(None);
        self.emit = true;
    }
}

#[derive(Clone)]
pub struct AudienceService {
    inner: Arc<Mutex<Inner>>,
    output: Arc<dyn QuestionOutput>,
}

impl AudienceService {
    pub fn new(output: Arc<dyn QuestionOutput>, sample_available: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                state: AudienceState {
                    engage: None,
                    live_poll_id: None,
                    sample_available,
                    questions: Vec::new(),
                    sources: Vec::new(),
                    live_id: None,
                    session: None,
                },
                sources: HashMap::new(),
                listeners: kairo_core::Listeners::default(),
            })),
            output,
        }
    }

    /// A copy: callers can never change the queue through it.
    pub fn state(&self) -> AudienceState {
        lock(&self.inner).state.clone()
    }

    /// Returns an id for [`Self::remove_listener`].
    pub fn on_changed(&self, listener: impl Fn(&AudienceState) + Send + Sync + 'static) -> u64 {
        lock(&self.inner).listeners.add(listener).0
    }

    pub fn remove_listener(&self, id: u64) {
        lock(&self.inner)
            .listeners
            .remove(kairo_core::ListenerId(id));
    }

    fn finish(&self, after: After) -> AudienceState {
        for action in after.actions {
            match action {
                Action::ShowQuestion(q) => self.output.show_question(q),
                Action::ClearQuestion => self.output.clear_question(),
                Action::ShowPoll(p) => self.output.show_poll(&p),
                Action::ClearPoll => self.output.clear_poll(),
            }
        }
        let (state, listeners) = {
            let inner = lock(&self.inner);
            (inner.state.clone(), inner.listeners.clone())
        };
        if after.emit {
            listeners.notify(&state);
        }
        state
    }

    fn emitting(&self) -> AudienceState {
        self.finish(After {
            emit: true,
            ..After::default()
        })
    }

    /// `true` when the source with `id` should drop the live layer as it goes.
    fn holds_live(state: &AudienceState, id: &str) -> bool {
        (id == "kairo" && state.live_id.as_deref() == Some("join"))
            || state
                .questions
                .iter()
                .any(|q| Some(&q.id) == state.live_id.as_ref() && q.source_id == id)
    }

    fn on_update(&self, id: &str, update: AudienceUpdate) {
        let mut after = After {
            emit: true,
            ..After::default()
        };
        {
            let mut inner = lock(&self.inner);
            let inner = &mut *inner;
            let state = &mut inner.state;
            if let Some(engage) = update.engage {
                if let Some(Some(poll_id)) = &state.live_poll_id
                    && let Some(poll) = engage.polls.iter().find(|p| &p.poll.id == poll_id)
                {
                    after.actions.push(Action::ShowPoll(poll.clone()));
                }
                state.engage = Some(engage);
            }
            if let Some(questions) = update.questions {
                let prefix = format!("{id}:");
                let valid: Vec<_> = questions
                    .into_iter()
                    .filter_map(|mut q| {
                        let text =
                            normalize_audience_text(&serde_json::Value::String(q.text.clone()))?;
                        if q.source_id != id || !q.id.starts_with(&prefix) {
                            return None;
                        }
                        q.text = text;
                        q.author = q
                            .author
                            .as_deref()
                            .map(|a| slice_utf16(kairo_domain::js::trim(a), 80))
                            .filter(|a| !a.is_empty());
                        q.votes = if q.votes.is_finite() {
                            q.votes.floor().max(0.0)
                        } else {
                            0.0
                        };
                        Some(q)
                    })
                    .collect();
                state.questions = merge_audience_questions(
                    &state.questions,
                    &valid,
                    update.moderation.unwrap_or(AudienceMergeMode::Review),
                );
            }
            let stopped = update
                .status
                .as_ref()
                .is_some_and(|s| s.state == AudienceSourceState::Stopped);
            if let Some(status) = update.status {
                state.sources.retain(|s| s.id != id);
                state.sources.push(status);
            }
            if let Some(session) = update.session {
                if let Some(next) = &session
                    && Some(&next.room_id) != state.session.as_ref().map(|s| &s.room_id)
                {
                    // Previous rooms stay private to their own session; never send
                    // their moderation ids to a new room under the same source name.
                    state.questions.retain(|q| q.source_id != id);
                }
                state.session = session;
            }
            if stopped {
                if let Some(entry) = inner.sources.remove(id) {
                    entry.unsubscribe();
                }
                if id == "kairo" {
                    state.session = None;
                    state.engage = None;
                    after.clear_poll(state);
                }
                if Self::holds_live(state, id) {
                    after.clear(state);
                }
                state.questions.retain(|q| q.source_id != id);
            }
        }
        self.finish(after);
    }

    fn sink(&self, id: String, active: Arc<AtomicBool>) -> UpdateSink {
        let weak = Arc::downgrade(&self.inner);
        let output = self.output.clone();
        Arc::new(move |update| {
            if !active.load(Ordering::SeqCst) {
                return;
            }
            if let Some(inner) = weak.upgrade() {
                AudienceService {
                    inner,
                    output: output.clone(),
                }
                .on_update(&id, update);
            }
        })
    }

    /// Starting a source that is already running changes nothing.
    pub async fn start(&self, source: Arc<dyn AudienceSource>) -> Result<AudienceState> {
        let id = source.id().to_owned();
        let active = Arc::new(AtomicBool::new(true));
        {
            let mut inner = lock(&self.inner);
            if inner.sources.contains_key(&id) {
                return Ok(inner.state.clone());
            }
            inner.sources.insert(
                id.clone(),
                Entry {
                    source: source.clone(),
                    active: active.clone(),
                },
            );
        }
        source.subscribe(self.sink(id.clone(), active.clone()));
        if let Err(error) = source.start().await {
            active.store(false, Ordering::SeqCst);
            lock(&self.inner).sources.remove(&id);
            let _ = source.stop().await;
            return Err(error);
        }
        Ok(self.emitting())
    }

    /// Quit closes local windows and sockets even when the room is
    /// unreachable; it never waits for a remote end.
    pub fn shutdown(&self) {
        let entries: Vec<Entry> = {
            let mut inner = lock(&self.inner);
            let entries = inner.sources.drain().map(|(_, e)| e).collect();
            let state = &mut inner.state;
            state.live_id = None;
            state.session = None;
            for source in &mut state.sources {
                source.state = AudienceSourceState::Stopped;
            }
            entries
        };
        for entry in entries {
            entry.unsubscribe();
            entry.source.dispose();
        }
    }

    pub fn open(&self, id: &str) -> AudienceState {
        let source = lock(&self.inner).sources.get(id).map(|e| e.source.clone());
        if let Some(source) = source {
            source.open();
        }
        self.state()
    }

    fn source(&self, id: &str) -> Option<Arc<dyn AudienceSource>> {
        lock(&self.inner).sources.get(id).map(|e| e.source.clone())
    }

    pub async fn stop(&self, id: &str) -> Result<AudienceState> {
        let Some(source) = self.source(id) else {
            return Ok(self.state());
        };
        source.stop().await?;
        let mut after = After {
            emit: true,
            ..After::default()
        };
        {
            let mut inner = lock(&self.inner);
            if let Some(entry) = inner.sources.remove(id) {
                entry.unsubscribe();
            }
            let state = &mut inner.state;
            for s in state.sources.iter_mut().filter(|s| s.id == id) {
                s.state = AudienceSourceState::Stopped;
            }
            if id == "kairo" {
                state.session = None;
                state.engage = None;
                after.clear_poll(state);
            }
            if Self::holds_live(state, id) {
                after.clear(state);
            }
            state.questions.retain(|q| q.source_id != id);
        }
        Ok(self.finish(after))
    }

    /// Lets go of the current engagement without ending it for the room,
    /// before another one is resumed or after it is deleted.
    pub fn detach_engagement(&self) {
        let mut after = After {
            emit: true,
            ..After::default()
        };
        let entry = {
            let mut inner = lock(&self.inner);
            let entry = inner.sources.remove("kairo");
            if let Some(entry) = &entry {
                entry.unsubscribe();
            }
            let state = &mut inner.state;
            after.clear(state);
            after.clear_poll(state);
            state.session = None;
            state.engage = None;
            state.questions.retain(|q| q.source_id != "kairo");
            state.sources.retain(|s| s.id != "kairo");
            entry
        };
        if let Some(entry) = entry {
            entry.source.dispose();
        }
        self.finish(after);
    }

    pub async fn command(&self, command: EngageCommand) -> Result<AudienceState> {
        let source = self.source("kairo");
        let pending = source.as_ref().and_then(|s| s.command(command));
        pending
            .ok_or_else(|| AudienceError::new("Open an engagement first."))?
            .await?;
        Ok(self.state())
    }

    pub async fn manage(&self, action: &str, body: serde_json::Value) -> Result<AudienceState> {
        let source = self.source("kairo");
        let pending = source
            .as_ref()
            .and_then(|s| s.manage(action.to_owned(), body));
        pending
            .ok_or_else(|| AudienceError::new("Open an engagement first."))?
            .await?;
        Ok(self.state())
    }

    pub fn show_poll(&self, id: &str) -> Result<AudienceState> {
        let mut after = After {
            emit: true,
            ..After::default()
        };
        {
            let mut inner = lock(&self.inner);
            let poll = inner
                .state
                .engage
                .as_ref()
                .and_then(|e| e.polls.iter().find(|p| p.poll.id == id))
                .cloned()
                .ok_or_else(|| AudienceError::new("Choose an existing poll."))?;
            after.actions.push(Action::ShowPoll(poll));
            inner.state.live_poll_id = Some(Some(id.to_owned()));
        }
        Ok(self.finish(after))
    }

    pub fn clear_poll(&self) -> AudienceState {
        let mut after = After::default();
        after.clear_poll(&mut lock(&self.inner).state);
        self.finish(after)
    }

    pub async fn set_room_settings(&self, settings: AudienceRoomSettings) -> Result<AudienceState> {
        let (source, has_session) = {
            let inner = lock(&self.inner);
            (
                inner.sources.get("kairo").map(|e| e.source.clone()),
                inner.state.session.is_some(),
            )
        };
        let pending = source
            .as_ref()
            .filter(|_| has_session)
            .and_then(|s| s.set_settings(settings));
        pending
            .ok_or_else(|| AudienceError::new("Start a Q&A first."))?
            .await?;
        Ok(self.state())
    }

    /// The join link of the running room: only ever this one, never a
    /// window-supplied URL.
    pub fn audience_page_url(&self) -> Result<String> {
        lock(&self.inner)
            .state
            .session
            .as_ref()
            .map(|s| s.url.clone())
            .ok_or_else(|| AudienceError::new("Start a Q&A first."))
    }

    /// `state` arrives as text from the window and is checked here.
    pub async fn moderate(&self, id: &str, state: &str) -> Result<AudienceState> {
        let state = AudienceQuestionState::from_name(state)
            .ok_or_else(|| AudienceError::new("Choose a valid question state."))?;
        let source = {
            let inner = lock(&self.inner);
            let question = inner
                .state
                .questions
                .iter()
                .find(|q| q.id == id)
                .ok_or_else(|| AudienceError::new("That question no longer exists."))?;
            inner
                .sources
                .get(&question.source_id)
                .map(|e| e.source.clone())
        };
        // Upstream first; while disconnected the source queues the desired state.
        if let Some(pending) = source
            .as_ref()
            .and_then(|s| s.set_state(id.to_owned(), state))
        {
            pending.await?;
        }
        let mut after = After {
            emit: true,
            ..After::default()
        };
        {
            let mut inner = lock(&self.inner);
            let questions = std::mem::take(&mut inner.state.questions);
            inner.state.questions = set_audience_question_state(questions, id, state);
            if state != AudienceQuestionState::Approved
                && inner.state.live_id.as_deref() == Some(id)
            {
                after.clear(&mut inner.state);
            }
        }
        Ok(self.finish(after))
    }

    pub fn show(&self, id: &str) -> Result<AudienceState> {
        let mut after = After {
            emit: true,
            ..After::default()
        };
        {
            let mut inner = lock(&self.inner);
            let question = inner
                .state
                .questions
                .iter()
                .find(|q| q.id == id && q.state == AudienceQuestionState::Approved)
                .ok_or_else(|| AudienceError::new("Approve this question before showing it."))?;
            after
                .actions
                .push(Action::ShowQuestion(ProgramQuestion::Question {
                    text: question.text.clone(),
                    author: question.author.clone(),
                }));
            inner.state.live_id = Some(id.to_owned());
        }
        Ok(self.finish(after))
    }

    pub fn clear(&self) -> AudienceState {
        let mut after = After::default();
        after.clear(&mut lock(&self.inner).state);
        self.finish(after)
    }

    /// Clear All, or another program control, took the layer down.
    pub fn sync_cleared(&self) {
        let changed = {
            let mut inner = lock(&self.inner);
            inner.state.live_id.take().is_some()
        };
        if changed {
            self.emitting();
        }
    }

    pub fn show_join_code(&self) -> Result<AudienceState> {
        let mut after = After {
            emit: true,
            ..After::default()
        };
        {
            let mut inner = lock(&self.inner);
            let room = inner
                .state
                .session
                .as_ref()
                .ok_or_else(|| AudienceError::new("Start a Q&A session first."))?;
            after
                .actions
                .push(Action::ShowQuestion(ProgramQuestion::Join {
                    kind: JoinTag::Join,
                    code: room.code.clone(),
                    url: room.url.clone(),
                    qr_data_url: room.qr_data_url.clone(),
                }));
            inner.state.live_id = Some("join".into());
        }
        Ok(self.finish(after))
    }
}
