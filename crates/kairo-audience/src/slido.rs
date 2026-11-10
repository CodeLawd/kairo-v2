//! `sources/slido.ts`, `slido-selectors.ts`, `slido-health.ts` and
//! `preload/audience-slido.ts`: reads a Slido event's questions from a web
//! view window the operator can sign in to.
//!
//! This module holds everything that does not depend on a window toolkit.
//! A [`SlidoWindowFactory`] (the `ui-web` crate, on `gpui-webview`) opens the
//! window; this module decides what to load, what to allow, and what each
//! page read means. The page is third-party and its init script runs in the
//! page's own world, so every message is checked here as untrusted input.

use crate::source::{AudienceSource, AudienceUpdate, BoxFuture, Sinks, UpdateSink};
use crate::{AudienceError, Result, lock, now_ms, slice_utf16};
use kairo_domain::audience::{
    AudienceMergeMode, AudienceQuestion, AudienceQuestionState, AudienceSourceState,
    AudienceSourceStatus, AudienceSourceStep, AudienceStatusKind, SlidoEvent,
    normalize_audience_text,
};
use kairo_domain::js;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use url::Url;

/// The persisted profile name (`persist:audience-slido` in Electron).
pub const SLIDO_PROFILE: &str = "audience-slido";
/// `new BrowserWindow({ width: 1100, height: 800, title: 'Sign in to Slido' })`.
pub const SLIDO_WINDOW_TITLE: &str = "Sign in to Slido";
pub const SLIDO_WINDOW_SIZE: (u32, u32) = (1100, 800);
/// `slidoReadStale`: no good read for this long is a failure.
pub const STALE_AFTER_MS: f64 = 30_000.0;
/// Wry reports no failed loads, so a page that never finishes loading is
/// treated as unreachable after this long (Electron used `did-fail-load`).
pub const LOAD_TIMEOUT_MS: f64 = 30_000.0;

const SELECTORS_JS: &str = include_str!("slido/selectors.js");
const BRIDGE_JS: &str = include_str!("slido/bridge.js");

const MSG_NOT_FOUND: &str =
    "No live Slido event with that code. Check the code, or start the event in Slido.";
const MSG_MODERATOR_PAGE: &str =
    "Couldn’t open the moderator page. Open the Slido window and try again.";
const MSG_UNREADABLE: &str = "Couldn’t read this event. Check the code, then try again.";
const MSG_STOPPED_COMING: &str =
    "Questions stopped coming in from Slido. The ones you have are safe.";
const MSG_UNREACHABLE: &str = "Couldn’t reach Slido. Check your internet connection.";
const MSG_BAD_CODE: &str = "That doesn’t look like a Slido code or link.";

const ALLOWED_HOSTS: [&str; 5] = [
    "sli.do",
    "slido.com",
    "accounts.google.com",
    "cisco.com",
    "webex.com",
];

/// `allowedSlidoNavigation`.
pub fn allowed_slido_navigation(address: &str) -> bool {
    let Ok(url) = Url::parse(address) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    url.scheme() == "https"
        && ALLOWED_HOSTS.iter().any(|allowed| {
            host == *allowed
                || host
                    .strip_suffix(allowed)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
}

/// Wry's navigation handler is asked about frames as well as the page, where
/// Electron's `will-navigate` was asked about the page only. Blank frames
/// stay allowed; every other address needs the allowlist.
pub fn allowed_slido_frame_navigation(address: &str) -> bool {
    matches!(address, "about:blank" | "about:srcdoc") || allowed_slido_navigation(address)
}

/// `setWindowOpenHandler`: only a moderator may open a window (Google or
/// Cisco sign-in), and only to an allowed address.
pub fn allowed_slido_window(address: &str, moderator: bool) -> bool {
    moderator && allowed_slido_navigation(address)
}

/// `slidoReadStale`.
pub fn slido_read_stale(last_good_at: Option<f64>, now: f64) -> bool {
    last_good_at.is_some_and(|at| now - at >= STALE_AFTER_MS)
}

/// The single script the window injects at document start, in every page.
/// `selectors.js` is `slido-selectors.ts` compiled unchanged (see
/// `tools/slido-page/build.sh`); `bridge.js` is the preload with `ipcRenderer`
/// replaced by `window.ipc`.
pub fn page_script(code: &str, moderator: bool) -> String {
    let config = serde_json::json!({ "code": code, "moderator": moderator });
    format!("(function () {{\nconst kairoSlido = {config};\n{SELECTORS_JS}\n{BRIDGE_JS}\n}})();\n")
}

/// What the window must be built with.
#[derive(Clone, Debug)]
pub struct SlidoWindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    /// Persisted cookie and storage profile.
    pub profile: &'static str,
    pub moderator: bool,
    pub script: String,
    pub start_url: String,
}

/// Things a window reports. All are untrusted except `Loaded`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlidoWindowEvent {
    /// A page script message and the address of the frame that sent it.
    Message { frame_url: String, body: String },
    /// `did-finish-load`.
    Loaded,
}

pub type SlidoEventSink = Arc<dyn Fn(SlidoWindowEvent) + Send + Sync>;

/// One Slido window. It denies permissions, filters navigation with
/// [`allowed_slido_navigation`] and [`allowed_slido_window`], hides instead of
/// closing, and is never handed to a screen surface.
pub trait SlidoWindow: Send + Sync {
    fn load_url(&self, url: &str);
    fn current_url(&self) -> String;
    fn show(&self);
    fn hide(&self);
    fn focus(&self);
    fn is_visible(&self) -> bool;
    /// Closes the window for good, with its child windows.
    fn destroy(&self);
}

pub trait SlidoWindowFactory: Send + Sync {
    /// Async because the window is built on the UI thread, which may be the
    /// caller's own thread.
    fn open(
        &self,
        config: SlidoWindowConfig,
        events: SlidoEventSink,
    ) -> BoxFuture<'_, Result<Arc<dyn SlidoWindow>>>;
}

use kairo_domain::time::Clock;

#[derive(Default)]
struct State {
    window: Option<Arc<dyn SlidoWindow>>,
    last_questions: HashMap<String, f64>,
    last_good_at: Option<f64>,
    /// Last read that found questions, shown to the operator as "updated …".
    last_read_at: Option<f64>,
    event_name: Option<String>,
    not_found: bool,
    hidden_once: bool,
    opened_moderator: bool,
    last_status: String,
    last_payload: String,
    stopped: bool,
    started_at: Option<f64>,
}

struct Inner {
    id: String,
    code: Option<String>,
    event_id: Option<String>,
    moderator: bool,
    skip_review: bool,
    factory: Arc<dyn SlidoWindowFactory>,
    clock: Clock,
    sinks: Sinks,
    state: Mutex<State>,
    watchdog: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

pub struct SlidoAudienceSource {
    inner: Arc<Inner>,
}

impl SlidoAudienceSource {
    pub fn new(
        event: SlidoEvent,
        moderator: bool,
        skip_review: bool,
        factory: Arc<dyn SlidoWindowFactory>,
    ) -> Result<Self> {
        Self::with_clock(event, moderator, skip_review, factory, Arc::new(now_ms))
    }

    pub(crate) fn with_clock(
        event: SlidoEvent,
        moderator: bool,
        skip_review: bool,
        factory: Arc<dyn SlidoWindowFactory>,
        clock: Clock,
    ) -> Result<Self> {
        let (code, event_id) = match event {
            SlidoEvent::Code { code } => (Some(code), None),
            SlidoEvent::Event { event_id } => (None, Some(event_id)),
        };
        let key = code.as_deref().or(event_id.as_deref()).unwrap_or("");
        let valid = !key.is_empty()
            && key.len() <= 64
            && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !valid {
            return Err(AudienceError::new(MSG_BAD_CODE));
        }
        let id = format!("slido:{}", key.to_ascii_lowercase());
        Ok(Self {
            inner: Arc::new(Inner {
                id,
                code,
                event_id,
                moderator,
                skip_review,
                factory,
                clock,
                sinks: Sinks::default(),
                state: Mutex::default(),
                watchdog: Mutex::default(),
            }),
        })
    }

    /// One watchdog pass. The started source runs it every second.
    #[cfg(test)]
    pub(crate) fn tick(&self) {
        self.inner.tick();
    }

    #[cfg(test)]
    pub(crate) fn deliver(&self, event: SlidoWindowEvent) {
        self.inner.handle(event);
    }
}

impl Inner {
    fn emit(&self, update: AudienceUpdate) {
        self.sinks.emit(update);
    }

    fn status(
        &self,
        state: AudienceSourceState,
        error: Option<&str>,
        step: Option<AudienceSourceStep>,
    ) {
        let (status, changed) = {
            let mut s = lock(&self.state);
            // Reads arrive every second; refresh "updated …" at most every 10 seconds.
            let key = format!(
                "{}:{}:{}:{}:{}",
                state.as_str(),
                error.unwrap_or(""),
                step.map_or("", |s| s.as_str()),
                s.event_name.as_deref().unwrap_or(""),
                s.last_read_at
                    .map_or(String::new(), |at| (at / 10_000.0).floor().to_string()),
            );
            if s.last_status == key || (s.stopped && state != AudienceSourceState::Stopped) {
                (None, false)
            } else {
                s.last_status = key;
                let label = s.event_name.clone().unwrap_or_else(|| match &self.code {
                    Some(code) => format!("Slido #{code}"),
                    None => "Slido event".into(),
                });
                (
                    Some(AudienceSourceStatus {
                        id: self.id.clone(),
                        kind: AudienceStatusKind::Slido,
                        label,
                        state,
                        step,
                        error: error.map(str::to_owned),
                        joined: None,
                        can_open: Some(self.moderator),
                        code: self.code.clone(),
                        event_id: self.event_id.clone(),
                        event_name: s.event_name.clone(),
                        moderator: Some(self.moderator),
                        skip_review: Some(self.skip_review),
                        updated_at: s.last_read_at,
                    }),
                    true,
                )
            }
        };
        if changed {
            self.emit(AudienceUpdate {
                status,
                ..Default::default()
            });
        }
    }

    fn window(&self) -> Option<Arc<dyn SlidoWindow>> {
        lock(&self.state).window.clone()
    }

    fn handle(self: &Arc<Self>, event: SlidoWindowEvent) {
        match event {
            SlidoWindowEvent::Loaded => {
                lock(&self.state).last_good_at = Some((self.clock)());
                self.navigated();
            }
            SlidoWindowEvent::Message { frame_url, body } => self.message(&frame_url, &body),
        }
    }

    /// `navigated`: after sign-in on the participant site, move a moderator to
    /// the admin page for the same event.
    fn navigated(self: &Arc<Self>) {
        let Some(window) = self.window() else { return };
        let Ok(address) = Url::parse(&window.current_url()) else {
            return;
        };
        let event = event_id_of(&address);
        let host = address.host_str().unwrap_or("");
        let open_admin = {
            let mut s = lock(&self.state);
            if self.moderator && !s.opened_moderator && host == "app.sli.do" && event.is_some() {
                s.opened_moderator = true;
                true
            } else {
                false
            }
        };
        if open_admin {
            window.show();
            self.status(
                AudienceSourceState::Connecting,
                None,
                Some(AudienceSourceStep::SignIn),
            );
            // `loadURL` is not awaited: a load that never completes is the
            // watchdog's job. A refused address is the only failure seen here.
            let target = format!(
                "https://admin.sli.do/event/{}/polls",
                event.unwrap_or_default()
            );
            if !allowed_slido_navigation(&target) {
                self.status(AudienceSourceState::Error, Some(MSG_MODERATOR_PAGE), None);
            } else {
                window.load_url(&target);
            }
        } else if self.moderator {
            let waiting = {
                let s = lock(&self.state);
                s.opened_moderator && s.last_read_at.is_none()
            };
            if waiting {
                let step = if host == "admin.sli.do" {
                    AudienceSourceStep::OpenEvent
                } else {
                    AudienceSourceStep::SignIn
                };
                self.status(AudienceSourceState::Connecting, None, Some(step));
            }
        }
    }

    /// `read`: one message from the page, which may say anything.
    fn message(self: &Arc<Self>, frame_url: &str, body: &str) {
        let Some(window) = self.window() else { return };
        if lock(&self.state).stopped {
            return;
        }
        let Ok(address) = Url::parse(&window.current_url()) else {
            return;
        };
        // Only the window's own page may speak: ignore frames from other origins.
        match Url::parse(frame_url) {
            Ok(frame) if frame.origin() == address.origin() => {}
            _ => return,
        }
        let Ok(raw) = serde_json::from_str::<Value>(body) else {
            return;
        };
        let Some(page) = raw.as_object() else { return };
        if page.get("kairoNavigated").and_then(Value::as_bool) == Some(true) {
            self.navigated();
            return;
        }
        let host = address.host_str().unwrap_or("");
        if page.get("notFound").and_then(Value::as_bool) == Some(true)
            && host == "app.sli.do"
            && !address.path().starts_with("/event/")
        {
            lock(&self.state).not_found = true;
            self.status(AudienceSourceState::Error, Some(MSG_NOT_FOUND), None);
            return;
        }
        if let Some(name) = page.get("eventName").and_then(Value::as_str) {
            let name = js::trim(name);
            if !name.is_empty() && host == "app.sli.do" {
                lock(&self.state).event_name = Some(slice_utf16(name, 80));
            }
        }
        let wanted = if self.moderator {
            "admin.sli.do"
        } else {
            "app.sli.do"
        };
        let Some(items) = page.get("questions").and_then(Value::as_array) else {
            return;
        };
        if host != wanted || !address.path().starts_with("/event/") {
            return;
        }
        let now = (self.clock)();
        let state = if self.skip_review {
            AudienceQuestionState::Approved
        } else {
            AudienceQuestionState::Review
        };
        let questions: Vec<AudienceQuestion> = {
            let mut s = lock(&self.state);
            items
                .iter()
                .take(5000)
                .filter_map(|item| {
                    let item = item.as_object()?;
                    let external_id = item.get("externalId")?.as_str()?;
                    if external_id.is_empty() || external_id.encode_utf16().count() > 200 {
                        return None;
                    }
                    // `author` must be a string or null; a missing key is rejected.
                    let author = match item.get("author")? {
                        Value::Null => None,
                        Value::String(author) => Some(author.as_str()),
                        _ => return None,
                    };
                    let text = normalize_audience_text(item.get("text")?)?;
                    let votes = safe_votes(item.get("votes")?)?;
                    let id = format!("{}:{external_id}", self.id);
                    let created_at = *s.last_questions.entry(id.clone()).or_insert(now);
                    Some(AudienceQuestion {
                        id,
                        source_id: self.id.clone(),
                        text,
                        author: author
                            .map(|a| slice_utf16(js::trim(a), 80))
                            .filter(|a| !a.is_empty()),
                        votes,
                        state,
                        original_text: None,
                        edited_by: None,
                        sent_by: None,
                        sent_at: None,
                        created_at,
                    })
                })
                .collect()
        };
        // An event with no questions yet still counts as connected.
        if questions.is_empty() && page.get("empty").and_then(Value::as_bool) != Some(true) {
            return;
        }
        {
            let mut s = lock(&self.state);
            s.last_good_at = Some(now);
            s.last_read_at = Some(now);
        }
        self.status(AudienceSourceState::Live, None, None);
        // Signed in and reading: the sign-in window has done its job.
        let hide = {
            let mut s = lock(&self.state);
            if self.moderator && !s.hidden_once && window.is_visible() {
                s.hidden_once = true;
                true
            } else {
                false
            }
        };
        if hide {
            window.hide();
        }
        let serialized = serde_json::to_string(&questions).unwrap_or_default();
        let changed = {
            let mut s = lock(&self.state);
            let changed = !questions.is_empty() && s.last_payload != serialized;
            if changed {
                s.last_payload = serialized;
            }
            changed
        };
        if changed {
            self.emit(AudienceUpdate {
                questions: Some(questions),
                moderation: self.skip_review.then_some(AudienceMergeMode::Trusted),
                ..Default::default()
            });
        }
    }

    fn tick(&self) {
        let now = (self.clock)();
        let (not_found, stale, never_loaded, never_read) = {
            let s = lock(&self.state);
            (
                s.not_found,
                slido_read_stale(s.last_good_at, now),
                s.last_good_at.is_none()
                    && s.started_at.is_some_and(|at| now - at >= LOAD_TIMEOUT_MS),
                s.last_read_at.is_none(),
            )
        };
        if not_found {
            return;
        }
        if never_loaded {
            self.status(AudienceSourceState::Error, Some(MSG_UNREACHABLE), None);
            return;
        }
        if !stale {
            return;
        }
        let url = self.window().map(|w| w.current_url()).unwrap_or_default();
        // Waiting for someone to sign in is not a failure.
        if self.moderator && !url.contains("admin.sli.do/event/") {
            return;
        }
        self.status(
            AudienceSourceState::Error,
            Some(if never_read {
                MSG_UNREADABLE
            } else {
                MSG_STOPPED_COMING
            }),
            None,
        );
    }

    fn dispose(&self) {
        let window = {
            let mut s = lock(&self.state);
            s.stopped = true;
            s.window.take()
        };
        if let Some(task) = lock(&self.watchdog).take() {
            task.abort();
        }
        if let Some(window) = window {
            window.destroy();
        }
        self.status(AudienceSourceState::Stopped, None, None);
    }
}

fn event_id_of(address: &Url) -> Option<String> {
    let rest = address.path().strip_prefix("/event/")?;
    let (id, _) = rest.split_once('/')?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| id.to_owned())
}

/// `Number.isSafeInteger(votes) && votes >= 0`.
fn safe_votes(value: &Value) -> Option<f64> {
    let votes = value.as_f64()?;
    (votes >= 0.0 && votes.fract() == 0.0 && votes <= 9_007_199_254_740_991.0).then_some(votes)
}

impl AudienceSource for SlidoAudienceSource {
    fn id(&self) -> &str {
        &self.inner.id
    }

    fn subscribe(&self, sink: UpdateSink) {
        self.inner.sinks.add(sink);
    }

    fn start(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let inner = self.inner.clone();
            inner.status(
                AudienceSourceState::Connecting,
                None,
                Some(AudienceSourceStep::Finding),
            );
            let weak: Weak<Inner> = Arc::downgrade(&inner);
            let events: SlidoEventSink = Arc::new(move |event| {
                if let Some(inner) = weak.upgrade() {
                    inner.handle(event);
                }
            });
            let start_url = match (&inner.event_id, &inner.code) {
                (Some(id), _) => format!("https://app.sli.do/event/{id}"),
                _ => "https://app.sli.do/".into(),
            };
            let config = SlidoWindowConfig {
                title: SLIDO_WINDOW_TITLE.into(),
                width: SLIDO_WINDOW_SIZE.0,
                height: SLIDO_WINDOW_SIZE.1,
                profile: SLIDO_PROFILE,
                moderator: inner.moderator,
                script: page_script(inner.code.as_deref().unwrap_or(""), inner.moderator),
                start_url,
            };
            lock(&inner.state).started_at = Some((inner.clock)());
            let window = inner.factory.open(config, events).await?;
            {
                let mut state = lock(&inner.state);
                // Stopped while the window was being built: nothing else
                // holds it now, so close it here rather than leak it.
                if state.stopped {
                    drop(state);
                    window.destroy();
                    return Ok(());
                }
                state.window = Some(window);
            }
            let weak = Arc::downgrade(&inner);
            let task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    let Some(inner) = weak.upgrade() else { break };
                    inner.tick();
                }
            });
            *lock(&inner.watchdog) = Some(task);
            Ok(())
        })
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.inner.dispose();
            Ok(())
        })
    }

    fn open(&self) {
        if !self.inner.moderator {
            return;
        }
        if let Some(window) = self.inner.window() {
            window.show();
            window.focus();
        }
    }

    fn dispose(&self) {
        self.inner.dispose();
    }
}

#[cfg(test)]
mod tests;
