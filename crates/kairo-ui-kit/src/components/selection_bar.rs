//! `SelectionBar` (`components/shared/SelectionBar.tsx`): what appears while
//! items are selected. How many, what can be done, and a way out.

use crate::icons::KairoIcon;
use crate::theme::KairoThemeExt;
use crate::tokens::{TextStep, radius, text};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder,
};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The text the bar shows: the hint before anything is picked, else a count.
pub fn selection_label(count: usize, noun: &str, hint: Option<&str>) -> String {
    match hint {
        Some(hint) if count == 0 => hint.to_owned(),
        _ => format!(
            "{count} {noun}{} selected",
            if count == 1 { "" } else { "s" }
        ),
    }
}

/// The selection bar.
#[derive(IntoElement)]
pub struct SelectionBar {
    count: usize,
    noun: SharedString,
    hint: Option<SharedString>,
    actions: Vec<AnyElement>,
    on_clear: Option<Handler>,
}

impl SelectionBar {
    /// A bar for `count` items called `noun` (singular, e.g. "slide").
    pub fn new(count: usize, noun: impl Into<SharedString>) -> Self {
        Self {
            count,
            noun: noun.into(),
            hint: None,
            actions: Vec::new(),
            on_clear: None,
        }
    }

    /// Shown before anything is selected, in select mode.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Adds an action, usually a [`SelectionAction`].
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    /// Called by Clear. Owners also clear on Escape, as the tooltip says.
    pub fn on_clear(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_clear = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SelectionBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let label = selection_label(self.count, &self.noun, self.hint.as_deref());
        div()
            .id("selection-bar")
            .test_support()
            .role(Role::Region)
            .aria_label("Selection")
            .rounded(radius::xl())
            .bg(k.surface_elevated)
            .px_3()
            .py_2p5()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_3()
                    .gap_y_2()
                    .child(
                        div()
                            .id("selection-count")
                            .test_support()
                            .role(Role::Status)
                            .aria_label(label.clone())
                            .text_size(text::XS.size)
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(k.text_primary)
                            .child(label),
                    )
                    .child(
                        h_flex()
                            .min_w_0()
                            .flex_1()
                            .flex_wrap()
                            .gap_1p5()
                            .children(self.actions),
                    )
                    .child(
                        Button::new("selection-clear")
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .foreground(k.text(TextStep::T400))
                                    .hover(k.surface_tertiary)
                                    .active(k.surface_tertiary),
                            )
                            .label("Clear")
                            .icon(Icon::new(KairoIcon::X).size_3())
                            .xsmall()
                            .px_1p5()
                            .py_1()
                            .text_size(text::LABEL.size)
                            .tooltip("Clear selection (Esc)")
                            .when_some(self.on_clear, |this, handler| {
                                this.on_click(move |event, window, cx| handler(event, window, cx))
                            }),
                    ),
            )
    }
}

/// A small action button for the selection bar.
#[derive(IntoElement)]
pub struct SelectionAction {
    id: ElementId,
    label: SharedString,
    icon: Option<KairoIcon>,
    danger: bool,
    disabled: bool,
    tooltip: Option<SharedString>,
    on_click: Option<Handler>,
}

impl SelectionAction {
    /// An action with a label.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            danger: false,
            disabled: false,
            tooltip: None,
            on_click: None,
        }
    }

    /// A leading icon.
    pub fn icon(mut self, icon: KairoIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Deletes or removes: red text, rose hover.
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// Greys the action out and ignores clicks.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// A tooltip.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
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

impl RenderOnce for SelectionAction {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let variant = if self.danger {
            ButtonCustomVariant::new(cx)
                .foreground(k.danger_text_soft)
                .hover(k.tints.rose)
                .active(k.tints.rose)
        } else {
            ButtonCustomVariant::new(cx)
                .color(k.surface_elevated)
                .foreground(k.text_primary)
                .hover(k.surface_border)
                .active(k.surface_border)
        };
        Button::new(self.id)
            .custom(variant)
            .label(self.label)
            .small()
            .h_7()
            .px_2p5()
            .text_size(text::XS.size)
            .font_weight(gpui_kit::FontWeight::MEDIUM)
            .disabled(self.disabled)
            .when_some(self.icon, |this, icon| {
                this.icon(Icon::new(icon).size_3p5())
            })
            .when_some(self.tooltip, |this, tip| this.tooltip(tip))
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::selection_label;

    #[test]
    fn counts_pluralise_and_hint_shows_only_at_zero() {
        assert_eq!(selection_label(1, "slide", None), "1 slide selected");
        assert_eq!(
            selection_label(3, "verse", Some("Pick verses")),
            "3 verses selected"
        );
        assert_eq!(selection_label(0, "item", Some("Pick items")), "Pick items");
        assert_eq!(selection_label(0, "item", None), "0 items selected");
    }
}
