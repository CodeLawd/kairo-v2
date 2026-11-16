//! Shared full-window surface for every step before the booth.
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::{Disableable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, ParentElement, Role, SharedString,
    StatefulInteractiveElement, Styled, div, rems,
};
use kairo_ui_kit::brand::{KairoMark, MarkSize};
use kairo_ui_kit::onboarding::{OnboardingColors, StageGlow};

pub fn surface(ob: &OnboardingColors) -> gpui_kit::Div {
    v_flex()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .bg(ob.bg)
        .text_color(ob.text)
        .child(StageGlow::new(ob))
}

pub fn top_bar() -> gpui_kit::Div {
    h_flex()
        .relative()
        .h(rems(4.))
        .flex_shrink_0()
        .pl(rems(5.5))
        .pr_6()
        .gap_4()
        .items_center()
        .child(
            div()
                .id("account-stage-drag")
                .occlude()
                .absolute()
                .inset_0()
                .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
        )
}

pub fn form_page(
    title_id: &'static str,
    title: String,
    blurb: String,
    body: AnyElement,
    ob: &OnboardingColors,
    progress: f32,
) -> AnyElement {
    let form = v_flex()
        .my_auto()
        .flex_shrink_0()
        .w_full()
        .max_w(rems(28.))
        .mx_auto()
        .py_8()
        .px_6()
        .items_center()
        .child(entry(
            div().child(KairoMark::new(MarkSize::Medium)),
            progress,
            0,
        ))
        .child(entry(
            div()
                .id(title_id)
                .test_support()
                .role(Role::Heading)
                .aria_label(SharedString::from(title.clone()))
                .mt_6()
                .text_size(rems(2.))
                .text_center()
                .child(title),
            progress,
            1,
        ))
        .child(
            div()
                .mt_2()
                .text_size(rems(0.875))
                .text_color(ob.text_deep)
                .text_center()
                .child(blurb),
        )
        .child(entry(
            div().mt_8().w_full().max_w(rems(22.5)).child(body),
            progress,
            2,
        ));
    v_flex()
        .min_h_full()
        .w_full()
        .items_center()
        .child(form)
        .into_any_element()
}

pub fn quiet(id: &'static str, label: &'static str, disabled: bool) -> Button {
    Button::new(id)
        .cursor_pointer()
        .ghost()
        .label(label)
        .disabled(disabled)
}

pub fn primary(
    id: &'static str,
    label: &'static str,
    disabled: bool,
    ob: &OnboardingColors,
    cx: &gpui_kit::App,
) -> Button {
    Button::new(id)
        .cursor_pointer()
        .custom(
            ButtonCustomVariant::new(cx)
                .color(ob.cta.0)
                .foreground(ob.cta.1)
                .hover(ob.cta.2)
                .active(ob.cta.2),
        )
        .label(label)
        .disabled(disabled)
        .min_w(rems(13.75))
        .h(rems(2.5))
        .bg(ob.cta.0)
        .text_color(ob.cta.1)
}

fn entry<E: Styled>(el: E, progress: f32, ix: usize) -> E {
    let progress = crate::fluid::staged(progress, ix, 3);
    el.relative()
        .top(rems(1.5 * (1. - progress)))
        .opacity(progress)
}
