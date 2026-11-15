//! Shared live pages, main-thread owned. One page per normalized URL; all
//! consumers use its source texture, never a second browser or scroll mirror.

use kairo_compositor::{Raster, thread::SourceFeed};
use kairo_engine_proto::{WebInput, WebLayer};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use winit::event_loop::ActiveEventLoop;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac::Page;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::Page;

pub const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / 30);
const MAX_PAGES: usize = 8;
const MAX_SCROLL: f64 = 16_777_216.;

pub fn address(url: &str) -> Option<String> {
    kairo_domain::web::web_address(url, false)
}

pub fn source(url: &str) -> u64 {
    crate::source_id(&format!("web:{}", address(url).unwrap_or_default()))
}

pub fn scroll(x: f64, y: f64) -> Option<(f64, f64)> {
    (x.is_finite() && y.is_finite()).then(|| {
        // Math.round, including negative half values, then bounded before JS.
        (
            (x + 0.5).floor().clamp(0., MAX_SCROLL),
            (y + 0.5).floor().clamp(0., MAX_SCROLL),
        )
    })
}

pub fn input_valid(input: &WebInput) -> bool {
    match input {
        WebInput::Move { x, y } | WebInput::Click { x, y } => {
            x.is_finite() && y.is_finite() && (0. ..=1920.).contains(x) && (0. ..=1080.).contains(y)
        }
        WebInput::Text { text } => text.len() <= 4096 && !text.contains('\0'),
        WebInput::Key { key } => key_code(key).is_some(),
    }
}

/// macOS virtual key codes / Unicode function characters. Windows maps names
/// separately; arbitrary content never becomes an executable script.
fn key_code(key: &str) -> Option<(u16, char)> {
    Some(match key {
        "Enter" => (36, '\r'),
        "Tab" => (48, '\t'),
        "Backspace" => (51, '\u{8}'),
        "Delete" => (117, '\u{f728}'),
        "Escape" => (53, '\u{1b}'),
        "ArrowLeft" => (123, '\u{f702}'),
        "ArrowRight" => (124, '\u{f703}'),
        "ArrowDown" => (125, '\u{f701}'),
        "ArrowUp" => (126, '\u{f700}'),
        "Home" => (115, '\u{f729}'),
        "End" => (119, '\u{f72b}'),
        "PageUp" => (116, '\u{f72c}'),
        "PageDown" => (121, '\u{f72d}'),
        _ => return None,
    })
}

pub fn pointer() -> Arc<Raster> {
    static ARROW: OnceLock<Arc<Raster>> = OnceLock::new();
    ARROW.get_or_init(|| {
        use kairo_scene::media::{BoxFit, FitMode, decode_picture};
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="56" height="56" viewBox="0 0 32 32"><path d="M6 3 L26 16.5 L16.8 17.8 L21.6 27.4 L17.6 29.3 L12.8 19.6 L6 26.5 Z" fill="#111111" stroke="#ffffff" stroke-width="1.6" stroke-linejoin="round"/></svg>"##;
        let mut picture = decode_picture(svg, BoxFit { width: 56, height: 56, fit: FitMode::Fill }).expect("built-in pointer SVG");
        Raster::new(56, 56, picture.frames.remove(0).pixels)
    }).clone()
}

/// Coalesced, bounded signals; content cannot grow a queue while the engine is busy.
#[derive(Default)]
pub(super) struct Signals {
    pub frame: Option<Arc<Raster>>,
    pub timing: Option<(Instant, Instant)>,
    pub scroll: Option<(f64, f64)>,
    pub error: Option<String>,
    pub loaded: bool,
    #[cfg_attr(
        not(target_os = "macos"),
        allow(dead_code, reason = "only WKWebView reports a crashed page")
    )]
    pub crashed: bool,
}

/// Page IPC carries scroll observations only. It cannot invoke engine commands.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    allow(dead_code, reason = "the Linux web layer has no page IPC yet")
)]
fn observed_scroll(body: &str) -> Option<(f64, f64)> {
    if body.len() > 256 {
        return None;
    }
    let data: serde_json::Value = serde_json::from_str(body).ok()?;
    if data.get("type")?.as_str()? != "scroll" {
        return None;
    }
    scroll(data.get("x")?.as_f64()?, data.get("y")?.as_f64()?)
}

#[derive(Default)]
struct Registry {
    outputs: HashMap<String, WebLayer>,
    last_scroll: Option<(String, f64, f64)>,
}

impl Registry {
    fn urls(&self) -> HashSet<String> {
        self.outputs.values().map(|w| w.url.clone()).collect()
    }
    fn remember(&mut self, url: &str, x: f64, y: f64) {
        if let Some((x, y)) = scroll(x, y) {
            self.last_scroll = Some((url.into(), x, y));
        }
    }
    fn restore(&self, url: &str) -> Option<(f64, f64)> {
        self.last_scroll
            .as_ref()
            .filter(|(u, _, _)| u == url)
            .map(|(_, x, y)| (*x, *y))
    }
}

pub enum Notice {
    Navigated { output: String, url: String },
    Error(String),
}

#[derive(Default)]
pub struct Webs {
    registry: Registry,
    pages: HashMap<u64, Page>,
    owners: HashMap<String, u64>,
    next_page: u64,
    limit: usize,
    limited: HashSet<String>,
    failed: HashSet<String>,
    navigated: HashMap<u64, String>,
    retired: HashMap<u64, Instant>,
}

impl Webs {
    /// Runs on winit's main thread. Only ready, eligible surfaces are supplied.
    pub fn sync(
        &mut self,
        el: &ActiveEventLoop,
        wanted: Vec<(String, WebLayer)>,
        _feed: &SourceFeed,
    ) -> Vec<Notice> {
        let mut notices = Vec::new();
        let before = self.registry.outputs.clone();
        self.registry.outputs = wanted
            .into_iter()
            .filter_map(|(o, mut w)| {
                w.url = address(&w.url)?;
                w.reload.is_finite().then_some((o, w))
            })
            .collect();
        let urls = self.registry.urls();
        let old_urls: HashSet<_> = before.values().map(|w| w.url.clone()).collect();
        self.owners
            .retain(|url, page| urls.contains(url) && self.pages.contains_key(page));
        self.failed.retain(|u| urls.contains(u));
        self.limited.retain(|u| urls.contains(u));
        // Resolve existing and navigated pages before freeing owners. A page's
        // identity is independent of its original URL, so returning to A after
        // adopting A→B can create A while another output continues showing B.
        for url in &urls {
            if !self.owners.contains_key(url)
                && let Some((id, _)) = self
                    .pages
                    .iter()
                    .find(|(_, p)| p.location().as_deref() == Some(url.as_str()))
            {
                self.owners.insert(url.clone(), *id);
            }
        }
        let live: HashSet<_> = self.owners.values().copied().collect();
        self.pages.retain(|id, _| live.contains(id));
        self.navigated.retain(|id, _| live.contains(id));
        let limit = if self.limit == 0 {
            MAX_PAGES
        } else {
            self.limit
        };
        let mut reloaded = HashSet::new();
        for url in &urls {
            self.retired.remove(&source(url));
            let changed = self.registry.outputs.iter().any(|(o, w)| {
                &w.url == url
                    && before
                        .get(o)
                        .is_some_and(|b| b.url == *url && b.reload != w.reload)
            });
            if changed {
                self.failed.remove(url);
            }
            if !self.owners.contains_key(url) && !self.failed.contains(url) {
                if self.pages.len() >= limit {
                    if self.limited.insert(url.clone()) {
                        notices.push(Notice::Error(format!(
                            "at most {limit} live web pages are supported"
                        )));
                    }
                    continue;
                }
                match Page::new(el, url, self.registry.restore(url)) {
                    Ok(page) => {
                        self.next_page += 1;
                        self.pages.insert(self.next_page, page);
                        self.owners.insert(url.clone(), self.next_page);
                        self.limited.remove(url);
                    }
                    Err(error) => {
                        self.failed.insert(url.clone());
                        notices.push(Notice::Error(format!("web {url}: {error}")));
                    }
                }
            }
            if let Some(id) = self.owners.get(url)
                && changed
                && reloaded.insert(*id)
                && let Some(page) = self.pages.get_mut(id)
            {
                page.reload();
            }
        }
        for url in old_urls.difference(&urls) {
            self.retired
                .insert(source(url), Instant::now() + Duration::from_millis(5100));
        }
        notices
    }

    /// Optional lower resource budget; never above the engine's safety limit.
    pub fn with_page_limit(limit: usize) -> Self {
        Self {
            limit: limit.clamp(1, MAX_PAGES),
            ..Self::default()
        }
    }
    pub fn location(&self, output: &str) -> Option<String> {
        let url = &self.registry.outputs.get(output)?.url;
        self.pages.get(self.owners.get(url)?)?.location()
    }

    pub fn live_pages(&self) -> usize {
        self.pages.len()
    }

    pub fn active(&self) -> bool {
        !self.pages.is_empty() || !self.retired.is_empty()
    }

    pub fn scroll(&mut self, output: &str, x: f64, y: f64) {
        let Some(url) = self.registry.outputs.get(output).map(|w| w.url.clone()) else {
            return;
        };
        let Some((x, y)) = scroll(x, y) else {
            return;
        };
        self.registry.remember(&url, x, y);
        if let Some(page) = self.owners.get(&url).and_then(|id| self.pages.get(id)) {
            page.scroll(x, y);
        }
    }

    pub fn input(&self, output: &str, input: &WebInput) {
        if !input_valid(input) {
            return;
        }
        if let Some(w) = self.registry.outputs.get(output)
            && let Some(p) = self.owners.get(&w.url).and_then(|id| self.pages.get(id))
        {
            p.input(input);
        }
    }

    pub fn tick(&mut self, now: Instant, feed: &SourceFeed) -> Vec<Notice> {
        let mut notices = Vec::new();
        self.retired.retain(|id, until| {
            if now >= *until {
                feed.remove(*id);
                false
            } else {
                true
            }
        });
        for (id, page) in &mut self.pages {
            let ids: HashSet<_> = self
                .registry
                .outputs
                .values()
                .filter(|w| self.owners.get(&w.url) == Some(id))
                .map(|w| source(&w.url))
                .collect();
            page.capture(now, feed, ids.into_iter().collect());
            let (signals, location) = page.drain();
            let url = self
                .registry
                .outputs
                .values()
                .find(|w| self.owners.get(&w.url) == Some(id))
                .map(|w| w.url.clone())
                .unwrap_or_default();
            if let Some(frame) = signals.frame {
                let (started, ready) = signals.timing.unwrap_or((now, now));
                for w in self
                    .registry
                    .outputs
                    .values()
                    .filter(|w| self.owners.get(&w.url) == Some(id))
                {
                    feed.rgba_frame(source(&w.url), frame.clone(), started, ready);
                }
            }
            if !signals.loaded
                && let Some((x, y)) = signals.scroll
            {
                self.registry
                    .remember(location.as_deref().unwrap_or(&url), x, y);
            }
            if let Some(error) = signals.error {
                notices.push(Notice::Error(format!("web {url}: {error}")));
            }
            if signals.loaded
                && let Some((x, y)) = self.registry.restore(location.as_deref().unwrap_or(&url))
            {
                page.scroll(x, y);
            }
            if let Some(location) = location
                && self.navigated.get(id) != Some(&location)
            {
                self.navigated.insert(*id, location.clone());
                for (output, w) in &self.registry.outputs {
                    if self.owners.get(&w.url) == Some(id) {
                        notices.push(Notice::Navigated {
                            output: output.clone(),
                            url: location.clone(),
                        });
                    }
                }
            }
        }
        notices
    }
}

// Explicit unsupported-platform error, never a white page presented as success.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
enum Page {}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Page {
    fn new(_: &ActiveEventLoop, _: &str, _: Option<(f64, f64)>) -> Result<Self, String> {
        Err("web capture is not available on this platform".into())
    }
    fn reload(&mut self) {
        match *self {}
    }
    fn scroll(&self, _: f64, _: f64) {
        match *self {}
    }
    fn input(&self, _: &WebInput) {
        match *self {}
    }
    fn capture(&mut self, _: Instant, _: &SourceFeed, _: Vec<u64>) {
        match *self {}
    }
    fn drain(&mut self) -> (Signals, Option<String>) {
        match *self {}
    }
    fn location(&self) -> Option<String> {
        match *self {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_urls_and_scroll_are_bounded() {
        let mut r = Registry::default();
        for o in ["a", "b"] {
            r.outputs.insert(
                o.into(),
                WebLayer {
                    url: "https://example.com/".into(),
                    reload: 0.,
                },
            );
        }
        assert_eq!(r.urls().len(), 1);
        r.outputs.remove("a");
        assert_eq!(r.urls().len(), 1);
        r.remember("https://example.com/", 1.6, 5.5);
        assert_eq!(r.restore("https://example.com/"), Some((2., 6.)));
        assert_eq!(r.restore("https://other.example/"), None);
        r.remember("https://example.com/", f64::NAN, 0.);
        assert_eq!(r.restore("https://example.com/"), Some((2., 6.)));
        r.outputs.remove("b");
        assert!(r.urls().is_empty());
    }
    #[test]
    fn content_cannot_send_commands_or_unbounded_values() {
        assert_eq!(
            observed_scroll(r#"{"type":"scroll","x":2.4,"y":-5}"#),
            Some((2., 0.))
        );
        for s in [
            r#"{"type":"Shutdown","x":0,"y":0}"#,
            r#"{"type":"scroll","x":"3","y":0}"#,
            r#"{"type":"scroll","x":1e999,"y":0}"#,
            "null",
            "[]",
            "{}",
        ] {
            assert_eq!(observed_scroll(s), None);
        }
        assert_eq!(observed_scroll(&"x".repeat(257)), None);
        assert!(!input_valid(&WebInput::Click {
            x: f64::INFINITY,
            y: 0.
        }));
        assert!(!input_valid(&WebInput::Text {
            text: "x".repeat(4097)
        }));
        assert!(!input_valid(&WebInput::Key {
            key: "window.close()".into()
        }));
        assert_eq!(address("javascript:alert(1)"), None);
        assert_eq!(source("example.com"), source("https://example.com/"));
    }
}

/// Response-stage CDP interception adds a real browser-enforced CSP sandbox.
/// The stable Fetch.fulfillRequest operation preserves the original body when
/// body is omitted at the response stage; bytes are never proxied through Rust.
#[cfg(any(target_os = "windows", test))]
fn sandbox_response(body: &str) -> Result<(&'static str, String), &'static str> {
    use serde_json::{Value, json};
    if body.len() > 1_048_576 {
        return Err("oversized browser response metadata");
    }
    let event: Value =
        serde_json::from_str(body).map_err(|_| "invalid browser response metadata")?;
    let id = event
        .get("requestId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or("invalid intercepted request id")?;
    let Some(status) = event
        .get("responseStatusCode")
        .and_then(Value::as_u64)
        .filter(|s| (100..=599).contains(s))
    else {
        return Ok((
            "Fetch.failRequest",
            json!({"requestId":id,"errorReason":"BlockedByClient"}).to_string(),
        ));
    };
    let mut headers = event
        .get("responseHeaders")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if headers.len() > 512
        || headers.iter().any(|h| {
            h.get("name").and_then(Value::as_str).is_none()
                || h.get("value").and_then(Value::as_str).is_none()
        })
    {
        return Err("invalid intercepted response headers");
    }
    headers.push(json!({"name":"Content-Security-Policy","value":"sandbox allow-scripts allow-same-origin allow-forms allow-presentation"}));
    headers.push(json!({"name":"Referrer-Policy","value":"strict-origin-when-cross-origin"}));
    Ok((
        "Fetch.fulfillRequest",
        json!({"requestId":id,"responseCode":status,"responseHeaders":headers}).to_string(),
    ))
}

#[cfg(test)]
mod sandbox_tests {
    #[test]
    fn adds_native_sandbox_without_removing_headers_or_replacing_body() {
        let (method,params)=super::sandbox_response(r#"{"requestId":"r1","responseStatusCode":200,"responseHeaders":[{"name":"X-Frame-Options","value":"DENY"},{"name":"Content-Security-Policy","value":"default-src 'self'"}]}"#).unwrap();
        assert_eq!(method, "Fetch.fulfillRequest");
        let p: serde_json::Value = serde_json::from_str(&params).unwrap();
        assert!(p.get("body").is_none());
        assert_eq!(p["responseHeaders"].as_array().unwrap().len(), 4);
        assert_eq!(p["responseHeaders"][0]["value"], "DENY");
        assert_eq!(
            p["responseHeaders"][2]["value"],
            "sandbox allow-scripts allow-same-origin allow-forms allow-presentation"
        );
        assert_eq!(
            p["responseHeaders"][3]["value"],
            "strict-origin-when-cross-origin"
        );
        assert_eq!(
            super::sandbox_response(r#"{"requestId":"r2","responseErrorReason":"Failed"}"#)
                .unwrap()
                .0,
            "Fetch.failRequest"
        );
        assert!(super::sandbox_response("{}").is_err());
        assert!(super::sandbox_response(&"x".repeat(1_048_577)).is_err());
    }
}
