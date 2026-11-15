//! The host mounted once over the operator window (`TransferHost.tsx`).

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Context, ElementId, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, div, px, size,
};
use kairo_domain::kairo_bundle::SongImportChoice;
use kairo_domain::lyrics::duplicate::describe_duplicate;
use kairo_ui_kit::components::modal::{MovableDialog, MovableDialogEvent};
use kairo_ui_kit::components::{SegmentOption, SegmentedControl};
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::model::{
    DuplicateChoice, EXPORT_NAME_MAX, ExportPicker, ImportReview, Notice, PLAYLIST_CHOICES,
    describe_commit, export_notice, exported_label,
};
use crate::{FeedSink, Listening, TransferBackend, TransferFeed};

/// `NOTICE_MS`.
pub const NOTICE_FOR: Duration = Duration::from_millis(6_000);

/// Element ids, for tests.
pub mod ids {
    pub const NOTICE: &str = "transfer-notice";
    pub const NOTICE_DISMISS: &str = "transfer-notice-dismiss";
    pub const IMPORT: &str = "transfer-import";
    pub const IMPORT_CANCEL: &str = "transfer-import-cancel";
    pub const IMPORT_CLOSE: &str = "transfer-import-close";
    pub const IMPORT_SETLIST: &str = "transfer-import-setlist";
    pub const IMPORT_BULK: &str = "transfer-import-bulk";
    pub const IMPORT_PLAYLIST: &str = "transfer-import-playlist";
    pub const IMPORT_ERROR: &str = "transfer-import-error";
    pub const EXPORT: &str = "transfer-export";
    pub const EXPORT_CANCEL: &str = "transfer-export-cancel";
    pub const EXPORT_CLOSE: &str = "transfer-export-close";
    pub const EXPORT_SELECT: &str = "transfer-export-select";
}

struct Review {
    state: ImportReview,
    frame: Entity<MovableDialog>,
    skipped_open: bool,
}

struct Picker {
    state: ExportPicker,
    frame: Entity<MovableDialog>,
    query: Entity<InputState>,
    name: Entity<InputState>,
}

/// The review screen, the song export picker and the result notice.
pub struct TransferHost {
    backend: Arc<dyn TransferBackend>,
    review: Option<Review>,
    picker: Option<Picker>,
    notice: Option<Notice>,
    notice_task: Option<Task<()>>,
    _listening: Listening,
    _feed: Task<()>,
    _subscriptions: Vec<Subscription>,
}

fn dialog_size(window: &Window, width: f32, height: f32) -> gpui_kit::Size<gpui_kit::Pixels> {
    let viewport = window.viewport_size();
    // `max-h-[min(640px,90vh)]`, and never wider than the window.
    size(
        px(width.min(f32::from(viewport.width) - 32.)),
        px(height.min(f32::from(viewport.height) * 0.9)),
    )
}

impl TransferHost {
    pub fn new(
        backend: Arc<dyn TransferBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tx, mut rx) = futures_channel::mpsc::unbounded();
        let sink: FeedSink = Arc::new(move |f| {
            let _ = tx.unbounded_send(f);
        });
        let listening = backend.listen(sink);
        let feed = cx.spawn_in(window, async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(f) = rx.next().await {
                let applied = this.update_in(cx, |t, window, cx| t.on_feed(f, window, cx));
                if applied.is_err() {
                    break;
                }
            }
        });
        Self {
            backend,
            review: None,
            picker: None,
            notice: None,
            notice_task: None,
            _listening: listening,
            _feed: feed,
            _subscriptions: Vec::new(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.review.is_some() || self.picker.is_some()
    }

    pub fn review(&self) -> Option<&ImportReview> {
        self.review.as_ref().map(|r| &r.state)
    }

    pub fn picker(&self) -> Option<&ExportPicker> {
        self.picker.as_ref().map(|p| &p.state)
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn on_feed(&mut self, feed: TransferFeed, window: &mut Window, cx: &mut Context<Self>) {
        match feed {
            TransferFeed::Opened(preview) => self.open_review(*preview, window, cx),
            TransferFeed::Notice(notice) => self.notify(Some(notice), cx),
            TransferFeed::PickSongs(preselect) => self.open_picker(preselect, window, cx),
            TransferFeed::Songs(songs) => {
                if let Some(picker) = &mut self.picker {
                    picker.state.set_songs(songs);
                    cx.notify();
                }
            }
        }
    }

    /// Shows the notice, which clears itself after [`NOTICE_FOR`].
    pub fn notify(&mut self, notice: Option<Notice>, cx: &mut Context<Self>) {
        self.notice = notice;
        self.notice_task = self.notice.as_ref().map(|_| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(NOTICE_FOR).await;
                let _ = this.update(cx, |t, cx| {
                    t.notice = None;
                    cx.notify();
                });
            })
        });
        cx.notify();
    }

    // ── Import review ──────────────────────────────────────────────────

    pub fn open_review(
        &mut self,
        preview: kairo_domain::kairo_bundle::TransferPreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A second file replaces the first; the first is forgotten.
        if let Some(old) = self.review.take() {
            self.backend.discard(old.state.preview.token);
        }
        let frame = cx.new(|cx| {
            MovableDialog::new(
                "kairo-import",
                format!("Import “{}”", preview.name),
                dialog_size(window, 512., 640.),
                window,
                cx,
            )
        });
        self._subscriptions.push(cx.subscribe(
            &frame,
            |this, _, event: &MovableDialogEvent, cx| {
                if matches!(event, MovableDialogEvent::Dismiss) {
                    this.close_review(cx);
                }
            },
        ));
        self.review = Some(Review {
            state: ImportReview::new(preview),
            frame,
            skipped_open: false,
        });
        cx.notify();
    }

    /// Cancel: refused while importing; discards the parsed file.
    pub fn close_review(&mut self, cx: &mut Context<Self>) {
        let Some(review) = &self.review else { return };
        if review.state.busy {
            return;
        }
        let token = review.state.preview.token.clone();
        self.backend.discard(token);
        self.review = None;
        cx.notify();
    }

    fn edit_review(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut ImportReview)) {
        if let Some(review) = &mut self.review
            && !review.state.busy
        {
            edit(&mut review.state);
            cx.notify();
        }
    }

    pub fn run_import(&mut self, cx: &mut Context<Self>) {
        let Some(review) = &mut self.review else {
            return;
        };
        if review.state.busy || review.state.nothing_to_do() {
            return;
        }
        review.state.busy = true;
        review.state.error = None;
        let request = review.state.request();
        let token = request.token.clone();
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.commit(
            request,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The import was interrupted.".into()));
            let _ = this.update(cx, |this, cx| {
                // Only the review that asked.
                let Some(review) = &mut this.review else {
                    return;
                };
                if review.state.preview.token != token {
                    return;
                }
                match result {
                    Ok(result) => {
                        this.review = None;
                        this.notify(Some(Notice::ok(describe_commit(&result))), cx);
                    }
                    Err(error) => {
                        review.state.error = Some(error);
                        review.state.busy = false;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ── Export picker ──────────────────────────────────────────────────

    pub fn open_picker(
        &mut self,
        preselect: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let frame = cx.new(|cx| {
            MovableDialog::new(
                "kairo-export",
                "Export songs",
                dialog_size(window, 448., 640.),
                window,
                cx,
            )
        });
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search songs"));
        let name = cx.new(|cx| InputState::new(window, cx));
        self._subscriptions.push(cx.subscribe(
            &frame,
            |this, _, event: &MovableDialogEvent, cx| {
                if matches!(event, MovableDialogEvent::Dismiss) {
                    this.close_picker(cx);
                }
            },
        ));
        self._subscriptions
            .push(cx.subscribe(&query, |this, query, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(picker) = &mut this.picker
                {
                    picker.state.query = query.read(cx).value().to_string();
                    cx.notify();
                }
            }));
        self._subscriptions.push(cx.subscribe_in(
            &name,
            window,
            |_, name, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = name.read(cx).value().to_string();
                    if value.chars().count() > EXPORT_NAME_MAX {
                        let cut: String = value.chars().take(EXPORT_NAME_MAX).collect();
                        name.update(cx, |input, cx| input.set_value(cut, window, cx));
                    }
                    cx.notify();
                }
            },
        ));
        query.update(cx, |q, cx| q.focus(window, cx));
        self.picker = Some(Picker {
            state: ExportPicker::new(Vec::new(), &preselect),
            frame,
            query,
            name,
        });
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.songs(Box::new(move |r| {
            let _ = tx.send(r);
        }));
        cx.spawn(async move |this, cx| {
            let songs = rx.await.unwrap_or_else(|_| Ok(Vec::new()));
            let _ = this.update(cx, |this, cx| match songs {
                Ok(songs) => {
                    if let Some(picker) = &mut this.picker {
                        picker.state.set_songs(songs);
                        cx.notify();
                    }
                }
                Err(error) => this.notify(Some(Notice::error(error)), cx),
            });
        })
        .detach();
        cx.notify();
    }

    /// Escape, Close and Cancel: refused while saving.
    pub fn close_picker(&mut self, cx: &mut Context<Self>) {
        if self.picker.as_ref().is_some_and(|p| p.state.busy) {
            return;
        }
        self.picker = None;
        cx.notify();
    }

    pub fn run_export(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        if picker.state.busy || picker.state.count() == 0 {
            return;
        }
        let request = picker.state.request(&picker.name.read(cx).value());
        picker.state.busy = true;
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.export(
            request,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = rx
                .await
                .unwrap_or_else(|_| Err("The export was interrupted.".into()));
            let _ = this.update(cx, |this, cx| {
                if let Some(picker) = &mut this.picker {
                    picker.state.busy = false;
                }
                match result {
                    Ok(result) if result.saved => {
                        this.picker = None;
                        let name = result.file_name.unwrap_or_default();
                        this.notify(Some(export_notice(&name)), cx);
                    }
                    // Cancelled in the save dialog: the picker stays.
                    Ok(_) => {}
                    Err(error) => this.notify(Some(Notice::error(error)), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ── Rendering ──────────────────────────────────────────────────────

    fn close_button(
        id: &'static str,
        cx: &mut Context<Self>,
        on: fn(&mut Self, &mut Context<Self>),
    ) -> AnyElement {
        let k = *cx.kairo();
        div()
            .id(id)
            .test_support()
            .role(Role::Button)
            .aria_label("Close")
            .size_7()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .text_color(k.text(TextStep::T400))
            .hover(move |s| s.bg(k.surface_tertiary).text_color(k.text_primary))
            .on_click(cx.listener(move |this, _, _, cx| on(this, cx)))
            .child(Icon::new(KairoIcon::X).size(px(14.)))
            .into_any_element()
    }

    fn render_review(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let review = self.review.as_ref()?;
        let k = *cx.kairo();
        let state = &review.state;
        let preview = &state.preview;
        let busy = state.busy;
        let kind_icon = if state.is_playlist() {
            KairoIcon::BookOpen
        } else if preview.setlist.is_some() {
            KairoIcon::ListMusic
        } else {
            KairoIcon::Music2
        };
        let handle = review
            .frame
            .update(cx, |f, cx| f.drag_handle("kairo-import-drag", cx));
        let header = handle
            .flex()
            .items_start()
            .gap_3()
            .px_5()
            .py_4()
            .child(
                div()
                    .size_9()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(radius::xl())
                    .bg(k.tints.teal)
                    .text_color(k.accent(Step::S300))
                    .child(Icon::new(kind_icon).size_4()),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child(format!("Import “{}”", preview.name)),
                    )
                    .child(
                        div()
                            .mt_0p5()
                            .truncate()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(exported_label(preview)),
                    ),
            )
            .child(Self::close_button(
                ids::IMPORT_CLOSE,
                cx,
                Self::close_review,
            ));

        let mut body = v_flex().gap_4();
        if let Some(playlist) = &preview.playlist {
            body = body.child(
                v_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .rounded(radius::xl())
                            .bg(k.surface_secondary)
                            .px_3p5()
                            .py_3()
                            .child(
                                div()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text_primary)
                                    .child(playlist.title.clone()),
                            )
                            .child(
                                div()
                                    .mt_0p5()
                                    .text_size(text::XS.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(format!(
                                        "Scripture playlist · {} passage{} · verse text included",
                                        playlist.item_count,
                                        if playlist.item_count == 1 { "" } else { "s" }
                                    )),
                            ),
                    )
                    .when(playlist.exists, |this| {
                        let entity = cx.entity();
                        this.child(
                            v_flex()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T300))
                                        .child("You already have this playlist."),
                                )
                                .child(
                                    SegmentedControl::new(
                                        ids::IMPORT_PLAYLIST,
                                        "Existing playlist",
                                    )
                                    .fit()
                                    .selected(
                                        PLAYLIST_CHOICES
                                            .iter()
                                            .find(|c| c.0 == state.playlist)
                                            .map_or("keep-both", |c| c.1),
                                    )
                                    .options(
                                        PLAYLIST_CHOICES
                                            .iter()
                                            .map(|c| SegmentOption::new(c.1, c.2)),
                                    )
                                    .on_change(
                                        move |key, _, cx| {
                                            let Some(choice) = PLAYLIST_CHOICES
                                                .iter()
                                                .find(|c| c.1 == key.as_ref())
                                                .map(|c| c.0)
                                            else {
                                                return;
                                            };
                                            entity.update(cx, |this, cx| {
                                                this.edit_review(cx, |r| r.playlist = choice)
                                            });
                                        },
                                    ),
                                ),
                        )
                    }),
            );
        } else {
            if state.all_already_here() {
                body = body.child(
                    div()
                        .rounded(radius::xl())
                        .bg(k.tints.teal)
                        .px_3p5()
                        .py_2p5()
                        .text_size(text::LABEL.size)
                        .text_color(k.accent(Step::S200))
                        .child(state.already_here_text()),
                );
            }
            body = body.child(
                div()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T400))
                    .child(state.summary()),
            );
            if let Some(setlist) = &preview.setlist {
                let entity = cx.entity();
                body = body.child(
                    h_flex()
                        .gap_2p5()
                        .rounded(radius::xl())
                        .bg(k.surface_secondary)
                        .px_3p5()
                        .py_2p5()
                        .child(
                            Checkbox::new(ids::IMPORT_SETLIST)
                                .checked(state.create_setlist)
                                .disabled(busy)
                                .on_click(move |v, _, cx| {
                                    let v = *v;
                                    entity.update(cx, |this, cx| {
                                        this.edit_review(cx, |r| r.create_setlist = v)
                                    });
                                }),
                        )
                        .child(
                            Icon::new(KairoIcon::ListMusic)
                                .size_3p5()
                                .text_color(k.text(TextStep::T500)),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(text::SM.size)
                                        .text_color(k.text_primary)
                                        .child(format!("Create playlist “{}”", setlist.name)),
                                )
                                .child(
                                    div()
                                        .text_size(text::XS.size)
                                        .text_color(k.text(TextStep::T500))
                                        .child(format!(
                                            "{} songs in service order",
                                            setlist.song_count
                                        )),
                                ),
                        ),
                );
            }
            if state.duplicates() > 0 {
                let entity = cx.entity();
                body = body.child(
                    h_flex()
                        .flex_wrap()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T300))
                                .child("For songs you already have"),
                        )
                        .child(
                            SegmentedControl::new(ids::IMPORT_BULK, "All duplicates")
                                .fit()
                                .selected(state.bulk.key())
                                .options(
                                    DuplicateChoice::ALL
                                        .iter()
                                        .map(|c| SegmentOption::new(c.1, c.2)),
                                )
                                .on_change(move |key, _, cx| {
                                    if let Some(choice) = DuplicateChoice::from_key(key) {
                                        entity.update(cx, |this, cx| {
                                            this.edit_review(cx, |r| r.apply_bulk(choice))
                                        });
                                    }
                                }),
                        ),
                );
            }
            let rows = preview.songs.iter().enumerate().map(|(index, song)| {
                let choice = state.choice(&song.id);
                let id = song.id.clone();
                let entity = cx.entity();
                let detail = match &song.duplicate {
                    Some(found) => describe_duplicate(found),
                    None => {
                        let sections = format!(
                            "{} section{}",
                            song.section_count,
                            if song.section_count == 1 { "" } else { "s" }
                        );
                        if song.artist.is_empty() {
                            sections
                        } else {
                            format!("{} · {sections}", song.artist)
                        }
                    }
                };
                h_flex()
                    .gap_2p5()
                    .px_3()
                    .py_2()
                    .child(if song.duplicate.is_some() {
                        div().w_3p5().flex_shrink_0().into_any_element()
                    } else {
                        Checkbox::new(ElementId::NamedInteger(
                            "transfer-song".into(),
                            index as u64,
                        ))
                        .checked(choice == SongImportChoice::Add)
                        .disabled(busy)
                        .on_click({
                            let id = id.clone();
                            let entity = entity.clone();
                            move |v, _, cx| {
                                let next = if *v {
                                    SongImportChoice::Add
                                } else {
                                    SongImportChoice::Skip
                                };
                                let id = id.clone();
                                entity.update(cx, |this, cx| {
                                    this.edit_review(cx, |r| r.set_choice(&id, next))
                                });
                            }
                        })
                        .into_any_element()
                    })
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .text_color(k.text_primary)
                                    .child(song.title.clone()),
                            )
                            .child(
                                div()
                                    .mt_0p5()
                                    .truncate()
                                    .text_size(text::XS.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(detail),
                            ),
                    )
                    .child(if song.duplicate.is_some() {
                        SegmentedControl::new(
                            ElementId::NamedInteger("transfer-song-choice".into(), index as u64),
                            format!("What to do with {}", song.title),
                        )
                        .fit()
                        .compact(true)
                        .selected(DuplicateChoice::of(choice).key())
                        .options(
                            DuplicateChoice::ALL
                                .iter()
                                .map(|c| SegmentOption::new(c.1, c.2)),
                        )
                        .on_change(move |key, _, cx| {
                            if let Some(next) = DuplicateChoice::from_key(key) {
                                let id = id.clone();
                                entity.update(cx, |this, cx| {
                                    this.edit_review(cx, |r| r.set_choice(&id, next.choice()))
                                });
                            }
                        })
                        .into_any_element()
                    } else {
                        div()
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(k.tints.teal)
                            .px_1p5()
                            .py_0p5()
                            .text_size(px(10.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.accent(Step::S300))
                            .child("New")
                            .into_any_element()
                    })
            });
            body = body.child(
                v_flex()
                    .rounded(radius::xl())
                    .overflow_hidden()
                    .children(rows),
            );
        }

        if let Some(skipped) = preview.skipped_files.as_ref().filter(|s| !s.is_empty()) {
            let open = review.skipped_open;
            body = body.child(
                v_flex()
                    .text_size(text::XS.size)
                    .text_color(k.warning_text)
                    .child(
                        div()
                            .id("transfer-skipped")
                            .test_support()
                            .role(Role::Button)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(r) = &mut this.review {
                                    r.skipped_open = !r.skipped_open;
                                    cx.notify();
                                }
                            }))
                            .child(format!(
                                "{} {} file{} could not be read and will be skipped",
                                if open { "▾" } else { "▸" },
                                skipped.len(),
                                if skipped.len() == 1 { "" } else { "s" }
                            )),
                    )
                    .when(open, |this| {
                        this.child(
                            v_flex()
                                .mt_1p5()
                                .gap_0p5()
                                .pl_3()
                                .text_color(k.text(TextStep::T500))
                                .children(skipped.iter().map(|f| {
                                    div()
                                        .truncate()
                                        .child(format!("{} — {}", f.file_name, f.error))
                                })),
                        )
                    }),
            );
        }
        if let Some(error) = &state.error {
            body = body.child(
                div()
                    .id(ids::IMPORT_ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .text_size(text::LABEL.size)
                    .text_color(k.danger_text)
                    .child(error.clone()),
            );
        }

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .px_5()
            .py_3()
            .child(
                secondary_button(ids::IMPORT_CANCEL, "Cancel", cx)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.close_review(cx))),
            )
            .child(
                primary_button(ids::IMPORT, state.import_label(), cx)
                    .loading(busy)
                    .disabled(busy || state.nothing_to_do())
                    .on_click(cx.listener(|this, _, _, cx| this.run_import(cx))),
            );
        let content = v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("transfer-import-body")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px_5()
                    .py_4()
                    .child(body),
            )
            .child(footer);
        let frame = review.frame.clone();
        Some(frame.update(cx, |f, cx| f.frame(content, window, cx)))
    }

    fn render_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.picker.as_ref()?;
        let k = *cx.kairo();
        let state = &picker.state;
        let busy = state.busy;
        let handle = picker
            .frame
            .update(cx, |f, cx| f.drag_handle("kairo-export-drag", cx));
        let header = handle
            .flex()
            .items_start()
            .gap_3()
            .px_5()
            .py_4()
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child("Export songs"),
                    )
                    .child(
                        div()
                            .mt_0p5()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child("Saves a .kairo file any Kairo can import, lyrics included."),
                    ),
            )
            .child(Self::close_button(
                ids::EXPORT_CLOSE,
                cx,
                Self::close_picker,
            ));
        let search = v_flex()
            .gap_2()
            .px_5()
            .py_3()
            .child(
                Input::new(&picker.query).small().prefix(
                    Icon::new(KairoIcon::ScanSearch)
                        .size_3p5()
                        .text_color(k.text(TextStep::T500)),
                ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .text_size(text::XS.size)
                    .child(
                        div()
                            .id(ids::EXPORT_SELECT)
                            .test_support()
                            .role(Role::Button)
                            .cursor_pointer()
                            .text_color(k.accent(Step::S400))
                            .hover(move |s| s.text_color(k.accent(Step::S300)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(p) = &mut this.picker {
                                    p.state.toggle_all_visible();
                                    cx.notify();
                                }
                            }))
                            .child(state.select_label()),
                    )
                    .child(div().text_color(k.text(TextStep::T500)).child(format!(
                        "{} of {} selected",
                        state.count(),
                        state.songs.len()
                    ))),
            );
        let visible = state.visible();
        let list = if visible.is_empty() {
            div()
                .px_3()
                .py_6()
                .text_center()
                .text_size(text::LABEL.size)
                .text_color(k.text(TextStep::T500))
                .child(state.empty_text())
                .into_any_element()
        } else {
            v_flex()
                .children(visible.into_iter().enumerate().map(|(index, song)| {
                    let id = song.id.clone();
                    let entity = cx.entity();
                    h_flex()
                        .id(ElementId::NamedInteger(
                            "transfer-export-row".into(),
                            index as u64,
                        ))
                        .gap_2p5()
                        .rounded_md()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .hover(move |s| s.bg(k.surface_tertiary))
                        .on_click({
                            let id = id.clone();
                            let entity = entity.clone();
                            move |_, _, cx| {
                                let id = id.clone();
                                entity.update(cx, |this, cx| {
                                    if let Some(p) = &mut this.picker {
                                        p.state.toggle(&id);
                                        cx.notify();
                                    }
                                })
                            }
                        })
                        .child(
                            Checkbox::new(ElementId::NamedInteger(
                                "transfer-export-song".into(),
                                index as u64,
                            ))
                            .checked(state.selected.contains(&song.id))
                            .on_click(move |_, _, cx| {
                                let id = id.clone();
                                entity.update(cx, |this, cx| {
                                    if let Some(p) = &mut this.picker {
                                        p.state.toggle(&id);
                                        cx.notify();
                                    }
                                })
                            }),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(text::SM.size)
                                        .text_color(k.text_primary)
                                        .child(song.title.clone()),
                                )
                                .when(!song.artist.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .truncate()
                                            .text_size(text::XS.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(song.artist.clone()),
                                    )
                                }),
                        )
                }))
                .into_any_element()
        };
        let placeholder = state.name_placeholder();
        picker.name.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        let footer = v_flex()
            .gap_2p5()
            .px_5()
            .py_3()
            .child(Input::new(&picker.name).small())
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        secondary_button(ids::EXPORT_CANCEL, "Cancel", cx)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.close_picker(cx))),
                    )
                    .child(
                        primary_button(ids::EXPORT, state.export_label(), cx)
                            .icon(Icon::new(KairoIcon::Upload).size_3p5())
                            .loading(busy)
                            .disabled(busy || state.count() == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.run_export(cx))),
                    ),
            );
        let content = v_flex()
            .size_full()
            .child(header)
            .child(search)
            .child(
                div()
                    .id("transfer-export-list")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px_2()
                    .py_1p5()
                    .child(list),
            )
            .child(footer);
        let frame = picker.frame.clone();
        Some(frame.update(cx, |f, cx| f.frame(content, window, cx)))
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.notice.as_ref()?;
        let k = *cx.kairo();
        Some(
            h_flex()
                .absolute()
                .bottom_5()
                .left_0()
                .right_0()
                .justify_center()
                .child(
                    h_flex()
                        .id(ids::NOTICE)
                        .test_support()
                        .role(Role::Status)
                        .aria_label(notice.text.clone())
                        .max_w_112()
                        .items_center()
                        .gap_2p5()
                        .rounded(radius::xl())
                        .bg(k.surface_elevated)
                        .px_3p5()
                        .py_2p5()
                        .text_size(text::SM.size)
                        .text_color(if notice.ok {
                            k.text_primary
                        } else {
                            k.danger_text_soft
                        })
                        .occlude()
                        .child(if notice.ok {
                            Icon::new(KairoIcon::CheckCircle)
                                .size_3p5()
                                .text_color(k.accent(Step::S400))
                        } else {
                            Icon::new(KairoIcon::AlertTriangle).size_3p5()
                        })
                        .child(div().min_w_0().flex_1().child(notice.text.clone()))
                        .child(
                            div()
                                .id(ids::NOTICE_DISMISS)
                                .test_support()
                                .role(Role::Button)
                                .aria_label("Dismiss")
                                .cursor_pointer()
                                .text_color(k.text(TextStep::T500))
                                .hover(move |s| s.text_color(k.text_primary))
                                .on_click(cx.listener(|this, _, _, cx| this.notify(None, cx)))
                                .child(Icon::new(KairoIcon::X).size_3()),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// The teal primary button (`btn-primary`).
fn primary_button(id: &'static str, label: impl Into<SharedString>, cx: &App) -> Button {
    let k = cx.kairo();
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(k.accent(Step::S500))
                .foreground(k.on_accent)
                .hover(k.accent(Step::S600))
                .active(k.accent(Step::S600)),
        )
        .small()
        .label(label.into())
}

/// The neutral secondary button (`btn-secondary`).
fn secondary_button(id: &'static str, label: impl Into<SharedString>, cx: &App) -> Button {
    let k = cx.kairo();
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(k.surface_tertiary)
                .foreground(k.text(TextStep::T200))
                .hover(k.surface_elevated)
                .active(k.surface_elevated),
        )
        .small()
        .label(label.into())
}

impl Render for TransferHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .children(self.render_notice(cx))
            .children(self.render_review(window, cx))
            .children(self.render_picker(window, cx))
    }
}
