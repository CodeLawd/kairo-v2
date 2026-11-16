//! The pieces every pane is built from: `PrefGroup` (a titled grouped box),
//! `PrefRow` (label and hint on the left, the control on the right), the
//! switch, the Save bar and the small buttons. Colours come from the Kairo
//! tokens.

use crate::model::Section;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PrefGroup;
use kairo_ui_kit::tokens::{TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

/// A titled group: the heading and description above one grouped box.
pub fn group(
    title: Option<&str>,
    description: Option<&str>,
    rows: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    let k = cx.kairo();
    v_flex()
        .gap_2()
        .when_some(title, |this, title| {
            this.child(
                v_flex()
                    .px_0p5()
                    .child(
                        div()
                            .id(SharedString::from(format!("settings-group-{title}")))
                            .role(Role::Heading)
                            .text_size(text::SM.size)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child(title.to_owned()),
                    )
                    .when_some(description, |this, d| {
                        this.child(
                            div()
                                .mt_0p5()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T500))
                                .child(d.to_owned()),
                        )
                    }),
            )
        })
        .child(PrefGroup::new().rows(rows))
        .into_any_element()
}

fn label_block(label: &str, hint: Option<SharedString>, cx: &App) -> AnyElement {
    let k = cx.kairo();
    v_flex()
        .min_w_0()
        .child(
            div()
                .text_size(text::SM.size)
                .text_color(k.text_primary)
                .child(label.to_owned()),
        )
        .when_some(hint, |this, hint| {
            this.child(
                div()
                    .mt_1()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child(hint),
            )
        })
        .into_any_element()
}

/// A row: label and hint on the left, the control on the right.
pub fn row(
    label: &str,
    hint: Option<impl Into<SharedString>>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    h_flex()
        .min_h(px(44.))
        .items_center()
        .justify_between()
        .gap_3()
        .px(px(14.))
        .py_2()
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(label_block(label, hint.map(Into::into), cx)),
        )
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}

/// A stacked row: label and hint, then the control under them.
pub fn stacked(
    label: &str,
    hint: Option<impl Into<SharedString>>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    v_flex()
        .gap_2()
        .px(px(14.))
        .py(px(10.))
        .child(label_block(label, hint.map(Into::into), cx))
        .child(control)
        .into_any_element()
}

/// A switch (`Toggle`).
pub fn toggle(
    id: impl Into<ElementId>,
    label: &str,
    checked: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Switch::new(id)
        .checked(checked)
        .accessibility_label(label.to_owned())
        .on_change(move |value, window, cx| on_change(*value, window, cx))
}

/// The Save bar under a pane: "Save", then "Saved" for two seconds.
pub fn save_bar(
    section: Section,
    saved: bool,
    on_save: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let k = cx.kairo();
    h_flex()
        .justify_end()
        .pt_1()
        .child(
            Button::new(SharedString::from(crate::window::ids::save(section.id())))
                .cursor_pointer()
                .small()
                .icon(Icon::new(if saved {
                    KairoIcon::CheckCircle
                } else {
                    KairoIcon::Save
                }))
                .label(if saved { "Saved" } else { "Save" })
                .map(|b| if saved { b.primary() } else { b })
                .on_click(on_save),
        )
        .when(saved, |this| {
            this.child(
                div()
                    .id(SharedString::from(format!(
                        "settings-saved-{}",
                        section.id()
                    )))
                    .test_support()
                    .role(Role::Status)
                    .aria_label("Saved")
                    .size_0()
                    .text_color(k.success_text),
            )
        })
        .into_any_element()
}

/// A small secondary button (`btn-secondary text-[12px]`).
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .cursor_pointer()
        .kairo_size()
        .label(label)
        .disabled(disabled)
        .on_click(on_click)
}

/// The account avatar: initials on stone, or a person glyph.
pub fn avatar(initials: Option<String>, size: f32, cx: &App) -> AnyElement {
    let k = cx.kairo();
    div()
        .flex_shrink_0()
        .size(px(size))
        .rounded_full()
        .bg(k.stone)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px((size * 0.36).round()))
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(k.white)
        .map(|this| match initials {
            Some(initials) => this.child(initials),
            None => this.child(Icon::new(KairoIcon::CircleUser).size(px((size * 0.6).round()))),
        })
        .into_any_element()
}

/// A notice line with a warning glyph (`AlertTriangle` on a tertiary card).
pub fn notice(id: impl Into<ElementId>, message: impl Into<SharedString>, cx: &App) -> AnyElement {
    let k = cx.kairo();
    h_flex()
        .id(id)
        .test_support()
        .role(Role::Alert)
        .gap(px(10.))
        .items_center()
        .rounded(kairo_ui_kit::tokens::radius::lg())
        .bg(k.surface_tertiary)
        .px(px(14.))
        .py(px(10.))
        .text_size(text::XS.size)
        .text_color(k.text(TextStep::T300))
        .child(
            Icon::new(KairoIcon::AlertTriangle)
                .size(px(14.))
                .text_color(k.hue(kairo_ui_kit::Hue::Amber)),
        )
        .child(message.into())
        .into_any_element()
}

/// A muted line of text.
pub fn muted(text_: impl Into<SharedString>, cx: &App) -> AnyElement {
    let k = cx.kairo();
    div()
        .text_size(text::LABEL.size)
        .text_color(k.text(TextStep::T500))
        .child(text_.into())
        .into_any_element()
}
