//! House audio (`TracksPanel.tsx`, `TracksPlayer.tsx`,
//! `useTracksPlaybackStore.ts`, prompt 084): the walk-in tracks in the
//! booth toolbox's Audio tab. Play, pause, stop and seek drive the
//! operator process's own player (`kairo_audio::tracks::TracksPlayer`, 056),
//! which follows the library; the panel reads its position at display rate
//! only while a track plays.

use kairo_ui_kit::components::PopoverTriggerExt;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div, prelude::FluentBuilder,
    px,
};
use kairo_domain::media::{TrackItem, TracksLibrary, format_media_clock};
use kairo_ui_kit::components::{ConfirmRequest, confirm};
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

/// How often the position is read while a track plays (one display frame).
pub const POLL: Duration = Duration::from_millis(16);
/// The seek bar's resolution.
const SEEK_STEPS: f32 = 1000.;
/// The longest track name.
pub const MAX_NAME: usize = 120;

/// Element ids, for tests.
pub mod ids {
    pub const PANEL: &str = "tracks-panel";
    pub const EMPTY: &str = "tracks-empty";
    pub const ADD_FIRST: &str = "tracks-add-first";
    pub const ADD: &str = "tracks-add";
    pub const RESCAN: &str = "tracks-rescan";
    pub const FOLDER: &str = "tracks-folder";
    pub const ERROR: &str = "tracks-error";
    pub const RENAME: &str = "tracks-rename";
    pub const ELAPSED: &str = "tracks-elapsed";
    pub const TOTAL: &str = "tracks-total";
    pub fn play(id: &str) -> String {
        format!("tracks-play-{id}")
    }
    pub fn name(id: &str) -> String {
        format!("tracks-name-{id}")
    }
    pub fn stop(id: &str) -> String {
        format!("tracks-stop-{id}")
    }
    pub fn more(id: &str) -> String {
        format!("tracks-more-{id}")
    }
}

/// The player's progress (`setTime`), plus why the live track cannot play.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackTime {
    pub current_time: f64,
    pub duration: f64,
    pub ended: bool,
    /// The player's failure (no audio device, unreadable file).
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TracksCall {
    Play(String),
    SetPaused(bool),
    Stop,
    Rename {
        id: String,
        name: String,
    },
    /// Answers the library unchanged.
    Reveal(String),
    /// Moves the file to the Trash.
    Remove(String),
    /// The Add tracks dialog.
    Import,
    ChooseFolder,
    Rescan,
}

pub type TracksReply = Box<dyn FnOnce(Result<TracksLibrary, String>) + Send>;
pub type TracksSink = Arc<dyn Fn(TracksLibrary) + Send + Sync>;

pub trait TracksBackend: Send + Sync + 'static {
    fn library(&self) -> TracksLibrary;
    /// `tracks:library`; dropping the guard stops it.
    fn listen(&self, sink: TracksSink) -> Box<dyn Send>;
    fn call(&self, call: TracksCall, reply: TracksReply);
    /// The local player's position, read on the UI thread (cheap).
    fn time(&self) -> TrackTime;
    /// `requestSeek`: never before 0.
    fn seek(&self, seconds: f64);
}

/// The live item.
pub fn live(library: &TracksLibrary) -> Option<&TrackItem> {
    library
        .live_id
        .as_deref()
        .and_then(|id| library.items.iter().find(|i| i.id == id))
}

/// `requestSeek`'s clamp: within the track once its length is known.
pub fn clamp_seek(seconds: f64, duration: f64) -> f64 {
    let cap = if duration > 0. { duration } else { seconds };
    cap.min(seconds.max(0.))
}

/// The rename to send: trimmed, at most 120 characters, and changed.
pub fn rename_target(draft: &str, current: &str) -> Option<String> {
    let trimmed: String = draft.trim().chars().take(MAX_NAME).collect();
    (!trimmed.is_empty() && trimmed != current).then_some(trimmed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TracksEvent {
    /// File → Import → Audio: the booth shows its Audio tab.
    ShowAudioTab,
}

pub struct TracksPanel {
    backend: Arc<dyn TracksBackend>,
    library: TracksLibrary,
    time: TrackTime,
    busy: bool,
    error: Option<String>,
    scrub: Option<f64>,
    renaming: Option<String>,
    rename: Entity<InputState>,
    cancel_rename: bool,
    seek: Entity<SliderState>,
    polls: u64,
    _poll: Option<Task<()>>,
    _feed: Option<Task<()>>,
    _listening: Box<dyn Send>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TracksEvent> for TracksPanel {}

impl TracksPanel {
    pub fn new(
        backend: Arc<dyn TracksBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let library = backend.library();
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<TracksLibrary>();
        let listening = backend.listen(Arc::new(move |l| {
            let _ = tx.unbounded_send(l);
        }));
        let feed = cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(l) = rx.next().await {
                if this.update(cx, |this, cx| this.set_library(l, cx)).is_err() {
                    break;
                }
            }
        });
        let rename = cx.new(|cx| InputState::new(window, cx));
        let seek = cx.new(|_| SliderState::new().min(0.).max(SEEK_STEPS).step(1.));
        let mut subs = Vec::new();
        subs.push(
            cx.subscribe(&rename, |this, state, e: &InputEvent, cx| match e {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let draft = state.read(cx).value().to_string();
                    this.commit_rename(&draft, cx);
                }
                _ => {}
            }),
        );
        subs.push(cx.subscribe(&seek, |this, _, e: &SliderEvent, cx| match e {
            SliderEvent::Change(v) => {
                let s = v.start() as f64 / SEEK_STEPS as f64 * this.time.duration;
                this.scrub = Some(s);
                this.seek_to(s, cx);
            }
            SliderEvent::Release(_) => {
                this.scrub = None;
                cx.notify();
            }
        }));
        let weak = cx.entity().downgrade();
        let rename_focus = rename.read(cx).focus_handle(cx);
        subs.push(cx.intercept_keystrokes(move |e, window, cx| {
            if e.keystroke.key == "escape" && rename_focus.is_focused(window) {
                let _ = weak.update(cx, |this, cx| {
                    this.cancel_rename = true;
                    this.renaming = None;
                    cx.notify();
                });
                cx.stop_propagation();
            }
        }));
        let mut this = Self {
            backend,
            library,
            time: TrackTime::default(),
            busy: false,
            error: None,
            scrub: None,
            renaming: None,
            rename,
            cancel_rename: false,
            seek,
            polls: 0,
            _poll: None,
            _feed: Some(feed),
            _listening: listening,
            _subscriptions: subs,
        };
        this.follow_playback(cx);
        this
    }

    pub fn library(&self) -> &TracksLibrary {
        &self.library
    }
    pub fn time(&self) -> &TrackTime {
        &self.time
    }
    pub fn error(&self) -> Option<&str> {
        self.library
            .error
            .as_deref()
            .or(self.error.as_deref())
            .or(self.time.error.as_deref())
    }
    pub fn busy(&self) -> bool {
        self.busy
    }
    pub fn renaming(&self) -> Option<&str> {
        self.renaming.as_deref()
    }
    /// Position reads so far (for the display-rate check).
    pub fn polls(&self) -> u64 {
        self.polls
    }
    /// A live track, not paused and not ended.
    pub fn playing(&self) -> bool {
        live(&self.library).is_some() && !self.library.live_paused && !self.time.ended
    }
    /// What the bar shows: the drag while scrubbing, else the player.
    pub fn shown(&self) -> f64 {
        self.scrub.unwrap_or(self.time.current_time)
    }

    fn set_library(&mut self, library: TracksLibrary, cx: &mut Context<Self>) {
        let was = self.library.live_id.clone();
        self.library = library;
        if self.library.live_id != was || live(&self.library).is_none() {
            self.time = TrackTime::default();
        }
        self.follow_playback(cx);
        cx.notify();
    }

    /// Reads the position every frame while playing, and once otherwise.
    fn follow_playback(&mut self, cx: &mut Context<Self>) {
        self.read_time();
        if !self.playing() {
            self._poll = None;
            return;
        }
        if self._poll.is_some() {
            return;
        }
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let more = this
                    .update(cx, |this, cx| {
                        this.read_time();
                        cx.notify();
                        this.playing()
                    })
                    .unwrap_or(false);
                if !more {
                    let _ = this.update(cx, |this, _| this._poll = None);
                    break;
                }
            }
        }));
    }

    fn read_time(&mut self) {
        self.polls += 1;
        self.time = if live(&self.library).is_some() {
            self.backend.time()
        } else {
            TrackTime::default()
        };
    }

    fn run(&mut self, call: TracksCall, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.call(
            call,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let answer = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match answer {
                    Ok(library) => this.set_library(library, cx),
                    Err(e) => this.error = Some(e.trim_start_matches("Error: ").to_owned()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The play button: plays, or toggles pause on the live track (an ended
    /// track plays again from the start).
    pub fn press_play(&mut self, id: String, cx: &mut Context<Self>) {
        if self.library.live_id.as_deref() == Some(id.as_str()) {
            let paused = !(self.library.live_paused || self.time.ended);
            self.run(TracksCall::SetPaused(paused), cx);
        } else {
            self.run(TracksCall::Play(id), cx);
        }
    }

    /// The name: plays a track that is not live; does nothing on the live one.
    pub fn press_name(&mut self, id: String, cx: &mut Context<Self>) {
        if self.library.live_id.as_deref() != Some(id.as_str()) {
            self.run(TracksCall::Play(id), cx);
        }
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.scrub = None;
        self.run(TracksCall::Stop, cx);
    }

    pub fn seek_to(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let s = clamp_seek(seconds, self.time.duration);
        self.backend.seek(s);
        self.time.current_time = s;
        self.time.ended = false;
        self.follow_playback(cx);
        cx.notify();
    }

    pub fn add(&mut self, cx: &mut Context<Self>) {
        self.run(TracksCall::Import, cx);
    }
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.run(TracksCall::Rescan, cx);
    }
    pub fn choose_folder(&mut self, cx: &mut Context<Self>) {
        self.run(TracksCall::ChooseFolder, cx);
    }
    pub fn reveal(&mut self, id: String, cx: &mut Context<Self>) {
        self.run(TracksCall::Reveal(id), cx);
    }
    /// After the confirm: the folder is the library, so the file goes to the Trash.
    pub fn remove(&mut self, id: String, cx: &mut Context<Self>) {
        self.run(TracksCall::Remove(id), cx);
    }

    /// File → Import → Audio.
    pub fn import_request(&mut self, cx: &mut Context<Self>) {
        cx.emit(TracksEvent::ShowAudioTab);
        self.add(cx);
    }

    pub fn begin_rename(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .library
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.name.clone())
        else {
            return;
        };
        self.cancel_rename = false;
        self.renaming = Some(id);
        self.rename.update(cx, |s, cx| {
            s.set_value(name, window, cx);
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        cx.notify();
    }

    /// Enter or blur saves; Esc cancels.
    pub fn commit_rename(&mut self, draft: &str, cx: &mut Context<Self>) {
        let Some(id) = self.renaming.take() else {
            return;
        };
        if std::mem::take(&mut self.cancel_rename) {
            cx.notify();
            return;
        }
        let current = self
            .library
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.name.clone())
            .unwrap_or_default();
        if let Some(name) = rename_target(draft, &current) {
            self.run(TracksCall::Rename { id, name }, cx);
        }
        cx.notify();
    }

    fn confirm_remove(&mut self, item: TrackItem, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity().downgrade();
        confirm(
            ConfirmRequest::new(
                format!("Move “{}” to the Trash?", item.name),
                "Move to Trash",
            )
            .detail("The file leaves the audio folder, and you can restore it from the Trash.")
            .destructive(),
            window,
            cx,
            move |ok, _, cx| {
                if ok {
                    let _ = page.update(cx, |p, cx| p.remove(item.id.clone(), cx));
                }
            },
        );
    }

    fn render_item(
        &self,
        item: &TrackItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let active = self.library.live_id.as_deref() == Some(item.id.as_str());
        let playing = active && self.playing();
        let busy = self.busy;
        let entity = cx.entity();
        let (play, name_e, stop, more) = (
            entity.clone(),
            entity.clone(),
            entity.clone(),
            entity.clone(),
        );
        let (id1, id2) = (item.id.clone(), item.id.clone());
        let menu_item = item.clone();
        let ready = self.time.duration > 0.;
        let shown = self.shown();
        if active && ready && self.scrub.is_none() {
            let v = (shown.min(self.time.duration) / self.time.duration) as f32 * SEEK_STEPS;
            self.seek.update(cx, |s, cx| s.set_value(v, window, cx));
        }
        let name: AnyElement = if self.renaming.as_deref() == Some(item.id.as_str()) {
            div()
                .id(ids::RENAME)
                .test_support()
                .flex_1()
                .min_w_0()
                .child(Input::new(&self.rename).xsmall().aria_label("Track name"))
                .into_any_element()
        } else {
            div()
                .id(SharedString::from(ids::name(&item.id)))
                .test_support()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(text::XS.size)
                .map(|d| {
                    if active {
                        d.font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                    } else {
                        d.font_weight(FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T200))
                    }
                })
                .when(!busy, |d| {
                    d.on_click(move |_, _, cx| {
                        name_e.update(cx, |p, cx| p.press_name(id2.clone(), cx))
                    })
                })
                .child(item.name.clone())
                .into_any_element()
        };
        v_flex()
            .p_1()
            .child(
                v_flex()
                    .rounded(radius::md())
                    .px_2p5()
                    .py_2()
                    .map(|d| {
                        if active {
                            d.bg(k.surface_rail)
                        } else {
                            d.hover(|s| s.bg(k.surface_rail.opacity(0.6)))
                        }
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new(SharedString::from(ids::play(&item.id)))
                                    .xsmall()
                                    .map(|b| if active { b.primary() } else { b.ghost() })
                                    .icon(Icon::new(if playing {
                                        KairoIcon::Pause
                                    } else {
                                        KairoIcon::Play
                                    }))
                                    .accessibility_label(SharedString::from(format!(
                                        "{} {}",
                                        if playing { "Pause" } else { "Play" },
                                        item.name
                                    )))
                                    .tooltip(if playing { "Pause" } else { "Play" })
                                    .disabled(busy)
                                    .on_click(move |_, _, cx| {
                                        play.update(cx, |p, cx| p.press_play(id1.clone(), cx))
                                    }),
                            )
                            .child(name)
                            .when(active, |d| {
                                d.child(
                                    Button::new(SharedString::from(ids::stop(&item.id)))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(KairoIcon::X))
                                        .accessibility_label(SharedString::from(format!(
                                            "Stop {}",
                                            item.name
                                        )))
                                        .tooltip("Stop and clear")
                                        .disabled(busy)
                                        .on_click(move |_, _, cx| {
                                            stop.update(cx, |p, cx| p.stop(cx))
                                        }),
                                )
                            })
                            .child(
                                Button::new(SharedString::from(ids::more(&item.id)))
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(KairoIcon::MoreHorizontal))
                                    .accessibility_label(SharedString::from(format!(
                                        "Options for {}",
                                        item.name
                                    )))
                                    .disabled(busy)
                                    .popover_trigger()
                                    .dropdown_menu(move |menu, _, _| {
                                        let (r, s, t) = (more.clone(), more.clone(), more.clone());
                                        let (i1, i2, it) = (
                                            menu_item.id.clone(),
                                            menu_item.id.clone(),
                                            menu_item.clone(),
                                        );
                                        menu.item(
                                            PopupMenuItem::new("Rename…")
                                                .icon(Icon::new(KairoIcon::Edit2))
                                                .on_click(move |_, w, cx| {
                                                    r.update(cx, |p, cx| {
                                                        p.begin_rename(i1.clone(), w, cx)
                                                    })
                                                }),
                                        )
                                        .item(
                                            PopupMenuItem::new("Show in folder")
                                                .icon(Icon::new(KairoIcon::FolderOpen))
                                                .on_click(move |_, _, cx| {
                                                    s.update(cx, |p, cx| p.reveal(i2.clone(), cx))
                                                }),
                                        )
                                        .separator()
                                        .item(
                                            PopupMenuItem::new("Move to Trash…")
                                                .icon(Icon::new(KairoIcon::Trash2))
                                                .on_click(move |_, w, cx| {
                                                    t.update(cx, |p, cx| {
                                                        p.confirm_remove(it.clone(), w, cx)
                                                    })
                                                }),
                                        )
                                    }),
                            ),
                    )
                    .when(active, |d| {
                        d.child(
                            h_flex()
                                .mt_2()
                                .pl_8()
                                .gap_2()
                                .child(
                                    div()
                                        .id(ids::ELAPSED)
                                        .test_support()
                                        .w_8()
                                        .flex_none()
                                        .text_right()
                                        .text_size(text::MICRO.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(format_media_clock(shown)),
                                )
                                .child(
                                    div().flex_1().min_w_0().child(
                                        Slider::new(&self.seek)
                                            .disabled(!ready)
                                            .accessibility_label("Seek"),
                                    ),
                                )
                                .child(
                                    div()
                                        .id(ids::TOTAL)
                                        .test_support()
                                        .w_8()
                                        .flex_none()
                                        .text_size(text::MICRO.size)
                                        .text_color(k.text(TextStep::T500))
                                        .child(if ready {
                                            format_media_clock(self.time.duration)
                                        } else {
                                            "–:––".into()
                                        }),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Render for TracksPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let busy = self.busy;
        let entity = cx.entity();
        let (rescan, folder, add, add_first) =
            (entity.clone(), entity.clone(), entity.clone(), entity);
        let items = self.library.items.clone();
        let list: AnyElement = if items.is_empty() {
            v_flex()
                .id(ids::EMPTY)
                .test_support()
                .items_center()
                .gap_2()
                .px_4()
                .py_6()
                .child(div().flex().items_center().justify_center().size_9().rounded_full().bg(k.surface_tertiary).child(Icon::new(KairoIcon::Music2).size_4().text_color(k.text(TextStep::T400))))
                .child(div().text_size(text::XS.size).font_weight(FontWeight::MEDIUM).text_color(k.text(TextStep::T300)).child("No walk-in tracks yet"))
                .child(div().max_w(px(256.)).text_center().text_size(text::LABEL.size).text_color(k.text(TextStep::T500)).child("Music for before and after the service. It plays on this Mac, from a folder next to your Media."))
                .child(Button::new(ids::ADD_FIRST).primary().xsmall().icon(Icon::new(KairoIcon::Plus)).label("Add tracks").disabled(busy).on_click(move |_, _, cx| add_first.update(cx, |p, cx| p.add(cx))))
                .into_any_element()
        } else {
            v_flex()
                .children(items.iter().map(|i| self.render_item(i, window, cx)))
                .into_any_element()
        };
        v_flex()
            .id(ids::PANEL)
            .test_support()
            .px_3()
            .pt_3()
            .child(
                h_flex()
                    .min_h_7()
                    .gap_1p5()
                    .px_1()
                    .pb_1p5()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(text::XS.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text(TextStep::T200))
                            .child("House audio"),
                    )
                    .child(
                        Button::new(ids::RESCAN)
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::RefreshCw))
                            .accessibility_label("Refresh the audio folder")
                            .tooltip("Refresh the audio folder")
                            .disabled(busy)
                            .on_click(move |_, _, cx| rescan.update(cx, |p, cx| p.rescan(cx))),
                    )
                    .child(
                        Button::new(ids::FOLDER)
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::FolderOpen))
                            .accessibility_label("Choose audio folder")
                            .tooltip("Choose audio folder")
                            .disabled(busy)
                            .on_click(move |_, _, cx| {
                                folder.update(cx, |p, cx| p.choose_folder(cx))
                            }),
                    )
                    .when(!items.is_empty(), |d| {
                        d.child(
                            Button::new(ids::ADD)
                                .xsmall()
                                .icon(Icon::new(KairoIcon::Plus))
                                .label("Add")
                                .disabled(busy)
                                .on_click(move |_, _, cx| add.update(cx, |p, cx| p.add(cx))),
                        )
                    }),
            )
            .children(self.error().map(|e| {
                div()
                    .id(ids::ERROR)
                    .test_support()
                    .px_1()
                    .pb_2()
                    .text_size(text::LABEL.size)
                    .text_color(k.danger_text)
                    .child(e.to_owned())
            }))
            .child(
                div()
                    .overflow_hidden()
                    .rounded(radius::lg())
                    .bg(k.surface)
                    .border_1()
                    .border_color(k.neutral(Step::S800).opacity(0.5))
                    .child(list),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeks_clamp_and_renames_trim() {
        assert_eq!(clamp_seek(-3., 100.), 0.);
        assert_eq!(clamp_seek(130., 100.), 100.);
        assert_eq!(
            clamp_seek(12., 0.),
            12.,
            "no cap before the length is known"
        );
        assert_eq!(rename_target("  Walk in  ", "Old"), Some("Walk in".into()));
        assert_eq!(rename_target("Old", "Old"), None);
        assert_eq!(rename_target("   ", "Old"), None);
        assert_eq!(
            rename_target(&"x".repeat(200), "Old")
                .unwrap()
                .chars()
                .count(),
            MAX_NAME
        );
    }
}
