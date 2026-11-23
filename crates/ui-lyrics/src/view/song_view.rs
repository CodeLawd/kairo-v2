//! The song view (`SectionSlideGrid`, `SongDivider`, `SlideContextMenu`,
//! `QuickEditCard` and the song header in `Lyrics.tsx`): one continuous
//! responsive grid per song across section boundaries, section colours and
//! labels, hotkeys, arrows, zoom, picks, paint, quick edit and push, with
//! every song of an open playlist laid out in order.

use kairo_ui_kit::components::PopoverTriggerExt;
use std::rc::Rc;

use gpui_kit::Focusable;
use gpui_kit::StyledImage;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, FontWeight, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, canvas, div, prelude::FluentBuilder,
    px,
};
use kairo_domain::lyrics::LyricsSong;
use kairo_domain::lyrics::reorder::Side;
use kairo_ui_kit::components::{ConfirmRequest, ZoomChanged, ZoomControl, ZoomRange, confirm};
use kairo_ui_kit::tokens::{Hue, Step, TextStep, radius, section_color, section_text, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::LyricsWorkspace;
use crate::grid::{self, GridItem, SlideCard};
use crate::model::{Lyrics, PickGesture};
use crate::rows;

/// Element ids, for tests.
pub mod ids {
    pub const VIEW: &str = super::super::ids::SONG;
    pub const GRID: &str = "lyrics-slide-grid";
    pub const HINT: &str = "lyrics-slide-hint";
    pub const DONE: &str = "lyrics-select-done";
    pub const ACTIONS: &str = "lyrics-song-actions";
    pub const QUICK_EDIT: &str = "lyrics-quick-edit";
    pub const QUICK_EDIT_SAVE: &str = "lyrics-quick-edit-save";
    pub const SEND_ERROR: &str = "lyrics-send-error";

    pub fn slide(index: usize) -> String {
        format!("lyrics-slide-{index}")
    }
    /// A card of another song in the playlist.
    pub fn other_slide(song: &str, index: usize) -> String {
        format!("lyrics-slide-{song}-{index}")
    }
    pub fn line(slide: usize, line: usize) -> String {
        format!("lyrics-slide-{slide}-line-{line}")
    }
    pub fn divider(song: &str) -> String {
        format!("lyrics-song-band-{song}")
    }
}

/// A slide being dragged within the open song.
#[derive(Clone)]
pub struct SlideDrag {
    pub from: usize,
    label: SharedString,
}

impl gpui_kit::Render for SlideDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        kairo_ui_kit::components::DragPill::new(self.label.clone(), "♪").render_pill(cx)
    }
}

/// The view's own controls: zoom, the quick-edit text box and the grid's
/// measured width.
pub(super) struct SongControls {
    zoom: Entity<ZoomControl>,
    quick: Entity<TextareaState>,
    quick_for: Option<usize>,
    width: Rc<std::cell::Cell<f32>>,
    _subscriptions: Vec<Subscription>,
}

/// A user's line colour (`#RRGGBB` from the song file) as a fill.
fn line_color(hex: &str) -> Option<Hsla> {
    gpui_kit::Rgba::try_from(hex).ok().map(Hsla::from)
}

impl SongControls {
    pub(super) fn new(
        model: &Entity<Lyrics>,
        window: &mut Window,
        cx: &mut Context<LyricsWorkspace>,
    ) -> Self {
        let zoom = cx.new(|cx| {
            ZoomControl::new(
                "lyrics-zoom",
                "Slide size",
                ZoomRange {
                    min: grid::ZOOM_MIN,
                    max: grid::ZOOM_MAX,
                    default: grid::ZOOM_DEFAULT,
                    step: 5.0,
                    button_step: 10.0,
                },
                grid::ZOOM_DEFAULT,
                window,
                cx,
            )
        });
        let quick = cx.new(|cx| TextareaState::new(window, cx));
        let mut subs = Vec::new();
        let m = model.clone();
        subs.push(
            cx.subscribe(&zoom, move |_, _, ZoomChanged(v): &ZoomChanged, cx| {
                let v = *v;
                m.update(cx, |m, cx| m.set_zoom(v, cx));
            }),
        );
        let m = model.clone();
        subs.push(
            cx.subscribe(&quick, move |this, _, event: &InputEvent, cx| {
                // Clicking away saves (`onBlur={() => finish(true)}`).
                if let InputEvent::Blur = event {
                    let text = this.song.quick.read(cx).value().to_string();
                    m.update(cx, |m, cx| m.save_quick_edit(text, cx));
                }
            }),
        );
        let weak = cx.entity().downgrade();
        let focus = quick.read(cx).focus_handle(cx);
        let m = model.clone();
        subs.push(cx.intercept_keystrokes(move |event, window, cx| {
            if !focus.is_focused(window) {
                return;
            }
            let k = &event.keystroke;
            let command = k.modifiers.platform || k.modifiers.control;
            if k.key == "escape" {
                m.update(cx, |m, cx| m.cancel_quick_edit(cx));
                cx.stop_propagation();
            } else if k.key == "enter" && command {
                let text = weak
                    .read_with(cx, |this, cx| this.song.quick.read(cx).value().to_string())
                    .unwrap_or_default();
                m.update(cx, |m, cx| m.save_quick_edit(text, cx));
                cx.stop_propagation();
            }
        }));
        Self {
            zoom,
            quick,
            quick_for: None,
            width: Rc::new(std::cell::Cell::new(720.)),
            _subscriptions: subs,
        }
    }

    pub(super) fn sync(&mut self, model: &Entity<Lyrics>, window: &mut Window, cx: &mut App) {
        let m = model.read(cx);
        let editing = m.quick_edit().map(|(i, t)| (i, t.to_owned()));
        let now = editing.as_ref().map(|(i, _)| *i);
        if now != self.quick_for {
            if let Some((_, text)) = editing {
                self.quick.update(cx, |s, cx| {
                    s.set_value(text, window, cx);
                    s.focus(window, cx);
                });
            }
            self.quick_for = now;
        }
    }
}

impl LyricsWorkspace {
    /// Keys while the song view has focus: arrows step, letters and digits
    /// are section hotkeys, Esc ends paint, picks and select mode.
    pub(super) fn song_key(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        if m.alt || m.control || m.platform || event.is_held {
            return;
        }
        let handled = self.model.update(cx, |model, cx| match k.key.as_str() {
            "escape" => model.escape(cx),
            "left" | "up" if !m.shift => model.arrow(false, cx).is_some(),
            "right" | "down" if !m.shift => model.arrow(true, cx).is_some(),
            key if !m.shift => model.hotkey(key, cx).is_some(),
            _ => false,
        });
        if handled {
            cx.stop_propagation();
        }
    }

    fn delete_slides(&mut self, slides: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.model.read(cx).selected_song().map(|s| s.title.clone()) else {
            return;
        };
        if slides.is_empty() {
            return;
        }
        let what = if slides.len() == 1 {
            "this slide".to_owned()
        } else {
            format!("these {} slides", slides.len())
        };
        let model = self.model.clone();
        confirm(
            ConfirmRequest::new(format!("Delete {what} from “{title}”?"), "Delete").destructive(),
            window,
            cx,
            move |ok, _, cx| {
                if ok {
                    model.update(cx, |m, cx| m.delete_slides(slides, cx));
                }
            },
        );
    }

    fn render_card(
        &self,
        card: &SlideCard,
        width: f32,
        passive_song: Option<&str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let zoom = m.zoom();
        let scale = zoom / 100.;
        let index = card.index;
        let passive = passive_song.is_some();
        let song_id = passive_song
            .map(str::to_owned)
            .or_else(|| m.selected_id().map(str::to_owned))
            .unwrap_or_default();
        let live = m.live_slide_of(&song_id) == Some(index);
        let picked = !passive && m.slide_picks().contains(&index);
        let pushing = !passive && m.pushing() == Some(index);
        let painting = !passive && m.paint().is_some();
        let busy = m.busy();
        let fill = section_color(rows::section_kind(card.kind));
        let slide_h = width * 9. / 16.;
        let bar_h = (18. * scale).round();
        let id: SharedString = match passive_song {
            Some(song) => ids::other_slide(song, index).into(),
            None => ids::slide(index).into(),
        };
        let border = if live {
            k.live
        } else if picked {
            k.accent(Step::S500)
        } else {
            k.white.opacity(0.12)
        };
        // The scripture card's style: the lyrics theme's background under a
        // scrim, a black label strip, square corners (owner, DECISIONS 113).
        let background = m.card_background().clone();
        let artwork = background.image.is_some() || background.color.is_some();
        let model = self.model.clone();
        let size = grid::text_size(card.lines.len(), zoom);
        let lines: Vec<AnyElement> = if card.lines.is_empty() {
            vec![
                div()
                    .italic()
                    .text_size(px(11.))
                    .text_color(k.white.opacity(0.2))
                    .child("Empty")
                    .into_any_element(),
            ]
        } else {
            card.lines
                .iter()
                .enumerate()
                .map(|(j, line)| {
                    let color = line
                        .color
                        .as_deref()
                        .and_then(line_color)
                        .unwrap_or(k.white.opacity(0.9));
                    let (section, source) = (card.section, line.source_line_index);
                    let paint = model.clone();
                    div()
                        .id(SharedString::from(ids::line(index, j)))
                        .test_support()
                        .w_full()
                        .text_center()
                        .line_clamp(2)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(size))
                        .text_color(color)
                        .child(line.text.clone())
                        .when(painting, |d| {
                            d.cursor_pointer()
                                .hover(|s| s.underline())
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    paint.update(cx, |m, cx| m.paint_line(section, source, cx))
                                })
                        })
                        .into_any_element()
                })
                .collect()
        };
        let click = model.clone();
        let click_song = song_id.clone();
        let label = card.label.clone();
        let mut el = v_flex()
            .id(id)
            .test_support()
            .role(Role::Button)
            .aria_label(SharedString::from(format!("Show slide {}", index + 1)))
            .relative()
            .w(px(width))
            .overflow_hidden()
            .map(|d| {
                if live || picked {
                    d.border_2()
                } else {
                    d.border(px(0.5))
                }
            })
            .border_color(border)
            .when(!live && !picked, |d| {
                d.hover(|s| s.border_color(k.white.opacity(0.3)))
            })
            .when(pushing, |d| d.opacity(0.7))
            .cursor_pointer()
            .on_click(move |event, _, cx| {
                let mods = event.modifiers();
                let gesture = if mods.shift {
                    Some(PickGesture::Range)
                } else if mods.platform || mods.control {
                    Some(PickGesture::Toggle)
                } else {
                    None
                };
                let song = click_song.clone();
                click.update(cx, |m, cx| {
                    if passive {
                        m.push_slide(song, index, cx)
                    } else {
                        m.click_slide(index, gesture, cx)
                    }
                })
            })
            .child(
                div()
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .h(px(slide_h))
                            .w_full()
                            .overflow_hidden()
                            .bg(gpui_kit::black())
                            .when_some(background.color, |d, rgb| {
                                d.child(
                                    div()
                                        .absolute()
                                        .size_full()
                                        .bg(gpui_kit::rgb(rgb))
                                        .opacity(background.opacity),
                                )
                            })
                            .when_some(background.image.clone(), |d, path| {
                                d.child(
                                    gpui_kit::img(std::path::PathBuf::from(path))
                                        .absolute()
                                        .size_full()
                                        .object_fit(gpui_kit::ObjectFit::Cover)
                                        .opacity(background.opacity),
                                )
                            })
                            .when(artwork, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .size_full()
                                        .bg(gpui_kit::black().opacity(0.4)),
                                )
                            })
                            .child(
                                v_flex()
                                    .absolute()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .gap_0p5()
                                    .px(px(14.))
                                    .overflow_hidden()
                                    .children(lines),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(bar_h.max(22.)))
                            .gap_2()
                            .px_2p5()
                            // The section's colour fills the strip so the
                            // sections read apart at a glance (owner).
                            .bg(fill)
                            .text_color(section_text())
                            .text_size(px((11.5 * scale).round()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(if card.first {
                                        label.clone()
                                    } else {
                                        String::new()
                                    }),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .opacity(0.7)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child((index + 1).to_string()),
                            ),
                    ),
            )
            .children(card.hotkey.clone().map(|key| {
                div()
                    .absolute()
                    .left(px(6.))
                    .top(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size_5()
                    .rounded(radius::sm())
                    .bg(k.hue(Hue::Orange))
                    .text_color(k.neutral(Step::S950))
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .child(key)
            }))
            .when(picked, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .top(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size_4()
                        .rounded_full()
                        .bg(k.accent(Step::S500))
                        .text_color(k.on_accent)
                        .child(Icon::new(KairoIcon::Check).size_2p5()),
                )
            });
        if passive || busy {
            return el.into_any_element();
        }
        // The open song's cards drag, drop and take the slide menu.
        let drag = SlideDrag {
            from: index,
            label: format!("Slide {}", index + 1).into(),
        };
        let half = |before: bool, cx: &mut Context<Self>| {
            let m = model.clone();
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w_1_2()
                .when(before, |d| d.left_0())
                .when(!before, |d| d.right_0())
                .drag_over::<SlideDrag>(move |s, _, _, _| {
                    if before {
                        s.border_l_2().border_color(fill)
                    } else {
                        s.border_r_2().border_color(fill)
                    }
                })
                .on_drop(cx.listener(move |_, drag: &SlideDrag, _, cx| {
                    let side = if before { Side::Before } else { Side::After };
                    let from = drag.from;
                    m.update(cx, |m, cx| m.move_slide(from, index, side, cx));
                    cx.stop_propagation();
                }))
        };
        if !painting {
            el = el
                .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .child(half(true, cx))
                .child(half(false, cx));
        }
        let menu_model = model.clone();
        let page = cx.entity();
        let section = card.section;
        if painting {
            return el.into_any_element();
        }
        el.context_menu(move |menu, window, cx| {
            slide_menu(menu, &menu_model, &page, index, section, window, cx)
        })
        .into_any_element()
    }

    fn render_quick_edit(
        &self,
        card: &SlideCard,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let scale = self.model.read(cx).zoom() / 100.;
        let model = self.model.clone();
        let quick = self.song.quick.clone();
        v_flex()
            .id(ids::QUICK_EDIT)
            .test_support()
            .relative()
            .w(px(width))
            .rounded(radius::md())
            .border_2()
            .border_color(k.white)
            .child(
                div()
                    .relative()
                    .h(px(width * 9. / 16.))
                    .bg(k.monitor())
                    .child(
                        Textarea::new(&self.song.quick)
                            .appearance(false)
                            .h(px(width * 9. / 16.))
                            .aria_label(format!("Edit slide {}", card.index + 1)),
                    )
                    .child(
                        div().absolute().right(px(6.)).top(px(6.)).child(
                            Button::new(ids::QUICK_EDIT_SAVE)
                                .cursor_pointer()
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(KairoIcon::X))
                                .accessibility_label("Close and save")
                                .on_click(move |_, _, cx| {
                                    let text = quick.read(cx).value().to_string();
                                    model.update(cx, |m, cx| m.save_quick_edit(text, cx))
                                }),
                        ),
                    ),
            )
            .child(
                h_flex()
                    .h(px((22. * scale).round()))
                    .gap_2()
                    .px_2()
                    .bg(section_color(rows::section_kind(card.kind)))
                    .text_color(section_text())
                    .text_size(px((11. * scale).round()))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .opacity(0.9)
                            .child("⌘↵ save · Esc cancel"),
                    )
                    .child((card.index + 1).to_string()),
            )
            .into_any_element()
    }

    fn render_grid(&self, song: &LyricsSong, passive: bool, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let (columns, card_w) = grid::layout(self.song.width.get(), m.zoom());
        let card_w = card_w.floor().max(1.);
        let items = grid::song_grid(song, m.gloss());
        let editing = (!passive).then(|| m.quick_edit().map(|(i, _)| i)).flatten();
        let passive_song = passive.then_some(song.id.as_str());
        let mut children: Vec<AnyElement> = items
            .iter()
            .map(|item| match item {
                GridItem::Card(card) if editing == Some(card.index) => {
                    self.render_quick_edit(card, card_w, cx)
                }
                GridItem::Card(card) => self.render_card(card, card_w, passive_song, cx),
                GridItem::EmptySection { kind, label, .. } => div()
                    .rounded(radius::sm())
                    .px_2()
                    .py_0p5()
                    .bg(section_color(rows::section_kind(*kind)))
                    .text_color(section_text())
                    .text_size(text::XS.size)
                    .child(label.clone())
                    .into_any_element(),
            })
            .collect();
        let _ = k;
        let mut rows = Vec::new();
        while !children.is_empty() {
            let take = children.len().min(columns);
            let row: Vec<AnyElement> = children.drain(..take).collect();
            rows.push(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap(px(grid::GAP))
                    .children(row),
            );
        }
        v_flex()
            .id(if passive {
                SharedString::from(format!("{}-{}", ids::GRID, song.id))
            } else {
                ids::GRID.into()
            })
            .test_support()
            .w_full()
            .gap(px(grid::GAP))
            .children(rows)
            .into_any_element()
    }

    fn render_song_band(
        &self,
        song: &LyricsSong,
        position: usize,
        current: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let count = grid::slide_count_label(grid::slide_count(song));
        let model = self.model.clone();
        let id = song.id.clone();
        h_flex()
            .id(SharedString::from(ids::divider(&song.id)))
            .test_support()
            .role(Role::Button)
            .aria_label(SharedString::from(song.title.clone()))
            .aria_selected(current)
            .min_h_9()
            .w_full()
            .gap_2p5()
            .px_4()
            .mx(px(-16.))
            .map(|d| {
                if current {
                    // Already the open song: pressing it changes nothing.
                    d.bg(k.surface_tertiary)
                        .text_color(k.text_primary)
                        .on_click(|_, _, _| {})
                } else {
                    d.bg(k.monitor().opacity(0.25))
                        .text_color(k.text(TextStep::T300))
                        .hover(|s| s.bg(k.surface_secondary))
                        .on_click(move |_, _, cx| {
                            model.update(cx, |m, cx| m.select_song(id.clone(), cx))
                        })
                }
            })
            .child(
                div()
                    .text_size(text::XS.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text(TextStep::T500))
                    .child(position.to_string()),
            )
            .child(
                Icon::new(KairoIcon::Music2.fill())
                    .size_3p5()
                    .text_color(if current {
                        k.hue(Hue::Violet)
                    } else {
                        k.text(TextStep::T500)
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(text::SM.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(song.title.clone()),
            )
            .when(!song.artist.is_empty(), |d| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child(song.artist.clone()),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .flex_none()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child(count),
            )
            .into_any_element()
    }

    fn render_actions(&self, song: &LyricsSong, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.clone();
        let busy = self.model.read(cx).busy();
        let has_keys = song.sections.iter().any(|s| s.hotkey.is_some());
        let text = grid::song_text(song);
        let id = song.id.clone();
        let gloss = self.model.read(cx).gloss().to_owned();
        let translating = self.model.read(cx).translating();
        let can_undo = self.model.read(cx).can_undo_translation();
        let translate_model = model.clone();
        let undo_model = model.clone();
        Button::new(ids::ACTIONS)
            .cursor_pointer()
            .ghost()
            .xsmall()
            .icon(Icon::new(KairoIcon::MoreHorizontal))
            .accessibility_label("Song actions")
            .popover_trigger()
            .dropdown_menu(move |menu, window, cx| {
                let (sel, auto, clear, paint, export) = (
                    model.clone(),
                    model.clone(),
                    model.clone(),
                    model.clone(),
                    model.clone(),
                );
                let (text, id, gloss) = (text.clone(), id.clone(), gloss.clone());
                let mut menu = menu
                    .item(
                        PopupMenuItem::new("Select slides").on_click(move |_, _, cx| {
                            sel.update(cx, |m, cx| m.start_selecting(cx))
                        }),
                    )
                    .separator()
                    .item(PopupMenuItem::new("Copy lyrics").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                    }))
                    .item(
                        PopupMenuItem::new("Assign hotkeys")
                            .disabled(busy)
                            .on_click(move |_, _, cx| {
                                auto.update(cx, |m, cx| m.set_hotkeys(true, cx))
                            }),
                    );
                if has_keys {
                    menu = menu.item(PopupMenuItem::new("Clear hotkeys").disabled(busy).on_click(
                        move |_, _, cx| clear.update(cx, |m, cx| m.set_hotkeys(false, cx)),
                    ));
                }
                let tm = translate_model.clone();
                menu = menu.submenu(
                    if translating {
                        "Translating…"
                    } else {
                        "Translate to English"
                    },
                    window,
                    cx,
                    move |mut menu, _, _| {
                        menu = menu.label("Sung in");
                        for (code, name) in crate::model::TRANSLATE_SOURCES {
                            let m = tm.clone();
                            menu =
                                menu.item(PopupMenuItem::new(name).disabled(translating).on_click(
                                    move |_, _, cx| m.update(cx, |m, cx| m.translate(code, cx)),
                                ));
                        }
                        menu
                    },
                );
                if can_undo {
                    let um = undo_model.clone();
                    menu = menu.item(
                        PopupMenuItem::new("Undo translation")
                            .disabled(translating)
                            .on_click(move |_, _, cx| {
                                um.update(cx, |m, cx| m.undo_translation(cx))
                            }),
                    );
                }
                menu.submenu("Paint line colour", window, cx, move |menu, _, _| {
                    let (a, b) = (paint.clone(), paint.clone());
                    let g = gloss.clone();
                    menu.item(
                        PopupMenuItem::new("Gloss colour").on_click(move |_, _, cx| {
                            a.update(cx, |m, cx| m.set_paint(Some(Some(g.clone())), cx))
                        }),
                    )
                    .item(
                        PopupMenuItem::new("Clear colour (automatic)").on_click(move |_, _, cx| {
                            b.update(cx, |m, cx| m.set_paint(Some(None), cx))
                        }),
                    )
                })
                .separator()
                .item(
                    PopupMenuItem::new("Export as Kairo file…")
                        .icon(Icon::new(KairoIcon::Upload))
                        .on_click(move |_, _, cx| {
                            export.update(cx, |m, cx| m.export_song(id.clone(), cx))
                        }),
                )
            })
            .into_any_element()
    }

    /// The right pane when a song is open.
    pub(super) fn render_song_view(&self, song: LyricsSong, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let playlist: Vec<LyricsSong> = m.playlist_songs().into_iter().cloned().collect();
        let list_name = m.viewing_setlist().map(|l| l.name.clone());
        let selecting = m.selecting();
        let picked = m.slide_picks().len();
        let painting = m.paint().is_some();
        let error = m.error().map(str::to_owned);
        let translate_error = m.translate_error().map(str::to_owned);
        let hint = grid::hint(painting, selecting, picked);
        let header: AnyElement = if let (false, Some(name)) = (playlist.is_empty(), list_name) {
            h_flex()
                .gap_2()
                .items_baseline()
                .child(
                    div()
                        .truncate()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.text_primary)
                        .child(name),
                )
                .child(
                    div()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child(playlist.len().to_string()),
                )
                .into_any_element()
        } else {
            let credits = [
                Some(if song.artist.is_empty() {
                    "Unknown artist".to_owned()
                } else {
                    song.artist.clone()
                }),
                song.ccli_number
                    .clone()
                    .filter(|c| !c.is_empty())
                    .map(|c| format!("CCLI #{c}")),
                song.copyright
                    .clone()
                    .filter(|c| !c.is_empty())
                    .map(|c| format!("© {c}")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(text::BASE.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.text_primary)
                        .child(song.title.clone()),
                )
                .child(
                    div()
                        .mt_0p5()
                        .truncate()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child(credits),
                )
                .into_any_element()
        };
        let done = (selecting).then(|| {
            let model = self.model.clone();
            Button::new(ids::DONE)
                .cursor_pointer()
                .small()
                .icon(Icon::new(KairoIcon::Check))
                .label("Done")
                .on_click(move |_, _, cx| model.update(cx, |m, cx| m.clear_slide_picks(cx)))
        });
        let blocks: Vec<AnyElement> = if playlist.is_empty() {
            vec![self.render_grid(&song, false, cx)]
        } else {
            playlist
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let current = s.id == song.id;
                    v_flex()
                        .when(i > 0, |d| d.pt_6())
                        .child(self.render_song_band(s, i + 1, current, cx))
                        .child(div().pt_3().child(self.render_grid(s, !current, cx)))
                        .into_any_element()
                })
                .collect()
        };
        let measured = self.song.width.clone();
        let view = cx.entity();
        v_flex()
            .id(ids::VIEW)
            .test_support()
            .track_focus(&self.song_focus)
            .on_key_down(cx.listener(Self::song_key))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    // A click inside the quick-edit box must not blur it:
                    // the blur saves and closes the edit.
                    if !this.song_focus.contains_focused(window, cx) {
                        this.song_focus.focus(window, cx);
                    }
                }),
            )
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                h_flex()
                    .flex_none()
                    .flex_wrap()
                    .justify_between()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .bg(k.surface)
                    .child(div().flex_1().min_w_0().child(header))
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_2()
                            .children(done)
                            .child(self.song.zoom.clone())
                            .child(self.render_actions(&song, cx))
                            .child({
                                let edit = self.model.clone();
                                Button::new(super::ids::EDIT)
                                    .cursor_pointer()
                                    .outline()
                                    .small()
                                    .icon(Icon::new(KairoIcon::Edit2))
                                    .label("Edit")
                                    .on_click(move |_, _, cx| edit.update(cx, |m, cx| m.edit(cx)))
                            }),
                    ),
            )
            .children(error.map(|e| {
                div()
                    .id(ids::SEND_ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .px_4()
                    .py_2()
                    .text_size(text::XS.size)
                    .text_color(k.danger_text)
                    .child(e)
            }))
            .children(translate_error.map(|e| {
                div()
                    .id(super::ids::TRANSLATE_ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .mx_4()
                    .mt_3()
                    .rounded(radius::lg())
                    .bg(k.tints.red)
                    .px_3p5()
                    .py_2p5()
                    .text_size(text::XS.size)
                    .text_color(k.danger_text)
                    .child(e)
            }))
            .child(
                div()
                    .id("lyrics-song-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_4()
                    .pb_4()
                    .when(playlist.is_empty(), |d| d.pt_4())
                    .child(
                        v_flex()
                            .relative()
                            .gap_4()
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        let w = f32::from(bounds.size.width);
                                        if (measured.get() - w).abs() > 0.5 {
                                            measured.set(w);
                                            // Refreshing mid-draw is ignored, and a cached view would not redraw:
                                            // notify the view once this frame ends.
                                            window.on_next_frame(move |_, cx| {
                                                view.update(cx, |_, cx| cx.notify())
                                            });
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                            .children(blocks)
                            .child(
                                div()
                                    .id(ids::HINT)
                                    .test_support()
                                    .pt_1()
                                    .text_center()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T600))
                                    .child(hint),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// Right-click on a slide (`SlideContextMenu`).
fn slide_menu(
    menu: PopupMenu,
    model: &Entity<Lyrics>,
    page: &Entity<LyricsWorkspace>,
    slide: usize,
    section: usize,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let m = model.read(cx);
    let targets = m.menu_slides(slide);
    let many = targets.len() > 1;
    let song = m.selected_song();
    let hotkey = song
        .and_then(|s| s.sections.get(section))
        .and_then(|s| s.hotkey.clone());
    let used: Vec<String> = song
        .map(|s| {
            s.sections
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != section)
                .filter_map(|(_, s)| s.hotkey.clone())
                .collect()
        })
        .unwrap_or_default();
    let song_id = m.selected_id().map(str::to_owned).unwrap_or_default();
    let k = *cx.kairo();
    let (live, quick, copy, select) = (model.clone(), model.clone(), model.clone(), model.clone());
    let edit = model.clone();
    let (group_model, key_model) = (model.clone(), model.clone());
    let delete_page = page.clone();
    let group_targets = targets.clone();
    let delete_targets = targets.clone();
    let mut menu = menu.item(PopupMenuItem::new("Go live").on_click(move |_, _, cx| {
        let song = song_id.clone();
        live.update(cx, |m, cx| m.push_slide(song, slide, cx))
    }));
    if !many {
        menu = menu.item(
            PopupMenuItem::new("Quick Edit")
                .on_click(move |_, _, cx| quick.update(cx, |m, cx| m.start_quick_edit(slide, cx))),
        );
    }
    menu.item(
        PopupMenuItem::new("Edit song…")
            .on_click(move |_, _, cx| edit.update(cx, |m, cx| m.edit(cx))),
    )
    .separator()
    .submenu(
        if many {
            format!("Group {} slides", targets.len())
        } else {
            "Group".into()
        },
        window,
        cx,
        move |mut menu, _, _| {
            for choice in grid::label_choices() {
                let (m, t, c) = (group_model.clone(), group_targets.clone(), choice.clone());
                let swatch = section_color(rows::section_kind(choice.kind));
                let label = choice.label.clone();
                menu = menu.item(
                    PopupMenuItem::element(move |_, _| {
                        h_flex()
                            .gap_2()
                            .child(div().size_3p5().rounded(radius::sm()).bg(swatch))
                            .child(label.clone())
                    })
                    .on_click(move |_, _, cx| {
                        m.update(cx, |m, cx| m.label_slides(t.clone(), c.clone(), cx))
                    }),
                );
            }
            menu
        },
    )
    .submenu(
        match &hotkey {
            Some(key) => format!("Hot Key ({key})"),
            None => "Hot Key".into(),
        },
        window,
        cx,
        move |mut menu, _, _| {
            menu = menu
                .label("Press the key in the song view to put this section live.")
                .scrollable(true)
                .max_h(px(320.));
            for key in grid::HOTKEYS.chars().map(String::from) {
                let m = key_model.clone();
                let elsewhere = used.contains(&key);
                let current = hotkey.as_deref() == Some(key.as_str());
                let shown = if current {
                    format!("{key}  ✓")
                } else if elsewhere {
                    format!("{key}  (moves here)")
                } else {
                    key.clone()
                };
                menu = menu.item(PopupMenuItem::new(shown).on_click(move |_, _, cx| {
                    m.update(cx, |m, cx| m.set_hotkey(section, Some(key.clone()), cx))
                }));
            }
            if hotkey.is_some() {
                let m = key_model.clone();
                menu = menu
                    .separator()
                    .item(
                        PopupMenuItem::new("Remove hot key").on_click(move |_, _, cx| {
                            m.update(cx, |m, cx| m.set_hotkey(section, None, cx))
                        }),
                    );
            }
            menu
        },
    )
    .separator()
    .item(PopupMenuItem::new("Copy text").on_click(move |_, _, cx| {
        if let Some(text) = copy.read(cx).slide_text(slide) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }))
    .item(PopupMenuItem::new("Select").on_click(move |_, _, cx| {
        select.update(cx, |m, cx| m.pick_slide(slide, PickGesture::Toggle, cx))
    }))
    .separator()
    .item(
        PopupMenuItem::element(move |_, _| {
            div().text_color(k.danger_text).child(if many {
                format!("Delete {} slides", delete_targets.len())
            } else {
                "Delete slide".into()
            })
        })
        .on_click(move |_, window, cx| {
            let t = targets.clone();
            delete_page.update(cx, |p, cx| p.delete_slides(t, window, cx))
        }),
    )
}
