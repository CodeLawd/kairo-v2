//! The keyframe animations in `index.css`, `onboarding.css` and
//! `tailwind.config.js`, as data.
//!
//! Each [`Keyframes`] keeps the CSS name, duration, timing function and
//! stops. [`Keyframes::sample`] gives the frame at any time; [`animate`]
//! applies opacity and translation to an element. GPUI elements have no
//! general `transform`, `filter` or `background-position`, so scale, blur,
//! brightness and gradient sweeps are exposed in the [`Frame`] for elements
//! that can use them (SVG transforms, sized boxes) and are otherwise left
//! out. DECISIONS.md lists those. With reduced motion, [`animate`] renders
//! the final frame and nothing moves.

use crate::theme::KairoThemeExt;
use gpui_kit::base::Easing;
use gpui_kit::{Animation, AnimationExt, App, ElementId, IntoElement, Styled, px, relative};
use std::time::Duration;

/// One keyframe's values. Defaults are the element at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// `opacity`.
    pub opacity: f32,
    /// `translateX` in pixels.
    pub x: f32,
    /// `translateY` in pixels.
    pub y: f32,
    /// `translateX` as a fraction of the element's width (`100%` is 1).
    pub x_fraction: f32,
    /// `translateY` as a fraction of the element's height.
    pub y_fraction: f32,
    /// `scale`.
    pub scale: f32,
    /// `scaleX`.
    pub scale_x: f32,
    /// `scaleY`.
    pub scale_y: f32,
    /// `rotate` in degrees.
    pub rotate: f32,
    /// `filter: blur()` in pixels.
    pub blur: f32,
    /// `filter: brightness()`.
    pub brightness: f32,
    /// `height` as a fraction (the listening bars).
    pub height: f32,
    /// `background-position` x as a fraction (glint, boot shimmer, drift).
    pub background_x: f32,
}

/// The element at rest.
pub const REST: Frame = Frame {
    opacity: 1.0,
    x: 0.0,
    y: 0.0,
    x_fraction: 0.0,
    y_fraction: 0.0,
    scale: 1.0,
    scale_x: 1.0,
    scale_y: 1.0,
    rotate: 0.0,
    blur: 0.0,
    brightness: 1.0,
    height: 1.0,
    background_x: 0.0,
};

const fn rest() -> Frame {
    REST
}

impl Frame {
    fn lerp(self, other: Frame, t: f32) -> Frame {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Frame {
            opacity: l(self.opacity, other.opacity),
            x: l(self.x, other.x),
            y: l(self.y, other.y),
            x_fraction: l(self.x_fraction, other.x_fraction),
            y_fraction: l(self.y_fraction, other.y_fraction),
            scale: l(self.scale, other.scale),
            scale_x: l(self.scale_x, other.scale_x),
            scale_y: l(self.scale_y, other.scale_y),
            rotate: l(self.rotate, other.rotate),
            blur: l(self.blur, other.blur),
            brightness: l(self.brightness, other.brightness),
            height: l(self.height, other.height),
            background_x: l(self.background_x, other.background_x),
        }
    }
}

/// A CSS timing function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timing {
    /// `linear`.
    Linear,
    /// `ease-out`.
    EaseOut,
    /// `ease-in-out`.
    EaseInOut,
    /// `cubic-bezier(x1, y1, x2, y2)`.
    Bezier(f32, f32, f32, f32),
    /// `steps(1)` (end).
    Step,
}

/// `cubic-bezier(.16, 1, .3, 1)`: the shared `--ease` and `ease-out-expo`.
pub const OUT_EXPO: Timing = Timing::Bezier(0.16, 1.0, 0.3, 1.0);

impl Timing {
    fn easing(self) -> Easing {
        match self {
            Timing::Linear => Easing::Linear,
            Timing::EaseOut => Easing::EaseOut,
            Timing::EaseInOut => Easing::EaseInOut,
            Timing::Bezier(x1, y1, x2, y2) => {
                Easing::cubic_bezier(x1, y1, x2, y2).unwrap_or(Easing::Linear)
            }
            Timing::Step => Easing::Linear,
        }
    }

    /// Eased progress for linear progress `t` in `0..=1`.
    pub fn sample(self, t: f32) -> f32 {
        match self {
            Timing::Step => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            _ => self.easing().sample(t.clamp(0.0, 1.0)),
        }
    }
}

/// One `@keyframes` rule with the timing its class uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframes {
    /// The CSS name.
    pub name: &'static str,
    /// Duration of one cycle.
    pub duration_ms: u64,
    /// The timing function, applied per interval as CSS does.
    pub timing: Timing,
    /// Whether it loops (`infinite`).
    pub repeat: bool,
    /// Whether it plays forwards then backwards (`alternate`).
    pub alternate: bool,
    /// Offsets in `0..=1` with their frames, ascending, first at 0 and last
    /// at 1.
    pub stops: &'static [(f32, Frame)],
}

impl Keyframes {
    /// The frame at linear progress `t` through one cycle.
    pub fn sample(&self, t: f32) -> Frame {
        let t = t.clamp(0.0, 1.0);
        let stops = self.stops;
        let Some(last) = stops.last() else {
            return REST;
        };
        if t >= last.0 {
            return last.1;
        }
        for pair in stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if t >= a.0 && t <= b.0 {
                let span = (b.0 - a.0).max(f32::EPSILON);
                let local = self.timing.sample((t - a.0) / span);
                return a.1.lerp(b.1, local);
            }
        }
        stops[0].1
    }

    /// The final frame (what reduced motion shows).
    pub fn end(&self) -> Frame {
        self.stops.last().map_or(REST, |s| s.1)
    }

    /// The GPUI animation for one cycle (or a loop).
    pub fn animation(&self) -> Animation {
        let animation = Animation::new(Duration::from_millis(self.duration_ms));
        if self.repeat {
            animation.repeat()
        } else {
            animation
        }
    }
}

macro_rules! f {
    ($($field:ident: $value:expr),* $(,)?) => {
        Frame { $($field: $value,)* ..rest() }
    };
}

// ── index.css ────────────────────────────────────────────────────────────

/// A detected verse drops into place.
pub const DETECT_CARD_IN: Keyframes = Keyframes {
    name: "detect-card-in",
    duration_ms: 350,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, y: -8.0, scale: 0.97)), (1.0, REST)],
};
/// A detected verse's state swaps in softly.
pub const DETECT_STATE_IN: Keyframes = Keyframes {
    name: "detect-state-in",
    duration_ms: 300,
    timing: Timing::EaseOut,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, y: 2.0)), (1.0, REST)],
};
/// The transcript waiting for speech: four bars breathing in turn.
pub const LISTENING: Keyframes = Keyframes {
    name: "listening",
    duration_ms: 1100,
    timing: Timing::EaseInOut,
    repeat: true,
    alternate: false,
    stops: &[
        (0.0, f!(height: 0.3)),
        (0.5, f!(height: 1.0)),
        (1.0, f!(height: 0.3)),
    ],
};
/// The launch mark.
pub const SPLASH_MARK_IN: Keyframes = Keyframes {
    name: "splash-mark-in",
    duration_ms: 600,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, scale: 0.9)), (1.0, REST)],
};
/// The launch copy (0.12s delay in its class).
pub const SPLASH_COPY_IN: Keyframes = Keyframes {
    name: "splash-copy-in",
    duration_ms: 500,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, y: 6.0)), (1.0, REST)],
};
/// A launch step.
pub const SPLASH_STEP_IN: Keyframes = Keyframes {
    name: "splash-step-in",
    duration_ms: 280,
    timing: Timing::EaseOut,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, y: 3.0)), (1.0, REST)],
};
/// The caustic drifting across the launch glass.
pub const SPLASH_GLASS_DRIFT: Keyframes = Keyframes {
    name: "splash-glass-drift",
    duration_ms: 3200,
    timing: Timing::EaseInOut,
    repeat: true,
    alternate: true,
    stops: &[
        (0.0, f!(background_x: 0.0)),
        (0.5, f!(background_x: 0.6)),
        (1.0, f!(background_x: 1.0)),
    ],
};
/// "Booting up": a light band running through muted text.
pub const SPLASH_BOOT: Keyframes = Keyframes {
    name: "splash-boot",
    duration_ms: 1400,
    timing: Timing::Linear,
    repeat: true,
    alternate: false,
    stops: &[(0.0, f!(background_x: 1.0)), (1.0, f!(background_x: 0.0))],
};
/// The audience-message ticker (`overlay.html`). Its duration depends on
/// the message: use [`ticker_duration`].
pub const PA_TICKER: Keyframes = Keyframes {
    name: "pa-ticker",
    duration_ms: 8_000,
    timing: Timing::Linear,
    repeat: true,
    alternate: false,
    stops: &[(0.0, REST), (1.0, f!(x_fraction: -1.0))],
};

// ── tailwind.config.js ───────────────────────────────────────────────────

/// `animate-fade-in`.
pub const FADE_IN: Keyframes = Keyframes {
    name: "fadeIn",
    duration_ms: 400,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0)), (1.0, REST)],
};
/// `animate-slide-in`.
pub const SLIDE_IN: Keyframes = Keyframes {
    name: "slideIn",
    duration_ms: 400,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, x: -12.0)), (1.0, REST)],
};
/// `animate-spring-in`: menus and the row context menu.
pub const SPRING_IN: Keyframes = Keyframes {
    name: "springIn",
    duration_ms: 200,
    timing: Timing::Bezier(0.2, 0.0, 0.0, 1.0),
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, scale: 0.98, y: 4.0)), (1.0, REST)],
};
/// `animate-caret-blink`: the fake caret in the code boxes.
pub const CARET_BLINK: Keyframes = Keyframes {
    name: "caretBlink",
    duration_ms: 1250,
    timing: Timing::EaseOut,
    repeat: true,
    alternate: false,
    stops: &[
        (0.0, REST),
        (0.2, f!(opacity: 0.0)),
        (0.5, f!(opacity: 0.0)),
        (0.7, REST),
        (1.0, REST),
    ],
};

// ── onboarding.css ───────────────────────────────────────────────────────

/// `ob-fade`: the scrim and coach root.
pub const OB_FADE: Keyframes = Keyframes {
    name: "ob-fade",
    duration_ms: 250,
    timing: Timing::EaseOut,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0)), (1.0, REST)],
};
/// `ob-dialog`: the dialog scales in.
pub const OB_DIALOG: Keyframes = Keyframes {
    name: "ob-dialog",
    duration_ms: 450,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, y: 10.0, scale: 0.97)), (1.0, REST)],
};
/// `ob-in`: a slide enters in the direction of travel (28px, `--dir` 1).
/// Use [`ob_in_frame`] for the other direction or distance.
pub const OB_IN: Keyframes = Keyframes {
    name: "ob-in",
    duration_ms: 600,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, x: 28.0)), (1.0, REST)],
};
/// `ob-out`: the leaving slide.
pub const OB_OUT: Keyframes = Keyframes {
    name: "ob-out",
    duration_ms: 300,
    timing: Timing::Bezier(0.4, 0.0, 1.0, 1.0),
    repeat: false,
    alternate: false,
    stops: &[(0.0, REST), (1.0, f!(opacity: 0.0, x: -40.0))],
};
/// `ob-rise`.
pub const OB_RISE: Keyframes = Keyframes {
    name: "ob-rise",
    duration_ms: 500,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, y: 10.0)), (1.0, REST)],
};
/// `ob-pop`.
pub const OB_POP: Keyframes = Keyframes {
    name: "ob-pop",
    duration_ms: 450,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0, scale: 0.9)), (1.0, REST)],
};
/// `ob-line`: callout lines draw in.
pub const OB_LINE: Keyframes = Keyframes {
    name: "ob-line",
    duration_ms: 500,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(scale_y: 0.0)), (1.0, REST)],
};
/// `ob-fill`: the progress bar fills.
pub const OB_FILL: Keyframes = Keyframes {
    name: "ob-fill",
    duration_ms: 800,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(scale_x: 0.0)), (1.0, REST)],
};
/// `ob-select`: a choice's fill arrives (the fill is the element's own).
pub const OB_SELECT: Keyframes = Keyframes {
    name: "ob-select",
    duration_ms: 400,
    timing: Timing::EaseOut,
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(opacity: 0.0)), (1.0, REST)],
};
/// `ob-blink`: the typing caret.
pub const OB_BLINK: Keyframes = Keyframes {
    name: "ob-blink",
    duration_ms: 1000,
    timing: Timing::Step,
    repeat: true,
    alternate: false,
    stops: &[(0.0, REST), (0.5, f!(opacity: 0.0)), (1.0, REST)],
};
/// `ob-shake`: an error shakes once.
pub const OB_SHAKE: Keyframes = Keyframes {
    name: "ob-shake",
    duration_ms: 350,
    timing: Timing::EaseInOut,
    repeat: false,
    alternate: false,
    stops: &[
        (0.0, REST),
        (0.25, f!(x: -4.0)),
        (0.75, f!(x: 4.0)),
        (1.0, REST),
    ],
};
/// `ob-focus`: the welcome mark comes into focus.
pub const OB_FOCUS: Keyframes = Keyframes {
    name: "ob-focus",
    duration_ms: 1100,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[
        (0.0, f!(opacity: 0.0, scale: 0.88, blur: 12.0)),
        (1.0, REST),
    ],
};
/// `ob-glint`: one glint of light crosses the mark (0.75s delay).
pub const OB_GLINT: Keyframes = Keyframes {
    name: "ob-glint",
    duration_ms: 1100,
    timing: Timing::Bezier(0.45, 0.0, 0.2, 1.0),
    repeat: false,
    alternate: false,
    stops: &[(0.0, f!(background_x: 1.0)), (1.0, f!(background_x: 0.0))],
};
/// `ob-letter-in`: "Welcome to Kairo" rises in letter by letter.
pub const OB_LETTER_IN: Keyframes = Keyframes {
    name: "ob-letter-in",
    duration_ms: 900,
    timing: OUT_EXPO,
    repeat: false,
    alternate: false,
    stops: &[
        (
            0.0,
            f!(opacity: 0.0, y_fraction: 1.05, rotate: 6.0, blur: 4.0),
        ),
        (1.0, REST),
    ],
};
/// `ob-letter-wave`: the letters brighten in a wave.
pub const OB_LETTER_WAVE: Keyframes = Keyframes {
    name: "ob-letter-wave",
    duration_ms: 600,
    timing: Timing::EaseInOut,
    repeat: false,
    alternate: false,
    stops: &[
        (0.0, REST),
        (0.5, f!(brightness: 1.45, y_fraction: -0.05)),
        (1.0, REST),
    ],
};

/// Every keyframe rule, for the gallery and tests.
pub const ALL: &[Keyframes] = &[
    DETECT_CARD_IN,
    DETECT_STATE_IN,
    LISTENING,
    SPLASH_MARK_IN,
    SPLASH_COPY_IN,
    SPLASH_STEP_IN,
    SPLASH_GLASS_DRIFT,
    SPLASH_BOOT,
    PA_TICKER,
    FADE_IN,
    SLIDE_IN,
    SPRING_IN,
    CARET_BLINK,
    OB_FADE,
    OB_DIALOG,
    OB_IN,
    OB_OUT,
    OB_RISE,
    OB_POP,
    OB_LINE,
    OB_FILL,
    OB_SELECT,
    OB_BLINK,
    OB_SHAKE,
    OB_FOCUS,
    OB_GLINT,
    OB_LETTER_IN,
    OB_LETTER_WAVE,
];

/// The ticker's cycle for a message (`program.ts`: at least 8 s, else
/// 0.22 s per character, rounded).
pub fn ticker_duration(message: &str) -> Duration {
    let chars = message.chars().count() as f64;
    Duration::from_secs((chars * 0.22).round().max(8.0) as u64)
}

/// The `ob-in` start frame for a direction (`--dir`, 1 or -1) and distance.
pub fn ob_in_frame(direction: f32, distance: f32) -> Frame {
    f!(opacity: 0.0, x: direction.signum() * distance)
}

/// The stagger delay for the `i`th item (`--base + --i * 55ms`; `ob-pop`
/// uses 60ms).
pub fn stagger(base_ms: u64, index: u64, step_ms: u64) -> Duration {
    Duration::from_millis(base_ms + index * step_ms)
}

/// Plays `keyframes` on `element`, applying opacity and pixel translation.
/// With reduced motion the element is drawn at its final frame.
pub fn animate<E>(
    element: E,
    id: impl Into<ElementId>,
    keyframes: Keyframes,
    cx: &App,
) -> gpui_kit::AnyElement
where
    E: IntoElement + Styled + 'static,
{
    if cx.reduced_motion() {
        return apply(element, keyframes.end()).into_any_element();
    }
    element
        .with_animation(id, keyframes.animation(), move |el, delta| {
            let t = if keyframes.alternate {
                let doubled = delta * 2.0;
                if doubled > 1.0 {
                    2.0 - doubled
                } else {
                    doubled
                }
            } else {
                delta
            };
            apply(el, keyframes.sample(t))
        })
        .into_any_element()
}

fn apply<E: Styled>(element: E, frame: Frame) -> E {
    element
        .opacity(frame.opacity.clamp(0.0, 1.0))
        .relative()
        .left(px(frame.x))
        .top(px(frame.y))
        .when_fraction(frame)
}

trait FractionExt: Styled + Sized {
    fn when_fraction(self, frame: Frame) -> Self {
        if frame.x_fraction == 0.0 && frame.y_fraction == 0.0 {
            return self;
        }
        self.left(relative(frame.x_fraction))
            .top(relative(frame.y_fraction))
    }
}

impl<E: Styled> FractionExt for E {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_starts_at_zero_and_ends_at_one() {
        for k in ALL {
            assert!(k.stops.len() >= 2, "{}", k.name);
            assert_eq!(k.stops[0].0, 0.0, "{}", k.name);
            assert_eq!(k.stops.last().unwrap().0, 1.0, "{}", k.name);
            assert!(k.stops.windows(2).all(|w| w[0].0 < w[1].0), "{}", k.name);
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = ALL.iter().map(|k| k.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ALL.len());
    }

    #[test]
    fn sampling_hits_the_stops() {
        assert_eq!(DETECT_CARD_IN.sample(0.0).y, -8.0);
        assert_eq!(DETECT_CARD_IN.sample(1.0), REST);
        assert_eq!(OB_SHAKE.sample(0.25).x, -4.0);
        assert_eq!(OB_SHAKE.sample(0.75).x, 4.0);
        assert!((LISTENING.sample(0.5).height - 1.0).abs() < 1e-6);
        assert_eq!(CARET_BLINK.sample(0.35).opacity, 0.0);
        // steps(1): stays at the start value until the stop.
        assert_eq!(OB_BLINK.sample(0.49).opacity, 1.0);
        assert_eq!(OB_BLINK.sample(0.5).opacity, 0.0);
    }

    #[test]
    fn out_expo_front_loads_the_motion() {
        assert!(OUT_EXPO.sample(0.2) > 0.6);
        assert_eq!(OUT_EXPO.sample(0.0), 0.0);
        assert!((OUT_EXPO.sample(1.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn ob_in_follows_direction() {
        assert_eq!(ob_in_frame(-1.0, 28.0).x, -28.0);
        assert_eq!(stagger(100, 3, 55), Duration::from_millis(265));
        assert_eq!(ticker_duration("short"), Duration::from_secs(8));
        assert_eq!(ticker_duration(&"x".repeat(100)), Duration::from_secs(22));
    }
}
