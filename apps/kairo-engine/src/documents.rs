//! Embedded document videos (prompt 058): `__controlDocumentVideo` and
//! `__documentPlayback` from `overlay.html`, as pure rules over the
//! players' status, so they can be checked without a window.

use crate::program::Shell;
use kairo_domain::documents::{DocumentPlaybackStatus, DocumentVideoCommand, DocumentVideoStatus};
use kairo_video::{Status, Videos};

/// `video.error`: the file cannot be decoded.
pub const CANNOT_PLAY: &str =
    "This embedded video cannot be played. Convert it to MP4 (H.264/AAC) or WebM and import again.";
/// `data-play-error`: `play()` was refused.
pub const PLAY_FAILED: &str =
    "Video playback could not start. Check the video format and try again.";

/// What a command does to a video's picture and sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transport {
    Pause,
    Play,
    Seek(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayError {
    Keep,
    Clear,
    Set,
}

#[derive(Debug, PartialEq)]
pub struct Plan {
    pub transport: Vec<Transport>,
    /// `visibility`, when the command changes it.
    pub visible: Option<bool>,
    pub play_error: PlayError,
}

/// The clip's end in its file: the known duration capped by the clip's
/// end, else the clip's end, else `unknown`.
fn end_of(status: Option<&Status>, end: Option<f64>, unknown: f64) -> f64 {
    match status.map(|s| s.end).filter(|e| e.is_finite()) {
        Some(duration) => end.map_or(duration, |e| e.min(duration)),
        None => end.unwrap_or(unknown),
    }
}

/// `__controlDocumentVideo` for a clip at `(start, end)` with `status`
/// (`None`: no player for it yet).
pub fn control(
    command: &DocumentVideoCommand,
    (start, end): (f64, Option<f64>),
    status: Option<&Status>,
) -> Plan {
    match command {
        DocumentVideoCommand::Pause => Plan {
            transport: vec![Transport::Pause],
            visible: None,
            play_error: PlayError::Keep,
        },
        DocumentVideoCommand::Seek { seconds } => {
            let end = end_of(status, end, f64::INFINITY);
            // Setting `currentTime` to NaN throws in the page: nothing moves.
            let transport = if seconds.is_finite() {
                vec![Transport::Seek((start + seconds).max(start).min(end))]
            } else {
                vec![]
            };
            Plan {
                transport,
                visible: Some(true),
                play_error: PlayError::Keep,
            }
        }
        DocumentVideoCommand::Play | DocumentVideoCommand::Restart => {
            let cannot = status.is_none_or(|s| s.error.is_some());
            if cannot {
                // `play()` rejects: hidden and paused, with the reason.
                return Plan {
                    transport: vec![Transport::Pause],
                    visible: Some(false),
                    play_error: PlayError::Set,
                };
            }
            let end = end_of(status, end, f64::INFINITY);
            let at_end = status.is_some_and(|s| s.ended || s.position >= end);
            let mut transport = Vec::new();
            if matches!(command, DocumentVideoCommand::Restart) || at_end {
                transport.push(Transport::Seek(start));
            }
            transport.push(Transport::Play);
            Plan {
                transport,
                visible: Some(true),
                play_error: PlayError::Clear,
            }
        }
    }
}

/// One video's `__documentPlayback` entry.
pub fn video_status(
    (start, end): (f64, Option<f64>),
    status: Option<&Status>,
    visible: bool,
    play_error: Option<&str>,
) -> DocumentVideoStatus {
    let end = end_of(status, end, 0.0);
    let position = status.map_or(start, |s| s.position);
    DocumentVideoStatus {
        current_time: (position - start).max(0.0),
        duration: (end - start).max(0.0),
        paused: status.is_none_or(|s| s.paused),
        ended: status.is_some_and(|s| s.ended) || (end > 0.0 && position >= end),
        visible,
        error: if status.is_some_and(|s| s.error.is_some()) {
            Some(CANNOT_PLAY.into())
        } else {
            play_error.map(str::to_owned)
        },
    }
}

/// `__documentPlayback`: `None` outside document mode or before a slide.
pub fn playback(
    document_mode: bool,
    shell: &Shell,
    videos: Option<&Videos>,
) -> Option<DocumentPlaybackStatus> {
    if !document_mode {
        return None;
    }
    let shown = shell.doc_videos.as_ref()?;
    let statuses = (0..shown.slide.videos.len())
        .map(|i| {
            let status = videos.and_then(|v| v.status(shown.sources[i]));
            video_status(
                shown.range(i).unwrap_or((0.0, None)),
                status.as_ref(),
                shown.visible[i],
                shown.play_error[i].as_deref(),
            )
        })
        .collect();
    Some(DocumentPlaybackStatus {
        id: shown.slide.id.clone(),
        page: shown.slide.page,
        videos: statuses,
    })
}

/// The slide's videos whose players now exist (`ids` reported by the
/// compositor) and have not been put at their start yet: each once, with
/// its start.
pub fn prime(shell: &mut Shell, ids: &[u64]) -> Vec<(u64, f64)> {
    let Some(shown) = shell.doc_videos.as_mut() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..shown.sources.len() {
        if !shown.primed[i] && ids.contains(&shown.sources[i]) {
            shown.primed[i] = true;
            out.push((shown.sources[i], shown.range(i).map_or(0.0, |r| r.0)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(position: f64, end: f64) -> Status {
        Status {
            position,
            end,
            paused: false,
            ended: false,
            error: None,
            shown: true,
        }
    }

    #[test]
    fn pause_only_pauses() {
        let plan = control(
            &DocumentVideoCommand::Pause,
            (2.0, None),
            Some(&status(3.0, 10.0)),
        );
        assert_eq!(plan.transport, vec![Transport::Pause]);
        assert_eq!(plan.visible, None);
        assert_eq!(plan.play_error, PlayError::Keep);
    }

    #[test]
    fn seek_is_relative_to_the_clip_and_clamped() {
        let s = status(3.0, 10.0);
        let seek = |seconds| {
            control(
                &DocumentVideoCommand::Seek { seconds },
                (2.0, Some(8.0)),
                Some(&s),
            )
        };
        assert_eq!(seek(1.5).transport, vec![Transport::Seek(3.5)]);
        assert_eq!(seek(-4.0).transport, vec![Transport::Seek(2.0)]);
        assert_eq!(seek(99.0).transport, vec![Transport::Seek(8.0)]);
        assert_eq!(seek(1.0).visible, Some(true));
        assert!(seek(f64::NAN).transport.is_empty());
        // The clip's end past the file's: the file's duration wins.
        let short = status(1.0, 5.0);
        let plan = control(
            &DocumentVideoCommand::Seek { seconds: 99.0 },
            (0.0, Some(8.0)),
            Some(&short),
        );
        assert_eq!(plan.transport, vec![Transport::Seek(5.0)]);
        // Duration not known yet: the clip's end, else unbounded.
        let unknown = status(0.0, f64::INFINITY);
        let plan = control(
            &DocumentVideoCommand::Seek { seconds: 99.0 },
            (0.0, None),
            Some(&unknown),
        );
        assert_eq!(plan.transport, vec![Transport::Seek(99.0)]);
    }

    #[test]
    fn play_resumes_and_restarts_only_at_the_end_or_when_asked() {
        let mid = status(4.0, 10.0);
        let play = control(&DocumentVideoCommand::Play, (2.0, None), Some(&mid));
        assert_eq!(play.transport, vec![Transport::Play]);
        assert_eq!(play.visible, Some(true));
        assert_eq!(play.play_error, PlayError::Clear);
        let restart = control(&DocumentVideoCommand::Restart, (2.0, None), Some(&mid));
        assert_eq!(
            restart.transport,
            vec![Transport::Seek(2.0), Transport::Play]
        );
        let done = status(10.0, 10.0);
        let again = control(&DocumentVideoCommand::Play, (2.0, None), Some(&done));
        assert_eq!(again.transport, vec![Transport::Seek(2.0), Transport::Play]);
        // Past the clip's own end.
        let past = status(6.0, 10.0);
        let again = control(&DocumentVideoCommand::Play, (2.0, Some(5.0)), Some(&past));
        assert_eq!(again.transport, vec![Transport::Seek(2.0), Transport::Play]);
    }

    #[test]
    fn play_that_cannot_start_hides_the_video_with_the_reason() {
        let mut broken = status(0.0, f64::INFINITY);
        broken.error = Some("decoder".into());
        for status in [None, Some(&broken)] {
            let plan = control(&DocumentVideoCommand::Play, (0.0, None), status);
            assert_eq!(plan.transport, vec![Transport::Pause]);
            assert_eq!(plan.visible, Some(false));
            assert_eq!(plan.play_error, PlayError::Set);
        }
    }

    #[test]
    fn status_reports_clip_times_and_the_two_errors() {
        let s = video_status((2.0, Some(8.0)), Some(&status(5.0, 10.0)), true, None);
        assert_eq!(s.current_time, 3.0);
        assert_eq!(s.duration, 6.0);
        assert!(!s.paused && !s.ended && s.visible);
        assert_eq!(s.error, None);
        // Stopped at the clip's end counts as ended.
        let s = video_status((2.0, Some(8.0)), Some(&status(8.0, 10.0)), true, None);
        assert!(s.ended);
        // Unknown duration and no clip end: duration 0.
        let s = video_status((0.0, None), Some(&status(1.0, f64::INFINITY)), false, None);
        assert_eq!(s.duration, 0.0);
        assert!(!s.ended);
        // No player yet: paused at the start.
        let s = video_status((2.0, None), None, false, None);
        assert_eq!((s.current_time, s.paused), (0.0, true));
        let s = video_status((0.0, None), None, false, Some(PLAY_FAILED));
        assert_eq!(s.error.as_deref(), Some(PLAY_FAILED));
        let mut broken = status(0.0, 10.0);
        broken.error = Some("x".into());
        let s = video_status((0.0, None), Some(&broken), false, Some(PLAY_FAILED));
        assert_eq!(s.error.as_deref(), Some(CANNOT_PLAY), "video.error wins");
    }
}
