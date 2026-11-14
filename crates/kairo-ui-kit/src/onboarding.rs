//! First-run setup's own palette (`onboarding.css`): the website's near-black
//! with a blue-violet stage glow, softer white text, filled cards, and the
//! fixed colours the tour's mock-ups draw slides and song groups in. Light
//! theme has its own values, as the stylesheet does.
//!
//! [`StageGlow`] draws the `--ob-glow` radial gradients. GPUI has no radial
//! gradient, so each ellipse is painted as rings of falling opacity.

use crate::tokens::Appearance;
use gpui_kit::{
    App, Bounds, Hsla, IntoElement, PathBuilder, Pixels, RenderOnce, Rgba, Styled, Window, canvas,
    point, px, rgb, rgba,
};

/// One `radial-gradient(ellipse W% H% at X% Y%, colour, transparent 70%)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    /// Ellipse radii as fractions of the box.
    pub radius: (f32, f32),
    /// Centre as fractions of the box.
    pub at: (f32, f32),
    /// The colour at the centre, with its alpha.
    pub color: Hsla,
}

/// Setup's colours for one appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnboardingColors {
    /// `--ob-bg`.
    pub bg: Hsla,
    /// `--ob-edge`: the dialog's hairline.
    pub edge: Hsla,
    /// `--ob-text`.
    pub text: Hsla,
    /// `--ob-text-deep`.
    pub text_deep: Hsla,
    /// `--ob-card`, `--ob-card-2`, `--ob-card-3`.
    pub card: [Hsla; 3],
    /// `--ob-blue`: focus, progress, callout lines.
    pub blue: Hsla,
    /// `--ob-blue-soft`: tour bullets.
    pub blue_soft: Hsla,
    /// `--ob-glow`.
    pub glows: [Option<Glow>; 3],
    /// `.btn-primary` inside setup: fill, text, hover.
    pub cta: (Hsla, Hsla, Hsla),
    /// The scrim (`bg-black/75`).
    pub scrim: Hsla,
    /// Mock-up slide text (`#F6F3EB`) and its muted line (`#A29F96`).
    pub slide_text: (Hsla, Hsla),
    /// Thumbnail text (`#ECECE9`).
    pub thumb_text: Hsla,
    /// `.onboarding-audience-slide` default (`#232B31`).
    pub slide_bg: Hsla,
    /// `bg-ink`: the lyric and stage mock-ups.
    pub ink: Hsla,
    /// The mock-ups' backgrounds, in the order the source lists them:
    /// `#313E4C`, `#232B31`, `#36362D`, `#292922`, `#414037`, `#3F5268`,
    /// `#0C111D`.
    pub art: [Hsla; 7],
    /// Song group colours: verse `#3B63D9`, chorus `#C8372D`, bridge `#7C3AED`.
    pub groups: [Hsla; 3],
}

fn c(value: u32) -> Hsla {
    rgb(value).into()
}

fn ca(value: u32, alpha: f32) -> Hsla {
    let mut color: Hsla = rgb(value).into();
    color.a = alpha;
    color
}

const ART: [u32; 7] = [
    0x313E4C, 0x232B31, 0x36362D, 0x292922, 0x414037, 0x3F5268, 0x0C111D,
];

/// The palette for `appearance`.
pub fn colors(appearance: Appearance) -> OnboardingColors {
    let art = ART.map(c);
    let groups = [c(0x3B63D9), c(0xC8372D), c(0x7C3AED)];
    let shared = |bg, edge, text, text_deep, card, blue_soft, glows, cta| OnboardingColors {
        bg,
        edge,
        text,
        text_deep,
        card,
        blue: c(0x3B6BFF),
        blue_soft,
        glows,
        cta,
        scrim: rgba(0x000000bf).into(),
        slide_text: (c(0xF6F3EB), c(0xA29F96)),
        thumb_text: c(0xECECE9),
        slide_bg: c(0x232B31),
        ink: c(0x0A0B10),
        art,
        groups,
    };
    if appearance.is_dark() {
        shared(
            c(0x0A0B10),
            ca(0xFFFFFF, 0.16),
            c(0xF6F3EB),
            c(0x7D8399),
            [c(0x181B23), c(0x1D212B), c(0x252936)],
            c(0x8AB4FF),
            [
                Some(Glow {
                    radius: (0.55, 0.6),
                    at: (0.72, 0.55),
                    color: ca(0x3B6BFF, 0.22),
                }),
                Some(Glow {
                    radius: (0.35, 0.4),
                    at: (0.9, 0.88),
                    color: ca(0x7B5CFF, 0.18),
                }),
                Some(Glow {
                    radius: (0.6, 0.7),
                    at: (0.0, 0.9),
                    color: ca(0x2554DC, 0.14),
                }),
            ],
            (c(0xF4F0E9), c(0x0A0B10), c(0xE9E5DE)),
        )
    } else {
        shared(
            c(0xF6F7F9),
            ca(0x0A0B10, 0.14),
            c(0x0A0B10),
            c(0x565B6E),
            [c(0xFFFFFF), c(0xECEEF3), c(0xE1E4EB)],
            c(0x2F5BE0),
            [
                Some(Glow {
                    radius: (0.55, 0.6),
                    at: (0.72, 0.55),
                    color: ca(0x3B6BFF, 0.12),
                }),
                Some(Glow {
                    radius: (0.35, 0.4),
                    at: (0.9, 0.88),
                    color: ca(0x7B5CFF, 0.1),
                }),
                None,
            ],
            (c(0x0A0B10), c(0xF6F3EB), c(0x262934)),
        )
    }
}

/// Rings per glow.
const RINGS: usize = 14;

/// The opacity of ring `i` of `RINGS`, outermost first, so the painted
/// stack approximates a linear fall from the centre to transparent at 70%.
fn ring_alpha(alpha: f32) -> f32 {
    // Stacking n rings of a gives 1 - (1 - a)^n at the centre.
    1.0 - (1.0 - alpha).powf(1.0 / RINGS as f32)
}

fn ellipse(centre: (f32, f32), radius: (f32, f32)) -> Option<gpui_kit::Path<Pixels>> {
    let (cx, cy) = centre;
    let (rx, ry) = radius;
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let mut builder = PathBuilder::fill();
    builder.move_to(point(px(cx + rx), px(cy)));
    builder.arc_to(
        point(px(rx), px(ry)),
        px(0.),
        false,
        true,
        point(px(cx - rx), px(cy)),
    );
    builder.arc_to(
        point(px(rx), px(ry)),
        px(0.),
        false,
        true,
        point(px(cx + rx), px(cy)),
    );
    builder.close();
    builder.build().ok()
}

fn paint_glow(bounds: Bounds<Pixels>, glow: Glow, window: &mut Window) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let centre = (
        f32::from(bounds.origin.x) + glow.at.0 * w,
        f32::from(bounds.origin.y) + glow.at.1 * h,
    );
    // `ellipse W% H%`: percentages of the box; the colour fades out by 70%.
    let (rx, ry) = (glow.radius.0 * w * 0.7, glow.radius.1 * h * 0.7);
    let mut color = glow.color;
    color.a = ring_alpha(glow.color.a);
    for ring in 0..RINGS {
        let scale = 1.0 - ring as f32 / RINGS as f32;
        if let Some(path) = ellipse(centre, (rx * scale, ry * scale)) {
            window.paint_path(path, color);
        }
    }
}

/// The stage glow behind setup, filling its parent.
#[derive(IntoElement)]
pub struct StageGlow {
    glows: [Option<Glow>; 3],
}

impl StageGlow {
    /// The glows of `colors`.
    pub fn new(colors: &OnboardingColors) -> Self {
        Self {
            glows: colors.glows,
        }
    }
}

impl RenderOnce for StageGlow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let glows = self.glows;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
                    for glow in glows.into_iter().flatten() {
                        paint_glow(bounds, glow, window);
                    }
                });
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }
}

/// Mixes two colours in RGB (for `ob-select` fills arriving).
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    let l = |x: f32, y: f32| x + (y - x) * t;
    Hsla::from(Rgba {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_stack_to_the_centre_alpha() {
        let a = ring_alpha(0.22);
        let stacked = 1.0 - (1.0 - a).powi(RINGS as i32);
        assert!((stacked - 0.22).abs() < 1e-4);
    }

    #[test]
    fn light_drops_the_third_glow() {
        assert!(colors(Appearance::Dark).glows[2].is_some());
        assert!(colors(Appearance::Light).glows[2].is_none());
        assert_eq!(
            colors(Appearance::Dark).text,
            colors(Appearance::Dark).slide_text.0
        );
    }
}
