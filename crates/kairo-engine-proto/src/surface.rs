//! What one surface shows, and how each command changes it.
//!
//! A port of `ProgramSurface`'s bookkeeping (`program-surface.ts`). The
//! client keeps one of these per surface as the intended state, answers the
//! orchestrator's booleans from it, and replays it into a relaunched engine.
//! The engine keeps its own copy and draws from it. Both apply commands with
//! the same function, so they cannot disagree about what a command did.

use crate::messages::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The program layers above the slide (`SurfaceLayers`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Layers {
    /// `ProgramLayer` each; `None` is the empty markup that clears it.
    pub poll: Option<Json>,
    pub question: Option<Json>,
    pub message: Option<Json>,
    pub props: Option<Json>,
    pub timer: Option<Json>,
    pub logo: Option<Json>,
    pub camera: Option<CameraLayer>,
    pub web: Option<WebLayer>,
    pub info: Option<Json>,
}

impl Layers {
    pub fn layer(&self, kind: LayerKind) -> Option<&Json> {
        match kind {
            LayerKind::Poll => self.poll.as_ref(),
            LayerKind::Question => self.question.as_ref(),
            LayerKind::Message => self.message.as_ref(),
            LayerKind::Props => self.props.as_ref(),
            LayerKind::Timer => self.timer.as_ref(),
            LayerKind::Logo => self.logo.as_ref(),
        }
    }

    fn layer_mut(&mut self, kind: LayerKind) -> &mut Option<Json> {
        match kind {
            LayerKind::Poll => &mut self.poll,
            LayerKind::Question => &mut self.question,
            LayerKind::Message => &mut self.message,
            LayerKind::Props => &mut self.props,
            LayerKind::Timer => &mut self.timer,
            LayerKind::Logo => &mut self.logo,
        }
    }

    /// `hasAnyLayer`.
    pub fn any(&self) -> bool {
        LayerKind::ALL.iter().any(|k| self.layer(*k).is_some())
            || self.camera.is_some()
            || self.web.is_some()
            || self.info.is_some()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SurfaceState {
    pub spec: SurfaceSpec,
    /// Closed by the client; nothing reopens it until `RestoreSurface`.
    pub parked: bool,
    pub config: SurfaceConfig,
    pub layers: Layers,
    pub test_pattern: Option<TestPattern>,
    /// Which output last rendered here; a page swap only continues its deck.
    pub last_output_id: Option<String>,
    /// What is painted, kept so a background can be swapped underneath and
    /// a rebuilt surface comes back showing the same thing.
    pub last_render: Option<SlideContent>,
    pub document_mode: bool,
    /// `DocumentSlide` of the page on screen, with its videos.
    pub document_slide: Option<Json>,
}

/// Whether a surface takes pushes (`ProgramSurface.available`), as the side
/// applying the command sees it. The engine always applies what it is sent.
pub type Availability = bool;

impl SurfaceState {
    pub fn new(spec: SurfaceSpec) -> Self {
        Self {
            spec,
            parked: false,
            config: SurfaceConfig::default(),
            layers: Layers::default(),
            test_pattern: None,
            last_output_id: None,
            last_render: None,
            document_mode: false,
            document_slide: None,
        }
    }

    /// `parksOnClose`: a display or card surface stays parked; NDI and
    /// Syphon reopen on their next paint.
    pub fn parks_on_close(&self) -> bool {
        !matches!(
            self.spec.sink,
            SinkSpec::Ndi { .. } | SinkSpec::Syphon { .. }
        )
    }

    /// Applies `command` and returns what `ProgramSurface` would have
    /// returned (or `true` for its `void` methods). `available` is false
    /// while the sink cannot take pushes; parking is checked here.
    pub fn apply(&mut self, command: &Command, available: Availability) -> bool {
        let available = available && !self.parked;
        match command {
            Command::OpenSurface(spec) => {
                // `retargetDevice` reopens a card surface parked by a lost acquisition.
                if matches!(spec.sink, SinkSpec::Device { .. }) {
                    self.parked = false;
                }
                self.spec = spec.clone();
                true
            }
            Command::CloseSurface { .. } => {
                if self.parks_on_close() {
                    self.parked = true;
                }
                true
            }
            Command::RestoreSurface { .. } => {
                self.parked = false;
                true
            }
            Command::Configure { config, .. } => {
                self.config = config.clone();
                true
            }
            Command::ShowSlide { source, slide, .. } => self.show_slide(source, slide, available),
            Command::SetBackground { background, .. } => {
                if !available {
                    return false;
                }
                let Some(render) = self.last_render.as_mut() else {
                    return false;
                };
                render.theme = with_background(&render.theme, |b| {
                    *b = background.value().ok().and_then(object).unwrap_or_default();
                });
                self.painted();
                true
            }
            Command::PatchBackground { playback, .. } => {
                let Some(render) = self.last_render.as_mut() else {
                    return false;
                };
                let playback = playback.value().ok().and_then(object).unwrap_or_default();
                render.theme = with_background(&render.theme, |b| {
                    for (from, to) in [
                        ("loop", "mediaLoop"),
                        ("hue", "hue"),
                        ("saturation", "saturation"),
                        ("brightness", "brightness"),
                        ("contrast", "contrast"),
                    ] {
                        match playback.get(from) {
                            Some(value) => b.insert(to.into(), value.clone()),
                            None => b.remove(to),
                        };
                    }
                });
                true
            }
            Command::ClearText { background, .. } => {
                let Some(render) = self.last_render.as_mut() else {
                    return false;
                };
                render.reference.clear();
                render.text.clear();
                render.colored_lines = None;
                if let Some(next) = background.as_ref().and_then(|b| b.value().ok())
                    && background_of(&render.theme).as_ref() != Some(&next)
                {
                    render.theme = with_background(&render.theme, |b| {
                        *b = object(next).unwrap_or_default();
                    });
                    self.painted();
                }
                true
            }
            Command::Clear { .. } => {
                self.last_output_id = None;
                self.last_render = None;
                self.painted();
                true
            }
            Command::VideoTransport { .. } => true,
            Command::ShowDocument {
                source,
                slide,
                document,
                ..
            } => {
                if !self.show_slide(source, slide, available) {
                    return false;
                }
                self.document_mode = true;
                self.document_slide = document.clone();
                true
            }
            Command::SwapPage {
                source,
                media_path,
                document,
                ..
            } => {
                if !self.document_mode || self.last_output_id.as_ref() != Some(source) {
                    return false;
                }
                let Some(render) = self.last_render.as_mut() else {
                    return false;
                };
                render.theme = with_background(&render.theme, |b| {
                    b.insert("mediaPath".into(), Value::String(media_path.clone()));
                });
                self.document_slide = document.clone();
                true
            }
            Command::PreloadPage { .. } => self.document_mode,
            Command::DocumentVideos { document, .. } => {
                self.document_slide = Some(document.clone());
                true
            }
            Command::ControlDocumentVideo {
                id, page, video, ..
            } => self.controls_video(id, *page, *video),
            Command::SetLayer { kind, layer, .. } => {
                *self.layers.layer_mut(*kind) = layer.clone();
                true
            }
            Command::SetInfo { info, .. } => {
                self.layers.info = info.clone();
                true
            }
            Command::SetCamera { camera, .. } => {
                self.layers.camera = camera.clone();
                true
            }
            Command::SetWeb { web, .. } => {
                self.layers.web = web.clone();
                true
            }
            Command::WebPointer { .. } | Command::WebScroll { .. } => self.layers.web.is_some(),
            Command::SetTestPattern { pattern, .. } => {
                if !available {
                    return false;
                }
                self.test_pattern = *pattern;
                true
            }
            Command::RetainSurfaces { .. } | Command::Shutdown => true,
        }
    }

    fn show_slide(&mut self, source: &str, slide: &SlideContent, available: bool) -> bool {
        if !available {
            return false;
        }
        self.last_output_id = Some(source.to_owned());
        self.last_render = Some(slide.clone());
        self.painted();
        true
    }

    /// `paint` leaves document mode.
    fn painted(&mut self) {
        self.document_mode = false;
        self.document_slide = None;
    }

    /// The guard in `controlDocumentVideo`.
    fn controls_video(&self, id: &str, page: f64, video: u32) -> bool {
        if !self.document_mode {
            return false;
        }
        let Some(slide) = self.document_slide.as_ref().and_then(|s| s.value().ok()) else {
            return false;
        };
        slide.get("id").and_then(Value::as_str) == Some(id)
            && slide.get("page").and_then(Value::as_f64) == Some(page)
            && slide
                .get("videos")
                .and_then(Value::as_array)
                .is_some_and(|videos| (video as usize) < videos.len())
    }

    /// Commands that rebuild this state on an engine that has never seen it,
    /// in the order `ProgramSurface.recover` and `ready` use: shell settings,
    /// layers, test pattern, then the slide (instant) and its document.
    pub fn replay(&self) -> Vec<Command> {
        let output = self.spec.output.clone();
        let mut commands = vec![Command::OpenSurface(self.spec.clone())];
        if self.config != SurfaceConfig::default() {
            commands.push(Command::Configure {
                output: output.clone(),
                config: self.config.clone(),
            });
        }
        for kind in LayerKind::ALL {
            if let Some(layer) = self.layers.layer(kind) {
                commands.push(Command::SetLayer {
                    output: output.clone(),
                    kind,
                    layer: Some(layer.clone()),
                });
            }
        }
        if self.layers.info.is_some() {
            commands.push(Command::SetInfo {
                output: output.clone(),
                info: self.layers.info.clone(),
            });
        }
        if self.layers.camera.is_some() {
            commands.push(Command::SetCamera {
                output: output.clone(),
                camera: self.layers.camera.clone(),
            });
        }
        if self.layers.web.is_some() {
            commands.push(Command::SetWeb {
                output: output.clone(),
                web: self.layers.web.clone(),
            });
        }
        if self.test_pattern.is_some() {
            commands.push(Command::SetTestPattern {
                output: output.clone(),
                pattern: self.test_pattern,
            });
        }
        if let Some(slide) = self.last_render.clone() {
            let source = self.last_output_id.clone().unwrap_or_default();
            commands.push(if self.document_mode {
                Command::ShowDocument {
                    output: output.clone(),
                    source,
                    slide,
                    document: self.document_slide.clone(),
                }
            } else {
                Command::ShowSlide {
                    output: output.clone(),
                    source,
                    slide,
                    instant: true,
                }
            });
        }
        if self.parked {
            commands.push(Command::CloseSurface {
                output,
                destroy: false,
            });
        }
        commands
    }
}

fn object(value: Value) -> Option<Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// The theme's background, compared by value as `JSON.stringify` compared it.
fn background_of(theme: &Json) -> Option<Value> {
    theme.value().ok()?.get_mut("background").map(Value::take)
}

/// `{ ...theme, background: f(theme.background) }`. A theme that is not an
/// object is left as it is: the scene builder reports it.
fn with_background(theme: &Json, edit: impl FnOnce(&mut Map<String, Value>)) -> Json {
    let Ok(Value::Object(mut theme_map)) = theme.value() else {
        return theme.clone();
    };
    let mut background = theme_map
        .remove("background")
        .and_then(object)
        .unwrap_or_default();
    edit(&mut background);
    theme_map.insert("background".into(), Value::Object(background));
    Json::from_value(&Value::Object(theme_map))
}
