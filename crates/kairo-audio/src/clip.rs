//! One file's sound on its own thread, feeding a mixer source: an
//! `<audio>` or the sound of a `<video>`.
//!
//! The rules are the media element's as `overlay-fit.ts` and the players
//! use them: a range (`data-media-start`, `data-media-end`, cut to the
//! duration; a start at or past the end plays from 0), a rate, a loop that
//! wraps inside the range, pause, seek clamped to the range, and a stop at
//! the end of a range that does not loop. Play after the end starts again
//! from the range start (`if (audio.ended) currentTime = 0`).
//!
//! The position is what the mixer has taken, not what was decoded, so it is
//! the sound being heard. Pause and seek drop what is queued and decode
//! again from the new place.

use crate::decode::AudioFile;
use crate::mixer::{RATE, SourceInput};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// How far ahead of the mixer a clip decodes.
const AHEAD_FRAMES: usize = RATE as usize / 2;
const EPS: f64 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipSettings {
    pub start: Option<f64>,
    pub end: Option<f64>,
    /// 0.5 to 2; anything else plays at 1.
    pub rate: f64,
    pub looped: bool,
}

impl Default for ClipSettings {
    fn default() -> Self {
        Self {
            start: None,
            end: None,
            rate: 1.0,
            looped: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipControl {
    Play,
    Pause,
    Seek(f64),
    Loop(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipStatus {
    /// `currentTime`, in media seconds.
    pub position: f64,
    /// The file's length; 0 until known (`Number.isFinite(duration) ? … : 0`).
    pub duration: f64,
    pub paused: bool,
    pub ended: bool,
    pub error: Option<String>,
    /// The file has no sound.
    pub silent: bool,
    /// The file is open and its length known.
    pub ready: bool,
}

struct Inbox {
    controls: Vec<ClipControl>,
    stop: bool,
    status: ClipStatus,
}

struct Shared {
    inbox: Mutex<Inbox>,
    wake: Condvar,
}

pub struct ClipPlayer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl ClipPlayer {
    /// Starts playing `path` into `input`, paused when `paused`.
    pub fn start(path: PathBuf, settings: ClipSettings, paused: bool, input: SourceInput) -> Self {
        let shared = Arc::new(Shared {
            inbox: Mutex::new(Inbox {
                controls: Vec::new(),
                stop: false,
                status: ClipStatus {
                    position: settings.start.unwrap_or(0.0),
                    duration: 0.0,
                    paused,
                    ended: false,
                    error: None,
                    silent: false,
                    ready: false,
                },
            }),
            wake: Condvar::new(),
        });
        let inner = shared.clone();
        let thread = std::thread::Builder::new()
            .name("audio clip".into())
            .spawn(move || run(path, settings, paused, input, &inner))
            .map_err(|e| tracing::error!("cannot start an audio thread: {e}"))
            .ok();
        Self { shared, thread }
    }

    pub fn control(&self, control: ClipControl) {
        let mut inbox = self.shared.inbox.lock().unwrap_or_else(|e| e.into_inner());
        inbox.controls.push(control);
        self.shared.wake.notify_all();
    }

    pub fn status(&self) -> ClipStatus {
        self.shared
            .inbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }
}

impl Drop for ClipPlayer {
    fn drop(&mut self) {
        {
            let mut inbox = self.shared.inbox.lock().unwrap_or_else(|e| e.into_inner());
            inbox.stop = true;
            self.shared.wake.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// `videoBounds`: start and end of the range, given the duration.
pub fn bounds(settings: &ClipSettings, duration: Option<f64>) -> (f64, f64) {
    let duration = duration.unwrap_or(f64::INFINITY);
    let start = settings
        .start
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(0.0);
    let end = settings
        .end
        .filter(|e| e.is_finite() && *e > 0.0)
        .unwrap_or(f64::INFINITY)
        .min(duration);
    (if start < end { start } else { 0.0 }, end)
}

struct Play {
    file: Option<AudioFile>,
    input: SourceInput,
    rate: f64,
    looped: bool,
    start: f64,
    end: f64,
    /// Media time the queued sound starts from, and the mixer's count then.
    base: f64,
    base_consumed: u64,
    /// Frames pushed since `base`, and the media time decoding has reached.
    pushed: u64,
    decoded: f64,
    finished: bool,
    paused: bool,
    scratch: Vec<f32>,
}

impl Play {
    fn played(&self) -> u64 {
        self.input
            .control
            .consumed()
            .saturating_sub(self.base_consumed)
            .min(self.pushed)
    }

    /// The media time being heard. A loop folds back into the range.
    fn position(&self) -> f64 {
        let raw = self.base + self.played() as f64 * self.rate / f64::from(RATE);
        if self.looped && self.end.is_finite() && self.end > self.start && raw >= self.end {
            self.start + (raw - self.start) % (self.end - self.start)
        } else {
            raw.min(self.end)
        }
    }

    /// Drops what is queued and decodes again from `at`.
    fn restart(&mut self, at: f64) -> Result<(), String> {
        let control = &self.input.control;
        control.set_playing(false);
        control.set_held(true);
        control.flush();
        // The mixer drops the queue on its next render; wait for it so the
        // new sound is not dropped with the old.
        for _ in 0..200 {
            if !control.flush_pending() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.base = at;
        self.base_consumed = control.consumed();
        self.pushed = 0;
        self.decoded = at;
        self.finished = false;
        if let Some(file) = self.file.as_mut() {
            file.seek(at)?;
        }
        Ok(())
    }

    /// Decodes ahead while there is room. Returns false when idle.
    fn feed(&mut self) -> Result<bool, String> {
        let Some(file) = self.file.as_mut() else {
            return Ok(false);
        };
        if self.paused || self.finished || self.input.queued() >= AHEAD_FRAMES {
            return Ok(false);
        }
        self.scratch.clear();
        let more = file.read(&mut self.scratch)?;
        let per_frame = self.rate / f64::from(RATE);
        let frames = self.scratch.len() / 2;
        // Cut at the range end.
        let left = ((self.end - self.decoded) / per_frame).max(0.0);
        let keep = if left.is_finite() {
            (left.floor() as usize).min(frames)
        } else {
            frames
        };
        let mut offset = 0;
        while offset < keep {
            let n = self.input.push(&self.scratch[offset * 2..keep * 2]);
            offset += n;
            if n == 0 {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        self.pushed += keep as u64;
        self.decoded += keep as f64 * per_frame;
        let at_end = !more || keep < frames || self.decoded >= self.end - EPS;
        if at_end {
            if self.looped && self.end.is_finite() && self.end > self.start {
                // The next lap follows straight on; the queue is not dropped.
                file.seek(self.start)?;
                self.decoded = self.start;
            } else {
                self.finished = true;
                // A file shorter than the prime plays what it has.
                self.input.control.set_held(false);
                // Draining the last of the queue is not a glitch.
                self.input.control.set_playing(false);
            }
        } else if self.input.queued() >= PRIME_FRAMES {
            // Held until 250 ms is queued, as a media element waits for
            // enough data, then counted as playing (running dry is then a
            // glitch). The prime is well above the output's 85 ms cushion,
            // which the device takes in one burst when it opens.
            self.input.control.set_held(false);
            self.input.control.set_playing(true);
        }
        Ok(true)
    }

    /// Everything queued has been heard after the stream ended.
    fn drained(&self) -> bool {
        self.finished && self.played() >= self.pushed
    }
}

/// Frames queued before a started or seeked clip is heard: 250 ms.
const PRIME_FRAMES: usize = RATE as usize / 4;

fn run(path: PathBuf, settings: ClipSettings, paused: bool, input: SourceInput, shared: &Shared) {
    let set = |f: &dyn Fn(&mut ClipStatus)| {
        let mut inbox = shared.inbox.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut inbox.status);
    };
    let rate = if settings.rate.is_finite() && settings.rate > 0.0 {
        settings.rate.clamp(0.5, 2.0)
    } else {
        1.0
    };
    let file = match AudioFile::open(&path, rate) {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "audio cannot be played");
            set(&|s| s.error = Some(error.clone()));
            return;
        }
    };
    let duration = file.as_ref().and_then(AudioFile::duration);
    let (start, end) = bounds(&settings, duration);
    let silent = file.is_none();
    set(&|s| {
        s.silent = silent;
        s.duration = duration.unwrap_or(0.0);
        s.position = start;
        s.ready = true;
    });
    let mut play = Play {
        file,
        input,
        rate,
        looped: settings.looped,
        start,
        end,
        base: start,
        base_consumed: 0,
        pushed: 0,
        decoded: start,
        finished: false,
        paused,
        scratch: Vec::with_capacity(16_384),
    };
    if let Err(error) = play.restart(start) {
        set(&|s| s.error = Some(error.clone()));
        return;
    }
    let mut ended = false;
    loop {
        let controls = {
            let mut inbox = shared.inbox.lock().unwrap_or_else(|e| e.into_inner());
            if inbox.stop {
                break;
            }
            std::mem::take(&mut inbox.controls)
        };
        for control in controls {
            let result = match control {
                ClipControl::Pause => {
                    if play.paused {
                        Ok(())
                    } else {
                        play.paused = true;
                        let at = play.position();
                        play.restart(at)
                    }
                }
                ClipControl::Play => {
                    if ended || play.position() >= play.end - EPS {
                        // Resuming at the end starts the range again.
                        ended = false;
                        play.paused = false;
                        play.restart(play.start)
                    } else if play.paused {
                        play.paused = false;
                        Ok(())
                    } else {
                        Ok(())
                    }
                }
                ClipControl::Seek(seconds) => {
                    if seconds.is_finite() {
                        ended = false;
                        let at = seconds.clamp(play.start, play.end.min(f64::MAX));
                        play.restart(at)
                    } else {
                        Ok(())
                    }
                }
                ClipControl::Loop(looped) => {
                    play.looped = looped;
                    Ok(())
                }
            };
            if let Err(error) = result {
                tracing::warn!(path = %path.display(), %error, "audio control failed");
                set(&|s| s.error = Some(error.clone()));
            }
        }
        let busy = match play.feed() {
            Ok(busy) => busy,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "audio decoding failed");
                set(&|s| s.error = Some(error.clone()));
                play.finished = true;
                play.input.control.set_playing(false);
                false
            }
        };
        if !ended && play.drained() && !play.paused {
            ended = true;
        }
        let position = if ended {
            play.end
                .min(play.base + play.pushed as f64 * play.rate / f64::from(RATE))
        } else {
            play.position()
        };
        set(&|s| {
            s.position = position;
            s.paused = play.paused || ended;
            s.ended = ended;
        });
        if !busy {
            let inbox = shared.inbox.lock().unwrap_or_else(|e| e.into_inner());
            if inbox.controls.is_empty() && !inbox.stop {
                let _ = shared.wake.wait_timeout(inbox, Duration::from_millis(20));
            }
        }
    }
    play.input.control.set_playing(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_video_bounds() {
        let s = |start, end| ClipSettings {
            start,
            end,
            ..ClipSettings::default()
        };
        assert_eq!(bounds(&s(None, None), Some(10.0)), (0.0, 10.0));
        assert_eq!(bounds(&s(Some(2.0), Some(5.0)), Some(10.0)), (2.0, 5.0));
        // End cut to the duration; a start past the end plays from 0.
        assert_eq!(bounds(&s(Some(2.0), Some(50.0)), Some(10.0)), (2.0, 10.0));
        assert_eq!(bounds(&s(Some(12.0), None), Some(10.0)), (0.0, 10.0));
        assert_eq!(
            bounds(&s(Some(f64::NAN), Some(-1.0)), None),
            (0.0, f64::INFINITY)
        );
    }
}
