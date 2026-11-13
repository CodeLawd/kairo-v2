//! `.pref-group` (`index.css`): grouped rows as in macOS System Settings.
//! One rounded box per group, rows split by a faint line inset from both
//! sides. Used by Settings, Screens and the operator's program panels.

use crate::theme::KairoThemeExt;
use crate::tokens::{metrics, radius};
use gpui_kit::component::v_flex;
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, Styled, Window, div,
    prelude::FluentBuilder,
};

/// A settings box: rows divided by inset hairlines.
#[derive(IntoElement, Default)]
pub struct PrefGroup {
    rows: Vec<AnyElement>,
    dim: bool,
}

impl PrefGroup {
    /// An empty group.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a row.
    pub fn row(mut self, row: impl IntoElement) -> Self {
        self.rows.push(row.into_any_element());
        self
    }

    /// Adds rows.
    pub fn rows(mut self, rows: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.rows
            .extend(rows.into_iter().map(IntoElement::into_any_element));
        self
    }

    /// `.pref-group-dim`: the group sits on the darker workspace surface, so
    /// its fill is `surface` and the line drops a step to stay faint.
    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }
}

impl RenderOnce for PrefGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = cx.kairo();
        let fill = if self.dim {
            k.surface
        } else {
            k.surface_secondary
        };
        let line = k.pref_line(self.dim);
        v_flex()
            .rounded(radius::md())
            .bg(fill)
            .overflow_hidden()
            .children(self.rows.into_iter().enumerate().map(|(ix, row)| {
                div()
                    .relative()
                    .when(ix > 0, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top_0()
                                .left(metrics::pref_line_inset())
                                .right(metrics::pref_line_inset())
                                .h_px()
                                .bg(line),
                        )
                    })
                    .child(row)
            }))
    }
}
