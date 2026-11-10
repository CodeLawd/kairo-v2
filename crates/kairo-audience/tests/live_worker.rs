//! The audience service against the Electron checkout's real audience
//! Worker and `AudienceRoom` Durable Object, served locally by Miniflare
//! (`tools/audience-worker/serve.mjs`). Needs Node and the checkout's
//! `server/node_modules`:
//!
//! `cargo test -p kairo-audience --test live_worker -- --ignored --nocapture`
//!
//! The operator's traffic goes through a local TCP proxy, so the test can cut
//! the connection and watch the source reconnect.

use futures_util::{SinkExt, StreamExt};
use kairo_audience::{
    AudienceService, BoxFuture, EngageApi, EngageFlows, EngageMethod, QuestionOutput,
};
use kairo_domain::audience::{AudienceQuestionState, AudienceSourceState, AudienceState};
use kairo_domain::program::ProgramQuestion;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// The account server side of `engageRequest`, as a signed-in device.
struct DeviceApi {
    base: String,
    http: reqwest::Client,
}

impl EngageApi for DeviceApi {
    fn engage_request(
        &self,
        method: EngageMethod,
        path: &str,
        body: Option<Value>,
    ) -> BoxFuture<'static, kairo_audience::Result<Value>> {
        let url = format!("{}/v1/engage{path}", self.base);
        let request = match method {
            EngageMethod::Get => self.http.get(url),
            EngageMethod::Post => self.http.post(url).json(&body.unwrap_or(json!({}))),
        }
        .bearer_auth("device");
        Box::pin(async move {
            let response = request
                .send()
                .await
                .map_err(|e| kairo_audience::AudienceError::new(e.to_string()))?;
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(kairo_audience::AudienceError::new(format!(
                    "{status}: {text}"
                )));
            }
            Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
        })
    }
}

#[derive(Default)]
struct Screen {
    shown: Mutex<Vec<String>>,
}

impl QuestionOutput for Screen {
    fn show_question(&self, question: ProgramQuestion) {
        let label = match question {
            ProgramQuestion::Join { code, .. } => format!("join {code}"),
            ProgramQuestion::Question { text, .. } => format!("question {text}"),
        };
        self.shown.lock().unwrap().push(label);
    }
    fn clear_question(&self) {
        self.shown.lock().unwrap().push("clear".into());
    }
    fn show_poll(&self, poll: &kairo_domain::audience::PollView) {
        self.shown
            .lock()
            .unwrap()
            .push(format!("poll {}", poll.poll.title));
    }
}

/// Forwards to the Worker; `cut` drops every live connection.
struct Proxy {
    base: String,
    cut: tokio::sync::watch::Sender<u64>,
}

async fn proxy(target: std::net::SocketAddr) -> Proxy {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (cut, watch) = tokio::sync::watch::channel(0u64);
    tokio::spawn(async move {
        while let Ok((mut inbound, _)) = listener.accept().await {
            let mut watch = watch.clone();
            // Only cuts after this connection opened apply to it.
            watch.borrow_and_update();
            tokio::spawn(async move {
                let Ok(mut outbound) = TcpStream::connect(target).await else {
                    return;
                };
                tokio::select! {
                    _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => {}
                    _ = watch.changed() => {}
                }
            });
        }
    });
    Proxy { base, cut }
}

struct Participant {
    write: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
        Message,
    >,
    events: Arc<Mutex<Vec<Value>>>,
}

async fn join(base: &str, code: &str) -> Participant {
    let session = reqwest::get(format!("{base}/v1/engage/{code}/session"))
        .await
        .unwrap();
    assert_eq!(session.status(), 200, "audience session");
    let cookie = session.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut request = format!("{}/v1/engage/{code}/ws", base.replace("http", "ws"))
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Cookie", cookie.parse().unwrap());
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (write, mut read) = socket.split();
    let events: Arc<Mutex<Vec<Value>>> = Arc::default();
    let e = events.clone();
    tokio::spawn(async move {
        while let Some(Ok(message)) = read.next().await {
            if let Message::Text(text) = message {
                e.lock().unwrap().push(serde_json::from_str(&text).unwrap());
            }
        }
    });
    Participant { write, events }
}

impl Participant {
    async fn send(&mut self, value: Value) {
        self.write
            .send(Message::Text(value.to_string().into()))
            .await
            .unwrap();
    }
    async fn wait(&self, what: &str, predicate: impl Fn(&Value) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.events.lock().unwrap().iter().any(&predicate) {
            assert!(Instant::now() < deadline, "participant never saw {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

async fn until(
    service: &AudienceService,
    what: &str,
    predicate: impl Fn(&AudienceState) -> bool,
) -> AudienceState {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = service.state();
        if predicate(&state) {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {state:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn live(state: &AudienceState) -> bool {
    state
        .sources
        .iter()
        .any(|s| s.id == "kairo" && s.state == AudienceSourceState::Live)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Node and the Electron checkout's server/node_modules"]
async fn create_join_ask_vote_moderate_poll_reconnect_end() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut worker = tokio::process::Command::new("node")
        .arg(root.join("tools/audience-worker/serve.mjs"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("node");
    let mut lines = BufReader::new(worker.stdout.take().unwrap()).lines();
    let base = loop {
        let line = tokio::time::timeout(Duration::from_secs(60), lines.next_line())
            .await
            .expect("worker start")
            .unwrap()
            .expect("READY line");
        if let Some(url) = line.strip_prefix("READY ") {
            break url.to_owned();
        }
    };
    let target: std::net::SocketAddr = base.trim_start_matches("http://").parse().unwrap();
    let proxy = proxy(target).await;

    let screen = Arc::new(Screen::default());
    let service = AudienceService::new(screen.clone(), false);
    let api = Arc::new(DeviceApi {
        base: base.clone(),
        http: reqwest::Client::new(),
    });
    let flows = EngageFlows::new(service.clone(), api, proxy.base.clone(), None, false);

    // Create a draft engagement, find it in the list, resume it (which opens a draft).
    flows
        .create(&serde_json::from_value(json!({ "title": "Sunday", "status": "draft", "settings": { "review": true, "showQuestions": true, "names": "optional" } })).unwrap())
        .await
        .unwrap();
    let listed = flows.list().await.unwrap();
    let code = listed
        .iter()
        .find(|e| e.name == "Sunday")
        .expect("listed")
        .code
        .clone();
    let started = Instant::now();
    flows.resume(&code, false).await.unwrap();
    let state = until(&service, "live", live).await;
    eprintln!("resume to live: {:?}", started.elapsed());
    let session = state.session.clone().unwrap();
    assert_eq!(session.code, code);
    assert!(session.settings.review);
    assert!(session.qr_data_url.starts_with("data:image/png;base64,"));
    assert!(
        !serde_json::to_string(&state)
            .unwrap()
            .contains("operatorToken")
    );
    assert_eq!(
        flows
            .list()
            .await
            .unwrap()
            .iter()
            .find(|e| e.code == code)
            .unwrap()
            .status,
        kairo_domain::audience::EngagementStatus::Open
    );
    service.show_join_code().unwrap();

    // Join and ask; review is on, so the question waits.
    let mut alice = join(&base, &code).await;
    let mut bob = join(&base, &code).await;
    // The room sends `joined: 0` and no `joined` events; wait for the snapshots.
    alice.wait("snapshot", |e| e["type"] == "snapshot").await;
    bob.wait("snapshot", |e| e["type"] == "snapshot").await;
    alice.send(json!({ "type": "ask", "text": "  Why do we sing?  ", "author": "Alice", "requestId": "ask-1" })).await;
    let state = until(&service, "question", |s| !s.questions.is_empty()).await;
    let question = state.questions[0].clone();
    assert_eq!(
        (question.text.as_str(), question.state),
        ("Why do we sing?", AudienceQuestionState::Review)
    );
    assert!(service.show(&question.id).is_err(), "review first");

    // Edits are for questions in review; the original is kept. Approved ones are refused.
    service
        .command(
            serde_json::from_value(
                json!({ "type": "edit", "id": question.id, "text": "Why do we sing together?" }),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    until(&service, "edit", |s| {
        s.questions.iter().any(|q| {
            q.text == "Why do we sing together?"
                && q.original_text.as_deref() == Some("Why do we sing?")
        })
    })
    .await;
    // Approve; Bob sees it and votes; the vote reaches the operator.
    service.moderate(&question.id, "approved").await.unwrap();
    bob.wait("approved question", |e| {
        e["type"] == "question"
            || (e["type"] == "snapshot" && !e["questions"].as_array().unwrap().is_empty())
    })
    .await;
    bob.send(json!({ "type": "vote", "id": question.id, "voted": true, "requestId": "vote-1" }))
        .await;
    until(&service, "vote", |s| {
        s.questions
            .iter()
            .any(|q| q.id == question.id && q.votes == 1.0)
    })
    .await;
    service.show(&question.id).unwrap();
    assert_eq!(
        service
            .command(
                serde_json::from_value(
                    json!({ "type": "edit", "id": question.id, "text": "Changed" })
                )
                .unwrap()
            )
            .await
            .unwrap_err()
            .0,
        "Only questions in Review can be edited (up to 280 characters)."
    );

    // Reply through the operator commands.
    service
        .command(serde_json::from_value(json!({ "type": "reply", "questionId": question.id, "text": "Because joy", "audience": "everyone" })).unwrap())
        .await
        .unwrap();
    bob.wait("reply", |e| {
        e["type"] == "engage"
            && e["replies"]
                .as_array()
                .is_some_and(|r| r.iter().any(|r| r["text"] == "Because joy"))
    })
    .await;
    let rejected = service
        .command(
            serde_json::from_value(json!({ "type": "present", "id": "kairo:missing" })).unwrap(),
        )
        .await;
    eprintln!("present of a missing question: {rejected:?}");
    assert!(
        rejected.is_err(),
        "the room rejects a missing question by request id"
    );

    // Polls: save, open, vote, show results.
    service.command(serde_json::from_value(json!({ "type": "poll-save", "poll": { "title": "Next hymn?", "options": ["A", "B"] } })).unwrap()).await.unwrap();
    let state = until(&service, "poll", |s| {
        s.engage.as_ref().is_some_and(|e| !e.polls.is_empty())
    })
    .await;
    let poll = state.engage.unwrap().polls[0].poll.clone();
    service.command(serde_json::from_value(json!({ "type": "poll-state", "id": poll.id, "state": "open", "showResults": true })).unwrap()).await.unwrap();
    service.show_poll(&poll.id).unwrap();
    alice
        .wait("open poll", |e| {
            e["type"] == "engage"
                && e["polls"]
                    .as_array()
                    .is_some_and(|p| p.iter().any(|p| p["state"] == "open"))
        })
        .await;
    for p in [&mut alice, &mut bob] {
        p.send(json!({ "type": "poll-vote", "id": poll.id, "options": [poll.options[0].id], "requestId": "pv" })).await;
    }
    until(&service, "poll votes", |s| {
        s.engage.as_ref().is_some_and(|e| {
            e.polls[0].voters == 2.0 && e.polls[0].totals.get(&poll.options[0].id) == Some(&2.0)
        })
    })
    .await;
    assert!(
        screen
            .shown
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.starts_with("poll "))
            .count()
            >= 2,
        "live poll follows results"
    );

    // Room settings round-trip through the ack.
    service
        .set_room_settings(
            serde_json::from_value(
                json!({ "review": false, "showQuestions": true, "names": "off" }),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        service.state().session.unwrap().settings.names,
        kairo_domain::audience::AudienceNames::Off
    );

    // Moderators: a bad address is the room's message; invite then revoke.
    assert_eq!(
        flows.invite("not-an-email", "X").await.unwrap_err().0,
        "Enter a valid email."
    );
    flows.invite("team@example.com", "Jane").await.unwrap();
    let state = until(&service, "moderator", |s| {
        s.engage
            .as_ref()
            .and_then(|e| e.moderators.as_ref())
            .is_some_and(|m| !m.is_empty())
    })
    .await;
    let moderator = state.engage.unwrap().moderators.unwrap()[0].clone();
    assert_eq!(
        (moderator.email.as_str(), moderator.name.as_deref()),
        ("team@example.com", Some("Jane"))
    );
    flows.revoke(&moderator.id).await.unwrap();
    until(&service, "revoked", |s| {
        s.engage
            .as_ref()
            .and_then(|e| e.moderators.as_ref())
            .is_some_and(|m| m[0].revoked)
    })
    .await;

    // Cut the operator's connection. Moderation made during the outage
    // reaches the room after the reconnect.
    // One question per 20 s per person, so Bob asks this one.
    bob.send(json!({ "type": "ask", "text": "Second question", "requestId": "ask-2" }))
        .await;
    let second = until(&service, "second", |s| {
        s.questions.iter().any(|q| q.text == "Second question")
    })
    .await
    .questions
    .into_iter()
    .find(|q| q.text == "Second question")
    .unwrap();
    let cut_at = Instant::now();
    proxy.cut.send_modify(|n| *n += 1);
    until(&service, "reconnecting", |s| {
        s.sources
            .iter()
            .any(|x| x.state != AudienceSourceState::Live)
    })
    .await;
    service.moderate(&second.id, "hidden").await.unwrap();
    until(&service, "live again", live).await;
    eprintln!("reconnected {:?} after the cut", cut_at.elapsed());
    let hidden = |e: &Value| {
        (e["type"] == "state" && e["id"] == second.id.as_str() && e["state"] == "hidden")
            || (e["type"] == "snapshot"
                && e["questions"].as_array().is_some_and(|q| {
                    q.iter()
                        .any(|q| q["id"] == second.id.as_str() && q["state"] == "hidden")
                }))
    };
    // The asker's own question shows its state.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        bob.send(json!({ "type": "snapshot" })).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let seen = bob.events.lock().unwrap().iter().any(|e| {
            hidden(e)
                || (e["type"] == "snapshot"
                    && e["ownQuestions"].as_array().is_some_and(|q| {
                        q.iter()
                            .any(|q| q["id"] == second.id.as_str() && q["state"] == "hidden")
                    }))
        });
        if seen {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "queued moderation never reached the room"
        );
    }
    assert_eq!(
        service
            .state()
            .questions
            .iter()
            .find(|q| q.id == second.id)
            .unwrap()
            .state,
        AudienceQuestionState::Hidden
    );

    // Answering the live question takes it off screen.
    service.moderate(&question.id, "answered").await.unwrap();
    assert_eq!(service.state().live_id, None);

    // End for everyone, then delete; the list no longer has it.
    service.stop("kairo").await.unwrap();
    alice
        .wait("ended", |e| {
            e["type"] == "ended" || (e["type"] == "snapshot" && e["ended"] == true)
        })
        .await;
    assert!(service.state().session.is_none());
    flows.resume(&code, true).await.unwrap();
    until(&service, "reopened", live).await;
    flows.delete(&code).await.unwrap();
    assert!(
        service.state().session.is_none(),
        "deleting the open engagement detaches it"
    );
    assert!(flows.list().await.unwrap().iter().all(|e| e.code != code));
    assert!(
        flows.resume(&code, false).await.is_err(),
        "a deleted engagement cannot be resumed"
    );

    // `audience:start` reserves a fresh room.
    let state = flows
        .start("kairo", &serde_json::from_value(json!({ "title": "Evening", "settings": { "review": false, "showQuestions": true, "names": "optional" } })).unwrap())
        .await
        .unwrap();
    assert_eq!(state.session.as_ref().unwrap().title, "Evening");
    until(&service, "new room live", live).await;
    service.stop("kairo").await.unwrap();
    service.shutdown();
    drop(worker.stdin.take());
    let _ = tokio::time::timeout(Duration::from_secs(10), worker.wait()).await;
}
