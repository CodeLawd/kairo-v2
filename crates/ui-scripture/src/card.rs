//! One verse card (`VerseThemePreview` in the grid): the slide over the
//! projected theme's background (the checkerboard when it is transparent),
//! and a black label bar under it. Square, borderless unless live or picked
//! (owner design, 2026-11-16, DECISIONS 113). Projected slides and the Live monitor keep the full theme;
//! the monitor is the engine's own frame (076).

use gpui_kit::StyledImage;
use gpui_kit::base::TestSupportExt;
use gpui_kit::{
    App, Bounds, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window, canvas,
    div, fill, point, prelude::FluentBuilder, px, size,
};
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, text};

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
    background: crate::backend::CardBackground,
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
            background: Default::default(),
            on_click: None,
        }
    }
    /// The theme background drawn under the text.
    pub fn background(mut self, background: crate::backend::CardBackground) -> Self {
        self.background = background;
        self
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
            k.white.opacity(0.12)
        };
        let background = self.background.clone();
        let themed = background.theme.clone();
        let checker = background.image.is_none() && background.color.is_none();
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
            // A hairline at rest; live and picked cards keep a full border.
            .border(if self.live || self.current {
                px(1.)
            } else {
                px(0.5)
            })
            .border_color(frame)
            .bg(gpui_kit::black())
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
                    .when(checker, |d| {
                        d.child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    paint_checker(bounds, light, dark, window)
                                },
                            )
                            .absolute()
                            .size_full(),
                        )
                    })
                    .when_some(background.color, |d, rgb| {
                        d.child(
                            div()
                                .absolute()
                                .size_full()
                                .bg(gpui_kit::rgb(rgb))
                                .opacity(background.opacity),
                        )
                    })
                    .when_some(background.image.clone(), |d, path| {
                        d.child(
                            gpui_kit::img(std::path::PathBuf::from(path))
                                .absolute()
                                .size_full()
                                .object_fit(gpui_kit::ObjectFit::Cover)
                                .opacity(background.opacity),
                        )
                    })
                    // A scrim over the artwork so white text reads clearly.
                    .when(!checker, |d| {
                        d.child(
                            div()
                                .absolute()
                                .size_full()
                                .bg(gpui_kit::black().opacity(0.4)),
                        )
                    })
                    .map(|d| match (themed, self.text.clone()) {
                        // As the output lays it out: the theme's verse and
                        // reference frames, fonts and colours at card scale.
                        (Some(theme), Some(text)) => d.child(themed_text(
                            &theme,
                            text,
                            self.slide_reference.clone(),
                            self.width,
                            self.slide_height,
                        )),
                        _ => d.child(
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
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(34.))
                    .px_3()
                    .bg(gpui_kit::black())
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T200))
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

/// `#rrggbb` (or `#rgb`) with an opacity.
fn css_color(value: &str, opacity: f64) -> Option<gpui_kit::Hsla> {
    let hex = value.trim().trim_start_matches('#');
    let hex = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => hex.to_owned(),
        _ => return None,
    };
    let rgb = u32::from_str_radix(&hex, 16).ok()?;
    Some(gpui_kit::Hsla::from(gpui_kit::rgb(rgb)).opacity(opacity.clamp(0.0, 1.0) as f32))
}

/// A slide's verse and reference sizes, when shown.
type Sizes = (Option<f32>, Option<f32>);

/// The verse and reference font sizes the output uses at 1920 × 1080: the
/// engine's own layout (`kairo_scene::analyse`, with auto-fit), cached per
/// slide since shaping is too slow to repeat every frame.
fn output_sizes(theme: &kairo_domain::overlay::OverlayTheme, reference: &str, text: &str) -> Sizes {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static SIZES: RefCell<HashMap<u64, Sizes>> =
            RefCell::new(HashMap::new());
    }
    let scene = kairo_scene::SlideScene::build(theme, reference, text, (1920, 1080), &[]);
    let key = scene.content_hash();
    if let Some(hit) = SIZES.with(|s| s.borrow().get(&key).copied()) {
        return hit;
    }
    let mut sizes = (None, None);
    for b in kairo_scene::analyse(&scene) {
        match b.kind {
            kairo_scene::BoxKind::Verse => sizes.0 = Some(b.size),
            kairo_scene::BoxKind::Reference => sizes.1 = Some(b.size),
        }
    }
    SIZES.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() > 4096 {
            s.clear();
        }
        s.insert(key, sizes);
    });
    sizes
}

/// One text box at its theme frame, scaled from 1920 × 1080; `output_px` is
/// the size the output draws it at.
fn text_box(
    style: &kairo_domain::overlay::OverlayTextStyle,
    text: SharedString,
    scale: f32,
    output_px: Option<f32>,
) -> gpui_kit::Div {
    use kairo_domain::overlay::{OverlayTextAlign, OverlayVerticalAlign};
    let frame = &style.frame;
    let size = (output_px.unwrap_or(style.font_size_px as f32) * scale).max(4.0);
    let line = style.line_height.max(0.8) as f32;
    let color = css_color(&style.color, style.color_opacity).unwrap_or(gpui_kit::white());
    div()
        .absolute()
        .left(gpui_kit::relative((frame.x_pct / 100.0) as f32))
        .top(gpui_kit::relative((frame.y_pct / 100.0) as f32))
        .w(gpui_kit::relative((frame.width_pct / 100.0) as f32))
        .h(gpui_kit::relative((frame.height_pct / 100.0) as f32))
        .flex()
        .flex_col()
        .overflow_hidden()
        .map(|d| match style.vertical_align {
            OverlayVerticalAlign::Top => d.justify_start(),
            OverlayVerticalAlign::Bottom => d.justify_end(),
            _ => d.justify_center(),
        })
        .child(
            div()
                .w_full()
                .when(!style.font_family.trim().is_empty(), |d| {
                    d.font_family(SharedString::from(
                        style
                            .font_family
                            .split(',')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .trim_matches('"')
                            .to_owned(),
                    ))
                })
                .text_size(px(size))
                .line_height(px(size * line))
                .font_weight(FontWeight(style.font_weight as f32))
                .text_color(color)
                .map(|d| match style.align {
                    OverlayTextAlign::Left => d.text_left(),
                    OverlayTextAlign::Right => d.text_right(),
                    _ => d.text_center(),
                })
                .child(text),
        )
}

/// The verse and its reference, laid out by `theme` on a `width` slide.
fn themed_text(
    theme: &kairo_domain::overlay::OverlayTheme,
    text: SharedString,
    reference: SharedString,
    width: Pixels,
    height: Pixels,
) -> gpui_kit::AnyElement {
    let scale = (f32::from(width) / 1920.0).min(f32::from(height) / 1080.0);
    let (verse_px, reference_px) = output_sizes(theme, &reference, &text);
    div()
        .absolute()
        .size_full()
        .child(text_box(&theme.verse, text, scale, verse_px))
        .when(theme.reference.show, |d| {
            d.child(text_box(
                &theme.reference.text,
                reference,
                scale,
                reference_px,
            ))
        })
        .into_any_element()
}
