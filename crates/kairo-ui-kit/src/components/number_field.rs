//! `NumberField` (`components/theme/NumberField.tsx`): a number box for the
//! theme inspector. Type a value, nudge it with the arrow keys or the up and
//! down buttons, or drag the label sideways to scrub it. Shift moves ten
//! steps at a time, for keys, buttons and scrubbing alike.

use crate::control::ControlSize;
use crate::icons::KairoIcon;
use crate::theme::KairoThemeExt;
use crate::tokens::{TextStep, radius, text};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    App, AppContext, Context, DragMoveEvent, ElementId, Empty, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder,
};

/// Pixels of horizontal drag on the label per step.
pub const SCRUB_PX_PER_STEP: f32 = 3.0;
/// Shift multiplies a step.
pub const SHIFT_MULTIPLIER: f64 = 10.0;

/// The value, step and bounds, without any UI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberModel {
    value: f64,
    step: f64,
    min: Option<f64>,
    max: Option<f64>,
}

/// Decimal places in `step`, as its shortest text shows them.
pub fn decimals_of(step: f64) -> usize {
    let text = format!("{step}");
    text.find('.').map_or(0, |dot| text.len() - dot - 1)
}

/// The number a user typed, or `None` while the text is not a number yet
/// (empty, `-`, `abc`). Infinite values are rejected.
pub fn parse_number(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok().filter(|n| n.is_finite())
}

impl NumberModel {
    /// A model. A non-finite value reads as 0, as `Number.isFinite` does in
    /// the original. A non-positive or non-finite step falls back to 1.
    pub fn new(value: f64, step: f64) -> Self {
        Self {
            value: if value.is_finite() { value } else { 0.0 },
            step: if step.is_finite() && step > 0.0 {
                step
            } else {
                1.0
            },
            min: None,
            max: None,
        }
    }

    /// Lower bound.
    pub fn min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    /// Upper bound.
    pub fn max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// The current value.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Replaces the value without the change rules (an outside update).
    pub fn set_value(&mut self, value: f64) {
        self.value = if value.is_finite() { value } else { 0.0 };
    }

    /// Rounds to the step's decimals and clamps. Returns the new value only
    /// when it differs from the current one, as the original calls
    /// `onChange` only then.
    pub fn commit(&mut self, next: f64) -> Option<f64> {
        if !next.is_finite() {
            return None;
        }
        let places = decimals_of(self.step) as i32;
        let factor = 10f64.powi(places);
        let mut clamped = (next * factor).round() / factor;
        if let Some(min) = self.min {
            clamped = clamped.max(min);
        }
        if let Some(max) = self.max {
            clamped = clamped.min(max);
        }
        if clamped == self.value {
            return None;
        }
        self.value = clamped;
        Some(clamped)
    }

    /// One step up (`1`) or down (`-1`), ten with Shift.
    pub fn nudge(&mut self, direction: i8, shift: bool) -> Option<f64> {
        let multiplier = if shift { SHIFT_MULTIPLIER } else { 1.0 };
        self.commit(self.value + f64::from(direction.signum()) * self.step * multiplier)
    }

    /// The value for a scrub that started at `start` and has moved `dx`
    /// pixels.
    pub fn scrub(&mut self, start: f64, dx: f32, shift: bool) -> Option<f64> {
        let steps = (dx / SCRUB_PX_PER_STEP).round() as f64;
        let multiplier = if shift { SHIFT_MULTIPLIER } else { 1.0 };
        self.commit(start + steps * self.step * multiplier)
    }

    /// Whether the increase button can act.
    pub fn can_increase(&self) -> bool {
        self.max.is_none_or(|max| self.value < max)
    }

    /// Whether the decrease button can act.
    pub fn can_decrease(&self) -> bool {
        self.min.is_none_or(|min| self.value > min)
    }

    /// The value as the field shows it: the shortest text for the number,
    /// as a browser number input shows `value={1}` as `1`.
    pub fn display(&self) -> String {
        let places = decimals_of(self.step) as i32;
        let factor = 10f64.powi(places);
        let rounded = (self.value * factor).round() / factor;
        // `-0` prints as `0`, as `String(-0)` does.
        let rounded = if rounded == 0.0 { 0.0 } else { rounded };
        format!("{rounded}")
    }
}

/// Emitted when the value changes through the field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberFieldChanged(pub f64);

/// The drag payload while scrubbing the label.
#[derive(Clone, Copy)]
struct Scrub {
    start: f64,
    origin_x: f32,
}

impl Render for Scrub {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// The field.
pub struct NumberField {
    id: ElementId,
    label: SharedString,
    suffix: Option<SharedString>,
    icon: Option<KairoIcon>,
    model: NumberModel,
    input: Entity<InputState>,
    _subscription: Subscription,
}

impl EventEmitter<NumberFieldChanged> for NumberField {}

impl NumberField {
    /// A field for `model`, labelled `label`.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        model: NumberModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(model.display()));
        let subscription = cx.subscribe_in(&input, window, Self::on_input);
        Self {
            id: id.into(),
            label: label.into(),
            suffix: None,
            icon: None,
            model,
            input,
            _subscription: subscription,
        }
    }

    /// A unit shown after the number (`px`, `%`).
    pub fn with_suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// A leading icon.
    pub fn with_icon(mut self, icon: KairoIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The current value.
    pub fn value(&self) -> f64 {
        self.model.value()
    }

    /// Sets the value from outside (undo, another control). Does not emit.
    pub fn set_value(&mut self, value: f64, window: &mut Window, cx: &mut Context<Self>) {
        self.model.set_value(value);
        self.sync_text(window, cx);
        cx.notify();
    }

    fn sync_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.model.display();
        self.input.update(cx, |input, cx| {
            if input.value() != text.as_str() {
                input.set_value(text, window, cx);
            }
        });
    }

    fn apply(&mut self, changed: Option<f64>, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_text(window, cx);
        if let Some(value) = changed {
            cx.emit(NumberFieldChanged(value));
        }
        cx.notify();
    }

    fn on_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                // Text that is not a number yet is left alone while typing.
                if let Some(next) = parse_number(&input.read(cx).value())
                    && let Some(value) = self.model.commit(next)
                {
                    cx.emit(NumberFieldChanged(value));
                    cx.notify();
                }
            }
            // Leaving the field shows the value it settled on.
            InputEvent::Blur | InputEvent::PressEnter { .. } => self.sync_text(window, cx),
            InputEvent::Focus => {}
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let direction = match event.keystroke.key.as_str() {
            "up" => 1,
            "down" => -1,
            _ => return,
        };
        cx.stop_propagation();
        window.prevent_default();
        let changed = self.model.nudge(direction, event.keystroke.modifiers.shift);
        self.apply(changed, window, cx);
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), name.into())
    }
}

impl Focusable for NumberField {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl Render for NumberField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let stepper =
            |cx: &mut Context<Self>, direction: i8, id: ElementId, enabled: bool, label: String| {
                Button::new(id)
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .foreground(k.text(TextStep::T500))
                            .hover(k.surface_elevated)
                            .active(k.surface_elevated),
                    )
                    .icon(Icon::new(if direction > 0 {
                        KairoIcon::ChevronUp
                    } else {
                        KairoIcon::ChevronDown
                    }))
                    .xsmall()
                    .flex_1()
                    .px_1()
                    .tab_stop(false)
                    .accessibility_label(label)
                    .disabled(!enabled)
                    .on_click(
                        cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                            let changed = this.model.nudge(direction, event.modifiers().shift);
                            this.apply(changed, window, cx);
                        }),
                    )
            };
        let up = stepper(
            cx,
            1,
            self.child_id("increase"),
            self.model.can_increase(),
            format!("Increase {}", self.label),
        );
        let down = stepper(
            cx,
            -1,
            self.child_id("decrease"),
            self.model.can_decrease(),
            format!("Decrease {}", self.label),
        );
        let start = self.model.value();
        v_flex()
            .id(self.id.clone())
            .min_w_0()
            .child(
                div()
                    .id(self.child_id("label"))
                    .mb_1()
                    .cursor_ew_resize()
                    .text_size(text::LABEL.size)
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(k.text(TextStep::T500))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Drag to adjust")
                            .build(window, cx)
                    })
                    .child(self.label.clone())
                    .on_drag(
                        Scrub {
                            start,
                            origin_x: 0.0,
                        },
                        |scrub, position, _, cx| {
                            let scrub = Scrub {
                                origin_x: f32::from(position.x),
                                ..*scrub
                            };
                            cx.new(|_| scrub)
                        },
                    )
                    .on_drag_move(
                        cx.listener(|this, event: &DragMoveEvent<Scrub>, window, cx| {
                            let scrub = *event.drag(cx);
                            let dx = f32::from(event.event.position.x - event.bounds.origin.x)
                                - scrub.origin_x;
                            let changed =
                                this.model
                                    .scrub(scrub.start, dx, event.event.modifiers.shift);
                            this.apply(changed, window, cx);
                        }),
                    ),
            )
            .child(
                h_flex()
                    .h_8()
                    .gap_2()
                    .rounded(radius::base())
                    .bg(k.surface_tertiary)
                    .pl_2p5()
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T200))
                    .capture_key_down(cx.listener(Self::on_key))
                    .when_some(self.icon, |this, icon| {
                        this.child(
                            Icon::new(icon)
                                .size_3p5()
                                .text_color(k.text(TextStep::T500)),
                        )
                    })
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.input)
                                .kairo_size()
                                .id(self.child_id("input"))
                                .appearance(false)
                                .small()
                                .aria_label(self.label.clone()),
                        ),
                    )
                    .when_some(self.suffix.clone(), |this, suffix| {
                        this.child(
                            div()
                                .flex_shrink_0()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T500))
                                .child(suffix),
                        )
                    })
                    .child(
                        v_flex()
                            .h_full()
                            .flex_shrink_0()
                            .py_0p5()
                            .pr_0p5()
                            .child(up)
                            .child(down),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_follow_the_step() {
        assert_eq!(decimals_of(1.0), 0);
        assert_eq!(decimals_of(0.05), 2);
        assert_eq!(decimals_of(0.1), 1);
    }

    #[test]
    fn commit_rounds_clamps_and_skips_no_change() {
        let mut m = NumberModel::new(5.0, 0.1).min(0.0).max(10.0);
        assert_eq!(m.commit(5.04), None);
        assert_eq!(m.commit(5.06), Some(5.1));
        assert_eq!(m.commit(99.0), Some(10.0));
        assert_eq!(m.commit(-3.0), Some(0.0));
        assert_eq!(m.commit(f64::NAN), None);
    }

    #[test]
    fn shift_nudges_ten_steps_and_stops_at_bounds() {
        let mut m = NumberModel::new(0.0, 1.0).min(0.0).max(15.0);
        assert_eq!(m.nudge(1, false), Some(1.0));
        assert_eq!(m.nudge(1, true), Some(11.0));
        assert_eq!(m.nudge(1, true), Some(15.0));
        assert_eq!(m.nudge(1, false), None);
        assert!(!m.can_increase());
        assert!(m.can_decrease());
    }

    #[test]
    fn scrub_moves_one_step_per_three_pixels() {
        let mut m = NumberModel::new(10.0, 1.0);
        assert_eq!(m.scrub(10.0, 9.0, false), Some(13.0));
        assert_eq!(m.scrub(10.0, -3.0, true), Some(0.0));
        assert_eq!(m.scrub(10.0, 1.0, false), Some(10.0));
    }

    #[test]
    fn non_finite_inputs_fall_back() {
        let m = NumberModel::new(f64::INFINITY, 0.0);
        assert_eq!(m.value(), 0.0);
        assert_eq!(m.step, 1.0);
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("-"), None);
        assert_eq!(parse_number("1e999"), None);
        assert_eq!(parse_number(" 2.5 "), Some(2.5));
    }

    #[test]
    fn display_uses_the_step_precision() {
        assert_eq!(NumberModel::new(1.0, 0.05).display(), "1");
        assert_eq!(NumberModel::new(0.1 + 0.2, 0.1).display(), "0.3");
        assert_eq!(NumberModel::new(-0.0, 1.0).display(), "0");
        assert_eq!(NumberModel::new(12.0, 1.0).display(), "12");
    }
}
