//! `KairoAudienceSource` against a local WebSocket server: the handshake
//! rules, acks, queued moderation, reconnect and fallback, ending.
//! `tests/engage-socket-recovery.test.ts` is the first test.

use futures_util::{SinkExt, StreamExt};
use kairo_audience::{AudienceSource, AudienceUpdate, KairoAudienceSource, RoomFetcher};
use kairo_domain::audience::{AudienceQuestionState, AudienceRoomCreated, AudienceSourceState};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;

fn room(token: &str) -> AudienceRoomCreated {
    serde_json::from_value(json!({
        "roomId": "room", "code": "75XNMW", "operatorToken": token, "title": "Test",
        "url": "https://kairopresenter.com/q/75XNMW",
        "settings": { "review": false, "showQuestions": true, "names": "optional" }
    }))
    .unwrap()
}

fn fetcher(token: &'static str, calls: Arc<AtomicUsize>) -> RoomFetcher {
    Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(room(token)) })
    })
}

/// What the server does with one accepted operator socket.
#[derive(Clone)]
enum Script {
    /// Send a snapshot, then forward every client message to the test.
    Live,
    /// Send a snapshot, then close straight away.
    CloseAfterSnapshot,
}

struct Server {
    base: String,
    /// Each handshake: its path and Authorization header.
    handshakes: Arc<Mutex<Vec<(String, String)>>>,
    /// Client messages, as JSON.
    received: mpsc::UnboundedReceiver<Value>,
    /// Sends to the most recent socket.
    push: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>>,
}

// tungstenite fixes the handshake callback's error type.
#[allow(clippy::result_large_err)]
async fn server(
    reject: impl Fn(&str, usize) -> Option<u16> + Send + Sync + 'static,
    scripts: Vec<Script>,
) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handshakes: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let push: Arc<Mutex<Option<mpsc::UnboundedSender<Message>>>> = Arc::default();
    let (tx, received) = mpsc::unbounded_channel();
    let reject = Arc::new(reject);
    let (h, p) = (handshakes.clone(), push.clone());
    tokio::spawn(async move {
        let mut index = 0;
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (h, p, tx, reject) = (h.clone(), p.clone(), tx.clone(), reject.clone());
            let script = scripts.get(index).cloned().unwrap_or(Script::Live);
            let n = index;
            index += 1;
            tokio::spawn(async move {
                let callback =
                    |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
                        let auth = request
                            .headers()
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_owned();
                        let path = request.uri().to_string();
                        h.lock().unwrap().push((path, auth.clone()));
                        match reject(&auth, n) {
                            Some(code) => {
                                let mut error = ErrorResponse::new(None);
                                *error.status_mut() = StatusCode::from_u16(code).unwrap();
                                Err(error)
                            }
                            None => Ok(response),
                        }
                    };
                let Ok(socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
                    return;
                };
                let (mut write, mut read) = socket.split();
                // The recovery test's snapshot has only these fields.
                let snapshot = json!({ "type": "snapshot", "joined": 2, "questions": [], "settings": room("x").settings });
                write
                    .send(Message::Text(snapshot.to_string().into()))
                    .await
                    .unwrap();
                if matches!(script, Script::CloseAfterSnapshot) {
                    let _ = write.send(Message::Close(None)).await;
                    return;
                }
                let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
                *p.lock().unwrap() = Some(out_tx);
                loop {
                    tokio::select! {
                        out = out_rx.recv() => match out {
                            Some(m) => { if write.send(m).await.is_err() { return } }
                            None => return,
                        },
                        incoming = read.next() => match incoming {
                            Some(Ok(Message::Text(text))) => { let _ = tx.send(serde_json::from_str(&text).unwrap()); }
                            Some(Ok(Message::Ping(data))) => { let _ = write.send(Message::Pong(data)).await; }
                            Some(Ok(_)) => {}
                            _ => return,
                        },
                    }
                }
            });
        }
    });
    Server {
        base,
        handshakes,
        received,
        push,
    }
}

impl Server {
    fn send(&self, value: Value) {
        self.push
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(Message::Text(value.to_string().into()))
            .unwrap();
    }
    async fn next(&mut self) -> Value {
        tokio::time::timeout(Duration::from_secs(10), self.received.recv())
            .await
            .expect("a client message")
            .unwrap()
    }
}

type Log = Arc<Mutex<Vec<AudienceUpdate>>>;

fn watch(source: &KairoAudienceSource) -> Log {
    let log: Log = Arc::default();
    let l = log.clone();
    source.subscribe(Arc::new(move |update| l.lock().unwrap().push(update)));
    log
}

async fn wait_for(log: &Log, what: &str, predicate: impl Fn(&AudienceUpdate) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !log.lock().unwrap().iter().any(&predicate) {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {:#?}",
            log.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn state_is(state: AudienceSourceState) -> impl Fn(&AudienceUpdate) -> bool {
    move |u| u.status.as_ref().is_some_and(|s| s.state == state)
}

fn error_is(message: &'static str) -> impl Fn(&AudienceUpdate) -> bool {
    move |u| u.status.as_ref().and_then(|s| s.error.as_deref()) == Some(message)
}

#[tokio::test]
async fn a_rejected_operator_socket_renews_its_credential_and_becomes_live() {
    let server = server(
        |auth, _| (auth != "Bearer new-token").then_some(403),
        vec![],
    )
    .await;
    let renewals = Arc::new(AtomicUsize::new(0));
    let source = KairoAudienceSource::new(
        server.base.clone(),
        fetcher("old-token", Arc::default()),
        None,
        Some(fetcher("new-token", renewals.clone())),
    );
    let log = watch(&source);
    source.start().await.unwrap();
    wait_for(&log, "live", state_is(AudienceSourceState::Live)).await;
    assert_eq!(renewals.load(Ordering::SeqCst), 1);
    let handshakes = server.handshakes.lock().unwrap().clone();
    assert_eq!(
        handshakes[0],
        (
            "/v1/engage/75XNMW/ws?role=operator".into(),
            "Bearer old-token".into()
        )
    );
    assert_eq!(handshakes[1].1, "Bearer new-token");
    // The session carries a QR code and no token.
    let session = log
        .lock()
        .unwrap()
        .iter()
        .find_map(|u| u.session.clone().flatten())
        .unwrap();
    assert!(session.qr_data_url.starts_with("data:image/png;base64,"));
    assert!(!serde_json::to_string(&session).unwrap().contains("token"));
    source.dispose();
}

#[tokio::test]
async fn a_second_rejection_or_no_renewal_asks_to_reopen() {
    let server = server(|_, _| Some(401), vec![]).await;
    let renewals = Arc::new(AtomicUsize::new(0));
    let source = KairoAudienceSource::new(
        server.base.clone(),
        fetcher("a", Arc::default()),
        None,
        Some(fetcher("b", renewals.clone())),
    );
    let log = watch(&source);
    source.start().await.unwrap();
    wait_for(
        &log,
        "rejected",
        error_is("Engage access was rejected. Open this engagement again to reconnect."),
    )
    .await;
    assert_eq!(renewals.load(Ordering::SeqCst), 1, "renews once only");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        server.handshakes.lock().unwrap().len(),
        2,
        "no reconnect loop while access is rejected"
    );
    source.dispose();

    let source = KairoAudienceSource::new(
        server.base.clone(),
        fetcher("a", Arc::default()),
        None,
        None,
    );
    let log = watch(&source);
    source.start().await.unwrap();
    wait_for(
        &log,
        "rejected",
        error_is("Engage access was rejected. Open this engagement again to reconnect."),
    )
    .await;
    source.dispose();
}

#[tokio::test]
async fn a_failed_renewal_reports_expired_access() {
    let server = server(|_, _| Some(403), vec![]).await;
    let failing: RoomFetcher =
        Arc::new(|| Box::pin(async { Err(kairo_audience::AudienceError::new("offline")) }));
    let source = KairoAudienceSource::new(
        server.base.clone(),
        fetcher("a", Arc::default()),
        None,
        Some(failing),
    );
    let log = watch(&source);
    source.start().await.unwrap();
    wait_for(
        &log,
        "expired",
        error_is("Engage access expired. Open this engagement again to reconnect."),
    )
    .await;
    source.dispose();
}

#[tokio::test]
async fn a_gone_room_ends_the_session() {
    for code in [404, 410] {
        let server = server(move |_, _| Some(code), vec![]).await;
        let source = KairoAudienceSource::new(
            server.base.clone(),
            fetcher("a", Arc::default()),
            None,
            None,
        );
        let log = watch(&source);
        source.start().await.unwrap();
        wait_for(&log, "stopped", state_is(AudienceSourceState::Stopped)).await;
        assert!(log.lock().unwrap().iter().any(|u| u.session == Some(None)));
        assert_eq!(
            source
                .set_state("kairo:q".into(), AudienceQuestionState::Hidden)
                .unwrap()
                .await
                .unwrap_err()
                .0,
            "This session has ended."
        );
    }
}

#[tokio::test]
async fn commands_wait_for_their_ack_or_error() {
    let mut server = server(|_, _| None, vec![]).await;
    let source = KairoAudienceSource::new(
        server.base.clone(),
        fetcher("t", Arc::default()),
        None,
        None,
    );
    let log = watch(&source);
    // Not connected yet.
    assert_eq!(
        source
            .command(serde_json::from_value(json!({"type":"present","id":"q"})).unwrap())
            .unwrap()
            .await
            .unwrap_err()
            .0,
        "Wait for Engage to reconnect, then try again."
    );
    assert_eq!(
        source
            .set_settings(Default::default())
            .unwrap()
            .await
            .unwrap_err()
            .0,
        "Wait until the Q&A is connected, then try again."
    );
    source.start().await.unwrap();
    wait_for(&log, "live", state_is(AudienceSourceState::Live)).await;
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|u| u.status.as_ref().and_then(|s| s.joined) == Some(2.0))
    );

    let s = source.clone();
    let ok = tokio::spawn(async move {
        s.command(
            serde_json::from_value(
                json!({"type":"reply","questionId":"q","text":"Hi","audience":"everyone"}),
            )
            .unwrap(),
        )
        .unwrap()
        .await
    });
    let sent = server.next().await;
    assert_eq!(sent["type"], "reply");
    assert_eq!(sent["questionId"], "q");
    server.send(json!({ "type": "ack", "requestId": sent["requestId"] }));
    ok.await.unwrap().unwrap();

    let s = source.clone();
    let rejected = tokio::spawn(async move {
        s.command(serde_json::from_value(json!({"type":"edit","id":"q","text":"x"})).unwrap())
            .unwrap()
            .await
    });
    let sent = server.next().await;
    server.send(json!({ "type": "error", "message": "That question is not in this room.", "requestId": sent["requestId"] }));
    assert_eq!(
        rejected.await.unwrap().unwrap_err().0,
        "That question is not in this room."
    );

    // Settings apply to the session only after the ack.
    let s = source.clone();
    let settings = tokio::spawn(async move {
        s.set_settings(
            serde_json::from_value(json!({"review": true, "showQuestions": false, "names": "off"}))
                .unwrap(),
        )
        .unwrap()
        .await
    });
    let sent = server.next().await;
    assert_eq!(sent["settings"]["names"], "off");
    server.send(json!({ "type": "ack", "requestId": sent["requestId"] }));
    settings.await.unwrap().unwrap();
    wait_for(&log, "settings", |u| {
        u.session
            .clone()
            .flatten()
            .is_some_and(|s| s.settings.review)
    })
    .await;

    // A room-wide error marks the status, not a request.
    server.send(json!({ "type": "error", "message": "Room is busy" }));
    wait_for(&log, "room error", error_is("Room is busy")).await;

    // Engage snapshots and questions pass through; bad JSON is ignored.
    server
        .push
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .send(Message::Text("not json".into()))
        .unwrap();
    server.send(json!({ "type": "question", "question": { "id": "kairo:1", "sourceId": "kairo", "text": "Q", "author": null, "votes": 1, "state": "approved", "createdAt": 1 } }));
    wait_for(&log, "question", |u| {
        u.questions
            .as_ref()
            .is_some_and(|q| q.len() == 1 && q[0].id == "kairo:1")
    })
    .await;

    // No ack: the request times out after 8 s.
    let started = Instant::now();
    let silent = source
        .command(serde_json::from_value(json!({"type":"present","id":"q"})).unwrap())
        .unwrap()
        .await;
    assert_eq!(
        silent.unwrap_err().0,
        "That change did not go through. Try again."
    );
    assert!(started.elapsed() >= Duration::from_secs(8));
    let _ = server.next().await;

    // A pending request is told when the engagement goes away.
    let s = source.clone();
    let orphan = tokio::spawn(async move {
        s.command(serde_json::from_value(json!({"type":"present","id":"q"})).unwrap())
            .unwrap()
            .await
    });
    let _ = server.next().await;
    server.send(json!({ "type": "ended" }));
    assert_eq!(
        orphan.await.unwrap().unwrap_err().0,
        "Engagement disconnected."
    );
    wait_for(&log, "stopped", state_is(AudienceSourceState::Stopped)).await;
}

#[tokio::test]
async fn moderation_queues_while_offline_and_resends_after_each_snapshot() {
    let mut server = server(|_, _| None, vec![Script::CloseAfterSnapshot]).await;
    let source = KairoAudienceSource::new(
        server.base.clone(),
        fetcher("t", Arc::default()),
        None,
        None,
    );
    let log = watch(&source);
    source.start().await.unwrap();
    // The first socket closes after its snapshot; queue while it is away.
    wait_for(&log, "reconnecting", |u| {
        u.status.as_ref().is_some_and(|s| {
            s.state == AudienceSourceState::Connecting
                && s.error.as_deref() == Some("Reconnecting. Your questions are safe.")
        })
    })
    .await;
    source
        .set_state("kairo:a".into(), AudienceQuestionState::Approved)
        .unwrap()
        .await
        .unwrap();
    source
        .set_state("kairo:a".into(), AudienceQuestionState::Hidden)
        .unwrap()
        .await
        .unwrap();
    source
        .set_state("kairo:b".into(), AudienceQuestionState::Answered)
        .unwrap()
        .await
        .unwrap();
    // Reconnect comes after about a second (attempt 0: 1 s plus jitter).
    let first = server.next().await;
    let second = server.next().await;
    assert_eq!(
        (first["id"].as_str(), first["state"].as_str()),
        (Some("kairo:a"), Some("hidden")),
        "only the latest per question"
    );
    assert_eq!(
        (second["id"].as_str(), second["state"].as_str()),
        (Some("kairo:b"), Some("answered"))
    );
    // Acked moderation is not resent.
    server.send(json!({ "type": "ack", "requestId": first["requestId"] }));
    server.send(
        json!({ "type": "snapshot", "joined": 3, "questions": [], "settings": room("x").settings }),
    );
    let again = server.next().await;
    assert_eq!(again["id"], "kairo:b");
    source.dispose();
}

#[tokio::test]
async fn reconnects_alternate_with_the_fallback_base() {
    let primary = server(
        |_, _| None,
        vec![Script::CloseAfterSnapshot, Script::CloseAfterSnapshot],
    )
    .await;
    let fallback = server(|_, _| None, vec![Script::CloseAfterSnapshot]).await;
    let source = KairoAudienceSource::new(
        primary.base.clone(),
        fetcher("t", Arc::default()),
        Some(fallback.base.clone()),
        None,
    );
    let log = watch(&source);
    source.start().await.unwrap();
    // Snapshot resets the attempt count, so every reconnect after a live
    // socket is attempt 1, which goes to the fallback.
    let deadline = Instant::now() + Duration::from_secs(10);
    while fallback.handshakes.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "fallback never tried");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .filter(|u| state_is(AudienceSourceState::Live)(u))
            .count()
            >= 1
    );
    source.dispose();
}

#[tokio::test]
async fn ending_needs_a_live_socket_and_an_ack() {
    let mut server = server(|_, _| None, vec![]).await;
    // Without a room, stop just finishes.
    let idle = KairoAudienceSource::new(
        server.base.clone(),
        fetcher("t", Arc::default()),
        None,
        None,
    );
    let idle_log = watch(&idle);
    idle.stop().await.unwrap();
    assert!(
        idle_log
            .lock()
            .unwrap()
            .iter()
            .any(state_is(AudienceSourceState::Stopped))
    );
    assert_eq!(
        idle.manage("open".into(), json!({}))
            .unwrap()
            .await
            .unwrap_err()
            .0,
        "Open an engagement first."
    );

    let unreachable = KairoAudienceSource::new(
        "http://127.0.0.1:9",
        fetcher("t", Arc::default()),
        None,
        None,
    );
    let log = watch(&unreachable);
    unreachable.start().await.unwrap();
    wait_for(&log, "lost", error_is("Lost the connection. Reconnecting…")).await;
    assert_eq!(
        unreachable.stop().await.unwrap_err().0,
        "Wait until the Q&A reconnects, then end it, so everyone sees it has ended."
    );
    unreachable.dispose();

    let source = KairoAudienceSource::new(
        server.base.clone(),
        fetcher("t", Arc::default()),
        None,
        None,
    );
    let log = watch(&source);
    source.start().await.unwrap();
    wait_for(&log, "live", state_is(AudienceSourceState::Live)).await;
    let s = source.clone();
    let ending = tokio::spawn(async move { s.stop().await });
    let sent = server.next().await;
    assert_eq!(sent["type"], "end");
    server.send(json!({ "type": "ack", "requestId": sent["requestId"] }));
    ending.await.unwrap().unwrap();
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(state_is(AudienceSourceState::Stopped))
    );
    // Stopping twice is a no-op.
    source.stop().await.unwrap();
}

/// `cargo test -p kairo-audience --test socket an_unanswered_ping_terminates -- --ignored`
#[tokio::test]
#[ignore = "slow: waits two 15 s heartbeats"]
async fn an_unanswered_ping_terminates_and_reconnects() {
    // A server that stops reading after the snapshot never answers pings.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let a = accepted.clone();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            a.fetch_add(1, Ordering::SeqCst);
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            socket
                .send(Message::Text(json!({ "type": "snapshot", "joined": 0, "questions": [], "settings": room("x").settings }).to_string().into()))
                .await
                .unwrap();
            held.push(socket);
        }
    });
    let source = KairoAudienceSource::new(base, fetcher("t", Arc::default()), None, None);
    let log = watch(&source);
    let started = Instant::now();
    source.start().await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    while accepted.load(Ordering::SeqCst) < 2 {
        assert!(Instant::now() < deadline, "never reconnected");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_secs(30),
        "terminated after {elapsed:?}"
    );
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(error_is("Reconnecting. Your questions are safe."))
    );
    eprintln!("terminated and reconnected after {elapsed:?}");
    source.dispose();
}

fn big_snapshot(target: usize) -> String {
    let text = "Q".repeat(270);
    let mut questions = Vec::new();
    let mut size = 0;
    let mut i = 0;
    while size < target {
        let q = json!({ "id": format!("kairo:{i:08}"), "sourceId": "kairo", "text": text, "author": "Someone", "votes": i % 50, "state": "approved", "createdAt": 1_700_000_000_000u64 + i });
        size += q.to_string().len() + 1;
        questions.push(q);
        i += 1;
    }
    json!({ "type": "snapshot", "joined": 0, "questions": questions, "settings": room("x").settings }).to_string()
}

/// `cargo test -p kairo-audience --release --test socket snapshot_at_the_cap -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "timing, run by hand"]
async fn snapshot_at_the_cap_parses_off_the_service() {
    let cap = kairo_domain::audience::AUDIENCE_MAX_SNAPSHOT_BYTES;
    let snapshot = big_snapshot(cap - 64 * 1024);
    assert!(snapshot.len() <= cap);
    let count = serde_json::from_str::<Value>(&snapshot).unwrap()["questions"]
        .as_array()
        .unwrap()
        .len();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (sent_tx, mut sent_rx) = mpsc::unbounded_channel::<Instant>();
    let over = "x".repeat(cap + 1);
    let accepted = Arc::new(AtomicUsize::new(0));
    let a = accepted.clone();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            let n = a.fetch_add(1, Ordering::SeqCst);
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            if n == 0 {
                // Over the cap: the client drops the socket and reconnects.
                let _ = socket.send(Message::Text(over.clone().into())).await;
            } else {
                let _ = sent_tx.send(Instant::now());
                socket
                    .send(Message::Text(snapshot.clone().into()))
                    .await
                    .unwrap();
            }
            held.push(socket);
        }
    });
    let source = KairoAudienceSource::new(base, fetcher("t", Arc::default()), None, None);
    let (got_tx, mut got_rx) = mpsc::unbounded_channel::<(Instant, usize)>();
    source.subscribe(Arc::new(move |u: AudienceUpdate| {
        if let Some(q) = u.questions.filter(|q| !q.is_empty()) {
            let _ = got_tx.send((Instant::now(), q.len()));
        }
    }));
    source.start().await.unwrap();
    let sent = tokio::time::timeout(Duration::from_secs(20), sent_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let (got, n) = tokio::time::timeout(Duration::from_secs(20), got_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(n, count);
    assert!(
        accepted.load(Ordering::SeqCst) >= 2,
        "the oversized message forced a reconnect"
    );
    eprintln!(
        "{} bytes, {count} questions: sent to parsed update {:?}",
        cap - 64 * 1024,
        got - sent
    );
    // Parse alone, as the socket task does it.
    let text = big_snapshot(cap - 64 * 1024);
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let v: Value = serde_json::from_str(&text).unwrap();
        let q: Vec<kairo_domain::audience::AudienceQuestion> = v["questions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|q| serde_json::from_value(q.clone()).ok())
            .collect();
        assert_eq!(q.len(), count);
        times.push(start.elapsed());
    }
    times.sort();
    eprintln!(
        "parse of {} bytes: median {:?}, min {:?}, max {:?}",
        text.len(),
        times[2],
        times[0],
        times[4]
    );
    source.dispose();
}
