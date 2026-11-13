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

use crate::layers::{LayerJob, LayerSource, LiveLayer};
use crate::text::{TextRequest, slide_is_blank};
use crate::visuals::{self, Job};
use kairo_compositor::thread::{MediaControl, Op};
use kairo_compositor::{Content, Fill, Rect, Slot, VideoLayout, clamp_transition, video_layout};
use kairo_domain::program::{ConfidenceInfo, ProgramLayer};
use kairo_engine_proto::*;
use kairo_scene::program::Pattern;
use serde_json::{Value, json};

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
    /// Program layers whose countdowns or clocks tick, by slot.
    pub live: std::collections::HashMap<Slot, LiveLayer>,
    /// Source ids of the cameras that are live, by label.
    cameras: std::collections::HashMap<String, u64>,
    /// The camera source the camera slot shows.
    camera_shown: Option<u64>,
    /// Tests draw at a fixed moment instead of the wall clock.
    #[cfg(test)]
    pub pinned: Option<kairo_scene::program::Now>,
}

impl Shell {
    pub fn new(output: &str, frame_height: u32) -> Self {
        Self {
            frame_height: kairo_compositor::frame_height(frame_height),
            ms: 0,
            web_shown: None,
            output: output.to_owned(),
            text: None,
            visuals: Vec::new(),
            element_keys: [None, None],
            generations: std::collections::HashMap::new(),
            next_generation: 0,
            live: std::collections::HashMap::new(),
            cameras: std::collections::HashMap::new(),
            camera_shown: None,
            #[cfg(test)]
            pinned: None,
        }
    }

    /// Camera `label` is live as source `id`: the camera node, if this
    /// surface wants that camera and does not show it yet.
    pub fn camera_live(&mut self, label: &str, id: u64, state: &SurfaceState) -> Option<Op> {
        self.cameras.insert(label.to_owned(), id);
        let wanted = state.layers.camera.as_ref()?;
        if wanted.label != label {
            return None;
        }
        self.camera_op(label, self.ms)
    }

    /// Camera `label` is live as source `id`, known before a command or
    /// a rebuild asks for it.
    pub fn know_camera(&mut self, label: &str, id: u64) {
        self.cameras.insert(label.to_owned(), id);
    }

    /// `__setCamera`'s node: the stream, cover-fitted on black.
    fn camera_op(&mut self, label: &str, ms: u32) -> Option<Op> {
        let id = *self.cameras.get(label)?;
        if self.camera_shown == Some(id) {
            return None;
        }
        self.camera_shown = Some(id);
        let mut content = Content::new(Fill::Source {
            id,
            backdrop: OPAQUE_BLACK,
        });
        content.look.fit = kairo_compositor::Fit::Cover;
        Some(Op::Swap {
            slot: Slot::Camera,
            content: Some(content),
            ms,
        })
    }

    /// Whether a result made for `generation` is still the newest for `slot`.
    pub fn is_current(&self, slot: Slot, generation: u64) -> bool {
        self.generations.get(&slot) == Some(&generation)
    }

    /// A drawn layer arrived: remember its readouts while it is the slot's
    /// newest, and give back what to show. The caller completes the
    /// command waiting for it, or else sends it as an update.
    pub fn layer_done(
        &mut self,
        slot: Slot,
        key: &str,
        generation: u64,
        out: crate::layers::LayerOut,
    ) -> Content {
        let content = out.content();
        if self.is_current(slot, generation) {
            if out.live.is_empty() {
                self.live.remove(&slot);
            } else {
                self.live.insert(
                    slot,
                    LiveLayer {
                        key: key.to_owned(),
                        generation,
                        children: out.children,
                        parts: out.live,
                    },
                );
            }
        }
        content
    }

    /// Readouts whose text, colour or dimming changed at `now`.
    pub fn tick(
        &mut self,
        now: &kairo_scene::program::Now,
    ) -> Vec<(Slot, String, u64, crate::layers::LiveJob)> {
        let mut jobs = Vec::new();
        for (slot, layer) in &mut self.live {
            for job in layer.due(now) {
                jobs.push((*slot, layer.key.clone(), layer.generation, job));
            }
        }
        jobs
    }

    /// A redrawn readout: the update for its node, unless the layer has
    /// been replaced meanwhile.
    pub fn live_done(
        &mut self,
        slot: Slot,
        generation: u64,
        out: crate::layers::LiveOut,
    ) -> Option<Op> {
        let layer = self
            .live
            .get_mut(&slot)
            .filter(|l| l.generation == generation)?;
        let content = layer.replace(out.index, out.content)?;
        Some(Op::Update {
            slot,
            key: layer.key.clone(),
            content,
        })
    }

    fn generation(&mut self, slot: Slot) -> u64 {
        self.next_generation += 1;
        self.generations.insert(slot, self.next_generation);
        self.next_generation
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
        if state.stage.is_some() {
            return if matches!(command, Command::SetStage { .. }) && before.stage != state.stage {
                self.stage(state, 0)
            } else {
                vec![]
            };
        }
        match command {
            Command::SetStage { .. } => self.stage(state, 0),
            Command::Configure { config, .. } => {
                let mut ops = self.configure(config);
                ops.extend(self.web(state.layers.web.as_ref().filter(|_| config.web_enabled)));
                ops
            }
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
            Command::SetLayer { kind, layer, .. } => {
                let source = layer.as_ref().and_then(|l| self.program_layer(l));
                self.layer(layer_slot(*kind), source, ms, 0)
            }
            // The info layer is shown and hidden, never faded.
            Command::SetInfo { info, .. } => {
                let source = info.as_ref().and_then(|i| self.info_layer(i));
                self.layer(Slot::Info, source, 0, 0)
            }
            Command::SetWeb { web, .. } => {
                self.web(web.as_ref().filter(|_| state.config.web_enabled))
            }
            Command::WebPointer { point, .. } => vec![self.pointer(*point)],
            Command::SetCamera { camera, .. } => match camera {
                None => {
                    self.camera_shown = None;
                    vec![Op::Swap {
                        slot: Slot::Camera,
                        content: None,
                        ms,
                    }]
                }
                // Until the camera is live the old node stays, as
                // `__setCamera` kept it until the new stream played.
                Some(camera) => self.camera_op(&camera.label, ms).into_iter().collect(),
            },
            Command::SetTestPattern { .. } => self.pattern(state, 0),
            // In order with the slides around it, to the background video
            // current when it is applied.
            Command::VideoTransport { transport, .. } => vec![Op::Media(match transport {
                VideoTransport::Pause(p) => MediaControl::Pause(*p),
                VideoTransport::Seek(s) => MediaControl::Seek(*s),
            })],
            // No pixels change in this engine yet: document pages and their
            // videos (058), web scrolling (059).
            Command::SwapPage { .. }
            | Command::PreloadPage { .. }
            | Command::DocumentVideos { .. }
            | Command::ControlDocumentVideo { .. }
            | Command::WebScroll { .. }
            | Command::WebInput { .. } => vec![],
            Command::OpenSurface(_)
            | Command::RetainSurfaces { .. }
            | Command::CloseSurface { .. }
            | Command::RestoreSurface { .. }
            | Command::Shutdown
            | Command::ControlDisplay { .. }
            | Command::IdentifyDisplays
            | Command::DisplayTestPattern { .. } => vec![],
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
        if state.stage.is_some() {
            self.live.clear();
            return self.stage(state, 0);
        }
        self.web_shown = None;
        self.camera_shown = None;
        let mut ops = self.configure(&state.config);
        if let Some(camera) = &state.layers.camera {
            ops.extend(self.camera_op(&camera.label, 0));
        }
        self.live.clear();
        for kind in LayerKind::ALL {
            if let Some(layer) = state.layers.layer(kind) {
                let source = self.program_layer(layer);
                let at = ops.len();
                ops.extend(self.layer(layer_slot(kind), source, 0, at));
            }
        }
        if let Some(info) = &state.layers.info {
            let source = self.info_layer(info);
            let at = ops.len();
            ops.extend(self.layer(Slot::Info, source, 0, at));
        }
        if state.test_pattern.is_some() {
            let at = ops.len();
            ops.extend(self.pattern(state, at));
        }
        let ms = self.ms;
        self.ms = 0;
        ops.extend(
            self.web(
                state
                    .layers
                    .web
                    .as_ref()
                    .filter(|_| state.config.web_enabled),
            ),
        );
        self.ms = ms;
        if state.last_render.is_some() {
            ops.extend(self.paint(state, true));
            if state.document_mode {
                ops.push(Op::ProgramBackground(Some(OPAQUE_BLACK)));
            }
        }
        ops
    }

    /// The moment a layer is drawn at.
    fn now(&self) -> kairo_scene::program::Now {
        #[cfg(test)]
        if let Some(now) = &self.pinned {
            return now.clone();
        }
        crate::layers::now()
    }

    /// Whether moving parts are held at their start (tests only).
    fn still(&self) -> bool {
        #[cfg(test)]
        if self.pinned.is_some() {
            return true;
        }
        false
    }

    /// A program layer's payload; one that cannot be read clears the layer.
    fn program_layer(&mut self, layer: &Json) -> Option<LayerSource> {
        match layer.decode::<ProgramLayer>() {
            Ok(layer) => Some(LayerSource::Program(layer)),
            Err(error) => {
                tracing::error!(output = self.output, %error, "a program layer could not be read; clearing it");
                None
            }
        }
    }

    /// `__setInfo`'s `ConfidenceInfo`; unreadable is the same as none.
    fn info_layer(&mut self, info: &Json) -> Option<LayerSource> {
        match info.decode::<ConfidenceInfo>() {
            Ok(info) => Some(LayerSource::Info(info)),
            Err(error) => {
                tracing::error!(output = self.output, %error, "the info layer could not be read; hiding it");
                None
            }
        }
    }

    /// The diagnostic pattern over everything, named for the output.
    fn stage(&mut self, state: &SurfaceState, at: usize) -> Vec<Op> {
        let source = state.stage.as_ref().and_then(|view| match view.decode() {
            Ok(view) => Some(LayerSource::Stage(view)),
            Err(error) => {
                tracing::warn!(%error, "invalid stage view");
                None
            }
        });
        self.layer(Slot::Background, source, 0, at)
    }

    fn pattern(&mut self, state: &SurfaceState, at: usize) -> Vec<Op> {
        let source = state.test_pattern.map(|pattern| LayerSource::Pattern {
            pattern: match pattern {
                TestPattern::Bars => Pattern::Bars,
                TestPattern::Text => Pattern::Text,
                TestPattern::Motion => Pattern::Motion,
            },
            name: state.spec.label.clone(),
        });
        self.layer(Slot::Pattern, source, 0, at)
    }

    /// `swapLayer` for a drawn layer: a new node every time (the shell
    /// never keeps one), filled when its first stage is drawn. `at` is
    /// where the returned operation lands in the command's list.
    fn layer(&mut self, slot: Slot, source: Option<LayerSource>, ms: u32, at: usize) -> Vec<Op> {
        let generation = self.generation(slot);
        self.live.remove(&slot);
        let Some(source) = source else {
            return vec![Op::Keyed {
                slot,
                content: None,
                ms,
            }];
        };
        let key = format!("{slot:?}#{generation}");
        self.visuals.push(VisualRequest {
            op: at,
            slot,
            key: key.clone(),
            generation,
            blocking: true,
            job: Job::Layer(LayerJob {
                source,
                frame: (FRAME_WIDTH as u32, self.frame_height),
                now: self.now(),
                still: self.still(),
            }),
        });
        vec![Op::Keyed {
            slot,
            content: Some((Content::empty(), key)),
            ms,
        }]
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
        let web = web.filter(|w| crate::web::address(&w.url).is_some() && w.reload.is_finite());
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
            content: web.map(|w| {
                let mut content = Content::new(Fill::Source {
                    id: crate::web::source(&w.url),
                    backdrop: [1.0; 4],
                });
                content.look.fit = kairo_compositor::Fit::Contain;
                Content::new(Fill::Group(std::sync::Arc::new(vec![
                    Content::new(Fill::Solid([1.0; 4])),
                    content,
                ])))
            }),
            ms: self.ms,
        });
        ops
    }

    /// `__setWebPointer`: the arrow's tip at the point, inside the frame.
    fn pointer(&self, point: Option<(f64, f64)>) -> Op {
        let content = match (point, &self.web_shown) {
            (Some((x, y)), Some(_)) if x.is_finite() && y.is_finite() => {
                let x = x.clamp(0.0, FRAME_WIDTH);
                let y = y.clamp(0.0, 1080.0);
                let page = kairo_compositor::letterbox((1920, self.frame_height), (1920, 1080));
                let scale = page.width / 1920.0;
                Some(Content {
                    rect: Some(Rect::new(
                        page.x + (x - 10.0).max(0.0) as f32 * scale,
                        page.y + (y - 5.0).max(0.0) as f32 * scale,
                        56.0 * scale,
                        56.0 * scale,
                    )),
                    ..Content::new(Fill::Image(crate::web::pointer()))
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
        // Only the transport operation itself: no node is touched, and the
        // compositor addresses it to the current background video alone,
        // never to an element video (`media_uses` marks elements
        // `transport: false`; tested in kairo-compositor `tests/video.rs`).
        assert!(planned.visuals.is_empty());
        assert!(matches!(
            planned.ops.as_slice(),
            [Op::Media(MediaControl::Pause(true))]
        ));
    }

    #[test]
    fn web_pointer_uses_the_same_canonical_fit_as_the_page() {
        let mut shell = Shell::new("o", 1440);
        shell.web(Some(&WebLayer {
            url: "https://example.com/".into(),
            reload: 0.,
        }));
        let Op::Swap {
            content: Some(c), ..
        } = shell.pointer(Some((960., 540.)))
        else {
            panic!("pointer");
        };
        assert_eq!(c.rect, Some(Rect::new(950., 715., 56., 56.)));
        assert!(matches!(
            shell.pointer(Some((f64::NAN, 2.))),
            Op::Swap { content: None, .. }
        ));
    }

    #[test]
    fn web_eligibility_clears_page_and_pointer_and_restores_the_source() {
        let mut shell = Shell::new("o", 1080);
        let mut state = state_with(json!({}));
        let set = Command::SetWeb {
            output: "o".into(),
            web: Some(WebLayer {
                url: "https://example.com/".into(),
                reload: 0.,
            }),
        };
        let before = state.clone();
        state.apply(&set, true);
        assert!(
            shell
                .command(&set, &before, &state)
                .ops
                .iter()
                .any(|op| matches!(
                    op,
                    Op::Swap {
                        slot: Slot::Web,
                        content: Some(Content {
                            fill: Fill::Group(_),
                            ..
                        }),
                        ..
                    }
                ))
        );
        let mut config = state.config.clone();
        config.web_enabled = false;
        let disable = Command::Configure {
            output: "o".into(),
            config,
        };
        let before = state.clone();
        state.apply(&disable, true);
        let ops = shell.command(&disable, &before, &state).ops;
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::Swap {
                slot: Slot::Web,
                content: None,
                ..
            }
        )));
        assert!(matches!(
            shell.pointer(Some((10., 20.))),
            Op::Swap { content: None, .. }
        ));
        assert!(
            state.layers.web.is_some(),
            "eligibility does not erase intended state"
        );
        let mut config = state.config.clone();
        config.web_enabled = true;
        let enable = Command::Configure {
            output: "o".into(),
            config,
        };
        let before = state.clone();
        state.apply(&enable, true);
        assert!(
            shell
                .command(&enable, &before, &state)
                .ops
                .iter()
                .any(|op| matches!(
                    op,
                    Op::Swap {
                        slot: Slot::Web,
                        content: Some(Content {
                            fill: Fill::Group(_),
                            ..
                        }),
                        ..
                    }
                ))
        );
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
                assert!(
                    matches!(content.fill, Fill::Image(_)),
                    "pointer needs pixels"
                );
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

    fn message_layer() -> Json {
        Json::from_value(&json!({"kind": "message", "text": "Hi", "style": {
            "position": "bottom", "fontSizePx": 44, "color": "#fff",
            "background": "#000", "scroll": false}}))
    }

    #[test]
    fn program_layers_fade_with_the_transition_and_info_cuts() {
        let mut shell = Shell::new("o", 1080);
        let state = state_with(json!({}));
        let _ = shell.command(
            &Command::Configure {
                output: "o".into(),
                config: SurfaceConfig {
                    transition_ms: 400.0,
                    ..SurfaceConfig::default()
                },
            },
            &state,
            &state,
        );
        let planned = shell.command(
            &Command::SetLayer {
                output: "o".into(),
                kind: LayerKind::Message,
                layer: Some(message_layer()),
            },
            &state,
            &state,
        );
        assert!(matches!(
            planned.ops.as_slice(),
            [Op::Keyed {
                slot: Slot::Message,
                content: Some(_),
                ms: 400
            }]
        ));
        // The node waits for its pixels: one blocking request for it.
        assert_eq!(planned.visuals.len(), 1);
        assert!(planned.visuals[0].blocking && planned.visuals[0].op == 0);
        let info = Json::from_value(&json!({"clock": true, "stageMessage": null,
            "layout": {"position": "top", "size": "small"}, "sizes": {"barPx": 48, "messagePx": 40},
            "style": {"rollover": true, "color": "#fff", "overrunColor": "#f00", "backdrop": false}}));
        let planned = shell.command(
            &Command::SetInfo {
                output: "o".into(),
                info: Some(info),
            },
            &state,
            &state,
        );
        assert!(matches!(
            planned.ops.as_slice(),
            [Op::Keyed {
                slot: Slot::Info,
                content: Some(_),
                ms: 0
            }]
        ));
        // Each set is a new node, even with the same content (`swapLayer`).
        let a = shell.command(
            &Command::SetLayer {
                output: "o".into(),
                kind: LayerKind::Logo,
                layer: Some(message_layer()),
            },
            &state,
            &state,
        );
        let b = shell.command(
            &Command::SetLayer {
                output: "o".into(),
                kind: LayerKind::Logo,
                layer: Some(message_layer()),
            },
            &state,
            &state,
        );
        let key = |p: &Planned| match &p.ops[0] {
            Op::Keyed {
                content: Some((_, key)),
                ..
            } => key.clone(),
            _ => panic!(),
        };
        assert_ne!(key(&a), key(&b));
        assert!(!shell.is_current(Slot::Logo, a.visuals[0].generation));
    }

    #[test]
    fn an_unreadable_layer_clears_its_slot() {
        let mut shell = Shell::new("o", 1080);
        let state = state_with(json!({}));
        let planned = shell.command(
            &Command::SetLayer {
                output: "o".into(),
                kind: LayerKind::Poll,
                layer: Some(Json("{\"kind\":\"nope\"}".into())),
            },
            &state,
            &state,
        );
        assert!(matches!(
            planned.ops.as_slice(),
            [Op::Keyed {
                slot: Slot::Poll,
                content: None,
                ..
            }]
        ));
        assert!(planned.visuals.is_empty());
        let planned = shell.command(
            &Command::SetInfo {
                output: "o".into(),
                info: Some(Json("[]".into())),
            },
            &state,
            &state,
        );
        assert!(matches!(
            planned.ops.as_slice(),
            [Op::Keyed {
                slot: Slot::Info,
                content: None,
                ..
            }]
        ));
    }

    #[test]
    fn a_rebuilt_window_draws_every_layer_and_the_pattern_again() {
        let mut shell = Shell::new("o", 1080);
        let mut state = state_with(json!({}));
        state.last_render = None;
        state.apply(
            &Command::SetLayer {
                output: "o".into(),
                kind: LayerKind::Message,
                layer: Some(message_layer()),
            },
            true,
        );
        state.apply(
            &Command::SetTestPattern {
                output: "o".into(),
                pattern: Some(TestPattern::Bars),
            },
            true,
        );
        let planned = shell.rebuild(&state);
        let slots: Vec<Slot> = planned
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Keyed {
                    slot,
                    content: Some(_),
                    ..
                } => Some(*slot),
                _ => None,
            })
            .collect();
        assert_eq!(slots, [Slot::Message, Slot::Pattern]);
        // Each request fills its own operation.
        for request in &planned.visuals {
            assert!(
                matches!(&planned.ops[request.op], Op::Keyed { slot, .. } if *slot == request.slot)
            );
        }
    }

    fn camera_command(label: Option<&str>) -> Command {
        Command::SetCamera {
            output: "o".into(),
            camera: label.map(|l| CameraLayer {
                label: l.into(),
                audio_label: None,
            }),
        }
    }

    #[test]
    fn the_camera_node_waits_for_its_stream_then_cover_fits_on_black() {
        let mut shell = Shell::new("o", 1080);
        let before = state_with(json!({}));
        let mut after = before.clone();
        let command = camera_command(Some("Logitech BRIO"));
        after.apply(&command, true);
        // Not live yet: the old node stays.
        assert!(shell.command(&command, &before, &after).ops.is_empty());
        let op = shell
            .camera_live("Logitech BRIO", 77, &after)
            .expect("swap");
        match op {
            Op::Swap {
                slot: Slot::Camera,
                content: Some(content),
                ..
            } => {
                assert!(
                    matches!(content.fill, Fill::Source { id: 77, backdrop } if backdrop == OPAQUE_BLACK)
                );
                assert_eq!(content.look.fit, kairo_compositor::Fit::Cover);
            }
            other => panic!("{other:?}"),
        }
        // Shown once; another camera going live is not this surface's.
        assert!(shell.camera_live("Logitech BRIO", 77, &after).is_none());
        assert!(
            shell
                .camera_live("FaceTime HD Camera", 78, &after)
                .is_none()
        );
        // A rebuilt window shows it at once.
        let rebuilt = shell.rebuild(&after);
        assert!(rebuilt.ops.iter().any(|op| matches!(
            op,
            Op::Swap {
                slot: Slot::Camera,
                content: Some(_),
                ms: 0
            }
        )));
        // Switching to a camera already live swaps at once; off clears.
        let mut third = after.clone();
        let switch = camera_command(Some("FaceTime HD Camera"));
        third.apply(&switch, true);
        let ops = shell.command(&switch, &after, &third).ops;
        assert!(
            matches!(&ops[..], [Op::Swap { slot: Slot::Camera, content: Some(c), .. }] if matches!(c.fill, Fill::Source { id: 78, .. }))
        );
        let mut off = third.clone();
        let none = camera_command(None);
        off.apply(&none, true);
        let ops = shell.command(&none, &third, &off).ops;
        assert!(matches!(
            &ops[..],
            [Op::Swap {
                slot: Slot::Camera,
                content: None,
                ..
            }]
        ));
        // Live again later: no camera wanted, nothing shown.
        assert!(shell.camera_live("FaceTime HD Camera", 78, &off).is_none());
    }
}

#[cfg(test)]
mod stage_sink_tests {
    use super::*;
    fn state() -> SurfaceState {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../tests/golden/054-programs/cases.json"
        ))
        .unwrap();
        let view = cases
            .as_array()
            .unwrap()
            .iter()
            .find_map(|case| case.get("stage"))
            .unwrap();
        let mut state = SurfaceState::new(SurfaceSpec {
            output: "stage".into(),
            label: "Stage".into(),
            sink: SinkSpec::Display(DisplaySink {
                binding: DisplayBinding {
                    display_id: Some(1),
                    label: "A".into(),
                    size: Some((1920, 1080)),
                },
                rehearsal: true,
                frame_height: 1080,
            }),
        });
        state.stage = Some(Json::from_value(view));
        state
    }
    #[test]
    fn stage_rebuild_uses_existing_stage_renderer_and_identical_updates_do_not_rasterize() {
        let state = state();
        let mut shell = Shell::new("stage", 1080);
        let rebuild = shell.rebuild(&state);
        assert!(rebuild.visuals.iter().any(|r| matches!(
            &r.job,
            Job::Layer(LayerJob {
                source: LayerSource::Stage(_),
                ..
            })
        )));
        let unchanged = shell.command(
            &Command::SetStage {
                output: "stage".into(),
                view: state.stage.clone().unwrap(),
            },
            &state,
            &state,
        );
        assert!(unchanged.ops.is_empty());
        assert!(unchanged.visuals.is_empty());
    }
}
