//! Control sizes from Electron's `index.css`: `.btn-primary`/`.btn-secondary`
//! (`px-3.5 py-1.5 text-sm`, 32 px tall) and `.input` (`px-3 py-2 text-sm`,
//! 36 px tall). gpui-kit's own small sizes are tighter, so shared controls
//! take these instead.

use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::Input;
use gpui_kit::{Styled, px};

/// Electron's control sizes on gpui-kit controls.
pub trait ControlSize: Sized {
    /// The size of Electron's `.btn-*` and `.input` classes.
    fn kairo_size(self) -> Self;
}

impl ControlSize for Button {
    fn kairo_size(self) -> Self {
        // Medium is 32 px tall with 14 px text; Electron pads 14 px a side.
        self.with_size(gpui_kit::component::Size::Medium)
            .px(px(14.))
    }
}

impl ControlSize for Input {
    fn kairo_size(self) -> Self {
        // A floor, not a height, so multi-line fields still grow.
        self.px(px(12.)).min_h(px(36.))
    }
}
