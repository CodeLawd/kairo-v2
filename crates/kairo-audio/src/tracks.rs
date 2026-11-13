//! The house players of the operator process: `TracksPlayer` (walk-in
//! music) and `ProgramAudioFallback` (the live video's sound when no screen
//! can play it). Each has its own mixer and output; neither feeds NDI.

use crate::clip::{ClipControl, ClipPlayer, ClipSettings};
use crate::mixer::{MixerControl, SourceKind, mixer};
use crate::output::{AudioOut, Host, SystemHost};
use std::path::PathBuf;

/// `setTime`'s payload.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackTime {
    pub current_time: f64,
    pub duration: f64,
    pub ended: bool,
}

struct Playing {
    key: String,
    source: u64,
    control: std::sync::Arc<crate::mixer::SourceControl>,
    clip: ClipPlayer,
}

/// One clip on one output: what both players are made of.
struct Deck {
    out: AudioOut,
    control: MixerControl,
    playing: Option<Playing>,
    label: String,
}

impl Deck {
    fn new(host: impl Host) -> Self {
        let (mixer, control, _) = mixer(0);
        Self {
            out: AudioOut::start(mixer, host),
            control,
            playing: None,
            label: String::new(),
        }
    }

    fn stop(&mut self) {
        if let Some(old) = self.playing.take() {
            drop(old.clip);
            self.control.remove(old.source);
        }
        self.out.set_output(false, &self.label);
    }

    fn load(
        &mut self,
        key: String,
        path: PathBuf,
        settings: ClipSettings,
        paused: bool,
        gain: f32,
    ) {
        self.stop();
        let Some((source, input)) = self.control.add(SourceKind::Tracks) else {
            tracing::error!("no room in the mixer for a track");
            return;
        };
        input.control.set_gain(gain);
        let control = input.control.clone();
        self.out.set_output(true, &self.label);
        self.playing = Some(Playing {
            key,
            source,
            control,
            clip: ClipPlayer::start(path, settings, paused, input),
        });
    }

    fn set_label(&mut self, label: &str) {
        self.label = label.to_owned();
        self.out.set_output(self.playing.is_some(), label);
    }
}

/// `TracksPlayer.tsx`: one track live at a time, at full volume, on the
/// device named by `outputLabel` (re-matched when devices change).
pub struct TracksPlayer {
    deck: Deck,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::fake::FakeHost;
    use crate::tests::{fixture, ramp};
    use std::time::{Duration, Instant};

    fn wait(what: &str, mut ok: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !ok() {
            assert!(Instant::now() < end, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_track_plays_pauses_seeks_stops_and_reports_progress() {
        let host = FakeHost::default();
        host.plug("a", "Speakers", true);
        host.plug("b", "Desk", false);
        let mut tracks = TracksPlayer::with_host(host.clone());
        tracks.set_output_label("Desk");
        assert_eq!(tracks.time(), TrackTime::default());
        let song = ramp("song.wav", 3.0);
        tracks.set_live(Some(("t1", song.clone())), false);
        wait("device", || tracks.device().as_deref() == Some("Desk"));
        wait("progress", || tracks.time().current_time > 0.2);
        assert!((tracks.time().duration - 3.0).abs() < 0.01);
        tracks.set_live(Some(("t1", song.clone())), true);
        std::thread::sleep(Duration::from_millis(100));
        let held = tracks.time().current_time;
        std::thread::sleep(Duration::from_millis(200));
        assert!((tracks.time().current_time - held).abs() < 1e-9, "paused");
        tracks.seek(2.8);
        tracks.set_live(Some(("t1", song.clone())), false);
        wait("ended", || tracks.time().ended);
        // Play again after the end: from 0.
        tracks.set_live(Some(("t1", song.clone())), true);
        tracks.set_live(Some(("t1", song)), false);
        wait("restarted", || {
            !tracks.time().ended && tracks.time().current_time < 1.0
        });
        // Another track starts from its beginning; none stops and resets.
        let other = fixture("other.wav", "sine=f=330:d=2", &[]);
        tracks.set_live(Some(("t2", other)), false);
        wait("second track", || {
            (tracks.time().duration - 2.0).abs() < 0.01
        });
        tracks.set_live(None, false);
        assert_eq!(tracks.time(), TrackTime::default());
        wait("silent", || tracks.device().is_none());
    }

    #[test]
    fn the_library_drives_the_player() {
        use kairo_domain::media::{TrackItem, TracksLibrary};
        let host = FakeHost::default();
        host.plug("a", "Speakers", true);
        let mut tracks = TracksPlayer::with_host(host);
        let song = ramp("library.wav", 2.0);
        let item = TrackItem {
            id: "t".into(),
            name: "Walk in".into(),
            path: song.to_string_lossy().into_owned(),
            ext: "wav".into(),
            size: 1.0,
        };
        let mut library = TracksLibrary {
            folder: String::new(),
            items: vec![item],
            live_id: Some("t".into()),
            live_paused: false,
            error: None,
        };
        tracks.follow(&library);
        wait("playing", || tracks.time().current_time > 0.1);
        // A live id whose item was removed stops.
        library.items.clear();
        tracks.follow(&library);
        assert_eq!(tracks.time(), TrackTime::default());
    }

    #[test]
    fn a_missing_track_reports_an_error() {
        let host = FakeHost::default();
        host.plug("a", "Speakers", true);
        let mut tracks = TracksPlayer::with_host(host);
        tracks.set_live(Some(("x", crate::tests::dir().join("missing.mp3"))), false);
        wait("error", || tracks.error().is_some());
    }

    #[test]
    fn the_fallback_follows_cue_loop_pause_and_steps_back() {
        let host = FakeHost::default();
        host.plug("a", "Speakers", true);
        let mut fallback = FallbackPlayer::with_host(host);
        let clip = ramp("fallback.wav", 2.0);
        let mut live = FallbackLive {
            path: clip,
            cue: "1".into(),
            start: Some(0.5),
            end: Some(1.0),
            rate: None,
            looped: true,
            paused: false,
            volume: 0.5,
            output_label: String::new(),
        };
        fallback.update(Some(&live));
        assert!(fallback.is_playing());
        let first = fallback.deck.playing.as_ref().unwrap().key.clone();
        live.paused = true;
        fallback.update(Some(&live));
        live.volume = 0.2;
        fallback.update(Some(&live));
        assert!((fallback.deck.playing.as_ref().unwrap().control.gain() - 0.2).abs() < 1e-6);
        assert_eq!(
            fallback.deck.playing.as_ref().unwrap().key,
            first,
            "same player"
        );
        live.cue = "2".into();
        fallback.update(Some(&live));
        assert_ne!(
            fallback.deck.playing.as_ref().unwrap().key,
            first,
            "a new cue is a new player"
        );
        fallback.update(None);
        assert!(!fallback.is_playing());
    }
}

impl Default for TracksPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl TracksPlayer {
    pub fn new() -> Self {
        Self::with_host(SystemHost)
    }

    pub fn with_host(host: impl Host) -> Self {
        Self {
            deck: Deck::new(host),
        }
    }

    /// `presentation.audio.outputLabel`.
    pub fn set_output_label(&mut self, label: &str) {
        self.deck.set_label(label);
    }

    /// `library.liveId` and `livePaused`: `None` stops and unloads.
    pub fn set_live(&mut self, live: Option<(&str, PathBuf)>, paused: bool) {
        let Some((id, path)) = live else {
            self.deck.stop();
            return;
        };
        let same = self.deck.playing.as_ref().is_some_and(|p| p.key == id);
        if !same {
            // A new track starts from 0.
            self.deck
                .load(id.to_owned(), path, ClipSettings::default(), paused, 1.0);
            return;
        }
        if let Some(playing) = &self.deck.playing {
            // `if (audio.ended) currentTime = 0` before `play()`: the clip rewinds itself.
            playing.clip.control(if paused {
                ClipControl::Pause
            } else {
                ClipControl::Play
            });
        }
    }

    /// The library as the tracks service pushes it: the live item plays,
    /// or pauses with `livePaused`; a live id with no item is none.
    pub fn follow(&mut self, library: &kairo_domain::media::TracksLibrary) {
        let live = library
            .live_id
            .as_deref()
            .and_then(|id| library.items.iter().find(|i| i.id == id));
        self.set_live(
            live.map(|i| (i.id.as_str(), PathBuf::from(&i.path))),
            library.live_paused,
        );
    }

    /// `requestSeek`: never before 0.
    pub fn seek(&self, seconds: f64) {
        if let Some(playing) = &self.deck.playing {
            playing.clip.control(ClipControl::Seek(seconds.max(0.0)));
        }
    }

    /// The progress the transport shows.
    pub fn time(&self) -> TrackTime {
        match &self.deck.playing {
            None => TrackTime::default(),
            Some(p) => {
                let s = p.clip.status();
                TrackTime {
                    current_time: s.position,
                    duration: s.duration,
                    ended: s.ended,
                }
            }
        }
    }

    /// The failure of the live track, if it could not play.
    pub fn error(&self) -> Option<String> {
        self.deck
            .playing
            .as_ref()
            .and_then(|p| p.clip.status().error)
    }

    /// The device playing, for diagnostics.
    pub fn device(&self) -> Option<String> {
        self.deck.out.device()
    }
}

/// What the fallback plays: the live background video and its playback.
#[derive(Clone, Debug, PartialEq)]
pub struct FallbackLive {
    pub path: PathBuf,
    /// `liveCue`: a new cue is a fresh player, even of the same clip.
    pub cue: String,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub rate: Option<f64>,
    pub looped: bool,
    pub paused: bool,
    pub volume: f64,
    pub output_label: String,
}

/// `ProgramAudioFallback.tsx`, given whether it is active
/// ([`crate::routing::fallback_active`]).
pub struct FallbackPlayer {
    deck: Deck,
    looped: bool,
    paused: bool,
}

impl Default for FallbackPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl FallbackPlayer {
    pub fn new() -> Self {
        Self::with_host(SystemHost)
    }

    pub fn with_host(host: impl Host) -> Self {
        Self {
            deck: Deck::new(host),
            looped: false,
            paused: false,
        }
    }

    /// `None` when inactive: the room never hears two copies.
    pub fn update(&mut self, live: Option<&FallbackLive>) {
        let Some(live) = live else {
            self.deck.stop();
            return;
        };
        if self.deck.label != live.output_label {
            self.deck.set_label(&live.output_label);
        }
        let key = format!(
            "{}|{}|{:?}|{:?}|{:?}",
            live.path.display(),
            live.cue,
            live.start,
            live.end,
            live.rate
        );
        let gain = live.volume.clamp(0.0, 1.0) as f32;
        let same = self.deck.playing.as_ref().is_some_and(|p| p.key == key);
        if !same {
            let settings = ClipSettings {
                start: live.start,
                end: live.end,
                rate: live.rate.unwrap_or(1.0),
                looped: live.looped,
            };
            self.deck
                .load(key, live.path.clone(), settings, live.paused, gain);
            self.looped = live.looped;
            self.paused = live.paused;
            return;
        }
        let Some(playing) = &self.deck.playing else {
            return;
        };
        if self.looped != live.looped {
            self.looped = live.looped;
            playing.clip.control(ClipControl::Loop(live.looped));
        }
        if self.paused != live.paused {
            self.paused = live.paused;
            playing.clip.control(if live.paused {
                ClipControl::Pause
            } else {
                ClipControl::Play
            });
        }
        // `element.volume`; the mixer ramps it over one render.
        playing.control.set_gain(gain);
    }

    pub fn is_playing(&self) -> bool {
        self.deck.playing.is_some()
    }
}
