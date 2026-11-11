//! The `overlay.html` shell API on top of the compositor: which layer each
//! command touches, with what transition. Commands arrive here only after
//! the shared surface reducer has applied them.
//!
//! Layer pixels come from the scene builder: slide text (prompt 052) is drawn
//! by `kairo-scene` on a worker pool and filled in when ready (`text`),
//! while backgrounds and elements (053), program layers (054), video (055),
//! camera (057), documents (058) and web (059) follow in later prompts.
//! Until then their nodes exist without pixels, so every swap, retire and
//! crossfade already happens as it will; colour backgrounds draw now.

use crate::text::{TextRequest, slide_is_blank};
use crate::visuals::{self, Job};
use kairo_compositor::thread::Op;
use kairo_compositor::{Content, Fill, Rect, Slot, VideoLayout, clamp_transition, video_layout};
use kairo_engine_proto::*;
use serde_json::{Value, json};
use std::collections::HashSet;

const FRAME_WIDTH: f64 = 1920.0;
const OPAQUE_BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// What a command does: operations for the compositor now, and the slide
/// whose text must be drawn before they are applied.
pub struct Planned {
    pub ops: Vec<Op>,
    pub text: Option<TextRequest>,
    pub visuals: Vec<VisualRequest>,
}

/// A background picture or an elements layer whose pixels a worker makes.
pub struct VisualRequest {
    /// The operation whose content a blocking request fills.
    pub op: usize,
    pub slot: Slot,
    pub key: String,
    pub generation: u64,
    /// Elements layers: the slide waits for the first stage. Pictures: the
    /// result arrives as an update to the node.
    pub blocking: bool,
    pub job: Job,
}

/// One surface's shell settings (`config`, `webShown`).
pub struct Shell {
    frame_height: u32,
    ms: u32,
    web_shown: Option<String>,
    /// Things not drawn yet, logged once per surface.
    noted: HashSet<String>,
    output: String,
    /// The slide whose words the operations being built must carry.
    text: Option<TextRequest>,
    /// Pictures and elements the operations being built need.
    visuals: Vec<VisualRequest>,
    /// The key of the node each elements slot holds (`setElements` keeps a
    /// node whose markup is identical), `[below, above]`.
    element_keys: [Option<String>; 2],
    /// Which request last asked for each slot's pixels; older results are stale.
    generations: std::collections::HashMap<Slot, u64>,
    next_generation: u64,
}

impl Shell {
    pub fn new(output: &str, frame_height: u32) -> Self {
        Self {
            frame_height: kairo_compositor::frame_height(frame_height),
            ms: 0,
            web_shown: None,
            noted: HashSet::new(),
            output: output.to_owned(),
            text: None,
            visuals: Vec::new(),
            element_keys: [None, None],
            generations: std::collections::HashMap::new(),
            next_generation: 0,
        }
    }

    /// Whether a result made for `generation` is still the newest for `slot`.
    pub fn is_current(&self, slot: Slot, generation: u64) -> bool {
        self.generations.get(&slot) == Some(&generation)
    }

    fn generation(&mut self, slot: Slot) -> u64 {
        self.next_generation += 1;
        self.generations.insert(slot, self.next_generation);
        self.next_generation
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
    ) -> Planned {
        self.text = None;
        self.visuals.clear();
        let ops = self.command_ops(command, before, state);
        Planned {
            ops,
            text: self.text.take(),
            visuals: std::mem::take(&mut self.visuals),
        }
    }

    fn command_ops(
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
                Some(slide) => {
                    let theme = slide.theme.value().unwrap_or(Value::Null);
                    let bg = theme.get("background").cloned().unwrap_or(Value::Null);
                    let plan = visuals::background(&bg, (FRAME_WIDTH as u32, self.frame_height));
                    vec![Op::Keyed {
                        slot: Slot::Background,
                        content: plan.content.map(|c| (c, plan.key)),
                        ms: 0,
                    }]
                }
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
    pub fn rebuild(&mut self, state: &SurfaceState) -> Planned {
        self.text = None;
        self.visuals.clear();
        // A new window starts with no nodes.
        self.element_keys = [None, None];
        let ops = self.rebuild_ops(state);
        Planned {
            ops,
            text: self.text.take(),
            visuals: std::mem::take(&mut self.visuals),
        }
    }

    fn rebuild_ops(&mut self, state: &SurfaceState) -> Vec<Op> {
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
            Some(slide) => {
                let op = ops.len();
                let bg = self.background(slide, ms, op);
                ops.push(bg);
            }
        }
        let theme = slide
            .and_then(|slide| slide.theme.value().ok())
            .unwrap_or(Value::Null);
        for above in [false, true] {
            let slot = visuals::element_slot(above);
            let index = usize::from(above);
            let plan = slide.and_then(|_| {
                visuals::elements(&theme, above, (FRAME_WIDTH as u32, self.frame_height))
            });
            match plan {
                None => {
                    self.element_keys[index] = None;
                    ops.push(Op::Keyed {
                        slot,
                        content: None,
                        ms,
                    });
                }
                // Identical markup keeps the node and everything on it.
                Some(plan) if self.element_keys[index].as_deref() == Some(plan.key.as_str()) => {}
                Some(plan) => {
                    self.element_keys[index] = Some(plan.key.clone());
                    let generation = self.generation(slot);
                    self.visuals.push(VisualRequest {
                        op: ops.len(),
                        slot,
                        key: plan.key.clone(),
                        generation,
                        blocking: true,
                        job: Job::Elements(plan.clone_plan()),
                    });
                    ops.push(Op::Keyed {
                        slot,
                        content: Some((Content::new(Fill::Pending), plan.key)),
                        ms,
                    });
                }
            }
        }
        // `renderOverlayHTML` always yields a `.pa-layer`, so a slide always
        // swaps in a text node, empty when there is nothing to say. Its
        // pixels are drawn off-thread and replace the placeholder.
        ops.push(Op::Swap {
            slot: Slot::Text,
            content: slide.map(|_| Content::empty()),
            ms,
        });
        if let Some(slide) = slide
            && !slide_is_blank(slide)
        {
            self.text = Some(TextRequest {
                slide: slide.clone(),
                frame: (FRAME_WIDTH as u32, self.frame_height),
            });
        }
        ops
    }

    /// `backgroundLayerHTML` and `backgroundKey` for the slide's theme. A
    /// picture is decoded off-thread and arrives as an update to the node.
    fn background(&mut self, slide: &SlideContent, ms: u32, op: usize) -> Op {
        let theme = slide.theme.value().unwrap_or(Value::Null);
        let background = theme.get("background").cloned().unwrap_or(Value::Null);
        let plan = visuals::background(&background, (FRAME_WIDTH as u32, self.frame_height));
        if let Some(image) = plan.image {
            let generation = self.generation(Slot::Background);
            self.visuals.push(VisualRequest {
                op,
                slot: Slot::Background,
                key: plan.key.clone(),
                generation,
                blocking: false,
                job: Job::Image(image),
            });
        } else {
            self.generation(Slot::Background);
        }
        Op::Keyed {
            slot: Slot::Background,
            content: plan.content.map(|c| (c, plan.key)),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let _ = shell.command(
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
        let planned = shell.command(
            &Command::ShowSlide {
                output: "o".into(),
                source: "o".into(),
                slide,
                instant: false,
            },
            &blank,
            &state,
        );
        let ops = planned.ops;
        // The words are drawn off-thread: the swap holds a placeholder and
        // the request says what to draw.
        let request = planned.text.expect("a slide with words asks for text");
        assert_eq!(request.frame, (1920, 1080));
        assert_eq!(request.slide.text, "x");
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
        match shell.background(state.last_render.as_ref().unwrap(), 0, 0) {
            Op::Keyed {
                content: Some((_, key)),
                ..
            } => assert_eq!(key, "VIDEO:/a.mp4:3"),
            other => panic!("{other:?}"),
        }
    }

    fn slide_command(theme: Value) -> Command {
        Command::ShowSlide {
            output: "o".into(),
            source: "o".into(),
            slide: SlideContent {
                reference: String::new(),
                text: "words".into(),
                theme: Json::from_value(&theme),
                colored_lines: None,
            },
            instant: false,
        }
    }

    fn element(id: &str, above: bool, fill: &str) -> Value {
        let mut el = serde_json::to_value(kairo_domain::overlay_elements::create_overlay_element(
            kairo_domain::overlay::OverlayElementKind::Rectangle,
            None,
            id,
            None,
        ))
        .unwrap();
        el["aboveText"] = json!(above);
        el["fill"] = json!(fill);
        el
    }

    fn element_ops(planned: &Planned) -> usize {
        planned
            .ops
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Op::Keyed {
                        slot: Slot::ElementsBelow | Slot::ElementsAbove,
                        ..
                    }
                )
            })
            .count()
    }

    #[test]
    fn identical_element_sets_keep_their_nodes_across_slides() {
        let mut shell = Shell::new("o", 1080);
        let theme = json!({"elements": [element("a", false, "#ff0000")]});
        let first = state_with(theme.clone());
        let planned = shell.command(
            &slide_command(theme.clone()),
            &state_with(json!({})),
            &first,
        );
        // Below has a node to make; above has none to retire yet but says so.
        assert_eq!(element_ops(&planned), 2);
        assert_eq!(planned.visuals.len(), 1);
        assert!(planned.visuals[0].blocking && planned.visuals[0].slot == Slot::ElementsBelow);
        // The next verse, same theme: nothing to do for the layers at all.
        let again = shell.command(&slide_command(theme.clone()), &first, &first);
        assert_eq!(
            element_ops(&again),
            1,
            "only the empty above layer is repeated"
        );
        assert!(again.visuals.is_empty(), "no work for an identical set");
        // A changed set crossfades in a new node.
        let changed = json!({"elements": [element("a", false, "#00ff00")]});
        let planned = shell.command(
            &slide_command(changed.clone()),
            &first,
            &state_with(changed),
        );
        assert_eq!(planned.visuals.len(), 1);
        // Clearing retires them, and the same set is then new again.
        let cleared = shell.command(
            &Command::Clear { output: "o".into() },
            &first,
            &state_with(json!({})),
        );
        assert_eq!(element_ops(&cleared), 2);
        let back = shell.command(&slide_command(theme.clone()), &first, &first);
        assert_eq!(back.visuals.len(), 1);
        // A fresh window starts with no nodes, so a rebuild makes them.
        shell.command(&slide_command(theme), &first, &first);
        let rebuilt = shell.rebuild(&first);
        assert_eq!(rebuilt.visuals.len(), 1);
    }

    #[test]
    fn a_result_is_stale_once_a_newer_request_replaced_it() {
        let mut shell = Shell::new("o", 1080);
        let theme = json!({"background": {"type": "image", "mediaPath": "/a.png", "opacity": 1}});
        let a = state_with(theme.clone());
        let first = shell.command(&slide_command(theme), &state_with(json!({})), &a);
        let old = first.visuals[0].generation;
        assert!(shell.is_current(Slot::Background, old));
        let theme2 = json!({"background": {"type": "image", "mediaPath": "/b.png", "opacity": 1}});
        let second = shell.command(&slide_command(theme2.clone()), &a, &state_with(theme2));
        assert!(!shell.is_current(Slot::Background, old));
        assert!(shell.is_current(Slot::Background, second.visuals[0].generation));
    }

    #[test]
    fn a_patch_keeps_the_picture_and_asks_for_no_new_work() {
        let mut shell = Shell::new("o", 1080);
        let theme = json!({"background": {"type": "image", "mediaPath": "/a.png", "opacity": 1, "hue": 40}});
        let state = state_with(theme.clone());
        shell.command(&slide_command(theme), &state_with(json!({})), &state);
        let planned = shell.command(
            &Command::PatchBackground {
                output: "o".into(),
                playback: Json::from_value(&json!({"hue": 90})),
                filter: "hue-rotate(90deg)".into(),
            },
            &state,
            &state,
        );
        assert!(planned.visuals.is_empty());
        let [
            Op::Keyed {
                content: Some((c, key)),
                ms: 0,
                ..
            },
        ] = planned.ops.as_slice()
        else {
            panic!("{:?}", planned.ops)
        };
        assert_eq!(key, "IMG:/a.png:");
        assert!(
            matches!(c.fill, Fill::Pending),
            "kept node keeps its pixels"
        );
    }

    #[test]
    fn transport_never_addresses_element_videos() {
        let mut shell = Shell::new("o", 1080);
        let mut video =
            serde_json::to_value(kairo_domain::overlay_elements::create_overlay_element(
                kairo_domain::overlay::OverlayElementKind::Video,
                Some("/v.mp4"),
                "v",
                None,
            ))
            .unwrap();
        video["aboveText"] = json!(false);
        let theme = json!({"elements": [video]});
        let state = state_with(theme.clone());
        shell.command(&slide_command(theme), &state_with(json!({})), &state);
        let planned = shell.command(
            &Command::VideoTransport {
                output: "o".into(),
                transport: kairo_engine_proto::VideoTransport::Pause(true),
            },
            &state,
            &state,
        );
        assert!(planned.ops.is_empty() && planned.visuals.is_empty());
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
        let ops = shell
            .command(
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
            )
            .ops;
        assert!(ops.is_empty());
        assert_eq!(shell.ms, 0);
    }
}
