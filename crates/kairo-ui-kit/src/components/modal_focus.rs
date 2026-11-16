//! Focus for a modal dialog, as Radix `Dialog` handled it (prompt 106).
//!
//! While a modal is open, focus moves into it, Tab and Shift-Tab cycle inside
//! it, and when it closes focus goes back to whatever had it before (the
//! trigger, usually). [`MovableDialog`](super::MovableDialog) does this for
//! the windows it frames; `ModalFocus` gives the same to dialogs a view draws
//! itself, whose open and close happen where no `Window` is at hand: the view
//! calls [`ModalFocus::sync`] from `render` with whether the dialog is open,
//! and gives the dialog element `.track_focus(modal.handle())` and
//! `.focus_trap(id, modal.handle())`.

use gpui_kit::{App, FocusHandle, Window};

/// A modal dialog's focus: its own handle and the one to give focus back to.
pub struct ModalFocus {
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    open: bool,
}

impl ModalFocus {
    /// A closed dialog's focus.
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            restore: None,
            open: false,
        }
    }

    /// The dialog's focus handle.
    pub fn handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Call where the dialog opens, if a `Window` is at hand there: it
    /// remembers what has focus (the trigger) and moves focus into the
    /// dialog. A dialog that focuses one of its own fields can do so after.
    pub fn open(&mut self, window: &mut Window, cx: &mut App) {
        if !self.open {
            self.open = true;
            self.restore = window.focused(cx);
            self.focus.focus(window, cx);
        }
    }

    /// Call each render with whether the dialog is open. The first frame it
    /// is open without [`Self::open`] having run, it does what `open` does;
    /// a dialog opened with focus already inside keeps it. The first frame
    /// after it closes, focus goes back, unless the person moved it out of
    /// the dialog in the meantime. Returns `true` on the frame it opened, so
    /// a dialog can put initial focus on one of its controls.
    pub fn sync(&mut self, open: bool, window: &mut Window, cx: &mut App) -> bool {
        let opened = !self.open && open;
        match (self.open, open) {
            (false, true) => {
                self.open = true;
                self.restore = window
                    .focused(cx)
                    .filter(|f| !self.focus.contains(f, window));
                if !self.focus.contains_focused(window, cx) {
                    self.focus.focus(window, cx);
                }
            }
            (true, false) => {
                self.open = false;
                let inside = window
                    .focused(cx)
                    .is_none_or(|f| f == self.focus || self.focus.contains(&f, window));
                if let Some(restore) = self.restore.take()
                    && inside
                {
                    restore.focus(window, cx);
                }
            }
            _ => {}
        }
        opened
    }
}

#[cfg(test)]
mod tests {
    use super::ModalFocus;
    use gpui_kit::component::FocusTrapElement;
    use gpui_kit::{
        Context, FocusHandle, InteractiveElement, IntoElement, ParentElement, Render,
        TestAppContext, Window, div, prelude::FluentBuilder,
    };

    struct Page {
        trigger: FocusHandle,
        modal: ModalFocus,
        open: bool,
    }

    impl Render for Page {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.modal.sync(self.open, window, cx);
            div()
                .child(div().id("trigger").track_focus(&self.trigger))
                .when(self.open, |d| {
                    d.child(div().id("dialog").focus_trap("dialog", self.modal.handle()))
                })
        }
    }

    #[gpui_kit::test]
    fn focus_moves_in_and_comes_back(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (page, cx) = cx.add_window_view(|_, cx| Page {
            trigger: cx.focus_handle(),
            modal: ModalFocus::new(cx),
            open: false,
        });
        cx.update(|window, cx| {
            let trigger = page.read(cx).trigger.clone();
            trigger.focus(window, cx);
            page.update(cx, |p, cx| {
                p.open = true;
                cx.notify()
            });
            window.draw(cx).clear(cx);
            window.draw(cx).clear(cx);
            assert!(page.read(cx).modal.handle().is_focused(window));
            page.update(cx, |p, cx| {
                p.open = false;
                cx.notify()
            });
            window.draw(cx).clear(cx);
            window.draw(cx).clear(cx);
            assert!(page.read(cx).trigger.is_focused(window), "focus went back");
        });
    }
}
