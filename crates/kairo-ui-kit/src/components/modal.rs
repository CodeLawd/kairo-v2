//! Modal windows drawn inside the app window.
//!
//! - [`MovableDialog`] (`components/shared/MovableDialog.tsx`): the in-app
//!   windows (Settings, Screens) that the operator can drag by a header or
//!   sidebar marked as a handle.
//! - [`FrostedPanel`] (`.glass` / `.modal-glass` in `index.css`): the
//!   frosted fill for a modal's sidebar, title bar or a whole small dialog.
//!
//! GPUI cannot blur what is behind a surface inside one window, so the
//! frost is the translucent `--glass` tint without `backdrop-filter`. This is
//! logged in DECISIONS.md.

use crate::theme::KairoThemeExt;
use crate::tokens::{metrics, radius};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::FocusTrapElement;
use gpui_kit::{
    AnyElement, App, AppContext, Bounds, Context, DragMoveEvent, ElementId, Empty, EventEmitter,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels,
    Point, Render, RenderOnce, Role, SharedString, Size, StatefulInteractiveElement, Styled,
    Window, div, point, prelude::FluentBuilder, px,
};

/// How much of a dragged dialog must stay on screen, in pixels.
pub const DRAG_EDGE: f32 = 48.0;
/// The smallest gap kept above a dragged dialog.
pub const DRAG_TOP: f32 = 8.0;

/// Clamps a dialog's offset from the centred position so at least
/// [`DRAG_EDGE`] of it stays inside the viewport and its top never goes
/// above [`DRAG_TOP`] (`clampOffset` in the original). Results are rounded.
pub fn clamp_offset(offset: Point<f32>, dialog: Size<f32>, viewport: Size<f32>) -> Point<f32> {
    let base_left = (viewport.width - dialog.width) / 2.0;
    let base_top = (viewport.height - dialog.height) / 2.0;
    let left = base_left + offset.x;
    let top = base_top + offset.y;
    let next_left = left
        .max(DRAG_EDGE - dialog.width)
        .min(viewport.width - DRAG_EDGE);
    let next_top = top.max(DRAG_TOP).min(viewport.height - DRAG_EDGE);
    point(
        (next_left - base_left).round(),
        (next_top - base_top).round(),
    )
}

/// What the dialog reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovableDialogEvent {
    /// Escape or the scrim asked to close. The owner removes the dialog.
    Dismiss,
}

#[derive(Clone, Copy)]
struct DialogDrag;

impl Render for DialogDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// The retained state of a movable dialog: its offset and focus.
pub struct MovableDialog {
    id: ElementId,
    label: SharedString,
    offset: Point<f32>,
    size: Size<Pixels>,
    focus: FocusHandle,
    restore_focus: Option<FocusHandle>,
    dragging: bool,
    press: Option<(Point<Pixels>, Point<f32>)>,
}

impl EventEmitter<MovableDialogEvent> for MovableDialog {}

impl MovableDialog {
    /// A dialog of `size`, named `label` for assistive technology. The
    /// element focused when it opened gets focus back on [`Self::restore`].
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        size: Size<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let restore_focus = window.focused(cx);
        focus.focus(window, cx);
        Self {
            id: id.into(),
            label: label.into(),
            offset: point(0.0, 0.0),
            size,
            focus,
            restore_focus,
            dragging: false,
            press: None,
        }
    }

    /// The offset from the centred position.
    pub fn offset(&self) -> Point<f32> {
        self.offset
    }

    /// Whether a drag is in progress.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Resizes the frame (the green light's zoom).
    pub fn set_size(&mut self, size: Size<Pixels>, cx: &mut Context<Self>) {
        self.size = size;
        cx.notify();
    }

    /// Returns focus to whatever had it before the dialog opened.
    pub fn restore(&self, window: &mut Window, cx: &mut App) {
        if let Some(handle) = &self.restore_focus {
            handle.focus(window, cx);
        }
    }

    /// The element that starts a drag: put the header or sidebar inside it.
    /// Buttons, inputs and other controls inside keep their own pointer
    /// behaviour, because GPUI delivers the press to them first.
    pub fn drag_handle(
        &self,
        id: impl Into<ElementId>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        div()
            .id(id)
            // The press point and offset, in window coordinates: the handle
            // moves with the dialog, so positions relative to it drift.
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, _| {
                    this.press = Some((event.position, this.offset));
                }),
            )
            .on_drag(DialogDrag, |drag, _, _, cx| cx.new(|_| *drag))
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<DialogDrag>, window, cx| {
                    let Some((origin, start)) = this.press else {
                        return;
                    };
                    let dx = f32::from(event.event.position.x - origin.x);
                    let dy = f32::from(event.event.position.y - origin.y);
                    let viewport = window.viewport_size();
                    this.dragging = true;
                    this.offset = clamp_offset(
                        point(start.x + dx, start.y + dy),
                        Size::new(f32::from(this.size.width), f32::from(this.size.height)),
                        Size::new(f32::from(viewport.width), f32::from(viewport.height)),
                    );
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.press = None;
                    this.dragging = false;
                    cx.notify();
                }),
            )
    }

    /// Wraps `content` in the scrim and the positioned, focus-trapped frame.
    pub fn frame(
        &self,
        content: impl IntoElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let viewport = window.viewport_size();
        let left = (viewport.width - self.size.width) / 2.0 + px(self.offset.x);
        let top = (viewport.height - self.size.height) / 2.0 + px(self.offset.y);
        crate::motion::enter(
            div()
                .id(self.id.clone())
                .absolute()
                .inset_0()
                .bg(k.scrim())
                .occlude()
                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation()
                })
                .child(crate::motion::rise_in(
                    div().absolute().inset_0().child(
                        div()
                            .id("movable-dialog-frame")
                            .test_support()
                            .role(Role::Dialog)
                            .aria_label(self.label.clone())
                            .track_focus(&self.focus)
                            .key_context("MovableDialog")
                            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                                if event.keystroke.key == "escape" {
                                    cx.stop_propagation();
                                    cx.emit(MovableDialogEvent::Dismiss);
                                }
                            }))
                            .absolute()
                            .left(left)
                            .top(top)
                            .w(self.size.width)
                            .h(self.size.height)
                            .rounded(radius::xl())
                            .overflow_hidden()
                            .bg(k.surface)
                            .border(metrics::panel_border_width())
                            .border_color(k.panel_border)
                            .when(self.dragging, |this| this.cursor_grabbing())
                            .child(content)
                            .focus_trap(
                                ElementId::NamedChild(
                                    std::sync::Arc::new(self.id.clone()),
                                    "trap".into(),
                                ),
                                &self.focus,
                            ),
                    ),
                    "movable-dialog-rise",
                    cx,
                )),
            "movable-dialog-enter",
            cx,
        )
    }

    /// The frame's bounds in window coordinates, for tests and anchoring.
    pub fn frame_bounds(&self, window: &Window) -> Bounds<Pixels> {
        let viewport = window.viewport_size();
        Bounds::new(
            point(
                (viewport.width - self.size.width) / 2.0 + px(self.offset.x),
                (viewport.height - self.size.height) / 2.0 + px(self.offset.y),
            ),
            self.size,
        )
    }
}

impl Focusable for MovableDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// How frosted a panel is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Frost {
    /// `.glass`: a modal's sidebar or title bar.
    #[default]
    Panel,
    /// `.modal-glass`: a whole small dialog, a touch more opaque.
    Modal,
}

/// A frosted surface.
#[derive(IntoElement)]
pub struct FrostedPanel {
    frost: Frost,
    children: Vec<AnyElement>,
    rounded: bool,
}

impl FrostedPanel {
    /// A panel with the given frost.
    pub fn new(frost: Frost) -> Self {
        Self {
            frost,
            children: Vec::new(),
            rounded: frost == Frost::Modal,
        }
    }

    /// Rounds the corners (default for [`Frost::Modal`]).
    pub fn rounded(mut self, rounded: bool) -> Self {
        self.rounded = rounded;
        self
    }
}

impl ParentElement for FrostedPanel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for FrostedPanel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = cx.kairo();
        let fill = match self.frost {
            Frost::Panel => k.glass_panel(),
            Frost::Modal => k.glass_modal(),
        };
        div()
            .bg(fill)
            .when(self.rounded, |this| this.rounded(radius::xl()))
            .children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sz(w: f32, h: f32) -> Size<f32> {
        Size::new(w, h)
    }

    #[test]
    fn keeps_a_grab_edge_on_screen() {
        let view = sz(1000.0, 800.0);
        let dialog = sz(400.0, 300.0);
        // Far right: left edge stops 48px from the right side.
        let p = clamp_offset(point(5000.0, 0.0), dialog, view);
        assert_eq!(p.x, (1000.0 - 48.0) - 300.0);
        // Far left: 48px of the dialog remains.
        let p = clamp_offset(point(-5000.0, 0.0), dialog, view);
        assert_eq!(p.x, (48.0 - 400.0) - 300.0);
        // Top never above 8px; bottom keeps 48px.
        assert_eq!(
            clamp_offset(point(0.0, -5000.0), dialog, view).y,
            8.0 - 250.0
        );
        assert_eq!(
            clamp_offset(point(0.0, 5000.0), dialog, view).y,
            (800.0 - 48.0) - 250.0
        );
    }

    #[test]
    fn small_moves_pass_through_rounded() {
        let p = clamp_offset(point(10.4, -20.6), sz(400.0, 300.0), sz(1000.0, 800.0));
        assert_eq!(p, point(10.0, -21.0));
    }
}
