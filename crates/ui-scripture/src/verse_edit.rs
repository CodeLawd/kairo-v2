//! Edit text for a playlist passage, laid out like the Props editor: the
//! passage's verses down the left, the chosen one on a full slide in the
//! middle. The verse and its reference line are typed straight onto the
//! slide, drawn as the theme draws them (font, size fitted to the words,
//! alignment), so what is typed is what goes on screen.
//!
//! A long verse can be split at the caret into parts that keep the verse's
//! name; each part is its own slide. Save writes every change at once.

use std::rc::Rc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, AppContext, Context, Entity, EventEmitter, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, div, px, size,
};
use kairo_domain::scripture::ScriptureVerse;
use kairo_domain::scripture::query::{VERSE_LABEL_KEY, verse_reference, verse_reference_line};
use kairo_ui_kit::components::modal::{MovableDialog, MovableDialogEvent};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::card::{VerseCard, verse_text_style};
use crate::model::Scripture;
use crate::slide_text::{DragAnchor, OnSelect, SlideLayout, move_vertical, slide_text};

/// Element ids, for tests.
pub mod ids {
    pub const DIALOG: &str = "scripture-verse-edit";
    pub const TEXT: &str = "scripture-verse-edit-text";
    pub const REFERENCE: &str = "scripture-verse-edit-reference";
    pub const SAVE: &str = "scripture-verse-edit-save";
    pub const CANCEL: &str = "scripture-verse-edit-cancel";
    pub const RESET: &str = "scripture-verse-edit-reset";
    pub const SPLIT: &str = "scripture-verse-edit-split";
    pub const JOIN: &str = "scripture-verse-edit-join";

    pub fn verse(index: usize) -> String {
        format!("scripture-verse-edit-verse-{index}")
    }
}

/// The window, as the Props editor's.
const WINDOW: (f32, f32) = (1100.0, 720.0);
const RAIL_W: f32 = 240.0;
/// Header and the tool row under the slide.
const CHROME_H: f32 = 150.0;
const STAGE_PAD: f32 = 24.0;

/// The dialog closed (saved or not).
pub struct VerseEditDone;

/// One slide's worth of a verse: the whole verse, or one part of a split.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    verse: ScriptureVerse,
    /// The words on the slide.
    pub text: String,
    /// The whole reference line as shown, translation and all.
    pub label: String,
    /// The line it would have without a name of its own.
    standard: String,
}

impl Part {
    fn of(verse: &ScriptureVerse, translation: &str, show_translation: bool) -> Self {
        let reference = verse_reference(&verse.book, verse.chapter, verse.verse);
        Self {
            text: verse.text.clone(),
            label: verse_reference_line(Some(verse), &reference, translation, show_translation),
            standard: verse_reference_line(None, &reference, translation, show_translation),
            verse: verse.clone(),
        }
    }

    fn same_verse(&self, other: &Part) -> bool {
        self.verse.book == other.verse.book
            && self.verse.chapter == other.verse.chapter
            && self.verse.verse == other.verse.verse
    }

    /// Back to a stored verse: the line is kept only when it differs from
    /// the standard one; a cleared line goes back to the standard one.
    fn into_verse(self) -> ScriptureVerse {
        let mut verse = self.verse;
        verse.text = self.text.trim().to_owned();
        let label = self.label.trim();
        if label == self.standard {
            verse.extra.remove(VERSE_LABEL_KEY);
        } else {
            verse
                .extra
                .insert(VERSE_LABEL_KEY.into(), serde_json::Value::from(label));
        }
        verse
    }
}

/// Splits `text` at byte `at` into two non-empty parts.
pub fn split_text(text: &str, at: usize) -> Option<(String, String)> {
    let at = at.min(text.len());
    if !text.is_char_boundary(at) {
        return None;
    }
    let (left, right) = (text[..at].trim(), text[at..].trim());
    (!left.is_empty() && !right.is_empty()).then(|| (left.to_owned(), right.to_owned()))
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Verse,
    Reference,
}

pub struct VerseEditor {
    model: Entity<Scripture>,
    item_id: String,
    title: String,
    initial: Vec<Part>,
    parts: Vec<Part>,
    selected: usize,
    frame: Entity<MovableDialog>,
    /// The fields the keys go to; the slide draws what they hold.
    verse_input: Entity<TextareaState>,
    reference_input: Entity<InputState>,
    /// The drawn verse's and reference's layouts, for ↑ ↓.
    verse_layout: SlideLayout,
    reference_layout: SlideLayout,
    verse_anchor: DragAnchor,
    reference_anchor: DragAnchor,
    /// The caret blinks: shown half the time, and on every change.
    caret_on: bool,
    _blink: Task<()>,
    _subscriptions: Vec<Subscription>,
}

/// One blink, as macOS text fields blink.
const BLINK: std::time::Duration = std::time::Duration::from_millis(530);

impl EventEmitter<VerseEditDone> for VerseEditor {}

impl VerseEditor {
    /// Playlist item `item_id` (see [`Scripture::editable_item`]), open on
    /// verse `selected`.
    pub fn new(
        model: Entity<Scripture>,
        item_id: String,
        selected: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let item = model.read(cx).editable_item(&item_id).cloned();
        let (title, translation, verses) = item.map_or_else(
            || (String::new(), String::new(), Vec::new()),
            |i| (i.reference, i.translation, i.verses),
        );
        let show = model.read(cx).options().show_translation;
        let parts: Vec<Part> = verses
            .iter()
            .map(|v| Part::of(v, &translation, show))
            .collect();
        let selected = selected.min(parts.len().saturating_sub(1));
        let viewport = window.viewport_size();
        let frame = cx.new(|cx| {
            MovableDialog::new(
                "scripture-verse-edit-frame",
                format!("Edit {title}"),
                size(
                    px(WINDOW.0.min(f32::from(viewport.width) - 32.0)),
                    px(WINDOW.1.min(f32::from(viewport.height) - 32.0)),
                ),
                window,
                cx,
            )
        });
        let verse_input = cx.new(|cx| TextareaState::new(window, cx));
        let reference_input = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe(&frame, |this, _, _: &MovableDialogEvent, cx| this.close(cx)),
            // A moved caret or a new selection (arrows, ⌘A) shows at once.
            cx.observe(&verse_input, |this, _, cx| {
                this.caret_on = true;
                cx.notify();
            }),
            cx.observe(&reference_input, |this, _, cx| {
                this.caret_on = true;
                cx.notify();
            }),
            cx.subscribe(&verse_input, |this, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let value = input.read(cx).value().to_string();
                    if let Some(part) = this.parts.get_mut(this.selected) {
                        part.text = value;
                    }
                    this.caret_on = true;
                    cx.notify();
                }
            }),
            cx.subscribe(&reference_input, |this, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let value = input.read(cx).value().to_string();
                    if let Some(part) = this.parts.get_mut(this.selected) {
                        part.label = value;
                    }
                    this.caret_on = true;
                    cx.notify();
                }
            }),
        ];
        // Before the fields' own bindings: ↑ ↓ follow the slide's rows.
        let weak = cx.entity().downgrade();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let k = &event.keystroke;
            let m = &k.modifiers;
            let command = m.platform || m.control;
            let handled = weak
                .update(cx, |this, cx| {
                    if !this.typing(window, cx) {
                        return false;
                    }
                    match k.key.as_str() {
                        // ⌥↵ (as in the lyrics editor) or ⌘⇧↵ splits at the
                        // caret; ⌘↵ saves.
                        "enter" if m.alt || (command && m.shift) => {
                            this.split(window, cx);
                            true
                        }
                        "enter" if command => {
                            this.save(cx);
                            true
                        }
                        "up" | "down" if !command && !m.alt => {
                            this.vertical(k.key == "up", m.shift, window, cx)
                        }
                        _ => false,
                    }
                })
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });
        let blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let alive = this.update(cx, |this, cx| {
                    this.caret_on = !this.caret_on;
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        let mut this = Self {
            model,
            item_id,
            title,
            initial: parts.clone(),
            parts,
            selected,
            frame,
            verse_input,
            reference_input,
            verse_layout: Default::default(),
            reference_layout: Default::default(),
            verse_anchor: Default::default(),
            reference_anchor: Default::default(),
            caret_on: true,
            _blink: blink,
            _subscriptions: {
                let mut subscriptions = subscriptions;
                subscriptions.push(keys);
                subscriptions
            },
        };
        this.load(window, cx);
        this
    }

    /// The part on the slide.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The parts as edited so far.
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// The field the verse's keys go to.
    pub fn verse_field(&self) -> &Entity<TextareaState> {
        &self.verse_input
    }

    /// The field the reference line's keys go to.
    pub fn reference_field(&self) -> &Entity<InputState> {
        &self.reference_input
    }

    /// Puts the selected part into the fields, caret at the end of the verse.
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(part) = self.parts.get(self.selected).cloned() else {
            return;
        };
        let label_end = part.label.len();
        self.reference_input.update(cx, |s, cx| {
            s.set_value(part.label, window, cx);
            s.set_selected_range(label_end..label_end, cx);
        });
        let end = part.text.len();
        self.verse_input.update(cx, |s, cx| {
            s.set_value(part.text, window, cx);
            s.set_selected_range(end..end, cx);
            s.focus(window, cx);
        });
    }

    pub fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.parts.len() || index == self.selected {
            return;
        }
        self.selected = index;
        self.load(window, cx);
        cx.notify();
    }

    /// A click or drag on the slide: that field takes the keys, with the
    /// caret or selection where the mouse put it.
    fn pick(
        &mut self,
        field: Field,
        range: std::ops::Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match field {
            Field::Verse => self.verse_input.update(cx, |s, cx| {
                s.set_selected_range(range, cx);
                s.focus(window, cx);
            }),
            Field::Reference => self.reference_input.update(cx, |s, cx| {
                s.set_selected_range(range, cx);
                s.focus(window, cx);
            }),
        }
        self.caret_on = true;
        cx.notify();
    }

    /// Whether one of the slide's fields has the keys.
    fn typing(&self, window: &Window, cx: &App) -> bool {
        self.verse_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
            || self
                .reference_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    }

    /// ↑ ↓ (with ⇧ to select) move between the rows as the slide draws
    /// them, not as the unseen field wraps. `false` when the key is not
    /// theirs (no field focused, or already on the first or last row).
    fn vertical(
        &mut self,
        up: bool,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let options = self.model.read(cx).options().clone();
        let verse_focused = self
            .verse_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let reference_focused = self
            .reference_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let Some(part) = self.parts.get(self.selected) else {
            return false;
        };
        let style = verse_text_style(&options.background, &part.label, &part.text, 16.0, 9.0);
        let (field, layout, align, selection, len) = if verse_focused {
            let s = self.verse_input.read(cx);
            (
                Field::Verse,
                self.verse_layout.clone(),
                style.align,
                s.selected_range(),
                part.text.len(),
            )
        } else if reference_focused {
            let s = self.reference_input.read(cx);
            (
                Field::Reference,
                self.reference_layout.clone(),
                style.reference_align,
                s.selected_range(),
                part.label.len(),
            )
        } else {
            return false;
        };
        let to = move_vertical(&layout, selection.end, align, up)
            .unwrap_or(if up { 0 } else { len })
            .min(len);
        let range = if shift {
            selection.start.min(to)..selection.start.max(to)
        } else {
            to..to
        };
        self.pick(field, range, window, cx);
        true
    }

    /// Splits the verse at the caret: the words after it become the next
    /// slide, under the same name.
    pub fn split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.verse_input.read(cx).cursor();
        let Some(part) = self.parts.get(self.selected).cloned() else {
            return;
        };
        let Some((left, right)) = split_text(&part.text, at) else {
            return;
        };
        self.parts[self.selected].text = left;
        self.parts.insert(
            self.selected + 1,
            Part {
                text: right,
                ..part
            },
        );
        self.selected += 1;
        self.load(window, cx);
        cx.notify();
    }

    /// Whether the next part is more of the same verse.
    fn can_join(&self) -> bool {
        self.parts
            .get(self.selected)
            .zip(self.parts.get(self.selected + 1))
            .is_some_and(|(a, b)| a.same_verse(b))
    }

    /// Joins the selected part with the next part of the same verse.
    pub fn join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_join() {
            return;
        }
        let next = self.parts.remove(self.selected + 1);
        let part = &mut self.parts[self.selected];
        part.text = format!("{} {}", part.text.trim_end(), next.text.trim_start());
        self.load(window, cx);
        cx.notify();
    }

    fn edited(&self, index: usize) -> bool {
        self.parts
            .get(index)
            .is_some_and(|p| !self.initial.contains(p))
    }

    /// Puts the selected verse back as it was when the editor opened.
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.parts.get(self.selected).cloned() else {
            return;
        };
        let first = self
            .parts
            .iter()
            .position(|p| p.same_verse(&current))
            .unwrap_or(self.selected);
        self.parts.retain(|p| !p.same_verse(&current));
        let originals: Vec<Part> = self
            .initial
            .iter()
            .filter(|p| p.same_verse(&current))
            .cloned()
            .collect();
        let at = first.min(self.parts.len());
        self.parts.splice(at..at, originals);
        self.selected = at.min(self.parts.len().saturating_sub(1));
        self.load(window, cx);
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        let id = self.item_id.clone();
        let verses: Vec<ScriptureVerse> =
            self.parts.iter().cloned().map(Part::into_verse).collect();
        self.model
            .update(cx, |m, cx| m.save_item_verses(&id, verses, cx));
        self.close(cx);
    }

    /// Gives focus back to what had it before the dialog opened.
    pub fn restore_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.frame.update(cx, |f, cx| f.restore(window, cx));
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(VerseEditDone);
    }

    /// "2 of 3" when the part's verse is split.
    fn part_of(&self, index: usize) -> Option<(usize, usize)> {
        let part = self.parts.get(index)?;
        let group: Vec<usize> = (0..self.parts.len())
            .filter(|&i| self.parts[i].same_verse(part))
            .collect();
        (group.len() > 1).then(|| {
            (
                group.iter().position(|&i| i == index).unwrap_or(0) + 1,
                group.len(),
            )
        })
    }

    /// Whether the selected verse differs from how it opened.
    fn verse_edited(&self) -> bool {
        let Some(current) = self.parts.get(self.selected) else {
            return false;
        };
        let now: Vec<&Part> = self
            .parts
            .iter()
            .filter(|p| p.same_verse(current))
            .collect();
        let was: Vec<&Part> = self
            .initial
            .iter()
            .filter(|p| p.same_verse(current))
            .collect();
        now != was
    }

    fn render_rail(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let k = *cx.kairo();
        let rows: Vec<_> = self
            .parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                let selected = i == self.selected;
                let edited = self.edited(i);
                let number = kairo_domain::js::number_to_string(part.verse.verse);
                let piece = self.part_of(i).map(|(n, of)| format!("{n}/{of}"));
                h_flex()
                    .id(SharedString::from(ids::verse(i)))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(match &piece {
                        Some(p) => format!("{}, part {p}", part.label),
                        None => part.label.clone(),
                    })
                    .w_full()
                    .items_start()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(radius::lg())
                    .cursor_pointer()
                    .when(selected, |d| d.bg(k.accent[5]).text_color(k.on_accent))
                    .when(!selected, |d| d.hover(|s| s.bg(k.surface_tertiary)))
                    .on_click(cx.listener(move |this, _, window, cx| this.select(i, window, cx)))
                    .child(
                        v_flex()
                            .flex_none()
                            .w_7()
                            .child(
                                div()
                                    .text_size(text::LABEL.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(number),
                            )
                            .children(
                                piece.map(|p| {
                                    div().text_size(text::MICRO.size).opacity(0.75).child(p)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(text::XS.size)
                            .line_clamp(2)
                            .when(!selected, |d| d.text_color(k.text(TextStep::T300)))
                            .child(part.text.clone()),
                    )
                    .when(edited, |d| {
                        d.child(
                            div()
                                .flex_none()
                                .mt_1()
                                .size_1p5()
                                .rounded_full()
                                .bg(if selected { k.on_accent } else { k.accent[5] }),
                        )
                    })
            })
            .collect();
        v_flex()
            .flex_none()
            .w(px(RAIL_W))
            .h_full()
            .min_h_0()
            .bg(k.surface_secondary)
            .border_r_1()
            .border_color(k.surface_line)
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .text_size(text::LABEL.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text(TextStep::T400))
                    .child(format!("SLIDES · {}", self.parts.len())),
            )
            .child(
                v_flex()
                    .id("scripture-verse-edit-rail")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .px_2p5()
                    .pb_2()
                    .children(rows),
            )
            .into_any_element()
    }
}

impl Render for VerseEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let options = self.model.read(cx).options().clone();
        let background = options.background.clone();
        let Some(part) = self.parts.get(self.selected).cloned() else {
            return self.frame.update(cx, |f, cx| f.frame(div(), window, cx));
        };
        let full_reference = part.label.clone();
        // The slide fills the space beside the rail at 16:9.
        let frame_size = self.frame.read(cx).frame_bounds(window).size;
        let room_w = (f32::from(frame_size.width) - RAIL_W - STAGE_PAD * 2.0).max(160.0);
        let room_h = (f32::from(frame_size.height) - CHROME_H - STAGE_PAD * 2.0).max(90.0);
        let stage_w = room_w.min(room_h * 16.0 / 9.0);
        let stage_h = stage_w * 9.0 / 16.0;
        let style = verse_text_style(&background, &full_reference, &part.text, stage_w, stage_h);

        let verse_field = self.verse_input.read(cx);
        let verse_focused = verse_field.focus_handle(cx).is_focused(window);
        let verse_selection = verse_field.selected_range();
        let cursor = verse_field.cursor();
        let reference_field = self.reference_input.read(cx);
        let reference_focused = reference_field.focus_handle(cx).is_focused(window);
        let reference_selection = reference_field.selected_range();
        let this = cx.entity();
        let pick = |field: Field| -> OnSelect {
            let this = this.clone();
            Rc::new(move |range, window, cx| {
                this.update(cx, |t, cx| t.pick(field, range, window, cx))
            })
        };
        let caret = k.accent[5];
        let verse = div()
            .id(ids::TEXT)
            .test_support()
            .w_full()
            .child(slide_text(
                "scripture-verse-edit-verse-text",
                &part.text,
                "",
                verse_selection,
                verse_focused,
                self.caret_on,
                style.align,
                caret,
                self.verse_layout.clone(),
                self.verse_anchor.clone(),
                pick(Field::Verse),
            ));
        let reference = div()
            .id(ids::REFERENCE)
            .test_support()
            .w_full()
            .child(slide_text(
                "scripture-verse-edit-reference-text",
                &part.label,
                "",
                reference_selection,
                reference_focused,
                self.caret_on,
                style.reference_align,
                caret,
                self.reference_layout.clone(),
                self.reference_anchor.clone(),
                pick(Field::Reference),
            ));
        let slide = VerseCard::new("scripture-verse-edit-slide", part.label.clone())
            .slide(full_reference.clone(), None)
            .size(stage_w, stage_h)
            .background(background)
            .bare(true)
            .edit_layers(verse, reference, part.text.clone());
        // The fields that take the keys sit under the slide, unseen; the
        // verse field wraps at the slide's width so ↑ ↓ follow its rows.
        let fields = div()
            .absolute()
            .top_0()
            .left_0()
            .opacity(0.)
            .child(
                div().w(style.wrap_width).child(
                    Textarea::new(&self.verse_input)
                        .appearance(false)
                        .w(style.wrap_width)
                        .h(px(stage_h * 0.9))
                        .text_size(style.size)
                        .line_height(style.line_height)
                        .font_weight(style.weight)
                        .when_some(style.family.clone(), |t, f| t.font_family(f))
                        .aria_label(format!("Text of {}", part.label)),
                ),
            )
            .child(
                div().w(px(240.)).child(
                    Input::new(&self.reference_input)
                        .appearance(false)
                        .aria_label("Reference line"),
                ),
            );

        let verse_edited = self.verse_edited();
        let any_edit = self.parts != self.initial;
        let can_split = split_text(&part.text, cursor).is_some();
        let can_join = self.can_join();
        let where_ = match self.part_of(self.selected) {
            Some((n, of)) => format!("{} · part {n} of {of}", part.label),
            None => part.label.clone(),
        };
        let header = h_flex()
            .flex_none()
            .justify_between()
            .gap_3()
            .px_6()
            .py_4()
            .border_b_1()
            .border_color(k.surface_line)
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(text::LG.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child(format!("Edit {}", self.title)),
                    )
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child("Click the words or the reference on the slide to change them. This playlist only, not the Bible."),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(
                        Button::new(ids::CANCEL)
                            .cursor_pointer()
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(
                        Button::new(ids::SAVE)
                            .cursor_pointer()
                            .primary()
                            .small()
                            .label("Save")
                            .disabled(!any_edit)
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            );
        let tools = h_flex()
            .w(px(stage_w))
            .justify_between()
            .gap_2()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child(where_),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .child(
                        Button::new(ids::SPLIT)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::Scissors))
                            .label("Split at cursor")
                            .tooltip("Move the words after the cursor to a new slide (⌥↵)")
                            .disabled(!can_split)
                            .on_click(cx.listener(|this, _, window, cx| this.split(window, cx))),
                    )
                    .child(
                        Button::new(ids::JOIN)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .label("Join with next")
                            .disabled(!can_join)
                            .on_click(cx.listener(|this, _, window, cx| this.join(window, cx))),
                    )
                    .child(
                        Button::new(ids::RESET)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .label("Undo changes")
                            .tooltip("Put this verse back as it was")
                            .disabled(!verse_edited)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    ),
            );
        let stage = v_flex()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p(px(STAGE_PAD))
            .bg(k.surface)
            .child(
                // The unseen fields sit inside the slide's bounds, and the
                // slide occludes them: every click is the slide's.
                div().relative().child(fields).child(
                    div()
                        .id("scripture-verse-edit-stage")
                        .occlude()
                        .child(slide),
                ),
            )
            .child(tools);
        let rail = self.render_rail(cx);
        let content = v_flex()
            .id(ids::DIALOG)
            .test_support()
            .size_full()
            .child(header)
            .child(h_flex().flex_1().min_h_0().child(rail).child(stage));
        self.frame
            .update(cx, |frame, cx| frame.frame(content, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::scripture::query::custom_reference_line;

    #[test]
    fn a_split_needs_words_on_both_sides() {
        assert_eq!(
            split_text("one two three", 7),
            Some(("one two".into(), "three".into()))
        );
        assert_eq!(split_text("one two", 0), None);
        assert_eq!(split_text("one two", 7), None);
        assert_eq!(split_text("é", 1), None, "inside a character");
    }

    #[test]
    fn a_name_is_kept_only_when_it_differs() {
        let verse: ScriptureVerse = serde_json::from_value(serde_json::json!({
            "book": "Joshua", "chapter": 1.0, "verse": 4.0, "text": "From"
        }))
        .unwrap();
        let mut part = Part::of(&verse, "NKJV", true);
        assert_eq!(part.label, "Joshua 1:4 (NKJV)");
        assert!(
            part.clone()
                .into_verse()
                .extra
                .get(VERSE_LABEL_KEY)
                .is_none()
        );
        part.label = "Joshua 1:4a — New King James".into();
        let saved = part.into_verse();
        assert_eq!(
            custom_reference_line(&saved),
            Some("Joshua 1:4a — New King James")
        );
    }
}
