//! `MacTrafficLights` (`components/shared/MacTrafficLights.tsx`): window
//! controls for an in-app modal window (Settings, Screens). Red closes;
//! green zooms when the window sits in a resizable frame; yellow is greyed,
//! as macOS shows a button that cannot act.

use crate::theme::KairoThemeExt;
use crate::tokens::metrics;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::h_flex;
use gpui_kit::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The three window buttons.
#[derive(IntoElement)]
pub struct TrafficLights {
    id: ElementId,
    on_close: Option<Handler>,
    on_zoom: Option<Handler>,
}

impl TrafficLights {
    /// Lights with no actions yet.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            on_close: None,
            on_zoom: None,
        }
    }

    /// Red: close the window.
    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    /// Green: zoom the frame. Without it, green is drawn greyed like yellow.
    pub fn on_zoom(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_zoom = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for TrafficLights {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let group: SharedString = format!("{}-lights", self.id).into();
        let light = |name: &'static str,
                     label: &'static str,
                     color,
                     glyph: &'static str,
                     glyph_color,
                     handler: Option<Handler>| {
            let group = group.clone();
            div()
                .id(ElementId::NamedChild(
                    std::sync::Arc::new(self.id.clone()),
                    name.into(),
                ))
                .test_support()
                .role(Role::Button)
                .aria_label(label)
                .tab_index(0)
                .size(metrics::traffic_light())
                .rounded_full()
                .bg(color)
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::tokens::text::MICRO.size)
                .font_weight(gpui_kit::FontWeight::BOLD)
                .line_height(crate::tokens::text::r(0.5))
                .focus_visible(move |s| s.border_1().border_color(k.white.opacity(0.3)))
                .child(
                    div()
                        .text_color(glyph_color)
                        .invisible()
                        .group_hover(group, |s| s.visible())
                        .child(glyph),
                )
                .when_some(handler, |this, handler| {
                    this.on_click(move |event, window, cx| handler(event, window, cx))
                })
        };
        let greyed = || {
            div()
                .size(metrics::traffic_light())
                .rounded_full()
                .bg(k.surface_elevated)
        };
        h_flex()
            .group(group.clone())
            .gap(metrics::traffic_light_gap())
            .child(light(
                "close",
                "Close",
                k.traffic_close,
                "×",
                k.traffic_close_glyph,
                self.on_close,
            ))
            .child(greyed())
            .map(|this| match self.on_zoom {
                Some(zoom) => this.child(light(
                    "zoom",
                    "Zoom",
                    k.traffic_zoom,
                    "+",
                    k.traffic_zoom_glyph,
                    Some(zoom),
                )),
                None => this.child(greyed()),
            })
    }
}
