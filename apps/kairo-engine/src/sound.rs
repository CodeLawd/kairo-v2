//! Program sound in the engine (prompt 056): `overlay.html`'s audio graph
//! for every surface at once.
//!
//! Each background (and document) video's sound is a clip player keyed by
//! the same source id as its picture, so outputs showing the same file at
//! the same cue share it, as they share the decoder. Its speaker gain is
//! the volume when an output showing it is the audio surface
//! (`config.audio.enabled`, which the client sets on exactly one surface),
//! else 0; it reaches the NDI tap of every output showing it whose
//! `ndiAudio` is on, at full level. Element videos are decoration and never
//! make sound.
//!
//! A live camera's audio input (prompt 057) is a source of its own, one per
//! input label, shared by every output that shows it. Its speaker gain is
//! the volume on the audio surface when that surface's `cameraAudible` is
//! set (`applyCameraAudio`); it reaches the NDI tap of every output that
//! shows it. An input that stops is opened again every second.
//!
//! The picture keeps the clock (prompt 055): the sound follows the video
//! player's position, pause and end, and is moved when it drifts.

use kairo_audio::mixer::{MAX_TAPS, MixerControl, SourceControl, SourceKind, TapSwitch, mixer};
use kairo_audio::output::{AudioOut, SystemHost};
use kairo_audio::tap::TapSender;
use kairo_audio::{AudioBlock, ClipControl, ClipPlayer, ClipSettings};
use kairo_compositor::MediaUse;
use kairo_engine_proto::SurfaceConfig;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// How often a camera input that stopped is tried again.
const CAMERA_RETRY: std::time::Duration = std::time::Duration::from_secs(1);

fn open_input(
    label: &str,
    input: kairo_audio::mixer::SourceInput,
) -> Option<kairo_camera::mic::Mic> {
    match kairo_camera::mic::open(label, input) {
        Ok(mic) => Some(mic),
        Err(error) => {
            tracing::warn!(%label, %error, "camera sound is not available");
            None
        }
    }
}

/// Sound further than this from the picture is moved to it.
pub const DRIFT: f64 = 0.08;

struct VideoSound {
    settings: ClipSettings,
    source: u64,
    control: Arc<SourceControl>,
    clip: ClipPlayer,
    looped: bool,
    playing: bool,
}

struct CameraSound {
    source: u64,
    control: Arc<SourceControl>,
    mic: Option<kairo_camera::mic::Mic>,
    retry_at: std::time::Instant,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct SurfaceSound {
    enabled: bool,
    camera_audible: bool,
    volume: f64,
    label: String,
    ndi_audio: bool,
}

/// What the picture is doing, for the sound to follow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PictureClock {
    pub position: f64,
    pub playing: bool,
}

/// Where tapped blocks go: the NDI feed of the output that owns the tap.
type NdiAudio = Arc<std::sync::Mutex<Option<Arc<dyn Fn(&str, &AudioBlock) + Send + Sync>>>>;

pub struct Sound {
    /// `tap_owner` as the tap sender thread sees it.
    owners: Arc<std::sync::Mutex<[Option<String>; MAX_TAPS]>>,
    ndi: NdiAudio,
    out: AudioOut,
    control: MixerControl,
    taps: Vec<Arc<TapSwitch>>,
    _sender: TapSender,
    /// The output each tap serves.
    tap_owner: [Option<String>; MAX_TAPS],
    surfaces: HashMap<String, SurfaceSound>,
    uses: HashMap<String, Vec<MediaUse>>,
    videos: HashMap<u64, VideoSound>,
    /// Each output's camera audio input label.
    camera_inputs: HashMap<String, String>,
    /// Camera audio inputs being captured, by label.
    cameras: HashMap<String, CameraSound>,
    /// Blocks tapped with no NDI feed to take them.
    pub unsent: Arc<AtomicU64>,
    /// Sound minus picture while both play: samples, sum of |d|, worst |d|.
    drift: (u64, f64, f64),
    resyncs: u64,
}

fn clamp_volume(v: f64) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        let (n, sum, worst) = self.drift;
        if n > 0 || self.resyncs > 0 {
            tracing::info!(
                samples = n,
                mean_ms = format!("{:.1}", sum / n.max(1) as f64 * 1000.0),
                worst_ms = format!("{:.1}", worst * 1000.0),
                resyncs = self.resyncs,
                device_glitches = self.out.stats().underruns.load(Ordering::Relaxed),
                source_glitches =
                    self.glitches() - self.out.stats().underruns.load(Ordering::Relaxed),
                "program sound against its picture"
            );
        }
    }
}

impl Default for Sound {
    fn default() -> Self {
        Self::new()
    }
}

impl Sound {
    pub fn new() -> Self {
        let (mixer, control, outputs) = mixer(MAX_TAPS);
        let unsent = Arc::new(AtomicU64::new(0));
        let taps = outputs.iter().map(|o| o.switch.clone()).collect();
        let counter = unsent.clone();
        let owners: Arc<std::sync::Mutex<[Option<String>; MAX_TAPS]>> = Arc::default();
        let ndi: NdiAudio = Arc::default();
        let (seen, send) = (owners.clone(), ndi.clone());
        let sender = TapSender::start(outputs, move |tap, block: AudioBlock| {
            // The block passed `handleNdiAudio`'s checks.
            let owner = seen.lock().unwrap_or_else(|e| e.into_inner())[tap].clone();
            let route = send.lock().unwrap_or_else(|e| e.into_inner()).clone();
            match (owner, route) {
                (Some(owner), Some(route)) => route(&owner, &block),
                _ => {
                    if counter.fetch_add(1, Ordering::Relaxed) == 0 {
                        tracing::info!(
                            samples = block.samples,
                            "NDI audio is being tapped with no NDI feed to carry it"
                        );
                    }
                }
            }
        });
        Self {
            owners,
            ndi,
            out: AudioOut::start(mixer, SystemHost),
            control,
            taps,
            _sender: sender,
            tap_owner: Default::default(),
            surfaces: HashMap::new(),
            uses: HashMap::new(),
            videos: HashMap::new(),
            camera_inputs: HashMap::new(),
            cameras: HashMap::new(),
            unsent,
            drift: (0, 0.0, 0.0),
            resyncs: 0,
        }
    }

    /// `__configure`'s sound settings for `output`.
    pub fn configure(&mut self, output: &str, config: &SurfaceConfig) {
        self.surfaces.insert(
            output.to_owned(),
            SurfaceSound {
                enabled: config.audio.enabled,
                camera_audible: config.camera_audible,
                volume: config.audio.volume,
                label: config.audio.output_label.clone(),
                ndi_audio: config.ndi_audio,
            },
        );
        self.route();
    }

    /// A surface is gone: its sound and its tap with it.
    pub fn forget(&mut self, output: &str) {
        self.surfaces.remove(output);
        self.uses.remove(output);
        self.camera_inputs.remove(output);
        self.reconcile_cameras();
        self.reconcile();
    }

    /// Only these surfaces remain.
    pub fn retain(&mut self, outputs: &[String]) {
        let gone: Vec<String> = self
            .surfaces
            .keys()
            .chain(self.uses.keys())
            .chain(self.camera_inputs.keys())
            .filter(|o| !outputs.contains(o))
            .cloned()
            .collect();
        for output in gone {
            self.forget(&output);
        }
    }

    /// The compositor's report of `output`'s video nodes.
    pub fn media(&mut self, output: &str, uses: &[MediaUse]) {
        let sounding: Vec<MediaUse> = uses.iter().filter(|u| !u.params.element).cloned().collect();
        if sounding.is_empty() {
            self.uses.remove(output);
        } else {
            self.uses.insert(output.to_owned(), sounding);
        }
        self.reconcile();
    }

    /// Clip players for every video shown, none for the rest.
    fn reconcile(&mut self) {
        let mut shown: HashMap<u64, &MediaUse> = HashMap::new();
        for u in self.uses.values().flatten() {
            shown.entry(u.id).or_insert(u);
        }
        let gone: Vec<u64> = self
            .videos
            .keys()
            .copied()
            .filter(|id| !shown.contains_key(id))
            .collect();
        for id in gone {
            if let Some(video) = self.videos.remove(&id) {
                drop(video.clip);
                self.control.remove(video.source);
            }
        }
        for (id, u) in shown {
            if let Some(video) = self.videos.get_mut(&id) {
                if video.looped != u.params.looped {
                    video.looped = u.params.looped;
                    video.clip.control(ClipControl::Loop(u.params.looped));
                }
                continue;
            }
            let Some((source, input)) = self.control.add(SourceKind::Video) else {
                tracing::warn!("too many sounds at once; a video plays silent");
                continue;
            };
            let control = input.control.clone();
            let settings = ClipSettings {
                start: u.params.start_seconds,
                end: u.params.end_seconds,
                rate: u.params.rate,
                looped: u.params.looped,
            };
            // Paused until the picture is seen to play.
            let clip = ClipPlayer::start(PathBuf::from(&*u.params.path), settings, true, input);
            self.videos.insert(
                id,
                VideoSound {
                    settings,
                    source,
                    control,
                    clip,
                    looped: u.params.looped,
                    playing: false,
                },
            );
        }
        self.route();
    }

    /// `output`'s camera audio input, or none (`__setCamera`'s `audioLabel`).
    pub fn camera(&mut self, output: &str, input: Option<&str>) {
        match input.filter(|l| !l.is_empty()) {
            Some(label) => {
                self.camera_inputs
                    .insert(output.to_owned(), label.to_owned());
            }
            None => {
                self.camera_inputs.remove(output);
            }
        }
        self.reconcile_cameras();
        self.route();
    }

    /// One capture per input label in use, none for the rest.
    fn reconcile_cameras(&mut self) {
        let wanted: HashSet<String> = self.camera_inputs.values().cloned().collect();
        let gone: Vec<String> = self
            .cameras
            .keys()
            .filter(|l| !wanted.contains(*l))
            .cloned()
            .collect();
        for label in gone {
            if let Some(camera) = self.cameras.remove(&label) {
                drop(camera.mic);
                self.control.remove(camera.source);
            }
        }
        for label in wanted {
            if self.cameras.contains_key(&label) {
                continue;
            }
            let Some((source, input)) = self.control.add(SourceKind::Camera) else {
                tracing::warn!(%label, "too many sounds at once; the camera plays silent");
                continue;
            };
            let control = input.control.clone();
            let mic = open_input(&label, input);
            self.cameras.insert(
                label,
                CameraSound {
                    source,
                    control,
                    mic,
                    retry_at: std::time::Instant::now() + CAMERA_RETRY,
                },
            );
        }
    }

    /// Camera inputs that stopped or never opened are tried again.
    pub fn check_cameras(&mut self) {
        let now = std::time::Instant::now();
        let mut reopen = Vec::new();
        for (label, camera) in &mut self.cameras {
            let stopped = camera.mic.as_ref().is_none_or(|m| m.failed());
            if stopped && now >= camera.retry_at {
                camera.retry_at = now + CAMERA_RETRY;
                if camera.mic.take().is_some() {
                    tracing::warn!(%label, "camera sound stopped; trying again");
                }
                reopen.push(label.clone());
            }
        }
        for label in reopen {
            // A fresh source: the old ring may hold a stale tail.
            if let Some(camera) = self.cameras.remove(&label) {
                self.control.remove(camera.source);
            }
            let Some((source, input)) = self.control.add(SourceKind::Camera) else {
                continue;
            };
            let control = input.control.clone();
            let mic = open_input(&label, input);
            self.cameras.insert(
                label,
                CameraSound {
                    source,
                    control,
                    mic,
                    retry_at: now + CAMERA_RETRY,
                },
            );
        }
        if !self.cameras.is_empty() {
            self.route();
        }
    }

    /// Whether camera sound is being captured or looked for.
    pub fn has_cameras(&self) -> bool {
        !self.cameras.is_empty()
    }

    /// Gains, taps and the device, from who is audible and who taps.
    fn route(&mut self) {
        // At most one surface is enabled; if a client sends more, the
        // first by name wins, so the output never doubles.
        let mut names: Vec<&String> = self.surfaces.keys().collect();
        names.sort();
        let speaker = names
            .iter()
            .find(|n| self.surfaces[**n].enabled)
            .map(|n| (*n).clone());
        let label = speaker
            .as_ref()
            .map(|s| self.surfaces[s].label.clone())
            .unwrap_or_default();
        self.out.set_output(
            speaker.is_some() && (!self.videos.is_empty() || !self.cameras.is_empty()),
            &label,
        );
        // Taps: one per surface with NDI audio on.
        let tapping: HashSet<&String> = self
            .surfaces
            .iter()
            .filter(|(_, s)| s.ndi_audio)
            .map(|(o, _)| o)
            .collect();
        for (i, owner) in self.tap_owner.iter_mut().enumerate() {
            if owner.as_ref().is_some_and(|o| !tapping.contains(o)) {
                *owner = None;
                self.taps[i].set_on(false);
            }
        }
        for output in tapping {
            if self.tap_owner.iter().flatten().any(|o| o == output) {
                continue;
            }
            match self.tap_owner.iter().position(Option::is_none) {
                Some(i) => {
                    self.tap_owner[i] = Some(output.clone());
                    self.taps[i].set_on(true);
                }
                None => tracing::warn!(output, "more NDI audio feeds than taps"),
            }
        }
        for (id, video) in &self.videos {
            let on = |output: &String| {
                self.uses
                    .get(output)
                    .is_some_and(|u| u.iter().any(|u| u.id == *id))
            };
            let gain = speaker
                .as_ref()
                .filter(|s| on(s))
                .map_or(0.0, |s| clamp_volume(self.surfaces[s].volume));
            video.control.set_gain(gain);
            let mut mask = 0u8;
            for (i, owner) in self.tap_owner.iter().enumerate() {
                if owner.as_ref().is_some_and(on) {
                    mask |= 1 << i;
                }
            }
            video.control.set_taps(mask);
        }
        self.route_cameras(speaker.as_ref());
        *self.owners.lock().unwrap_or_else(|e| e.into_inner()) = self.tap_owner.clone();
    }

    /// Sends each tap's blocks to the NDI feed of the output it serves.
    pub fn send_ndi_audio(&mut self, route: impl Fn(&str, &AudioBlock) + Send + Sync + 'static) {
        *self.ndi.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(route));
    }

    fn route_cameras(&self, speaker: Option<&String>) {
        for (label, camera) in &self.cameras {
            let shows = |output: &String| self.camera_inputs.get(output) == Some(label);
            let gain = speaker
                .filter(|s| shows(s) && self.surfaces[*s].camera_audible)
                .map_or(0.0, |s| clamp_volume(self.surfaces[s].volume));
            camera.control.set_gain(gain);
            let mut mask = 0u8;
            for (i, owner) in self.tap_owner.iter().enumerate() {
                if owner.as_ref().is_some_and(shows) {
                    mask |= 1 << i;
                }
            }
            camera.control.set_taps(mask);
        }
    }

    /// `__setVideoPaused` and `__seekVideo` on source `id`.
    pub fn control(&mut self, id: u64, control: ClipControl) {
        if let Some(video) = self.videos.get(&id) {
            video.clip.control(control);
        }
    }

    /// Whether any video has sound to keep in step.
    pub fn active(&self) -> bool {
        !self.videos.is_empty()
    }

    /// Keeps each video's sound with its picture: playing while it plays,
    /// paused while it is paused or ended, moved when it drifts.
    pub fn follow(&mut self, clock: impl Fn(u64) -> Option<PictureClock>) {
        // What is heard lags what the mixer takes by the device queue: aim
        // the sound that much ahead so it is heard with its picture.
        let latency = self.out.latency();
        for (id, video) in &mut self.videos {
            let Some(picture) = clock(*id) else { continue };
            let status = video.clip.status();
            if !status.ready || status.silent || status.error.is_some() {
                continue;
            }
            if picture.playing != video.playing {
                video.playing = picture.playing;
                if picture.playing {
                    video
                        .clip
                        .control(ClipControl::Seek(picture.position + latency));
                    video.clip.control(ClipControl::Play);
                } else {
                    video.clip.control(ClipControl::Pause);
                    video.clip.control(ClipControl::Seek(picture.position));
                }
                continue;
            }
            if picture.playing && !status.paused {
                let heard = status.position - latency;
                let mut d = (heard - picture.position).abs();
                // Both wrap a loop on their own: compare across the wrap.
                let (start, end) = kairo_audio::clip::bounds(
                    &video.settings,
                    Some(status.duration).filter(|d| *d > 0.0),
                );
                let span = end - start;
                if video.looped && span.is_finite() && span > 0.0 {
                    d %= span;
                    d = d.min(span - d);
                }
                if d > DRIFT {
                    self.resyncs += 1;
                    video
                        .clip
                        .control(ClipControl::Seek(picture.position + latency));
                } else {
                    self.drift.0 += 1;
                    self.drift.1 += d;
                    self.drift.2 = self.drift.2.max(d);
                }
            }
        }
    }

    /// The device playing, `None` when silent.
    pub fn device(&self) -> Option<String> {
        self.out.device()
    }

    /// Device frames that had to be silence.
    pub fn glitches(&self) -> u64 {
        self.out.stats().underruns.load(Ordering::Relaxed)
            + self
                .videos
                .values()
                .map(|v| v.control.underruns())
                .sum::<u64>()
    }

    /// Whether `output`'s NDI tap is collecting sound.
    pub fn tapping(&self, output: &str) -> bool {
        self.tap_owner
            .iter()
            .position(|o| o.as_deref() == Some(output))
            .is_some_and(|i| self.taps[i].is_on())
    }

    /// The speaker gain and tap mask of source `id`.
    #[cfg(test)]
    pub fn routing(&self, id: u64) -> Option<(f32, u8)> {
        self.videos
            .get(&id)
            .map(|v| (v.control.gain(), v.control.taps()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_compositor::MediaParams;
    use kairo_engine_proto::AudioConfig;
    use std::time::{Duration, Instant};

    fn video(id: u64, path: &str, element: bool) -> MediaUse {
        MediaUse {
            node: id * 10,
            id,
            params: MediaParams {
                path: path.into(),
                looped: false,
                start_seconds: None,
                end_seconds: None,
                rate: 1.0,
                element,
            },
            transport: !element,
        }
    }

    fn config(enabled: bool, volume: f64, ndi_audio: bool) -> SurfaceConfig {
        SurfaceConfig {
            audio: AudioConfig {
                enabled,
                volume,
                output_label: String::new(),
            },
            ndi_audio,
            ..SurfaceConfig::default()
        }
    }

    #[test]
    fn only_the_audio_surface_is_audible_and_taps_hear_their_own_videos() {
        let mut sound = Sound::new();
        // Silent while configuring, so no speaker opens during the test.
        sound.configure("screen", &config(false, 0.6, false));
        sound.configure("ndi", &config(false, 0.6, true));
        sound.media(
            "screen",
            &[
                video(1, "/nope/a.mp4", false),
                video(9, "/nope/e.mp4", true),
            ],
        );
        sound.media(
            "ndi",
            &[
                video(1, "/nope/a.mp4", false),
                video(2, "/nope/b.mp4", false),
            ],
        );
        // Element videos never make sound.
        assert!(sound.routing(9).is_none());
        assert!(sound.tapping("ndi") && !sound.tapping("screen"));
        let (g1, m1) = sound.routing(1).unwrap();
        let (g2, m2) = sound.routing(2).unwrap();
        assert_eq!((g1, g2), (0.0, 0.0));
        // Both reach the NDI tap, at full level whatever the speaker does.
        assert_eq!(m1, m2);
        assert_ne!(m1, 0);
        // The screen becomes the audio surface: its video takes the volume.
        sound.configure("screen", &config(true, 0.6, false));
        assert!((sound.routing(1).unwrap().0 - 0.6).abs() < 1e-6);
        assert_eq!(
            sound.routing(2).unwrap().0,
            0.0,
            "not shown on the audio surface"
        );
        // NDI audio off: the tap stops and no video feeds it.
        sound.configure("ndi", &config(false, 0.6, false));
        assert!(!sound.tapping("ndi"));
        assert_eq!(sound.routing(2).unwrap().1, 0);
        sound.configure("screen", &config(false, 0.6, false));
        // A surface that goes takes its videos' sound with it.
        sound.forget("ndi");
        assert!(sound.routing(2).is_none());
        sound.retain(&[]);
        assert!(!sound.active());
    }

    fn ramp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kairo-sound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ramp.wav");
        if !path.exists() {
            let ok = std::process::Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "aevalsrc=exprs=0.1*t|0.1*t:s=48000:d=6",
                    "-c:a",
                    "pcm_f32le",
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success();
            assert!(ok);
        }
        path
    }

    #[test]
    fn video_sound_follows_its_picture() {
        let path = ramp();
        let mut sound = Sound::new();
        sound.configure("screen", &config(false, 1.0, false));
        sound.media("screen", &[video(1, path.to_str().unwrap(), false)]);
        fn status(sound: &Sound) -> kairo_audio::ClipStatus {
            sound.videos[&1].clip.status()
        }
        let end = Instant::now() + Duration::from_secs(5);
        while !status(&sound).ready {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(status(&sound).paused, "waits for the picture");
        // The picture plays from 2 s.
        let mut picture = PictureClock {
            position: 2.0,
            playing: true,
        };
        let started = Instant::now();
        sound.follow(|_| Some(picture));
        std::thread::sleep(Duration::from_millis(300));
        let heard = status(&sound).position;
        let expected = 2.0 + started.elapsed().as_secs_f64();
        assert!((heard - expected).abs() < 0.15, "{heard} vs {expected}");
        // The picture jumps (a seek or a loop): the sound is moved to it.
        picture.position = 4.5;
        sound.follow(|_| Some(picture));
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            (status(&sound).position - 4.6).abs() < 0.15,
            "{}",
            status(&sound).position
        );
        // Paused picture: paused sound, where the picture stopped.
        picture = PictureClock {
            position: 4.8,
            playing: false,
        };
        sound.follow(|_| Some(picture));
        std::thread::sleep(Duration::from_millis(100));
        assert!(status(&sound).paused);
        assert!(
            (status(&sound).position - 4.8).abs() < 0.02,
            "{}",
            status(&sound).position
        );
    }

    fn camera_config(enabled: bool, camera_audible: bool, ndi_audio: bool) -> SurfaceConfig {
        SurfaceConfig {
            camera_audible,
            ..config(enabled, 0.6, ndi_audio)
        }
    }

    #[test]
    fn camera_sound_is_heard_only_where_camera_audible_and_tapped_where_shown() {
        let mut sound = Sound::new();
        sound.configure("screen", &camera_config(true, true, false));
        sound.configure("ndi", &camera_config(false, false, true));
        // An input that does not exist: its source is still routed.
        sound.camera("screen", Some("No Such Input (0000:0000)"));
        sound.camera("ndi", Some("No Such Input (0000:0000)"));
        assert!(sound.has_cameras());
        let camera = &sound.cameras["No Such Input (0000:0000)"];
        assert!(camera.mic.is_none());
        assert!((camera.control.gain() - 0.6).abs() < 1e-6);
        assert_eq!(camera.control.taps().count_ones(), 1);
        // Not camera-audible: silent in the room, still on NDI.
        sound.configure("screen", &camera_config(true, false, false));
        let camera = &sound.cameras["No Such Input (0000:0000)"];
        assert_eq!(camera.control.gain(), 0.0);
        assert_eq!(camera.control.taps().count_ones(), 1);
        // Both outputs drop it: the source goes.
        sound.camera("screen", None);
        sound.camera("ndi", Some(""));
        assert!(!sound.has_cameras());
        // Retrying a missing input keeps one source.
        sound.camera("screen", Some("No Such Input"));
        sound.cameras.get_mut("No Such Input").unwrap().retry_at = std::time::Instant::now();
        sound.check_cameras();
        assert_eq!(sound.cameras.len(), 1);
        sound.forget("screen");
        assert!(!sound.has_cameras());
    }
}
