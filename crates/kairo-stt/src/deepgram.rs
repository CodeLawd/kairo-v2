//! Deepgram streaming recognition. From `src/main/services/stt/deepgram.ts`.
//!
//! One actor task owns the socket, the 30 s replay buffer, the licence grant
//! and every timer. Each open attempt bumps the connection generation; a
//! grant, socket or message from an older generation is released or ignored,
//! so a stale connection can never transcribe into a newer session. Finals and
//! interims carry the generation as their audio epoch, because provider
//! offsets restart on every connection.
//!
//! The query string matches what `@deepgram/sdk` 5 sends (checked against a
//! local socket server); see DECISIONS.md (038).

use crate::access::TranscriptionAccess;
use crate::relay::PcmStream;
use futures_util::{SinkExt, StreamExt};
use kairo_domain::cloud::TranscriptionGrant;
use kairo_domain::transcript::{InterimResult, TranscriptResult, TranscriptWord};
use kairo_net::ApiError;
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub const DEEPGRAM_ENDPOINT: &str = "wss://api.deepgram.com/v1/listen";
/// 30 s of 16 kHz mono PCM16 kept while the socket is down.
pub const REPLAY_BUFFER_BYTES: usize = 960_000;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Every delay the connection uses. Defaults are Electron's.
#[derive(Clone, Debug)]
pub struct Timings {
    pub reconnect_base: Duration,
    pub reconnect_max: Duration,
    pub reconnect_tries: u32,
    pub keepalive: Duration,
    /// No frame from the server for this long means a half-open socket.
    pub idle_timeout: Duration,
    /// Wait while another computer's session lapses.
    pub session_wait: Duration,
    pub session_wait_tries: u32,
    /// Retries for a failed renewal, inside the lease's slack.
    pub renewal_retries: [Duration; 3],
    /// One second of `renewAfterSeconds`.
    pub renew_unit: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            reconnect_base: Duration::from_secs(1),
            reconnect_max: Duration::from_secs(30),
            reconnect_tries: 20,
            keepalive: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(30),
            session_wait: Duration::from_secs(30),
            session_wait_tries: 8,
            renewal_retries: [5, 15, 30].map(Duration::from_secs),
            renew_unit: Duration::from_secs(1),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SttEvent {
    Interim(InterimResult),
    Final(TranscriptResult),
    UtteranceEnd(f64),
    /// `fatal` when transcription has stopped and will not retry.
    Error {
        message: String,
        fatal: bool,
    },
    Connected,
    Disconnected,
    Reconnecting {
        attempt: u32,
        delay_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SessionStats {
    pub total_words: u64,
    pub average_confidence: f64,
    pub uptime_ms: f64,
    pub reconnect_count: u64,
}

#[derive(Default)]
struct StatsInner {
    started_at: Option<std::time::Instant>,
    total_words: u64,
    confidence_sum: f64,
    confidence_count: u64,
    reconnect_count: u64,
}

struct Shared {
    connected: AtomicBool,
    active: AtomicBool,
    stats: Mutex<StatsInner>,
    replay_bytes: std::sync::atomic::AtomicUsize,
}

enum Command {
    Configure {
        language: String,
        keyterms: Vec<String>,
    },
    Connect(oneshot::Sender<()>),
    Disconnect,
    Attach(PcmStream),
    Detach,
}

/// Handle to the connection actor. Cloning shares it.
#[derive(Clone)]
pub struct DeepgramStt {
    commands: mpsc::UnboundedSender<Command>,
    events: broadcast::Sender<SttEvent>,
    shared: Arc<Shared>,
}

impl DeepgramStt {
    /// Spawns the actor on the current tokio runtime.
    pub fn new(access: Arc<dyn TranscriptionAccess>) -> Self {
        Self::with_endpoint(access, DEEPGRAM_ENDPOINT.into(), Timings::default())
    }

    pub fn with_endpoint(
        access: Arc<dyn TranscriptionAccess>,
        endpoint: String,
        timings: Timings,
    ) -> Self {
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(512);
        let shared = Arc::new(Shared {
            connected: false.into(),
            active: false.into(),
            stats: Mutex::new(StatsInner::default()),
            replay_bytes: 0.into(),
        });
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        let actor = Actor {
            access,
            endpoint,
            timings,
            events: events.clone(),
            shared: shared.clone(),
            internal: internal_tx,
            generation: 0,
            grant_id: None,
            refused_once: false,
            session_waits: 0,
            renewal: None,
            language: "en".into(),
            keyterms: Vec::new(),
            socket: None,
            audio: None,
            active: false,
            destroyed: false,
            reconnect_attempt: 0,
            reconnect: None,
            keepalive_at: None,
            last_frame_at: Instant::now(),
            buffer: VecDeque::new(),
            buffer_bytes: 0,
            connect_waiters: Vec::new(),
        };
        tokio::spawn(actor.run(command_rx, internal_rx));
        Self {
            commands,
            events,
            shared,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SttEvent> {
        self.events.subscribe()
    }

    /// Keyterms are fixed per connection; a change applies from the next one.
    pub fn configure(&self, language: &str, keyterms: Vec<String>) {
        let _ = self.commands.send(Command::Configure {
            language: language.into(),
            keyterms,
        });
    }

    /// Starts the session. Returns once the first attempt has opened the
    /// socket, failed into a retry, or been refused.
    pub async fn connect(&self) {
        let (tx, rx) = oneshot::channel();
        if self.commands.send(Command::Connect(tx)).is_ok() {
            let _ = rx.await;
        }
    }

    pub fn disconnect(&self) {
        let _ = self.commands.send(Command::Disconnect);
    }

    /// Pull raw 16 kHz mono PCM16 from `stream`. Replaces an earlier stream.
    pub fn attach_stream(&self, stream: PcmStream) {
        let _ = self.commands.send(Command::Attach(stream));
    }

    pub fn detach_stream(&self) {
        let _ = self.commands.send(Command::Detach);
    }

    pub fn is_active(&self) -> bool {
        self.shared.active.load(Ordering::Acquire)
    }

    pub fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::Acquire)
    }

    /// Bytes held for replay while the socket is down.
    pub fn replay_buffer_bytes(&self) -> usize {
        self.shared.replay_bytes.load(Ordering::Acquire)
    }

    pub fn stats(&self) -> SessionStats {
        let s = self.shared.stats.lock().unwrap_or_else(|e| e.into_inner());
        SessionStats {
            total_words: s.total_words,
            average_confidence: if s.confidence_count > 0 {
                s.confidence_sum / s.confidence_count as f64
            } else {
                0.0
            },
            uptime_ms: s
                .started_at
                .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0),
            reconnect_count: s.reconnect_count,
        }
    }

    pub fn reset_stats(&self) {
        *self.shared.stats.lock().unwrap_or_else(|e| e.into_inner()) = StatsInner {
            started_at: Some(std::time::Instant::now()),
            ..StatsInner::default()
        };
    }
}

/// The listen URL for one connection, as `@deepgram/sdk` 5 builds it.
pub fn listen_url(endpoint: &str, language: &str, keyterms: &[String]) -> String {
    let mut url = format!(
        "{endpoint}?model=nova-3&language={}&punctuate=true&smart_format=true\
         &interim_results=true&utterance_end_ms=1500&vad_events=true&encoding=linear16\
         &sample_rate=16000&channels=1&no_delay=true",
        encode_component(language)
    );
    for term in keyterms {
        url.push_str("&keyterm=");
        url.push_str(&encode_component(term));
    }
    url
}

use kairo_domain::js::encode_uri_component as encode_component;

/// The server's words for "another computer holds the session".
fn is_session_limit(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    [
        "another computer is using transcription",
        "too many transcription sessions",
        "transcription session or monthly minute limit",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

fn is_refusal(error: &ApiError) -> bool {
    error.status.is_some_and(|s| (400..500).contains(&s))
}

fn stopped_message(reason: &str) -> String {
    let why = if reason.is_empty() {
        String::new()
    } else {
        format!("{reason}. ")
    };
    format!("Transcription stopped: {why}Your presentation is still live.")
}

use kairo_domain::js::now_ms;

fn result_id(now: f64) -> String {
    let random: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(6)
        .collect();
    format!("dg-{now}-{random}")
}

// ─── Provider messages ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DgWord {
    word: String,
    punctuated_word: Option<String>,
    start: f64,
    end: f64,
    confidence: f64,
}

#[derive(Deserialize)]
struct DgAlternative {
    transcript: Option<String>,
    confidence: Option<f64>,
    words: Option<Vec<DgWord>>,
}

#[derive(Deserialize)]
struct DgChannel {
    alternatives: Option<Vec<DgAlternative>>,
}

#[derive(Deserialize)]
struct DgMessage {
    #[serde(rename = "type")]
    kind: Option<String>,
    start: Option<f64>,
    is_final: Option<bool>,
    speech_final: Option<bool>,
    duration: Option<f64>,
    channel: Option<DgChannel>,
    last_word_end: Option<f64>,
}

// ─── Actor ──────────────────────────────────────────────────────────────────

enum Internal {
    Grant(u64, Result<TranscriptionGrant, ApiError>),
    Socket(u64, Result<Box<Socket>, String>),
    Renewed {
        id: String,
        attempt: usize,
        result: Result<TranscriptionGrant, ApiError>,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum ReconnectKind {
    Backoff,
    SessionWait,
}

struct Renewal {
    at: Instant,
    id: String,
    attempt: usize,
}

struct Actor {
    access: Arc<dyn TranscriptionAccess>,
    endpoint: String,
    timings: Timings,
    events: broadcast::Sender<SttEvent>,
    shared: Arc<Shared>,
    internal: mpsc::UnboundedSender<Internal>,
    generation: u64,
    grant_id: Option<String>,
    /// A renewal was refused and a fresh session tried; a second refusal stops.
    refused_once: bool,
    session_waits: u32,
    renewal: Option<Renewal>,
    language: String,
    keyterms: Vec<String>,
    socket: Option<Socket>,
    audio: Option<PcmStream>,
    active: bool,
    destroyed: bool,
    reconnect_attempt: u32,
    reconnect: Option<(Instant, ReconnectKind)>,
    keepalive_at: Option<Instant>,
    last_frame_at: Instant,
    buffer: VecDeque<Vec<u8>>,
    buffer_bytes: usize,
    connect_waiters: Vec<oneshot::Sender<()>>,
}

enum Wake {
    Command(Option<Command>),
    Internal(Internal),
    Frame(Option<Result<Message, String>>),
    Audio(Option<Vec<u8>>),
    Reconnect,
    Renew,
    KeepAlive,
}

async fn at(deadline: Option<Instant>) {
    match deadline {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

impl Actor {
    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut internal: mpsc::UnboundedReceiver<Internal>,
    ) {
        loop {
            let wake = {
                let socket = &mut self.socket;
                let audio = &self.audio;
                let reconnect_at = self.reconnect.map(|(t, _)| t);
                let renew_at = self.renewal.as_ref().map(|r| r.at);
                let keepalive_at = self.keepalive_at;
                tokio::select! {
                    command = commands.recv() => Wake::Command(command),
                    Some(message) = internal.recv() => Wake::Internal(message),
                    frame = async {
                        match socket.as_mut() {
                            Some(s) => s.next().await.map(|r| r.map_err(|e| e.to_string())),
                            None => std::future::pending().await,
                        }
                    } => Wake::Frame(frame),
                    chunk = async {
                        match audio {
                            Some(a) => a.recv().await,
                            None => std::future::pending().await,
                        }
                    } => Wake::Audio(chunk),
                    () = at(reconnect_at) => Wake::Reconnect,
                    () = at(renew_at) => Wake::Renew,
                    () = at(keepalive_at) => Wake::KeepAlive,
                }
            };
            match wake {
                Wake::Command(None) => {
                    self.disconnect();
                    return;
                }
                Wake::Command(Some(command)) => self.on_command(command),
                Wake::Internal(message) => self.on_internal(message).await,
                Wake::Frame(frame) => self.on_frame(frame).await,
                Wake::Audio(Some(chunk)) => self.on_audio(chunk).await,
                Wake::Audio(None) => self.audio = None,
                Wake::Reconnect => self.on_reconnect_timer(),
                Wake::Renew => self.on_renew_timer(),
                Wake::KeepAlive => self.on_keepalive().await,
            }
            self.publish_state();
        }
    }

    fn emit(&self, event: SttEvent) {
        let _ = self.events.send(event);
    }

    fn publish_state(&self) {
        self.shared
            .connected
            .store(self.socket.is_some(), Ordering::Release);
        self.shared.active.store(self.active, Ordering::Release);
        self.shared
            .replay_bytes
            .store(self.buffer_bytes, Ordering::Release);
    }

    fn reply_connect(&mut self) {
        for waiter in self.connect_waiters.drain(..) {
            let _ = waiter.send(());
        }
    }

    fn on_command(&mut self, command: Command) {
        match command {
            Command::Configure { language, keyterms } => {
                tracing::info!(language, keyterms = keyterms.len(), "deepgram configured");
                self.language = language;
                self.keyterms = keyterms;
            }
            Command::Connect(reply) => {
                if self.active {
                    let _ = reply.send(());
                    return;
                }
                self.destroyed = false;
                self.active = true;
                self.refused_once = false;
                self.session_waits = 0;
                self.shared
                    .stats
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .started_at = Some(std::time::Instant::now());
                self.connect_waiters.push(reply);
                self.open_socket();
            }
            Command::Disconnect => self.disconnect(),
            Command::Attach(stream) => {
                self.audio = Some(stream);
                tracing::info!("deepgram audio stream attached");
            }
            Command::Detach => self.audio = None,
        }
    }

    fn disconnect(&mut self) {
        self.generation += 1;
        self.release_grant();
        self.destroyed = true;
        self.active = false;
        self.reconnect_attempt = 0;
        self.buffer.clear();
        self.buffer_bytes = 0;
        self.reconnect = None;
        self.keepalive_at = None;
        self.close_socket();
        self.audio = None;
        self.reply_connect();
        self.emit(SttEvent::Disconnected);
        tracing::info!("deepgram disconnected");
    }

    /// Release the current grant and start a fresh one.
    fn open_socket(&mut self) {
        if !self.active || self.destroyed {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        tracing::info!(attempt = self.reconnect_attempt, "opening deepgram socket");
        let previous = self.release_grant_id();
        let access = self.access.clone();
        let internal = self.internal.clone();
        tokio::spawn(async move {
            if let Some(id) = previous {
                let _ = access.release_transcription(id).await;
            }
            let grant = access.transcription().await;
            let _ = internal.send(Internal::Grant(generation, grant));
        });
    }

    fn release_grant_id(&mut self) -> Option<String> {
        self.renewal = None;
        self.grant_id.take()
    }

    fn release_grant(&mut self) {
        if let Some(id) = self.release_grant_id() {
            let access = self.access.clone();
            tokio::spawn(async move {
                let _ = access.release_transcription(id).await;
            });
        }
    }

    fn current(&self, generation: u64) -> bool {
        self.active && generation == self.generation
    }

    async fn on_internal(&mut self, message: Internal) {
        match message {
            Internal::Grant(generation, result) => self.on_grant(generation, result),
            Internal::Socket(generation, result) => {
                self.on_socket(generation, result.map(|s| *s)).await
            }
            Internal::Renewed {
                id,
                attempt,
                result,
            } => self.on_renewed(id, attempt, result),
        }
    }

    fn on_grant(&mut self, generation: u64, result: Result<TranscriptionGrant, ApiError>) {
        let grant = match result {
            Ok(grant) => grant,
            Err(error) => {
                if !self.current(generation) {
                    return;
                }
                // Another computer holds the licence's session, often one
                // that crashed; its lease lapses within three minutes.
                if error.status == Some(403)
                    && is_session_limit(&error.message)
                    && self.session_waits < self.timings.session_wait_tries
                {
                    self.session_waits += 1;
                    tracing::warn!(
                        attempt = self.session_waits,
                        "another computer is using transcription; retrying"
                    );
                    self.emit(SttEvent::Reconnecting {
                        attempt: self.session_waits,
                        delay_ms: self.timings.session_wait.as_millis() as u64,
                    });
                    self.reconnect = Some((
                        Instant::now() + self.timings.session_wait,
                        ReconnectKind::SessionWait,
                    ));
                    self.reply_connect();
                    return;
                }
                // The licence server said no: retrying will not help.
                if is_refusal(&error) {
                    tracing::error!(status = ?error.status, reason = error.message, "transcription refused");
                    self.disconnect();
                    self.emit(SttEvent::Error {
                        message: stopped_message(&error.message),
                        fatal: true,
                    });
                    return;
                }
                // Unreachable: treat it like a dropped connection.
                tracing::error!(reason = error.message, "transcription grant failed");
                self.handle_socket_close();
                self.reply_connect();
                return;
            }
        };
        if !self.current(generation) {
            let access = self.access.clone();
            tokio::spawn(async move {
                let _ = access.release_transcription(grant.session_id).await;
            });
            return;
        }
        self.grant_id = Some(grant.session_id.clone());
        self.schedule_renewal(
            grant.session_id.clone(),
            self.timings
                .renew_unit
                .mul_f64(grant.renew_after_seconds.max(1.0)),
            0,
        );
        let url = listen_url(&self.endpoint, &self.language, &self.keyterms);
        let token = grant.access_token;
        let internal = self.internal.clone();
        tokio::spawn(async move {
            let result = connect_socket(&url, &token).await;
            let _ = internal.send(Internal::Socket(generation, result.map(Box::new)));
        });
    }

    async fn on_socket(&mut self, generation: u64, result: Result<Socket, String>) {
        let socket = match result {
            Ok(socket) => socket,
            Err(error) => {
                if !self.current(generation) {
                    return;
                }
                tracing::error!(error, "deepgram connect failed");
                self.handle_socket_close();
                self.reply_connect();
                return;
            }
        };
        if !self.current(generation) {
            tokio::spawn(close(socket));
            return;
        }
        tracing::info!(attempt = self.reconnect_attempt, "deepgram socket open");
        self.socket = Some(socket);
        self.reconnect = None;
        self.reconnect_attempt = 0;
        self.session_waits = 0;
        self.last_frame_at = Instant::now();
        self.keepalive_at = Some(Instant::now() + self.timings.keepalive);
        if !self.buffer.is_empty() {
            tracing::info!(
                chunks = self.buffer.len(),
                bytes = self.buffer_bytes,
                "draining buffered audio"
            );
            while let Some(chunk) = self.buffer.pop_front() {
                let len = chunk.len();
                let sent = match self.socket.as_mut() {
                    Some(s) => s.send(Message::Binary(chunk.clone().into())).await,
                    None => break,
                };
                if let Err(error) = sent {
                    tracing::error!(%error, "failed to drain buffered audio");
                    self.buffer.push_front(chunk);
                    self.close_socket();
                    self.handle_socket_close();
                    self.reply_connect();
                    return;
                }
                self.buffer_bytes -= len;
            }
        }
        self.emit(SttEvent::Connected);
        self.reply_connect();
    }

    fn schedule_renewal(&mut self, id: String, after: Duration, attempt: usize) {
        self.renewal = Some(Renewal {
            at: Instant::now() + after,
            id,
            attempt,
        });
    }

    fn on_renew_timer(&mut self) {
        let Some(renewal) = self.renewal.take() else {
            return;
        };
        if self.grant_id.as_deref() != Some(&renewal.id) || !self.active {
            return;
        }
        let access = self.access.clone();
        let internal = self.internal.clone();
        tokio::spawn(async move {
            let result = access.renew_transcription(renewal.id.clone()).await;
            let _ = internal.send(Internal::Renewed {
                id: renewal.id,
                attempt: renewal.attempt,
                result,
            });
        });
    }

    fn on_renewed(
        &mut self,
        id: String,
        attempt: usize,
        result: Result<TranscriptionGrant, ApiError>,
    ) {
        if self.grant_id.as_deref() != Some(&id) {
            return;
        }
        let error = match result {
            Ok(grant) => {
                self.refused_once = false;
                let after = self
                    .timings
                    .renew_unit
                    .mul_f64(grant.renew_after_seconds.max(1.0));
                self.schedule_renewal(id, after, 0);
                return;
            }
            Err(error) => error,
        };
        tracing::warn!(
            attempt,
            reason = error.message,
            "transcription renewal failed"
        );
        if let Some(&retry) = self.timings.renewal_retries.get(attempt) {
            self.schedule_renewal(id, retry, attempt + 1);
            return;
        }
        if is_refusal(&error) && !self.refused_once {
            // A lapsed or lost session clears with a new one; a real refusal
            // is refused again there, and stops.
            self.refused_once = true;
            tracing::warn!("renewal refused; starting a fresh transcription session");
            self.close_socket();
            self.handle_socket_close();
            return;
        }
        if is_refusal(&error) {
            self.disconnect();
            self.emit(SttEvent::Error {
                message: stopped_message(&error.message),
                fatal: true,
            });
            return;
        }
        tracing::warn!("licence server unreachable; reconnecting with a fresh session");
        self.close_socket();
        self.handle_socket_close();
    }

    fn close_socket(&mut self) {
        if let Some(socket) = self.socket.take() {
            tokio::spawn(close(socket));
        }
        self.keepalive_at = None;
    }

    fn handle_socket_close(&mut self) {
        self.close_socket();
        if !self.active || self.destroyed {
            return;
        }
        self.schedule_reconnect();
    }

    fn schedule_reconnect(&mut self) {
        if self.reconnect_attempt >= self.timings.reconnect_tries {
            tracing::error!("deepgram reconnect attempts exhausted");
            self.emit(SttEvent::Error {
                message: format!(
                    "Deepgram reconnect failed after {} attempts",
                    self.timings.reconnect_tries
                ),
                fatal: true,
            });
            self.active = false;
            self.reply_connect();
            return;
        }
        let delay = kairo_domain::time::Backoff::Exponential {
            base: self.timings.reconnect_base,
            max: self.timings.reconnect_max,
        }
        .delay(self.reconnect_attempt);
        self.reconnect_attempt += 1;
        tracing::info!(
            attempt = self.reconnect_attempt,
            ?delay,
            "deepgram reconnecting"
        );
        self.emit(SttEvent::Reconnecting {
            attempt: self.reconnect_attempt,
            delay_ms: delay.as_millis() as u64,
        });
        self.reconnect = Some((Instant::now() + delay, ReconnectKind::Backoff));
    }

    fn on_reconnect_timer(&mut self) {
        let Some((_, kind)) = self.reconnect.take() else {
            return;
        };
        if !self.active || self.destroyed {
            return;
        }
        if kind == ReconnectKind::Backoff {
            self.shared
                .stats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .reconnect_count += 1;
        }
        self.open_socket();
    }

    async fn on_keepalive(&mut self) {
        self.keepalive_at = None;
        if self.socket.is_none() {
            return;
        }
        if self.last_frame_at.elapsed() >= self.timings.idle_timeout {
            tracing::warn!("deepgram socket silent; treating it as closed");
            self.close_socket();
            self.handle_socket_close();
            return;
        }
        let Some(socket) = self.socket.as_mut() else {
            return;
        };
        // Deepgram's keepalive holds the stream open through silence; the
        // ping draws a pong so a half-open socket shows up as silence.
        let keepalive = Message::Text(r#"{"type":"KeepAlive"}"#.into());
        if !send_within(socket, keepalive).await
            || !send_within(socket, Message::Ping(Vec::new().into())).await
        {
            self.close_socket();
            self.handle_socket_close();
            return;
        }
        self.keepalive_at = Some(Instant::now() + self.timings.keepalive);
    }

    async fn on_frame(&mut self, frame: Option<Result<Message, String>>) {
        match frame {
            Some(Ok(message)) => {
                self.last_frame_at = Instant::now();
                match message {
                    Message::Text(text) => self.handle_message(&text),
                    Message::Close(_) => {
                        tracing::info!("deepgram socket closed");
                        self.handle_socket_close();
                    }
                    _ => {}
                }
            }
            Some(Err(error)) => {
                tracing::error!(error, "deepgram socket error");
                self.emit(SttEvent::Error {
                    message: error,
                    fatal: false,
                });
                self.handle_socket_close();
            }
            None => {
                tracing::info!("deepgram socket closed");
                self.handle_socket_close();
            }
        }
    }

    async fn on_audio(&mut self, chunk: Vec<u8>) {
        if let Some(socket) = self.socket.as_mut() {
            if !send_within(socket, Message::Binary(chunk.clone().into())).await {
                // Kept for the replay after reconnect, never silently lost.
                self.buffer_audio(chunk);
                self.close_socket();
                self.handle_socket_close();
            }
        } else if self.active && !self.destroyed {
            self.buffer_audio(chunk);
        }
    }

    /// Hold at most 30 s while reconnecting, dropping the oldest.
    fn buffer_audio(&mut self, chunk: Vec<u8>) {
        self.buffer_bytes += chunk.len();
        self.buffer.push_back(chunk);
        while self.buffer_bytes > REPLAY_BUFFER_BYTES {
            let Some(oldest) = self.buffer.pop_front() else {
                break;
            };
            self.buffer_bytes -= oldest.len();
        }
    }

    fn handle_message(&mut self, text: &str) {
        let Ok(message) = serde_json::from_str::<DgMessage>(text) else {
            return;
        };
        match message.kind.as_deref() {
            Some("Results") => self.handle_results(message),
            Some("UtteranceEnd") => {
                tracing::debug!(last_word_end = ?message.last_word_end, "utterance end");
                self.emit(SttEvent::UtteranceEnd(now_ms()));
            }
            _ => {}
        }
    }

    fn handle_results(&mut self, message: DgMessage) {
        let Some(alternative) = message
            .channel
            .and_then(|c| c.alternatives)
            .and_then(|a| a.into_iter().next())
        else {
            return;
        };
        let text =
            kairo_domain::js::trim(alternative.transcript.as_deref().unwrap_or("")).to_owned();
        if text.is_empty() {
            return;
        }
        let words: Vec<TranscriptWord> = alternative
            .words
            .unwrap_or_default()
            .into_iter()
            .map(|w| TranscriptWord {
                word: w.punctuated_word.unwrap_or(w.word),
                start: w.start,
                end: w.end,
                confidence: w.confidence,
                extra: Default::default(),
            })
            .collect();
        let confidence = alternative.confidence.unwrap_or(0.0);
        let now = now_ms();
        let epoch = Some(self.generation as f64);
        if message.is_final.unwrap_or(false) || message.speech_final.unwrap_or(false) {
            {
                let mut stats = self.shared.stats.lock().unwrap_or_else(|e| e.into_inner());
                stats.total_words += words.len() as u64;
                stats.confidence_sum += confidence;
                stats.confidence_count += 1;
            }
            self.emit(SttEvent::Final(TranscriptResult {
                audio_epoch: epoch,
                audio_start: message.start,
                audio_duration: message.duration,
                id: result_id(now),
                text,
                words,
                is_final: kairo_domain::serde_util::True,
                timestamp: now,
                duration: message.duration.unwrap_or(0.0),
                extra: Default::default(),
            }));
        } else {
            self.emit(SttEvent::Interim(InterimResult {
                audio_epoch: epoch,
                audio_start: message.start,
                audio_duration: message.duration,
                text,
                stability: confidence,
                timestamp: now,
            }));
        }
    }
}

/// A connect that neither opens nor fails in this time (a captive portal, a
/// black-holed route) counts as failed, so reconnect logic can run.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A send blocked this long means the kernel buffer is full on a half-open
/// socket; without a cap it would wait for the OS retransmit timeout.
const SEND_TIMEOUT: Duration = Duration::from_secs(5);

async fn connect_socket(url: &str, token: &str) -> Result<Socket, String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    let auth = format!("Bearer {token}")
        .parse()
        .map_err(|_| "invalid transcription token".to_string())?;
    request.headers_mut().insert("Authorization", auth);
    // Audio goes out in small frames as it is captured: Nagle's algorithm
    // would hold one back until the previous is acknowledged, and a delayed
    // ACK makes that about 40 ms on Linux.
    let connect = tokio_tungstenite::connect_async_with_config(request, None, true);
    let (socket, _) = tokio::time::timeout(CONNECT_TIMEOUT, connect)
        .await
        .map_err(|_| "connection timed out".to_string())?
        .map_err(|e| e.to_string())?;
    Ok(socket)
}

/// `socket.send` that gives up after [`SEND_TIMEOUT`]. False on error or timeout.
async fn send_within(socket: &mut Socket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(SEND_TIMEOUT, socket.send(message)).await,
        Ok(Ok(()))
    )
}

/// `CloseStream` asks Deepgram to flush; then the socket closes.
async fn close(mut socket: Socket) {
    send_within(
        &mut socket,
        Message::Text(r#"{"type":"CloseStream"}"#.into()),
    )
    .await;
    let _ = tokio::time::timeout(SEND_TIMEOUT, socket.close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_matches_the_sdk() {
        // Captured from @deepgram/sdk 5.3.0 against a local socket server.
        assert_eq!(
            listen_url(
                "wss://api.deepgram.com/v1/listen",
                "en",
                &["Habakkuk".into(), "Song of Solomon".into()]
            ),
            "wss://api.deepgram.com/v1/listen?model=nova-3&language=en&punctuate=true&smart_format=true\
             &interim_results=true&utterance_end_ms=1500&vad_events=true&encoding=linear16\
             &sample_rate=16000&channels=1&no_delay=true&keyterm=Habakkuk&keyterm=Song%20of%20Solomon"
        );
        assert!(!listen_url("x", "en", &[]).contains("keyterm"));
        assert!(listen_url("x", "pt-BR", &[]).contains("language=pt-BR&"));
    }

    #[test]
    fn session_limit_wording_old_and_new() {
        assert!(is_session_limit(
            "Another computer is using transcription on this licence"
        ));
        assert!(is_session_limit("Too many transcription sessions"));
        assert!(is_session_limit(
            "Transcription session or monthly minute limit reached"
        ));
        assert!(!is_session_limit(
            "This month's transcription minutes are used up"
        ));
    }

    #[test]
    fn stop_messages_carry_the_reason() {
        assert_eq!(
            stopped_message("Minutes used up"),
            "Transcription stopped: Minutes used up. Your presentation is still live."
        );
        assert_eq!(
            stopped_message(""),
            "Transcription stopped: Your presentation is still live."
        );
    }
}
