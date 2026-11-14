//! One verse card (`VerseThemePreview` in the grid): the slide on the dark
//! checkerboard with white text, no theme artwork (DESIGN.md), and the label
//! bar under it. Projected slides and the Live monitor keep the full theme;
//! the monitor is the engine's own frame (076).

use gpui_kit::base::TestSupportExt;
use gpui_kit::{
    App, Bounds, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window, canvas,
    div, fill, point, prelude::FluentBuilder, px, size,
};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, radius, text};

/// The checkerboard square, as the live monitor draws it.
const SQUARE: f32 = 12.0;

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct VerseCard {
    id: ElementId,
    reference: SharedString,
    /// The slide's reference line (with the translation when shown).
    slide_reference: SharedString,
    /// `None`: "Verse text unavailable".
    text: Option<SharedString>,
    number: usize,
    width: Pixels,
    slide_height: Pixels,
    live: bool,
    current: bool,
    sending: bool,
    on_click: Option<OnClick>,
}

impl VerseCard {
    pub fn new(id: impl Into<ElementId>, reference: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            reference: reference.into(),
            slide_reference: SharedString::default(),
            text: None,
            number: 1,
            width: px(180.),
            slide_height: px(101.),
            live: false,
            current: false,
            sending: false,
            on_click: None,
        }
    }
    pub fn slide(mut self, reference: impl Into<SharedString>, text: Option<SharedString>) -> Self {
        self.slide_reference = reference.into();
        self.text = text;
        self
    }
    pub fn number(mut self, number: usize) -> Self {
        self.number = number;
        self
    }
    pub fn size(mut self, width: f32, slide_height: f32) -> Self {
        self.width = px(width);
        self.slide_height = px(slide_height);
        self
    }
    /// Red frame: on screen.
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }
    /// White frame: the current or picked card.
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
    pub fn sending(mut self, sending: bool) -> Self {
        self.sending = sending;
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

/// Paints the checkerboard over `bounds`.
fn paint_checker(
    bounds: Bounds<Pixels>,
    light: gpui_kit::Hsla,
    dark: gpui_kit::Hsla,
    window: &mut Window,
) {
    window.paint_quad(fill(bounds, dark));
    let step = px(SQUARE);
    let cols = (f32::from(bounds.size.width) / SQUARE).ceil() as i32;
    let rows = (f32::from(bounds.size.height) / SQUARE).ceil() as i32;
    for y in 0..rows {
        for x in 0..cols {
            if (x + y) % 2 != 0 {
                continue;
            }
            let origin = point(
                bounds.origin.x + step * x as f32,
                bounds.origin.y + step * y as f32,
            );
            let w = step.min(bounds.origin.x + bounds.size.width - origin.x);
            let h = step.min(bounds.origin.y + bounds.size.height - origin.y);
            window.paint_quad(fill(Bounds::new(origin, size(w, h)), light));
        }
    }
}

impl RenderOnce for VerseCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let (light, dark) = (k.checker_light, k.checker_dark);
        let frame = if self.live {
            k.live
        } else if self.current {
            k.white
        } else {
            k.white.opacity(0.1)
        };
        // Text scales with the slide: a 1080-line slide's ~54 px body.
        let body = (f32::from(self.slide_height) * 0.07).clamp(7.0, 40.0);
        div()
            .id(self.id)
            .test_support()
            .role(Role::Button)
            .aria_label(SharedString::from(format!("Send {} live", self.reference)))
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(self.width)
            .overflow_hidden()
            .rounded(radius::md())
            .border_1()
            .border_color(frame)
            .bg(k.white.opacity(0.03))
            .when(!self.live && !self.current, |d| {
                d.hover(|s| s.border_color(k.white.opacity(0.3)))
            })
            .when_some(self.on_click, |d, f| d.on_click(f))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(self.slide_height)
                    .overflow_hidden()
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| paint_checker(bounds, light, dark, window),
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(body * 0.4))
                            .px(px(body))
                            .text_color(k.white)
                            .map(|d| match self.text {
                                Some(text) => d
                                    .child(
                                        div()
                                            .text_size(px(body))
                                            .line_height(px(body * 1.25))
                                            .text_center()
                                            .overflow_hidden()
                                            .max_h(self.slide_height * 0.72)
                                            .child(text),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(body * 0.7))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(self.slide_reference),
                                    ),
                                None => d.child(
                                    div()
                                        .text_size(text::LABEL.size)
                                        .italic()
                                        .text_color(k.text(TextStep::T500))
                                        .child("Verse text unavailable"),
                                ),
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(28.))
                    .px_2p5()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T300))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.reference),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(k.text(TextStep::T500))
                            .child(self.number.to_string()),
                    ),
            )
            .when(self.sending, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_1p5()
                        .right_1p5()
                        .px_1p5()
                        .py_0p5()
                        .bg(k.surface_elevated)
                        .text_size(text::MICRO.size)
                        .font_weight(FontWeight::BOLD)
                        .text_color(k.text_primary)
                        .child("SENDING"),
                )
            })
    }
}
