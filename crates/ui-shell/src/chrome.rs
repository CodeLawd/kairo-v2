//! Small pieces of the header and rail.

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{Icon, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder,
};
use kairo_ui_kit::KairoIcon;
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{Hue, TextStep, radius, text};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// How a header tool reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Ordinary.
    #[default]
    Default,
    /// Something is on screen (red).
    Live,
    /// Nothing to act on yet.
    Muted,
}

/// `ToolbarButton`: the icon in a rounded pill, a short label under it, the
/// detail in the tooltip and accessible name.
#[derive(IntoElement)]
pub struct ToolbarButton {
    id: ElementId,
    coach: Option<&'static str>,
    label: SharedString,
    title: SharedString,
    icon: AnyElement,
    badge: Option<Hsla>,
    tone: Tone,
    disabled: bool,
    on_click: Option<Handler>,
}

impl ToolbarButton {
    /// A tool with an icon element.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        title: impl Into<SharedString>,
        icon: impl IntoElement,
    ) -> Self {
        Self {
            id: id.into(),
            coach: None,
            label: label.into(),
            title: title.into(),
            icon: icon.into_any_element(),
            badge: None,
            tone: Tone::Default,
            disabled: false,
            on_click: None,
        }
    }

    /// Marks the button as a coach-tour target.
    pub fn coach_anchor(mut self, name: &'static str) -> Self {
        self.coach = Some(name);
        self
    }

    /// A status dot on the icon's corner.
    pub fn badge(mut self, color: Hsla) -> Self {
        self.badge = Some(color);
        self
    }

    /// The tone.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// Disables the tool.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The click handler. Without one the tool is a status.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ToolbarButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let (pill_bg, pill_fg, label_fg) = match self.tone {
            Tone::Live => (k.tints.live, k.live, k.live),
            Tone::Muted => (
                k.surface_tertiary,
                k.text(TextStep::T200),
                k.text(TextStep::T600),
            ),
            Tone::Default => (
                k.surface_tertiary,
                k.text(TextStep::T200),
                k.text(TextStep::T400),
            ),
        };
        let interactive = self.on_click.is_some() && !self.disabled;
        let group: SharedString = format!("tool-{}", self.id).into();
        v_flex()
            .id(self.id)
            .test_support()
            .relative()
            .when_some(self.coach, |this, name| {
                this.child(kairo_ui_kit::coach::coach_anchor(name))
            })
            // Pressing a tool never drags the window.
            .occlude()
            .group(group.clone())
            .role(if self.on_click.is_some() {
                Role::Button
            } else {
                Role::Status
            })
            .aria_label(self.title.clone())
            .when(interactive, |this| this.tab_index(0))
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .rounded(radius::base())
            .px_1()
            .when(self.disabled, |this| this.opacity(0.4))
            .focus_visible(move |s| s.border_1().border_color(k.white.opacity(0.3)))
            .tooltip({
                let title = self.title.clone();
                move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                }
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h_7()
                    .w_11()
                    .rounded_full()
                    .bg(pill_bg)
                    .text_color(pill_fg)
                    .when(interactive && self.tone != Tone::Live, |this| {
                        this.group_hover(group, move |s| s.bg(k.surface_elevated))
                    })
                    .child(self.icon)
                    .when_some(self.badge, |this, color| {
                        this.child(
                            div()
                                .absolute()
                                .top(gpui_kit::px(4.))
                                .right(gpui_kit::px(10.))
                                .size_2()
                                .rounded_full()
                                .bg(color)
                                .border_2()
                                .border_color(k.surface_tertiary),
                        )
                    }),
            )
            .child(
                div()
                    .max_w_40()
                    .truncate()
                    .text_size(text::MICRO.size)
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .line_height(gpui_kit::relative(1.))
                    .text_color(label_fg)
                    .child(self.label),
            )
            .when_some(self.on_click.filter(|_| interactive), |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

/// A hued header icon (`text-[rgb(var(--hue))]`).
pub fn hued(icon: KairoIcon, hue: Hue, cx: &App) -> Icon {
    Icon::new(icon).size_4().text_color(cx.kairo().hue(hue))
}
