//! Physical motion for the account stage: springs that keep their velocity
//! when retargeted, a release-velocity tracker, momentum projection and
//! rubber-banding, and the tour's pager built from them.
//!
//! Everything a person can touch here moves on a spring instead of a timed
//! curve, so it starts from the value on screen, can be grabbed and reversed
//! mid-flight, and carries the pointer's velocity through the release. The
//! state is caller-owned (an entity field) and stepped once per frame.

use gpui_kit::{SpringConfig, SpringState};
use std::time::Instant;

/// A spring as designers name it: how fast it arrives and how much it
/// overshoots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// Seconds one undamped oscillation would take; lower is snappier.
    pub response: f32,
    /// Damping ratio: 1 never overshoots, lower bounces.
    pub damping: f32,
}

impl Tuning {
    /// A move started by a button or key: no overshoot.
    pub const MOVE: Tuning = Tuning {
        response: 0.4,
        damping: 1.0,
    };
    /// A move thrown by a drag: the release had momentum, so it may bounce.
    pub const THROWN: Tuning = Tuning {
        response: 0.4,
        damping: 0.8,
    };
    /// A page arriving.
    pub const PAGE: Tuning = Tuning {
        response: 0.5,
        damping: 1.0,
    };
    /// The stage leaving for the booth.
    pub const LEAVE: Tuning = Tuning {
        response: 0.3,
        damping: 1.0,
    };

    fn config(self) -> SpringConfig {
        let frequency = std::f32::consts::TAU / self.response.max(0.01);
        SpringConfig::new(frequency * frequency, 2.0 * self.damping * frequency, 1.0)
    }
}

/// Within this of the target, at rest, a spring has arrived.
const EPSILON: f32 = 0.001;

/// One spring-driven value.
#[derive(Clone, Copy, Debug)]
pub struct Flick {
    state: SpringState,
    target: f32,
    tuning: Tuning,
    at: Instant,
}

impl Flick {
    /// At rest on `value`.
    pub fn new(value: f32) -> Self {
        Self::at(value, Instant::now())
    }

    /// Rests on a value in the caller’s clock domain.
    pub fn at(value: f32, now: Instant) -> Self {
        Self {
            state: SpringState {
                position: value,
                velocity: 0.0,
            },
            target: value,
            tuning: Tuning::MOVE,
            at: now,
        }
    }

    /// The value on screen.
    pub fn value(&self) -> f32 {
        self.state.position
    }

    /// Where it is heading.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// Units per second.
    pub fn velocity(&self) -> f32 {
        self.state.velocity
    }

    /// Whether it has yet to arrive.
    pub fn moving(&self) -> bool {
        self.state.position != self.target || self.state.velocity != 0.0
    }

    fn advance(&mut self, now: Instant) {
        if self.moving() {
            let elapsed = now.saturating_duration_since(self.at).as_secs_f32();
            let config = self.tuning.config();
            let next = config.step(self.state, self.target, elapsed);
            self.state = if config.is_settled(next, self.target, EPSILON) {
                SpringState {
                    position: self.target,
                    velocity: 0.0,
                }
            } else {
                next
            };
        }
        self.at = now;
    }

    /// Heads for `target` from the value and velocity it has now.
    pub fn to(&mut self, target: f32, tuning: Tuning, now: Instant) {
        self.advance(now);
        self.target = target;
        self.tuning = tuning;
    }

    /// Heads for `target` at `velocity`: the hand-off from a drag.
    pub fn throw(&mut self, target: f32, velocity: f32, tuning: Tuning, now: Instant) {
        self.advance(now);
        self.state.velocity = velocity;
        self.target = target;
        self.tuning = tuning;
    }

    /// Holds `value`: the pointer has it.
    pub fn pin(&mut self, value: f32, now: Instant) {
        self.state = SpringState {
            position: value,
            velocity: 0.0,
        };
        self.target = value;
        self.at = now;
    }

    /// Steps to `now`; `still` (reduced motion) arrives at once. Returns
    /// whether another frame is needed.
    pub fn tick(&mut self, now: Instant, still: bool) -> bool {
        if still {
            self.pin(self.target, now);
            return false;
        }
        self.advance(now);
        self.moving()
    }
}

/// How far a release at `velocity` (px/s) coasts, as scroll deceleration
/// does: the exponential decay UIKit uses, not `v² / 2a`.
pub fn project(velocity: f32, deceleration_rate: f32) -> f32 {
    (velocity / 1000.0) * deceleration_rate / (1.0 - deceleration_rate)
}

/// A normal scroll's deceleration rate.
pub const DECELERATION: f32 = 0.998;

/// Resistance past a boundary: the further past, the less it follows.
pub fn rubberband(overshoot: f32, dimension: f32, constant: f32) -> f32 {
    (overshoot * dimension * constant) / (dimension + constant * overshoot.abs())
}

/// How much of the pull past an edge shows.
pub const RUBBER: f32 = 0.55;

/// Samples kept for the release velocity.
const SAMPLES: usize = 5;
/// Samples older than this say nothing about the release.
const WINDOW_SECS: f32 = 0.1;

/// The pointer's recent positions, for its velocity at release.
#[derive(Clone, Copy, Debug)]
pub struct Velocity {
    samples: [(f32, Instant); SAMPLES],
    len: usize,
}

impl Velocity {
    /// No samples.
    pub fn new(now: Instant) -> Self {
        Self {
            samples: [(0.0, now); SAMPLES],
            len: 0,
        }
    }

    /// Records the pointer at `x`.
    pub fn push(&mut self, x: f32, now: Instant) {
        if self.len == SAMPLES {
            self.samples.rotate_left(1);
            self.len -= 1;
        }
        self.samples[self.len] = (x, now);
        self.len += 1;
    }

    /// Pixels per second at `now`; 0 when the pointer had stopped.
    pub fn at(&self, now: Instant) -> f32 {
        let recent = |t: Instant| now.saturating_duration_since(t).as_secs_f32() <= WINDOW_SECS;
        let samples = &self.samples[..self.len];
        let Some(&(last_x, last_t)) = samples.last().filter(|s| recent(s.1)) else {
            return 0.0;
        };
        let Some(&(first_x, first_t)) = samples.iter().find(|s| recent(s.1)) else {
            return 0.0;
        };
        let span = last_t.saturating_duration_since(first_t).as_secs_f32();
        if span <= 0.0 {
            return 0.0;
        }
        (last_x - first_x) / span
    }
}

/// Movement before a press becomes a drag.
pub const HYSTERESIS: f32 = 10.0;

#[derive(Clone, Copy, Debug)]
struct Grab {
    /// Where the pointer took hold.
    x: f32,
    /// The position it took hold of.
    position: f32,
    /// The page that was current.
    home: usize,
    /// Past the hysteresis: the pointer has the pages.
    live: bool,
    velocity: Velocity,
}

/// A row of pages on one spring. The position is in pages: 1.5 is halfway
/// from the second to the third.
#[derive(Clone, Copy, Debug)]
pub struct Pager {
    position: Flick,
    count: usize,
    grab: Option<Grab>,
}

impl Pager {
    /// `count` pages, resting on `index`.
    pub fn new(count: usize, index: usize) -> Self {
        Self::at(count, index, Instant::now())
    }

    /// Creates a pager in the caller’s clock domain.
    pub fn at(count: usize, index: usize, now: Instant) -> Self {
        Self {
            position: Flick::at(index.min(count.max(1) - 1) as f32, now),
            count: count.max(1),
            grab: None,
        }
    }

    /// The position on screen, in pages.
    pub fn position(&self) -> f32 {
        self.position.value()
    }

    /// Whether the pointer has the pages.
    pub fn dragging(&self) -> bool {
        self.grab.is_some_and(|g| g.live)
    }

    /// Whether a press is being followed.
    pub fn pressed(&self) -> bool {
        self.grab.is_some()
    }

    fn last(&self) -> f32 {
        (self.count - 1) as f32
    }

    /// Travels to `index`, from wherever it is and however fast it moves.
    pub fn go(&mut self, index: usize, now: Instant) {
        self.grab = None;
        self.position
            .to((index as f32).min(self.last()), Tuning::MOVE, now);
    }

    /// Rests on `index` at once.
    pub fn jump(&mut self, index: usize, now: Instant) {
        self.grab = None;
        self.position.pin((index as f32).min(self.last()), now);
    }

    /// The pointer went down at `x`. Pages in flight are caught where they
    /// are; pages at rest wait for the hysteresis.
    pub fn press(&mut self, x: f32, now: Instant) {
        let live = self.position.moving();
        let home = self.position.target().round().clamp(0.0, self.last()) as usize;
        let position = self.position.value();
        if live {
            self.position.pin(position, now);
        }
        let mut velocity = Velocity::new(now);
        velocity.push(x, now);
        self.grab = Some(Grab {
            x,
            position,
            home,
            live,
            velocity,
        });
    }

    /// The pointer moved to `x` over pages `width` wide. Returns whether the
    /// pages follow it.
    pub fn drag(&mut self, x: f32, width: f32, now: Instant) -> bool {
        let last = self.last();
        let Some(grab) = self.grab.as_mut() else {
            return false;
        };
        if !grab.live {
            if (x - grab.x).abs() < HYSTERESIS {
                return false;
            }
            // Take hold where the pointer is now, so nothing jumps.
            grab.live = true;
            grab.x = x;
        }
        grab.velocity.push(x, now);
        let raw = grab.position - (x - grab.x) / width.max(1.0);
        let resisted = if raw < 0.0 {
            -rubberband(-raw, 1.0, RUBBER)
        } else if raw > last {
            last + rubberband(raw - last, 1.0, RUBBER)
        } else {
            raw
        };
        self.position.pin(resisted, now);
        true
    }

    /// The pointer let go. Returns the page the release throws to: the one
    /// nearest where the momentum would carry it, at most one from home.
    pub fn release(&mut self, width: f32, now: Instant) -> Option<usize> {
        let grab = self.grab.take()?;
        if !grab.live {
            return None;
        }
        let width = width.max(1.0);
        let velocity = grab.velocity.at(now);
        let projected = self.position.value() - project(velocity, DECELERATION) / width;
        let home = grab.home as f32;
        let target = projected
            .round()
            .clamp(home - 1.0, home + 1.0)
            .clamp(0.0, self.last());
        self.position
            .throw(target, -velocity / width, Tuning::THROWN, now);
        Some(target as usize)
    }

    /// Steps the spring; returns whether another frame is needed.
    pub fn tick(&mut self, now: Instant, still: bool) -> bool {
        if self.dragging() {
            return false;
        }
        self.position.tick(now, still)
    }
}

/// Progress for item `index` of `count` that follow one another in, from one
/// spring's `progress`: later items start later and all arrive together.
pub fn staged(progress: f32, index: usize, count: usize) -> f32 {
    /// How much of the travel the whole stagger is spread over.
    const SPREAD: f32 = 0.35;
    let steps = count.saturating_sub(1).max(1) as f32;
    let delay = SPREAD * index.min(count.saturating_sub(1)) as f32 / steps;
    ((progress - delay) / (1.0 - delay)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn later(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn a_spring_arrives_without_overshoot_and_rests() {
        let start = Instant::now();
        let mut flick = Flick::new(0.0);
        flick.to(1.0, Tuning::MOVE, start);
        let mut peak: f32 = 0.0;
        for frame in 1..=120 {
            flick.tick(later(start, frame * 16), false);
            peak = peak.max(flick.value());
        }
        assert!(peak <= 1.0 + 1e-4, "critically damped never passes: {peak}");
        assert_eq!(flick.value(), 1.0);
        assert!(!flick.moving());
    }

    #[test]
    fn a_retarget_keeps_position_and_velocity() {
        let start = Instant::now();
        let mut flick = Flick::new(0.0);
        flick.to(1.0, Tuning::MOVE, start);
        flick.tick(later(start, 100), false);
        let (value, velocity) = (flick.value(), flick.velocity());
        assert!(value > 0.0 && velocity > 0.0);
        // Reversed mid-flight: it starts from here, still moving forward.
        flick.to(0.0, Tuning::MOVE, later(start, 100));
        assert_eq!(flick.value(), value);
        assert_eq!(flick.velocity(), velocity);
        flick.tick(later(start, 116), false);
        assert!(flick.value() > value, "it decelerates before turning");
    }

    #[test]
    fn reduced_motion_arrives_at_once() {
        let start = Instant::now();
        let mut flick = Flick::new(0.0);
        flick.to(1.0, Tuning::MOVE, start);
        assert!(!flick.tick(start, true));
        assert_eq!(flick.value(), 1.0);
    }

    #[test]
    fn projection_and_rubberband_follow_the_formulas() {
        assert!((project(1000.0, DECELERATION) - 499.0).abs() < 0.5);
        assert_eq!(project(0.0, DECELERATION), 0.0);
        let near = rubberband(0.1, 1.0, RUBBER);
        let far = rubberband(1.0, 1.0, RUBBER);
        assert!(near < 0.1 && far < 1.0 && far > near);
        // Each further step follows less.
        assert!(far - rubberband(0.9, 1.0, RUBBER) < near);
    }

    #[test]
    fn velocity_reads_the_recent_samples_only() {
        let start = Instant::now();
        let mut velocity = Velocity::new(start);
        for i in 0..5u64 {
            velocity.push(i as f32 * -10.0, later(start, i * 10));
        }
        assert!((velocity.at(later(start, 40)) + 1000.0).abs() < 1.0);
        // Held still before letting go: no throw.
        assert_eq!(velocity.at(later(start, 400)), 0.0);
    }

    #[test]
    fn a_press_waits_for_the_hysteresis_then_tracks_one_to_one() {
        let start = Instant::now();
        let mut pager = Pager::new(6, 2);
        pager.press(500.0, start);
        assert!(!pager.drag(495.0, 1000.0, later(start, 10)));
        assert!(pager.drag(488.0, 1000.0, later(start, 20)));
        assert_eq!(pager.position(), 2.0, "taking hold does not jump");
        assert!(pager.drag(388.0, 1000.0, later(start, 30)));
        assert!((pager.position() - 2.1).abs() < 1e-4);
    }

    #[test]
    fn a_slow_short_drag_returns_and_a_flick_advances() {
        let start = Instant::now();
        let mut pager = Pager::new(6, 2);
        pager.press(500.0, start);
        pager.drag(480.0, 1000.0, later(start, 10));
        pager.drag(400.0, 1000.0, later(start, 20));
        // Held, then released: back home.
        assert_eq!(pager.release(1000.0, later(start, 600)), Some(2));

        let mut pager = Pager::new(6, 2);
        pager.press(500.0, start);
        for i in 1..=5u64 {
            pager.drag(500.0 - i as f32 * 20.0, 1000.0, later(start, i * 8));
        }
        // A short, fast flick: momentum carries it one page, never two.
        assert_eq!(pager.release(1000.0, later(start, 41)), Some(3));
        assert!(pager.tick(later(start, 57), false));
        assert!(pager.position() > 2.0);
    }

    #[test]
    fn the_ends_resist_and_a_click_is_not_a_drag() {
        let start = Instant::now();
        let mut pager = Pager::new(6, 0);
        pager.press(100.0, start);
        pager.drag(120.0, 1000.0, later(start, 10));
        pager.drag(620.0, 1000.0, later(start, 20));
        let pulled = pager.position();
        assert!(pulled < 0.0 && pulled > -0.5, "{pulled}");
        assert_eq!(pager.release(1000.0, later(start, 500)), Some(0));

        let mut pager = Pager::new(6, 0);
        pager.press(100.0, start);
        assert_eq!(pager.release(1000.0, later(start, 50)), None);
    }

    #[test]
    fn pages_in_flight_are_caught_where_they_are() {
        let start = Instant::now();
        let mut pager = Pager::new(6, 0);
        pager.go(1, start);
        pager.tick(later(start, 120), false);
        let caught = pager.position();
        assert!(caught > 0.0 && caught < 1.0);
        pager.press(300.0, later(start, 120));
        assert!(pager.dragging(), "no hysteresis for a moving page");
        assert_eq!(pager.position(), caught);
    }

    #[test]
    fn staged_items_follow_and_arrive_together() {
        assert_eq!(staged(0.0, 0, 4), 0.0);
        assert!(staged(0.3, 0, 4) > staged(0.3, 3, 4));
        assert_eq!(staged(0.3, 3, 4), 0.0);
        for i in 0..4 {
            assert_eq!(staged(1.0, i, 4), 1.0);
        }
        assert_eq!(staged(0.5, 0, 1), 0.5);
    }
}
