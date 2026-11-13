//! `lib/drag.ts`: a compact "♪  Title" pill as the drag image, instead of a
//! picture of the whole row.
//!
//! `leavesTarget` is not needed: GPUI reports drag position against each
//! target's bounds, so moving over a child never reads as leaving.

use crate::theme::KairoThemeExt;
use crate::tokens::text;
use gpui_kit::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px,
};

/// The widest a pill gets before its label is cut with an ellipsis (260px).
pub fn max_width() -> gpui_kit::Pixels {
    px(260.)
}

/// The drag image for a row.
#[derive(IntoElement, Clone)]
pub struct DragPill {
    label: SharedString,
    glyph: SharedString,
}

impl DragPill {
    /// A pill reading `glyph  label` (the original's default glyph is ♪).
    pub fn new(label: impl Into<SharedString>, glyph: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            glyph: glyph.into(),
        }
    }

    /// The text the pill shows.
    pub fn text(&self) -> String {
        format!("{}  {}", self.glyph, self.label)
    }

    /// Renders the pill from a drag view's context.
    pub fn render_pill(self, cx: &App) -> gpui_kit::Div {
        let k = cx.kairo();
        div()
            .max_w(max_width())
            .px_3()
            .py_1p5()
            .bg(k.surface_elevated)
            .border_1()
            .border_color(k.surface_border)
            .text_color(k.text_primary)
            .text_size(text::XS.size)
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .whitespace_nowrap()
            .truncate()
            .child(self.text())
    }
}

impl RenderOnce for DragPill {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.render_pill(cx)
    }
}
