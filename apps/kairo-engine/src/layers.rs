//! Program layers, the info layer and test patterns on their way to the
//! compositor (prompt 054).
//!
//! A layer is planned and drawn by `kairo-scene` on the visual workers.
//! Its pieces become one group node: still pixels, a moving ticker,
//! pictures (decoded in a second stage, as an `<img>` arrives late), and
//! live readouts. A live readout is its own child of the group, so a tick
//! that changes a countdown redraws only that readout's pixels and swaps
//! that one child; the rest of the layer is reused as it is.

use crate::visuals::{Callback, Output, Shared};
use kairo_compositor::{Content, Fill, Fit, Look, Motion, Raster, Rect};
use kairo_domain::program::{ConfidenceInfo, ProgramLayer, StageView, format_clock};
use kairo_scene::media::{BoxFit, FitMode};
use kairo_scene::program::{
    Live, LiveState, Now, Pattern, Piece, plan_info, plan_layer, plan_pattern, plan_stage,
};
use kairo_scene::vector::Tile;
use kairo_scene::with_typesetter;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// What a layer draws.
#[derive(Clone, Debug)]
pub enum LayerSource {
    Program(ProgramLayer),
    Info(ConfidenceInfo),
    Pattern {
        pattern: Pattern,
        name: String,
    },
    /// `stage.html` (1920 × 1080). Stage windows are opened by prompt 060.
    #[cfg_attr(not(test), allow(dead_code))]
    Stage(StageView),
}

pub struct LayerJob {
    pub source: LayerSource,
    pub frame: (u32, u32),
    pub now: Now,
    /// Tests compare a first frame: moving parts stay where they start.
    pub still: bool,
}

/// A readout inside a layer's group.
#[derive(Clone, Debug)]
pub struct LivePart {
    /// Its index among the group's children.
    pub index: usize,
    pub live: Live,
    /// What it shows now (or is being redrawn to show).
    pub state: LiveState,
}

pub struct LayerOut {
    /// True once pictures are in. The engine needs no flag (the queue
    /// knows whether the command still waits); the test rig counts by it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub full: bool,
    pub children: Vec<Content>,
    pub live: Vec<LivePart>,
}

impl LayerOut {
    pub fn content(&self) -> Content {
        group(self.children.clone())
    }
}

pub struct LiveJob {
    pub index: usize,
    pub live: Live,
    pub state: LiveState,
}

pub struct LiveOut {
    pub index: usize,
    pub content: Content,
}

pub fn group(children: Vec<Content>) -> Content {
    Content::new(Fill::Group(Arc::new(children)))
}

/// The wall clock as layers read it: `Date.now()` and the local time.
pub fn now() -> Now {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    Now {
        ms: ms.floor(),
        clock: format_clock(chrono::Local::now()),
    }
}

pub(crate) fn image(tile: Tile) -> Content {
    Content {
        rect: Some(Rect::new(
            tile.x as f32,
            tile.y as f32,
            tile.width as f32,
            tile.height as f32,
        )),
        ..Content::new(Fill::Image(Raster::new(
            tile.width,
            tile.height,
            tile.pixels,
        )))
    }
}

fn rect(r: kairo_scene::Rect) -> Rect {
    Rect::new(r.x, r.y, r.width, r.height)
}

/// A readout's pixels for `state`; an empty readout keeps its place.
fn live_content(live: &Live, state: &LiveState) -> Content {
    with_typesetter(|ts| live.draw(ts, state)).map_or_else(Content::empty, image)
}

/// The pieces as contents. Pictures stay `Pending` unless `decode`.
fn contents(
    shared: &Shared,
    pieces: &[Piece],
    now: &Now,
    decode: bool,
    still: bool,
) -> (Vec<Content>, Vec<LivePart>, bool) {
    let mut children = Vec::with_capacity(pieces.len());
    let mut live = Vec::new();
    let mut pictures = false;
    for piece in pieces {
        let content = match piece {
            Piece::Pixels(tile) => image(tile.clone()),
            Piece::Moving {
                tile,
                clip,
                distance,
                period_ms,
                alternate,
            } => Content {
                clip: Some(rect(*clip)),
                motion: (!still).then_some(Motion {
                    distance: *distance,
                    period_ms: *period_ms,
                    alternate: *alternate,
                    ..Motion::default()
                }),
                ..image(tile.clone())
            },
            Piece::Scaled {
                width,
                height,
                pixels,
                rect: r,
                clip,
            } => Content {
                rect: Some(rect(*r)),
                clip: clip.map(rect),
                ..Content::new(Fill::Image(Raster::new(*width, *height, pixels.clone())))
            },
            Piece::Picture {
                path,
                rect: r,
                fit,
                opacity,
            } => {
                pictures = true;
                let mut content = Content {
                    rect: Some(rect(*r)),
                    opacity: *opacity,
                    look: Look {
                        fit: match fit {
                            FitMode::Cover => Fit::Cover,
                            FitMode::Contain => Fit::Contain,
                            FitMode::Fill => Fit::Fill,
                        },
                        ..Look::default()
                    },
                    ..Content::new(Fill::Pending)
                };
                if decode {
                    let target = BoxFit {
                        width: r.width.round().max(1.0) as u32,
                        height: r.height.round().max(1.0) as u32,
                        fit: *fit,
                    };
                    // A missing or broken picture draws nothing (DECISIONS 053).
                    content.fill = shared
                        .picture(path, target)
                        .map_or(Fill::Empty, |d| d.fill());
                }
                content
            }
            Piece::Live(l) => {
                let state = l.state(now);
                let content = live_content(l, &state);
                live.push(LivePart {
                    index: children.len(),
                    live: l.clone(),
                    state,
                });
                content
            }
        };
        children.push(content);
    }
    (children, live, pictures)
}

/// Plans and draws a layer. With pictures, `fast` gets it without them at
/// once and `done` gets it with them; otherwise `done` gets the only result.
pub fn run_layer(shared: &Shared, job: LayerJob, fast: Option<Callback>, done: Callback) {
    let plan = with_typesetter(|ts| match &job.source {
        LayerSource::Program(layer) => plan_layer(ts, layer, job.frame),
        LayerSource::Info(info) => plan_info(ts, info, job.frame),
        LayerSource::Pattern { pattern, name } => plan_pattern(ts, *pattern, name, job.frame),
        LayerSource::Stage(view) => plan_stage(ts, view),
    });
    let (children, live, pictures) = contents(shared, &plan.pieces, &job.now, false, job.still);
    let first = LayerOut {
        full: !pictures,
        children,
        live,
    };
    if !pictures {
        done(Output::Layer(first));
        return;
    }
    if let Some(fast) = fast {
        fast(Output::Layer(first));
    }
    let (children, live, _) = contents(shared, &plan.pieces, &job.now, true, job.still);
    done(Output::Layer(LayerOut {
        full: true,
        children,
        live,
    }));
}

pub fn run_live(job: LiveJob) -> LiveOut {
    LiveOut {
        index: job.index,
        content: live_content(&job.live, &job.state),
    }
}

/// A layer on screen whose readouts tick.
pub struct LiveLayer {
    pub key: String,
    pub generation: u64,
    pub children: Vec<Content>,
    pub parts: Vec<LivePart>,
}

impl LiveLayer {
    /// The readouts whose state differs at `now`; each is marked as being
    /// redrawn to its new state, so it is asked for once.
    pub fn due(&mut self, now: &Now) -> Vec<LiveJob> {
        let mut jobs = Vec::new();
        for part in &mut self.parts {
            let state = part.live.state(now);
            if state != part.state {
                part.state = state.clone();
                jobs.push(LiveJob {
                    index: part.index,
                    live: part.live.clone(),
                    state,
                });
            }
        }
        jobs
    }

    /// Puts a redrawn readout in; the group to show.
    pub fn replace(&mut self, index: usize, content: Content) -> Option<Content> {
        let child = self.children.get_mut(index)?;
        *child = content;
        Some(group(self.children.clone()))
    }
}
