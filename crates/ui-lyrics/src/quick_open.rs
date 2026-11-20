//! The ⌘F song finder (`SongQuickOpen.tsx`): the library ranked by title,
//! artist and lyric as you type, a web fallback on request, a preview that
//! flies out beside the list once a row is chosen, and ⌘↵ to queue a song on
//! the active setlist. It opens from any page as the shell's
//! `Overlay::SongSearch`.

use kairo_ui_kit::ControlSize;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    Disableable, Icon, Sizable, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list,
};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, Role, ScrollStrategy, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder, px,
    size,
};
use kairo_domain::lyrics::online_error::format_online_lyrics_error;
use kairo_domain::lyrics::{
    LyricsOnlinePreview, LyricsOnlineResult, LyricsSong, LyricsSongSection,
};
use kairo_ui_kit::motion;
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::{LyricsAnswer, LyricsBackend, LyricsCall, OnlineSource};
use crate::model::{Lyrics, ask};
use crate::rows::{self, MIN_ONLINE_QUERY, QuickHit, QuickIndex};
use crate::view::review_notice_element;

/// Element ids, for tests.
pub mod ids {
    pub const DIALOG: &str = "lyrics-quick-open";
    pub const QUERY: &str = "lyrics-quick-open-query";
    pub const CLEAR: &str = "lyrics-quick-open-clear";
    pub const COUNT: &str = "lyrics-quick-open-count";
    pub const LIST: &str = "lyrics-quick-open-list";
    pub const EMPTY: &str = "lyrics-quick-open-empty";
    pub const SEARCH_WEB: &str = "lyrics-quick-open-search-web";
    pub const PREVIEW: &str = "lyrics-quick-open-preview";
    pub const ADD: &str = "lyrics-quick-open-add";
    pub const OPEN: &str = "lyrics-quick-open-open";
    pub const BACKDROP: &str = "lyrics-quick-open-backdrop";

    pub fn row(index: usize) -> String {
        format!("lyrics-quick-open-row-{index}")
    }
}

const ROW_HEIGHT: f32 = 52.;
const ROW_TALL: f32 = 68.;
const WEB_HEADER: f32 = 30.;
const HEADER_HEIGHT: f32 = 54.;
const BODY_HEIGHT: f32 = 524.;

/// A row of the result list: a song in the library, or a web hit.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Local(QuickHit),
    Online(LyricsOnlineResult),
}

/// What the finder asks of the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickOpenEvent {
    Close,
}

pub struct QuickOpen {
    lyrics: Entity<Lyrics>,
    backend: Arc<dyn LyricsBackend>,
    input: Entity<InputState>,
    query: String,
    index: QuickIndex,
    revision: Option<u64>,
    hits: Vec<QuickHit>,
    active: usize,
    online: Vec<LyricsOnlineResult>,
    online_searching: bool,
    online_error: Option<String>,
    search_seq: u64,
    preview_open: bool,
    preview: Option<LyricsOnlinePreview>,
    preview_loading: bool,
    preview_seq: u64,
    importing: Option<String>,
    added: Option<String>,
    closing: bool,
    scroll: VirtualListScrollHandle,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<QuickOpenEvent> for QuickOpen {}

impl Focusable for QuickOpen {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl QuickOpen {
    pub fn new(lyrics: Entity<Lyrics>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let backend = lyrics.read(cx).backend().clone();
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search library or find any song online…")
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        let mut subs = Vec::new();
        subs.push(cx.observe(&lyrics, |this, _, cx| {
            this.reindex(cx);
            cx.notify();
        }));
        subs.push(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let value = this.input.read(cx).value().to_string();
                this.set_query(value, cx);
            }
        }));
        let weak = cx.entity().downgrade();
        let input_focus = input.read(cx).focus_handle(cx);
        subs.push(cx.intercept_keystrokes(move |event, window, cx| {
            if !input_focus.is_focused(window) {
                return;
            }
            let k = &event.keystroke;
            let command = k.modifiers.platform || k.modifiers.control;
            let key = k.key.clone();
            let handled = weak
                .update(cx, |this, cx| this.key(&key, command, cx))
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        }));
        let mut this = Self {
            lyrics,
            backend,
            input,
            query: String::new(),
            index: QuickIndex::default(),
            revision: None,
            hits: Vec::new(),
            active: 0,
            online: Vec::new(),
            online_searching: false,
            online_error: None,
            search_seq: 0,
            preview_open: false,
            preview: None,
            preview_loading: false,
            preview_seq: 0,
            importing: None,
            added: None,
            closing: false,
            scroll: VirtualListScrollHandle::new(),
            focus: cx.focus_handle(),
            _subscriptions: subs,
        };
        this.reindex(cx);
        this
    }

    fn reindex(&mut self, cx: &mut Context<Self>) {
        let lyrics = self.lyrics.read(cx);
        if self.revision == Some(lyrics.revision()) {
            return;
        }
        self.revision = Some(lyrics.revision());
        self.index = QuickIndex::new(lyrics.songs());
        self.rank(cx);
    }

    fn rank(&mut self, cx: &mut Context<Self>) {
        let lyrics = self.lyrics.read(cx);
        self.hits = rows::quick_open(lyrics.songs(), &self.index, &self.query);
        self.active = self.active.min(self.row_count().saturating_sub(1));
    }

    // ─── Reading ────────────────────────────────────────────────────────────

    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn hits(&self) -> &[QuickHit] {
        &self.hits
    }
    pub fn online(&self) -> &[LyricsOnlineResult] {
        &self.online
    }
    pub fn online_error(&self) -> Option<&str> {
        self.online_error.as_deref()
    }
    pub fn online_searching(&self) -> bool {
        self.online_searching
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn preview_open(&self) -> bool {
        self.preview_open
    }
    pub fn preview(&self) -> Option<&LyricsOnlinePreview> {
        self.preview.as_ref()
    }
    pub fn importing(&self) -> Option<&str> {
        self.importing.as_deref()
    }
    pub fn added(&self) -> Option<&str> {
        self.added.as_deref()
    }
    pub fn row_count(&self) -> usize {
        self.hits.len() + self.online.len()
    }
    pub fn row(&self, index: usize) -> Option<Row> {
        if let Some(hit) = self.hits.get(index) {
            return Some(Row::Local(hit.clone()));
        }
        self.online
            .get(index - self.hits.len())
            .cloned()
            .map(Row::Online)
    }
    fn can_search_web(&self) -> bool {
        self.query.trim().chars().count() >= MIN_ONLINE_QUERY
    }
    fn local_song(&self, hit: &QuickHit, cx: &gpui_kit::App) -> Option<LyricsSong> {
        self.lyrics.read(cx).songs().get(hit.song).cloned()
    }

    // ─── Acting ─────────────────────────────────────────────────────────────

    /// A new query is a new question: web hits, the preview and the active
    /// row reset.
    pub fn set_query(&mut self, value: String, cx: &mut Context<Self>) {
        if value == self.query {
            return;
        }
        self.query = value;
        self.search_seq += 1;
        self.online.clear();
        self.online_error = None;
        self.online_searching = false;
        self.preview_open = false;
        self.preview = None;
        self.preview_loading = false;
        self.active = 0;
        self.rank(cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        self.set_query(String::new(), cx);
    }

    /// On the operator's word only: never per keystroke.
    pub fn search_web(&mut self, cx: &mut Context<Self>) {
        if !self.can_search_web() || self.online_searching {
            return;
        }
        self.search_seq += 1;
        let seq = self.search_seq;
        self.online_searching = true;
        self.online_error = None;
        let call = ask(
            &self.backend,
            LyricsCall::SearchOnline(self.query.trim().to_owned()),
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.search_seq != seq {
                    return;
                }
                this.online_searching = false;
                match answer {
                    Ok(LyricsAnswer::Online(hits)) => {
                        if hits.is_empty() {
                            this.online_error = Some(rows::NO_LYRICS_FOUND.into());
                        }
                        this.online = hits;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        this.online.clear();
                        this.online_error = Some(format_online_lyrics_error(&e));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn set_active(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.row_count() || index == self.active {
            return;
        }
        self.active = index;
        self.scroll.scroll_to_item(index, ScrollStrategy::Center);
        if self.preview_open {
            self.load_preview(cx);
        }
        cx.notify();
    }

    /// A click: the row becomes active and the preview opens.
    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.row_count() {
            return;
        }
        self.active = index;
        self.preview_open = true;
        self.load_preview(cx);
        cx.notify();
    }

    /// Fetches the active web hit's lyrics, only while previewed.
    fn load_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_seq += 1;
        self.preview = None;
        self.preview_loading = false;
        let Some(Row::Online(result)) = self.row(self.active) else {
            return;
        };
        if rows::lyrics_unavailable(&result) {
            self.online_error = Some(rows::UNAVAILABLE_QUICK.into());
            return;
        }
        self.preview_loading = true;
        let seq = self.preview_seq;
        let call = ask(
            &self.backend,
            LyricsCall::PreviewOnline(OnlineSource::of(&result)),
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.preview_seq != seq {
                    return;
                }
                this.preview_loading = false;
                match answer {
                    Ok(LyricsAnswer::Preview(p)) => this.preview = Some(p),
                    Ok(_) => {}
                    Err(e) => {
                        this.preview = None;
                        this.online_error = Some(format_online_lyrics_error(&e));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Enter: open the song, or import a web hit then open it.
    pub fn open_row(&mut self, index: usize, cx: &mut Context<Self>) {
        match self.row(index) {
            Some(Row::Local(hit)) => {
                if let Some(song) = self.local_song(&hit, cx) {
                    self.open(song, cx);
                }
            }
            Some(Row::Online(result)) => self.import(result, cx),
            None => {}
        }
    }

    fn open(&mut self, song: LyricsSong, cx: &mut Context<Self>) {
        self.lyrics.update(cx, |m, cx| m.open_song(song, cx));
        self.close(cx);
    }

    fn import(&mut self, result: LyricsOnlineResult, cx: &mut Context<Self>) {
        if rows::lyrics_unavailable(&result) {
            self.backend
                .call(LyricsCall::OpenUrl(result.url), Box::new(|_| {}));
            return;
        }
        if self.importing.is_some() {
            return;
        }
        self.importing = Some(result.id.clone());
        self.online_error = None;
        let call = ask(
            &self.backend,
            LyricsCall::ImportOnline(OnlineSource::of(&result)),
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.importing = None;
                match answer {
                    Ok(LyricsAnswer::Song(song)) => this.open(song, cx),
                    Ok(_) => {}
                    Err(e) => this.online_error = Some(format_online_lyrics_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// ⌘↵: queue a library song on the active setlist (creating one first).
    pub fn add_active(&mut self, cx: &mut Context<Self>) {
        let Some(Row::Local(hit)) = self.row(self.active) else {
            return;
        };
        let Some(song) = self.local_song(&hit, cx) else {
            return;
        };
        let id = song.id.clone();
        let weak = cx.entity().downgrade();
        self.lyrics.update(cx, |m, cx| {
            m.add_to_active_setlist(id.clone(), cx, move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.added = Some(id);
                    cx.notify();
                });
            })
        });
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        if cx.reduced_motion() {
            cx.emit(QuickOpenEvent::Close);
            return;
        }
        self.closing = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(motion::MODAL_EXIT.duration_ms))
                .await;
            let _ = this.update(cx, |_, cx| {
                cx.emit(QuickOpenEvent::Close);
            });
        })
        .detach();
    }

    /// ↑/↓, ↵, ⌘↵ and Esc while the field has focus.
    pub fn key(&mut self, key: &str, command: bool, cx: &mut Context<Self>) -> bool {
        match key {
            "escape" => self.close(cx),
            "down" => {
                let next = (self.active + 1).min(self.row_count().saturating_sub(1));
                self.set_active(next, cx);
            }
            "up" => self.set_active(self.active.saturating_sub(1), cx),
            "enter" => match self.row(self.active) {
                Some(Row::Local(_)) if command => self.add_active(cx),
                Some(_) => self.open_row(self.active, cx),
                // Nothing matched: ↵ (or ⌘↵) asks the web.
                None if self.can_search_web() && !self.online_searching => self.search_web(cx),
                None => return false,
            },
            _ => return false,
        }
        cx.notify();
        true
    }

    // ─── Render ─────────────────────────────────────────────────────────────

    fn row_size(&self, index: usize) -> gpui_kit::Size<gpui_kit::Pixels> {
        let tall = match self.row(index) {
            Some(Row::Local(hit)) => hit.matched_lyric.is_some(),
            Some(Row::Online(r)) => r.snippet.as_deref().is_some_and(|s| !s.trim().is_empty()),
            None => false,
        };
        let first_web = index == self.hits.len() && !self.online.is_empty();
        let h = if tall { ROW_TALL } else { ROW_HEIGHT } + if first_web { WEB_HEADER } else { 0. };
        size(px(544.), px(h))
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let Some(row) = self.row(index) else {
            return div().into_any_element();
        };
        let active = index == self.active;
        let lyrics = self.lyrics.read(cx);
        let setlist = lyrics.active_setlist();
        let (icon, title, meta, snippet, queued, importing, is_local, open_label, badge) = match &row
        {
            Row::Local(hit) => {
                let song = &lyrics.songs()[hit.song];
                let sections = song.sections.len();
                let meta = match (song.artist.trim().is_empty(), sections) {
                    (false, 0) => format!("{} · No lyrics yet", song.artist.trim()),
                    (false, 1) => format!("{} · 1 section", song.artist.trim()),
                    (false, n) => format!("{} · {n} sections", song.artist.trim()),
                    (true, 0) => "No lyrics yet".into(),
                    (true, 1) => "1 section".into(),
                    (true, n) => format!("{n} sections"),
                };
                (
                    KairoIcon::Music2,
                    song.title.clone(),
                    meta,
                    hit.matched_lyric.as_ref().map(|l| l.trim().to_owned()),
                    setlist.is_some_and(|l| l.song_ids.contains(&song.id))
                        || self.added.as_deref() == Some(song.id.as_str()),
                    false,
                    true,
                    "Open",
                    None,
                )
            }
            Row::Online(r) => (
                KairoIcon::Globe,
                r.title.clone(),
                if rows::lyrics_unavailable(r) {
                    "Web match · Open source".into()
                } else if r.artist.trim().is_empty() {
                    "Unknown artist".into()
                } else {
                    r.artist.clone()
                },
                r.snippet
                    .as_ref()
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty()),
                false,
                self.importing.as_deref() == Some(r.id.as_str()),
                false,
                if rows::lyrics_unavailable(r) {
                    "Source"
                } else {
                    "Import"
                },
                Some(if r.existing_song_id.is_some() {
                    "In library".to_owned()
                } else {
                    r.provider.as_str().to_owned()
                }),
            ),
        };
        let first_web = index == self.hits.len() && matches!(row, Row::Online(_));
        let web_count = self.online.len();
        let row_h = if snippet.is_some() {
            ROW_TALL
        } else {
            ROW_HEIGHT
        };
        let entity = cx.entity();
        let click = entity.clone();
        let add_click = entity.clone();
        let open_click = entity;
        let has_meta = !meta.trim().is_empty();
        v_flex()
            .w_full()
            .when(first_web, |d| {
                d.child(
                    h_flex()
                        .h(px(WEB_HEADER))
                        .items_end()
                        .justify_between()
                        .px_2()
                        .pb_1()
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1p5()
                                .text_size(text::MICRO.size)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                                .child(Icon::new(KairoIcon::Globe).size_2p5())
                                .child("FROM THE WEB"),
                        )
                        .child(
                            div()
                                .text_size(text::MICRO.size)
                                .text_color(k.text(TextStep::T500))
                                .child(if web_count == 1 {
                                    "1 online match".to_owned()
                                } else {
                                    format!("{web_count} online matches")
                                }),
                        ),
                )
            })
            .child(
                div().w_full().h(px(row_h)).py_0p5().child(
                    h_flex()
                        .id(SharedString::from(ids::row(index)))
                        .test_support()
                        .role(Role::Button)
                        .aria_label(SharedString::from(title.clone()))
                        .relative()
                        .w_full()
                        .h_full()
                        .items_center()
                        .gap_2p5()
                        .px_2p5()
                        .rounded(radius::md())
                        .cursor_pointer()
                        .active(|s| s.opacity(0.84))
                        .map(|d| {
                            if active {
                                d.bg(k.accent(kairo_ui_kit::tokens::Step::S500).opacity(0.18))
                                    .border_1()
                                    .border_color(
                                        k.accent(kairo_ui_kit::tokens::Step::S500).opacity(0.45),
                                    )
                            } else {
                                d.border_1()
                                    .border_color(gpui_kit::transparent_black())
                                    .hover(|s| s.bg(k.surface_tertiary.opacity(0.65)))
                            }
                        })
                        .on_click(move |event, _, cx| {
                            click.update(cx, |this, cx| {
                                if event.click_count() >= 2 {
                                    this.open_row(index, cx)
                                } else {
                                    this.choose(index, cx)
                                }
                            })
                        })
                        .child(
                            div()
                                .size_7()
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(radius::sm())
                                .map(|d| {
                                    if active {
                                        d.bg(k.accent(kairo_ui_kit::tokens::Step::S500)
                                            .opacity(0.22))
                                            .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                                    } else if !is_local {
                                        d.bg(k.accent(kairo_ui_kit::tokens::Step::S500)
                                            .opacity(0.10))
                                            .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                                    } else {
                                        d.bg(k.surface_tertiary)
                                            .text_color(k.text(TextStep::T400))
                                    }
                                })
                                .child(Icon::new(icon).size_3p5()),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .justify_center()
                                .gap_0p5()
                                .child(
                                    h_flex()
                                        .min_w_0()
                                        .items_center()
                                        .gap_1p5()
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(text::SM.size)
                                                .font_weight(if active {
                                                    FontWeight::SEMIBOLD
                                                } else {
                                                    FontWeight::MEDIUM
                                                })
                                                .text_color(k.text_primary)
                                                .child(title),
                                        )
                                        .children(badge.map(|b| {
                                            div()
                                                .flex_none()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(radius::sm())
                                                .bg(k.surface_tertiary)
                                                .text_size(text::MICRO.size)
                                                .text_color(k.text(TextStep::T400))
                                                .child(b)
                                        })),
                                )
                                .when(has_meta, |d| {
                                    d.child(
                                        div()
                                            .truncate()
                                            .text_size(text::LABEL.size)
                                            .text_color(if active {
                                                k.text(TextStep::T300)
                                            } else {
                                                k.text(TextStep::T500)
                                            })
                                            .child(meta),
                                    )
                                })
                                .children(snippet.map(|s| {
                                    div()
                                        .truncate()
                                        .text_size(text::MICRO.size)
                                        .text_color(if active {
                                            k.accent(kairo_ui_kit::tokens::Step::S300)
                                        } else {
                                            k.text(TextStep::T400)
                                        })
                                        .child(format!("“{s}”"))
                                })),
                        )
                        .when(!active && queued, |d| {
                            d.child(
                                h_flex()
                                    .flex_none()
                                    .items_center()
                                    .gap_1()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded(radius::sm())
                                    .bg(k.surface_tertiary)
                                    .text_size(text::MICRO.size)
                                    .text_color(k.notice_text)
                                    .child(Icon::new(KairoIcon::ListMusic).size_3())
                                    .child("Queued"),
                            )
                        })
                        .when(importing, |d| {
                            d.child(motion::animate(
                                h_flex()
                                    .flex_none()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .py_0p5()
                                    .rounded(radius::sm())
                                    .bg(k.accent(kairo_ui_kit::tokens::Step::S500).opacity(0.16))
                                    .text_size(text::MICRO.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                                    .child(Icon::new(KairoIcon::Loader).size_3())
                                    .child("Importing…"),
                                ("quick-open-importing", index),
                                motion::PULSE,
                                cx,
                            ))
                        })
                        .when(active && !importing, |d| {
                            d.child(motion::animate(
                                h_flex()
                                    .flex_none()
                                    .items_center()
                                    .gap_1()
                                    .on_mouse_down(
                                        gpui_kit::MouseButton::Left,
                                        |_, _, cx| cx.stop_propagation(),
                                    )
                                    .when(is_local, |d| {
                                        d.child(
                                            Button::new(SharedString::from(format!(
                                                "lyrics-quick-open-row-add-{index}"
                                            )))
                                            .cursor_pointer()
                                            .ghost()
                                            .xsmall()
                                            .icon(Icon::new(if queued {
                                                KairoIcon::Check
                                            } else {
                                                KairoIcon::Plus
                                            }))
                                            .accessibility_label(if queued {
                                                "In playlist"
                                            } else {
                                                "Add to playlist"
                                            })
                                            .tooltip(if queued {
                                                "In playlist"
                                            } else {
                                                "Add to playlist (⌘↵)"
                                            })
                                            .on_click(move |_, _, cx| {
                                                add_click.update(cx, |this, cx| {
                                                    this.active = index;
                                                    this.add_active(cx);
                                                })
                                            }),
                                        )
                                    })
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "lyrics-quick-open-row-open-{index}"
                                        )))
                                        .cursor_pointer()
                                        .primary()
                                        .xsmall()
                                        .label(open_label)
                                        .on_click(move |_, _, cx| {
                                            open_click.update(cx, |this, cx| {
                                                this.open_row(index, cx)
                                            })
                                        }),
                                    ),
                                ("quick-open-row-actions", index),
                                motion::ACTION_IN,
                                cx,
                            ))
                        }),
                ),
            )
            .into_any_element()
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.preview_open {
            return None;
        }
        let k = *cx.kairo();
        let row = self.row(self.active)?;
        let entity = cx.entity();
        let lyrics = self.lyrics.read(cx);
        let setlist_name = lyrics.active_setlist().map(|l| l.name.clone());
        let body: AnyElement = match &row {
            Row::Online(r) => {
                let meta = format!(
                    "{} · {}",
                    if r.artist.is_empty() {
                        "Unknown artist"
                    } else {
                        &r.artist
                    },
                    r.provider.as_str()
                );
                let content: AnyElement = if self.preview_loading {
                    preview_skeleton(k, cx)
                } else if let Some(p) = &self.preview {
                    v_flex()
                        .mt_3()
                        .gap_2p5()
                        .children(review_notice_element(p.review.as_ref(), cx))
                        .children(p.sections.iter().map(|s| preview_section_card(k, s)))
                        .into_any_element()
                } else {
                    muted_line(
                        k,
                        self.online_error
                            .as_deref()
                            .unwrap_or("Could not read the lyrics on that page."),
                    )
                };
                v_flex()
                    .child(heading(k, &r.title))
                    .child(
                        h_flex()
                            .mt_1()
                            .items_center()
                            .gap_1p5()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(Icon::new(KairoIcon::Globe).size_2p5())
                            .child(meta)
                            .when(r.existing_song_id.is_some(), |d| {
                                d.child(
                                    div()
                                        .text_color(k.notice_text)
                                        .child("· already in your library"),
                                )
                            }),
                    )
                    .child(content)
                    .into_any_element()
            }
            Row::Local(hit) => {
                let song = &lyrics.songs()[hit.song];
                let artist = if song.artist.is_empty() {
                    "Unknown artist".to_owned()
                } else {
                    song.artist.clone()
                };
                let sections = song.sections.len();
                v_flex()
                    .child(heading(k, &song.title))
                    .child(
                        div()
                            .mt_1()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(if sections > 0 {
                                format!("{artist} · {sections} sections")
                            } else {
                                artist
                            }),
                    )
                    .child(if sections > 0 {
                        v_flex()
                            .mt_3()
                            .gap_2p5()
                            .children(song.sections.iter().map(|s| preview_section_card(k, s)))
                            .into_any_element()
                    } else {
                        muted_line(k, "This song has no lyrics saved yet.")
                    })
                    .into_any_element()
            }
        };
        let local = matches!(row, Row::Local(_));
        let added = match &row {
            Row::Local(hit) => lyrics
                .songs()
                .get(hit.song)
                .is_some_and(|s| self.added.as_deref() == Some(s.id.as_str())),
            _ => false,
        };
        let add_label = if added {
            "Added".to_owned()
        } else if let Some(name) = setlist_name {
            format!("Add to {name}")
        } else {
            "Add to playlist".into()
        };
        let open_label = match &row {
            Row::Online(r) if self.importing.as_deref() == Some(r.id.as_str()) => "Importing",
            Row::Online(r) if rows::lyrics_unavailable(r) => "Open source",
            Row::Online(_) => "Import & open",
            Row::Local(_) => "Open",
        };
        let (add, open, close_preview) = (entity.clone(), entity.clone(), entity);
        let active = self.active;
        let preview_panel = v_flex()
            .id(ids::PREVIEW)
            .test_support()
            .aria_label("Song preview")
            .w(px(440.))
            .h(px(HEADER_HEIGHT + BODY_HEIGHT))
            .flex_none()
            .overflow_hidden()
            .rounded(radius::xl())
            .bg(k.surface_secondary)
            .border_1()
            .border_color(k.surface_border)
            .child(
                h_flex()
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .border_b_1()
                    .border_color(k.panel_border)
                    .child(
                        div()
                            .text_size(text::LABEL.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text(TextStep::T400))
                            .child("LYRICS PREVIEW"),
                    )
                    .child(
                        Button::new("lyrics-quick-open-preview-close")
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::X))
                            .accessibility_label("Close preview")
                            .tooltip("Close preview")
                            .on_click(move |_, _, cx| {
                                close_preview.update(cx, |this, cx| {
                                    this.preview_open = false;
                                    cx.notify();
                                })
                            }),
                    ),
            )
            .child(
                div()
                    .id("lyrics-quick-open-preview-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_5()
                    .py_4()
                    .child(motion::animate(
                        div().child(body),
                        ("quick-open-preview-song", active),
                        motion::DETECT_STATE_IN,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .px_4()
                    .py_2p5()
                    .border_t_1()
                    .border_color(k.panel_border)
                    .child(
                        Button::new(ids::ADD)
                            .cursor_pointer()
                            .secondary()
                            .small()
                            .icon(Icon::new(if added {
                                KairoIcon::Check
                            } else {
                                KairoIcon::Plus
                            }))
                            .label(add_label)
                            .disabled(!local)
                            .when(!local, |b| b.tooltip("Import the song first"))
                            .on_click(move |_, _, cx| {
                                add.update(cx, |this, cx| this.add_active(cx))
                            }),
                    )
                    .child(
                        Button::new(ids::OPEN)
                            .cursor_pointer()
                            .primary()
                            .small()
                            .label(open_label)
                            .disabled(self.importing.is_some())
                            .on_click(move |_, _, cx| {
                                open.update(cx, |this, cx| this.open_row(active, cx))
                            }),
                    ),
            );
        Some(if self.closing {
            motion::animate(
                preview_panel,
                "quick-open-preview-out",
                motion::MODAL_EXIT,
                cx,
            )
        } else {
            motion::animate(
                preview_panel,
                "quick-open-preview-in",
                motion::FLYOUT_IN,
                cx,
            )
        })
    }
}

fn heading(k: kairo_ui_kit::KairoColors, title: &str) -> AnyElement {
    div()
        .text_size(text::LG.size)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(k.text_primary)
        .child(title.to_owned())
        .into_any_element()
}

fn muted_line(k: kairo_ui_kit::KairoColors, line: &str) -> AnyElement {
    div()
        .mt_6()
        .text_size(text::XS.size)
        .text_color(k.text(TextStep::T600))
        .child(line.to_owned())
        .into_any_element()
}

/// A section's label and its lines, as read-only text.
pub(crate) fn section_block(
    k: kairo_ui_kit::KairoColors,
    section: &LyricsSongSection,
) -> AnyElement {
    let label = if section.label.is_empty() {
        section.kind.as_str().to_owned()
    } else {
        section.label.clone()
    };
    v_flex()
        .child(
            div()
                .text_size(text::MICRO.size)
                .font_weight(FontWeight::MEDIUM)
                .text_color(k.text(TextStep::T600))
                .child(label.to_uppercase()),
        )
        .child(
            v_flex()
                .mt_1()
                .text_size(text::SM.size)
                .text_color(k.text(TextStep::T300))
                .children(
                    section
                        .lines
                        .iter()
                        .map(|l| div().min_h_2().child(l.clone())),
                ),
        )
        .into_any_element()
}

impl Render for QuickOpen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let count = rows::quick_count(&self.query, self.hits.len());
        let rows_n = self.row_count();
        let sizes: Rc<Vec<_>> = Rc::new((0..rows_n).map(|i| self.row_size(i)).collect());
        let entity = cx.entity();
        let list: AnyElement = if rows_n > 0 {
            v_virtual_list(cx.entity(), ids::LIST, sizes, |this, range, _, cx| {
                range.map(|i| this.render_row(i, cx)).collect::<Vec<_>>()
            })
            .track_scroll(&self.scroll)
            .flex_1()
            .min_h_0()
            .px_2()
            .into_any_element()
        } else if self.online_searching {
            v_flex()
                .id(ids::EMPTY)
                .test_support()
                .flex_1()
                .min_h_32()
                .px_3p5()
                .py_4()
                .gap_3()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2p5()
                        .px_3()
                        .py_2p5()
                        .rounded(radius::md())
                        .bg(k.accent(kairo_ui_kit::tokens::Step::S500).opacity(0.10))
                        .border_1()
                        .border_color(k.accent(kairo_ui_kit::tokens::Step::S500).opacity(0.28))
                        .child(
                            div()
                                .size_7()
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(k.accent(kairo_ui_kit::tokens::Step::S500).opacity(0.18))
                                .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                                .child(Icon::new(KairoIcon::Loader).size_3p5()),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .gap_0p5()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(text::SM.size)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(k.text_primary)
                                        .child(format!(
                                            "Searching online for “{}”…",
                                            self.query.trim()
                                        )),
                                )
                                .child(
                                    div()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child("Finding lyrics and formatting sections"),
                                ),
                        ),
                )
                .child(search_skeleton_rows(k, 3, cx))
                .into_any_element()
        } else {
            let detail = self.online_error.clone().unwrap_or_else(|| {
                if self.can_search_web() {
                    "Search the web below to find and import lyrics into your library.".into()
                } else {
                    "Type at least 3 characters to search lyrics online.".into()
                }
            });
            v_flex()
                .id(ids::EMPTY)
                .test_support()
                .flex_1()
                .min_h_32()
                .items_center()
                .justify_center()
                .px_6()
                .text_center()
                .child(
                    div()
                        .size_10()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(k.surface_tertiary)
                        .text_color(k.text(TextStep::T400))
                        .child(Icon::new(KairoIcon::Music2).size_5()),
                )
                .child(
                    div()
                        .mt_3()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.text_primary)
                        .child("Not in your library"),
                )
                .child(
                    div()
                        .mt_1()
                        .max_w(px(340.))
                        .text_size(text::LABEL.size)
                        .text_color(if self.online_error.is_some() {
                            k.danger_text
                        } else {
                            k.text(TextStep::T500)
                        })
                        .child(detail),
                )
                .into_any_element()
        };
        let search_web = (self.can_search_web()
            && !self.online_searching
            && self.online.is_empty())
        .then(|| {
            let e = entity.clone();
            let accent = k.accent(kairo_ui_kit::tokens::Step::S500);
            h_flex()
                .id(ids::SEARCH_WEB)
                .test_support()
                .role(Role::Button)
                .aria_label(format!("Search the web for “{}”", self.query.trim()))
                .aria_keyshortcuts("Meta+Enter")
                .mx_2p5()
                .my_2()
                .gap_2p5()
                .items_center()
                .px_3()
                .py_2()
                .rounded(radius::md())
                .bg(accent.opacity(0.10))
                .border_1()
                .border_color(accent.opacity(0.32))
                .hover(|s| s.bg(accent.opacity(0.18)))
                .active(|s| s.opacity(0.84))
                .cursor_pointer()
                .on_click(move |_, _, cx| e.update(cx, |this, cx| this.search_web(cx)))
                .child(
                    div()
                        .size_7()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius::sm())
                        .bg(accent.opacity(0.18))
                        .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                        .child(Icon::new(KairoIcon::Globe).size_3p5()),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .gap_0p5()
                        .child(
                            div()
                                .truncate()
                                .text_size(text::SM.size)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(k.text_primary)
                                .child(format!("Search online for “{}”", self.query.trim())),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(text::MICRO.size)
                                .text_color(k.text(TextStep::T400))
                                .child(if rows_n == 0 {
                                    "Press Enter to search lyrics on the web"
                                } else {
                                    "Not seeing it? Find & import lyrics from the web"
                                }),
                        ),
                )
                .child(kbd(k, if rows_n == 0 { "↵" } else { "⌘↵" }))
        });
        let web_discovery = (!self.can_search_web() && !self.online_searching).then(|| {
            h_flex()
                .mx_2p5()
                .my_1p5()
                .items_center()
                .gap_2()
                .px_2p5()
                .py_1p5()
                .rounded(radius::md())
                .bg(k.surface.opacity(0.55))
                .border_1()
                .border_color(k.panel_border)
                .child(
                    Icon::new(KairoIcon::Globe)
                        .size_3()
                        .text_color(k.accent(kairo_ui_kit::tokens::Step::S400)),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(text::MICRO.size)
                        .text_color(k.text(TextStep::T400))
                        .child("Song not in your library? Type 3+ letters to search & import from the web."),
                )
        });
        let searching_more = (self.online_searching && rows_n > 0).then(|| {
            let accent = k.accent(kairo_ui_kit::tokens::Step::S500);
            v_flex()
                .mx_2p5()
                .my_2()
                .gap_2()
                .p_2p5()
                .rounded(radius::md())
                .bg(accent.opacity(0.08))
                .border_1()
                .border_color(accent.opacity(0.25))
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .text_size(text::LABEL.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                        .child(Icon::new(KairoIcon::Loader).size_3())
                        .child(format!("Searching online for “{}”…", self.query.trim())),
                )
                .child(search_skeleton_rows(k, 2, cx))
        });
        let online_err = self.online_error.as_ref().filter(|_| rows_n > 0).map(|err| {
            let retry = entity.clone();
            motion::animate(
                h_flex()
                    .mx_2p5()
                    .mb_2()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded(radius::md())
                    .bg(k.tints.red)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_size(text::LABEL.size)
                            .text_color(k.danger_text_soft)
                            .child(err.clone()),
                    )
                    .when(self.can_search_web(), |d| {
                        d.child(
                            Button::new("lyrics-quick-open-retry-web")
                                .cursor_pointer()
                                .ghost()
                                .xsmall()
                                .label("Try again")
                                .on_click(move |_, _, cx| {
                                    retry.update(cx, |this, cx| this.search_web(cx))
                                }),
                        )
                    }),
                "quick-open-error-shake",
                motion::OB_SHAKE,
                cx,
            )
        });
        let searching_pill = self.online_searching.then(|| {
            motion::animate(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .bg(k.accent(kairo_ui_kit::tokens::Step::S500).opacity(0.14))
                    .text_size(text::MICRO.size)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                    .child(Icon::new(KairoIcon::Loader).size_3())
                    .child("Searching web…"),
                "quick-open-searching-pill",
                motion::PULSE,
                cx,
            )
        });
        let clear = (!self.query.is_empty()).then(|| {
            let e = entity.clone();
            Button::new(ids::CLEAR)
                .cursor_pointer()
                .ghost()
                .xsmall()
                .icon(Icon::new(KairoIcon::X))
                .accessibility_label("Clear search")
                .on_click(move |_, window, cx| e.update(cx, |this, cx| this.clear(window, cx)))
        });
        let close_btn = {
            let e = entity.clone();
            Button::new("lyrics-quick-open-close")
                .cursor_pointer()
                .ghost()
                .xsmall()
                .icon(Icon::new(KairoIcon::X))
                .accessibility_label("Close search")
                .tooltip("Close (Esc)")
                .on_click(move |_, _, cx| e.update(cx, |this, cx| this.close(cx)))
        };
        let backdrop = entity.clone();
        let dialog = v_flex()
            .id(ids::DIALOG)
            .test_support()
            .role(Role::Dialog)
            .aria_label("Search song library")
            .track_focus(&self.focus)
            .w(px(560.))
            .flex_none()
            .overflow_hidden()
            .rounded(radius::xl())
            .bg(k.surface_secondary)
            .border_1()
            .border_color(k.surface_border)
            .child(
                h_flex()
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .px_3p5()
                    .border_b_1()
                    .border_color(k.panel_border)
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.input)
                                .kairo_size()
                                .appearance(false)
                                .prefix(Icon::new(KairoIcon::ScanSearch).size_4()),
                        ),
                    )
                    .children(searching_pill)
                    .children(clear)
                    .child(close_btn),
            )
            .child(
                v_flex()
                    .h(px(BODY_HEIGHT))
                    .min_h_0()
                    .child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_3p5()
                            .pt_2p5()
                            .pb_1p5()
                            .child(
                                div()
                                    .id(ids::COUNT)
                                    .test_support()
                                    .text_size(text::LABEL.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text(TextStep::T500))
                                    .child(count),
                            )
                            .when(!self.preview_open && rows_n > 0, |d| {
                                d.child(
                                    div()
                                        .text_size(text::MICRO.size)
                                        .text_color(k.text(TextStep::T600))
                                        .child("Click to preview · Double-click to open"),
                                )
                            }),
                    )
                    .child(list)
                    .children(web_discovery)
                    .children(search_web)
                    .children(searching_more)
                    .children(online_err)
                    .child(
                        h_flex()
                            .flex_none()
                            .flex_wrap()
                            .items_center()
                            .gap_3p5()
                            .px_3p5()
                            .py_2p5()
                            .border_t_1()
                            .border_color(k.panel_border)
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(hint(k, "↑↓", "move"))
                            .child(hint(k, "↵", "open"))
                            .child(hint(k, "⌘↵", "add to playlist"))
                            .child(hint(k, "Esc", "close")),
                    ),
            );
        let animated_dialog = if self.closing {
            motion::animate(dialog, "quick-open-dialog-out", motion::MODAL_EXIT, cx)
        } else {
            motion::animate(dialog, "quick-open-dialog-in", motion::MODAL_ENTER, cx)
        };
        let scrim_curve = if self.closing {
            motion::SCRIM_OUT
        } else {
            motion::SCRIM_IN
        };
        let scrim_id = if self.closing {
            "quick-open-scrim-out"
        } else {
            "quick-open-scrim-in"
        };
        let backdrop_el = div()
            .id(ids::BACKDROP)
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            // Centred in the window (owner, 2026-11-16).
            .items_center()
            .px_4()
            .py_6()
            .bg(k.scrim())
            .occlude()
            .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                backdrop.update(cx, |this, cx| this.close(cx))
            })
            .child(
                h_flex()
                    .items_start()
                    .gap_3()
                    // Clicks inside the finder do not reach the backdrop.
                    .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation()
                    })
                    .child(animated_dialog)
                    .children(self.render_preview(cx)),
            );
        if cx.reduced_motion() {
            backdrop_el.into_any_element()
        } else {
            use gpui_kit::AnimationExt;
            backdrop_el
                .with_animation(scrim_id, scrim_curve.animation(), move |el, delta| {
                    el.opacity(scrim_curve.sample(delta).opacity.clamp(0.0, 1.0))
                })
                .into_any_element()
        }
    }
}

fn preview_section_card(
    k: kairo_ui_kit::KairoColors,
    section: &LyricsSongSection,
) -> AnyElement {
    div()
        .rounded(radius::md())
        .bg(k.surface.opacity(0.6))
        .border_1()
        .border_color(k.panel_border)
        .px_3()
        .py_2p5()
        .child(section_block(k, section))
        .into_any_element()
}

fn preview_skeleton(k: kairo_ui_kit::KairoColors, cx: &gpui_kit::App) -> AnyElement {
    let block = |widths: &[f32]| {
        v_flex()
            .gap_1p5()
            .rounded(radius::md())
            .bg(k.surface.opacity(0.6))
            .border_1()
            .border_color(k.panel_border)
            .px_3()
            .py_2p5()
            .child(div().h(px(10.)).w(px(64.)).rounded(radius::sm()).bg(k.surface_tertiary))
            .children(widths.iter().map(|w| {
                div()
                    .h(px(12.))
                    .w(gpui_kit::relative(*w))
                    .rounded(radius::sm())
                    .bg(k.surface_tertiary.opacity(0.7))
            }))
    };
    motion::animate(
        v_flex()
            .mt_3()
            .gap_2p5()
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .text_size(text::LABEL.size)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(k.accent(kairo_ui_kit::tokens::Step::S400))
                    .child(Icon::new(KairoIcon::Loader).size_3())
                    .child("Loading and reviewing lyrics…"),
            )
            .child(block(&[0.85, 0.72, 0.90, 0.60]))
            .child(block(&[0.78, 0.88, 0.65])),
        "quick-open-preview-skeleton",
        motion::PULSE,
        cx,
    )
}

fn search_skeleton_rows(
    k: kairo_ui_kit::KairoColors,
    count: usize,
    cx: &gpui_kit::App,
) -> AnyElement {
    let widths: [(f32, f32); 3] = [(0.58, 0.36), (0.46, 0.30), (0.64, 0.40)];
    motion::animate(
        v_flex().gap_1p5().children((0..count).map(|i| {
            let (tw, aw) = widths[i % widths.len()];
            h_flex()
                .items_center()
                .gap_2p5()
                .px_2p5()
                .py_2()
                .rounded(radius::md())
                .bg(k.surface.opacity(0.5))
                .border_1()
                .border_color(k.panel_border.opacity(0.6))
                .child(div().size_7().flex_none().rounded(radius::sm()).bg(k.surface_tertiary))
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .gap_1p5()
                        .child(
                            div()
                                .h(px(12.))
                                .w(gpui_kit::relative(tw))
                                .rounded(radius::sm())
                                .bg(k.surface_tertiary),
                        )
                        .child(
                            div()
                                .h(px(10.))
                                .w(gpui_kit::relative(aw))
                                .rounded(radius::sm())
                                .bg(k.surface_tertiary.opacity(0.65)),
                        ),
                )
        })),
        ("quick-open-search-skeleton", count),
        motion::PULSE,
        cx,
    )
}

fn kbd(k: kairo_ui_kit::KairoColors, keys: &str) -> AnyElement {
    div()
        .flex_none()
        .rounded(radius::sm())
        .bg(k.surface_tertiary)
        .border_1()
        .border_color(k.panel_border)
        .px_1p5()
        .py_0p5()
        .text_size(text::MICRO.size)
        .font_weight(FontWeight::MEDIUM)
        .text_color(k.text(TextStep::T300))
        .child(keys.to_owned())
        .into_any_element()
}

fn hint(k: kairo_ui_kit::KairoColors, keys: &str, what: &str) -> AnyElement {
    h_flex()
        .items_center()
        .gap_1p5()
        .child(kbd(k, keys))
        .child(what.to_owned())
        .into_any_element()
}
