//! The scripted service: a looping 1080p video background under lyrics and
//! verses, a running countdown, the info clock, a ticker message, document
//! page turns and a shared live web page, on every output that is available.
//!
//! The script runs on its own thread, the stand-in for the operator UI
//! thread, so the chaos "block the UI thread" event can stall it while the
//! engine and transcription keep going.

use kairo_core::engine::{EngineHandle, Sent};
use kairo_domain::overlay::{MediaFit, OverlayBackgroundType};
use kairo_engine_proto::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// One service cycle; a 10-minute run plays it twice.
pub const CYCLE: Duration = Duration::from_secs(300);
const LYRIC_EVERY: Duration = Duration::from_secs(8);
const VERSE_EVERY: Duration = Duration::from_secs(6);
const PAGE_EVERY: Duration = Duration::from_secs(4);

/// Public-domain text (John Newton, 1779).
const LYRICS: [&str; 6] = [
    "Amazing grace! How sweet the sound\nThat saved a wretch like me!",
    "I once was lost, but now am found;\nWas blind, but now I see.",
    "’Twas grace that taught my heart to fear,\nAnd grace my fears relieved;",
    "How precious did that grace appear\nThe hour I first believed.",
    "Through many dangers, toils and snares,\nI have already come;",
    "’Tis grace hath brought me safe thus far,\nAnd grace will lead me home.",
];

/// King James Version (public domain).
const VERSES: [(&str, &str); 5] = [
    (
        "John 3:16 (KJV)",
        "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
    ),
    (
        "Psalm 23:1 (KJV)",
        "The LORD is my shepherd; I shall not want.",
    ),
    (
        "Romans 8:28 (KJV)",
        "And we know that all things work together for good to them that love God, to them who are the called according to his purpose.",
    ),
    (
        "Isaiah 40:31 (KJV)",
        "But they that wait upon the LORD shall renew their strength; they shall mount up with wings as eagles; they shall run, and not be weary; and they shall walk, and not faint.",
    ),
    (
        "Philippians 4:13 (KJV)",
        "I can do all things through Christ which strengtheneth me.",
    ),
];

pub struct Fixtures {
    pub video: PathBuf,
    pub pages: Vec<PathBuf>,
}

fn ffmpeg(args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y"])
        .args(args)
        .status()
        .map_err(|e| format!("ffmpeg could not start: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("ffmpeg failed: {status}"))
}

/// A 20-second 1080p30 H.264 loop and six 1440 × 1080 document pages.
pub fn make_fixtures(dir: &Path) -> Result<Fixtures, String> {
    let video = dir.join("background-1080p30.mp4");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=1920x1080:r=30:d=20",
        "-pix_fmt",
        "yuv420p",
        "-g",
        "30",
        video.to_str().ok_or("fixture path is not UTF-8")?,
    ])?;
    let pattern = dir.join("page-%d.png");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=1440x1080:r=1:d=6",
        "-frames:v",
        "6",
        pattern.to_str().ok_or("fixture path is not UTF-8")?,
    ])?;
    let pages: Vec<PathBuf> = (1..=6).map(|n| dir.join(format!("page-{n}.png"))).collect();
    if let Some(missing) = pages.iter().find(|p| !p.exists()) {
        return Err(format!("ffmpeg did not write {}", missing.display()));
    }
    Ok(Fixtures { video, pages })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputKind {
    Display,
    Ndi,
    Syphon,
    Card,
}

#[derive(Clone, Debug)]
pub struct Output {
    pub id: String,
    pub kind: OutputKind,
}

/// Every output this machine might drive, as `(id, kind, spec)`.
/// `tag` keeps network feed names apart when two soaks run at once.
pub fn candidate_outputs(
    display: Option<&DisplayInfo>,
    card: bool,
    tag: &str,
) -> Vec<(Output, SurfaceSpec)> {
    let mut out = Vec::new();
    let mut push = |id: &str, kind: OutputKind, sink: SinkSpec| {
        out.push((
            Output {
                id: id.into(),
                kind,
            },
            SurfaceSpec {
                output: id.into(),
                label: format!("Soak {id}"),
                sink,
            },
        ));
    };
    if let Some(display) = display {
        push(
            "screen",
            OutputKind::Display,
            SinkSpec::Display(DisplaySink {
                binding: DisplayBinding {
                    display_id: Some(display.id),
                    label: display.label.clone(),
                    size: Some((display.bounds.width, display.bounds.height)),
                },
                rehearsal: true,
                frame_height: 1080,
            }),
        );
    }
    push(
        "ndi-a",
        OutputKind::Ndi,
        SinkSpec::Ndi {
            name: format!("Kairo Soak {tag} A"),
        },
    );
    push(
        "ndi-b",
        OutputKind::Ndi,
        SinkSpec::Ndi {
            name: format!("Kairo Soak {tag} B"),
        },
    );
    if cfg!(target_os = "macos") {
        push(
            "syphon",
            OutputKind::Syphon,
            SinkSpec::syphon(&format!("Soak {tag}"), 0),
        );
    }
    if card {
        push(
            "card",
            OutputKind::Card,
            SinkSpec::Device {
                port_id: "decklink:sim:0".into(),
                mode_id: "decklink:1".into(),
                key_fill: false,
            },
        );
    }
    out
}

/// Where the web layer goes: an output the script does not use for page
/// turn checks, preferring a network feed.
pub fn web_output(outputs: &[Output]) -> Option<String> {
    [OutputKind::Ndi, OutputKind::Syphon, OutputKind::Display]
        .iter()
        .find_map(|kind| {
            outputs
                .iter()
                .rfind(|o| &o.kind == kind)
                .map(|o| o.id.clone())
        })
}

pub enum Control {
    /// Stall this thread, as a blocked UI thread would.
    Block(Duration),
    Stop,
}

#[derive(Default)]
pub struct Counters {
    pub sent: AtomicU64,
    pub refused: AtomicU64,
    pub slides: AtomicU64,
    pub page_turns: AtomicU64,
    pub layer_updates: AtomicU64,
}

pub struct Script {
    pub engine: EngineHandle,
    /// Outputs that carry the service (the web output shows its page).
    pub outputs: Vec<String>,
    pub web: Option<(String, String)>,
    pub fixtures: Fixtures,
    pub counters: Arc<Counters>,
    pub start: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Lyrics,
    Verses,
    Document,
    VersesWithMessage,
}

/// What the service is doing `at` seconds into a cycle.
pub fn segment(at: Duration) -> Segment {
    let s = (at.as_secs_f64() % CYCLE.as_secs_f64()) as u64;
    match s {
        0..90 => Segment::Lyrics,
        90..180 => Segment::Verses,
        180..240 => Segment::Document,
        _ => Segment::VersesWithMessage,
    }
}

impl Script {
    fn send(&self, command: Command) -> Sent {
        let sent = self.engine.command(command);
        self.counters.sent.fetch_add(1, Ordering::Relaxed);
        if !sent.accepted {
            self.counters.refused.fetch_add(1, Ordering::Relaxed);
        }
        sent
    }

    fn slide(&self, theme_id: &str, reference: &str, text: &str) -> SlideContent {
        let built_in = kairo_domain::theme_library::BUILT_IN_THEMES
            .iter()
            .find(|t| t.id == theme_id)
            .or_else(|| kairo_domain::theme_library::BUILT_IN_THEMES.first())
            .expect("a built-in theme");
        let mut theme = built_in.theme.clone();
        theme.background.kind = OverlayBackgroundType::Video;
        theme.background.media_path = Some(self.fixtures.video.to_string_lossy().into_owned());
        theme.background.media_loop = Some(true);
        // The same file and cue on every slide keep the video running.
        theme.background.media_cue = Some(1.0);
        theme.background.opacity = 1.0;
        SlideContent {
            reference: reference.into(),
            text: text.into(),
            theme: Json::encode(&theme),
            colored_lines: None,
        }
    }

    fn page_slide(&self, page: usize) -> SlideContent {
        let mut theme = kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME.clone();
        theme.background.kind = OverlayBackgroundType::Image;
        theme.background.media_path =
            Some(self.fixtures.pages[page].to_string_lossy().into_owned());
        theme.background.media_fit = Some(MediaFit::Contain);
        theme.background.opacity = 1.0;
        SlideContent {
            reference: String::new(),
            text: String::new(),
            theme: Json::encode(&theme),
            colored_lines: None,
        }
    }

    fn deck(&self, page: usize) -> Json {
        Json::from_value(&serde_json::json!({
            "id": "soak-deck", "page": page,
            "path": self.fixtures.pages[page], "width": 1440, "height": 1080, "videos": []
        }))
    }

    fn show(&self, slide: &SlideContent) {
        for output in &self.outputs {
            self.send(Command::ShowSlide {
                output: output.clone(),
                source: output.clone(),
                slide: slide.clone(),
                instant: false,
            });
        }
        self.counters.slides.fetch_add(1, Ordering::Relaxed);
    }

    fn set_layer(&self, kind: LayerKind, layer: Option<serde_json::Value>) {
        for output in &self.outputs {
            self.send(Command::SetLayer {
                output: output.clone(),
                kind,
                layer: layer.as_ref().map(Json::from_value),
            });
        }
        self.counters.layer_updates.fetch_add(1, Ordering::Relaxed);
    }

    fn countdown(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64;
        let run = serde_json::json!({"durationSec": 300, "endsAt": now + 300_000.0,
            "remainingSec": 300, "rollover": true, "now": now});
        self.set_layer(
            LayerKind::Props,
            Some(serde_json::json!({"kind": "props", "timer": run,
                "props": [{"id": "countdown", "name": "Countdown", "background": null, "objects": [
                    {"id": "s", "kind": "shape", "x": 0.68, "y": 0.04, "w": 0.3, "h": 0.12,
                     "opacity": 0.8, "shape": "rectangle", "fill": "#101820", "radiusPx": 24},
                    {"id": "r", "kind": "timer", "x": 0.68, "y": 0.05, "w": 0.3, "h": 0.1,
                     "opacity": 1, "fontSizePx": 80, "color": "#5eead4",
                     "overrunColor": "#f87171", "bold": true, "align": "center"}]}]})),
        );
        for output in &self.outputs {
            self.send(Command::SetInfo {
                output: output.clone(),
                info: Some(Json::from_value(&serde_json::json!({"clock": true,
                    "stageMessage": "Service in progress",
                    "layout": {"position": "top", "size": "medium"},
                    "sizes": {"barPx": 72, "messagePx": 60},
                    "style": {"rollover": true, "color": "#5eead4",
                              "overrunColor": "#f87171", "backdrop": false}}))),
            });
        }
    }

    fn message(&self, on: bool) {
        self.set_layer(
            LayerKind::Message,
            on.then(|| {
                serde_json::json!({"kind": "message",
                "text": "Parents of child 42, please come to the nursery   ·   Welcome to everyone joining online",
                "style": {"position": "bottom", "fontSizePx": 44, "color": "#ffffff",
                          "background": "rgba(0,0,0,0.72)", "scroll": true}})
            }),
        );
    }

    fn start_web(&self) {
        if let Some((output, url)) = &self.web {
            self.send(Command::Configure {
                output: output.clone(),
                config: SurfaceConfig {
                    web_enabled: true,
                    ..SurfaceConfig::default()
                },
            });
            self.send(Command::SetWeb {
                output: output.clone(),
                web: Some(WebLayer {
                    url: url.clone(),
                    reload: 0.,
                }),
            });
        }
    }

    /// Runs until `Stop` or the channel closes.
    pub fn run(self, control: mpsc::Receiver<Control>) {
        self.start_web();
        self.countdown();
        let (mut lyric, mut verse, mut page) = (0usize, 0usize, 0usize);
        let mut due = Instant::now();
        let mut last_segment = None;
        let mut message_on = false;
        let mut cycle = u64::MAX;
        loop {
            match control.recv_timeout(Duration::from_millis(100)) {
                Ok(Control::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Ok(Control::Block(duration)) => {
                    tracing::info!("script thread blocked for {duration:?}");
                    std::thread::sleep(duration);
                    tracing::info!("script thread resumed");
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let elapsed = self.start.elapsed();
            let this_cycle = elapsed.as_secs() / CYCLE.as_secs();
            if this_cycle != cycle {
                cycle = this_cycle;
                self.countdown();
            }
            let now_segment = segment(elapsed);
            let entering = last_segment != Some(now_segment);
            last_segment = Some(now_segment);
            let want_message = now_segment == Segment::VersesWithMessage
                || (now_segment == Segment::Lyrics
                    && (60..80).contains(&(elapsed.as_secs() % CYCLE.as_secs())));
            if want_message != message_on {
                message_on = want_message;
                self.message(message_on);
            }
            if !entering && Instant::now() < due {
                continue;
            }
            match now_segment {
                Segment::Lyrics => {
                    self.show(&self.slide("lyrics-stage", "", LYRICS[lyric % LYRICS.len()]));
                    lyric += 1;
                    due = Instant::now() + LYRIC_EVERY;
                }
                Segment::Verses | Segment::VersesWithMessage => {
                    let (reference, text) = VERSES[verse % VERSES.len()];
                    self.show(&self.slide("broadcast", reference, text));
                    verse += 1;
                    due = Instant::now() + VERSE_EVERY;
                }
                Segment::Document => {
                    if entering {
                        page = 0;
                        for output in &self.outputs {
                            self.send(Command::ShowDocument {
                                output: output.clone(),
                                source: output.clone(),
                                slide: self.page_slide(0),
                                document: Some(self.deck(0)),
                            });
                        }
                    } else {
                        page = (page + 1) % self.fixtures.pages.len();
                        for output in &self.outputs {
                            self.send(Command::SwapPage {
                                output: output.clone(),
                                source: output.clone(),
                                media_path: self.fixtures.pages[page].to_string_lossy().into(),
                                document: Some(self.deck(page)),
                            });
                        }
                        let next = (page + 1) % self.fixtures.pages.len();
                        for output in &self.outputs {
                            self.send(Command::PreloadPage {
                                output: output.clone(),
                                media_path: self.fixtures.pages[next].to_string_lossy().into(),
                            });
                        }
                    }
                    self.counters.page_turns.fetch_add(1, Ordering::Relaxed);
                    due = Instant::now() + PAGE_EVERY;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cycle_has_every_segment_in_order() {
        let at = |s: u64| segment(Duration::from_secs(s));
        assert_eq!(at(0), Segment::Lyrics);
        assert_eq!(at(89), Segment::Lyrics);
        assert_eq!(at(90), Segment::Verses);
        assert_eq!(at(180), Segment::Document);
        assert_eq!(at(240), Segment::VersesWithMessage);
        assert_eq!(at(299), Segment::VersesWithMessage);
        assert_eq!(at(300), Segment::Lyrics);
        assert_eq!(at(480), Segment::Document);
    }

    #[test]
    fn the_web_layer_prefers_a_network_feed() {
        let o = |id: &str, kind| Output {
            id: id.into(),
            kind,
        };
        assert_eq!(
            web_output(&[
                o("screen", OutputKind::Display),
                o("ndi-a", OutputKind::Ndi),
                o("ndi-b", OutputKind::Ndi)
            ]),
            Some("ndi-b".into())
        );
        assert_eq!(
            web_output(&[
                o("screen", OutputKind::Display),
                o("card", OutputKind::Card)
            ]),
            Some("screen".into())
        );
        assert_eq!(web_output(&[o("card", OutputKind::Card)]), None);
        assert_eq!(web_output(&[]), None);
    }

    #[test]
    fn candidates_include_a_display_and_card_only_when_present() {
        let none = candidate_outputs(None, false, "t");
        assert!(none.iter().all(|(o, _)| o.kind != OutputKind::Display));
        assert!(none.iter().all(|(o, _)| o.kind != OutputKind::Card));
        let display = DisplayInfo {
            id: 7,
            label: "Test".into(),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            scale_factor: 1.0,
            native_id: 7,
            primary: true,
            hosts_main_window: true,
            refresh_millihertz: None,
        };
        let all = candidate_outputs(Some(&display), true, "t");
        assert_eq!(all[0].0.id, "screen");
        assert!(all.iter().any(|(o, _)| o.kind == OutputKind::Card));
    }
}
