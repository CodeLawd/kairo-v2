//! The `overlay.html` shell API on top of the compositor: which layer each
//! command touches, with what transition. Commands arrive here only after
//! the shared surface reducer has applied them.
//!
//! Layer pixels come from the scene builder in later prompts (text 052,
//! backgrounds and elements 053, program layers 054, video 055, camera 057,
//! documents 058, web 059). Until then their nodes exist without pixels, so
//! every swap, retire and crossfade already happens as it will; colour
//! backgrounds draw now.

use kairo_compositor::thread::Op;
use kairo_compositor::{Content, Fill, Rect, Slot, VideoLayout, clamp_transition, video_layout};
use kairo_engine_proto::*;
use serde_json::{Value, json};
use std::collections::HashSet;

const FRAME_WIDTH: f64 = 1920.0;
const OPAQUE_BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// One surface's shell settings (`config`, `webShown`).
pub struct Shell {
    frame_height: u32,
    ms: u32,
    web_shown: Option<String>,
    /// Things not drawn yet, logged once per surface.
    noted: HashSet<String>,
    output: String,
}

impl Shell {
    pub fn new(output: &str, frame_height: u32) -> Self {
        Self {
            frame_height: kairo_compositor::frame_height(frame_height),
            ms: 0,
            web_shown: None,
            noted: HashSet::new(),
            output: output.to_owned(),
        }
    }

    fn note(&mut self, what: String) {
        if self.noted.insert(what.clone()) {
            tracing::info!(output = self.output, "{what}");
        }
    }

    /// The operations for one applied command; `before` and `state` are
    /// the surface before and after it.
    pub fn command(
        &mut self,
        command: &Command,
        before: &SurfaceState,
        state: &SurfaceState,
    ) -> Vec<Op> {
        let ms = self.ms;
        match command {
            Command::Configure { config, .. } => self.configure(config),
            Command::ShowSlide { instant, .. } => self.paint(state, *instant),
            Command::SetBackground { .. } => self.paint(state, false),
            Command::ClearText { background, .. } => {
                // A changed background repaints the whole slide; otherwise
                // only the words go (`__clearText`).
                if background.is_some() && background_of(before) != background_of(state) {
                    self.paint(state, false)
                } else {
                    vec![Op::Swap {
                        slot: Slot::Text,
                        content: None,
                        ms,
                    }]
                }
            }
            Command::Clear { .. } => self.set_content(None, false),
            Command::ShowDocument { .. } => {
                let mut ops = self.paint(state, true);
                ops.push(Op::ProgramBackground(Some(OPAQUE_BLACK)));
                ops
            }
            Command::PatchBackground { .. } => match state.last_render.as_ref() {
                // Same file, so the node is kept and takes the new filter
                // and loop (`setBackground`'s keyed path).
                Some(slide) => vec![self.background(slide, 0)],
                None => vec![],
            },
            Command::SetLayer { kind, markup, .. } => vec![Op::Swap {
                slot: layer_slot(*kind),
                content: (!markup.is_empty()).then(Content::empty),
                ms,
            }],
            // The info layer is shown and hidden, never faded.
            Command::SetInfo { info, .. } => vec![Op::Swap {
                slot: Slot::Info,
                content: info.as_ref().map(|_| Content::empty()),
                ms: 0,
            }],
            Command::SetWeb { web, .. } => self.web(web.as_ref()),
            Command::WebPointer { point, .. } => vec![self.pointer(*point)],
            Command::SetCamera { camera, .. } => match camera {
                None => vec![Op::Swap {
                    slot: Slot::Camera,
                    content: None,
                    ms,
                }],
                Some(camera) => {
                    // `__setCamera` with no matching device keeps the old node.
                    self.note(format!(
                        "camera {:?} is not captured in this engine build (prompt 057)",
                        camera.label
                    ));
                    vec![]
                }
            },
            Command::SetTestPattern { pattern, .. } => {
                if pattern.is_some() {
                    self.note("test patterns are drawn from prompt 060".into());
                }
                vec![]
            }
            // No pixels change in this engine yet: video (055), document
            // pages and their videos (058), web scrolling (059).
            Command::SwapPage { .. }
            | Command::PreloadPage { .. }
            | Command::DocumentVideos { .. }
            | Command::ControlDocumentVideo { .. }
            | Command::VideoTransport { .. }
            | Command::WebScroll { .. } => vec![],
            Command::OpenSurface(_)
            | Command::RetainSurfaces { .. }
            | Command::CloseSurface { .. }
            | Command::RestoreSurface { .. }
            | Command::Shutdown => vec![],
        }
    }

    /// Everything needed to show `state` on a freshly opened output, as
    /// `ready` and `recover` rebuild a window.
    pub fn rebuild(&mut self, state: &SurfaceState) -> Vec<Op> {
        self.web_shown = None;
        let mut ops = self.configure(&state.config);
        for kind in LayerKind::ALL {
            if !state.layers.markup(kind).is_empty() {
                ops.push(Op::Swap {
                    slot: layer_slot(kind),
                    content: Some(Content::empty()),
                    ms: 0,
                });
            }
        }
        if state.layers.info.is_some() {
            ops.push(Op::Swap {
                slot: Slot::Info,
                content: Some(Content::empty()),
                ms: 0,
            });
        }
        let ms = self.ms;
        self.ms = 0;
        ops.extend(self.web(state.layers.web.as_ref()));
        self.ms = ms;
        if state.last_render.is_some() {
            ops.extend(self.paint(state, true));
            if state.document_mode {
                ops.push(Op::ProgramBackground(Some(OPAQUE_BLACK)));
            }
        }
        ops
    }

    /// `__configure`. A layout that throws abandons the whole call.
    fn configure(&mut self, config: &SurfaceConfig) -> Vec<Op> {
        let raw = match &config.video_layout {
            Some(json) => json.value().ok(),
            None => Some(default_video_settings()),
        };
        let layout: VideoLayout = match video_layout(raw.as_ref()) {
            Ok(layout) => layout,
            Err(_) => {
                tracing::warn!(
                    output = self.output,
                    "video layout threw; configure ignored"
                );
                return vec![];
            }
        };
        self.ms = clamp_transition(config.transition_ms);
        vec![Op::Layout(layout)]
    }

    fn paint(&mut self, state: &SurfaceState, instant: bool) -> Vec<Op> {
        self.set_content(state.last_render.as_ref(), instant)
    }

    /// `__setContent`: background (kept when the same file), theme
    /// elements, then the words. Leaves document mode.
    fn set_content(&mut self, slide: Option<&SlideContent>, instant: bool) -> Vec<Op> {
        let ms = if instant { 0 } else { self.ms };
        let mut ops = vec![Op::ProgramBackground(None)];
        match slide {
            None => ops.push(Op::Keyed {
                slot: Slot::Background,
                content: None,
                ms,
            }),
            Some(slide) => ops.push(self.background(slide, ms)),
        }
        // Theme elements (053) are not built yet, so none are present.
        for slot in [Slot::ElementsBelow, Slot::ElementsAbove] {
            ops.push(Op::Keyed {
                slot,
                content: None,
                ms,
            });
        }
        let words = slide.is_some_and(|s| !s.reference.is_empty() || !s.text.is_empty());
        ops.push(Op::Swap {
            slot: Slot::Text,
            content: words.then(Content::empty),
            ms,
        });
        ops
    }

    /// `backgroundLayerHTML` and `backgroundKey` for the slide's theme.
    fn background(&mut self, slide: &SlideContent, ms: u32) -> Op {
        let theme = slide.theme.value().unwrap_or(Value::Null);
        let background = theme.get("background").cloned().unwrap_or(Value::Null);
        let kind = background.get("type").and_then(Value::as_str).unwrap_or("");
        let media = background
            .get("mediaPath")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty());
        let opacity = background
            .get("opacity")
            .and_then(Value::as_f64)
            .map_or(1.0, |o| o.clamp(0.0, 1.0)) as f32;
        let (content, key) = match (kind, media) {
            ("image" | "video", Some(path)) => {
                self.note(format!(
                    "{kind} backgrounds are drawn from prompts 053 and 055"
                ));
                let tag = if kind == "video" { "VIDEO" } else { "IMG" };
                let cue = background
                    .get("mediaCue")
                    .and_then(Value::as_f64)
                    .filter(|c| c.fract() == 0.0 && c.abs() <= 9_007_199_254_740_991.0)
                    .map(|c| format!("{c}"))
                    .unwrap_or_default();
                (
                    Content {
                        opacity,
                        ..Content::empty()
                    },
                    format!("{tag}:{path}:{cue}"),
                )
            }
            ("color", _) => {
                let color = background
                    .get("color")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let fill = match parse_color(color) {
                    Some(rgba) => Fill::Solid(rgba),
                    None => {
                        self.note(format!(
                            "background colour {color:?} is drawn from prompt 053"
                        ));
                        Fill::Empty
                    }
                };
                (
                    Content {
                        opacity,
                        ..Content::new(fill)
                    },
                    format!("HTML:{background}"),
                )
            }
            ("gradient", _) => {
                self.note("gradient backgrounds are drawn from prompt 053".into());
                (
                    Content {
                        opacity,
                        ..Content::empty()
                    },
                    format!("HTML:{background}"),
                )
            }
            // 'transparent', and image or video with no file picked yet.
            _ => (Content::empty(), "HTML:transparent".into()),
        };
        Op::Keyed {
            slot: Slot::Background,
            content: Some((content, key)),
            ms,
        }
    }

    /// `__setWeb`: the same url and reload keep the page.
    fn web(&mut self, web: Option<&WebLayer>) -> Vec<Op> {
        let key = web.map(|w| format!("{} {}", w.url, kairo_number(w.reload)));
        if key == self.web_shown {
            return vec![];
        }
        self.web_shown = key;
        let mut ops = vec![Op::Swap {
            slot: Slot::WebPointer,
            content: None,
            ms: 0,
        }];
        ops.push(Op::Swap {
            slot: Slot::Web,
            // The page's frame is white until it paints (059 shows the page).
            content: web.map(|_| Content::new(Fill::Solid([1.0; 4]))),
            ms: self.ms,
        });
        ops
    }

    /// `__setWebPointer`: the arrow's tip at the point, inside the frame.
    fn pointer(&self, point: Option<(f64, f64)>) -> Op {
        let content = match (point, &self.web_shown) {
            (Some((x, y)), Some(_)) if x.is_finite() && y.is_finite() => {
                let x = x.clamp(0.0, FRAME_WIDTH);
                let y = y.clamp(0.0, f64::from(self.frame_height));
                Some(Content {
                    rect: Some(Rect::new(
                        (x - 10.0).max(0.0) as f32,
                        (y - 5.0).max(0.0) as f32,
                        56.0,
                        56.0,
                    )),
                    ..Content::empty()
                })
            }
            _ => None,
        };
        Op::Swap {
            slot: Slot::WebPointer,
            content,
            ms: 0,
        }
    }
}

fn background_of(state: &SurfaceState) -> Option<Value> {
    let theme = state.last_render.as_ref()?.theme.value().ok()?;
    theme.get("background").cloned()
}

fn layer_slot(kind: LayerKind) -> Slot {
    match kind {
        LayerKind::Poll => Slot::Poll,
        LayerKind::Question => Slot::Question,
        LayerKind::Message => Slot::Message,
        LayerKind::Props => Slot::Props,
        LayerKind::Timer => Slot::Timer,
        LayerKind::Logo => Slot::Logo,
    }
}

/// `DEFAULT_VIDEO_SETTINGS` in `src/lib/program.ts`.
fn default_video_settings() -> Value {
    json!({
        "layout": "full",
        "box": {"xPct": 68, "yPct": 68, "widthPct": 30, "heightPct": 30},
        "radiusPx": 12,
        "side": "right",
        "splitPct": 50,
    })
}

/// A number as JavaScript string concatenation writes it.
fn kairo_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// The CSS colours this engine draws before prompt 053's full parser:
/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()` with numbers
/// or percentages, and `transparent`, `black`, `white`. Premultiplied.
pub fn parse_color(raw: &str) -> Option<[f32; 4]> {
    let s = raw.trim().to_ascii_lowercase();
    let straight = match s.as_str() {
        "transparent" => [0.0, 0.0, 0.0, 0.0],
        "black" => [0.0, 0.0, 0.0, 1.0],
        "white" => [1.0, 1.0, 1.0, 1.0],
        _ if s.starts_with('#') => hex(&s[1..])?,
        _ => functional(&s)?,
    };
    let a = straight[3];
    Some([straight[0] * a, straight[1] * a, straight[2] * a, a])
}

fn hex(digits: &str) -> Option<[f32; 4]> {
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |i: usize| u8::from_str_radix(&digits[i..=i], 16).ok().map(|v| v * 17);
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    let channels: Vec<u8> = match digits.len() {
        3 | 4 => (0..digits.len()).map(nibble).collect::<Option<_>>()?,
        6 | 8 => (0..digits.len() / 2)
            .map(|i| byte(i * 2))
            .collect::<Option<_>>()?,
        _ => return None,
    };
    let alpha = channels.get(3).copied().unwrap_or(255);
    Some([
        f32::from(channels[0]) / 255.0,
        f32::from(channels[1]) / 255.0,
        f32::from(channels[2]) / 255.0,
        f32::from(alpha) / 255.0,
    ])
}

fn functional(s: &str) -> Option<[f32; 4]> {
    let inner = s
        .strip_prefix("rgba(")
        .or_else(|| s.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split([',', ' ', '/'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if !(parts.len() == 3 || parts.len() == 4) {
        return None;
    }
    let channel = |p: &str| -> Option<f32> {
        let v = match p.strip_suffix('%') {
            Some(pct) => pct.parse::<f32>().ok()? / 100.0 * 255.0,
            None => p.parse::<f32>().ok()?,
        };
        v.is_finite().then(|| v.clamp(0.0, 255.0).round() / 255.0)
    };
    let alpha = match parts.get(3) {
        None => 1.0,
        Some(p) => {
            let v = match p.strip_suffix('%') {
                Some(pct) => pct.parse::<f32>().ok()? / 100.0,
                None => p.parse::<f32>().ok()?,
            };
            if !v.is_finite() {
                return None;
            }
            v.clamp(0.0, 1.0)
        }
    };
    Some([
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        alpha,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_and_premultiply() {
        assert_eq!(parse_color("#FF0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_color(" #0f0 "), Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(
            parse_color("#ff000080").map(|c| (c[0] * 255.0).round()),
            Some(128.0)
        );
        assert_eq!(
            parse_color("rgba(255, 0, 0, 0.5)"),
            Some([0.5, 0.0, 0.0, 0.5])
        );
        assert_eq!(
            parse_color("rgb(0 0 255 / 50%)"),
            Some([0.0, 0.0, 0.5, 0.5])
        );
        assert_eq!(parse_color("rgb(100%, 0%, 0%)"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_color("transparent"), Some([0.0; 4]));
        assert_eq!(parse_color("Black"), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_color("rebeccapurple"), None);
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color("rgb(1,2)"), None);
        assert_eq!(parse_color("url(x)"), None);
    }

    fn state_with(theme: Value) -> SurfaceState {
        let mut state = SurfaceState::new(SurfaceSpec {
            output: "o".into(),
            label: "o".into(),
            sink: SinkSpec::Ndi { name: "n".into() },
        });
        state.last_output_id = Some("o".into());
        state.last_render = Some(SlideContent {
            reference: "Ps 1".into(),
            text: "x".into(),
            theme: Json::from_value(&theme),
            colored_lines: None,
        });
        state
    }

    #[test]
    fn a_slide_swaps_background_elements_and_words_with_the_transition() {
        let mut shell = Shell::new("o", 1080);
        let blank = state_with(json!({}));
        shell.command(
            &Command::Configure {
                output: "o".into(),
                config: SurfaceConfig {
                    transition_ms: 9000.0,
                    ..SurfaceConfig::default()
                },
            },
            &blank,
            &blank,
        );
        let state = state_with(
            json!({"background": {"type": "color", "color": "#FF0000", "opacity": 0.5}}),
        );
        let slide = state.last_render.clone().unwrap();
        let ops = shell.command(
            &Command::ShowSlide {
                output: "o".into(),
                source: "o".into(),
                slide,
                instant: false,
            },
            &blank,
            &state,
        );
        assert!(matches!(ops[0], Op::ProgramBackground(None)));
        match &ops[1] {
            Op::Keyed {
                slot: Slot::Background,
                content: Some((content, key)),
                ms: 5000,
            } => {
                assert!(matches!(content.fill, Fill::Solid([1.0, 0.0, 0.0, 1.0])));
                assert_eq!(content.opacity, 0.5);
                assert!(key.starts_with("HTML:"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            ops.last(),
            Some(Op::Swap {
                slot: Slot::Text,
                content: Some(_),
                ms: 5000
            })
        ));
    }

    #[test]
    fn media_backgrounds_key_on_the_file_and_cue() {
        let mut shell = Shell::new("o", 1080);
        let state = state_with(
            json!({"background": {"type": "video", "mediaPath": "/a.mp4", "mediaCue": 3, "opacity": 1}}),
        );
        match shell.background(state.last_render.as_ref().unwrap(), 0) {
            Op::Keyed {
                content: Some((_, key)),
                ..
            } => assert_eq!(key, "VIDEO:/a.mp4:3"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_same_web_page_is_kept_and_the_pointer_needs_one() {
        let mut shell = Shell::new("o", 1080);
        assert!(matches!(
            shell.pointer(Some((5.0, 5.0))),
            Op::Swap { content: None, .. }
        ));
        let page = WebLayer {
            url: "https://a".into(),
            reload: 0.0,
        };
        assert_eq!(shell.web(Some(&page)).len(), 2);
        assert!(shell.web(Some(&page)).is_empty());
        match shell.pointer(Some((5000.0, 2.0))) {
            Op::Swap {
                content: Some(content),
                ..
            } => {
                assert_eq!(content.rect, Some(Rect::new(1910.0, 0.0, 56.0, 56.0)));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_layout_that_throws_leaves_configure_undone() {
        let mut shell = Shell::new("o", 1080);
        let ops = shell.command(
            &Command::Configure {
                output: "o".into(),
                config: SurfaceConfig {
                    transition_ms: 700.0,
                    video_layout: Some(Json(r#"{"corner":5}"#.into())),
                    ..SurfaceConfig::default()
                },
            },
            &state_with(json!({})),
            &state_with(json!({})),
        );
        assert!(ops.is_empty());
        assert_eq!(shell.ms, 0);
    }
}
