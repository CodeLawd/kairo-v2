//! The ⌘F song finder (`SongQuickOpen.tsx`): the library ranked by title,
//! artist and lyric as you type, a web fallback on request, a preview that
//! flies out beside the list once a row is chosen, and ⌘↵ to queue a song on
//! the active setlist. It opens from any page as the shell's
//! `Overlay::SongSearch`.

use std::rc::Rc;
use std::sync::Arc;

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

const ROW_HEIGHT: f32 = 32.;
const ROW_TALL: f32 = 46.;
const WEB_HEADER: f32 = 28.;

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
        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Search titles, artists or lyrics"));
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
        cx.emit(QuickOpenEvent::Close);
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
        cx.emit(QuickOpenEvent::Close);
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
        size(px(440.), px(h))
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let Some(row) = self.row(index) else {
            return div().into_any_element();
        };
        let active = index == self.active;
        let lyrics = self.lyrics.read(cx);
        let setlist = lyrics.active_setlist();
        let (icon, title, meta, snippet, queued, importing) = match &row {
            Row::Local(hit) => {
                let song = &lyrics.songs()[hit.song];
                (
                    KairoIcon::Music2,
                    song.title.clone(),
                    song.artist.clone(),
                    hit.matched_lyric.as_ref().map(|l| l.trim().to_owned()),
                    setlist.is_some_and(|l| l.song_ids.contains(&song.id)),
                    false,
                )
            }
            Row::Online(r) => (
                KairoIcon::Globe,
                r.title.clone(),
                if rows::lyrics_unavailable(r) {
                    "Web match · Open source".into()
                } else {
                    format!(
                        "{} · {}",
                        if r.artist.is_empty() {
                            "Unknown artist"
                        } else {
                            &r.artist
                        },
                        r.provider.as_str()
                    )
                },
                r.snippet
                    .as_ref()
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty()),
                false,
                self.importing.as_deref() == Some(r.id.as_str()),
            ),
        };
        let first_web = index == self.hits.len() && matches!(row, Row::Online(_));
        let entity = cx.entity();
        let click = entity.clone();
        let hover = entity.clone();
        v_flex()
            .w_full()
            .when(first_web, |d| {
                d.child(
                    h_flex()
                        .h(px(WEB_HEADER))
                        .items_end()
                        .gap_1p5()
                        .px_2()
                        .pb_1()
                        .text_size(text::MICRO.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T600))
                        .child(Icon::new(KairoIcon::Globe).size_2p5())
                        .child("FROM THE WEB"),
                )
            })
            .child(
                h_flex()
                    .id(SharedString::from(ids::row(index)))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(SharedString::from(title.clone()))
                    .w_full()
                    .flex_1()
                    .gap_2p5()
                    .px_2()
                    .rounded(radius::md())
                    .map(|d| if active { d.bg(k.row_selected()) } else { d })
                    .on_mouse_move(move |_, _, cx| {
                        hover.update(cx, |this, cx| {
                            if this.active != index {
                                this.active = index;
                                if this.preview_open {
                                    this.load_preview(cx);
                                }
                                cx.notify();
                            }
                        })
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
                    .child(Icon::new(icon).size_3().text_color(k.text(TextStep::T500)))
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_1p5()
                                    .items_baseline()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::SM.size)
                                            .text_color(k.text_primary)
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(meta),
                                    ),
                            )
                            .children(snippet.map(|s| {
                                div()
                                    .truncate()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child(format!("“{s}”"))
                            })),
                    )
                    .when(queued, |d| {
                        d.child(
                            Icon::new(KairoIcon::ListMusic)
                                .size_3()
                                .text_color(k.notice_text),
                        )
                    })
                    .when(importing, |d| {
                        d.child(
                            Icon::new(KairoIcon::Loader)
                                .size_3()
                                .text_color(k.notice_text),
                        )
                    }),
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
                    muted_line(k, "Loading and reviewing lyrics…")
                } else if let Some(p) = &self.preview {
                    v_flex()
                        .mt_4()
                        .gap_4()
                        .children(review_notice_element(p.review.as_ref(), cx))
                        .children(p.sections.iter().map(|s| section_block(k, s)))
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
                            .mt_4()
                            .gap_4()
                            .children(song.sections.iter().map(|s| section_block(k, s)))
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
        let (add, open) = (entity.clone(), entity);
        let active = self.active;
        Some(
            v_flex()
                .id(ids::PREVIEW)
                .test_support()
                .aria_label("Song preview")
                .w(px(448.))
                .h(px(520.))
                .flex_none()
                .overflow_hidden()
                .rounded(radius::xl())
                .bg(k.surface_secondary)
                .child(
                    div()
                        .id("lyrics-quick-open-preview-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px_5()
                        .py_4()
                        .child(body),
                )
                .child(
                    h_flex()
                        .flex_none()
                        .justify_end()
                        .gap_2()
                        .px_4()
                        .py_2p5()
                        .child(
                            Button::new(ids::ADD)
                                .ghost()
                                .xsmall()
                                .label(add_label)
                                .disabled(!local)
                                .when(!local, |b| b.tooltip("Import the song first"))
                                .on_click(move |_, _, cx| {
                                    add.update(cx, |this, cx| this.add_active(cx))
                                }),
                        )
                        .child(
                            Button::new(ids::OPEN)
                                .primary()
                                .xsmall()
                                .label(open_label)
                                .disabled(self.importing.is_some())
                                .on_click(move |_, _, cx| {
                                    open.update(cx, |this, cx| this.open_row(active, cx))
                                }),
                        ),
                )
                .into_any_element(),
        )
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
            .px_1p5()
            .into_any_element()
        } else {
            let (title, detail) = if self.online_searching {
                ("Searching the web…", None)
            } else {
                (
                    "Not in your library",
                    Some(
                        self.online_error
                            .clone()
                            .unwrap_or_else(|| "Try another title, artist, or lyric.".into()),
                    ),
                )
            };
            v_flex()
                .id(ids::EMPTY)
                .test_support()
                .flex_1()
                .min_h_32()
                .items_center()
                .justify_center()
                .px_5()
                .child(
                    Icon::new(if self.online_searching {
                        KairoIcon::Loader
                    } else {
                        KairoIcon::Music2
                    })
                    .size_5()
                    .text_color(k.text(TextStep::T600)),
                )
                .child(
                    div()
                        .mt_3()
                        .text_size(text::XS.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T400))
                        .child(title),
                )
                .children(detail.map(|d| {
                    div()
                        .mt_1()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T600))
                        .child(d)
                }))
                .into_any_element()
        };
        let search_web = (self.can_search_web()
            && !self.online_searching
            && self.online.is_empty())
        .then(|| {
            let e = entity.clone();
            h_flex()
                .id(ids::SEARCH_WEB)
                .test_support()
                .role(Role::Button)
                .mx_1p5()
                .mb_1p5()
                .gap_2p5()
                .px_2()
                .py_1p5()
                .rounded(radius::md())
                .text_color(k.text(TextStep::T300))
                .hover(|s| s.bg(k.surface_tertiary))
                .on_click(move |_, _, cx| e.update(cx, |this, cx| this.search_web(cx)))
                .child(Icon::new(KairoIcon::Globe).size_3())
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(text::SM.size)
                        .child(format!("Search the web for “{}”", self.query.trim())),
                )
                .child(kbd(k, "⌘↵"))
        });
        let searching_more = (self.online_searching && rows_n > 0).then(|| {
            div()
                .px_3()
                .py_2()
                .text_size(text::LABEL.size)
                .text_color(k.text(TextStep::T600))
                .child("Searching the web…")
        });
        let clear = (!self.query.is_empty()).then(|| {
            let e = entity.clone();
            Button::new(ids::CLEAR)
                .ghost()
                .xsmall()
                .icon(Icon::new(KairoIcon::X))
                .accessibility_label("Clear search")
                .on_click(move |_, window, cx| e.update(cx, |this, cx| this.clear(window, cx)))
        });
        let backdrop = entity.clone();
        div()
            .id(ids::BACKDROP)
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(72.))
            .px_4()
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
                    .child(
                        v_flex()
                            .id(ids::DIALOG)
                            .test_support()
                            .role(Role::Dialog)
                            .aria_label("Search song library")
                            .track_focus(&self.focus)
                            .w(px(480.))
                            .flex_none()
                            .overflow_hidden()
                            .rounded(radius::xl())
                            .bg(k.glass_modal())
                            .child(
                                h_flex()
                                    .h_12()
                                    .flex_none()
                                    .gap_2p5()
                                    .px_3p5()
                                    .child(
                                        div().flex_1().child(
                                            Input::new(&self.input)
                                                .appearance(false)
                                                .prefix(Icon::new(KairoIcon::ScanSearch).size_4()),
                                        ),
                                    )
                                    .children(clear),
                            )
                            .child(
                                v_flex()
                                    .h(px(520.))
                                    .min_h_0()
                                    .child(
                                        div()
                                            .id(ids::COUNT)
                                            .test_support()
                                            .flex_none()
                                            .px_3p5()
                                            .pt_2()
                                            .pb_1()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(count),
                                    )
                                    .child(list)
                                    .children(search_web)
                                    .children(searching_more)
                                    .child(
                                        h_flex()
                                            .flex_none()
                                            .flex_wrap()
                                            .gap_3()
                                            .px_3p5()
                                            .py_2()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(hint(k, "↑↓", "move"))
                                            .child(hint(k, "↵", "open"))
                                            .child(hint(k, "⌘↵", "add to playlist")),
                                    ),
                            ),
                    )
                    .children(self.render_preview(cx)),
            )
    }
}

fn kbd(k: kairo_ui_kit::KairoColors, keys: &str) -> AnyElement {
    div()
        .flex_none()
        .rounded(radius::sm())
        .bg(k.surface_tertiary)
        .px_1p5()
        .text_size(text::MICRO.size)
        .text_color(k.text(TextStep::T500))
        .child(keys.to_owned())
        .into_any_element()
}

fn hint(k: kairo_ui_kit::KairoColors, keys: &str, what: &str) -> AnyElement {
    h_flex()
        .gap_1()
        .child(
            div()
                .text_color(k.text(TextStep::T400))
                .child(keys.to_owned()),
        )
        .child(what.to_owned())
        .into_any_element()
}
