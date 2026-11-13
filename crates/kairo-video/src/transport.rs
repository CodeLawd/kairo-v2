//! One stream's playback rules, apart from any thread or decoder so they
//! can be run on a fake clock: the frame queue, the presentation clock,
//! rate, the trimmed range and its loop, pause, seek and the ended state.
//!
//! The rules are `overlay.html`'s (`startBackgroundVideo`, `videoBounds`,
//! `setBackgroundLoop`, `__setVideoPaused`, `__seekVideo`,
//! `__patchBackground`, `__videoEnded`) and `overlay-fit.ts`'s
//! (`mediaBounds`, `prepareOverlayMedia`).
//!
//! Time is kept on a "line": a number of media seconds that only moves
//! forward while playing. Each decoded frame gets the line at which it is
//! due; a lap of a looping range starts where the last one ended, so the
//! decoder can run ahead through the wrap and the first frames after it are
//! ready before they are due (spike 004 lost up to 86 ms at each wrap by
//! seeking on the playback path).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Frames decoded ahead of the clock.
pub const QUEUE: usize = 4;
const EPS: f64 = 1e-6;

/// A decoded frame and its presentation time in seconds from the stream's
/// start.
pub struct Decoded<P> {
    pub pts: f64,
    pub picture: P,
}

/// Where frames come from: the FFmpeg decoder, or a fake in tests.
pub trait Source {
    type Picture;
    /// The next frame in decode order; `None` at the end of the stream.
    fn next(&mut self) -> Result<Option<Decoded<Self::Picture>>, String>;
    /// Moves to the keyframe at or before `seconds`; frames before it
    /// follow and are dropped by the transport.
    fn seek(&mut self, seconds: f64) -> Result<(), String>;
    /// The container's duration, when it states one.
    fn duration(&self) -> Option<f64>;
    /// Seconds between frames.
    fn interval(&self) -> f64;
}

/// `data-media-start`, `data-media-end` and `data-media-rate`, already
/// normalised (`normalizeMediaPlayback`): start above 0, end above start,
/// rate within 0.5 to 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub rate: f64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            start: None,
            end: None,
            rate: 1.0,
        }
    }
}

/// What the transport reports to `__videoEnded` and the logs.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// `currentTime`.
    pub position: f64,
    /// The range's end, `f64::INFINITY` until the duration is known.
    pub end: f64,
    /// `video.paused`: paused by the operator, or stopped at the end.
    pub paused: bool,
    pub ended: bool,
    pub error: Option<String>,
    /// A frame has been shown.
    pub shown: bool,
}

impl Status {
    /// `__videoEnded`: ended, failed, or paused at (or past) the range end.
    pub fn video_ended(&self) -> bool {
        self.ended || self.error.is_some() || (self.paused && self.position >= self.end - EPS)
    }
}

struct Queued<P> {
    line: f64,
    /// The line at which this frame's lap starts.
    lap: f64,
    picture: P,
}

pub struct Transport<S: Source> {
    source: Option<S>,
    timing: Timing,
    looped: bool,
    duration: Option<f64>,
    /// Where the stream really ended, when that came before its duration.
    eof: Option<f64>,
    interval: f64,
    /// `data-pa-paused`: the operator paused it.
    user_paused: bool,
    ended: bool,
    error: Option<String>,
    // The clock: `base_line` at `base_at`, moving at `rate` while running.
    base_line: f64,
    base_at: Instant,
    running: bool,
    /// The clock waits for a frame: opening, after a seek or a rewind.
    waiting: bool,
    queue: VecDeque<Queued<S::Picture>>,
    /// The line at which the lap being decoded starts.
    lap: f64,
    /// Decoded frames that end before this time are dropped (seek pre-roll).
    floor: Option<f64>,
    /// The decoder reached the range end and will not loop.
    done: bool,
    end_line: Option<f64>,
    lap_frames: usize,
    last_pts: Option<f64>,
    /// The lap and line of the frame on screen.
    shown_lap: f64,
    shown_line: Option<f64>,
    /// Frames decoded but never shown because the clock had passed them.
    pub dropped: u64,
    pub presented: u64,
}

impl<S: Source> Transport<S> {
    pub fn new(timing: Timing, looped: bool, now: Instant) -> Self {
        Self {
            source: None,
            timing,
            looped,
            duration: None,
            eof: None,
            interval: 1.0 / 30.0,
            user_paused: false,
            ended: false,
            error: None,
            base_line: 0.0,
            base_at: now,
            running: false,
            waiting: true,
            queue: VecDeque::new(),
            lap: 0.0,
            floor: None,
            done: false,
            end_line: None,
            lap_frames: 0,
            last_pts: None,
            shown_lap: 0.0,
            shown_line: None,
            dropped: 0,
            presented: 0,
        }
    }

    /// The stream is open (`loadedmetadata`): go to the range start.
    pub fn open(&mut self, source: S, now: Instant) {
        self.duration = source.duration().filter(|d| d.is_finite() && *d > 0.0);
        let interval = source.interval();
        if interval.is_finite() && interval > 0.0 {
            self.interval = interval;
        }
        self.source = Some(source);
        let start = self.start();
        if start > 0.0 {
            self.seek_to(start, now);
        }
    }

    /// The file could not be opened or decoded: nothing is shown and the
    /// video counts as ended.
    pub fn fail(&mut self, message: String) {
        self.error = Some(message);
        self.done = true;
        self.running = false;
        self.waiting = false;
        self.queue.clear();
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The range end: `min(data-media-end, duration)`, or where the stream
    /// really stopped.
    fn end(&self) -> f64 {
        let mut end = self.timing.end.unwrap_or(f64::INFINITY);
        if let Some(d) = self.duration {
            end = end.min(d);
        }
        if let Some(e) = self.eof {
            end = end.min(e);
        }
        end
    }

    /// `videoBounds`: a start at or past the end plays from 0.
    fn start(&self) -> f64 {
        let start = self.timing.start.unwrap_or(0.0);
        if start < self.end() { start } else { 0.0 }
    }

    fn rate(&self) -> f64 {
        self.timing.rate
    }

    fn clock(&self, now: Instant) -> f64 {
        if self.running {
            self.base_line + now.saturating_duration_since(self.base_at).as_secs_f64() * self.rate()
        } else {
            self.base_line
        }
    }

    fn rebase(&mut self, now: Instant) {
        self.base_line = self.clock(now);
        self.base_at = now;
    }

    /// `currentTime`.
    pub fn position(&self, now: Instant) -> f64 {
        let (start, end) = (self.start(), self.end());
        (start + (self.clock(now) - self.shown_lap)).clamp(start, end.max(start))
    }

    pub fn status(&self, now: Instant) -> Status {
        Status {
            position: self.position(now),
            end: self.end(),
            paused: self.user_paused || self.ended,
            ended: self.ended,
            error: self.error.clone(),
            shown: self.shown_line.is_some(),
        }
    }

    pub fn looped(&self) -> bool {
        self.looped
    }

    /// Decodes one frame if the queue has room. True when it did work, so
    /// the caller should present and come back without waiting.
    pub fn decode_step(&mut self, now: Instant) -> bool {
        if self.error.is_some() || self.done || self.queue.len() >= QUEUE {
            return false;
        }
        let Some(source) = self.source.as_mut() else {
            return false;
        };
        match source.next() {
            Err(message) => {
                self.rebase(now);
                self.fail(message);
            }
            Ok(None) => {
                // A stream that stops before its stated duration ends there.
                let stopped = self.last_pts.map(|p| p + self.interval);
                if let Some(at) = stopped
                    && at < self.end()
                {
                    self.eof = Some(at);
                }
                // Nothing decoded and nothing asked to be skipped: the stream
                // has no frames. (After a seek past the last frame, the
                // frame on screen stays.)
                if self.shown_line.is_none()
                    && self.lap_frames == 0
                    && self.queue.is_empty()
                    && self.last_pts.is_none()
                    && self.floor.is_none()
                {
                    self.fail("the file has no video frames".into());
                    return true;
                }
                self.range_end();
            }
            Ok(Some(frame)) => {
                if let Some(floor) = self.floor {
                    if frame.pts + self.interval <= floor + EPS {
                        return true;
                    }
                    self.floor = None;
                }
                let (start, end) = (self.start(), self.end());
                if frame.pts >= end - EPS {
                    self.range_end();
                    return true;
                }
                self.lap_frames += 1;
                self.last_pts = Some(frame.pts);
                self.queue.push_back(Queued {
                    line: self.lap + (frame.pts - start).max(0.0),
                    lap: self.lap,
                    picture: frame.picture,
                });
            }
        }
        true
    }

    /// The decoder reached the range end: wrap to the start of the next lap
    /// when looping, else stop there.
    fn range_end(&mut self) {
        let (start, end) = (self.start(), self.end());
        let len = if end.is_finite() {
            (end - start).max(0.0)
        } else {
            // Unknown duration: the stream ended after its last frame.
            self.last_pts
                .map_or(0.0, |p| (p + self.interval - start).max(0.0))
        };
        if self.looped && self.lap_frames > 0 && len > 0.0 {
            let Some(source) = self.source.as_mut() else {
                return;
            };
            if let Err(message) = source.seek(start) {
                self.fail(message);
                return;
            }
            self.lap += len;
            self.floor = Some(start);
            self.lap_frames = 0;
            self.last_pts = None;
        } else {
            self.done = true;
            self.end_line = Some(self.lap + len);
        }
    }

    /// The frame due now, if a new one is. The caller hands it to the
    /// compositor; the compositor keeps showing it until the next.
    pub fn present(&mut self, now: Instant) -> Option<S::Picture> {
        if self.error.is_some() {
            return None;
        }
        if self.waiting {
            if let Some(item) = self.queue.pop_front() {
                // The first frame shows even when paused (`preload`).
                self.waiting = false;
                self.show(&item);
                self.base_at = now;
                self.running = !self.user_paused && !self.ended;
                return Some(item.picture);
            }
            if self.done && self.queue.is_empty() {
                // Nothing to show in what is left of the range.
                self.waiting = false;
                self.running = !self.user_paused;
                self.base_at = now;
            } else {
                return None;
            }
        }
        if !self.running {
            return None;
        }
        let clock = self.clock(now);
        let mut due = None;
        while self.queue.front().is_some_and(|q| q.line <= clock + EPS) {
            let item = self.queue.pop_front().expect("front");
            if due.is_some() {
                self.dropped += 1;
            }
            due = Some(item);
        }
        if let Some(item) = &due {
            self.show(item);
        }
        if self.queue.is_empty() {
            if self.done {
                if let Some(end) = self.end_line
                    && clock >= end - EPS
                {
                    // The non-looping end: pause there (`checkEnd`).
                    self.ended = true;
                    self.running = false;
                    self.base_line = end;
                    self.base_at = now;
                }
            } else if let Some(shown) = self.shown_line {
                // The decoder is behind: hold the clock one frame after the
                // frame on screen rather than skip what it decodes next.
                let hold = shown + self.interval;
                if clock > hold {
                    self.base_line = hold;
                    self.base_at = now;
                }
            }
        }
        due.map(|q| q.picture)
    }

    fn show(&mut self, item: &Queued<S::Picture>) {
        self.shown_lap = item.lap;
        self.shown_line = Some(item.line);
        self.presented += 1;
    }

    /// When `present` next has something to do, if the clock decides it.
    pub fn next_wake(&self) -> Option<Instant> {
        if !self.running || self.waiting {
            return None;
        }
        let line = match self.queue.front() {
            Some(q) => q.line,
            None if self.done => self.end_line?,
            None => return None,
        };
        let secs = ((line - self.base_line) / self.rate()).max(0.0);
        Some(self.base_at + Duration::from_secs_f64(secs))
    }

    /// Whether the decoder wants to run.
    pub fn wants_decode(&self) -> bool {
        self.source.is_some() && self.error.is_none() && !self.done && self.queue.len() < QUEUE
    }

    /// `__setVideoPaused`. Resuming from the end rewinds to the range start.
    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        if paused {
            if self.running {
                self.rebase(now);
                self.running = false;
            }
            self.user_paused = true;
            return;
        }
        self.user_paused = false;
        if self.error.is_some() {
            return;
        }
        if self.ended || self.position(now) >= self.end() - EPS {
            self.seek_to(self.start(), now);
            return;
        }
        if !self.waiting && !self.running {
            self.running = true;
            self.base_at = now;
        }
    }

    /// `__seekVideo`: a time that is not finite is ignored, others are
    /// clamped to the range.
    pub fn seek(&mut self, seconds: f64, now: Instant) {
        if !seconds.is_finite() || self.error.is_some() {
            return;
        }
        let (start, end) = (self.start(), self.end());
        self.seek_to(seconds.max(start).min(end), now);
    }

    fn seek_to(&mut self, target: f64, now: Instant) {
        let (start, end) = (self.start(), self.end());
        // At the end, show the last frame and let the end rules run.
        let decode_at = if target >= end - EPS {
            (end - self.interval).max(start)
        } else {
            target
        };
        let line = self.clock(now);
        self.queue.clear();
        self.done = false;
        self.end_line = None;
        self.ended = false;
        self.lap_frames = 0;
        self.last_pts = None;
        self.lap = line - (target - start);
        self.shown_lap = self.lap;
        self.base_line = line;
        self.base_at = now;
        self.running = false;
        self.waiting = true;
        self.floor = Some(decode_at);
        if let Some(source) = self.source.as_mut()
            && let Err(message) = source.seek(decode_at)
        {
            self.fail(message);
        }
    }

    /// `setBackgroundLoop` and `__patchBackground`: the loop changes without
    /// restarting; a finished video that now loops starts again, unless the
    /// operator paused it.
    pub fn set_loop(&mut self, looped: bool, now: Instant) {
        if looped == self.looped {
            return;
        }
        self.looped = looped;
        if self.error.is_some() {
            return;
        }
        if looped {
            if self.ended || (!self.user_paused && self.position(now) >= self.end() - EPS) {
                if !self.user_paused {
                    self.seek_to(self.start(), now);
                }
            } else if self.done {
                // Stopped decoding at the end it is still playing towards.
                self.done = false;
                self.end_line = None;
                self.range_end();
            }
            return;
        }
        // Frames already decoded past the wrap are dropped; the lap on
        // screen becomes the last.
        if self.lap > self.shown_lap + EPS {
            self.queue.retain(|q| q.lap <= self.shown_lap + EPS);
            let len = (self.end() - self.start()).max(0.0);
            self.done = true;
            self.end_line = Some(self.shown_lap + len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames numbered from 0 at `fps`, `count` of them, keyframes every
    /// `gop`; records what was asked of it.
    struct Fake {
        fps: f64,
        count: u32,
        gop: u32,
        at: u32,
        duration: Option<f64>,
        fail_at: Option<u32>,
        seeks: Vec<f64>,
    }

    impl Fake {
        fn new(fps: f64, count: u32) -> Self {
            Self {
                fps,
                count,
                gop: 10,
                at: 0,
                duration: Some(f64::from(count) / fps),
                fail_at: None,
                seeks: Vec::new(),
            }
        }
    }

    impl Source for Fake {
        type Picture = u32;
        fn next(&mut self) -> Result<Option<Decoded<u32>>, String> {
            if self.fail_at == Some(self.at) {
                return Err("decode failed".into());
            }
            if self.at >= self.count {
                return Ok(None);
            }
            let n = self.at;
            self.at += 1;
            Ok(Some(Decoded {
                pts: f64::from(n) / self.fps,
                picture: n,
            }))
        }
        fn seek(&mut self, seconds: f64) -> Result<(), String> {
            self.seeks.push(seconds);
            let frame = (seconds * self.fps + 1e-9).floor() as u32;
            self.at = (frame / self.gop * self.gop).min(self.count);
            Ok(())
        }
        fn duration(&self) -> Option<f64> {
            self.duration
        }
        fn interval(&self) -> f64 {
            1.0 / self.fps
        }
    }

    /// Runs the transport like its thread does, on a fake clock in 1 ms
    /// steps, and records each frame shown with the time it was shown.
    struct Rig {
        t: Transport<Fake>,
        t0: Instant,
        ms: u64,
        shown: Vec<(u64, u32)>,
    }

    impl Rig {
        fn new(fake: Fake, timing: Timing, looped: bool) -> Self {
            let t0 = Instant::now();
            let mut t = Transport::new(timing, looped, t0);
            t.open(fake, t0);
            Self {
                t,
                t0,
                ms: 0,
                shown: Vec::new(),
            }
        }
        fn now(&self) -> Instant {
            self.t0 + Duration::from_millis(self.ms)
        }
        fn run(&mut self, ms: u64) {
            for _ in 0..ms {
                let now = self.now();
                loop {
                    if let Some(p) = self.t.present(now) {
                        self.shown.push((self.ms, p));
                    }
                    if !self.t.decode_step(now) {
                        break;
                    }
                }
                self.ms += 1;
            }
        }
        fn last(&self) -> u32 {
            self.shown.last().expect("a frame").1
        }
        fn status(&self) -> Status {
            self.t.status(self.now())
        }
    }

    #[test]
    fn plays_at_its_rate_and_ends_paused_at_the_end() {
        let mut r = Rig::new(Fake::new(10.0, 20), Timing::default(), false);
        r.run(1);
        assert_eq!(r.shown, vec![(0, 0)]);
        r.run(1000);
        assert_eq!(r.last(), 10, "10 fps for one second");
        assert!(!r.status().video_ended());
        r.run(1500);
        assert_eq!(r.last(), 19);
        let s = r.status();
        assert!(s.ended && s.paused && s.video_ended());
        assert!((s.position - 2.0).abs() < 1e-9);
        assert_eq!(r.t.dropped, 0);
        // Frame n is shown at n × 100 ms.
        for &(ms, n) in &r.shown {
            assert_eq!(ms, u64::from(n) * 100, "frame {n}");
        }
    }

    #[test]
    fn rate_scales_the_clock() {
        let mut r = Rig::new(
            Fake::new(10.0, 40),
            Timing {
                rate: 2.0,
                ..Timing::default()
            },
            false,
        );
        r.run(1001);
        assert_eq!(r.last(), 20);
        let mut r = Rig::new(
            Fake::new(10.0, 40),
            Timing {
                rate: 0.5,
                ..Timing::default()
            },
            false,
        );
        r.run(1001);
        assert_eq!(r.last(), 5);
    }

    #[test]
    fn a_trimmed_loop_wraps_from_end_to_start_without_a_gap() {
        // 0.5 to 1.5 s of a 10 fps clip: frames 5 to 14, then 5 again.
        let timing = Timing {
            start: Some(0.5),
            end: Some(1.5),
            rate: 1.0,
        };
        let mut r = Rig::new(Fake::new(10.0, 40), timing, true);
        r.run(3001);
        let frames: Vec<u32> = r.shown.iter().map(|s| s.1).collect();
        let lap: Vec<u32> = (5..15).collect();
        assert_eq!(&frames[..10], &lap[..]);
        assert_eq!(&frames[10..20], &lap[..]);
        assert_eq!(&frames[20..30], &lap[..]);
        // Every frame is exactly 100 ms after the one before, across wraps.
        for w in r.shown.windows(2) {
            assert_eq!(w[1].0 - w[0].0, 100, "{:?}", w);
        }
        assert!(!r.status().video_ended());
        assert_eq!(r.t.dropped, 0);
    }

    #[test]
    fn a_trimmed_range_without_loop_stops_at_its_end() {
        let timing = Timing {
            start: Some(0.5),
            end: Some(1.5),
            rate: 1.0,
        };
        let mut r = Rig::new(Fake::new(10.0, 40), timing, false);
        r.run(2000);
        assert_eq!(r.shown.first().map(|s| s.1), Some(5));
        assert_eq!(r.last(), 14);
        let s = r.status();
        assert!(s.ended && s.video_ended());
        assert!((s.position - 1.5).abs() < 1e-9);
    }

    #[test]
    fn a_start_past_the_end_plays_from_zero() {
        let timing = Timing {
            start: Some(9.0),
            end: None,
            rate: 1.0,
        };
        let mut r = Rig::new(Fake::new(10.0, 20), timing, false);
        r.run(1);
        assert_eq!(r.last(), 0);
    }

    #[test]
    fn seek_shows_the_frame_at_the_target_and_clamps_to_the_range() {
        let timing = Timing {
            start: Some(0.5),
            end: Some(3.0),
            rate: 1.0,
        };
        let mut r = Rig::new(Fake::new(10.0, 40), timing, false);
        r.run(100);
        r.t.seek(2.04, r.now());
        r.run(1);
        assert_eq!(r.last(), 20, "the frame covering 2.04 s");
        assert!((r.status().position - 2.04).abs() < 0.0015);
        r.run(100);
        assert_eq!(r.last(), 21);
        // Before the range start: the start.
        r.t.seek(0.0, r.now());
        r.run(1);
        assert_eq!(r.last(), 5);
        // Not finite: ignored.
        let before = r.status().position;
        r.t.seek(f64::NAN, r.now());
        r.t.seek(f64::INFINITY, r.now());
        assert!((r.status().position - before).abs() < 1e-9);
    }

    #[test]
    fn seek_while_paused_shows_the_new_frame_and_stays_paused() {
        let mut r = Rig::new(Fake::new(10.0, 40), Timing::default(), false);
        r.run(250);
        r.t.set_paused(true, r.now());
        r.run(500);
        assert_eq!(r.last(), 2, "held while paused");
        r.t.seek(3.0, r.now());
        r.run(500);
        assert_eq!(r.last(), 30);
        let s = r.status();
        assert!(s.paused && !s.ended && !s.video_ended());
        assert!((s.position - 3.0).abs() < 1e-9);
        r.t.set_paused(false, r.now());
        r.run(101);
        assert_eq!(r.last(), 31);
    }

    #[test]
    fn pause_keeps_the_position_and_resume_continues_from_it() {
        let mut r = Rig::new(Fake::new(10.0, 40), Timing::default(), false);
        r.run(351);
        r.t.set_paused(true, r.now());
        let at = r.status().position;
        r.run(2000);
        assert!((r.status().position - at).abs() < 1e-9);
        assert_eq!(r.last(), 3);
        r.t.set_paused(false, r.now());
        r.run(51);
        assert_eq!(r.last(), 4, "0.35 s + 50 ms passes 0.4 s");
    }

    #[test]
    fn resuming_from_the_end_rewinds_to_the_range_start() {
        let timing = Timing {
            start: Some(1.0),
            end: Some(2.0),
            rate: 1.0,
        };
        let mut r = Rig::new(Fake::new(10.0, 40), timing, false);
        r.run(1500);
        assert!(r.status().video_ended());
        r.t.set_paused(false, r.now());
        assert!(!r.status().video_ended(), "rewinding is not ended");
        r.run(1);
        assert_eq!(r.last(), 10);
        r.run(1200);
        assert!(r.status().ended);
    }

    #[test]
    fn paused_at_the_end_counts_as_ended() {
        let mut r = Rig::new(Fake::new(10.0, 20), Timing::default(), false);
        r.run(10);
        r.t.set_paused(true, r.now());
        r.t.seek(2.0, r.now());
        r.run(10);
        let s = r.status();
        assert!(!s.ended && s.paused && s.video_ended());
        assert_eq!(r.last(), 19, "the last frame");
    }

    #[test]
    fn seeking_to_the_end_while_playing_loops_or_ends() {
        let mut r = Rig::new(Fake::new(10.0, 20), Timing::default(), true);
        r.run(10);
        r.t.seek(99.0, r.now());
        r.run(250);
        assert_eq!(
            &r.shown.iter().map(|s| s.1).collect::<Vec<_>>()[1..],
            &[19, 0, 1, 2]
        );
        let mut r = Rig::new(Fake::new(10.0, 20), Timing::default(), false);
        r.run(10);
        r.t.seek(99.0, r.now());
        r.run(250);
        assert!(r.status().ended);
    }

    #[test]
    fn enabling_loop_on_a_finished_video_restarts_it() {
        let mut r = Rig::new(Fake::new(10.0, 10), Timing::default(), false);
        r.run(1200);
        assert!(r.status().ended);
        r.t.set_loop(true, r.now());
        assert!(!r.status().video_ended());
        r.run(1);
        assert_eq!(r.last(), 0);
        r.run(2000);
        assert!(!r.status().ended, "now it loops");
    }

    #[test]
    fn enabling_loop_on_a_finished_video_the_operator_paused_does_nothing() {
        let mut r = Rig::new(Fake::new(10.0, 10), Timing::default(), false);
        r.run(1200);
        r.t.set_paused(true, r.now());
        r.t.set_loop(true, r.now());
        r.run(100);
        assert!(r.status().video_ended());
        assert_eq!(r.last(), 9);
    }

    #[test]
    fn loop_changes_while_playing_do_not_restart() {
        let mut r = Rig::new(Fake::new(10.0, 10), Timing::default(), false);
        r.run(450);
        r.t.set_loop(true, r.now());
        r.run(1000);
        // Played 0..9 then wrapped: no jump back at the moment of the change.
        let frames: Vec<u32> = r.shown.iter().map(|s| s.1).collect();
        assert_eq!(&frames[..14], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3]);
        // Turned off just after a wrap was decoded ahead: the lap on screen
        // is the last.
        let mut r = Rig::new(Fake::new(10.0, 10), Timing::default(), true);
        r.run(960);
        assert_eq!(r.last(), 9);
        r.t.set_loop(false, r.now());
        r.run(1000);
        assert_eq!(r.last(), 9);
        assert!(r.status().ended);
    }

    #[test]
    fn a_patch_that_keeps_the_loop_changes_nothing() {
        let mut r = Rig::new(Fake::new(10.0, 40), Timing::default(), true);
        r.run(500);
        let seeks = r.t.source.as_ref().map(|s| s.seeks.len());
        r.t.set_loop(true, r.now());
        r.run(100);
        assert_eq!(r.t.source.as_ref().map(|s| s.seeks.len()), seeks);
        assert_eq!(r.last(), 5);
    }

    #[test]
    fn a_decode_error_ends_the_video_and_keeps_its_last_frame_on_screen() {
        let mut fake = Fake::new(10.0, 40);
        fake.fail_at = Some(8);
        let mut r = Rig::new(fake, Timing::default(), true);
        r.run(2000);
        // Frames still queued when decoding failed are not shown.
        let last = r.last();
        assert!(last < 8);
        let s = r.status();
        assert!(s.error.is_some() && s.video_ended());
        // Transport on a failed video is ignored.
        r.t.set_paused(false, r.now());
        r.t.seek(0.0, r.now());
        r.run(100);
        assert_eq!(r.last(), last);
    }

    #[test]
    fn a_stream_with_no_frames_is_an_error() {
        let mut r = Rig::new(Fake::new(10.0, 0), Timing::default(), false);
        r.run(10);
        assert!(r.shown.is_empty());
        assert!(r.status().error.is_some() && r.status().video_ended());
    }

    #[test]
    fn a_stream_shorter_than_its_duration_ends_or_loops_at_its_last_frame() {
        let mut fake = Fake::new(10.0, 10);
        fake.duration = Some(5.0);
        let mut r = Rig::new(fake, Timing::default(), true);
        r.run(2050);
        let frames: Vec<u32> = r.shown.iter().map(|s| s.1).collect();
        assert_eq!(&frames[..12], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1]);
        let mut fake = Fake::new(10.0, 10);
        fake.duration = None;
        let mut r = Rig::new(fake, Timing::default(), false);
        r.run(1200);
        assert!(r.status().ended);
    }

    #[test]
    fn paused_before_the_first_frame_shows_it_and_stays_paused() {
        let t0 = Instant::now();
        let mut t: Transport<Fake> = Transport::new(Timing::default(), false, t0);
        t.set_paused(true, t0);
        t.open(Fake::new(10.0, 40), t0);
        let mut shown = Vec::new();
        for ms in 0..1000 {
            let now = t0 + Duration::from_millis(ms);
            loop {
                if let Some(p) = t.present(now) {
                    shown.push(p);
                }
                if !t.decode_step(now) {
                    break;
                }
            }
        }
        assert_eq!(shown, vec![0]);
        assert!(t.status(t0).paused);
    }

    #[test]
    fn a_slow_decoder_holds_the_clock_instead_of_skipping() {
        // Present without decoding for a while (the decoder stalled), then
        // let it catch up: no frame is dropped.
        let mut r = Rig::new(Fake::new(10.0, 40), Timing::default(), false);
        r.run(1);
        for _ in 0..1000 {
            let now = r.now();
            if let Some(p) = r.t.present(now) {
                r.shown.push((r.ms, p));
            }
            r.ms += 1;
        }
        r.run(500);
        assert_eq!(r.t.dropped, 0);
        let frames: Vec<u32> = r.shown.iter().map(|s| s.1).collect();
        assert_eq!(frames, (0..frames.len() as u32).collect::<Vec<_>>());
    }

    #[test]
    fn next_wake_is_the_next_frame_time() {
        let mut r = Rig::new(Fake::new(10.0, 40), Timing::default(), false);
        r.run(1);
        let wake = r.t.next_wake().expect("a wake");
        assert_eq!(wake.duration_since(r.t0), Duration::from_millis(100));
        r.t.set_paused(true, r.now());
        assert_eq!(r.t.next_wake(), None);
    }
}
