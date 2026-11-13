//! `ZoomControl` (`components/shared/ZoomControl.tsx`): − [slider] + for card
//! and slide size in Scripture and Lyrics. Plain, like ProPresenter's size
//! slider: no box, no percentage. Double-click it to reset.

use crate::icons::KairoIcon;
use crate::theme::KairoThemeExt;
use crate::tokens::{TextStep, radius};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex};
use gpui_kit::{
    AppContext, ClickEvent, Context, ElementId, Entity, EventEmitter, InteractiveElement,
    IntoElement, ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, div,
};

/// The range and steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomRange {
    /// Smallest value.
    pub min: f32,
    /// Largest value.
    pub max: f32,
    /// Double-click resets to this.
    pub default: f32,
    /// Slider step (5 in the original).
    pub step: f32,
    /// − and + step (10 in the original).
    pub button_step: f32,
}

impl ZoomRange {
    /// `value` after the − (`-1`) or + (`1`) button, clamped.
    pub fn bump(&self, value: f32, direction: i8) -> f32 {
        (value + f32::from(direction.signum()) * self.button_step).clamp(self.min, self.max)
    }
}

/// Emitted with the new value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomChanged(pub f32);

/// The control.
pub struct ZoomControl {
    id: ElementId,
    label: SharedString,
    range: ZoomRange,
    value: f32,
    slider: Entity<SliderState>,
    _subscription: Subscription,
}

impl EventEmitter<ZoomChanged> for ZoomControl {}

impl ZoomControl {
    /// A control labelled `label` (e.g. "Verse card size").
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        range: ZoomRange,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let value = value.clamp(range.min, range.max);
        let slider = cx.new(|_| {
            SliderState::new()
                .min(range.min)
                .max(range.max)
                .step(range.step)
                .default_value(value)
        });
        let subscription = cx.subscribe_in(
            &slider,
            window,
            |this, _, event: &SliderEvent, window, cx| {
                if let SliderEvent::Change(value) = event {
                    this.set(value.start(), window, cx);
                }
            },
        );
        Self {
            id: id.into(),
            label: label.into(),
            range,
            value,
            slider,
            _subscription: subscription,
        }
    }

    /// The current value.
    pub fn value(&self) -> f32 {
        self.value
    }

    fn set(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        let value = value.clamp(self.range.min, self.range.max);
        if value == self.value {
            return;
        }
        self.value = value;
        self.slider
            .update(cx, |slider, cx| slider.set_value(value, window, cx));
        cx.emit(ZoomChanged(value));
        cx.notify();
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), name.into())
    }
}

impl Render for ZoomControl {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let variant = ButtonCustomVariant::new(cx)
            .foreground(k.text(TextStep::T500))
            .hover(k.surface.opacity(0.0))
            .active(k.surface.opacity(0.0));
        let button = |cx: &mut Context<Self>,
                      id,
                      icon,
                      label: &'static str,
                      direction: i8,
                      enabled: bool| {
            Button::new(id)
                .custom(variant)
                .icon(Icon::new(icon).size_3())
                .small()
                .rounded(radius::sm())
                .accessibility_label(label)
                .disabled(!enabled)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    let next = this.range.bump(this.value, direction);
                    this.set(next, window, cx);
                }))
        };
        let smaller = button(
            cx,
            self.child_id("smaller"),
            KairoIcon::ZoomOut,
            "Smaller",
            -1,
            self.value > self.range.min,
        );
        let larger = button(
            cx,
            self.child_id("larger"),
            KairoIcon::ZoomIn,
            "Larger",
            1,
            self.value < self.range.max,
        );
        let tooltip: SharedString = format!("{} · double-click to reset", self.label).into();
        h_flex()
            .id(self.id.clone())
            .test_support()
            .role(Role::Group)
            .aria_label(self.label.clone())
            .flex_shrink_0()
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                if event.click_count() == 2 {
                    this.set(this.range.default, window, cx);
                }
            }))
            .child(smaller)
            .child(
                div()
                    .w_20()
                    .flex_shrink_0()
                    .px_1()
                    .child(Slider::new(&self.slider).accessibility_label(self.label.clone())),
            )
            .child(larger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_step_and_clamp() {
        let r = ZoomRange {
            min: 50.0,
            max: 200.0,
            default: 100.0,
            step: 5.0,
            button_step: 10.0,
        };
        assert_eq!(r.bump(100.0, 1), 110.0);
        assert_eq!(r.bump(195.0, 1), 200.0);
        assert_eq!(r.bump(55.0, -1), 50.0);
    }
}
