//! One width for every page's library sidebar (`useSidebarWidth.ts`):
//! Scripture, Lyrics, Documents, Web and Engage share it, so switching pages
//! never jumps. Kept under `kairo.page-sidebar-width`; the first of the old
//! per-page keys seeds it.

use std::rc::Rc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::{
    App, AppContext, DragMoveEvent, ElementId, Global, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, Role, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::KairoThemeExt;
use crate::tokens::Step;

pub const MIN: f32 = 200.0;
pub const MAX: f32 = 480.0;
pub const DEFAULT: f32 = 280.0;
/// Room the page keeps beside the sidebar.
pub const PAGE_ROOM: f32 = 360.0;
pub const STORAGE_KEY: &str = "kairo.page-sidebar-width";
/// The widths pages kept before they were shared.
pub const LEGACY_KEYS: [&str; 4] = [
    "kairo.scripture-sidebar-width",
    "kairo.lyrics-library-width",
    "kairo.documents-sidebar-width",
    "kairo.web-sidebar-width",
];

/// `savedWidth`: the first stored width at least `MIN`, at most `MAX`.
pub fn saved_width(read: impl Fn(&str) -> Option<f64>) -> f32 {
    for key in std::iter::once(STORAGE_KEY).chain(LEGACY_KEYS) {
        if let Some(v) = read(key).filter(|v| v.is_finite() && *v >= f64::from(MIN)) {
            return (v as f32).min(MAX);
        }
    }
    DEFAULT
}

/// The widest the sidebar may be beside a page `container` wide.
pub fn maximum(container: Option<f32>) -> f32 {
    match container {
        Some(w) => (w - PAGE_ROOM).clamp(MIN, MAX),
        None => MAX,
    }
}

/// The shared width.
pub struct PageSidebar {
    preferred: f32,
    save: Option<Rc<dyn Fn(f32)>>,
    drag: Option<(f32, f32)>,
}

impl Global for PageSidebar {}

impl PageSidebar {
    /// Sets the stored width and how a change is kept.
    pub fn install(cx: &mut App, preferred: f32, save: impl Fn(f32) + 'static) {
        cx.set_global(PageSidebar {
            preferred: preferred.clamp(MIN, MAX),
            save: Some(Rc::new(save)),
            drag: None,
        });
    }

    fn get(cx: &App) -> f32 {
        cx.try_global::<PageSidebar>()
            .map_or(DEFAULT, |s| s.preferred)
    }

    /// The width a page draws beside a container `container` wide.
    pub fn width(container: Option<f32>, cx: &App) -> f32 {
        Self::get(cx).min(maximum(container))
    }

    /// Sets it for every page, kept on its range, and saved.
    pub fn set(next: f32, container: Option<f32>, cx: &mut App) {
        let clamped = next.clamp(MIN, maximum(container));
        if !cx.has_global::<PageSidebar>() {
            cx.set_global(PageSidebar {
                preferred: DEFAULT,
                save: None,
                drag: None,
            });
        }
        let save = {
            let s = cx.global_mut::<PageSidebar>();
            s.preferred = clamped;
            s.save.clone()
        };
        if let Some(save) = save {
            save(clamped);
        }
        cx.refresh_windows();
    }
}

#[derive(Clone)]
struct SidebarDrag;

impl Render for SidebarDrag {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The resize handle at the sidebar's edge: drag to resize, double-click to
/// put the default back.
pub fn handle(id: impl Into<ElementId>, container: Option<f32>, cx: &App) -> impl IntoElement {
    let k = *cx.kairo();
    div()
        .id(id.into())
        .test_support()
        .role(Role::Splitter)
        .aria_label("Resize sidebar")
        .flex_none()
        .w_1p5()
        .h_full()
        .cursor_col_resize()
        .bg(k.surface_secondary)
        .hover(|s| s.bg(k.neutral(Step::S600)))
        .on_drag(SidebarDrag, |d, _, _, cx| cx.new(|_| d.clone()))
        .on_drag_move(move |event: &DragMoveEvent<SidebarDrag>, _, cx: &mut App| {
            let x = f32::from(event.event.position.x);
            let start = {
                let width = PageSidebar::width(container, cx);
                if !cx.has_global::<PageSidebar>() {
                    cx.set_global(PageSidebar {
                        preferred: DEFAULT,
                        save: None,
                        drag: None,
                    });
                }
                *cx.global_mut::<PageSidebar>()
                    .drag
                    .get_or_insert((x, width))
            };
            PageSidebar::set(start.1 + x - start.0, container, cx);
        })
        .on_mouse_up(MouseButton::Left, |_, _, cx: &mut App| {
            if cx.has_global::<PageSidebar>() {
                cx.global_mut::<PageSidebar>().drag = None;
            }
        })
        .on_drop(|_: &SidebarDrag, _, cx: &mut App| {
            if cx.has_global::<PageSidebar>() {
                cx.global_mut::<PageSidebar>().drag = None;
            }
        })
        .on_click(move |e, _, cx| {
            if e.click_count() == 2 {
                PageSidebar::set(DEFAULT, container, cx);
            }
        })
        .child(div().size_full().px(px(0.)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_width_reads_the_new_key_then_the_old_ones() {
        assert_eq!(saved_width(|_| None), DEFAULT);
        assert_eq!(
            saved_width(|k| (k == "kairo.lyrics-library-width").then_some(320.0)),
            320.0
        );
        assert_eq!(saved_width(|k| (k == STORAGE_KEY).then_some(900.0)), MAX);
        assert_eq!(
            saved_width(|_| Some(150.0)),
            DEFAULT,
            "too narrow is ignored"
        );
        assert_eq!(maximum(Some(700.0)), 340.0);
        assert_eq!(maximum(Some(300.0)), MIN);
        assert_eq!(maximum(None), MAX);
    }
}
