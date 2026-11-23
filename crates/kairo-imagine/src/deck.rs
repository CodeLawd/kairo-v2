//! Slides Kairo designs and draws itself. The assistant writes structured
//! content (a layout, an eyebrow, a title, a lead and items with headings);
//! this module turns it into a design: shapes, cards, badges, rings, bars
//! and text boxes ([`Mark`]s). The same marks are drawn into the slide image
//! and written to PowerPoint as native, editable shapes. The image model only
//! paints optional pictures, never text.

use image::imageops;
use kairo_scene::color::{Rgba, parse_css};
use kairo_scene::{
    Align, BoxKind, Decoration, Fit, Paragraph, Rect, SlideScene, TextBox, TextStyle, Vertical,
    rasterize,
};
use serde::{Deserialize, Serialize};
use tiny_skia::{
    FillRule, IntSize, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform,
};

use crate::ImagineError;

pub const WIDTH: u32 = 1920;
pub const HEIGHT: u32 = 1080;

/// Font choices the assistant may pick from.
pub const FONTS: [&str; 14] = [
    "Helvetica Neue",
    "Inter",
    "Avenir Next",
    "Futura",
    "Gill Sans",
    "Optima",
    "Source Sans 3",
    "Georgia",
    "Palatino",
    "Baskerville",
    "Didot",
    "Rockwell",
    "Copperplate",
    "American Typewriter",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// The opening: eyebrow, large title, subtitle, a tagline.
    Title,
    /// A section break.
    Section,
    /// A single oversized statement or thesis in open space. No items.
    Hero,
    /// Two to six even cards, each a badge, a heading and a sentence.
    Cards,
    /// One large focal block and two or three smaller supporting blocks.
    Bento,
    /// Rows of points with a number, a heading and a sentence.
    Checklist,
    /// Two to four columns in order (steps, weeks, stages).
    Steps,
    /// Three to five connected milestones along a line.
    Timeline,
    /// Two to four large figures in cards.
    Stats,
    /// One to three massive figures with micro-labels and thin dividers.
    Kpi,
    /// Horizontal bars for values the operator gave.
    Chart,
    /// One quotation.
    Quote,
    /// Points beside a picture.
    ImageText,
    /// An edge-to-edge picture with the words in its open space.
    FullBleed,
    /// A 60/40 split: a picture and tight copy, or two contrasting states.
    Split,
    /// The headline on top and a wide picture or row of columns below.
    Panoramic,
    /// A clean typographic list with a vertical accent line.
    Editorial,
    /// A plain list.
    #[default]
    Bullets,
}

impl Layout {
    pub const ALL: [Layout; 18] = [
        Layout::Title,
        Layout::Section,
        Layout::Hero,
        Layout::Cards,
        Layout::Bento,
        Layout::Checklist,
        Layout::Steps,
        Layout::Timeline,
        Layout::Stats,
        Layout::Kpi,
        Layout::Chart,
        Layout::Quote,
        Layout::ImageText,
        Layout::FullBleed,
        Layout::Split,
        Layout::Panoramic,
        Layout::Editorial,
        Layout::Bullets,
    ];

    /// Title, section and quote slides: older decks kept their subtitle or
    /// quotation in the body.
    pub fn is_display(self) -> bool {
        matches!(self, Layout::Title | Layout::Section | Layout::Quote)
    }

    /// Slides drawn in the deck's title colours.
    pub fn is_dark(self) -> bool {
        matches!(
            self,
            Layout::Title | Layout::Section | Layout::Quote | Layout::Hero | Layout::FullBleed
        )
    }

    /// Slides that can carry a picture of their own.
    pub fn takes_art(self) -> bool {
        matches!(
            self,
            Layout::Title
                | Layout::Section
                | Layout::Quote
                | Layout::Hero
                | Layout::ImageText
                | Layout::FullBleed
                | Layout::Split
                | Layout::Panoramic
        )
    }

    /// A layout that holds the same content differently, used so no two
    /// slides in a row share a layout.
    pub fn sibling(self) -> Layout {
        match self {
            Layout::Title | Layout::Section => Layout::Hero,
            Layout::Hero => Layout::Section,
            Layout::Cards => Layout::Bento,
            Layout::Bento => Layout::Cards,
            Layout::Checklist | Layout::Bullets => Layout::Editorial,
            Layout::Editorial => Layout::Checklist,
            Layout::Steps => Layout::Timeline,
            Layout::Timeline => Layout::Steps,
            Layout::Stats | Layout::Chart => Layout::Kpi,
            Layout::Kpi => Layout::Stats,
            Layout::Quote => Layout::Hero,
            Layout::ImageText => Layout::Split,
            Layout::Split => Layout::ImageText,
            Layout::FullBleed => Layout::Panoramic,
            Layout::Panoramic => Layout::FullBleed,
        }
    }

    /// The archetype's name, for the deck's rhythm map.
    pub fn name(self) -> &'static str {
        match self {
            Layout::Title => "Title",
            Layout::Section => "Section break",
            Layout::Hero => "Hero statement",
            Layout::Cards => "Cards",
            Layout::Bento => "Bento grid",
            Layout::Checklist => "Checklist",
            Layout::Steps => "Steps",
            Layout::Timeline => "Timeline",
            Layout::Stats => "Stats",
            Layout::Kpi => "KPI spotlight",
            Layout::Chart => "Chart",
            Layout::Quote => "Quote",
            Layout::ImageText => "Text and picture",
            Layout::FullBleed => "Full-bleed visual",
            Layout::Split => "Split screen",
            Layout::Panoramic => "Panoramic",
            Layout::Editorial => "Editorial list",
            Layout::Bullets => "List",
        }
    }
}

/// Which side of the slide a picture (or the stronger half of a contrast)
/// takes. Words sit on the other side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    #[default]
    Right,
}

impl Side {
    pub fn flip(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// One item on a slide: a card, a row, a step, a figure or a bar.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// A badge ("01"), a step name ("Week 1"), a figure ("40%") or the
    /// value shown on a bar. Optional.
    #[serde(default)]
    pub label: String,
    pub heading: String,
    #[serde(default)]
    pub text: String,
    /// A chart bar's length.
    #[serde(default)]
    pub value: Option<f64>,
}

/// The large decoration on title slides, echoed small on content slides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ornament {
    #[default]
    Rings,
    Blobs,
    Dots,
    Arcs,
    Band,
    Frame,
    Lines,
    None,
}

/// How cards, rows and columns are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardStyle {
    /// Filled with the surface colour.
    #[default]
    Soft,
    /// A thin outline on the page.
    Outline,
    /// Filled, with an accent bar along the top.
    AccentTop,
    /// Filled with the soft accent.
    Tinted,
    /// No panel, only a rule above.
    Plain,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleAlign {
    #[default]
    Left,
    Center,
}

/// Where a title, section or quote slide's picture goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// Full bleed, darkened under the words.
    #[default]
    Background,
    /// The right half, beside the words.
    Split,
}

/// A deck's design direction, chosen by the assistant from the operator's
/// wishes. Older decks get the original look.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Design {
    pub ornament: Ornament,
    pub cards: CardStyle,
    /// Corner radius of cards and pictures, in slide pixels (0 to 48).
    pub radius: f32,
    /// The accent stripe down the left edge.
    pub stripe: bool,
    pub align: TitleAlign,
    pub uppercase_titles: bool,
    pub picture: Placement,
}

impl Default for Design {
    fn default() -> Self {
        Self {
            ornament: Ornament::Rings,
            cards: CardStyle::Soft,
            radius: 24.,
            stripe: true,
            align: TitleAlign::Left,
            uppercase_titles: false,
            picture: Placement::Background,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    #[default]
    Rect,
    Circle,
    Ring,
    Line,
}

/// A decorative shape the assistant places freely, behind the words.
/// Position and size are percentages of the slide.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shape {
    pub kind: ShapeKind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// A palette role (accent, accent_soft, text, muted, surface,
    /// title_background) or #RRGGBB.
    pub color: String,
    /// 0 to 1; 0 means opaque.
    pub opacity: f32,
    pub radius: f32,
}

/// A deck's look, chosen by the assistant. Colours are CSS hex.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeckTheme {
    /// The page colour of content slides.
    pub background: String,
    pub text: String,
    pub accent: String,
    pub heading_font: String,
    pub body_font: String,
    /// The visual style, in words, for any pictures.
    pub style: String,
    /// Cards and panels.
    #[serde(default)]
    pub surface: Option<String>,
    /// Secondary text.
    #[serde(default)]
    pub muted: Option<String>,
    /// Badges, tracks and callouts.
    #[serde(default)]
    pub accent_soft: Option<String>,
    /// The page and text colours of title, section and quote slides.
    #[serde(default)]
    pub title_background: Option<String>,
    #[serde(default)]
    pub title_text: Option<String>,
    #[serde(default)]
    pub design: Design,
}

impl Default for DeckTheme {
    fn default() -> Self {
        Self {
            background: "#F7F5EF".into(),
            text: "#142D33".into(),
            accent: "#C59B61".into(),
            heading_font: "Helvetica Neue".into(),
            body_font: "Helvetica Neue".into(),
            style: "calm, warm and modern".into(),
            surface: Some("#FFFFFF".into()),
            muted: Some("#5F7073".into()),
            accent_soft: Some("#DDE6E0".into()),
            title_background: Some("#142D33".into()),
            title_text: Some("#F7F5EF".into()),
            design: Design::default(),
        }
    }
}

fn color(css: &str, fallback: Rgba) -> Rgba {
    parse_css(css).unwrap_or(fallback)
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgba::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), 255)
}

fn alpha(c: Rgba, a: f32) -> Rgba {
    Rgba::new(c.r, c.g, c.b, (a.clamp(0., 1.) * 255.).round() as u8)
}

const WHITE: Rgba = Rgba::new(255, 255, 255, 255);

/// The theme's colours, with sensible ones for anything it leaves out.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Rgba,
    pub text: Rgba,
    pub accent: Rgba,
    pub surface: Rgba,
    pub muted: Rgba,
    pub accent_soft: Rgba,
    pub title_background: Rgba,
    pub title_text: Rgba,
}

impl DeckTheme {
    pub fn palette(&self) -> Palette {
        let background = color(&self.background, Rgba::new(247, 245, 239, 255));
        let text = color(&self.text, Rgba::new(20, 45, 51, 255));
        let accent = color(&self.accent, Rgba::new(197, 155, 97, 255));
        let opt = |c: &Option<String>, f: Rgba| c.as_deref().map_or(f, |c| color(c, f));
        Palette {
            background,
            text,
            accent,
            surface: opt(&self.surface, mix(background, WHITE, 0.75)),
            muted: opt(&self.muted, mix(text, background, 0.4)),
            accent_soft: opt(&self.accent_soft, mix(accent, background, 0.8)),
            title_background: opt(&self.title_background, text),
            title_text: opt(&self.title_text, background),
        }
    }
}

/// What one slide says and where it sits in the deck.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Content {
    pub layout: Layout,
    pub eyebrow: String,
    pub title: String,
    pub lead: String,
    pub points: Vec<Point>,
    pub callout: String,
    /// 1-based, and the deck's size and title, for the footer.
    pub page: usize,
    pub pages: usize,
    pub deck: String,
    /// A full-bleed picture behind a title, section or quote slide.
    pub background: bool,
    /// The slide has its own picture.
    pub art: bool,
    pub side: Side,
    pub shapes: Vec<Shape>,
}

/// One text box.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub rect: Rect,
    pub paragraphs: Vec<String>,
    /// The largest size it is drawn at, in slide pixels.
    pub size: f32,
    pub weight: f32,
    pub font: String,
    pub color: Rgba,
    pub align: Align,
    pub vertical: Vertical,
    pub letter_spacing: f32,
    pub line_height: f32,
}

impl Frame {
    pub fn bold(&self) -> bool {
        self.weight >= 600.
    }
}

/// Which picture a [`Mark::Picture`] shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picture {
    Background,
    Art,
}

/// One thing drawn on a slide, in order, back to front.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Rect {
        rect: Rect,
        fill: Rgba,
        radius: f32,
    },
    /// A rectangle's outline.
    Outline {
        rect: Rect,
        color: Rgba,
        width: f32,
        radius: f32,
    },
    /// An ellipse's outline.
    Ring {
        rect: Rect,
        color: Rgba,
        width: f32,
    },
    Picture {
        rect: Rect,
        source: Picture,
        radius: f32,
    },
    Text(Frame),
}

const fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

const M: f32 = 120.;
const SW: f32 = WIDTH as f32;
const SH: f32 = HEIGHT as f32;
const W: f32 = SW - 2. * M;
const GAP: f32 = 28.;

/// Where the picture goes on a text-and-picture slide.
pub const ART_RECT: Rect = rect(1010., 120., 790., 840.);

/// Builds text boxes in the deck's fonts and colours.
struct Type<'a> {
    theme: &'a DeckTheme,
}

impl Type<'_> {
    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        r: Rect,
        text: &str,
        size: f32,
        weight: f32,
        heading: bool,
        color: Rgba,
        align: Align,
        vertical: Vertical,
    ) -> Option<Mark> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some(Mark::Text(Frame {
            rect: r,
            paragraphs: text.lines().map(str::to_owned).collect(),
            size,
            weight,
            font: if heading {
                self.theme.heading_font.clone()
            } else {
                self.theme.body_font.clone()
            },
            color,
            align,
            vertical,
            letter_spacing: 0.,
            line_height: 1.2,
        }))
    }

    /// Small capitals with wide tracking: eyebrows, labels, footers.
    fn label(&self, r: Rect, text: &str, size: f32, color: Rgba, align: Align) -> Option<Mark> {
        let mut m = self.text(
            r,
            &text.to_uppercase(),
            size,
            800.,
            false,
            color,
            align,
            Vertical::Middle,
        )?;
        if let Mark::Text(f) = &mut m {
            f.letter_spacing = size * 0.18;
        }
        Some(m)
    }

    /// A slide title, in capitals when the design asks for them.
    fn title(
        &self,
        r: Rect,
        text: &str,
        size: f32,
        color: Rgba,
        align: Align,
        vertical: Vertical,
    ) -> Option<Mark> {
        let upper = self.theme.design.uppercase_titles;
        let text = if upper {
            text.to_uppercase()
        } else {
            text.to_owned()
        };
        let mut m = self.text(
            r,
            &text,
            if upper { size * 0.86 } else { size },
            800.,
            true,
            color,
            align,
            vertical,
        )?;
        if upper && let Mark::Text(f) = &mut m {
            f.letter_spacing = size * 0.02;
        }
        Some(m)
    }
}

fn round(r: Rect, fill: Rgba, radius: f32) -> Mark {
    Mark::Rect {
        rect: r,
        fill,
        radius,
    }
}

/// A card, row or column panel in the deck's card style.
fn panel(marks: &mut Vec<Mark>, r: Rect, p: Palette, d: &Design) {
    let radius = d.radius.clamp(0., 48.);
    match d.cards {
        CardStyle::Soft => marks.push(round(r, p.surface, radius)),
        CardStyle::Outline => marks.push(Mark::Outline {
            rect: r,
            color: alpha(p.text, 0.18),
            width: 2.,
            radius,
        }),
        CardStyle::AccentTop => {
            marks.push(round(r, p.surface, radius));
            marks.push(round(rect(r.x, r.y, r.width, 8.), p.accent, radius.min(4.)));
        }
        CardStyle::Tinted => marks.push(round(r, p.accent_soft, radius)),
        CardStyle::Plain => marks.push(round(rect(r.x, r.y, r.width, 3.), alpha(p.text, 0.14), 0.)),
    }
}

/// The WCAG contrast ratio of two colours.
fn contrast(a: Rgba, b: Rgba) -> f32 {
    let lum = |c: Rgba| {
        let ch = |v: u8| {
            let v = v as f32 / 255.;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The deck's ornament, filling `area` (large on title slides, a corner
/// accent elsewhere).
fn ornament(marks: &mut Vec<Mark>, d: &Design, p: Palette, ink: Rgba, big: bool) {
    // Centred words take the middle; the ornament keeps to the corner.
    let corner = big && d.align == TitleAlign::Center;
    let s = if corner {
        0.62
    } else if big {
        1.
    } else {
        0.42
    };
    let (cx, cy) = if corner {
        (SW - 80., 80.)
    } else if big {
        (1660., 230.)
    } else {
        (SW - 40., 40.)
    };
    match d.ornament {
        Ornament::None => {}
        Ornament::Rings => {
            for (i, r) in [460., 340., 230.].iter().enumerate() {
                let r = r * s;
                marks.push(Mark::Ring {
                    rect: rect(cx - r, cy - r, r * 2., r * 2.),
                    color: if i == 2 {
                        alpha(p.accent, 0.8)
                    } else {
                        alpha(ink, 0.16)
                    },
                    width: 3.,
                });
            }
        }
        Ornament::Blobs => {
            let r1 = 420. * s;
            let r2 = 300. * s;
            marks.push(round(
                rect(cx - r1, cy - r1, r1 * 2., r1 * 2.),
                alpha(p.accent, 0.22),
                r1,
            ));
            marks.push(round(
                rect(cx - r2 - 160. * s, cy + 120. * s - r2, r2 * 2., r2 * 2.),
                alpha(p.accent_soft, 0.35),
                r2,
            ));
        }
        Ornament::Dots => {
            let step = if big { 44. } else { 30. };
            let (cols, rows) = if big { (9, 7) } else { (6, 4) };
            let x0 = SW - M - step * (cols as f32 - 1.) - if big { 0. } else { -60. };
            let y0 = if big { 140. } else { 52. };
            for i in 0..cols {
                for j in 0..rows {
                    let r = if big { 5. } else { 4. };
                    marks.push(round(
                        rect(
                            x0 + i as f32 * step - r,
                            y0 + j as f32 * step - r,
                            r * 2.,
                            r * 2.,
                        ),
                        alpha(p.accent, 0.5),
                        r,
                    ));
                }
            }
        }
        Ornament::Arcs => {
            let r = 640. * s;
            marks.push(round(
                rect(SW - r * 0.9, -r * 0.9, r * 2., r * 2.),
                alpha(p.accent, 0.9),
                r,
            ));
            let r2 = r * 0.72;
            marks.push(round(
                rect(SW - r2 * 0.9, -r2 * 0.9, r2 * 2., r2 * 2.),
                alpha(p.accent_soft, 0.9),
                r2,
            ));
        }
        Ornament::Band => {
            if big {
                marks.push(round(
                    rect(0., SH - 150., SW, 150.),
                    alpha(p.accent, 0.9),
                    0.,
                ));
            } else {
                marks.push(round(rect(SW - 360., 0., 360., 14.), p.accent, 0.));
            }
        }
        Ornament::Frame => {
            let inset = if big { 48. } else { 32. };
            marks.push(Mark::Outline {
                rect: rect(inset, inset, SW - inset * 2., SH - inset * 2.),
                color: alpha(
                    if big { p.accent } else { ink },
                    if big { 0.7 } else { 0.12 },
                ),
                width: 3.,
                radius: d.radius.min(24.),
            });
        }
        Ornament::Lines => {
            let n = if big { 7 } else { 3 };
            for i in 0..n {
                let w = (if big { 520. } else { 220. }) - i as f32 * if big { 60. } else { 50. };
                let y = if big {
                    150. + i as f32 * 26.
                } else {
                    70. + i as f32 * 16.
                };
                marks.push(round(
                    rect(SW - M - w, y, w, 4.),
                    alpha(p.accent, 0.7 - i as f32 * 0.08),
                    2.,
                ));
            }
        }
    }
}

/// The free shapes the assistant placed, behind the words.
fn shapes(marks: &mut Vec<Mark>, list: &[Shape], p: Palette) {
    for s in list.iter().take(8) {
        let pct = |v: f32, of: f32| v.clamp(-50., 150.) / 100. * of;
        let r = rect(
            pct(s.x, SW),
            pct(s.y, SH),
            pct(s.w, SW).max(2.),
            pct(s.h, SH).max(2.),
        );
        let base = match s.color.as_str() {
            "accent" => p.accent,
            "accent_soft" => p.accent_soft,
            "text" => p.text,
            "muted" => p.muted,
            "surface" => p.surface,
            "title_background" => p.title_background,
            hex => color(hex, p.accent),
        };
        let c = alpha(base, if s.opacity > 0. { s.opacity } else { 1. });
        marks.push(match s.kind {
            ShapeKind::Rect => round(r, c, s.radius.max(0.)),
            ShapeKind::Circle => round(r, c, r.width.min(r.height) / 2.),
            ShapeKind::Ring => Mark::Ring {
                rect: r,
                color: c,
                width: 4.,
            },
            ShapeKind::Line => round(rect(r.x, r.y, r.width, r.height.min(8.)), c, 2.),
        });
    }
}

/// The design of one slide, back to front, and its page colour.
pub fn compose(theme: &DeckTheme, c: &Content) -> (Rgba, Vec<Mark>) {
    let p = theme.palette();
    let d = &theme.design;
    let t = Type { theme };
    let mut marks: Vec<Mark> = Vec::new();
    let dark = c.layout.is_dark();
    let page = if dark {
        p.title_background
    } else {
        p.background
    };
    let ink = if dark { p.title_text } else { p.text };
    let push = |marks: &mut Vec<Mark>, m: Option<Mark>| marks.extend(m);
    // The accent where it shows against the page, else the text colour.
    let hi = if contrast(p.accent, page) < 1.6 {
        ink
    } else {
        p.accent
    };
    let words_only = c.layout.is_display() || c.layout == Layout::Hero;
    let split_pic = words_only && c.art && d.picture == Placement::Split;
    let immersive = matches!(c.layout, Layout::FullBleed) || (c.layout == Layout::Split && c.art);

    // Pictures first.
    if c.layout == Layout::FullBleed && (c.art || c.background) {
        let source = if c.art {
            Picture::Art
        } else {
            Picture::Background
        };
        marks.push(Mark::Picture {
            rect: rect(0., 0., SW, SH),
            source,
            radius: 0.,
        });
    } else if split_pic {
        marks.push(Mark::Picture {
            rect: rect(SW * 0.52, 0., SW * 0.48, SH),
            source: Picture::Art,
            radius: 0.,
        });
    } else if words_only && (c.art || c.background) {
        let source = if c.art {
            Picture::Art
        } else {
            Picture::Background
        };
        marks.push(Mark::Picture {
            rect: rect(0., 0., SW, SH),
            source,
            radius: 0.,
        });
        marks.push(round(rect(0., 0., SW, SH), alpha(page, 0.68), 0.));
    }
    if !split_pic && !immersive {
        let op = Palette { accent: hi, ..p };
        ornament(&mut marks, d, op, ink, words_only);
    }
    if d.stripe && !immersive {
        marks.push(round(rect(0., 0., 24., SH), p.accent, 0.));
    }
    shapes(&mut marks, &c.shapes, p);

    // Title, section, quote and hero slides: where the words sit.
    let centre = d.align == TitleAlign::Center && !split_pic;
    let (tx, tw, al) = if split_pic {
        (M, SW * 0.52 - M - 70., Align::Left)
    } else if centre {
        (M, W, Align::Center)
    } else {
        (M, 1180., Align::Left)
    };
    let bar_x = if centre { SW / 2. - 55. } else { tx };

    match c.layout {
        Layout::Title => {
            push(
                &mut marks,
                t.label(rect(tx, 220., tw, 48.), &c.eyebrow, 26., hi, al),
            );
            push(
                &mut marks,
                t.title(
                    rect(tx, 290., tw, 340.),
                    &c.title,
                    if split_pic { 104. } else { 124. },
                    ink,
                    al,
                    Vertical::Bottom,
                ),
            );
            marks.push(round(rect(bar_x, 664., 110., 8.), hi, 4.));
            push(
                &mut marks,
                t.text(
                    rect(tx, 704., tw, 170.),
                    &c.lead,
                    44.,
                    400.,
                    false,
                    alpha(ink, 0.9),
                    al,
                    Vertical::Top,
                ),
            );
            push(
                &mut marks,
                t.text(
                    rect(tx, 920., tw, 90.),
                    &c.callout,
                    28.,
                    400.,
                    false,
                    alpha(ink, 0.75),
                    al,
                    Vertical::Middle,
                ),
            );
        }
        Layout::Section => {
            push(
                &mut marks,
                t.label(rect(tx, 380., tw, 48.), &c.eyebrow, 26., hi, al),
            );
            push(
                &mut marks,
                t.title(
                    rect(tx, 440., tw, 240.),
                    &c.title,
                    108.,
                    ink,
                    al,
                    Vertical::Middle,
                ),
            );
            marks.push(round(rect(bar_x, 708., 110., 8.), hi, 4.));
            push(
                &mut marks,
                t.text(
                    rect(tx, 748., tw, 120.),
                    &c.lead,
                    38.,
                    400.,
                    false,
                    alpha(ink, 0.85),
                    al,
                    Vertical::Top,
                ),
            );
        }
        Layout::Hero => {
            // One statement, oversized, in open space; off-centre unless
            // the design centres titles.
            let w = if centre || split_pic { tw } else { 1380. };
            push(
                &mut marks,
                t.label(rect(tx, 200., w, 48.), &c.eyebrow, 26., hi, al),
            );
            push(
                &mut marks,
                t.title(
                    rect(tx, 260., w, 500.),
                    &c.title,
                    132.,
                    ink,
                    al,
                    Vertical::Middle,
                ),
            );
            marks.push(round(rect(bar_x, 790., 110., 8.), hi, 4.));
            push(
                &mut marks,
                t.text(
                    rect(tx, 826., w, 140.),
                    &c.lead,
                    36.,
                    400.,
                    false,
                    alpha(ink, 0.85),
                    al,
                    Vertical::Top,
                ),
            );
        }
        Layout::Quote => {
            let quote = if c.lead.trim().is_empty() {
                c.points.first().map(|p| p.text.clone()).unwrap_or_default()
            } else {
                c.lead.clone()
            };
            let qx = if centre { SW / 2. - 80. } else { tx };
            push(
                &mut marks,
                t.text(
                    rect(qx, 110., 300., 260.),
                    "“",
                    280.,
                    800.,
                    true,
                    alpha(hi, 0.9),
                    Align::Left,
                    Vertical::Top,
                ),
            );
            let (bx, bw) = if centre {
                (tx, tw)
            } else {
                (tx + 40., tw - 40.)
            };
            push(
                &mut marks,
                t.text(
                    rect(bx, 300., bw, 440.),
                    &quote,
                    80.,
                    600.,
                    true,
                    ink,
                    al,
                    Vertical::Middle,
                ),
            );
            marks.push(round(
                rect(if centre { bar_x } else { bx }, 776., 110., 8.),
                hi,
                4.,
            ));
            push(
                &mut marks,
                t.label(rect(bx, 806., bw, 50.), &c.title, 26., hi, al),
            );
        }
        Layout::FullBleed => {
            // The words in a calm panel on the side away from the subject.
            let words_left = c.side == Side::Right;
            let pw = SW * 0.46;
            let px = if words_left { 0. } else { SW - pw };
            if c.art || c.background {
                marks.push(round(rect(px, 0., pw, SH), alpha(page, 0.84), 0.));
            }
            let x = px + if words_left { M } else { 90. };
            let w = pw - M - 90.;
            push(
                &mut marks,
                t.label(rect(x, 200., w, 44.), &c.eyebrow, 24., hi, Align::Left),
            );
            push(
                &mut marks,
                t.title(
                    rect(x, 254., w, 300.),
                    &c.title,
                    84.,
                    ink,
                    Align::Left,
                    Vertical::Bottom,
                ),
            );
            marks.push(round(rect(x, 578., 96., 6.), hi, 3.));
            push(
                &mut marks,
                t.text(
                    rect(x, 606., w, 120.),
                    &c.lead,
                    32.,
                    400.,
                    false,
                    alpha(ink, 0.88),
                    Align::Left,
                    Vertical::Top,
                ),
            );
            lead_ins(
                &mut marks,
                &t,
                &c.points,
                x,
                750.,
                w,
                960.,
                ink,
                alpha(ink, 0.75),
                3,
            );
        }
        Layout::Split if c.art => {
            // 60/40: the picture takes `side`, the copy the rest.
            let pw = SW * 0.58;
            let pic_x = if c.side == Side::Right { SW - pw } else { 0. };
            marks.push(Mark::Picture {
                rect: rect(pic_x, 0., pw, SH),
                source: Picture::Art,
                radius: 0.,
            });
            if d.stripe && c.side == Side::Right {
                marks.push(round(rect(0., 0., 24., SH), p.accent, 0.));
            }
            let x = if c.side == Side::Right { M } else { pw + 90. };
            let w = SW - pw - M - 90.;
            push(
                &mut marks,
                t.label(
                    rect(x, 140., w, 44.),
                    &c.eyebrow,
                    24.,
                    p.accent,
                    Align::Left,
                ),
            );
            push(
                &mut marks,
                t.title(
                    rect(x, 194., w, 280.),
                    &c.title,
                    66.,
                    p.text,
                    Align::Left,
                    Vertical::Bottom,
                ),
            );
            marks.push(round(rect(x, 496., 96., 6.), p.accent, 3.));
            push(
                &mut marks,
                t.text(
                    rect(x, 524., w, 120.),
                    &c.lead,
                    30.,
                    400.,
                    false,
                    p.muted,
                    Align::Left,
                    Vertical::Top,
                ),
            );
            lead_ins(
                &mut marks, &t, &c.points, x, 670., w, 980., p.text, p.muted, 3,
            );
        }
        _ => {
            let narrow = c.layout == Layout::ImageText;
            let head_w = if narrow { 840. } else { W };
            push(
                &mut marks,
                t.label(
                    rect(M, 66., head_w, 44.),
                    &c.eyebrow,
                    24.,
                    p.accent,
                    Align::Left,
                ),
            );
            push(
                &mut marks,
                t.title(
                    rect(M, 116., head_w, 132.),
                    &c.title,
                    76.,
                    p.text,
                    Align::Left,
                    Vertical::Middle,
                ),
            );
            marks.push(round(rect(M, 262., 96., 6.), p.accent, 3.));
            push(
                &mut marks,
                t.text(
                    rect(M, 290., head_w, 92.),
                    &c.lead,
                    34.,
                    400.,
                    false,
                    p.muted,
                    Align::Left,
                    Vertical::Middle,
                ),
            );
            let top = if c.lead.trim().is_empty() { 312. } else { 410. };
            let mut bottom = 966.;
            if !c.callout.trim().is_empty() && !narrow {
                marks.push(round(
                    rect(M, bottom - 96., W, 96.),
                    p.accent_soft,
                    d.radius.clamp(0., 40.),
                ));
                push(
                    &mut marks,
                    t.text(
                        rect(M + 40., bottom - 96., W - 80., 96.),
                        &c.callout,
                        32.,
                        800.,
                        true,
                        p.text,
                        Align::Center,
                        Vertical::Middle,
                    ),
                );
                bottom -= 122.;
            }
            body(&mut marks, &t, p, c, top, bottom);
            push(
                &mut marks,
                t.label(
                    rect(M, 1000., 1000., 44.),
                    &c.deck,
                    20.,
                    p.muted,
                    Align::Left,
                ),
            );
            if c.pages > 0 {
                push(
                    &mut marks,
                    t.label(
                        rect(SW - M - 300., 1000., 300., 44.),
                        &format!("{:02} / {:02}", c.page, c.pages),
                        20.,
                        p.muted,
                        Align::Right,
                    ),
                );
            }
        }
    }
    (page, marks)
}

/// Points as bold lead-ins with a line after each, from `top` down to at
/// most `bottom`.
#[allow(clippy::too_many_arguments)]
fn lead_ins(
    marks: &mut Vec<Mark>,
    t: &Type,
    points: &[Point],
    x: f32,
    top: f32,
    w: f32,
    bottom: f32,
    ink: Rgba,
    muted: Rgba,
    most: usize,
) {
    let points: Vec<&Point> = points.iter().take(most).collect();
    if points.is_empty() {
        return;
    }
    let h = ((bottom - top) / points.len() as f32).min(110.);
    for (i, pt) in points.iter().enumerate() {
        let y = top + i as f32 * h;
        marks.extend(t.text(
            rect(x, y, w, 40.),
            &pt.heading,
            28.,
            800.,
            true,
            ink,
            Align::Left,
            Vertical::Top,
        ));
        marks.extend(t.text(
            rect(x, y + 42., w, h - 48.),
            &pt.text,
            24.,
            400.,
            false,
            muted,
            Align::Left,
            Vertical::Top,
        ));
    }
}

/// About how many lines `text` wraps to at `size` in `width`, for
/// placing what follows a heading. Bold display faces run about 0.56 em
/// per character.
/// shortcut: an estimate, not shaped text; a long word can still run a line over.
fn est_height(text: &str, size: f32, width: f32) -> f32 {
    let chars = text.trim().chars().count() as f32;
    if chars == 0. {
        return 0.;
    }
    let per_line = (width / (size * 0.5)).max(1.);
    (chars / per_line).ceil().max(1.) * size * 1.2
}

/// Fits `n` boxes across `width` with gaps.
fn columns(n: usize, width: f32) -> f32 {
    (width - GAP * (n as f32 - 1.)) / n as f32
}

fn body(marks: &mut Vec<Mark>, t: &Type, p: Palette, c: &Content, top: f32, bottom: f32) {
    let d = &t.theme.design;
    let push = |marks: &mut Vec<Mark>, m: Option<Mark>| marks.extend(m);
    let points: Vec<&Point> = c.points.iter().take(7).collect();
    let n = points.len().max(1);
    let avail = bottom - top;
    let badge = |i: usize, label: &str| {
        if label.trim().is_empty() {
            format!("{:02}", i + 1)
        } else {
            label.trim().to_owned()
        }
    };
    let br = d.radius.clamp(0., 24.) * 0.66;
    let figure_of = |pt: &Point| {
        if pt.label.trim().is_empty() {
            pt.value.map(figure).unwrap_or_default()
        } else {
            pt.label.clone()
        }
    };
    match c.layout {
        Layout::Cards => {
            let n = n.min(6);
            let cols = match n {
                1 => 1,
                2 | 4 => 2,
                _ => 3,
            };
            let rows = n.div_ceil(cols);
            let w = columns(cols, W);
            let h = ((avail - GAP * (rows as f32 - 1.)) / rows as f32).min(320.);
            let small = cols == 3;
            for (i, pt) in points.iter().take(n).enumerate() {
                let (x, y) = (
                    M + (i % cols) as f32 * (w + GAP),
                    top + (i / cols) as f32 * (h + GAP),
                );
                panel(marks, rect(x, y, w, h), p, d);
                let badge_bg = if d.cards == CardStyle::Tinted {
                    p.surface
                } else {
                    p.accent_soft
                };
                marks.push(round(rect(x + 36., y + 36., 72., 72.), badge_bg, br));
                push(
                    marks,
                    t.text(
                        rect(x + 36., y + 36., 72., 72.),
                        &badge(i, &pt.label),
                        28.,
                        800.,
                        true,
                        p.text,
                        Align::Center,
                        Vertical::Middle,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(x + 132., y + 36., w - 168., 72.),
                        &pt.heading,
                        if small { 31. } else { 36. },
                        800.,
                        true,
                        p.text,
                        Align::Left,
                        Vertical::Middle,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(x + 132., y + 122., w - 168., h - 146.),
                        &pt.text,
                        if small { 25. } else { 29. },
                        400.,
                        false,
                        p.muted,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
            }
        }
        Layout::Bento => {
            // The first point is the focal block, in the title colours.
            let n = n.min(4);
            let focal_w = if n == 1 { W } else { W * 0.56 };
            let r = rect(M, top, focal_w, avail);
            let radius = d.radius.clamp(0., 48.);
            marks.push(round(r, p.title_background, radius));
            let fink = p.title_text;
            let fhi = if contrast(p.accent, p.title_background) < 1.6 {
                fink
            } else {
                p.accent
            };
            if let Some(pt) = points.first() {
                let fig = figure_of(pt);
                let mut y = top + 48.;
                if !fig.trim().is_empty() {
                    push(
                        marks,
                        t.text(
                            rect(M + 48., y, focal_w - 96., 150.),
                            &fig,
                            112.,
                            800.,
                            true,
                            fhi,
                            Align::Left,
                            Vertical::Middle,
                        ),
                    );
                    y += 170.;
                }
                push(
                    marks,
                    t.text(
                        rect(M + 48., y, focal_w - 96., 130.),
                        &pt.heading,
                        46.,
                        800.,
                        true,
                        fink,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(
                            M + 48.,
                            y + 140.,
                            focal_w - 96.,
                            (top + avail - 40.) - (y + 140.),
                        ),
                        &pt.text,
                        30.,
                        400.,
                        false,
                        alpha(fink, 0.82),
                        Align::Left,
                        Vertical::Bottom,
                    ),
                );
            }
            let rest: Vec<&&Point> = points.iter().skip(1).take(3).collect();
            if !rest.is_empty() {
                let x = M + focal_w + GAP;
                let w = W - focal_w - GAP;
                let h = (avail - GAP * (rest.len() as f32 - 1.)) / rest.len() as f32;
                for (i, pt) in rest.iter().enumerate() {
                    let y = top + i as f32 * (h + GAP);
                    panel(marks, rect(x, y, w, h), p, d);
                    let label = if pt.label.trim().is_empty() {
                        format!("{:02}", i + 2)
                    } else {
                        pt.label.clone()
                    };
                    push(
                        marks,
                        t.label(
                            rect(x + 36., y + 28., w - 72., 34.),
                            &label,
                            20.,
                            p.accent,
                            Align::Left,
                        ),
                    );
                    push(
                        marks,
                        t.text(
                            rect(x + 36., y + 66., w - 72., 46.),
                            &pt.heading,
                            30.,
                            800.,
                            true,
                            p.text,
                            Align::Left,
                            Vertical::Top,
                        ),
                    );
                    push(
                        marks,
                        t.text(
                            rect(x + 36., y + 114., w - 72., (h - 132.).max(30.)),
                            &pt.text,
                            24.,
                            400.,
                            false,
                            p.muted,
                            Align::Left,
                            Vertical::Top,
                        ),
                    );
                }
            }
        }
        Layout::Checklist => {
            let gap = 20.;
            let h = ((avail - gap * (n as f32 - 1.)) / n as f32).min(116.);
            for (i, pt) in points.iter().enumerate() {
                let y = top + i as f32 * (h + gap);
                panel(marks, rect(M, y, W, h), p, d);
                let badge_bg = if d.cards == CardStyle::Tinted {
                    p.surface
                } else {
                    p.accent_soft
                };
                marks.push(round(
                    rect(M + 32., y + (h - 52.) / 2., 52., 52.),
                    badge_bg,
                    br,
                ));
                // The bundled fonts have no check mark; number the rows.
                let mark = if pt.label.trim().is_empty() {
                    format!("{}", i + 1)
                } else {
                    pt.label.trim().to_owned()
                };
                push(
                    marks,
                    t.text(
                        rect(M + 32., y + (h - 52.) / 2., 52., 52.),
                        &mark,
                        28.,
                        800.,
                        true,
                        p.text,
                        Align::Center,
                        Vertical::Middle,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(M + 112., y, W * 0.36, h),
                        &pt.heading,
                        31.,
                        800.,
                        true,
                        p.text,
                        Align::Left,
                        Vertical::Middle,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(M + 112. + W * 0.38, y, W * 0.62 - 150., h),
                        &pt.text,
                        27.,
                        400.,
                        false,
                        p.muted,
                        Align::Left,
                        Vertical::Middle,
                    ),
                );
            }
        }
        Layout::Steps | Layout::Stats => {
            let n = n.min(4);
            let w = columns(n, W);
            let h = avail.min(440.);
            for (i, pt) in points.iter().take(n).enumerate() {
                let x = M + i as f32 * (w + GAP);
                panel(marks, rect(x, top, w, h), p, d);
                if c.layout == Layout::Steps {
                    let label = if pt.label.trim().is_empty() {
                        format!("Step {}", i + 1)
                    } else {
                        pt.label.clone()
                    };
                    push(
                        marks,
                        t.label(
                            rect(x + 40., top + 40., w - 80., 40.),
                            &label,
                            22.,
                            p.accent,
                            Align::Left,
                        ),
                    );
                    push(
                        marks,
                        t.text(
                            rect(x + 40., top + 98., w - 80., 112.),
                            &pt.heading,
                            40.,
                            800.,
                            true,
                            p.text,
                            Align::Left,
                            Vertical::Top,
                        ),
                    );
                    push(
                        marks,
                        t.text(
                            rect(
                                x + 40.,
                                top + 118. + est_height(&pt.heading, 40., w - 80.),
                                w - 80.,
                                (h - 148. - est_height(&pt.heading, 40., w - 80.)).max(40.),
                            ),
                            &pt.text,
                            28.,
                            400.,
                            false,
                            p.muted,
                            Align::Left,
                            Vertical::Top,
                        ),
                    );
                } else {
                    push(
                        marks,
                        t.text(
                            rect(x + 40., top + 40., w - 80., 150.),
                            &figure_of(pt),
                            96.,
                            800.,
                            true,
                            p.accent,
                            Align::Left,
                            Vertical::Middle,
                        ),
                    );
                    push(
                        marks,
                        t.text(
                            rect(x + 40., top + 206., w - 80., 70.),
                            &pt.heading,
                            32.,
                            800.,
                            true,
                            p.text,
                            Align::Left,
                            Vertical::Top,
                        ),
                    );
                    push(
                        marks,
                        t.text(
                            rect(x + 40., top + 282., w - 80., h - 306.),
                            &pt.text,
                            26.,
                            400.,
                            false,
                            p.muted,
                            Align::Left,
                            Vertical::Top,
                        ),
                    );
                }
            }
        }
        Layout::Kpi => {
            // Massive figures, micro-labels, hairline dividers; no boxes.
            let n = n.min(3);
            let top = top + ((avail - 420.) / 2.).max(0.);
            let w = W / n as f32;
            for (i, pt) in points.iter().take(n).enumerate() {
                let x = M + i as f32 * w;
                if i > 0 {
                    marks.push(round(
                        rect(x, top + 20., 2., avail.min(420.) - 40.),
                        alpha(p.text, 0.14),
                        0.,
                    ));
                }
                let inset = if i > 0 { 56. } else { 0. };
                push(
                    marks,
                    t.text(
                        rect(x + inset, top + 10., w - inset - 40., 210.),
                        &figure_of(pt),
                        if n == 1 { 220. } else { 160. },
                        800.,
                        true,
                        p.accent,
                        Align::Left,
                        Vertical::Middle,
                    ),
                );
                push(
                    marks,
                    t.label(
                        rect(x + inset, top + 240., w - inset - 40., 40.),
                        &pt.heading,
                        24.,
                        p.text,
                        Align::Left,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(
                            x + inset,
                            top + 290.,
                            w - inset - 40.,
                            (avail - 300.).max(60.),
                        ),
                        &pt.text,
                        28.,
                        400.,
                        false,
                        p.muted,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
            }
        }
        Layout::Timeline => {
            let n = n.clamp(1, 5);
            let w = W / n as f32;
            // Centred in the space the slide leaves.
            let top = top + ((avail - 360.) / 2.).max(0.);
            let line_y = top + 120.;
            marks.push(round(
                rect(M + w / 2., line_y - 2., W - w, 4.),
                alpha(p.accent, 0.35),
                2.,
            ));
            for (i, pt) in points.iter().take(n).enumerate() {
                let cx = M + w * (i as f32 + 0.5);
                push(
                    marks,
                    t.label(
                        rect(cx - w / 2. + 12., top + 20., w - 24., 40.),
                        &pt.label,
                        20.,
                        p.accent,
                        Align::Center,
                    ),
                );
                marks.push(round(rect(cx - 34., line_y - 34., 68., 68.), p.accent, 34.));
                push(
                    marks,
                    t.text(
                        rect(cx - 34., line_y - 34., 68., 68.),
                        &format!("{}", i + 1),
                        30.,
                        800.,
                        true,
                        p.background,
                        Align::Center,
                        Vertical::Middle,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(cx - w / 2. + 20., line_y + 64., w - 40., 80.),
                        &pt.heading,
                        32.,
                        800.,
                        true,
                        p.text,
                        Align::Center,
                        Vertical::Top,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(
                            cx - w / 2. + 20.,
                            line_y + 80. + est_height(&pt.heading, 32., w - 40.),
                            w - 40.,
                            (bottom - line_y - 160.).max(60.),
                        ),
                        &pt.text,
                        25.,
                        400.,
                        false,
                        p.muted,
                        Align::Center,
                        Vertical::Top,
                    ),
                );
            }
        }
        Layout::Split => {
            // Two contrasting states; the stronger one takes `side`, 60/40.
            let (a, b) = (points.first(), points.get(1));
            let strong_w = W * 0.58 - GAP / 2.;
            let weak_w = W - strong_w - GAP;
            let (weak_x, strong_x) = if c.side == Side::Right {
                (M, M + weak_w + GAP)
            } else {
                (M + strong_w + GAP, M)
            };
            let h = avail.min(440.);
            let radius = d.radius.clamp(0., 48.);
            panel(marks, rect(weak_x, top, weak_w, h), p, d);
            marks.push(round(
                rect(strong_x, top, strong_w, h),
                p.title_background,
                radius,
            ));
            let shi = if contrast(p.accent, p.title_background) < 1.6 {
                p.title_text
            } else {
                p.accent
            };
            for (pt, x, w, ink, muted, accent) in [
                (a, weak_x, weak_w, p.text, p.muted, p.accent),
                (
                    b,
                    strong_x,
                    strong_w,
                    p.title_text,
                    alpha(p.title_text, 0.8),
                    shi,
                ),
            ] {
                let Some(pt) = pt else { continue };
                push(
                    marks,
                    t.label(
                        rect(x + 48., top + 44., w - 96., 40.),
                        &pt.label,
                        22.,
                        accent,
                        Align::Left,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(x + 48., top + 100., w - 96., 140.),
                        &pt.heading,
                        44.,
                        800.,
                        true,
                        ink,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(
                            x + 48.,
                            top + 120. + est_height(&pt.heading, 44., w - 96.),
                            w - 96.,
                            (h - 150. - est_height(&pt.heading, 44., w - 96.)).max(40.),
                        ),
                        &pt.text,
                        28.,
                        400.,
                        false,
                        muted,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
            }
        }
        Layout::Panoramic => {
            if c.art {
                marks.push(Mark::Picture {
                    rect: rect(M, top, W, avail),
                    source: Picture::Art,
                    radius: d.radius.clamp(0., 48.),
                });
            } else {
                // A wide row of columns split by hairlines.
                let n = n.min(5);
                let w = W / n as f32;
                marks.push(round(rect(M, top, W, 3.), alpha(p.text, 0.16), 0.));
                for (i, pt) in points.iter().take(n).enumerate() {
                    let x = M + i as f32 * w;
                    if i > 0 {
                        marks.push(round(
                            rect(x, top + 40., 2., (avail - 60.).min(340.)),
                            alpha(p.text, 0.12),
                            0.,
                        ));
                    }
                    let inset = if i > 0 { 36. } else { 0. };
                    push(
                        marks,
                        t.label(
                            rect(x + inset, top + 40., w - inset - 24., 36.),
                            &badge(i, &pt.label),
                            20.,
                            p.accent,
                            Align::Left,
                        ),
                    );
                    push(
                        marks,
                        t.text(
                            rect(x + inset, top + 90., w - inset - 24., 110.),
                            &pt.heading,
                            32.,
                            800.,
                            true,
                            p.text,
                            Align::Left,
                            Vertical::Top,
                        ),
                    );
                    push(
                        marks,
                        t.text(
                            rect(
                                x + inset,
                                top + 106. + est_height(&pt.heading, 32., w - inset - 24.),
                                w - inset - 24.,
                                (avail - 130. - est_height(&pt.heading, 32., w - inset - 24.))
                                    .max(60.),
                            ),
                            &pt.text,
                            25.,
                            400.,
                            false,
                            p.muted,
                            Align::Left,
                            Vertical::Top,
                        ),
                    );
                }
            }
        }
        Layout::Editorial => {
            let n = n.min(5);
            let h = (avail / n as f32).min(140.);
            marks.push(round(rect(M, top, 6., h * n as f32 - 24.), p.accent, 3.));
            for (i, pt) in points.iter().take(n).enumerate() {
                let y = top + i as f32 * h;
                push(
                    marks,
                    t.text(
                        rect(M + 56., y, W * 0.8, 48.),
                        &pt.heading,
                        36.,
                        800.,
                        true,
                        p.text,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(M + 56., y + 52., W * 0.8, h - 64.),
                        &pt.text,
                        28.,
                        400.,
                        false,
                        p.muted,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
            }
        }
        Layout::Chart => {
            let max = points
                .iter()
                .filter_map(|p| p.value)
                .fold(0., f64::max)
                .max(f64::EPSILON);
            let row = (avail / n as f32).min(104.);
            let label_w = 520.;
            let track_x = M + label_w + 20.;
            let track_w = W - label_w - 20. - 170.;
            for (i, pt) in points.iter().enumerate() {
                let y = top + i as f32 * row;
                let bar_h = (row * 0.5).min(52.);
                let by = y + (row - bar_h) / 2.;
                let r = (bar_h / 2.).min(d.radius.max(2.));
                push(
                    marks,
                    t.text(
                        rect(M, y, label_w, row),
                        &pt.heading,
                        30.,
                        800.,
                        true,
                        p.text,
                        Align::Left,
                        Vertical::Middle,
                    ),
                );
                marks.push(round(rect(track_x, by, track_w, bar_h), p.accent_soft, r));
                let share = (pt.value.unwrap_or(0.) / max).clamp(0., 1.) as f32;
                marks.push(round(
                    rect(track_x, by, (track_w * share).max(bar_h), bar_h),
                    p.accent,
                    r,
                ));
                push(
                    marks,
                    t.text(
                        rect(track_x + track_w + 24., y, 146., row),
                        &figure_of(pt),
                        30.,
                        800.,
                        true,
                        p.text,
                        Align::Left,
                        Vertical::Middle,
                    ),
                );
            }
        }
        Layout::ImageText | Layout::Bullets => {
            let w = if c.layout == Layout::ImageText {
                820.
            } else {
                W
            };
            let gap = 18.;
            let h = ((avail - gap * (n as f32 - 1.)) / n as f32).min(150.);
            for (i, pt) in points.iter().enumerate() {
                let y = top + i as f32 * (h + gap);
                marks.push(round(rect(M, y + 14., 14., 14.), p.accent, 7.));
                let has_text = !pt.text.trim().is_empty();
                push(
                    marks,
                    t.text(
                        rect(M + 36., y, w - 36., if has_text { 46. } else { h }),
                        &pt.heading,
                        31.,
                        if has_text { 800. } else { 500. },
                        has_text,
                        p.text,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
                push(
                    marks,
                    t.text(
                        rect(M + 36., y + 50., w - 36., h - 50.),
                        &pt.text,
                        26.,
                        400.,
                        false,
                        p.muted,
                        Align::Left,
                        Vertical::Top,
                    ),
                );
            }
            if c.layout == Layout::ImageText {
                marks.push(Mark::Picture {
                    rect: ART_RECT,
                    source: Picture::Art,
                    radius: d.radius.clamp(0., 48.),
                });
            }
        }
        Layout::Title | Layout::Section | Layout::Quote | Layout::Hero | Layout::FullBleed => {}
    }
}

/// A chart or figure value as people write it: whole numbers without
/// decimals.
fn figure(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

// ─── Drawing ────────────────────────────────────────────────────────────────

fn skia(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn rounded(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let radius = radius.min(r.width / 2.).min(r.height / 2.).max(0.);
    if radius <= 0. {
        return Some(PathBuilder::from_rect(tiny_skia::Rect::from_xywh(
            r.x, r.y, r.width, r.height,
        )?));
    }
    let (x, y, w, h, k) = (r.x, r.y, r.width, r.height, radius * 0.5523);
    let mut pb = PathBuilder::new();
    pb.move_to(x + radius, y);
    pb.line_to(x + w - radius, y);
    pb.cubic_to(
        x + w - radius + k,
        y,
        x + w,
        y + radius - k,
        x + w,
        y + radius,
    );
    pb.line_to(x + w, y + h - radius);
    pb.cubic_to(
        x + w,
        y + h - radius + k,
        x + w - radius + k,
        y + h,
        x + w - radius,
        y + h,
    );
    pb.line_to(x + radius, y + h);
    pb.cubic_to(
        x + radius - k,
        y + h,
        x,
        y + h - radius + k,
        x,
        y + h - radius,
    );
    pb.line_to(x, y + radius);
    pb.cubic_to(x, y + radius - k, x + radius - k, y, x + radius, y);
    pb.close();
    pb.finish()
}

fn decode(bytes: &[u8]) -> Result<image::RgbaImage, ImagineError> {
    image::load_from_memory(bytes)
        .map(|i| i.to_rgba8())
        .map_err(|_| ImagineError::Api("an image couldn't be read".into()))
}

/// Scales `img` to cover `w` x `h`, cropping the middle.
fn cover(img: &image::RgbaImage, w: u32, h: u32) -> image::RgbaImage {
    let scale = (w as f32 / img.width() as f32).max(h as f32 / img.height() as f32);
    let (sw, sh) = (
        ((img.width() as f32 * scale).ceil() as u32).max(w),
        ((img.height() as f32 * scale).ceil() as u32).max(h),
    );
    let scaled = imageops::resize(img, sw, sh, imageops::FilterType::Triangle);
    imageops::crop_imm(&scaled, (sw - w) / 2, (sh - h) / 2, w, h).to_image()
}

fn pixmap_of(img: image::RgbaImage) -> Option<Pixmap> {
    let (w, h) = img.dimensions();
    let mut data = img.into_raw();
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3] as u16;
        for c in &mut px[..3] {
            *c = ((*c as u16 * a + 127) / 255) as u8;
        }
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h)?)
}

fn text_box(f: &Frame) -> TextBox {
    TextBox {
        kind: BoxKind::Verse,
        rect: f.rect,
        padding: 0.,
        vertical: f.vertical,
        backdrop: None,
        size: Fit::Auto { cap: f.size },
        style: TextStyle {
            family: stack(&f.font),
            weight: f.weight,
            line_height: f.line_height,
            tabular: false,
            letter_spacing: f.letter_spacing,
            align: f.align,
            decoration: Decoration::None,
            color: f.color,
            shadow: None,
            stroke: None,
        },
        paragraphs: f
            .paragraphs
            .iter()
            .map(|text| Paragraph {
                text: text.clone(),
                color: None,
            })
            .collect(),
    }
}

/// The CSS font stack for a font name.
fn stack(font: &str) -> String {
    kairo_scene::FONT_STACKS
        .iter()
        .find(|(name, _)| *name == font)
        .map(|(_, stack)| stack.to_string())
        .unwrap_or_else(|| format!("'{font}', 'Helvetica Neue', 'Source Sans 3', sans-serif"))
}

/// A picture cropped to fill `r`, as PNG.
pub fn picture_png(bytes: &[u8], r: Rect) -> Result<Vec<u8>, ImagineError> {
    let img = cover(
        &decode(bytes)?,
        r.width.round() as u32,
        r.height.round() as u32,
    );
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|_| ImagineError::Storage("a picture couldn't be saved".into()))?;
    Ok(out)
}

/// Draws one slide as a 1920x1080 PNG.
pub fn render(
    theme: &DeckTheme,
    content: &Content,
    background: Option<&[u8]>,
    art: Option<&[u8]>,
) -> Result<Vec<u8>, ImagineError> {
    let (page, marks) = compose(theme, content);
    let fail = || ImagineError::Storage("the slide couldn't be drawn".into());
    let mut canvas = Pixmap::new(WIDTH, HEIGHT).ok_or_else(fail)?;
    canvas.fill(skia(page));
    let mut texts = Vec::new();
    for mark in &marks {
        match mark {
            Mark::Rect {
                rect: r,
                fill,
                radius,
            } => {
                let mut paint = Paint::default();
                paint.set_color(skia(*fill));
                paint.anti_alias = true;
                if let Some(path) = rounded(*r, *radius) {
                    canvas.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            Mark::Outline {
                rect: r,
                color,
                width,
                radius,
            } => {
                let mut paint = Paint::default();
                paint.set_color(skia(*color));
                paint.anti_alias = true;
                let stroke = Stroke {
                    width: *width,
                    ..Stroke::default()
                };
                if let Some(path) = rounded(*r, *radius) {
                    canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            Mark::Ring {
                rect: r,
                color,
                width,
            } => {
                let mut paint = Paint::default();
                paint.set_color(skia(*color));
                paint.anti_alias = true;
                let stroke = Stroke {
                    width: *width,
                    ..Stroke::default()
                };
                if let Some(path) =
                    PathBuilder::from_circle(r.x + r.width / 2., r.y + r.height / 2., r.width / 2.)
                {
                    canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            Mark::Picture {
                rect: r,
                source,
                radius,
            } => {
                let bytes = match source {
                    Picture::Background => background,
                    Picture::Art => art,
                };
                let Some(bytes) = bytes else { continue };
                let (w, h) = (r.width.round() as u32, r.height.round() as u32);
                let Some(pic) = pixmap_of(cover(&decode(bytes)?, w, h)) else {
                    continue;
                };
                let mask = if *radius > 0. {
                    let mut mask = Mask::new(WIDTH, HEIGHT).ok_or_else(fail)?;
                    if let Some(path) = rounded(*r, *radius) {
                        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
                    }
                    Some(mask)
                } else {
                    None
                };
                canvas.draw_pixmap(
                    r.x.round() as i32,
                    r.y.round() as i32,
                    pic.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    mask.as_ref(),
                );
            }
            Mark::Text(f) => texts.push(text_box(f)),
        }
    }
    let scene = SlideScene {
        frame: (WIDTH, HEIGHT),
        boxes: texts,
    };
    if let Some(text) = rasterize(&scene)
        && let Some(size) = IntSize::from_wh(text.width, text.height)
        && let Some(layer) = Pixmap::from_vec(text.pixels, size)
    {
        canvas.draw_pixmap(
            text.x as i32,
            text.y as i32,
            layer.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    canvas.encode_png().map_err(|_| fail())
}

/// The art prompt for the shared background: never any lettering.
pub fn background_prompt(theme: &DeckTheme, prompt: &str) -> String {
    format!(
        "{} Style: {}. A wide 16:9 background with calm, open space. No text, letters, numbers, words or logos anywhere.",
        prompt.trim(),
        theme.style.trim()
    )
}

/// The art prompt for one slide's picture, framed for where it sits, in
/// the deck's locked art direction.
pub fn art_prompt(theme: &DeckTheme, prompt: &str, layout: Layout, side: Side) -> String {
    let space = match side {
        Side::Right => "left",
        Side::Left => "right",
    };
    let framing = match layout {
        Layout::FullBleed => format!(
            "A wide panoramic composition with the subject toward the {} and calm, clean negative space across the {space} half for text.",
            match side {
                Side::Right => "right",
                Side::Left => "left",
            }
        ),
        Layout::Split => "One isolated subject, centred, on a clean, simple backdrop; it must still read when cropped to a tall frame.".into(),
        Layout::Panoramic => "A very wide cinematic panorama with detail spread horizontally; it must read when cropped to a wide strip.".into(),
        Layout::ImageText => "A clear focal subject on a simple background; it must read when cropped close to square.".into(),
        _ => "An abstract, atmospheric texture or quiet scene with a large open area and little detail where words will sit.".into(),
    };
    format!(
        "{} {framing} Art direction for every picture in this deck: {}. Original, concrete visual metaphor; no stock-photo clichés. No text, letters, numbers, words, labels, interfaces or logos anywhere.",
        prompt.trim(),
        theme.style.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(w, h, image::Rgba([200, 40, 40, 255]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn content(layout: Layout) -> Content {
        Content {
            layout,
            eyebrow: "Where AI can help".into(),
            title: "Sermon & Teaching Preparation".into(),
            lead: "Use AI to support preparation, not to replace study.".into(),
            points: (1..=4)
                .map(|i| Point {
                    label: String::new(),
                    heading: format!("Point {i}"),
                    text: "A short sentence about it.".into(),
                    value: Some(i as f64 * 10.),
                })
                .collect(),
            callout: "The goal is more time for people.".into(),
            page: 2,
            pages: 5,
            deck: "AI Tools for the Church".into(),
            background: true,
            art: false,
            side: Side::Right,
            shapes: Vec::new(),
        }
    }

    #[test]
    fn every_layout_is_designed_inside_the_slide_and_draws() {
        let theme = DeckTheme::default();
        for layout in Layout::ALL {
            let c = content(layout);
            let (_, marks) = compose(&theme, &c);
            assert!(
                marks.iter().any(|m| matches!(m, Mark::Text(_))),
                "{layout:?} has words"
            );
            for m in &marks {
                let r = match m {
                    Mark::Rect { rect, .. }
                    | Mark::Picture { rect, .. }
                    | Mark::Text(Frame { rect, .. }) => *rect,
                    Mark::Ring { .. } | Mark::Outline { .. } => continue,
                };
                assert!(
                    r.x >= 0.
                        && r.y >= 0.
                        && r.x + r.width <= WIDTH as f32 + 0.5
                        && r.y + r.height <= HEIGHT as f32 + 0.5,
                    "{layout:?} {r:?}"
                );
            }
            let bytes = render(&theme, &c, Some(&png(64, 36)), Some(&png(40, 40))).unwrap();
            let img = image::load_from_memory(&bytes).unwrap();
            assert_eq!((img.width(), img.height()), (WIDTH, HEIGHT));
        }
    }

    #[test]
    fn every_design_direction_draws_differently() {
        let mut seen = std::collections::HashSet::new();
        let looks = [
            (
                Ornament::Rings,
                CardStyle::Soft,
                TitleAlign::Left,
                Placement::Background,
            ),
            (
                Ornament::Blobs,
                CardStyle::Outline,
                TitleAlign::Center,
                Placement::Split,
            ),
            (
                Ornament::Dots,
                CardStyle::AccentTop,
                TitleAlign::Left,
                Placement::Background,
            ),
            (
                Ornament::Arcs,
                CardStyle::Tinted,
                TitleAlign::Center,
                Placement::Background,
            ),
            (
                Ornament::Band,
                CardStyle::Plain,
                TitleAlign::Left,
                Placement::Background,
            ),
            (
                Ornament::Frame,
                CardStyle::Soft,
                TitleAlign::Center,
                Placement::Background,
            ),
            (
                Ornament::Lines,
                CardStyle::Outline,
                TitleAlign::Left,
                Placement::Background,
            ),
            (
                Ornament::None,
                CardStyle::Plain,
                TitleAlign::Left,
                Placement::Background,
            ),
        ];
        for (ornament, cards, align, picture) in looks {
            let theme = DeckTheme {
                design: Design {
                    ornament,
                    cards,
                    align,
                    picture,
                    radius: 8.,
                    stripe: false,
                    uppercase_titles: true,
                },
                ..DeckTheme::default()
            };
            for layout in [Layout::Title, Layout::Cards] {
                let mut c = content(layout);
                c.art = true;
                c.shapes = vec![Shape {
                    kind: ShapeKind::Circle,
                    x: 80.,
                    y: 70.,
                    w: 10.,
                    h: 10.,
                    color: "accent".into(),
                    opacity: 0.3,
                    radius: 0.,
                }];
                let bytes = render(&theme, &c, Some(&png(64, 36)), Some(&png(40, 40))).unwrap();
                assert!(
                    seen.insert(bytes),
                    "{ornament:?} {cards:?} {layout:?} looks new"
                );
            }
        }
    }

    #[test]
    fn cards_are_cards_and_charts_are_bars() {
        let theme = DeckTheme::default();
        let (_, cards) = compose(&theme, &content(Layout::Cards));
        let panels = cards
            .iter()
            .filter(|m| matches!(m, Mark::Rect { radius, .. } if *radius >= 20.))
            .count();
        assert!(panels >= 4, "one panel per card");
        let (_, chart) = compose(&theme, &content(Layout::Chart));
        let bars = chart
            .iter()
            .filter(|m| matches!(m, Mark::Rect { fill, .. } if *fill == theme.palette().accent))
            .count();
        assert!(bars >= 4);
        assert_eq!(figure(40.), "40");
        assert_eq!(figure(2.5), "2.5");
    }

    #[test]
    fn older_themes_still_have_every_colour() {
        let old: DeckTheme = serde_json::from_str(
            r##"{"background":"#1E2430","text":"#F5F3EE","accent":"#E8B04A","heading_font":"Georgia","body_font":"Source Sans 3","style":"calm"}"##,
        )
        .unwrap();
        let p = old.palette();
        assert_ne!(p.surface, p.background);
        assert!(background_prompt(&old, "hills").contains("No text"));
    }
}
