//! `tests/audience-service.test.ts` and the service half of
//! `tests/engage-socket-recovery.test.ts`, plus the paths those leave out.

use kairo_audience::{
    AudienceService, AudienceSource, AudienceUpdate, BoxFuture, EngageApi, EngageFlows,
    EngageMethod, QuestionOutput, SampleAudienceSource, UpdateSink, sample_allowed,
};
use kairo_domain::audience::{
    AudienceQuestion, AudienceQuestionState, AudienceSession, AudienceSourceOptions,
    AudienceSourceState, AudienceSourceStatus, AudienceStatusKind,
};
use kairo_domain::program::ProgramQuestion;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Screen {
    live: Mutex<Option<ProgramQuestion>>,
    polls: Mutex<Vec<String>>,
}

impl QuestionOutput for Screen {
    fn show_question(&self, question: ProgramQuestion) {
        *self.live.lock().unwrap() = Some(question);
    }
    fn clear_question(&self) {
        *self.live.lock().unwrap() = None;
    }
    fn show_poll(&self, poll: &kairo_domain::audience::PollView) {
        self.polls.lock().unwrap().push(poll.poll.id.clone());
    }
    fn clear_poll(&self) {
        self.polls.lock().unwrap().push("clear".into());
    }
}

fn service() -> (AudienceService, Arc<Screen>) {
    let screen = Arc::new(Screen::default());
    (AudienceService::new(screen.clone(), false), screen)
}

fn status(id: &str, state: AudienceSourceState) -> AudienceSourceStatus {
    serde_json::from_value(
        json!({ "id": id, "kind": "kairo", "label": "Kairo Q&A", "state": state }),
    )
    .unwrap()
}

fn session() -> AudienceSession {
    serde_json::from_value(json!({
        "roomId": "room", "code": "ABCDEF", "title": "Test", "url": "https://example.com", "qrDataUrl": "",
        "settings": { "review": false, "showQuestions": true, "names": "optional" }
    }))
    .unwrap()
}

fn question(id: &str, source: &str, text: &str) -> AudienceQuestion {
    serde_json::from_value(json!({ "id": id, "sourceId": source, "text": text, "author": null, "votes": 0, "state": "review", "createdAt": 100 })).unwrap()
}

/// A scripted source: `start` publishes `on_start`; the test publishes more.
struct Fake {
    id: String,
    sink: Mutex<Option<UpdateSink>>,
    on_start: Mutex<Option<AudienceUpdate>>,
    started: AtomicBool,
    stopped: AtomicBool,
    disposed: AtomicBool,
    fail_start: bool,
}

impl Fake {
    fn new(id: &str, on_start: AudienceUpdate) -> Arc<Self> {
        Arc::new(Self {
            id: id.into(),
            sink: Mutex::default(),
            on_start: Mutex::new(Some(on_start)),
            started: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            disposed: AtomicBool::new(false),
            fail_start: false,
        })
    }
    fn publish(&self, update: AudienceUpdate) {
        let sink = self.sink.lock().unwrap().clone();
        if let Some(sink) = sink {
            sink(update);
        }
    }
}

impl AudienceSource for Fake {
    fn id(&self) -> &str {
        &self.id
    }
    fn subscribe(&self, sink: UpdateSink) {
        *self.sink.lock().unwrap() = Some(sink);
    }
    fn start(&self) -> BoxFuture<'_, kairo_audience::Result<()>> {
        Box::pin(async move {
            self.started.store(true, Ordering::SeqCst);
            if let Some(update) = self.on_start.lock().unwrap().take() {
                self.publish(update);
            }
            if self.fail_start {
                return Err(kairo_audience::AudienceError::new("could not reserve"));
            }
            Ok(())
        })
    }
    fn stop(&self) -> BoxFuture<'_, kairo_audience::Result<()>> {
        Box::pin(async move {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
    fn dispose(&self) {
        self.disposed.store(true, Ordering::SeqCst);
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn questions_require_approval_show_separately_and_answering_clears() {
    rt().block_on(async {
        let (service, screen) = service();
        service
            .start(Arc::new(SampleAudienceSource::new()))
            .await
            .unwrap();
        assert_eq!(service.state().questions.len(), 3);
        assert!(service.show("sample:0").unwrap_err().0.contains("Approve"));
        service.moderate("sample:0", "approved").await.unwrap();
        assert!(screen.live.lock().unwrap().is_none());
        service.show("sample:0").unwrap();
        assert_eq!(
            *screen.live.lock().unwrap(),
            Some(ProgramQuestion::Question {
                text: "How can we put what we learned today into practice this week?".into(),
                author: Some("Ruth".into()),
            })
        );
        service.moderate("sample:0", "answered").await.unwrap();
        assert!(screen.live.lock().unwrap().is_none());
        assert_eq!(service.state().live_id, None);
        let mut copy = service.state();
        copy.questions.clear();
        assert_eq!(service.state().questions.len(), 3);
        service.stop("sample").await.unwrap();
        assert_eq!(
            service.state().sources[0].state,
            AudienceSourceState::Stopped
        );
        assert!(service.state().questions.is_empty());
    });
}

#[test]
fn a_room_ending_remotely_clears_the_join_card_and_allows_a_new_session() {
    rt().block_on(async {
        let (service, screen) = service();
        let make = || {
            Fake::new(
                "kairo",
                AudienceUpdate {
                    session: Some(Some(session())),
                    status: Some(status("kairo", AudienceSourceState::Live)),
                    ..Default::default()
                },
            )
        };
        let first = make();
        service.start(first.clone()).await.unwrap();
        first.publish(AudienceUpdate {
            questions: Some(vec![question("kairo:old", "kairo", "Previous room")]),
            ..Default::default()
        });
        service.show_join_code().unwrap();
        assert!(screen.live.lock().unwrap().is_some());
        first.publish(AudienceUpdate {
            status: Some(status("kairo", AudienceSourceState::Stopped)),
            session: Some(None),
            ..Default::default()
        });
        assert!(screen.live.lock().unwrap().is_none());
        assert_eq!(service.state().live_id, None);
        service.start(make()).await.unwrap();
        assert_eq!(service.state().session.unwrap().code, "ABCDEF");
        assert!(service.state().questions.is_empty());
    });
}

#[test]
fn stopping_an_unrelated_source_keeps_the_join_card() {
    rt().block_on(async {
        let (service, screen) = service();
        service
            .start(Fake::new(
                "kairo",
                AudienceUpdate {
                    session: Some(Some(session())),
                    ..Default::default()
                },
            ))
            .await
            .unwrap();
        service
            .start(Arc::new(SampleAudienceSource::new()))
            .await
            .unwrap();
        service.show_join_code().unwrap();
        service.stop("sample").await.unwrap();
        assert!(screen.live.lock().unwrap().is_some());
        assert_eq!(service.state().live_id.as_deref(), Some("join"));
    });
}

#[test]
fn shutdown_disposes_without_awaiting_a_remote_end() {
    rt().block_on(async {
        let (service, _) = service();
        let source = Fake::new(
            "slido:test",
            AudienceUpdate {
                status: Some(status("slido:test", AudienceSourceState::Live)),
                ..Default::default()
            },
        );
        service.start(source.clone()).await.unwrap();
        service.shutdown();
        assert!(source.disposed.load(Ordering::SeqCst));
        assert!(!source.stopped.load(Ordering::SeqCst));
        assert_eq!(
            service.state().sources[0].state,
            AudienceSourceState::Stopped
        );
        source.publish(AudienceUpdate {
            status: Some(status("slido:test", AudienceSourceState::Live)),
            ..Default::default()
        });
        assert_eq!(
            service.state().sources[0].state,
            AudienceSourceState::Stopped
        );
    });
}

#[test]
fn repeated_connect_is_idempotent_and_keeps_the_queue() {
    rt().block_on(async {
        let (service, _) = service();
        service
            .start(Arc::new(SampleAudienceSource::new()))
            .await
            .unwrap();
        service.moderate("sample:0", "approved").await.unwrap();
        let duplicate = Fake::new("sample", AudienceUpdate::default());
        let state = service.start(duplicate.clone()).await.unwrap();
        assert!(!duplicate.started.load(Ordering::SeqCst));
        assert_eq!(state.questions.len(), 3);
        assert_eq!(state.questions[0].state, AudienceQuestionState::Approved);
    });
}

#[test]
fn sample_availability_is_explicit_and_defaults_off() {
    let screen = Arc::new(Screen::default());
    assert!(
        !AudienceService::new(screen.clone(), false)
            .state()
            .sample_available
    );
    assert!(AudienceService::new(screen, true).state().sample_available);
    assert!(!sample_allowed(true), "never in a packaged build");
}

#[test]
fn validation_errors_and_failed_starts() {
    rt().block_on(async {
        let (service, screen) = service();
        let counts = Arc::new(AtomicUsize::new(0));
        let c = counts.clone();
        service.on_changed(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(
            service.moderate("x", "approved").await.unwrap_err().0,
            "That question no longer exists."
        );
        assert_eq!(
            service.moderate("x", "bogus").await.unwrap_err().0,
            "Choose a valid question state."
        );
        assert_eq!(
            service.show_join_code().unwrap_err().0,
            "Start a Q&A session first."
        );
        assert_eq!(
            service.audience_page_url().unwrap_err().0,
            "Start a Q&A first."
        );
        assert_eq!(
            service.show_poll("p").unwrap_err().0,
            "Choose an existing poll."
        );
        assert_eq!(
            service
                .command(serde_json::from_value(json!({"type":"present","id":"q"})).unwrap())
                .await
                .unwrap_err()
                .0,
            "Open an engagement first."
        );
        assert_eq!(
            service.manage("open", json!({})).await.unwrap_err().0,
            "Open an engagement first."
        );
        assert_eq!(
            service
                .set_room_settings(Default::default())
                .await
                .unwrap_err()
                .0,
            "Start a Q&A first."
        );
        assert_eq!(service.stop("missing").await.unwrap().sources.len(), 0);

        // A start that fails is undone and its source told to stop.
        let failing = Arc::new(Fake {
            fail_start: true,
            ..Arc::into_inner(Fake::new("kairo", AudienceUpdate::default())).unwrap()
        });
        assert_eq!(
            service.start(failing.clone()).await.unwrap_err().0,
            "could not reserve"
        );
        assert!(failing.stopped.load(Ordering::SeqCst));
        failing.publish(AudienceUpdate {
            session: Some(Some(session())),
            ..Default::default()
        });
        assert!(
            service.state().session.is_none(),
            "unsubscribed after a failed start"
        );

        // sync_cleared only reports a change when something was live.
        let before = counts.load(Ordering::SeqCst);
        service.sync_cleared();
        assert_eq!(counts.load(Ordering::SeqCst), before);
        assert!(screen.live.lock().unwrap().is_none());
    });
}

#[test]
fn incoming_questions_are_sanitised_and_scoped_to_their_source() {
    rt().block_on(async {
        let (service, _) = service();
        let source = Fake::new(
            "kairo",
            AudienceUpdate {
                session: Some(Some(session())),
                ..Default::default()
            },
        );
        service.start(source.clone()).await.unwrap();
        let mut long_author = question("kairo:a", "kairo", "  Why?  ");
        long_author.author = Some(format!("  {}  ", "x".repeat(100)));
        long_author.votes = 3.7;
        let mut negative = question("kairo:b", "kairo", "Ok");
        negative.votes = -4.0;
        negative.author = Some("   ".into());
        let mut infinite = question("kairo:c", "kairo", "Inf");
        infinite.votes = f64::INFINITY;
        source.publish(AudienceUpdate {
            questions: Some(vec![
                long_author,
                negative,
                infinite,
                question("kairo:blank", "kairo", "   "),
                question("other:x", "kairo", "wrong prefix"),
                question("kairo:y", "sample", "wrong source"),
            ]),
            ..Default::default()
        });
        let q = service.state().questions;
        assert_eq!(
            q.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(),
            ["kairo:a", "kairo:b", "kairo:c"]
        );
        assert_eq!(q[0].text, "Why?");
        assert_eq!(q[0].author.as_deref().map(str::len), Some(80));
        assert_eq!(q[0].votes, 3.0);
        assert_eq!((q[1].votes, q[1].author.clone()), (0.0, None));
        assert_eq!(q[2].votes, 0.0);

        // A new room under the same source drops the old room's questions.
        let mut next = session();
        next.room_id = "room-2".into();
        source.publish(AudienceUpdate {
            session: Some(Some(next)),
            ..Default::default()
        });
        assert!(service.state().questions.is_empty());
    });
}

#[test]
fn polls_follow_engage_updates_and_detach_clears_everything() {
    rt().block_on(async {
        let (service, screen) = service();
        let source = Fake::new("kairo", AudienceUpdate { session: Some(Some(session())), ..Default::default() });
        service.start(source.clone()).await.unwrap();
        let engage = |voters: u32| -> kairo_domain::audience::EngageSnapshot {
            serde_json::from_value(json!({
                "engagement": { "id": "e", "code": "ABCDEF", "name": "N", "orgName": "O", "startsAt": null, "endsAt": null,
                    "status": "open", "activePollId": null,
                    "activities": { "qa": { "enabled": true, "review": false, "showQuestions": true, "names": "optional" }, "polls": { "enabled": true } } },
                "replies": [],
                "polls": [{ "id": "p1", "title": "T", "options": [], "multiple": false, "state": "open", "showResults": false,
                    "totals": {}, "voters": voters, "voted": [] }]
            }))
            .unwrap()
        };
        source.publish(AudienceUpdate { engage: Some(engage(0)), ..Default::default() });
        service.show_poll("p1").unwrap();
        source.publish(AudienceUpdate { engage: Some(engage(5)), ..Default::default() });
        assert_eq!(*screen.polls.lock().unwrap(), ["p1", "p1"], "a live poll refreshes on screen");
        service.show_join_code().unwrap();
        service.detach_engagement();
        let state = service.state();
        assert!(state.session.is_none() && state.engage.is_none() && state.live_id.is_none());
        assert_eq!(state.live_poll_id, Some(None));
        assert!(state.sources.is_empty());
        assert!(screen.live.lock().unwrap().is_none());
        assert!(source.disposed.load(Ordering::SeqCst));
    });
}

struct Api(Mutex<Vec<(EngageMethod, String, Option<Value>)>>);

impl EngageApi for Api {
    fn engage_request(
        &self,
        method: EngageMethod,
        path: &str,
        body: Option<Value>,
    ) -> BoxFuture<'static, kairo_audience::Result<Value>> {
        self.0.lock().unwrap().push((method, path.into(), body));
        Box::pin(async { Err(kairo_audience::AudienceError::new("offline")) })
    }
}

#[test]
fn engage_flows_check_codes_and_sources_before_any_request() {
    rt().block_on(async {
        let (service, _) = service();
        let api = Arc::new(Api(Mutex::default()));
        let flows = EngageFlows::new(service, api.clone(), "http://127.0.0.1:9", None, false);
        for bad in ["abcdef", "ABCDE", "ABCDEI", "ABCDEF1", "ABCDE0"] {
            assert_eq!(flows.resume(bad, false).await.unwrap_err().0, "Choose a valid engagement.");
            assert_eq!(flows.delete(bad).await.unwrap_err().0, "Choose a valid engagement.");
        }
        assert!(api.0.lock().unwrap().is_empty());
        let none = AudienceSourceOptions::default();
        assert_eq!(
            flows.start("sample", &none).await.unwrap_err().0,
            "Sample questions require KAIRO_AUDIENCE_SAMPLE=1 in development."
        );
        assert_eq!(flows.start("other", &none).await.unwrap_err().0, "That audience source is not available.");
        assert_eq!(flows.reopen().await.unwrap_err().0, "Open an engagement first.");
        assert_eq!(flows.invite("a@b.c", "A").await.unwrap_err().0, "Open an engagement first.");
        assert_eq!(flows.revoke("id").await.unwrap_err().0, "Open an engagement first.");

        // Starting a room sends the trimmed title, normalised settings and nulls as given.
        let options: AudienceSourceOptions =
            serde_json::from_value(json!({ "title": format!("  {}  ", "t".repeat(120)), "endsAt": null })).unwrap();
        assert_eq!(flows.start("kairo", &options).await.unwrap_err().0, "offline");
        let calls = api.0.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        let (method, path, body) = &calls[0];
        assert_eq!((*method, path.as_str()), (EngageMethod::Post, ""));
        assert_eq!(
            body.clone().unwrap(),
            json!({ "title": "t".repeat(100), "settings": { "review": false, "showQuestions": true, "names": "optional" }, "endsAt": null })
        );
        assert_eq!(flows.list().await.unwrap_err().0, "offline");
        assert_eq!(api.0.lock().unwrap().last().unwrap().0, EngageMethod::Get);
        let _ = AudienceStatusKind::Kairo;
    });
}

struct OneWindowFactory(Mutex<Option<kairo_audience::SlidoWindowConfig>>);

struct IdleWindow;

impl kairo_audience::SlidoWindow for IdleWindow {
    fn load_url(&self, _: &str) {}
    fn current_url(&self) -> String {
        String::new()
    }
    fn show(&self) {}
    fn hide(&self) {}
    fn focus(&self) {}
    fn is_visible(&self) -> bool {
        false
    }
    fn destroy(&self) {}
}

impl kairo_audience::SlidoWindowFactory for OneWindowFactory {
    fn open(
        &self,
        config: kairo_audience::SlidoWindowConfig,
        _events: kairo_audience::SlidoEventSink,
    ) -> kairo_audience::BoxFuture<'_, kairo_audience::Result<Arc<dyn kairo_audience::SlidoWindow>>>
    {
        *self.0.lock().unwrap() = Some(config);
        Box::pin(async { Ok(Arc::new(IdleWindow) as Arc<dyn kairo_audience::SlidoWindow>) })
    }
}

#[test]
fn audience_start_for_slido() {
    rt().block_on(async {
        let (service, _) = service();
        let api = Arc::new(Api(Mutex::default()));
        let flows = EngageFlows::new(service.clone(), api, "http://127.0.0.1:9", None, false);
        let with = |code: &str, moderator: bool| -> AudienceSourceOptions {
            serde_json::from_value(json!({ "code": code, "moderator": moderator })).unwrap()
        };
        // Without a window host the source is unavailable; a bad code is refused first.
        assert_eq!(
            flows
                .start("slido", &with("a b", false))
                .await
                .unwrap_err()
                .0,
            "That doesn’t look like a Slido code or link."
        );
        assert_eq!(
            flows
                .start("slido", &AudienceSourceOptions::default())
                .await
                .unwrap_err()
                .0,
            "That doesn’t look like a Slido code or link."
        );
        assert_eq!(
            flows
                .start("slido", &with("9374525", false))
                .await
                .unwrap_err()
                .0,
            "That audience source is not available."
        );

        let factory = Arc::new(OneWindowFactory(Mutex::default()));
        let flows = flows.with_slido(factory.clone());
        // A link works as well as a code.
        let state = flows
            .start(
                "slido",
                &with("https://app.sli.do/event/AbCdEf123/live/questions", true),
            )
            .await
            .unwrap();
        let config = factory.0.lock().unwrap().clone().unwrap();
        assert_eq!(config.start_url, "https://app.sli.do/event/AbCdEf123");
        assert!(config.moderator);
        let status = state
            .sources
            .iter()
            .find(|s| s.id == "slido:abcdef123")
            .unwrap();
        assert_eq!(status.can_open, Some(true));
        assert_eq!(status.skip_review, Some(false));
        service.stop("slido:abcdef123").await.unwrap();
    });
}
