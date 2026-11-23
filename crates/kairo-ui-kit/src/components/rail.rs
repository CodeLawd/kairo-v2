//! A workspace-rail item (`railHueClass` and `RAIL_LABEL` in `lib/hue.ts`):
//! the mark with its name underneath, the whole column clickable. Grey at
//! rest so the rail stays quiet, its hue on hover, and the hue on a quiet
//! elevated fill when active or while its popover is open.

use crate::icons::KairoIcon;
use crate::theme::KairoThemeExt;
use crate::tokens::{Appearance, Hue, TextStep, radius, text};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{Icon, v_flex};
use gpui_kit::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, linear_color_stop,
    linear_gradient, prelude::FluentBuilder, px, transparent_black,
};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One rail item.
#[derive(IntoElement)]
pub struct RailItem {
    id: ElementId,
    icon: KairoIcon,
    label: SharedString,
    hue: Hue,
    active: bool,
    open: bool,
    compact: bool,
    on_click: Option<Handler>,
}

impl RailItem {
    /// An item for a tool.
    pub fn new(
        id: impl Into<ElementId>,
        icon: KairoIcon,
        label: impl Into<SharedString>,
        hue: Hue,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            hue,
            active: false,
            open: false,
            compact: false,
            on_click: None,
        }
    }

    /// Icon only, for a rail too short for every name. The name stays the
    /// accessible label; the caller gives it a tooltip.
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// The current workspace.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Its popover is open (`data-state=open`).
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// The click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for RailItem {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let hue = k.hue(self.hue);
        let lifted = self.active || self.open;
        let dark = k.appearance == Appearance::Dark;
        let active_rim = if dark {
            k.white.opacity(0.16)
        } else {
            gpui_kit::black().opacity(0.10)
        };
        let hover_rim = if dark {
            k.white.opacity(0.10)
        } else {
            gpui_kit::black().opacity(0.07)
        };
        let sheen_top = if dark {
            k.white.opacity(0.09)
        } else {
            k.white.opacity(0.5)
        };
        v_flex()
            .id(self.id)
            .test_support()
            .role(Role::Button)
            .aria_label(self.label.clone())
            .aria_selected(self.active)
            .tab_index(0)
            .relative()
            .overflow_hidden()
            .w_full()
            .items_center()
            .justify_center()
            .gap(px(3.))
            .rounded(radius::lg())
            .border_1()
            .px_0p5()
            .py_1()
            .map(|this| {
                if lifted {
                    this.bg(k.surface_tertiary)
                        .border_color(active_rim)
                        .shadow_sm()
                        .text_color(hue)
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(radius::lg())
                                .bg(linear_gradient(
                                    180.,
                                    linear_color_stop(sheen_top, 0.0),
                                    linear_color_stop(k.white.opacity(0.0), 0.55),
                                )),
                        )
                } else {
                    this.border_color(transparent_black())
                        .text_color(k.text(TextStep::T500))
                        .hover(move |s| {
                            s.bg(k.surface_tertiary.opacity(0.75))
                                .border_color(hover_rim)
                                .text_color(hue)
                        })
                }
            })
            .focus_visible(move |s| s.border_2().border_color(hue.opacity(0.6)))
            .child(Icon::new(self.icon.fill()).size_5())
            .when(!self.compact, |this| {
                this.child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_size(text::LABEL.size)
                        .line_height(text::LABEL.line_height)
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(self.label),
                )
            })
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}
