//! `onboarding.css`'s motion primitives as functions of time: `ob-in`,
//! `ob-rise`, `ob-pop`, `ob-select`, `ob-line`, `ob-out`, the caret and the
//! typing and countdown hooks of the tour's mock-ups.
//!
//! `--dir` is the direction of travel, `--i` staggers siblings (55 ms; 60 ms
//! for `ob-pop`), `--base` delays a group. GPUI has no transforms, so
//! `ob-pop` (scale 0.9 to 1) and `ob-line` (scaleY) play as a fade and a
//! growing height. Under reduced motion everything is at rest.

use gpui_kit::{Styled, px};
use kairo_ui_kit::motion::OUT_EXPO;
use kairo_ui_kit::motion::Timing;

/// Time since a page, slide or view appeared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    /// Milliseconds since it appeared.
    pub t: f32,
    /// `--dir`: 1 forward, -1 back.
    pub dir: f32,
    /// Reduced motion: everything at rest.
    pub still: bool,
}

/// How long after appearing anything in setup still moves, apart from the
/// loops (caret, countdown).
pub const SETTLE_MS: f32 = 3_200.0;

impl Clock {
    /// A clock at rest.
    pub fn still() -> Self {
        Self {
            t: f32::MAX,
            dir: 1.0,
            still: true,
        }
    }

    fn progress(&self, delay: f32, duration: f32) -> f32 {
        if self.still {
            return 1.0;
        }
        ((self.t - delay) / duration).clamp(0.0, 1.0)
    }

    /// `ob-in` eased progress (0.6 s, `--ease`).
    pub fn in_progress(&self, i: usize, base: f32) -> f32 {
        OUT_EXPO.sample(self.progress(base + i as f32 * 55.0, 600.0))
    }

    /// `ob-in`: enters `dist` px from the direction of travel.
    pub fn ob_in<E: Styled>(&self, el: E, i: usize, base: f32, dist: f32) -> E {
        let e = self.in_progress(i, base);
        el.opacity(e)
            .relative()
            .left(px(self.dir * dist * (1.0 - e)))
    }

    /// `ob-rise`: rises 10 px (0.5 s).
    pub fn rise<E: Styled>(&self, el: E, i: usize, base: f32) -> E {
        let e = OUT_EXPO.sample(self.progress(base + i as f32 * 55.0, 500.0));
        el.opacity(e).relative().top(px(10.0 * (1.0 - e)))
    }

    /// `ob-pop` (0.45 s, 60 ms stagger), as a fade.
    pub fn pop<E: Styled>(&self, el: E, i: usize, base: f32) -> E {
        let e = OUT_EXPO.sample(self.progress(base + i as f32 * 60.0, 450.0));
        el.opacity(e)
    }

    /// `ob-select`: how far a fill has arrived (0.4 s ease-out).
    pub fn select(&self, base: f32) -> f32 {
        Timing::EaseOut.sample(self.progress(base, 400.0))
    }

    /// `ob-line`: a callout line's drawn fraction (0.5 s).
    pub fn line(&self, base: f32) -> f32 {
        OUT_EXPO.sample(self.progress(base, 500.0))
    }

    /// `ob-blink`: whether the caret shows (1 s, steps).
    pub fn caret(&self) -> bool {
        self.still || (self.t % 1000.0) < 500.0
    }

    /// `useTypewriter`: characters typed of `len`, one every `speed` ms after
    /// `delay`; all at once under reduced motion.
    pub fn typed(&self, len: usize, delay: f32, speed: f32) -> usize {
        if self.still {
            return len;
        }
        if self.t < delay {
            return 0;
        }
        (((self.t - delay) / speed).floor() as usize).min(len)
    }

    /// `useCountdown`: seconds left of `from`, starting again after 0;
    /// frozen under reduced motion.
    pub fn countdown(&self, from: u32) -> String {
        let left = if self.still {
            from
        } else {
            from - ((self.t / 1000.0).floor() as u32 % (from + 1))
        };
        format!("{:02}:{:02}", left / 60, left % 60)
    }

    /// Whether anything is still settling.
    pub fn settling(&self) -> bool {
        !self.still && self.t < SETTLE_MS
    }
}

/// `ob-out`: the leaving slide, `t` ms after it started leaving (0.3 s).
pub fn out<E: Styled>(el: E, t: f32, dir: f32) -> E {
    let e = Timing::Bezier(0.4, 0.0, 1.0, 1.0).sample((t / 300.0).clamp(0.0, 1.0));
    el.opacity(1.0 - e).relative().left(px(dir * -40.0 * e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: f32) -> Clock {
        Clock {
            t,
            dir: 1.0,
            still: false,
        }
    }

    #[test]
    fn typing_and_countdown_follow_the_hooks() {
        assert_eq!(at(400.0).typed(8, 500.0, 80.0), 0);
        assert_eq!(at(500.0 + 80.0).typed(8, 500.0, 80.0), 1);
        assert_eq!(at(10_000.0).typed(8, 500.0, 80.0), 8);
        assert_eq!(Clock::still().typed(8, 500.0, 80.0), 8);
        assert_eq!(at(0.0).countdown(299), "04:59");
        assert_eq!(at(1_000.0).countdown(299), "04:58");
        assert_eq!(at(299_000.0).countdown(299), "00:00");
        assert_eq!(at(300_000.0).countdown(299), "04:59");
    }

    #[test]
    fn stagger_and_rest() {
        assert_eq!(at(0.0).in_progress(0, 0.0), 0.0);
        assert!(at(55.0).in_progress(1, 0.0) < 0.01);
        assert_eq!(Clock::still().in_progress(3, 900.0), 1.0);
        assert!(at(2_000.0).in_progress(3, 0.0) > 0.99);
        assert!(at(0.0).caret() && !at(600.0).caret());
    }
}
