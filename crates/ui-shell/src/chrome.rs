//! Small pieces of the header and rail.

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{Icon, v_flex};
use gpui_kit::{
    AccessibleAction, AnyElement, App, ClickEvent, ElementId, Hsla, InteractiveElement,
    IntoElement, ParentElement, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled,
    Window, div, linear_color_stop, linear_gradient, prelude::FluentBuilder,
};
use kairo_ui_kit::KairoIcon;
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{Appearance, Hue, TextStep, radius, text};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type Toggle = Rc<dyn Fn(&mut Window, &mut App)>;

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
    /// Opens a popover that handles the pointer itself; this toggles it for
    /// assistive technology, which presses without a pointer.
    popover: Option<Toggle>,
    selected: bool,
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
            popover: None,
            selected: false,
        }
    }

    /// The trigger of a popover: a button whose click the popover handles.
    ///
    /// The popover opens on a mouse press, which VoiceOver and Narrator do
    /// not make, so `toggle` opens or closes it when they press the button.
    pub fn opens_popover(mut self, toggle: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.popover = Some(Rc::new(toggle));
        self
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

impl gpui_kit::component::Selectable for ToolbarButton {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.selected
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
        let interactive = (self.on_click.is_some() || self.popover.is_some()) && !self.disabled;
        let pill_bg = if self.selected {
            k.surface_elevated
        } else {
            pill_bg
        };
        let dark = k.appearance == Appearance::Dark;
        let rim = match (self.tone, self.selected, dark) {
            (Tone::Live, _, _) => k.live.opacity(0.35),
            (_, true, true) => k.white.opacity(0.24),
            (_, false, true) => k.white.opacity(0.14),
            (_, true, false) => gpui_kit::black().opacity(0.16),
            (_, false, false) => gpui_kit::black().opacity(0.09),
        };
        let hover_rim = if dark {
            k.white.opacity(0.24)
        } else {
            gpui_kit::black().opacity(0.15)
        };
        let sheen_top = if dark {
            k.white.opacity(0.10)
        } else {
            k.white.opacity(0.55)
        };
        let sheen_bottom = k.white.opacity(0.0);
        let group: SharedString = format!("tool-{}", self.id).into();
        v_flex()
            .id(self.id)
            .test_support()
            .relative()
            .when_some(self.coach, |this, name| {
                this.child(kairo_ui_kit::coach::coach_anchor(name))
            })
            // Pressing a tool never drags the window. A popover trigger is
            // wrapped by its popover, which takes the press and stops it
            // before the drag layer; occluding here would hide the press from
            // the popover too, and the button would never open.
            .when(self.popover.is_none(), |this| this.occlude())
            .group(group.clone())
            .role(if self.on_click.is_some() || self.popover.is_some() {
                Role::Button
            } else {
                Role::Status
            })
            .aria_label(self.title.clone())
            .when(self.popover.is_some(), |this| {
                this.aria_expanded(self.selected)
            })
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
                    .h_8()
                    .w_12()
                    .rounded_full()
                    .overflow_hidden()
                    .border_1()
                    .border_color(rim)
                    .bg(pill_bg)
                    .shadow_sm()
                    .text_color(pill_fg)
                    .when(interactive && self.tone != Tone::Live, |this| {
                        this.group_hover(group, move |s| {
                            s.bg(k.surface_elevated).border_color(hover_rim)
                        })
                    })
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_full()
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(sheen_top, 0.0),
                                linear_color_stop(sheen_bottom, 0.55),
                            )),
                    )
                    .child(self.icon)
                    .when_some(self.badge, |this, color| {
                        this.child(
                            div()
                                .absolute()
                                .top(gpui_kit::px(5.))
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
                    .text_size(text::LABEL.size)
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .line_height(gpui_kit::relative(1.))
                    .text_color(label_fg)
                    .child(self.label),
            )
            .when_some(self.on_click.filter(|_| interactive), |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when_some(self.popover.filter(|_| interactive), |this, toggle| {
                this.on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                    toggle(window, cx)
                })
            })
    }
}

/// A hued header icon (`text-[rgb(var(--hue))]`).
pub fn hued(icon: KairoIcon, hue: Hue, cx: &App) -> Icon {
    Icon::new(icon)
        .size(gpui_kit::px(18.))
        .text_color(cx.kairo().hue(hue))
}
