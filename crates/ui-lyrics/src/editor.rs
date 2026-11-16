//! The song editor (`SongReflowEditor.tsx` and the edit header in
//! `Lyrics.tsx`): the title and credits, then each section as a coloured
//! gutter with its slides as numbered text boxes. Enter is a new line; ⌥↵
//! (or ⌘↵) breaks into a new slide; Backspace at the start of a slide joins
//! it to the one above; ⌘Z / ⇧⌘Z undo and redo the editor's own history;
//! pasting a song with blank lines or `[Chorus]` lines keeps its shape.
//! Misspelt words get the system spell checker's wavy underline.

use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{
    Editor, EditorState, Input, InputEvent, InputState, TextDecoration, TextDecorationCollection,
};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, Focusable, HighlightStyle,
    InteractiveElement, IntoElement, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, UnderlineStyle, Window, div, px,
};
use kairo_domain::lyrics::LyricsSectionType;
use kairo_ui_kit::tokens::{TextStep, radius, section_color, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::LyricsBackend;
use crate::model::{Field, Lyrics};
use crate::reflow_edit::{Caret, ReflowDoc, SECTION_TYPES};
use crate::rows;

/// Element ids, for tests.
pub mod ids {
    pub const EDITOR: &str = "lyrics-editor";
    pub const TITLE: &str = "lyrics-editor-title";
    pub const ADD_SECTION: &str = "lyrics-editor-add-section";
    pub const ERROR: &str = "lyrics-editor-error";
    pub const RENAME: &str = "lyrics-editor-rename";

    pub fn slide(key: u64) -> String {
        format!("lyrics-editor-slide-{key}")
    }
    pub fn gutter(index: usize) -> String {
        format!("lyrics-editor-gutter-{index}")
    }
    pub fn slide_menu(key: u64) -> String {
        format!("lyrics-editor-slide-menu-{key}")
    }
}

/// Spell checking waits this long after typing stops.
pub const SPELL_AFTER: Duration = Duration::from_millis(300);

/// What the editor tells the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    Save,
}

struct SlideField {
    state: Entity<EditorState>,
    marks: Option<TextDecorationCollection>,
    misspelled: Vec<std::ops::Range<usize>>,
    _subscription: Subscription,
    _spell: Option<Task<()>>,
}

pub struct SongEditor {
    model: Entity<Lyrics>,
    backend: Arc<dyn LyricsBackend>,
    doc: ReflowDoc,
    fields: HashMap<u64, SlideField>,
    title: Entity<InputState>,
    artist: Entity<InputState>,
    copyright: Entity<InputState>,
    ccli: Entity<InputState>,
    rename: Entity<InputState>,
    renaming: Option<usize>,
    started: Instant,
    /// A slide to focus once its field exists.
    focus_next: Option<Caret>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EditorEvent> for SongEditor {}

impl SongEditor {
    pub fn new(model: Entity<Lyrics>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (fields, new, backend) = {
            let m = model.read(cx);
            let e = m.editing().expect("editing");
            (e.fields.clone(), e.new, m.backend().clone())
        };
        let input =
            |value: &str, placeholder: &str, window: &mut Window, cx: &mut Context<Self>| {
                let (value, placeholder) = (value.to_owned(), placeholder.to_owned());
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(placeholder)
                        .default_value(value)
                })
            };
        let title = input(&fields.title, "Song title", window, cx);
        let artist = input(&fields.artist, "Artist", window, cx);
        let copyright = input(&fields.copyright, "© Copyright", window, cx);
        let ccli = input(&fields.ccli_number, "CCLI #", window, cx);
        let rename = cx.new(|cx| InputState::new(window, cx));
        let mut subs = Vec::new();
        for (state, field) in [
            (&title, Field::Title),
            (&artist, Field::Artist),
            (&copyright, Field::Copyright),
            (&ccli, Field::Ccli),
        ] {
            subs.push(
                cx.subscribe(state, move |this, state, event: &InputEvent, cx| {
                    if let InputEvent::Change = event {
                        let value = state.read(cx).value().to_string();
                        this.model
                            .update(cx, |m, cx| m.set_edit_field(field, value, cx));
                    }
                }),
            );
        }
        subs.push(
            cx.subscribe(&rename, |this, state, event: &InputEvent, cx| match event {
                InputEvent::Change => {
                    if let Some(b) = this.renaming {
                        let label = state.read(cx).value().to_string();
                        let now = this.now();
                        this.doc.rename(b, &label, now);
                        this.emit_sections(cx);
                    }
                }
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    this.renaming = None;
                    cx.notify();
                }
                _ => {}
            }),
        );
        // Keys inside the slide fields go before the editor's own bindings.
        let weak = cx.entity().downgrade();
        subs.push(cx.intercept_keystrokes(move |event, window, cx| {
            let k = &event.keystroke;
            let handled = weak
                .update(cx, |this, cx| this.key(&k.key, k.modifiers, window, cx))
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        }));
        let doc = ReflowDoc::new(&fields.sections);
        let mut this = Self {
            model,
            backend,
            doc,
            fields: HashMap::new(),
            title,
            artist,
            copyright,
            ccli,
            rename,
            renaming: None,
            started: Instant::now(),
            focus_next: None,
            _subscriptions: subs,
        };
        this.sync_fields(window, cx);
        if new {
            this.title.update(cx, |s, cx| s.focus(window, cx));
        } else if let Some(first) = this.doc.first_slide() {
            this.focus_next = Some(Caret {
                slide: first,
                at: 0,
            });
            this.apply_focus(window, cx);
        }
        this
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn doc(&self) -> &ReflowDoc {
        &self.doc
    }

    /// The field holding `slide`, for tests.
    pub fn slide_state(&self, slide: u64) -> Option<&Entity<EditorState>> {
        self.fields.get(&slide).map(|f| &f.state)
    }

    /// Misspelt ranges currently underlined in `slide`.
    pub fn misspelled(&self, slide: u64) -> &[std::ops::Range<usize>] {
        self.fields
            .get(&slide)
            .map_or(&[], |f| f.misspelled.as_slice())
    }

    fn emit_sections(&mut self, cx: &mut Context<Self>) {
        let sections = self.doc.sections();
        self.model
            .update(cx, |m, cx| m.set_edit_sections(sections, cx));
        cx.notify();
    }

    /// The slide whose field has focus, and its caret.
    fn focused(&self, window: &Window, cx: &gpui_kit::App) -> Option<Caret> {
        self.fields.iter().find_map(|(key, f)| {
            let state = f.state.read(cx);
            state.focus_handle(cx).is_focused(window).then(|| Caret {
                slide: *key,
                at: state.cursor(),
            })
        })
    }

    /// One field per slide, its text in step with the document.
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted: Vec<(u64, String)> = self
            .doc
            .blocks()
            .iter()
            .flat_map(|b| b.slides.iter().map(|s| (s.key, s.text.clone())))
            .collect();
        self.fields
            .retain(|k, _| wanted.iter().any(|(w, _)| w == k));
        for (key, text) in wanted {
            match self.fields.get(&key) {
                Some(f) => {
                    if f.state.read(cx).value().as_ref() != text {
                        let t = text.clone();
                        f.state.update(cx, |s, cx| s.set_value(t, window, cx));
                        self.spell(key, cx);
                    }
                }
                None => {
                    let state = cx.new(|cx| {
                        EditorState::new(window, cx)
                            .line_number(false)
                            .folding(false)
                            .searchable(false)
                            .soft_wrap(true)
                            .auto_close(false)
                            .smart_indent(false)
                            .placeholder("Type the words for this slide")
                            .default_value(text)
                    });
                    let sub = cx.subscribe(&state, move |this, state, event: &InputEvent, cx| {
                        if let InputEvent::Change = event {
                            let (value, at) = {
                                let s = state.read(cx);
                                (s.value().to_string(), s.cursor())
                            };
                            let now = this.now();
                            this.doc
                                .edit(key, value, Some(Caret { slide: key, at }), now);
                            this.emit_sections(cx);
                            this.spell(key, cx);
                        }
                    });
                    self.fields.insert(
                        key,
                        SlideField {
                            state,
                            marks: None,
                            misspelled: Vec::new(),
                            _subscription: sub,
                            _spell: None,
                        },
                    );
                    self.spell(key, cx);
                }
            }
        }
    }

    /// Re-checks a slide's spelling after a pause, off the UI thread (the
    /// system checker can take seconds on its first call).
    fn spell(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(field) = self.fields.get_mut(&key) else {
            return;
        };
        let backend = self.backend.clone();
        let state = field.state.clone();
        field._spell = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SPELL_AFTER).await;
            let text = state.read_with(cx, |s, _| s.value().to_string());
            let checked = text.clone();
            let ranges = cx
                .background_executor()
                .spawn(async move { backend.misspelled(&checked) })
                .await;
            let _ = this.update(cx, |this, cx| this.mark_spelling(key, &text, ranges, cx));
        }));
    }

    fn mark_spelling(
        &mut self,
        key: u64,
        checked: &str,
        ranges: Vec<std::ops::Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let danger = cx.kairo().danger_text;
        let Some(field) = self.fields.get_mut(&key) else {
            return;
        };
        // Typed on since: a newer check follows.
        if field.state.read(cx).value().as_ref() != checked {
            return;
        }
        let text = checked;
        let ranges: Vec<_> = ranges
            .into_iter()
            .filter(|r| {
                r.end <= text.len()
                    && text.is_char_boundary(r.start)
                    && text.is_char_boundary(r.end)
            })
            .collect();
        let decorations: Vec<TextDecoration> = ranges
            .iter()
            .map(|r| {
                TextDecoration::new(
                    r.clone(),
                    HighlightStyle {
                        underline: Some(UnderlineStyle {
                            color: Some(danger),
                            thickness: px(1.),
                            wavy: true,
                        }),
                        ..Default::default()
                    },
                )
            })
            .collect();
        field.misspelled = ranges;
        match &field.marks {
            Some(marks) => marks.set(decorations, cx),
            None => {
                let marks = field
                    .state
                    .update(cx, |s, cx| s.create_decorations_collection(decorations, cx));
                field.marks = Some(marks);
            }
        }
        cx.notify();
    }

    fn apply_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(caret) = self.focus_next.take() else {
            return;
        };
        let Some(field) = self.fields.get(&caret.slide) else {
            return;
        };
        field.state.update(cx, |s, cx| {
            let at = caret.at.min(s.value().len());
            s.set_selected_range(at..at, cx);
            s.focus(window, cx);
        });
    }

    /// After a structural change: fields, then the caret.
    fn changed(&mut self, caret: Option<Caret>, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_fields(window, cx);
        self.emit_sections(cx);
        if caret.is_some() {
            self.focus_next = caret;
            self.apply_focus(window, cx);
        }
    }

    fn key(
        &mut self,
        key: &str,
        mods: gpui_kit::Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let command = mods.platform || mods.control;
        // ⌘S saves from anywhere in the editor.
        if command && !mods.alt && key == "s" {
            cx.emit(EditorEvent::Save);
            return true;
        }
        let Some(caret) = self.focused(window, cx) else {
            return false;
        };
        let now = self.now();
        match key {
            "enter" if mods.alt || command => {
                let next = self.doc.break_at(caret.slide, caret.at, now);
                self.changed(next, window, cx);
                true
            }
            "backspace" if !command && !mods.alt => {
                let empty_selection = self
                    .fields
                    .get(&caret.slide)
                    .map(|f| f.state.read(cx).selected_range())
                    .is_some_and(|r| r.is_empty());
                if caret.at != 0 || !empty_selection {
                    return false;
                }
                match self.doc.join_up(caret.slide, now) {
                    Some(next) => {
                        self.changed(Some(next), window, cx);
                        true
                    }
                    None => false,
                }
            }
            "z" if command && !mods.alt => {
                let target = if mods.shift {
                    self.doc.redo(Some(caret))
                } else {
                    self.doc.undo(Some(caret))
                };
                if let Some(target) = target {
                    self.changed(target, window, cx);
                }
                true
            }
            "y" if command && !mods.alt => {
                if let Some(target) = self.doc.redo(Some(caret)) {
                    self.changed(target, window, cx);
                }
                true
            }
            _ => false,
        }
    }

    /// A paste into a slide: a song's shape is kept, else plain text.
    fn paste(
        &mut self,
        slide: u64,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(range) = self
            .fields
            .get(&slide)
            .map(|f| f.state.read(cx).selected_range())
        else {
            return false;
        };
        let now = self.now();
        match self.doc.paste(slide, range, text, now) {
            Some(caret) => {
                self.changed(Some(caret), window, cx);
                true
            }
            None => false,
        }
    }

    /// Undo / redo from outside the keyboard (tests and menus).
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(target) = self.doc.undo(self.focused(window, cx)) {
            self.changed(target, window, cx);
        }
    }

    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(target) = self.doc.redo(self.focused(window, cx)) {
            self.changed(target, window, cx);
        }
    }

    pub fn paste_into(
        &mut self,
        slide: u64,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.paste(slide, text, window, cx)
    }

    pub fn set_type(
        &mut self,
        block: usize,
        kind: LyricsSectionType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = self.now();
        self.doc.set_type(block, kind, now);
        self.changed(None, window, cx);
    }

    pub fn move_block(
        &mut self,
        block: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = self.now();
        self.doc.move_block(block, to, now);
        self.changed(None, window, cx);
    }

    pub fn delete_block(&mut self, block: usize, window: &mut Window, cx: &mut Context<Self>) {
        let now = self.now();
        self.doc.delete_block(block, now);
        self.changed(None, window, cx);
    }

    pub fn start_section_at(
        &mut self,
        slide: u64,
        kind: LyricsSectionType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = self.now();
        let caret = self.doc.start_section_at(slide, kind, now);
        self.changed(caret, window, cx);
    }

    pub fn add_section(
        &mut self,
        kind: LyricsSectionType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = self.now();
        let caret = self.doc.add_section(kind, now);
        self.changed(Some(caret), window, cx);
    }

    pub fn delete_slide(&mut self, slide: u64, window: &mut Window, cx: &mut Context<Self>) {
        let now = self.now();
        self.doc.delete_slide(slide, now);
        self.changed(None, window, cx);
    }

    pub fn begin_rename(&mut self, block: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(label) = self.doc.blocks().get(block).map(|b| b.label.clone()) else {
            return;
        };
        self.renaming = Some(block);
        self.rename.update(cx, |s, cx| {
            s.set_value(label, window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn render_block(&self, b: usize, number: &mut usize, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let block = &self.doc.blocks()[b];
        let count = self.doc.blocks().len();
        let color = section_color(rows::section_kind(block.kind));
        let entity = cx.entity();
        let label = block.label.clone();
        let gutter = Button::new(SharedString::from(ids::gutter(b)))
            .ghost()
            .xsmall()
            .label(label.clone())
            .accessibility_label(SharedString::from(format!(
                "{label}: rename, change type, move or delete"
            )))
            .popover_trigger()
            .dropdown_menu({
                let entity = entity.clone();
                let current = block.kind;
                move |mut menu, _, _| {
                    let e = entity.clone();
                    menu = menu.item(PopupMenuItem::new("Rename…").on_click(
                        move |_, window, cx| {
                            e.update(cx, |this, cx| this.begin_rename(b, window, cx))
                        },
                    ));
                    menu = menu.separator().label("Type");
                    for (kind, name) in SECTION_TYPES {
                        let e = entity.clone();
                        let swatch = section_color(rows::section_kind(kind));
                        menu = menu.item(
                            PopupMenuItem::element(move |_, _| {
                                h_flex()
                                    .gap_2()
                                    .child(div().size_2p5().rounded(radius::sm()).bg(swatch))
                                    .child(name)
                            })
                            .checked(kind == current)
                            .on_click(move |_, window, cx| {
                                e.update(cx, |this, cx| this.set_type(b, kind, window, cx))
                            }),
                        );
                    }
                    let (up, down, del) = (entity.clone(), entity.clone(), entity.clone());
                    menu.separator()
                        .item(
                            PopupMenuItem::new("Move up")
                                .icon(Icon::new(KairoIcon::ArrowUp))
                                .disabled(b == 0)
                                .on_click(move |_, window, cx| {
                                    up.update(cx, |this, cx| this.move_block(b, b - 1, window, cx))
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Move down")
                                .icon(Icon::new(KairoIcon::ArrowDown))
                                .disabled(b + 1 >= count)
                                .on_click(move |_, window, cx| {
                                    down.update(cx, |this, cx| {
                                        this.move_block(b, b + 1, window, cx)
                                    })
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Delete section")
                                .icon(Icon::new(KairoIcon::Trash2))
                                .on_click(move |_, window, cx| {
                                    del.update(cx, |this, cx| this.delete_block(b, window, cx))
                                }),
                        )
                }
            });
        let rename = (self.renaming == Some(b)).then(|| {
            div().id(ids::RENAME).test_support().w(px(220.)).child(
                Input::new(&self.rename)
                    .kairo_size()
                    .small()
                    .aria_label("Section name"),
            )
        });
        let slides: Vec<AnyElement> = block
            .slides
            .iter()
            .map(|slide| {
                *number += 1;
                let n = *number;
                let key = slide.key;
                let Some(field) = self.fields.get(&key) else {
                    return div().into_any_element();
                };
                let menu_entity = entity.clone();
                let paste_entity = entity.downgrade();
                v_flex()
                    .child(
                        h_flex()
                            .h_6()
                            .pl_2p5()
                            .pr_1()
                            .bg(k.surface_tertiary)
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(div().flex_1().child(format!("{n}.")))
                            .child(
                                Button::new(SharedString::from(ids::slide_menu(key)))
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(KairoIcon::ChevronDown))
                                    .accessibility_label(SharedString::from(format!(
                                        "Slide {n} actions"
                                    )))
                                    .popover_trigger()
                                    .dropdown_menu(move |menu, window, cx| {
                                        let e = menu_entity.clone();
                                        let d = menu_entity.clone();
                                        menu.submenu(
                                            "New section from here",
                                            window,
                                            cx,
                                            move |mut menu, _, _| {
                                                for (kind, name) in SECTION_TYPES {
                                                    let e = e.clone();
                                                    menu = menu.item(
                                                        PopupMenuItem::new(name).on_click(
                                                            move |_, window, cx| {
                                                                e.update(cx, |this, cx| {
                                                                    this.start_section_at(
                                                                        key, kind, window, cx,
                                                                    )
                                                                })
                                                            },
                                                        ),
                                                    );
                                                }
                                                menu
                                            },
                                        )
                                        .separator()
                                        .item(
                                            PopupMenuItem::new("Delete slide")
                                                .icon(Icon::new(KairoIcon::Trash2))
                                                .on_click(move |_, window, cx| {
                                                    d.update(cx, |this, cx| {
                                                        this.delete_slide(key, window, cx)
                                                    })
                                                }),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(ids::slide(key)))
                            .test_support()
                            .px_6()
                            .py_3()
                            .child(
                                Editor::new(&field.state)
                                    .appearance(false)
                                    .h(px(84.))
                                    .aria_label(format!("{label}, slide {n}"))
                                    .on_paste(move |item, window, cx| {
                                        let Some(text) = item.text() else {
                                            return false;
                                        };
                                        paste_entity
                                            .update(cx, |this, cx| {
                                                this.paste(key, &text, window, cx)
                                            })
                                            .unwrap_or(false)
                                    }),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        h_flex()
            .items_stretch()
            .overflow_hidden()
            .rounded(radius::md())
            .bg(k.surface_secondary)
            .child(v_flex().w(px(4.)).flex_none().bg(color))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        h_flex()
                            .gap_2()
                            .px_1()
                            .py_1()
                            .child(gutter)
                            .children(rename),
                    )
                    .children(slides),
            )
            .into_any_element()
    }
}

impl Render for SongEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_focus(window, cx);
        let k = *cx.kairo();
        let error = self.model.read(cx).editing().and_then(|e| e.error.clone());
        let mut number = 0;
        let blocks: Vec<AnyElement> = (0..self.doc.blocks().len())
            .map(|b| self.render_block(b, &mut number, cx))
            .collect();
        let entity = cx.entity();
        v_flex()
            .id(ids::EDITOR)
            .test_support()
            .flex_1()
            .min_h_0()
            .child(
                v_flex()
                    .flex_none()
                    .gap_1()
                    .px_4()
                    .pb_2()
                    .child(div().id(ids::TITLE).test_support().child(Input::new(&self.title).kairo_size().appearance(false)))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(div().flex_1().child(Input::new(&self.artist).kairo_size().small().appearance(false)))
                            .child(div().flex_1().child(Input::new(&self.copyright).kairo_size().small().appearance(false)))
                            .child(div().w_24().child(Input::new(&self.ccli).kairo_size().small().appearance(false))),
                    ),
            )
            .children(error.map(|e| {
                div()
                    .id(ids::ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .mx_4()
                    .mb_2()
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
                    .id("lyrics-editor-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .mx_auto()
                            .w_full()
                            .max_w(px(768.))
                            .gap_3()
                            .px_4()
                            .pb_10()
                            .children(blocks)
                            .child(
                                h_flex()
                                    .gap_3()
                                    .pt_1()
                                    .child(
                                        Button::new(ids::ADD_SECTION)
                                            .ghost()
                                            .small()
                                            .icon(Icon::new(KairoIcon::Plus))
                                            .label("Add section")
                                            .popover_trigger().dropdown_menu(move |mut menu, _, _| {
                                                menu = menu.label("Section type");
                                                for (kind, name) in SECTION_TYPES {
                                                    let e = entity.clone();
                                                    menu = menu.item(PopupMenuItem::new(name).on_click(
                                                        move |_, window, cx| {
                                                            e.update(cx, |this, cx| this.add_section(kind, window, cx))
                                                        },
                                                    ));
                                                }
                                                menu
                                            }),
                                    )
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T600))
                                            .child("⌥↵ new slide · Backspace at the start joins it to the one above"),
                                    ),
                            ),
                    ),
            )
    }
}

impl Focusable for SongEditor {
    fn focus_handle(&self, cx: &gpui_kit::App) -> gpui_kit::FocusHandle {
        self.title.read(cx).focus_handle(cx)
    }
}
