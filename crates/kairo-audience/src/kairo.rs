//! `sources/kairo.ts`: the operator's socket to an Engage room.
//!
//! Rules kept from Electron:
//! - Reconnect after a close with `min(30 s, 1 s × 2^min(attempts, 5))` plus
//!   up to 500 ms of jitter; odd attempts use the fallback base when there is one.
//! - A ping every 15 s; no pong by the next tick terminates the socket.
//! - A 10 s handshake timeout and a `AUDIENCE_MAX_SNAPSHOT_BYTES` message cap.
//! - 404 or 410 on the handshake ends the session; 401 or 403 renews the
//!   operator credential once, then asks the operator to reopen.
//! - Commands, settings and end wait 8 s for their `ack`; moderation is
//!   queued (latest per question) and resent after every snapshot.
//! - The operator token stays here, never in a state snapshot.
//!
//! Incoming messages are parsed on the socket task, so a large snapshot never
//! holds the service.

use crate::qr::qr_data_url;
use crate::source::{AudienceSource, AudienceUpdate, BoxFuture, Sinks, UpdateSink};
use crate::{AudienceError, Result, lock, slice_utf16};
use futures_util::{SinkExt, StreamExt};
use kairo_domain::audience::{
    AUDIENCE_MAX_SNAPSHOT_BYTES, AudienceMergeMode, AudienceQuestion, AudienceQuestionState,
    AudienceRoomCreated, AudienceRoomSettings, AudienceSession, AudienceSourceState,
    AudienceSourceStatus, AudienceSourceStep, AudienceStatusKind, EngageCommand, EngageSnapshot,
    normalize_audience_room_settings,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

pub const HEARTBEAT: Duration = Duration::from_secs(15);
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
pub const ACK_TIMEOUT: Duration = Duration::from_secs(8);
pub const MANAGE_TIMEOUT: Duration = Duration::from_secs(90);

/// Reserves or renews the room (`engageRequest('post', …)`).
pub type RoomFetcher =
    Arc<dyn Fn() -> BoxFuture<'static, Result<AudienceRoomCreated>> + Send + Sync>;

const RECONNECTING: &str = "Reconnecting. Your questions are safe.";

struct Pending {
    request_id: String,
    id: String,
    state: AudienceQuestionState,
}

impl Pending {
    fn message(&self) -> String {
        json!({ "type": "state", "id": self.id, "state": self.state, "requestId": self.request_id })
            .to_string()
    }
}

struct SettingsAck {
    request_id: String,
    settings: AudienceRoomSettings,
    done: oneshot::Sender<()>,
}

#[derive(Default)]
struct State {
    /// Bumped for every socket; a stale socket's events are ignored.
    generation: u64,
    /// Sends to the current socket while it is open (`readyState === OPEN`).
    writer: Option<mpsc::UnboundedSender<Message>>,
    reconnect: Option<tokio::task::JoinHandle<()>>,
    attempts: u32,
    auth_recovery: bool,
    auth_recoveries: u32,
    stopped: bool,
    room: Option<AudienceRoomCreated>,
    session: Option<AudienceSession>,
    joined: f64,
    /// Insertion order is resend order.
    pending: Vec<Pending>,
    requests: HashMap<String, oneshot::Sender<std::result::Result<(), String>>>,
    end_ack: Option<(String, oneshot::Sender<()>)>,
    settings_ack: Option<SettingsAck>,
}

struct Shared {
    base_url: String,
    fallback_base_url: Option<String>,
    create_room: RoomFetcher,
    refresh_room: Option<RoomFetcher>,
    http: reqwest::Client,
    sinks: Sinks,
    state: Mutex<State>,
}

#[derive(Clone)]
pub struct KairoAudienceSource(Arc<Shared>);

fn random_below(n: u64) -> u64 {
    (uuid::Uuid::new_v4().as_u128() % u128::from(n)) as u64
}

fn request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `fetch`'s rejection text, so messages match what Electron showed.
fn fetch_error(error: &reqwest::Error) -> AudienceError {
    AudienceError::new(
        if kairo_net::transport::TransportError::from(error).is_timeout() {
            "The operation was aborted due to timeout"
        } else {
            "fetch failed"
        },
    )
}

/// `POST {base}/v1/engage/{code}/{action}` with the operator token and a
/// 90 s timeout. Shared with the Engage flows.
pub(crate) async fn operator_post(
    http: &reqwest::Client,
    base_url: &str,
    code: &str,
    action: &str,
    token: &str,
    body: &Value,
) -> Result<(bool, String)> {
    let url = url::Url::parse(base_url)
        .and_then(|base| base.join(&format!("/v1/engage/{code}/{action}")))
        .map_err(|error| AudienceError::new(error.to_string()))?;
    let response = http
        .post(url)
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .timeout(MANAGE_TIMEOUT)
        .send()
        .await
        .map_err(|e| fetch_error(&e))?;
    let ok = response.status().is_success();
    let text = response.text().await.map_err(|e| fetch_error(&e))?;
    Ok((ok, text))
}

impl KairoAudienceSource {
    pub fn new(
        base_url: impl Into<String>,
        create_room: RoomFetcher,
        fallback_base_url: Option<String>,
        refresh_room: Option<RoomFetcher>,
    ) -> Self {
        Self(Arc::new(Shared {
            base_url: base_url.into(),
            fallback_base_url,
            create_room,
            refresh_room,
            http: kairo_net::transport::client(),
            sinks: Sinks::default(),
            state: Mutex::new(State::default()),
        }))
    }

    fn is_open(&self) -> bool {
        lock(&self.0.state).writer.is_some()
    }
}

impl Shared {
    fn emit(&self, update: AudienceUpdate) {
        self.sinks.emit(update);
    }

    fn status(
        &self,
        state: AudienceSourceState,
        error: Option<&str>,
        step: Option<AudienceSourceStep>,
    ) {
        let joined = lock(&self.state).joined;
        self.emit(AudienceUpdate {
            status: Some(AudienceSourceStatus {
                id: "kairo".into(),
                kind: AudienceStatusKind::Kairo,
                label: "Engage Q&A".into(),
                state,
                step,
                error: error.map(str::to_owned),
                joined: Some(joined),
                can_open: None,
                code: None,
                event_id: None,
                event_name: None,
                moderator: None,
                skip_review: None,
                updated_at: None,
            }),
            ..Default::default()
        });
    }

    fn send(&self, message: String) -> bool {
        lock(&self.state)
            .writer
            .as_ref()
            .is_some_and(|w| w.send(Message::Text(message.into())).is_ok())
    }

    fn connect(self: &Arc<Self>) {
        let (generation, room, endpoint, attempts) = {
            let mut state = lock(&self.state);
            let Some(room) = state.room.clone().filter(|_| !state.stopped) else {
                return;
            };
            state.generation += 1;
            state.writer = None;
            let endpoint = match &self.fallback_base_url {
                Some(fallback) if state.attempts % 2 == 1 => fallback.clone(),
                _ => self.base_url.clone(),
            };
            (state.generation, room, endpoint, state.attempts)
        };
        self.status(
            AudienceSourceState::Connecting,
            (attempts > 0).then_some(RECONNECTING),
            Some(AudienceSourceStep::Connecting),
        );
        let this = self.clone();
        tokio::spawn(async move { this.run(generation, room, endpoint).await });
    }

    fn current(&self, generation: u64) -> bool {
        let state = lock(&self.state);
        !state.stopped && state.generation == generation
    }

    async fn run(self: Arc<Self>, generation: u64, room: AudienceRoomCreated, endpoint: String) {
        let url = url::Url::parse(&endpoint)
            .and_then(|base| base.join(&format!("/v1/engage/{}/ws?role=operator", room.code)));
        let mut url = match url {
            Ok(url) => url,
            Err(error) => {
                self.status(AudienceSourceState::Error, Some(&error.to_string()), None);
                return;
            }
        };
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        let _ = url.set_scheme(scheme);
        let host = url.host_str().unwrap_or_default().to_owned();

        let mut request = match url.as_str().into_client_request() {
            Ok(request) => request,
            Err(error) => {
                self.status(AudienceSourceState::Error, Some(&error.to_string()), None);
                return;
            }
        };
        if let Ok(value) = format!("Bearer {}", room.operator_token).parse() {
            request.headers_mut().insert("Authorization", value);
        }
        let config = WebSocketConfig::default()
            .max_message_size(Some(AUDIENCE_MAX_SNAPSHOT_BYTES))
            .max_frame_size(Some(AUDIENCE_MAX_SNAPSHOT_BYTES));
        let connected = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            tokio_tungstenite::connect_async_with_config(request, Some(config), false),
        )
        .await;
        let socket = match connected {
            Ok(Ok((socket, _))) => socket,
            Ok(Err(WsError::Http(response))) => {
                let status = response.status().as_u16();
                tracing::warn!(host, status, "[Engage] Socket handshake rejected");
                self.rejected(generation, status);
                return;
            }
            Ok(Err(error)) => {
                tracing::warn!(host, message = %error, "[Engage] Socket error");
                self.lost(generation);
                return;
            }
            Err(_) => {
                tracing::warn!(
                    host,
                    message = "Opening handshake has timed out",
                    "[Engage] Socket error"
                );
                self.lost(generation);
                return;
            }
        };

        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        {
            let mut state = lock(&self.state);
            if state.stopped || state.generation != generation {
                return;
            }
            state.writer = Some(tx);
        }
        let (mut write, mut read) = socket.split();
        let mut heartbeat =
            tokio::time::interval_at(tokio::time::Instant::now() + HEARTBEAT, HEARTBEAT);
        let mut responsive = true;
        let mut errored = false;
        loop {
            tokio::select! {
                outgoing = rx.recv() => match outgoing {
                    Some(message) => {
                        let closing = matches!(message, Message::Close(_));
                        if write.send(message).await.is_err() {
                            errored = true;
                            break;
                        }
                        if closing {
                            break;
                        }
                    }
                    None => break,
                },
                _ = heartbeat.tick() => {
                    if !responsive {
                        // `ws.terminate()`.
                        break;
                    }
                    responsive = false;
                    if write.send(Message::Ping(Vec::new().into())).await.is_err() {
                        errored = true;
                        break;
                    }
                }
                incoming = read.next() => match incoming {
                    Some(Ok(Message::Text(text))) => self.message(generation, &text),
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Ok(text) = std::str::from_utf8(&bytes) {
                            self.message(generation, text);
                        }
                    }
                    Some(Ok(Message::Pong(_))) => responsive = true,
                    Some(Ok(Message::Close(frame))) => {
                        tracing::info!(host, code = ?frame.as_ref().map(|f| u16::from(f.code)), "[Engage] Socket closed");
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::warn!(host, message = %error, "[Engage] Socket error");
                        errored = true;
                        break;
                    }
                    None => break,
                },
            }
        }
        if errored && self.current(generation) {
            self.status(
                AudienceSourceState::Error,
                Some("Lost the connection. Reconnecting…"),
                None,
            );
        }
        self.closed(generation);
    }

    /// A transport error before the socket opened, then its close.
    fn lost(self: &Arc<Self>, generation: u64) {
        if self.current(generation) {
            self.status(
                AudienceSourceState::Error,
                Some("Lost the connection. Reconnecting…"),
                None,
            );
        }
        self.closed(generation);
    }

    /// `ws.on('close')`: back off and try again, unless the source stopped,
    /// a newer socket exists, or the credential is being renewed.
    fn closed(self: &Arc<Self>, generation: u64) {
        let delay = {
            let mut state = lock(&self.state);
            if state.stopped || state.generation != generation {
                return;
            }
            state.writer = None;
            if state.auth_recovery {
                return;
            }
            let exponent = state.attempts.min(5);
            state.attempts += 1;
            Duration::from_millis((1000u64 << exponent).min(30_000) + random_below(500))
        };
        self.status(
            AudienceSourceState::Connecting,
            Some(RECONNECTING),
            Some(AudienceSourceStep::Connecting),
        );
        let this = self.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            this.connect();
        });
        let mut state = lock(&self.state);
        if let Some(old) = state.reconnect.replace(handle) {
            old.abort();
        }
    }

    /// `ws.on('unexpected-response')`.
    fn rejected(self: &Arc<Self>, generation: u64, status: u16) {
        if !self.current(generation) {
            return;
        }
        match status {
            404 | 410 => self.finish(),
            401 | 403 => {
                let refresh = {
                    let mut state = lock(&self.state);
                    state.auth_recovery = true;
                    match &self.refresh_room {
                        Some(refresh) if state.auth_recoveries < 1 => {
                            state.auth_recoveries += 1;
                            Some(refresh.clone())
                        }
                        _ => None,
                    }
                };
                let Some(refresh) = refresh else {
                    self.status(
                        AudienceSourceState::Error,
                        Some(
                            "Engage access was rejected. Open this engagement again to reconnect.",
                        ),
                        None,
                    );
                    return;
                };
                let this = self.clone();
                tokio::spawn(async move {
                    match refresh().await {
                        Ok(room) => {
                            {
                                let mut state = lock(&this.state);
                                if state.stopped {
                                    return;
                                }
                                state.room = Some(room);
                                state.auth_recovery = false;
                            }
                            this.connect();
                        }
                        Err(error) => {
                            tracing::warn!(message = %error, "[Engage] Could not renew operator access");
                            this.status(
                                AudienceSourceState::Error,
                                Some("Engage access expired. Open this engagement again to reconnect."),
                                None,
                            );
                        }
                    }
                });
            }
            // Terminated; the close that follows reconnects.
            _ => self.closed(generation),
        }
    }

    fn message(self: &Arc<Self>, generation: u64, text: &str) {
        if !self.current(generation) {
            return;
        }
        let Ok(event) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let questions = |value: &Value| -> Vec<AudienceQuestion> {
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|q| serde_json::from_value(q.clone()).ok())
                .collect()
        };
        match event["type"].as_str() {
            Some("engage") => {
                if let Ok(engage) = serde_json::from_value::<EngageSnapshot>(event) {
                    self.emit(AudienceUpdate {
                        engage: Some(engage),
                        ..Default::default()
                    });
                }
            }
            Some("snapshot") => {
                {
                    let mut state = lock(&self.state);
                    state.attempts = 0;
                    state.auth_recoveries = 0;
                    state.joined = event["joined"].as_f64().unwrap_or(0.0);
                }
                self.emit(AudienceUpdate {
                    questions: Some(questions(&event["questions"])),
                    moderation: Some(AudienceMergeMode::Authoritative),
                    ..Default::default()
                });
                let changed = {
                    let mut state = lock(&self.state);
                    let settings: Option<AudienceRoomSettings> =
                        serde_json::from_value(event["settings"].clone()).ok();
                    match (&mut state.session, settings) {
                        (Some(session), Some(settings)) if session.settings != settings => {
                            session.settings = settings;
                            Some(session.clone())
                        }
                        _ => None,
                    }
                };
                if let Some(session) = changed {
                    self.emit(AudienceUpdate {
                        session: Some(Some(session)),
                        ..Default::default()
                    });
                }
                self.status(AudienceSourceState::Live, None, None);
                let resend: Vec<String> = lock(&self.state)
                    .pending
                    .iter()
                    .map(Pending::message)
                    .collect();
                for message in resend {
                    self.send(message);
                }
            }
            Some("question") => {
                if let Ok(question) = serde_json::from_value(event["question"].clone()) {
                    self.emit(AudienceUpdate {
                        questions: Some(vec![question]),
                        moderation: Some(AudienceMergeMode::Authoritative),
                        ..Default::default()
                    });
                }
            }
            Some("joined") => {
                lock(&self.state).joined = event["joined"].as_f64().unwrap_or(0.0);
                self.status(AudienceSourceState::Live, None, None);
            }
            Some("ack") => {
                let Some(id) = event["requestId"].as_str() else {
                    return;
                };
                let settled = {
                    let mut state = lock(&self.state);
                    if let Some(request) = state.requests.remove(id) {
                        let _ = request.send(Ok(()));
                    }
                    state.pending.retain(|p| p.request_id != id);
                    if state.end_ack.as_ref().is_some_and(|(r, _)| r == id)
                        && let Some((_, done)) = state.end_ack.take()
                    {
                        let _ = done.send(());
                    }
                    match state.settings_ack.take() {
                        Some(ack) if ack.request_id == id => {
                            let session = state.session.as_mut().map(|session| {
                                session.settings = ack.settings.clone();
                                session.clone()
                            });
                            Some((ack.done, session))
                        }
                        other => {
                            state.settings_ack = other;
                            None
                        }
                    }
                };
                if let Some((done, session)) = settled {
                    if let Some(session) = session {
                        self.emit(AudienceUpdate {
                            session: Some(Some(session)),
                            ..Default::default()
                        });
                    }
                    let _ = done.send(());
                }
            }
            Some("ended") => self.finish(),
            Some("error") => {
                let message = event["message"].as_str().unwrap_or_default().to_owned();
                // A rejected moderation request concerns that question only;
                // the connection itself is still live.
                match event["requestId"].as_str().filter(|r| !r.is_empty()) {
                    Some(id) => {
                        let mut state = lock(&self.state);
                        if let Some(request) = state.requests.remove(id) {
                            let _ = request.send(Err(message));
                        }
                        state.pending.retain(|p| p.request_id != id);
                    }
                    None => self.status(AudienceSourceState::Error, Some(&message), None),
                }
            }
            _ => {}
        }
    }

    fn finish(&self) {
        {
            let mut state = lock(&self.state);
            state.stopped = true;
            if let Some(handle) = state.reconnect.take() {
                handle.abort();
            }
            if let Some(writer) = state.writer.take() {
                let _ = writer.send(Message::Close(None));
            }
            state.generation += 1;
            state.pending.clear();
            for (_, request) in state.requests.drain() {
                let _ = request.send(Err("Engagement disconnected.".into()));
            }
        }
        self.emit(AudienceUpdate {
            session: Some(None),
            ..Default::default()
        });
        self.status(AudienceSourceState::Stopped, None, None);
    }

    async fn await_ack<T>(
        rx: oneshot::Receiver<T>,
        timeout_message: &str,
        on_timeout: impl FnOnce(),
    ) -> std::result::Result<T, AudienceError> {
        match tokio::time::timeout(ACK_TIMEOUT, rx).await {
            Ok(Ok(value)) => Ok(value),
            // A dropped sender means the wait was abandoned; report it as Electron's timer would.
            _ => {
                on_timeout();
                Err(AudienceError::new(timeout_message))
            }
        }
    }
}

impl AudienceSource for KairoAudienceSource {
    fn id(&self) -> &str {
        "kairo"
    }

    fn subscribe(&self, sink: UpdateSink) {
        self.0.sinks.add(sink);
    }

    fn start(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let shared = &self.0;
            shared.status(
                AudienceSourceState::Connecting,
                None,
                Some(AudienceSourceStep::Reserving),
            );
            let room = (shared.create_room)().await?;
            let session = AudienceSession {
                room_id: room.room_id.clone(),
                code: room.code.clone(),
                url: room.url.clone(),
                title: room.title.clone(),
                settings: normalize_audience_room_settings(
                    &serde_json::to_value(&room.settings).unwrap_or_default(),
                ),
                qr_data_url: qr_data_url(&room.url)?,
            };
            {
                let mut state = lock(&shared.state);
                state.room = Some(room);
                state.session = Some(session.clone());
            }
            shared.emit(AudienceUpdate {
                session: Some(Some(session)),
                ..Default::default()
            });
            shared.connect();
            Ok(())
        })
    }

    fn command(&self, command: EngageCommand) -> Option<BoxFuture<'_, Result<()>>> {
        Some(Box::pin(async move {
            let shared = &self.0;
            if lock(&shared.state).stopped || !self.is_open() {
                return Err(AudienceError::new(
                    "Wait for Engage to reconnect, then try again.",
                ));
            }
            let id = request_id();
            let mut message = serde_json::to_value(&command)
                .map_err(|error| AudienceError::new(error.to_string()))?;
            message["requestId"] = Value::String(id.clone());
            let (tx, rx) = oneshot::channel();
            lock(&shared.state).requests.insert(id.clone(), tx);
            shared.send(message.to_string());
            match tokio::time::timeout(ACK_TIMEOUT, rx).await {
                Ok(Ok(Ok(()))) => Ok(()),
                Ok(Ok(Err(message))) => Err(AudienceError::new(message)),
                _ => {
                    lock(&shared.state).requests.remove(&id);
                    Err(AudienceError::new(
                        "That change did not go through. Try again.",
                    ))
                }
            }
        }))
    }

    fn manage(&self, action: String, body: Value) -> Option<BoxFuture<'_, Result<()>>> {
        Some(Box::pin(async move {
            let shared = &self.0;
            let room = {
                let state = lock(&shared.state);
                state.room.clone().filter(|_| !state.stopped)
            };
            let room = room.ok_or_else(|| AudienceError::new("Open an engagement first."))?;
            let (ok, text) = operator_post(
                &shared.http,
                &shared.base_url,
                &room.code,
                &action,
                &room.operator_token,
                &body,
            )
            .await?;
            if ok {
                return Ok(());
            }
            let text = slice_utf16(&text, 300);
            Err(AudienceError::new(if text.is_empty() {
                "Could not complete that change.".to_owned()
            } else {
                text
            }))
        }))
    }

    fn set_state(
        &self,
        question_id: String,
        state: AudienceQuestionState,
    ) -> Option<BoxFuture<'_, Result<()>>> {
        Some(Box::pin(async move {
            let shared = &self.0;
            let message = {
                let mut inner = lock(&shared.state);
                if inner.stopped {
                    return Err(AudienceError::new("This session has ended."));
                }
                // Keep only the latest moderation for a question during an outage.
                inner.pending.retain(|p| p.id != question_id);
                let pending = Pending {
                    request_id: request_id(),
                    id: question_id,
                    state,
                };
                let message = pending.message();
                inner.pending.push(pending);
                message
            };
            shared.send(message);
            Ok(())
        }))
    }

    fn set_settings(&self, settings: AudienceRoomSettings) -> Option<BoxFuture<'_, Result<()>>> {
        Some(Box::pin(async move {
            let shared = &self.0;
            if lock(&shared.state).stopped || !self.is_open() {
                return Err(AudienceError::new(
                    "Wait until the Q&A is connected, then try again.",
                ));
            }
            let id = request_id();
            let (tx, rx) = oneshot::channel();
            lock(&shared.state).settings_ack = Some(SettingsAck {
                request_id: id.clone(),
                settings: settings.clone(),
                done: tx,
            });
            shared.send(
                json!({ "type": "settings", "settings": settings, "requestId": id }).to_string(),
            );
            Shared::await_ack(rx, "That change didn’t go through. Try again.", || {
                lock(&shared.state).settings_ack = None;
            })
            .await
        }))
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let shared = &self.0;
            {
                let state = lock(&shared.state);
                if state.stopped {
                    return Ok(());
                }
                if state.room.is_none() {
                    drop(state);
                    shared.finish();
                    return Ok(());
                }
            }
            if !self.is_open() {
                return Err(AudienceError::new(
                    "Wait until the Q&A reconnects, then end it, so everyone sees it has ended.",
                ));
            }
            let id = request_id();
            let (tx, rx) = oneshot::channel();
            lock(&shared.state).end_ack = Some((id.clone(), tx));
            shared.send(json!({ "type": "end", "requestId": id }).to_string());
            Shared::await_ack(
                rx,
                "Couldn’t end the Q&A. Check your connection and try again.",
                || {
                    lock(&shared.state).end_ack = None;
                },
            )
            .await?;
            shared.finish();
            Ok(())
        })
    }

    fn dispose(&self) {
        self.0.finish();
    }
}
