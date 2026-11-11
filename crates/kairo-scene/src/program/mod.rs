//! The program layers above the slide, drawn as data instead of markup:
//! the audience message (still or ticker), props, the timer design, poll,
//! question and join card, logo, the confidence info bar, the stage
//! display and the diagnostic test patterns.
//!
//! Each layer becomes a [`LayerPlan`]: pixels drawn now, pictures the
//! caller decodes, a ticker that moves, and live parts (a countdown or a
//! clock) that change by themselves. A live part is drawn on its own, so a
//! tick redraws only its readout and never the rest of the layer.
//!
//! Layout follows the CSS of `program.ts`, `overlay.html` and `stage.html`
//! box by box; the comments name the rule each number comes from.

mod info;
mod layers;
mod pattern;
mod stage;

pub use info::plan_info;
pub use layers::plan_layer;
pub use pattern::{Pattern, plan_pattern};
pub use stage::{STAGE_FRAME, plan_stage};

use crate::color::{Rgba, parse_css};
use crate::media::FitMode;
use crate::raster::rasterize_with;
use crate::scene::{
    Align, BoxKind, Decoration, Fit, Paragraph, Rect, Shadow, SlideScene, TextBox, TextStyle,
    Vertical,
};
use crate::text::collapse;
use crate::typeset::Typesetter;
use crate::vector::{Tile, ellipse, rounded_rect};
use kairo_domain::program::{ProgramTimerState, format_timer, timer_readout};
use tiny_skia::{FillRule, Paint, Pixmap, Transform};

/// `'Source Sans 3','Helvetica Neue',Arial,sans-serif` (messages, props,
/// info bar, stage, the question card's `sans`) as Electron's screens
/// resolve it: the renderer registers the bundled face as `Source Sans 3
/// Variable`, so `Source Sans 3` is never found there and the screens draw
/// Helvetica Neue (Arial elsewhere). See DECISIONS 054.
pub const SANS: &str = "'Helvetica Neue',Arial,sans-serif";
pub const SERIF: &str = "Georgia,'Times New Roman',serif";
pub const MONO: &str = "'SF Mono',Menlo,Consolas,monospace";
pub const ARIAL: &str = "Arial,sans-serif";

/// The moment a layer is drawn at: wall time in milliseconds (`Date.now()`)
/// and the local clock as the screens show it (`9:05 PM`).
#[derive(Clone, Debug, PartialEq)]
pub struct Now {
    pub ms: f64,
    pub clock: String,
}

/// One drawn part of a layer, back to front.
#[derive(Clone, Debug)]
pub enum Piece {
    /// Pixels ready now.
    Pixels(Tile),
    /// Pixels that slide along x forever, cut to `clip` (the ticker, the
    /// diagnostic sweep).
    Moving {
        tile: Tile,
        clip: Rect,
        distance: f32,
        period_ms: u32,
        alternate: bool,
    },
    /// An `<img>`: a file the caller decodes, placed by `object-fit`.
    Picture {
        path: String,
        rect: Rect,
        fit: FitMode,
        opacity: f32,
    },
    /// Pixels the compositor scales into `rect` (the join QR code, which
    /// the browser enlarges smoothly).
    Scaled {
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        rect: Rect,
        clip: Option<Rect>,
    },
    /// A countdown or clock, drawn by [`Live::draw`] whenever its
    /// [`LiveState`] changes.
    Live(Live),
}

/// A drawn layer: what `swapLayer` would have put in the slot.
#[derive(Clone, Debug, Default)]
pub struct LayerPlan {
    pub pieces: Vec<Piece>,
}

impl LayerPlan {
    fn push(&mut self, tile: Option<Tile>) {
        if let Some(tile) = tile {
            self.pieces.push(Piece::Pixels(tile));
        }
    }

    /// Whether anything in it changes with time.
    pub fn is_live(&self) -> bool {
        self.pieces.iter().any(|p| matches!(p, Piece::Live(_)))
    }
}

/// How a live part looks right now. Equal states draw equal pixels, so a
/// tick that changes nothing draws nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveState {
    pub text: String,
    pub color: Rgba,
    pub opacity: f32,
}

/// What a countdown counts: `PropTimerView` without the moment.
#[derive(Clone, Debug, PartialEq)]
pub struct TimerRun {
    pub duration_sec: f64,
    pub ends_at: Option<f64>,
    pub remaining_sec: f64,
    pub rollover: bool,
}

impl TimerRun {
    /// `timerReadout` (and `propTimerUpdate`, `stage.html`'s `tick`): the
    /// text, whether time is up, whether it is paused part-way.
    fn readout(&self, now_ms: f64) -> (String, bool, bool) {
        let state = ProgramTimerState {
            duration_sec: self.duration_sec,
            ends_at: self.ends_at,
            remaining_sec: self.remaining_sec,
        };
        let r = timer_readout(&state, now_ms, self.rollover);
        (format_timer(r.seconds), r.time_up, r.paused)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LiveKind {
    /// A countdown: overrun colour when time is up, 0.55 opacity when paused.
    Timer {
        run: TimerRun,
        color: Rgba,
        overrun: Rgba,
    },
    /// The 12-hour wall clock.
    Clock { color: Rgba },
}

/// A countdown or clock and the box its text sits in.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub kind: LiveKind,
    /// The text's look; its colour comes from the state.
    pub text: TextLook,
    /// Where the text is laid out, and how.
    pub rect: Rect,
    pub vertical: Vertical,
    /// `overflow: hidden` boxes cut the text to `rect`.
    pub clipped: bool,
    /// The object's own opacity, under the paused dimming.
    pub opacity: f32,
    pub frame: (u32, u32),
}

impl Live {
    pub fn state(&self, now: &Now) -> LiveState {
        match &self.kind {
            LiveKind::Timer {
                run,
                color,
                overrun,
            } => {
                let (text, time_up, paused) = run.readout(now.ms);
                LiveState {
                    text,
                    color: if time_up { *overrun } else { *color },
                    opacity: if paused { 0.55 } else { 1.0 },
                }
            }
            LiveKind::Clock { color } => LiveState {
                text: now.clock.clone(),
                color: *color,
                opacity: 1.0,
            },
        }
    }

    /// The readout's pixels for `state`; `None` for empty text.
    pub fn draw(&self, ts: &mut Typesetter, state: &LiveState) -> Option<Tile> {
        if state.text.is_empty() {
            return None;
        }
        let mut look = self.text.clone();
        look.color = state.color;
        let tile = text_tile(
            ts,
            &look,
            &state.text,
            Space::NoWrap,
            self.rect,
            self.vertical,
            self.clipped,
            self.frame,
        )?;
        Some(fade(tile, self.opacity * state.opacity))
    }
}

/// How white space is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// `textContent` under `white-space: normal`: one paragraph, newlines
    /// are spaces.
    Normal,
    /// `escapeAndBreak` under `normal`: each newline is a `<br>`.
    Breaks,
    /// `white-space: pre-wrap`: spaces kept, newlines break.
    Pre,
    /// `white-space: nowrap`: collapsed, one line.
    NoWrap,
}

/// A run of text's CSS: family, size, weight, line height, spacing,
/// colour, alignment, tabular digits and shadow.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLook {
    pub family: &'static str,
    pub size: f32,
    pub weight: f32,
    /// A multiple of the size, or [`crate::LINE_HEIGHT_NORMAL`].
    pub line_height: f32,
    pub letter_spacing: f32,
    pub color: Rgba,
    pub align: Align,
    pub tabular: bool,
    pub shadow: Option<Shadow>,
}

impl TextLook {
    pub fn new(
        family: &'static str,
        size: f32,
        weight: f32,
        line_height: f32,
        color: Rgba,
    ) -> Self {
        Self {
            family,
            size,
            weight,
            line_height,
            letter_spacing: 0.0,
            color,
            align: Align::Left,
            tabular: false,
            shadow: None,
        }
    }

    fn style(&self) -> TextStyle {
        TextStyle {
            family: self.family.to_owned(),
            weight: self.weight,
            line_height: self.line_height,
            tabular: self.tabular,
            letter_spacing: self.letter_spacing,
            align: self.align,
            decoration: Decoration::None,
            color: self.color,
            shadow: self.shadow,
            stroke: None,
        }
    }
}

fn paragraphs(text: &str, space: Space) -> Vec<Paragraph> {
    let plain = |text: String| Paragraph { text, color: None };
    match space {
        Space::Normal | Space::NoWrap => {
            let one = collapse(text);
            if one.is_empty() {
                Vec::new()
            } else {
                vec![plain(one)]
            }
        }
        Space::Breaks => {
            let mut out: Vec<Paragraph> = text.split('\n').map(|l| plain(collapse(l))).collect();
            if out.last().is_some_and(|p| p.text.is_empty()) {
                out.pop();
            }
            out
        }
        Space::Pre => {
            let mut out: Vec<Paragraph> = text
                .split('\n')
                .map(|l| plain(l.trim_end_matches('\r').to_owned()))
                .collect();
            if out.last().is_some_and(|p| p.text.is_empty()) {
                out.pop();
            }
            out
        }
    }
}

/// Wide enough for one line of anything on a 1920 frame.
const NO_WRAP: f32 = 1.0e6;

fn text_box(
    look: &TextLook,
    text: &str,
    space: Space,
    rect: Rect,
    padding: f32,
    vertical: Vertical,
) -> TextBox {
    TextBox {
        kind: BoxKind::Reference,
        rect,
        padding,
        vertical,
        backdrop: None,
        size: Fit::Fixed(look.size),
        style: look.style(),
        paragraphs: paragraphs(text, space),
    }
}

/// The laid-out width and height of `text` in `width` (`NO_WRAP` for its
/// max-content size).
pub fn measure(
    ts: &mut Typesetter,
    look: &TextLook,
    text: &str,
    space: Space,
    width: f32,
) -> (f32, f32) {
    let width = if space == Space::NoWrap {
        NO_WRAP
    } else {
        width
    };
    let b = text_box(
        look,
        text,
        space,
        Rect {
            x: 0.0,
            y: 0.0,
            width,
            height: 0.0,
        },
        0.0,
        Vertical::Top,
    );
    if b.paragraphs.is_empty() {
        return (0.0, 0.0);
    }
    let shaped = ts.shape(&b, look.size, width, false);
    (shaped.width, shaped.height)
}

/// Draws `text` laid out in `rect`. A clipped box cuts the text at its
/// edges; otherwise glyphs may spill past it, as CSS lets them.
#[allow(clippy::too_many_arguments)]
pub fn text_tile(
    ts: &mut Typesetter,
    look: &TextLook,
    text: &str,
    space: Space,
    rect: Rect,
    vertical: Vertical,
    clipped: bool,
    frame: (u32, u32),
) -> Option<Tile> {
    let (rect, padding) = if clipped {
        (rect, 0.0)
    } else {
        // Room for ascenders above a tight line box, descenders, the
        // shadow and anything else that spills.
        let pad = look.size.max(16.0);
        (
            Rect {
                x: rect.x - pad,
                y: rect.y - pad,
                width: rect.width + 2.0 * pad,
                height: rect.height + 2.0 * pad,
            },
            pad,
        )
    };
    let mut b = text_box(look, text, space, rect, padding, vertical);
    if b.paragraphs.is_empty() {
        return None;
    }
    if space == Space::NoWrap {
        // One line, aligned in the box: lay it out wide and place it.
        let (w, _) = measure(ts, look, text, space, NO_WRAP);
        let inner = rect.width - 2.0 * padding;
        let shift = match look.align {
            Align::Left | Align::Justify => 0.0,
            Align::Center => (inner - w) / 2.0,
            Align::Right => inner - w,
        };
        b.style.align = Align::Left;
        b.rect.x += shift;
        b.rect.width = w + 2.0 * padding + 1.0;
        if clipped {
            // Keep the cut at the real box.
            let scene = SlideScene {
                frame,
                boxes: vec![b],
            };
            let tile = rasterize_with(ts, &scene).0?;
            return clip_tile(
                Tile {
                    x: tile.x as i32,
                    y: tile.y as i32,
                    width: tile.width,
                    height: tile.height,
                    pixels: tile.pixels,
                },
                rect,
            );
        }
    }
    let scene = SlideScene {
        frame,
        boxes: vec![b],
    };
    let tile = rasterize_with(ts, &scene).0?;
    Some(Tile {
        x: tile.x as i32,
        y: tile.y as i32,
        width: tile.width,
        height: tile.height,
        pixels: tile.pixels,
    })
}

/// Cuts a tile to `rect` (rounded outward to pixels).
fn clip_tile(tile: Tile, rect: Rect) -> Option<Tile> {
    let x0 = (rect.x.floor() as i32).max(tile.x);
    let y0 = (rect.y.floor() as i32).max(tile.y);
    let x1 = ((rect.x + rect.width).ceil() as i32).min(tile.x + tile.width as i32);
    let y1 = ((rect.y + rect.height).ceil() as i32).min(tile.y + tile.height as i32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h {
        let sy = (y0 - tile.y) as u32 + row;
        let start = ((sy * tile.width + (x0 - tile.x) as u32) * 4) as usize;
        pixels.extend_from_slice(&tile.pixels[start..start + (w * 4) as usize]);
    }
    Some(Tile {
        x: x0,
        y: y0,
        width: w,
        height: h,
        pixels,
    })
}

/// Multiplies a premultiplied tile by `opacity`.
pub fn fade(mut tile: Tile, opacity: f32) -> Tile {
    if opacity < 1.0 {
        let k = opacity.clamp(0.0, 1.0);
        for v in &mut tile.pixels {
            *v = (f32::from(*v) * k).round() as u8;
        }
    }
    tile
}

/// The corners of a box: px each, or an ellipse (`border-radius: 50%`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Corners {
    Px(f32),
    Ellipse,
}

/// A filled box. A square one is snapped to whole pixels, as Chromium
/// paints a plain background; a rounded one is drawn anti-aliased where it
/// falls (measured on the poll and question cards). `None` when it has no
/// area or no colour.
pub fn box_tile(rect: Rect, corners: Corners, color: Rgba, frame: (u32, u32)) -> Option<Tile> {
    if color.a == 0 {
        return None;
    }
    let square = matches!(corners, Corners::Px(r) if r <= 0.0);
    let (x0, y0, x1, y1) = if square {
        (
            rect.x.round(),
            rect.y.round(),
            (rect.x + rect.width).round(),
            (rect.y + rect.height).round(),
        )
    } else {
        (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height)
    };
    // Only what is on the frame is drawn.
    let cx0 = x0.floor().max(0.0);
    let cy0 = y0.floor().max(0.0);
    let cx1 = x1.ceil().min(frame.0 as f32);
    let cy1 = y1.ceil().min(frame.1 as f32);
    if cx1 <= cx0 || cy1 <= cy0 {
        return None;
    }
    let (w, h) = ((cx1 - cx0) as u32, (cy1 - cy0) as u32);
    let mut pixmap = Pixmap::new(w, h)?;
    let (bx, by, bw, bh) = (x0 - cx0, y0 - cy0, x1 - x0, y1 - y0);
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    let path = match corners {
        Corners::Ellipse => ellipse(bx, by, bw, bh),
        Corners::Px(r) => {
            // CSS scales all radii down together when they cannot fit.
            let r = r.max(0.0);
            let r = if r * 2.0 > bw.min(bh) {
                bw.min(bh) / 2.0
            } else {
                r
            };
            if r <= 0.0 {
                paint.anti_alias = false;
            }
            rounded_rect(bx, by, bw, bh, r)
        }
    }?;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    Some(Tile {
        x: cx0 as i32,
        y: cy0 as i32,
        width: w,
        height: h,
        pixels: pixmap.take(),
    })
}

/// A CSS colour as the browser takes it; an unreadable one is dropped.
pub fn css(color: &str) -> Option<Rgba> {
    parse_css(color)
}

/// `colorWithOpacity` in `program.ts`: hex colours take the card opacity;
/// anything else keeps its own alpha.
pub fn color_with_opacity(color: &str, opacity_pct: f64) -> Option<Rgba> {
    if opacity_pct >= 100.0 || opacity_pct.is_nan() {
        return css(color);
    }
    if opacity_pct <= 0.0 {
        return Some(Rgba::new(0, 0, 0, 0));
    }
    let hex = kairo_domain::js::trim(color);
    let digits = hex
        .strip_prefix('#')
        .filter(|d| d.chars().all(|c| c.is_ascii_hexdigit()));
    match digits {
        Some(d) if d.len() == 6 || d.len() == 3 => {
            let base = css(hex)?;
            let a = (kairo_domain::js::math_round(opacity_pct) / 100.0).clamp(0.0, 1.0);
            Some(Rgba::new(base.r, base.g, base.b, (a * 255.0).round() as u8))
        }
        _ => css(color),
    }
}

/// `object-fit` for a picture piece.
pub fn fit_of(cover: bool) -> FitMode {
    if cover {
        FitMode::Cover
    } else {
        FitMode::Contain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_colours_take_the_opacity_only_when_hex() {
        assert_eq!(
            color_with_opacity("#18212d", 95.0),
            Some(Rgba::new(0x18, 0x21, 0x2d, 242))
        );
        assert_eq!(
            color_with_opacity("#fff", 50.0),
            Some(Rgba::new(255, 255, 255, 128))
        );
        assert_eq!(
            color_with_opacity("#18212d", 0.0),
            Some(Rgba::new(0, 0, 0, 0))
        );
        assert_eq!(
            color_with_opacity("#18212d", 100.0),
            Some(Rgba::new(0x18, 0x21, 0x2d, 255))
        );
        // Not hex: the colour's own alpha stays.
        assert_eq!(
            color_with_opacity("rgba(0,0,0,0.5)", 20.0),
            css("rgba(0,0,0,0.5)")
        );
    }

    #[test]
    fn white_space_modes_read_text_as_css_does() {
        let texts = |p: Vec<Paragraph>| p.into_iter().map(|p| p.text).collect::<Vec<_>>();
        assert_eq!(texts(paragraphs(" a \n b ", Space::Normal)), ["a b"]);
        assert_eq!(texts(paragraphs(" a \n b \n", Space::Breaks)), ["a", "b"]);
        assert_eq!(
            texts(paragraphs(" a  \n\n b\n", Space::Pre)),
            [" a  ", "", " b"]
        );
        assert!(paragraphs("  ", Space::NoWrap).is_empty());
    }
}
