//! The generation placeholder, after beUI's image-generation loader: a field
//! of dots that swell around a slowly drifting point, a two-by-two status
//! mark, the status and the prompt. Drawn while OpenAI works; the finished
//! image replaces it.

use std::time::Instant;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    Bounds, Corners, Hsla, IntoElement, ParentElement, Styled, canvas, div, fill, point,
    prelude::FluentBuilder, px, size,
};
use kairo_ui_kit::KairoColors;
use kairo_ui_kit::tokens::{TextStep, text};

const DOT_GAP: f32 = 10.;

/// Milliseconds of animation since `started`; frozen at 0 for reduced motion.
pub fn clock(started: Instant, reduced: bool) -> f32 {
    if reduced {
        0.
    } else {
        started.elapsed().as_millis() as f32
    }
}

/// One dot: offset from the grid point, radius and alpha, for a dot at
/// `(x, y)` with the swell centred at `(tx, ty)` of `radius`.
pub fn dot(x: f32, y: f32, tx: f32, ty: f32, radius: f32) -> ((f32, f32), f32, f32) {
    let (dx, dy) = (x - tx, y - ty);
    let dist = (dx * dx + dy * dy).sqrt();
    let p = (1. - dist / radius).max(0.);
    let influence = p * p * (3. - 2. * p);
    let push = influence * influence * 9.;
    let offset = if dist > 0. {
        (dx / dist * push, dy / dist * push)
    } else {
        (0., 0.)
    };
    (offset, 0.65 + influence * 0.85, 0.17 + influence * 0.72)
}

/// The dot field, filling its parent.
pub fn dot_field(t: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let tx = w / 2. + (t / 1700.).sin() * w * 0.12;
            let ty = h / 2. + (t / 2100.).cos() * h * 0.1;
            let radius = w.min(h) * 0.38;
            let cols = (w / DOT_GAP).ceil() as usize + 1;
            let rows = (h / DOT_GAP).ceil() as usize + 1;
            let x0 = (w - (cols - 1) as f32 * DOT_GAP) / 2.;
            let y0 = (h - (rows - 1) as f32 * DOT_GAP) / 2.;
            for row in 0..rows {
                for col in 0..cols {
                    let (x, y) = (x0 + col as f32 * DOT_GAP, y0 + row as f32 * DOT_GAP);
                    let ((nx, ny), r, alpha) = dot(x, y, tx, ty, radius);
                    let at = Bounds::new(
                        point(px(ox + x + nx - r), px(oy + y + ny - r)),
                        size(px(r * 2.), px(r * 2.)),
                    );
                    window.paint_quad(
                        fill(at, color.opacity(alpha)).corner_radii(Corners::all(px(r))),
                    );
                }
            }
        },
    )
    .absolute()
    .size_full()
}

/// The 14 px mark: two diagonal squares lit, swapping every 600 ms.
pub fn status_mark(t: f32, color: Hsla) -> impl IntoElement {
    let lit = ((t / 600.) as u32).is_multiple_of(2);
    let square = |on: bool| {
        div()
            .size(px(4.))
            .bg(color.opacity(if on { 1. } else { 0.4 }))
    };
    v_flex()
        .flex_none()
        .size(px(14.))
        .p(px(2.))
        .gap(px(2.))
        .child(h_flex().gap(px(2.)).child(square(lit)).child(square(!lit)))
        .child(h_flex().gap(px(2.)).child(square(!lit)).child(square(lit)))
}

/// The status line and the prompt in curly quotes, for the card's foot.
pub fn status(t: f32, status: &str, prompt: &str, k: &KairoColors) -> impl IntoElement {
    v_flex()
        .gap_0p5()
        .min_w_0()
        .child(
            h_flex()
                .gap_2()
                .text_size(text::SM.size)
                .text_color(k.text_primary)
                .child(status_mark(t, k.text_primary))
                .child(status.to_owned()),
        )
        .when(!prompt.is_empty(), |d| {
            d.child(
                div()
                    .truncate()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child(format!("“{prompt}”")),
            )
        })
}

/// The output size badge.
pub fn badge(label: &str, k: &KairoColors) -> impl IntoElement {
    div()
        .px_2()
        .py_0p5()
        .rounded_full()
        .bg(k.surface.opacity(0.75))
        .text_size(px(10.))
        .text_color(k.text(TextStep::T500))
        .child(label.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_swell_near_the_centre_and_rest_far_away() {
        let ((nx, _), r, alpha) = dot(105., 100., 100., 100., 50.);
        assert!(nx > 0., "pushed outward");
        assert!(r > 1.3 && alpha > 0.8);
        let (offset, r, alpha) = dot(500., 500., 100., 100., 50.);
        assert_eq!(offset, (0., 0.));
        assert!((r - 0.65).abs() < 1e-6 && (alpha - 0.17).abs() < 1e-6);
        assert_eq!(clock(Instant::now(), true), 0.);
    }
}
