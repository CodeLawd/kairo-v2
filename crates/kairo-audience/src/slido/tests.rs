use super::*;
use kairo_domain::audience::AudienceSourceState as S;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
struct FakeWindow {
    url: Mutex<String>,
    visible: Mutex<bool>,
    calls: Mutex<Vec<String>>,
}

impl FakeWindow {
    fn at(&self, url: &str) {
        *self.url.lock().unwrap() = url.into();
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl SlidoWindow for FakeWindow {
    fn load_url(&self, url: &str) {
        self.calls.lock().unwrap().push(format!("load {url}"));
    }
    fn current_url(&self) -> String {
        self.url.lock().unwrap().clone()
    }
    fn show(&self) {
        *self.visible.lock().unwrap() = true;
        self.calls.lock().unwrap().push("show".into());
    }
    fn hide(&self) {
        *self.visible.lock().unwrap() = false;
        self.calls.lock().unwrap().push("hide".into());
    }
    fn focus(&self) {
        self.calls.lock().unwrap().push("focus".into());
    }
    fn is_visible(&self) -> bool {
        *self.visible.lock().unwrap()
    }
    fn destroy(&self) {
        self.calls.lock().unwrap().push("destroy".into());
    }
}

#[derive(Default)]
struct FakeFactory {
    window: Arc<FakeWindow>,
    config: Mutex<Option<SlidoWindowConfig>>,
    fail: bool,
}

impl SlidoWindowFactory for FakeFactory {
    fn open(
        &self,
        config: SlidoWindowConfig,
        _events: SlidoEventSink,
    ) -> BoxFuture<'_, Result<Arc<dyn SlidoWindow>>> {
        Box::pin(async move {
            if self.fail {
                return Err(AudienceError::new("no window"));
            }
            *self.config.lock().unwrap() = Some(config);
            let window: Arc<dyn SlidoWindow> = self.window.clone();
            Ok(window)
        })
    }
}

struct Rig {
    source: SlidoAudienceSource,
    window: Arc<FakeWindow>,
    factory: Arc<FakeFactory>,
    updates: Arc<Mutex<Vec<AudienceUpdate>>>,
    now: Arc<AtomicU64>,
}

impl Rig {
    async fn new(event: SlidoEvent, moderator: bool, skip_review: bool) -> Self {
        let factory = Arc::new(FakeFactory::default());
        let now = Arc::new(AtomicU64::new(1_000_000));
        let clock = now.clone();
        let source = SlidoAudienceSource::with_clock(
            event,
            moderator,
            skip_review,
            factory.clone(),
            Arc::new(move || clock.load(Ordering::SeqCst) as f64),
        )
        .unwrap();
        let updates: Arc<Mutex<Vec<AudienceUpdate>>> = Arc::default();
        let sink = updates.clone();
        source.subscribe(Arc::new(move |u| sink.lock().unwrap().push(u)));
        source.start().await.unwrap();
        Self {
            source,
            window: factory.window.clone(),
            factory,
            updates,
            now,
        }
    }

    fn code(code: &str) -> SlidoEvent {
        SlidoEvent::Code { code: code.into() }
    }

    fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    fn read(&self, url: &str, message: Value) {
        self.window.at(url);
        self.source.deliver(SlidoWindowEvent::Message {
            frame_url: url.into(),
            body: message.to_string(),
        });
    }

    fn statuses(&self) -> Vec<AudienceSourceStatus> {
        self.updates
            .lock()
            .unwrap()
            .iter()
            .filter_map(|u| u.status.clone())
            .collect()
    }

    fn last_status(&self) -> AudienceSourceStatus {
        self.statuses().pop().unwrap()
    }

    fn question_updates(&self) -> Vec<AudienceUpdate> {
        self.updates
            .lock()
            .unwrap()
            .iter()
            .filter(|u| u.questions.is_some())
            .cloned()
            .collect()
    }
}

fn question(id: &str, text: &str, votes: i64) -> Value {
    json!({ "externalId": id, "text": text, "author": "Anna", "votes": votes })
}

fn page(questions: Vec<Value>) -> Value {
    json!({ "questions": questions, "empty": false, "notFound": false, "eventName": null })
}

const PUBLIC: &str = "https://app.sli.do/event/abc123/live/questions";
const ADMIN: &str = "https://admin.sli.do/event/abc123/polls";

#[test]
fn navigation_allowlist() {
    for ok in [
        "https://app.sli.do/event/test",
        "https://sli.do",
        "https://admin.sli.do/x",
        "https://www.slido.com/",
        "https://accounts.google.com/o/oauth2/auth?x=1",
        "https://login.webex.com/",
        "https://id.cisco.com/oauth",
    ] {
        assert!(allowed_slido_navigation(ok), "{ok}");
    }
    for bad in [
        "file:///tmp/x",
        "javascript:alert(1)",
        "https://sli.do.evil.example",
        "https://evilsli.do",
        "https://notgoogle.com",
        "http://app.sli.do",
        "data:text/html,hi",
        "about:blank",
        "not a url",
        "",
        "https://accounts.google.com.evil.example/",
    ] {
        assert!(!allowed_slido_navigation(bad), "{bad}");
    }
}

#[test]
fn frames_may_be_blank() {
    assert!(allowed_slido_frame_navigation("about:blank"));
    assert!(allowed_slido_frame_navigation("about:srcdoc"));
    assert!(allowed_slido_frame_navigation("https://app.sli.do/x"));
    assert!(!allowed_slido_frame_navigation("about:blank#x"));
    assert!(!allowed_slido_frame_navigation(
        "https://www.google.com/recaptcha"
    ));
}

#[test]
fn google_sign_in_steps_are_allowed_by_path() {
    for ok in [
        "https://www.recaptcha.net/recaptcha/api2/anchor?ar=1&size=invisible",
        "https://www.google.com/recaptcha/api2/bframe",
        "https://accounts.google.com.ng/accounts/SetSID?ssdc=1&sidt=x",
        "https://accounts.google.co.uk/accounts/SetSID",
        "https://accounts.google.de/accounts/SetSID",
    ] {
        assert!(allowed_slido_frame_navigation(ok), "{ok}");
    }
    for bad in [
        "https://www.recaptcha.net/",
        "https://www.google.com/search?q=x",
        "https://accounts.google.com.ng/",
        "https://accounts.google.com.ng/ServiceLogin",
        "https://accounts.google.com.evil.example/accounts/SetSID",
        "https://accounts.google.evil/accounts/SetSID",
        "https://accounts.google.co.evil.uk/accounts/SetSID",
        "http://accounts.google.com.ng/accounts/SetSID",
        "https://sites.google.com/recaptcha/",
    ] {
        assert!(!allowed_slido_frame_navigation(bad), "{bad}");
    }
}

#[test]
fn only_moderators_open_windows_to_allowed_hosts() {
    assert!(allowed_slido_window("https://accounts.google.com/x", true));
    assert!(!allowed_slido_window(
        "https://accounts.google.com/x",
        false
    ));
    assert!(!allowed_slido_window("https://evil.example/", true));
}

#[test]
fn stale_reads() {
    assert!(!slido_read_stale(None, 1e12));
    assert!(!slido_read_stale(Some(0.0), 29_999.0));
    assert!(slido_read_stale(Some(0.0), 30_000.0));
}

#[test]
fn page_script_embeds_config_and_selectors() {
    let script = page_script("9374525", true);
    assert!(script.contains(r#""code":"9374525""#));
    assert!(script.contains(r#""moderator":true"#));
    assert!(script.contains("question-item[data-qid]"));
    assert!(script.contains("window.ipc"));
    // A hostile code cannot break out of the string.
    assert!(page_script("\"};alert(1);//", false).contains(r#"\"};alert(1);//"#));
}

#[tokio::test]
async fn rejects_bad_codes() {
    let factory: Arc<dyn SlidoWindowFactory> = Arc::new(FakeFactory::default());
    for bad in ["", "a b", "x".repeat(65).as_str(), "a/b", "é"] {
        let error = SlidoAudienceSource::new(Rig::code(bad), false, false, factory.clone())
            .err()
            .expect(bad);
        assert_eq!(error.0, "That doesn’t look like a Slido code or link.");
    }
    let ok = SlidoAudienceSource::new(Rig::code("AbC-1"), false, false, factory).unwrap();
    assert_eq!(ok.id(), "slido:abc-1");
}

#[tokio::test]
async fn start_opens_a_hidden_profile_window() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    let config = rig.factory.config.lock().unwrap().clone().unwrap();
    assert_eq!(config.profile, "audience-slido");
    assert_eq!(config.title, "Sign in to Slido");
    assert_eq!((config.width, config.height), (1100, 800));
    assert_eq!(config.start_url, "https://app.sli.do/");
    assert!(!config.moderator);
    assert!(!rig.window.is_visible());
    let status = rig.statuses().remove(0);
    assert_eq!(status.state, S::Connecting);
    assert_eq!(status.step, Some(AudienceSourceStep::Finding));
    assert_eq!(status.label, "Slido #1234");
    assert_eq!(status.kind, AudienceStatusKind::Slido);
    assert_eq!(status.code.as_deref(), Some("1234"));
    assert_eq!(status.can_open, Some(false));

    let by_id = Rig::new(
        SlidoEvent::Event {
            event_id: "AbCdEf".into(),
        },
        false,
        false,
    )
    .await;
    let config = by_id.factory.config.lock().unwrap().clone().unwrap();
    assert_eq!(config.start_url, "https://app.sli.do/event/AbCdEf");
    let status = by_id.statuses().remove(0);
    assert_eq!(status.label, "Slido event");
    assert_eq!(status.event_id.as_deref(), Some("AbCdEf"));
    assert_eq!(status.code, None);
}

#[tokio::test]
async fn start_reports_a_window_that_cannot_open() {
    let factory = Arc::new(FakeFactory {
        fail: true,
        ..Default::default()
    });
    let source = SlidoAudienceSource::new(Rig::code("1234"), false, false, factory).unwrap();
    assert_eq!(source.start().await.unwrap_err().0, "no window");
}

#[tokio::test]
async fn public_read_enters_review_once() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    let body = page(vec![
        question("7", "  A visible question?  ", 12),
        question("8", "Another", 0),
    ]);
    rig.read(PUBLIC, body.clone());
    let updates = rig.question_updates();
    assert_eq!(updates.len(), 1);
    let questions = updates[0].questions.clone().unwrap();
    assert_eq!(questions.len(), 2);
    assert_eq!(questions[0].id, "slido:1234:7");
    assert_eq!(questions[0].source_id, "slido:1234");
    assert_eq!(questions[0].text, "A visible question?");
    assert_eq!(questions[0].author.as_deref(), Some("Anna"));
    assert_eq!(questions[0].votes, 12.0);
    assert_eq!(questions[0].state, AudienceQuestionState::Review);
    assert_eq!(updates[0].moderation, None);
    assert_eq!(rig.last_status().state, S::Live);
    assert_eq!(rig.last_status().updated_at, Some(1_000_000.0));
    // The same page again is a heartbeat, not an update.
    rig.advance(1000);
    rig.read(PUBLIC, body);
    assert_eq!(rig.question_updates().len(), 1);
    // A change keeps `created_at` of questions already seen.
    rig.advance(1000);
    rig.read(
        PUBLIC,
        page(vec![
            question("7", "A visible question?", 13),
            question("9", "New", 1),
        ]),
    );
    let updates = rig.question_updates();
    assert_eq!(updates.len(), 2);
    let latest = updates[1].questions.clone().unwrap();
    assert_eq!(latest[0].created_at, 1_000_000.0);
    assert_eq!(latest[1].created_at, 1_002_000.0);
    assert_eq!(latest[0].votes, 13.0);
}

#[tokio::test]
async fn skip_review_approves_and_trusts() {
    let rig = Rig::new(Rig::code("1234"), false, true).await;
    rig.read(PUBLIC, page(vec![question("7", "Q?", 1)]));
    let update = rig.question_updates().remove(0);
    assert_eq!(update.moderation, Some(AudienceMergeMode::Trusted));
    assert_eq!(
        update.questions.unwrap()[0].state,
        AudienceQuestionState::Approved
    );
    assert_eq!(rig.last_status().skip_review, Some(true));
}

#[tokio::test]
async fn bad_items_are_dropped() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    let long_id = "i".repeat(201);
    rig.read(
        PUBLIC,
        page(vec![
            json!({ "externalId": "ok", "text": "Fine", "author": null, "votes": 2.0 }),
            json!({ "externalId": "", "text": "no id", "author": null, "votes": 1 }),
            json!({ "externalId": long_id, "text": "long id", "author": null, "votes": 1 }),
            json!({ "externalId": 5, "text": "numeric id", "author": null, "votes": 1 }),
            json!({ "externalId": "a", "text": "no author key", "votes": 1 }),
            json!({ "externalId": "b", "text": "numeric author", "author": 3, "votes": 1 }),
            json!({ "externalId": "c", "text": "   ", "author": null, "votes": 1 }),
            json!({ "externalId": "d", "text": 4, "author": null, "votes": 1 }),
            json!({ "externalId": "e", "text": "negative", "author": null, "votes": -1 }),
            json!({ "externalId": "f", "text": "fraction", "author": null, "votes": 1.5 }),
            json!({ "externalId": "g", "text": "huge", "author": null, "votes": 9007199254740993.0f64 }),
            json!({ "externalId": "h", "text": "string votes", "author": null, "votes": "3" }),
            json!("not an object"),
            Value::Null,
        ]),
    );
    let questions = rig.question_updates().remove(0).questions.unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].id, "slido:1234:ok");
    assert_eq!(questions[0].author, None);
}

#[tokio::test]
async fn text_and_author_are_clamped() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    let author = format!("  {}  ", "a".repeat(100));
    rig.read(
        PUBLIC,
        page(vec![
            json!({ "externalId": "1", "text": "x".repeat(400), "author": author, "votes": 1 }),
            json!({ "externalId": "2", "text": "Q", "author": "   ", "votes": 1 }),
        ]),
    );
    let questions = rig.question_updates().remove(0).questions.unwrap();
    assert_eq!(questions[0].text.chars().count(), 280);
    assert_eq!(questions[0].author.as_ref().unwrap().len(), 80);
    assert_eq!(questions[1].author, None);
}

#[tokio::test]
async fn caps_at_5000_questions() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    let many = (0..5100)
        .map(|i| question(&i.to_string(), "Q", 1))
        .collect();
    rig.read(PUBLIC, page(many));
    assert_eq!(
        rig.question_updates().remove(0).questions.unwrap().len(),
        5000
    );
}

#[tokio::test]
async fn an_event_with_no_questions_is_connected() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    rig.read(
        PUBLIC,
        json!({ "questions": [], "empty": true, "notFound": false, "eventName": null }),
    );
    assert_eq!(rig.last_status().state, S::Live);
    assert!(rig.question_updates().is_empty());
}

#[tokio::test]
async fn no_questions_and_not_empty_is_not_a_good_read() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    rig.read(PUBLIC, page(vec![]));
    assert_eq!(rig.last_status().state, S::Connecting);
    assert!(rig.question_updates().is_empty());
    // Items that all fail validation count the same.
    rig.read(PUBLIC, page(vec![json!({ "externalId": "" })]));
    assert_eq!(rig.last_status().state, S::Connecting);
}

#[tokio::test]
async fn wrong_code_is_reported_on_the_join_page_only() {
    let rig = Rig::new(Rig::code("0000"), false, false).await;
    let body = json!({ "questions": [], "empty": false, "notFound": true, "eventName": null });
    rig.read("https://app.sli.do/event/abc/live", body.clone());
    assert_eq!(rig.last_status().state, S::Connecting);
    rig.read("https://admin.sli.do/", body.clone());
    assert_eq!(rig.last_status().state, S::Connecting);
    rig.read("https://app.sli.do/", body);
    let status = rig.last_status();
    assert_eq!(status.state, S::Error);
    assert_eq!(
        status.error.as_deref(),
        Some("No live Slido event with that code. Check the code, or start the event in Slido.")
    );
    // The watchdog leaves a wrong code alone.
    rig.advance(120_000);
    rig.source.tick();
    assert_eq!(
        rig.last_status()
            .error
            .as_deref()
            .map(|e| e.starts_with("No live")),
        Some(true)
    );
}

#[tokio::test]
async fn event_name_becomes_the_label() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    let name = format!("  {}  ", "N".repeat(100));
    rig.read(PUBLIC, json!({ "questions": [question("1", "Q", 1)], "empty": false, "notFound": false, "eventName": name }));
    let status = rig.last_status();
    assert_eq!(status.label, "N".repeat(80));
    assert_eq!(status.event_name.as_deref(), Some("N".repeat(80).as_str()));
    // A name read from the admin host is ignored.
    let admin = Rig::new(Rig::code("1234"), true, false).await;
    admin.read(ADMIN, json!({ "questions": [question("1", "Q", 1)], "empty": false, "notFound": false, "eventName": "Nope" }));
    assert_eq!(admin.last_status().event_name, None);
}

#[tokio::test]
async fn reads_from_the_wrong_place_are_ignored() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    let body = page(vec![question("1", "Q", 1)]);
    // Admin host while in public mode.
    rig.read(ADMIN, body.clone());
    // Not an event path.
    rig.read("https://app.sli.do/", body.clone());
    // A frame from another origin.
    rig.window.at(PUBLIC);
    rig.source.deliver(SlidoWindowEvent::Message {
        frame_url: "https://evil.example/".into(),
        body: body.to_string(),
    });
    rig.source.deliver(SlidoWindowEvent::Message {
        frame_url: "garbage".into(),
        body: body.to_string(),
    });
    // Not JSON, an array, or a non-array `questions`.
    rig.source.deliver(SlidoWindowEvent::Message {
        frame_url: PUBLIC.into(),
        body: "{".into(),
    });
    rig.read(PUBLIC, json!([1, 2]));
    rig.read(PUBLIC, json!({ "questions": "x", "empty": true }));
    assert!(rig.question_updates().is_empty());
    assert_eq!(rig.last_status().state, S::Connecting);
    // An unparsable current address.
    rig.window.at("not a url");
    rig.source.deliver(SlidoWindowEvent::Message {
        frame_url: PUBLIC.into(),
        body: body.to_string(),
    });
    assert!(rig.question_updates().is_empty());
}

#[tokio::test]
async fn moderator_signs_in_then_reads_and_hides() {
    let rig = Rig::new(Rig::code("1234"), true, false).await;
    assert_eq!(rig.last_status().can_open, Some(true));
    // The participant page loads while someone signs in: nothing to do yet.
    rig.window.at("https://app.sli.do/");
    rig.source.deliver(SlidoWindowEvent::Loaded);
    assert!(rig.window.calls().is_empty());
    // The join lands on the event page: show the window and go to the admin page.
    rig.window
        .at("https://app.sli.do/event/abc123/live/questions");
    rig.source.deliver(SlidoWindowEvent::Message {
        frame_url: PUBLIC.into(),
        body: json!({ "kairoNavigated": true }).to_string(),
    });
    assert_eq!(
        rig.window.calls(),
        ["show", "load https://admin.sli.do/event/abc123/polls"]
    );
    assert_eq!(rig.last_status().step, Some(AudienceSourceStep::SignIn));
    // Once only.
    rig.source.deliver(SlidoWindowEvent::Loaded);
    assert_eq!(rig.window.calls().len(), 2);
    // On the admin page, waiting to open the event.
    rig.window.at("https://accounts.google.com/signin");
    rig.source.deliver(SlidoWindowEvent::Loaded);
    assert_eq!(rig.last_status().step, Some(AudienceSourceStep::SignIn));
    rig.window.at(ADMIN);
    rig.source.deliver(SlidoWindowEvent::Loaded);
    assert_eq!(rig.last_status().step, Some(AudienceSourceStep::OpenEvent));
    // A read hides the sign-in window, once.
    rig.read(ADMIN, page(vec![question("q1", "Pending?", 3)]));
    assert_eq!(rig.last_status().state, S::Live);
    assert!(!rig.window.is_visible());
    assert_eq!(
        rig.window.calls().iter().filter(|c| *c == "hide").count(),
        1
    );
    rig.window.show();
    rig.read(ADMIN, page(vec![question("q1", "Pending?", 4)]));
    assert!(rig.window.is_visible(), "later reads do not hide it again");
    // The operator can bring it back.
    rig.source.open();
    assert_eq!(
        &rig.window.calls()[rig.window.calls().len() - 2..],
        ["show", "focus"]
    );
}

#[tokio::test]
async fn moderator_ignores_participant_questions_and_public_ignores_admin() {
    let rig = Rig::new(Rig::code("1234"), true, false).await;
    rig.read(PUBLIC, page(vec![question("1", "Q", 1)]));
    assert!(rig.question_updates().is_empty());
}

#[tokio::test]
async fn open_does_nothing_for_a_participant_source() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    rig.source.open();
    assert!(rig.window.calls().is_empty());
}

#[tokio::test]
async fn watchdog_reports_a_page_that_stopped_answering() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    rig.window.at(PUBLIC);
    rig.source.deliver(SlidoWindowEvent::Loaded);
    rig.advance(29_000);
    rig.source.tick();
    assert_eq!(rig.last_status().state, S::Connecting);
    rig.advance(1_000);
    rig.source.tick();
    assert_eq!(
        rig.last_status().error.as_deref(),
        Some("Couldn’t read this event. Check the code, then try again.")
    );
    // After a good read the message changes.
    rig.read(PUBLIC, page(vec![question("1", "Q", 1)]));
    assert_eq!(rig.last_status().state, S::Live);
    rig.advance(30_000);
    rig.source.tick();
    let status = rig.last_status();
    assert_eq!(status.state, S::Error);
    assert_eq!(
        status.error.as_deref(),
        Some("Questions stopped coming in from Slido. The ones you have are safe.")
    );
    // Reads come back: live again.
    rig.read(PUBLIC, page(vec![question("1", "Q", 2)]));
    assert_eq!(rig.last_status().state, S::Live);
}

#[tokio::test]
async fn watchdog_waits_for_a_moderator_to_sign_in() {
    let rig = Rig::new(Rig::code("1234"), true, false).await;
    rig.window.at("https://accounts.google.com/signin");
    rig.source.deliver(SlidoWindowEvent::Loaded);
    rig.advance(300_000);
    rig.source.tick();
    assert!(rig.statuses().iter().all(|s| s.state != S::Error));
    // On the admin event page with nothing read, it is a failure.
    rig.window.at(ADMIN);
    rig.source.tick();
    assert_eq!(rig.last_status().state, S::Error);
}

#[tokio::test]
async fn a_page_that_never_loads_is_unreachable() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    rig.advance(29_000);
    rig.source.tick();
    assert_eq!(rig.last_status().state, S::Connecting);
    rig.advance(1_000);
    rig.source.tick();
    let status = rig.last_status();
    assert_eq!(status.state, S::Error);
    assert_eq!(
        status.error.as_deref(),
        Some("Couldn’t reach Slido. Check your internet connection.")
    );
}

#[tokio::test]
async fn status_updates_are_deduplicated_and_time_refreshes_every_ten_seconds() {
    let rig = Rig::new(Rig::code("1234"), false, false).await;
    let body = page(vec![question("1", "Q", 1)]);
    rig.read(PUBLIC, body.clone());
    let count = rig.statuses().len();
    rig.advance(1000);
    rig.read(PUBLIC, body.clone());
    assert_eq!(rig.statuses().len(), count, "same ten-second bucket");
    rig.advance(10_000);
    rig.read(PUBLIC, body);
    assert_eq!(rig.statuses().len(), count + 1);
}

#[tokio::test]
async fn stop_destroys_the_window_and_silences_the_source() {
    let rig = Rig::new(Rig::code("1234"), true, false).await;
    rig.source.stop().await.unwrap();
    assert_eq!(rig.last_status().state, S::Stopped);
    assert!(rig.window.calls().contains(&"destroy".to_string()));
    let before = rig.updates.lock().unwrap().len();
    rig.read(ADMIN, page(vec![question("1", "Q", 1)]));
    rig.source.tick();
    rig.source.open();
    assert_eq!(rig.updates.lock().unwrap().len(), before);
    // A second stop does not repeat the stopped status.
    rig.source.dispose();
    assert_eq!(rig.updates.lock().unwrap().len(), before);
}

/// `selectors.js` must stay the Electron source, compiled and otherwise unchanged.
/// Skipped when the Electron checkout or its esbuild is not beside this repository.
#[test]
fn selectors_js_matches_the_electron_source() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let esbuild = root.join("../kairo-presenter/node_modules/.bin/esbuild");
    let source =
        root.join("../kairo-presenter/src/main/services/audience/sources/slido-selectors.ts");
    if !esbuild.exists() || !source.exists() {
        eprintln!("skipped: no Electron checkout with esbuild");
        return;
    }
    let out = std::env::temp_dir().join(format!("kairo-slido-selectors-{}.js", std::process::id()));
    let status = std::process::Command::new(esbuild)
        .arg(&source)
        .args([
            "--bundle",
            "--format=iife",
            "--global-name=qaSelectors",
            "--platform=browser",
            "--log-level=error",
        ])
        .arg(format!("--outfile={}", out.display()))
        .status()
        .unwrap();
    assert!(status.success());
    let fresh = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    // esbuild names the source file in a comment, relative to where it ran.
    let code = |text: &str| -> String {
        text.lines()
            .filter(|line| !line.trim_start().starts_with("// ../"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        code(&fresh),
        code(SELECTORS_JS),
        "run tools/slido-page/build.sh"
    );
}
