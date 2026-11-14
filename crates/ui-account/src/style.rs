//! Shared bits of the account views' look (`.label`, the quiet text
//! buttons, the error and notice lines). Colours come from the Kairo tokens.

use gpui_kit::base::TestSupportExt;
use gpui_kit::{
    App, ClickEvent, Div, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, text};

/// `.label`: a field's name above it.
pub fn label(text_: impl Into<SharedString>, cx: &App) -> Div {
    let k = cx.kairo();
    div()
        .mb(px(6.))
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
    hover: Hsla,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl Styled + IntoElement {
    let label_ = label_.into();
    let el = div()
        .id(id)
        .test_support()
        .role(Role::Button)
        .aria_label(label_.clone())
        .text_color(color)
        .child(label_);
    if disabled {
        el.opacity(0.5)
    } else {
        el.cursor_pointer()
            .hover(move |s| s.text_color(hover))
            .on_click(on_click)
    }
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
