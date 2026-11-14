//! Small presentation pieces the booth panels share: section titles, icon
//! buttons, the "eyebrow" headings and the dialog host. Colours come from
//! the Kairo tokens only.

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Div, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, Styled, Window, anchored, deferred, div, point, prelude::FluentBuilder, px,
};
use kairo_ui_kit::tokens::{Step, TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

/// `text-[10px] font-semibold uppercase tracking-[0.08em]`.
pub fn eyebrow(label: impl Into<SharedString>, cx: &App) -> Div {
    let k = cx.kairo();
    div()
        .text_size(text::MICRO.size)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(k.text(TextStep::T500))
        .child(label.into().to_uppercase())
}

/// A square ghost button with an icon and a spoken label.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: KairoIcon,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let k = cx.kairo();
    let label = label.into();
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(k.text(TextStep::T500))
                .hover(k.surface_elevated)
                .active(k.surface_elevated),
        )
        .xsmall()
        .icon(Icon::new(icon).size_3())
        .accessibility_label(label.clone())
        .tooltip(label)
}

/// The teal primary button (`btn-primary`).
pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let k = cx.kairo();
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(k.accent(Step::S500))
                .foreground(k.on_accent)
                .hover(k.accent(Step::S600))
                .active(k.accent(Step::S600)),
        )
        .small()
        .label(label.into())
}

/// The neutral secondary button (`btn-secondary`).
pub fn secondary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let k = cx.kairo();
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(k.surface_tertiary)
                .foreground(k.text(TextStep::T200))
                .hover(k.surface_elevated)
                .active(k.surface_elevated),
        )
        .small()
        .label(label.into())
}

/// A titled group (`Section` in `ProgramPanels.tsx`): the title and its
/// action over one rounded box.
pub fn section(
    title: &str,
    action: Option<AnyElement>,
    flat: bool,
    body: impl IntoElement,
    cx: &App,
) -> Div {
    let k = cx.kairo();
    v_flex()
        .gap_1p5()
        .px(if flat { px(16.) } else { px(12.) })
        .pt(if flat { px(16.) } else { px(12.) })
        .child(
            h_flex()
                .min_h(px(28.))
                .px_1()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(text::LABEL.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.text(TextStep::T400))
                        .child(title.to_uppercase()),
                )
                .when_some(action, |this, action| this.child(action)),
        )
        .child(
            div()
                .when(!flat, |this| {
                    this.rounded_lg()
                        .bg(k.surface)
                        .overflow_hidden()
                        .px(px(14.))
                        .py_3()
                })
                .when(flat, |this| this.px_1().pb_2())
                .child(body),
        )
}

/// One line of muted helper copy.
pub fn hint(copy: impl Into<SharedString>, cx: &App) -> Div {
    let k = cx.kairo();
    div()
        .text_size(text::MICRO.size)
        .text_color(k.text(TextStep::T500))
        .child(copy.into())
}

/// Draws `content` over the whole window, above every panel (Radix's
/// portal). The content is expected to be a `MovableDialog` frame.
pub fn window_layer(content: impl IntoElement, window: &Window) -> AnyElement {
    let viewport = window.viewport_size();
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("booth-dialog-layer")
                .test_support()
                .w(viewport.width)
                .h(viewport.height)
                .child(content),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

/// A plain tooltip with `copy`.
pub fn tip(
    copy: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> gpui_kit::AnyView + 'static {
    let copy = copy.into();
    move |window, cx| gpui_kit::component::tooltip::Tooltip::new(copy.clone()).build(window, cx)
}

/// `aria-pressed` for a toggle button.
pub trait PressedExt: gpui_kit::StatefulInteractiveElement + Sized {
    fn aria_toggled_bool(self, on: bool) -> Self {
        self.aria_toggled(if on {
            gpui_kit::Toggled::True
        } else {
            gpui_kit::Toggled::False
        })
    }
}
impl<E: gpui_kit::StatefulInteractiveElement> PressedExt for E {}
