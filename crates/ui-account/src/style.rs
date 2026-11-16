//! Shared bits of the account views' look (`.label`, the quiet text
//! buttons, the error and notice lines). Colours come from the Kairo tokens.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Disableable, Sizable};
use gpui_kit::rems;
use gpui_kit::{
    App, ClickEvent, Div, ElementId, Hsla, ParentElement, SharedString, Styled, Window, div,
};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, text};

/// `.label`: a field's name above it.
pub fn label(text_: impl Into<SharedString>, cx: &App) -> Div {
    let k = cx.kairo();
    div()
        .mb(rems(0.375))
        .text_size(text::XS.size)
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .text_color(k.text(TextStep::T300))
        .child(text_.into())
}

/// A quiet text button (`text-[12px] text-slate-500 hover:text-slate-300`).
pub fn text_button(
    id: impl Into<ElementId>,
    label_: impl Into<SharedString>,
    color: Hsla,
    _hover: Hsla,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .cursor_pointer()
        .label(label_)
        .text_color(color)
        .disabled(disabled)
        .h_auto()
        .px_0()
        .py_0()
        .on_click(on_click)
}

/// The error colour (`text-rose-400`).
pub fn error_color(cx: &App) -> Hsla {
    cx.kairo().danger_text
}

/// Muted copy (`text-slate-500`).
pub fn muted(cx: &App) -> Hsla {
    cx.kairo().text(TextStep::T500)
}

/// Brighter copy (`text-slate-300`).
pub fn bright(cx: &App) -> Hsla {
    cx.kairo().text(TextStep::T300)
}

/// `encodeURIComponent` for the characters a user code can hold.
pub fn encode_uri_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_like_encode_uri_component() {
        assert_eq!(super::encode_uri_component("PROA-7K2X"), "PROA-7K2X");
        assert_eq!(super::encode_uri_component("a b/c"), "a%20b%2Fc");
    }
}
