//! `LibrarySection` (`components/shared/LibrarySection.tsx`): the LIBRARY
//! half of a page rail. The default library, then user-made ones. Create
//! with ＋, rename and delete on right-click (double-click also renames),
//! drag an item onto a row to file it there. Deleting a library returns its
//! items to the default, so the control is safe mid-service.
//!
//! Presentation and interaction live here; the page owns the data and runs
//! the library commands, then reports back with [`LibrarySection::landed`]
//! or [`LibrarySection::set_error`].

use crate::components::PopoverTriggerExt;
use crate::control::ControlSize;
use crate::icons::KairoIcon;
use crate::theme::KairoThemeExt;
use crate::tokens::{Hue, TextStep, radius, text};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, DragMoveEvent, ElementId, Entity, EventEmitter,
    InteractiveElement, IntoElement, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div, prelude::FluentBuilder,
};
use std::time::Duration;

/// The id of the built-in library (`DEFAULT_LIBRARY_ID` in `src/lib/libraries.ts`).
pub const DEFAULT_LIBRARY_ID: &str = "default";
/// The longest library name.
pub const MAX_NAME_CHARS: usize = 80;
/// How long a row stays marked after a drop.
pub const LANDED_FOR: Duration = Duration::from_millis(1200);

/// What a library holds; picks the row icon's hue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LibraryKind {
    /// Lyrics: violet.
    Songs,
    /// Scripture: blue.
    Scripture,
    /// Documents: cyan.
    Documents,
}

impl LibraryKind {
    /// The rail colour of the page that owns this kind (`KIND_HUE`).
    pub fn hue(self) -> Hue {
        match self {
            LibraryKind::Songs => Hue::Violet,
            LibraryKind::Scripture => Hue::Blue,
            LibraryKind::Documents => Hue::Cyan,
        }
    }

    /// `DEFAULT_LIBRARY_NAME[kind]`.
    pub fn default_name(self) -> &'static str {
        match self {
            LibraryKind::Songs => "All songs",
            LibraryKind::Scripture => "All passages",
            LibraryKind::Documents => "All slides",
        }
    }
}

/// One user-made library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryRow {
    /// Stable id.
    pub id: SharedString,
    /// Display name.
    pub name: SharedString,
    /// Items filed here.
    pub count: usize,
}

/// The drag payload for an item that can be filed into a library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryDragItem {
    /// The item being filed.
    pub item_id: SharedString,
    /// Its title, for notices.
    pub label: SharedString,
    /// Which libraries accept it.
    pub kind: LibraryKind,
}

impl Render for LibraryDragItem {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::components::drag::DragPill::new(self.label.clone(), "♪").render_pill(cx)
    }
}

/// What the section asks its page to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibrarySectionEvent {
    /// Show this library.
    Select(SharedString),
    /// Create a library (named "New library"); then call
    /// [`LibrarySection::begin_rename`] with its id.
    Create,
    /// One of the extra ＋ menu items, by index.
    AddItem(usize),
    /// Rename to a trimmed, non-empty, changed name.
    Rename {
        /// The library.
        id: SharedString,
        /// Its new name.
        name: SharedString,
    },
    /// Delete; items return to the default library. If it was selected,
    /// `Select(DEFAULT_LIBRARY_ID)` is sent first.
    Delete(SharedString),
    /// File an item here.
    Assign {
        /// The item.
        item_id: SharedString,
        /// Its title.
        label: SharedString,
        /// The library.
        library_id: SharedString,
    },
}

/// An extra entry in the ＋ menu, after "New library".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddItem {
    /// Menu label.
    pub label: SharedString,
    /// Optional icon.
    pub icon: Option<KairoIcon>,
}

struct Renaming {
    id: SharedString,
    original: SharedString,
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// The section.
pub struct LibrarySection {
    kind: LibraryKind,
    libraries: Vec<LibraryRow>,
    default_count: usize,
    active: SharedString,
    add_items: Vec<AddItem>,
    banded: bool,
    context_selection: bool,
    renaming: Option<Renaming>,
    drop_target: Option<SharedString>,
    landed: Option<SharedString>,
    landed_task: Option<Task<()>>,
    error: Option<SharedString>,
}

impl EventEmitter<LibrarySectionEvent> for LibrarySection {}

/// Trims and limits a typed name. `None` when nothing should change.
pub fn rename_target(draft: &str, current: &str) -> Option<String> {
    let trimmed: String = draft.trim().chars().take(MAX_NAME_CHARS).collect();
    if trimmed.is_empty() || trimmed == current {
        None
    } else {
        Some(trimmed)
    }
}

impl LibrarySection {
    /// A section for `kind` with the default library selected.
    pub fn new(kind: LibraryKind) -> Self {
        Self {
            kind,
            libraries: Vec::new(),
            default_count: 0,
            active: DEFAULT_LIBRARY_ID.into(),
            add_items: Vec::new(),
            banded: false,
            context_selection: false,
            renaming: None,
            drop_target: None,
            landed: None,
            landed_task: None,
            error: None,
        }
    }

    /// Extra things ＋ can make; with any, ＋ opens a menu.
    pub fn with_add_items(mut self, items: Vec<AddItem>) -> Self {
        self.add_items = items;
        self
    }

    /// The bold sidebar heading style.
    pub fn banded(mut self, banded: bool) -> Self {
        self.banded = banded;
        self
    }

    /// The chosen library is context for a list below it: mark it in grey.
    pub fn context_selection(mut self, context: bool) -> Self {
        self.context_selection = context;
        self
    }

    /// Replaces the rows and counts.
    pub fn set_libraries(
        &mut self,
        libraries: Vec<LibraryRow>,
        default_count: usize,
        cx: &mut Context<Self>,
    ) {
        self.libraries = libraries;
        self.default_count = default_count;
        cx.notify();
    }

    /// Marks the selected library.
    pub fn set_active(&mut self, id: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.active = id.into();
        cx.notify();
    }

    /// Shows a command's failure under the rows; `None` clears it.
    pub fn set_error(&mut self, error: Option<SharedString>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    /// The error shown, if any.
    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Whether `id` is being renamed.
    pub fn is_renaming(&self, id: &str) -> bool {
        self.renaming.as_ref().is_some_and(|r| r.id.as_ref() == id)
    }

    /// The row being hovered with a drag, if any.
    pub fn drop_target(&self) -> Option<&SharedString> {
        self.drop_target.as_ref()
    }

    /// The row briefly marked after a drop, if any.
    pub fn landed_row(&self) -> Option<&SharedString> {
        self.landed.as_ref()
    }

    /// Marks `id` for [`LANDED_FOR`] after a successful drop.
    pub fn landed(&mut self, id: impl Into<SharedString>, cx: &mut Context<Self>) {
        let id = id.into();
        self.landed = Some(id.clone());
        self.landed_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LANDED_FOR).await;
            let _ = this.update(cx, |this, cx| {
                if this.landed.as_ref() == Some(&id) {
                    this.landed = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    /// Opens the name field for `id`, focused with its text selected.
    pub fn begin_rename(
        &mut self,
        id: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = id.into();
        let Some(row) = self.libraries.iter().find(|row| row.id == id) else {
            return;
        };
        let original = row.name.clone();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(original.clone()));
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_rename(window, cx),
                _ => {}
            },
        );
        self.renaming = Some(Renaming {
            id,
            original,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Closes the name field without renaming (Escape).
    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        cx.notify();
    }

    fn commit_rename(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        let draft = renaming.input.read(cx).value();
        if let Some(name) = rename_target(&draft, &renaming.original) {
            self.error = None;
            cx.emit(LibrarySectionEvent::Rename {
                id: renaming.id,
                name: name.into(),
            });
        }
        cx.notify();
    }

    fn select(&mut self, id: SharedString, cx: &mut Context<Self>) {
        cx.emit(LibrarySectionEvent::Select(id));
    }

    fn delete(&mut self, id: SharedString, cx: &mut Context<Self>) {
        if self.active == id {
            cx.emit(LibrarySectionEvent::Select(DEFAULT_LIBRARY_ID.into()));
        }
        self.error = None;
        cx.emit(LibrarySectionEvent::Delete(id));
    }

    fn row_id(&self, id: &SharedString, part: &'static str) -> ElementId {
        ElementId::NamedChild(
            std::sync::Arc::new(ElementId::Name(format!("library-{}", id).into())),
            part.into(),
        )
    }

    fn row(
        &self,
        id: SharedString,
        name: SharedString,
        count: usize,
        user: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let selected = self.active == id;
        let dropping = self.drop_target.as_ref() == Some(&id);
        let landed = self.landed.as_ref() == Some(&id);
        let kind = self.kind;
        let (bg, fg) = if dropping {
            (Some(k.surface_border), k.text_primary)
        } else if landed {
            (Some(k.surface_elevated), k.text_primary)
        } else if selected {
            if self.context_selection {
                (Some(k.sidebar_pick_sources), k.text_primary)
            } else {
                (Some(k.row_selected()), k.text_primary)
            }
        } else {
            (None, k.text(TextStep::T400))
        };
        let trailing: AnyElement = if dropping {
            div()
                .flex_shrink_0()
                .text_size(text::MICRO.size)
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(k.text(TextStep::T300))
                .child("Move here")
                .into_any_element()
        } else if landed {
            Icon::new(KairoIcon::Check)
                .size_3()
                .text_color(k.text(TextStep::T200))
                .into_any_element()
        } else {
            div()
                .flex_shrink_0()
                .text_size(text::MICRO.size)
                .text_color(k.text(TextStep::T600))
                .child(count.to_string())
                .into_any_element()
        };
        let select_id = id.clone();
        let rename_id = id.clone();
        let drop_id = id.clone();
        let move_id = id.clone();
        let banded = self.banded;
        let row = h_flex()
            .id(self.row_id(&id, "row"))
            .test_support()
            .role(Role::Button)
            .aria_label(name.clone())
            .aria_selected(selected)
            .tab_index(0)
            .w_full()
            .gap_2()
            .rounded(radius::base())
            .px_2()
            .py_1p5()
            .text_size(text::XS.size)
            .text_color(fg)
            .when_some(bg, |this, bg| this.bg(bg))
            .when(bg.is_none(), |this| {
                this.hover(move |s| {
                    let fill = if banded {
                        k.white.opacity(0.05)
                    } else {
                        k.surface_tertiary
                    };
                    s.bg(fill).text_color(k.text(TextStep::T200))
                })
            })
            .focus_visible(|s| s.border_1().border_color(k.focus_ring()))
            .child(
                Icon::new(KairoIcon::Library.fill())
                    .size_3()
                    .text_color(k.hue(kind.hue())),
            )
            .child(div().min_w_0().flex_1().truncate().child(name.clone()))
            .child(trailing)
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if user && event.click_count() == 2 {
                    this.begin_rename(rename_id.clone(), window, cx);
                } else {
                    this.select(select_id.clone(), cx);
                }
            }))
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<LibraryDragItem>, _, cx| {
                    let accepts = event.drag(cx).kind == kind;
                    let inside = event.bounds.contains(&event.event.position);
                    let was = this.drop_target.as_ref() == Some(&move_id);
                    if accepts && inside && !was {
                        this.drop_target = Some(move_id.clone());
                        cx.notify();
                    } else if was && !inside {
                        this.drop_target = None;
                        cx.notify();
                    }
                },
            ))
            .can_drop(move |dragged, _, _| {
                dragged
                    .downcast_ref::<LibraryDragItem>()
                    .is_some_and(|item| item.kind == kind)
            })
            .on_drop(cx.listener(move |this, item: &LibraryDragItem, _, cx| {
                this.drop_target = None;
                this.error = None;
                cx.emit(LibrarySectionEvent::Assign {
                    item_id: item.item_id.clone(),
                    label: item.label.clone(),
                    library_id: drop_id.clone(),
                });
                cx.notify();
            }));
        if !user {
            return row.into_any_element();
        }
        let entity = cx.entity();
        let delete_label: SharedString =
            format!("Delete (items go to {})", kind.default_name()).into();
        let danger = k.danger_text;
        let menu_id = id.clone();
        row.context_menu(move |menu, _, _| {
            let rename = entity.clone();
            let delete = entity.clone();
            let rename_id = menu_id.clone();
            let delete_id = menu_id.clone();
            let label = delete_label.clone();
            menu.item(
                PopupMenuItem::new("Rename")
                    .icon(Icon::new(KairoIcon::Pencil))
                    .on_click(move |_, window, cx| {
                        rename.update(cx, |this, cx| {
                            this.begin_rename(rename_id.clone(), window, cx)
                        });
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::element(move |_, _| {
                    h_flex()
                        .gap_2()
                        .text_color(danger)
                        .child(Icon::new(KairoIcon::Trash2))
                        .child(label.clone())
                })
                .on_click(move |_, _, cx| {
                    delete.update(cx, |this, cx| this.delete(delete_id.clone(), cx));
                }),
            )
        })
        .into_any_element()
    }
}

impl Render for LibrarySection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let heading_text = if self.banded {
            k.text(TextStep::T400)
        } else {
            k.text(TextStep::T600)
        };
        let plus_variant = ButtonCustomVariant::new(cx)
            .foreground(k.text(TextStep::T600))
            .hover(k.surface_tertiary)
            .active(k.surface_tertiary);
        let add: AnyElement = if self.add_items.is_empty() {
            Button::new("library-new")
                .cursor_pointer()
                .custom(plus_variant)
                .icon(Icon::new(KairoIcon::Plus).size_3())
                .xsmall()
                .rounded(radius::sm())
                .accessibility_label("New library")
                .tooltip("New library")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.error = None;
                    cx.emit(LibrarySectionEvent::Create);
                }))
                .into_any_element()
        } else {
            let entity = cx.entity();
            let items = self.add_items.clone();
            Button::new("library-add")
                .cursor_pointer()
                .custom(plus_variant)
                .icon(Icon::new(KairoIcon::Plus).size_3())
                .xsmall()
                .rounded(radius::sm())
                .accessibility_label("New")
                .tooltip("New")
                .popover_trigger()
                .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, move |menu, _, _| {
                    let create = entity.clone();
                    let mut menu = menu.item(
                        PopupMenuItem::new("New library")
                            .icon(Icon::new(KairoIcon::Library))
                            .on_click(move |_, _, cx| {
                                create.update(cx, |this, cx| {
                                    this.error = None;
                                    cx.emit(LibrarySectionEvent::Create);
                                });
                            }),
                    );
                    for (ix, item) in items.iter().enumerate() {
                        let pick = entity.clone();
                        let mut entry =
                            PopupMenuItem::new(item.label.clone()).on_click(move |_, _, cx| {
                                pick.update(cx, |_, cx| cx.emit(LibrarySectionEvent::AddItem(ix)))
                            });
                        if let Some(icon) = item.icon {
                            entry = entry.icon(Icon::new(icon));
                        }
                        menu = menu.item(entry);
                    }
                    menu
                })
                .into_any_element()
        };

        let mut rows: Vec<AnyElement> = vec![self.row(
            DEFAULT_LIBRARY_ID.into(),
            self.kind.default_name().into(),
            self.default_count,
            false,
            cx,
        )];
        for library in self.libraries.clone() {
            if let Some(renaming) = self.renaming.as_ref().filter(|r| r.id == library.id) {
                let input = renaming.input.clone();
                rows.push(
                    div()
                        .id(self.row_id(&library.id, "rename"))
                        .rounded(radius::base())
                        .bg(k.surface_tertiary)
                        .text_size(text::XS.size)
                        .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.cancel_rename(cx);
                            }
                        }))
                        .child(
                            Input::new(&input)
                                .kairo_size()
                                .id(self.row_id(&library.id, "name"))
                                .small()
                                .aria_label("Library name"),
                        )
                        .into_any_element(),
                );
            } else {
                rows.push(self.row(library.id, library.name, library.count, true, cx));
            }
        }
        let _ = window;
        v_flex()
            .id("library-section")
            .child(
                h_flex()
                    .gap_1()
                    .map(|this| {
                        if self.banded {
                            this.h_7().pl_3().pr_2()
                        } else {
                            this.px_2().pb_1()
                        }
                    })
                    .child(
                        div()
                            .flex_1()
                            .text_size(if self.banded {
                                text::LABEL.size
                            } else {
                                text::MICRO.size
                            })
                            .font_weight(if self.banded {
                                gpui_kit::FontWeight::BOLD
                            } else {
                                gpui_kit::FontWeight::SEMIBOLD
                            })
                            .text_color(heading_text)
                            .child("LIBRARY"),
                    )
                    .child(add),
            )
            .child(v_flex().gap_0p5().children(rows))
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .id("library-error")
                        .test_support()
                        .role(Role::Alert)
                        .aria_label(error.clone())
                        .px_2()
                        .pt_1()
                        .text_size(text::MICRO.size)
                        .text_color(k.danger_text)
                        .child(error),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_trims_limits_and_skips_no_change() {
        assert_eq!(rename_target("  Hymns ", "Old"), Some("Hymns".into()));
        assert_eq!(rename_target("   ", "Old"), None);
        assert_eq!(rename_target("Old", "Old"), None);
        assert_eq!(
            rename_target(&"x".repeat(100), "Old")
                .unwrap()
                .chars()
                .count(),
            MAX_NAME_CHARS
        );
    }

    #[test]
    fn kinds_wear_their_page_colour() {
        assert_eq!(LibraryKind::Songs.hue(), Hue::Violet);
        assert_eq!(LibraryKind::Scripture.hue(), Hue::Blue);
        assert_eq!(LibraryKind::Documents.hue(), Hue::Cyan);
    }
}
