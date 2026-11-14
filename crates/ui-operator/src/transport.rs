//! The live video transport (`LiveVideoControls.tsx` and the seek rules in
//! `LiveOutputPreview.tsx`).

use kairo_domain::media::format_media_clock;
use kairo_engine_proto::VideoTime;

/// `LIVE_VIDEO_SKIP_SECONDS`.
pub const SKIP_SECONDS: f64 = 5.0;

/// `EMPTY_TIME`: before the engine reports, and after the cue changes.
pub const EMPTY_TIME: VideoTime = VideoTime {
    current_time: 0.0,
    duration: 0.0,
    ended: false,
};

/// `seek`: never before the range start (or 0 when the start is at or past
/// the end), never past a known duration.
pub fn clamp_seek(seconds: f64, time: &VideoTime, start: Option<f64>) -> f64 {
    let duration = time.duration;
    let start = start.unwrap_or(0.0);
    let lower = if duration > 0.0 && start >= duration {
        0.0
    } else {
        start
    };
    if duration > 0.0 {
        seconds.max(lower).min(duration)
    } else {
        seconds.max(lower)
    }
}

/// The pause button's request: `setPaused(!(paused || ended))`.
pub fn toggle_request(paused: bool, time: &VideoTime) -> bool {
    !(paused || time.ended)
}

/// `playing`: shows Pause rather than Play.
pub fn playing(paused: bool, time: &VideoTime) -> bool {
    !paused && !time.ended
}

/// The readout: `m:ss / m:ss`, with `–:––` until the duration is known.
pub fn clock_text(shown: f64, time: &VideoTime) -> (String, String) {
    let total = if time.duration > 0.0 {
        format_media_clock(time.duration)
    } else {
        "–:––".to_string()
    };
    (format_media_clock(shown), total)
}

/// What a key does inside the transport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransportKey {
    TogglePause,
    Skip(f64),
    Restart,
}

/// Space or `k` toggles, arrows skip 5 s, Home restarts.
pub fn key_action(key: &str) -> Option<TransportKey> {
    match key {
        "space" | "k" => Some(TransportKey::TogglePause),
        "left" => Some(TransportKey::Skip(-SKIP_SECONDS)),
        "right" => Some(TransportKey::Skip(SKIP_SECONDS)),
        "home" => Some(TransportKey::Restart),
        _ => None,
    }
}

/// The scrubber's fill, 0 to 1; 0 until the duration is known.
pub fn progress(shown: f64, time: &VideoTime) -> f32 {
    if time.duration > 0.0 {
        (shown.min(time.duration) / time.duration).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(current_time: f64, duration: f64, ended: bool) -> VideoTime {
        VideoTime {
            current_time,
            duration,
            ended,
        }
    }

    #[test]
    fn seeks_clamp_to_the_range() {
        assert_eq!(clamp_seek(-3.0, &t(0.0, 30.0, false), None), 0.0);
        assert_eq!(clamp_seek(99.0, &t(0.0, 30.0, false), None), 30.0);
        assert_eq!(clamp_seek(2.0, &t(0.0, 30.0, false), Some(10.0)), 10.0);
        assert_eq!(
            clamp_seek(2.0, &t(0.0, 30.0, false), Some(40.0)),
            2.0,
            "start past the end plays from 0"
        );
        assert_eq!(
            clamp_seek(99.0, &EMPTY_TIME, Some(1.0)),
            99.0,
            "unknown duration"
        );
        assert_eq!(clamp_seek(0.0, &EMPTY_TIME, Some(5.0)), 5.0);
    }

    #[test]
    fn pause_and_end() {
        assert!(toggle_request(false, &t(1.0, 9.0, false)));
        assert!(!toggle_request(true, &t(1.0, 9.0, false)));
        assert!(
            !toggle_request(false, &t(9.0, 9.0, true)),
            "an ended video plays again"
        );
        assert!(!playing(false, &t(9.0, 9.0, true)));
    }

    #[test]
    fn readout_and_keys() {
        assert_eq!(
            clock_text(65.0, &t(65.0, 125.0, false)),
            ("1:05".into(), "2:05".into())
        );
        assert_eq!(clock_text(0.0, &EMPTY_TIME).1, "–:––");
        assert_eq!(key_action("k"), Some(TransportKey::TogglePause));
        assert_eq!(key_action("left"), Some(TransportKey::Skip(-5.0)));
        assert_eq!(key_action("home"), Some(TransportKey::Restart));
        assert_eq!(key_action("x"), None);
        assert_eq!(progress(5.0, &t(5.0, 10.0, false)), 0.5);
        assert_eq!(progress(5.0, &EMPTY_TIME), 0.0);
    }
}
