//! The Deepgram actor against a scripted local WebSocket server.
//! Behaviours of `tests/stt-lifecycle.test.ts`, `tests/stt-keyterms.test.ts`
//! and `stt-audio-epoch.test.ts`, plus reconnect, half-open and replay order.

use futures_util::{SinkExt, StreamExt};
use kairo_domain::cloud::TranscriptionGrant;
use kairo_net::ApiError;
use kairo_stt::access::{BoxFuture, TranscriptionAccess};
use kairo_stt::deepgram::{DeepgramStt, SttEvent, Timings};
use kairo_stt::relay::AudioRelay;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

// ─── Fake licence server ────────────────────────────────────────────────────

type Scripted = VecDeque<Result<TranscriptionGrant, ApiError>>;

#[derive(Default)]
struct Access {
    grants: Mutex<Scripted>,
    renewals: Mutex<Scripted>,
    issued: Mutex<u32>,
    renewed: Mutex<u32>,
    released: Mutex<Vec<String>>,
    /// Hold `transcription()` until notified.
    gate: Option<Arc<tokio::sync::Notify>>,
}

fn grant(id: &str) -> TranscriptionGrant {
    TranscriptionGrant {
        access_token: format!("ephemeral-{id}"),
        expires_in: 30.0,
        session_id: id.into(),
        renew_after_seconds: 6.0,
    }
}

fn refused(status: Option<u16>, message: &str) -> ApiError {
    ApiError {
        status,
        message: message.into(),
    }
}

impl TranscriptionAccess for Access {
    fn transcription(&self) -> BoxFuture<'_, Result<TranscriptionGrant, ApiError>> {
        Box::pin(async move {
            if let Some(gate) = &self.gate {
                gate.notified().await;
            }
            let n = {
                let mut issued = self.issued.lock().unwrap();
                *issued += 1;
                *issued
            };
            self.grants
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(grant(&n.to_string())))
        })
    }

    fn renew_transcription(
        &self,
        id: String,
    ) -> BoxFuture<'_, Result<TranscriptionGrant, ApiError>> {
        Box::pin(async move {
            *self.renewed.lock().unwrap() += 1;
            self.renewals
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(grant(&id)))
        })
    }

    fn release_transcription(&self, id: String) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async move {
            self.released.lock().unwrap().push(id);
            Ok(())
        })
    }

    fn assert_transcription(&self) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async { Ok(()) })
    }
}

// ─── Scripted server ────────────────────────────────────────────────────────

enum ServerCmd {
    Send(Message),
    Close,
    /// Stop reading: no pongs, no data. A half-open peer.
    Mute,
}

struct Conn {
    uri: String,
    authorization: String,
    received: mpsc::UnboundedReceiver<Message>,
    commands: mpsc::UnboundedSender<ServerCmd>,
}

impl Conn {
    fn send_json(&self, value: serde_json::Value) {
        let _ = self
            .commands
            .send(ServerCmd::Send(Message::Text(value.to_string().into())));
    }

    async fn next_binary(&mut self) -> Vec<u8> {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), self.received.recv()).await {
                Ok(Some(Message::Binary(b))) => return b.to_vec(),
                Ok(Some(_)) => continue,
                other => panic!(
                    "no binary frame: {:?}",
                    other.map(|m| m.map(|m| m.to_string()))
                ),
            }
        }
    }

    async fn texts_until(&mut self, wanted: &str) {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), self.received.recv()).await {
                Ok(Some(Message::Text(t))) if t.contains(wanted) => return,
                Ok(Some(_)) => continue,
                _ => panic!("never saw {wanted}"),
            }
        }
    }
}

struct Server {
    url: String,
    conns: mpsc::UnboundedReceiver<Conn>,
}

impl Server {
    // The handshake callback's error type is tungstenite's.
    #[allow(clippy::result_large_err)]
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/v1/listen", listener.local_addr().unwrap());
        let (tx, conns) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let seen = Arc::new(Mutex::new((String::new(), String::new())));
                    let capture = seen.clone();
                    let callback = move |req: &Request, res: Response| {
                        let auth = req
                            .headers()
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_owned();
                        *capture.lock().unwrap() = (req.uri().to_string(), auth);
                        Ok(res)
                    };
                    let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, callback).await
                    else {
                        return;
                    };
                    let (uri, authorization) = seen.lock().unwrap().clone();
                    let (in_tx, received) = mpsc::unbounded_channel();
                    let (commands, mut cmd_rx) = mpsc::unbounded_channel();
                    let _ = tx.send(Conn {
                        uri,
                        authorization,
                        received,
                        commands,
                    });
                    let mut muted = false;
                    // A dropped handle leaves the connection open.
                    let mut handle_alive = true;
                    loop {
                        tokio::select! {
                            cmd = cmd_rx.recv(), if handle_alive => match cmd {
                                Some(ServerCmd::Send(m)) => { let _ = ws.send(m).await; }
                                Some(ServerCmd::Close) => { let _ = ws.close(None).await; return; }
                                Some(ServerCmd::Mute) => muted = true,
                                None => handle_alive = false,
                            },
                            frame = ws.next(), if !muted => match frame {
                                Some(Ok(m)) => { let _ = in_tx.send(m); }
                                _ => return,
                            },
                        }
                    }
                });
            }
        });
        Self { url, conns }
    }

    async fn accept(&mut self) -> Conn {
        tokio::time::timeout(Duration::from_secs(5), self.conns.recv())
            .await
            .expect("no connection")
            .unwrap()
    }

    async fn no_connection_for(&mut self, wait: Duration) {
        assert!(
            tokio::time::timeout(wait, self.conns.recv()).await.is_err(),
            "unexpected connection"
        );
    }
}

fn fast() -> Timings {
    Timings {
        reconnect_base: Duration::from_millis(20),
        reconnect_max: Duration::from_millis(100),
        reconnect_tries: 20,
        keepalive: Duration::from_millis(50),
        idle_timeout: Duration::from_millis(300),
        session_wait: Duration::from_millis(40),
        session_wait_tries: 8,
        renewal_retries: [20, 20, 20].map(Duration::from_millis),
        renew_unit: Duration::from_millis(10),
    }
}

fn service(access: Arc<Access>, server: &Server) -> (DeepgramStt, broadcast::Receiver<SttEvent>) {
    let stt = DeepgramStt::with_endpoint(access, server.url.clone(), fast());
    let events = stt.subscribe();
    stt.configure("en", Vec::new());
    (stt, events)
}

async fn next_event(
    events: &mut broadcast::Receiver<SttEvent>,
    wanted: impl Fn(&SttEvent) -> bool,
) -> SttEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("event timed out")
            .unwrap();
        if wanted(&event) {
            return event;
        }
    }
}

fn is_error(e: &SttEvent) -> bool {
    matches!(e, SttEvent::Error { .. })
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(30)).await;
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn each_connection_uses_a_new_bearer_token_and_releases_its_session() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    let (stt, _events) = service(access.clone(), &server);
    stt.configure("en-US", vec!["Habakkuk".into(), "Song of Solomon".into()]);
    stt.connect().await;
    let first = server.accept().await;
    assert!(stt.is_connected());
    stt.disconnect();
    stt.configure("fr", Vec::new());
    stt.connect().await;
    let second = server.accept().await;
    stt.disconnect();
    settle().await;

    assert_eq!(first.authorization, "Bearer ephemeral-1");
    assert_eq!(second.authorization, "Bearer ephemeral-2");
    assert_eq!(
        first.uri,
        "/v1/listen?model=nova-3&language=en-US&punctuate=true&smart_format=true&interim_results=true\
         &utterance_end_ms=1500&vad_events=true&encoding=linear16&sample_rate=16000&channels=1\
         &no_delay=true&keyterm=Habakkuk&keyterm=Song%20of%20Solomon"
    );
    assert!(second.uri.contains("language=fr&") && !second.uri.contains("keyterm"));
    assert_eq!(*access.released.lock().unwrap(), ["1", "2"]);
}

#[tokio::test]
async fn results_become_interims_and_finals_stamped_with_the_connection() {
    let mut server = Server::start().await;
    let (stt, mut events) = service(Arc::new(Access::default()), &server);
    stt.connect().await;
    let conn = server.accept().await;
    conn.send_json(serde_json::json!({
        "type": "Results", "start": 0, "duration": 1, "is_final": false,
        "channel": { "alternatives": [{ "transcript": " Next verse ", "confidence": 0.8 }] }
    }));
    conn.send_json(serde_json::json!({ "type": "Results", "channel": { "alternatives": [{ "transcript": "   " }] } }));
    conn.send_json(serde_json::json!({ "type": "Metadata" }));
    conn.send_json(serde_json::json!({
        "type": "Results", "start": 0.5, "duration": 1.25, "speech_final": true,
        "channel": { "alternatives": [{ "transcript": "John three sixteen", "confidence": 0.9,
            "words": [{ "word": "john", "punctuated_word": "John", "start": 0.5, "end": 0.8, "confidence": 0.95 }] }] }
    }));
    conn.send_json(serde_json::json!({ "type": "UtteranceEnd", "last_word_end": 1.7 }));

    let SttEvent::Interim(interim) =
        next_event(&mut events, |e| matches!(e, SttEvent::Interim(_))).await
    else {
        unreachable!()
    };
    assert_eq!(
        (interim.text.as_str(), interim.stability),
        ("Next verse", 0.8)
    );
    let SttEvent::Final(result) =
        next_event(&mut events, |e| matches!(e, SttEvent::Final(_))).await
    else {
        unreachable!()
    };
    assert_eq!(result.text, "John three sixteen");
    assert_eq!(result.words[0].word, "John");
    assert_eq!(
        (result.audio_start, result.audio_duration, result.duration),
        (Some(0.5), Some(1.25), 1.25)
    );
    assert_eq!(result.audio_epoch, interim.audio_epoch);
    assert!(result.id.starts_with("dg-"));
    next_event(&mut events, |e| matches!(e, SttEvent::UtteranceEnd(_))).await;
    let stats = stt.stats();
    assert_eq!((stats.total_words, stats.average_confidence), (1, 0.9));

    // A new connection is a new epoch.
    stt.disconnect();
    stt.connect().await;
    let conn = server.accept().await;
    conn.send_json(serde_json::json!({
        "type": "Results", "is_final": true, "channel": { "alternatives": [{ "transcript": "Amen" }] }
    }));
    let SttEvent::Final(later) = next_event(&mut events, |e| matches!(e, SttEvent::Final(_))).await
    else {
        unreachable!()
    };
    assert!(later.audio_epoch > result.audio_epoch);
    stt.disconnect();
}

#[tokio::test]
async fn an_old_connection_cannot_transcribe_into_a_new_session() {
    let mut server = Server::start().await;
    let (stt, mut events) = service(Arc::new(Access::default()), &server);
    stt.connect().await;
    let old = server.accept().await;
    stt.disconnect();
    stt.connect().await;
    let _new = server.accept().await;
    old.send_json(serde_json::json!({
        "type": "Results", "is_final": true, "channel": { "alternatives": [{ "transcript": "stale" }] }
    }));
    let _ = old.commands.send(ServerCmd::Close);
    settle().await;
    assert!(stt.is_connected());
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(event, SttEvent::Final(_)), "stale final leaked");
    }
    stt.disconnect();
}

#[tokio::test]
async fn audio_buffered_while_down_replays_in_order_before_new_audio() {
    let mut server = Server::start().await;
    let (stt, mut events) = service(Arc::new(Access::default()), &server);
    let relay = AudioRelay::default();
    stt.attach_stream(relay.create_stream());
    stt.connect().await;
    let mut first = server.accept().await;
    relay.feed(vec![0]);
    assert_eq!(first.next_binary().await, [0]);

    let _ = first.commands.send(ServerCmd::Close);
    let SttEvent::Reconnecting { attempt, delay_ms } =
        next_event(&mut events, |e| matches!(e, SttEvent::Reconnecting { .. })).await
    else {
        unreachable!()
    };
    assert_eq!((attempt, delay_ms), (1, 20));
    // Sent while the socket is down: held for replay.
    for n in 1..=3u8 {
        relay.feed(vec![n]);
    }
    let mut second = server.accept().await;
    next_event(&mut events, |e| matches!(e, SttEvent::Connected)).await;
    relay.feed(vec![4]);
    let mut got = Vec::new();
    for _ in 0..4 {
        got.extend(second.next_binary().await);
    }
    assert_eq!(got, [1, 2, 3, 4]);
    assert_eq!(stt.replay_buffer_bytes(), 0);
    assert_eq!(stt.stats().reconnect_count, 1);
    stt.disconnect();
}

#[tokio::test]
async fn the_replay_buffer_keeps_the_newest_30_seconds() {
    let server = Server::start().await;
    let access = Arc::new(Access {
        gate: Some(Arc::new(tokio::sync::Notify::new())),
        ..Access::default()
    });
    let (stt, _events) = service(access, &server);
    let relay = AudioRelay::default();
    let stream = relay.create_stream();
    let connecting = {
        let stt = stt.clone();
        tokio::spawn(async move { stt.connect().await })
    };
    settle().await;
    stt.attach_stream(stream);
    // 40 s of 100 ms chunks while the grant is pending; the relay's 5 s cap
    // does not apply because the actor keeps pulling.
    for _ in 0..400 {
        relay.feed(vec![0; 3200]);
        tokio::task::yield_now().await;
        while relay.queued_bytes() > 0 {
            tokio::task::yield_now().await;
        }
    }
    settle().await;
    assert_eq!(stt.replay_buffer_bytes(), 960_000);
    stt.disconnect();
    settle().await;
    assert_eq!(stt.replay_buffer_bytes(), 0);
    connecting.abort();
}

#[tokio::test]
async fn a_half_open_socket_is_detected_and_replaced() {
    let mut server = Server::start().await;
    let (stt, mut events) = service(Arc::new(Access::default()), &server);
    stt.connect().await;
    let mut conn = server.accept().await;
    conn.texts_until("KeepAlive").await;
    let _ = conn.commands.send(ServerCmd::Mute);
    next_event(&mut events, |e| matches!(e, SttEvent::Reconnecting { .. })).await;
    let _replacement = server.accept().await;
    next_event(&mut events, |e| matches!(e, SttEvent::Connected)).await;
    stt.disconnect();
}

#[tokio::test]
async fn reconnects_back_off_and_give_up_after_the_limit() {
    // A port with nothing listening.
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/v1/listen", closed.local_addr().unwrap());
    drop(closed);
    let timings = Timings {
        reconnect_tries: 4,
        ..fast()
    };
    let stt = DeepgramStt::with_endpoint(Arc::new(Access::default()), url, timings);
    let mut events = stt.subscribe();
    stt.connect().await;
    let mut delays = Vec::new();
    let message = loop {
        match next_event(&mut events, |_| true).await {
            SttEvent::Reconnecting { delay_ms, .. } => delays.push(delay_ms),
            SttEvent::Error { message, fatal } => break (message, fatal),
            _ => {}
        }
    };
    assert_eq!(delays, [20, 40, 80, 100]);
    assert_eq!(
        message,
        ("Deepgram reconnect failed after 4 attempts".into(), true)
    );
    assert!(!stt.is_active());
    stt.disconnect();
}

#[tokio::test]
async fn stopping_during_setup_releases_the_late_grant() {
    let mut server = Server::start().await;
    let gate = Arc::new(tokio::sync::Notify::new());
    let access = Arc::new(Access {
        gate: Some(gate.clone()),
        ..Access::default()
    });
    let (stt, _events) = service(access.clone(), &server);
    let connecting = {
        let stt = stt.clone();
        tokio::spawn(async move { stt.connect().await })
    };
    settle().await;
    stt.disconnect();
    gate.notify_one();
    connecting.await.unwrap();
    settle().await;
    server.no_connection_for(Duration::from_millis(100)).await;
    assert!(!stt.is_connected());
    assert_eq!(*access.released.lock().unwrap(), ["1"]);
}

#[tokio::test]
async fn an_unreachable_licence_server_retries_without_opening_a_socket() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    access
        .grants
        .lock()
        .unwrap()
        .push_back(Err(refused(None, "Activate this computer")));
    let (stt, mut events) = service(access.clone(), &server);
    stt.connect().await;
    assert!(!stt.is_connected());
    next_event(&mut events, |e| matches!(e, SttEvent::Connected)).await;
    server.accept().await;
    assert_eq!(*access.issued.lock().unwrap(), 2);
    stt.disconnect();
}

#[tokio::test]
async fn a_refusal_stops_with_the_server_reason_and_opens_nothing() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    access.grants.lock().unwrap().push_back(Err(refused(
        Some(402),
        "This month's transcription minutes are used up",
    )));
    let (stt, mut events) = service(access, &server);
    stt.connect().await;
    let SttEvent::Error { message, fatal } = next_event(&mut events, is_error).await else {
        unreachable!()
    };
    assert!(fatal);
    assert_eq!(
        message,
        "Transcription stopped: This month's transcription minutes are used up. Your presentation is still live."
    );
    server.no_connection_for(Duration::from_millis(100)).await;
    assert!(!stt.is_active());
}

#[tokio::test]
async fn a_failed_renewal_is_retried_before_anything_stops() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    for _ in 0..2 {
        access
            .renewals
            .lock()
            .unwrap()
            .push_back(Err(refused(Some(503), "Kairo is busy")));
    }
    let (stt, mut events) = service(access.clone(), &server);
    stt.connect().await;
    server.accept().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(*access.renewed.lock().unwrap() >= 3);
    assert_eq!(*access.issued.lock().unwrap(), 1);
    while let Ok(event) = events.try_recv() {
        assert!(!is_error(&event) && !matches!(event, SttEvent::Reconnecting { .. }));
    }
    stt.disconnect();
}

#[tokio::test]
async fn a_refused_renewal_starts_one_fresh_session() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    for _ in 0..4 {
        access
            .renewals
            .lock()
            .unwrap()
            .push_back(Err(refused(Some(403), "The transcription session expired")));
    }
    let (stt, mut events) = service(access.clone(), &server);
    stt.connect().await;
    server.accept().await;
    server.accept().await;
    assert_eq!(*access.issued.lock().unwrap(), 2);
    assert!(access.released.lock().unwrap().contains(&"1".to_string()));
    while let Ok(event) = events.try_recv() {
        assert!(!is_error(&event));
    }
    stt.disconnect();
}

#[tokio::test]
async fn stops_with_the_reason_when_the_fresh_session_is_refused_too() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    let used_up = "This month's transcription minutes are used up";
    access
        .grants
        .lock()
        .unwrap()
        .extend([Ok(grant("1")), Err(refused(Some(403), used_up))]);
    for _ in 0..4 {
        access
            .renewals
            .lock()
            .unwrap()
            .push_back(Err(refused(Some(403), used_up)));
    }
    let (stt, mut events) = service(access, &server);
    stt.connect().await;
    server.accept().await;
    let SttEvent::Error { message, fatal } = next_event(&mut events, is_error).await else {
        unreachable!()
    };
    assert!(fatal && message.contains("transcription minutes are used up"));
    assert!(!stt.is_connected());
}

#[tokio::test]
async fn an_unreachable_renewal_reconnects_with_a_fresh_session() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    for _ in 0..4 {
        access.renewals.lock().unwrap().push_back(Err(refused(
            None,
            "Could not reach Kairo. Check your internet connection.",
        )));
    }
    let (stt, mut events) = service(access.clone(), &server);
    stt.connect().await;
    server.accept().await;
    server.accept().await;
    assert_eq!(*access.issued.lock().unwrap(), 2);
    while let Ok(event) = events.try_recv() {
        assert!(!is_error(&event));
    }
    stt.disconnect();
}

#[tokio::test]
async fn waits_for_another_computers_session_to_end() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    let busy = "Another computer is using transcription on this licence";
    access
        .grants
        .lock()
        .unwrap()
        .extend([Err(refused(Some(403), busy)), Err(refused(Some(403), busy))]);
    let (stt, mut events) = service(access.clone(), &server);
    stt.connect().await;
    let SttEvent::Reconnecting { attempt, delay_ms } =
        next_event(&mut events, |e| matches!(e, SttEvent::Reconnecting { .. })).await
    else {
        unreachable!()
    };
    assert_eq!((attempt, delay_ms), (1, 40));
    server.accept().await;
    assert_eq!(*access.issued.lock().unwrap(), 3);
    while let Ok(event) = events.try_recv() {
        assert!(!is_error(&event));
    }
    // Session waits are not reconnects.
    assert_eq!(stt.stats().reconnect_count, 0);
    stt.disconnect();
}

#[tokio::test]
async fn stops_once_another_computer_keeps_the_session() {
    let mut server = Server::start().await;
    let access = Arc::new(Access::default());
    let busy = "Another computer is using transcription on this licence";
    for _ in 0..9 {
        access
            .grants
            .lock()
            .unwrap()
            .push_back(Err(refused(Some(403), busy)));
    }
    let (stt, mut events) = service(access.clone(), &server);
    stt.connect().await;
    let SttEvent::Error { message, fatal } = next_event(&mut events, is_error).await else {
        unreachable!()
    };
    assert!(fatal && message.contains("Another computer is using transcription"));
    assert_eq!(*access.issued.lock().unwrap(), 9);
    server.no_connection_for(Duration::from_millis(100)).await;
    settle().await;
    assert!(
        events.try_recv().is_err() || !is_error(&events.try_recv().unwrap_or(SttEvent::Connected))
    );
}

/// Network: the TLS stack reaches Deepgram and is refused without a token.
/// `cargo test -p kairo-stt --test deepgram -- --ignored tls`
#[tokio::test]
#[ignore = "needs network"]
async fn tls_handshake_reaches_deepgram_and_is_refused_without_a_token() {
    let url = kairo_stt::deepgram::listen_url(kairo_stt::deepgram::DEEPGRAM_ENDPOINT, "en", &[]);
    let error = tokio_tungstenite::connect_async(url)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("401"), "{error}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_to_socket_latency() {
    let mut server = Server::start().await;
    let (stt, mut events) = service(Arc::new(Access::default()), &server);
    let relay = AudioRelay::default();
    stt.attach_stream(relay.create_stream());
    stt.connect().await;
    let mut conn = server.accept().await;
    next_event(&mut events, |e| matches!(e, SttEvent::Connected)).await;
    let mut samples = Vec::new();
    for n in 0..200u32 {
        let mut chunk = vec![0u8; 3200];
        chunk[..4].copy_from_slice(&n.to_le_bytes());
        let sent = std::time::Instant::now();
        relay.feed(chunk);
        let got = conn.next_binary().await;
        samples.push(sent.elapsed());
        assert_eq!(u32::from_le_bytes(got[..4].try_into().unwrap()), n);
    }
    samples.sort();
    let p95 = samples[189];
    eprintln!(
        "relay to socket: p50 {:?} p95 {:?} max {:?}",
        samples[100], p95, samples[199]
    );
    assert!(p95 < Duration::from_millis(30));
    stt.disconnect();
}
