//! Fonts, shaping and glyph outlines, on parley and swash.
//!
//! One [`Typesetter`] per thread owns the font collection (system fonts plus
//! the bundled Source Sans 3), the shaping context and the outline scaler.
//! Nothing here is shared between threads, so the worker pool and the UI
//! process use the same code without locks.

use crate::color::Rgba;
use crate::glyphs::GlyphCache;
use crate::scene::{Align, Paragraph, TextBox};
use parley::fontique::{Blob, FontInfoOverride};
use parley::style::{FontFamily, FontFamilyName};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontWeight, GenericFamily, LayoutContext, LineHeight,
    OverflowWrap, PositionedLayoutItem, StyleProperty,
};
use std::cell::RefCell;
use std::sync::Arc;
use swash::scale::ScaleContext;

/// `FONT_STACKS` in `ThemeTypePanel.tsx`: the fonts a theme can pick.
/// Older themes may hold other CSS stacks, which resolve the same way.
pub const FONT_STACKS: [(&str, &str); 8] = [
    ("Helvetica Neue", "'Helvetica Neue', Arial, sans-serif"),
    ("Inter", "Inter, system-ui, sans-serif"),
    ("Georgia", "Georgia, 'Times New Roman', serif"),
    ("Times New Roman", "'Times New Roman', Times, serif"),
    ("Palatino", "Palatino, 'Palatino Linotype', serif"),
    ("Avenir Next", "'Avenir Next', Avenir, sans-serif"),
    ("Futura", "Futura, 'Trebuchet MS', sans-serif"),
    ("Courier", "'Courier New', Courier, monospace"),
];

/// The bundled Source Sans 3 (variable, SIL OFL 1.1), as the renderer's
/// `@fontsource-variable/source-sans-3` ships it: one file per script
/// subset. Each is registered under its own family name, because a family
/// is searched by weight, not by coverage, so a glyph missing from the Latin
/// subset is found by falling to the next family in the chain.
const BUNDLED: [(&str, &[u8]); 5] = [
    (
        "Source Sans 3",
        include_bytes!("../fonts/SourceSans3-latin.ttf"),
    ),
    (
        "Source Sans 3 Latin Extended",
        include_bytes!("../fonts/SourceSans3-latin-ext.ttf"),
    ),
    (
        "Source Sans 3 Vietnamese",
        include_bytes!("../fonts/SourceSans3-vietnamese.ttf"),
    ),
    (
        "Source Sans 3 Greek",
        include_bytes!("../fonts/SourceSans3-greek.ttf"),
    ),
    (
        "Source Sans 3 Cyrillic",
        include_bytes!("../fonts/SourceSans3-cyrillic.ttf"),
    ),
];

/// The family names of the bundled fonts, in fallback order.
pub fn bundled_families() -> impl Iterator<Item = &'static str> {
    BUNDLED.iter().map(|(name, _)| *name)
}

/// Whether the bundled Source Sans 3 has a glyph for `c` in any subset.
pub fn bundled_font_covers(c: char) -> bool {
    BUNDLED.iter().any(|(_, data)| {
        swash::FontRef::from_index(data, 0).is_some_and(|f| f.charmap().map(c) != 0)
    })
}

/// The families to try for a theme's CSS `font-family`: its own list in
/// order, then the bundled Source Sans 3 subsets (so Yoruba dots below and
/// tone marks always have a face), then the system sans-serif. A list CSS
/// cannot parse is a dropped declaration, which leaves the browser default,
/// a serif.
pub fn family_chain(css: &str) -> Vec<FontFamilyName<'static>> {
    let parsed: Result<Vec<FontFamilyName<'static>>, _> = FontFamilyName::parse_css_list(css)
        .map(|f| f.map(FontFamilyName::into_owned))
        .collect();
    let mut chain = match parsed {
        Ok(list) if !list.is_empty() => list,
        _ => vec![FontFamilyName::Generic(GenericFamily::Serif)],
    };
    chain.extend(bundled_families().map(|n| FontFamilyName::Named(n.to_owned().into())));
    chain.push(FontFamilyName::Generic(GenericFamily::SansSerif));
    chain
}

/// Per-line decoration geometry, from the line's first font.
#[derive(Clone, Copy, Debug, Default)]
pub struct DecorationMetrics {
    /// The font's ascent at the line's size. Chromium puts a line-through a
    /// third of it above the baseline.
    pub ascent: f32,
}

/// A font at one size and variation, shared by the glyphs of a run.
#[derive(Clone)]
pub struct RunFont {
    pub data: Blob<u8>,
    pub index: u32,
    pub size: f32,
    pub coords: Vec<i16>,
    pub coords_hash: u64,
}

/// One glyph of a line, positioned in box-content coordinates (origin at the
/// content box's top left, before vertical alignment), at its baseline.
#[derive(Clone, Copy)]
pub struct PlacedGlyph {
    pub run: u32,
    pub id: u16,
    pub x: f32,
    pub y: f32,
}

pub struct ShapedLine {
    /// The line's text, trimmed, for tests and diagnostics.
    pub text: String,
    pub color: Option<Rgba>,
    pub runs: Vec<RunFont>,
    pub glyphs: Vec<PlacedGlyph>,
    pub x: f32,
    pub baseline: f32,
    pub advance: f32,
    pub decoration: DecorationMetrics,
}

#[derive(Default)]
pub struct Shaped {
    pub height: f32,
    pub width: f32,
    pub lines: Vec<ShapedLine>,
}

pub struct Typesetter {
    fonts: FontContext,
    layout: LayoutContext<()>,
    pub(crate) scale: ScaleContext,
    pub(crate) glyph_cache: GlyphCache,
}

impl Typesetter {
    pub fn new() -> Self {
        let mut fonts = FontContext::new();
        for (name, data) in BUNDLED {
            let blob = Blob::new(Arc::new(data));
            fonts.collection.register_fonts(
                blob,
                Some(FontInfoOverride {
                    family_name: Some(name),
                    ..FontInfoOverride::default()
                }),
            );
        }
        Self {
            fonts,
            layout: LayoutContext::new(),
            scale: ScaleContext::new(),
            glyph_cache: GlyphCache::default(),
        }
    }

    /// Shape every paragraph of `b` at `size` into `width`, with glyph
    /// outlines when `outlines`. Blank paragraphs advance one line.
    pub fn shape(&mut self, b: &TextBox, size: f32, width: f32, glyphs: bool) -> Shaped {
        let mut out = Shaped::default();
        let style = &b.style;
        let line_height = style.line_height * size;
        let chain = family_chain(&style.family);
        for paragraph in &b.paragraphs {
            if paragraph.text.is_empty() {
                out.height += line_height;
                continue;
            }
            self.shape_paragraph(
                b,
                paragraph,
                &chain,
                size,
                line_height,
                width,
                glyphs,
                &mut out,
            );
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_paragraph(
        &mut self,
        b: &TextBox,
        paragraph: &Paragraph,
        chain: &[FontFamilyName<'static>],
        size: f32,
        line_height: f32,
        width: f32,
        glyphs: bool,
        out: &mut Shaped,
    ) {
        let style = &b.style;
        // The page's base direction is left to right whatever the text is;
        // parley would otherwise take it from the first strong character.
        // A left-to-right mark in front pins it and shapes to nothing.
        let force_ltr = paragraph.text.chars().any(is_rtl);
        let prefix = if force_ltr { "\u{200E}" } else { "" };
        let text = format!("{prefix}{}", paragraph.text);
        let mut builder = self
            .layout
            .ranged_builder(&mut self.fonts, &text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::List(
            chain.to_vec().into(),
        )));
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight)));
        builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(line_height)));
        builder.push_default(StyleProperty::LetterSpacing(style.letter_spacing));
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::BreakWord));
        if style.letter_spacing != 0.0 {
            // Cursive scripts join their letters, so letter-spacing is off
            // for them (CSS Text 3); the browser skips the whole run.
            for range in cursive_ranges(&text) {
                builder.push(StyleProperty::LetterSpacing(0.0), range);
            }
            // The direction mark takes no spacing either.
            if !prefix.is_empty() {
                builder.push(StyleProperty::LetterSpacing(0.0), 0..prefix.len());
            }
        }
        let mut layout = builder.build(&text);
        layout.break_all_lines(Some(width));
        layout.align(
            match style.align {
                Align::Left => Alignment::Left,
                Align::Right => Alignment::Right,
                Align::Justify => Alignment::Justify,
                Align::Center => Alignment::Center,
            },
            AlignmentOptions {
                align_when_overflowing: true,
            },
        );
        let top = out.height;
        for line in layout.lines() {
            let range = line.text_range();
            let start = range.start.saturating_sub(prefix.len());
            let end = range.end.saturating_sub(prefix.len());
            let line_text = paragraph.text.get(start..end).unwrap_or("").trim();
            let metrics = line.metrics();
            let mut shaped = ShapedLine {
                text: line_text.to_owned(),
                color: paragraph.color,
                runs: Vec::new(),
                glyphs: Vec::new(),
                x: metrics.offset,
                baseline: top + metrics.baseline,
                advance: metrics.advance - metrics.trailing_whitespace,
                decoration: DecorationMetrics::default(),
            };
            if glyphs {
                for item in line.items() {
                    let PositionedLayoutItem::GlyphRun(run) = item else {
                        continue;
                    };
                    let font = run.run().font();
                    let index = shaped.runs.len() as u32;
                    if shaped.runs.is_empty()
                        && let Some(font_ref) =
                            swash::FontRef::from_index(font.data.as_ref(), font.index as usize)
                    {
                        let m = font_ref
                            .metrics(run.run().normalized_coords())
                            .scale(run.run().font_size());
                        shaped.decoration = DecorationMetrics { ascent: m.ascent };
                    }
                    let coords = run.run().normalized_coords().to_vec();
                    let mut hasher = std::hash::DefaultHasher::new();
                    std::hash::Hash::hash(&coords, &mut hasher);
                    shaped.runs.push(RunFont {
                        data: font.data.clone(),
                        index: font.index,
                        size: run.run().font_size(),
                        coords_hash: std::hash::Hasher::finish(&hasher),
                        coords,
                    });
                    shaped
                        .glyphs
                        .extend(run.positioned_glyphs().map(|g| PlacedGlyph {
                            run: index,
                            id: g.id as u16,
                            x: g.x,
                            y: top + g.y,
                        }));
                }
            }
            out.lines.push(shaped);
        }
        out.height += layout.height();
        out.width = out.width.max(layout.width());
    }
}

impl Default for Typesetter {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static TYPESETTER: RefCell<Option<Typesetter>> = const { RefCell::new(None) };
}

/// Run `f` with this thread's typesetter, made on first use.
pub fn with_typesetter<R>(f: impl FnOnce(&mut Typesetter) -> R) -> R {
    TYPESETTER.with(|cell| {
        let mut slot = cell.borrow_mut();
        f(slot.get_or_insert_with(Typesetter::new))
    })
}

fn is_cursive(c: char) -> bool {
    matches!(c as u32,
        0x0600..=0x06FF | 0x0700..=0x074F | 0x0750..=0x077F | 0x07C0..=0x07FF
        | 0x08A0..=0x08FF | 0x1800..=0x18AF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Byte ranges of runs in a cursive script. Spaces, punctuation and marks
/// belong to the script of the letters around them, as in the browser's
/// script itemisation.
fn cursive_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    // `pending` holds the start of neutral characters seen since the last
    // cursive letter; they join the run only if a cursive letter follows
    // (or the run is still open at the end).
    let mut open: Option<usize> = None;
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        if is_cursive(c) {
            match (open, ranges.last_mut()) {
                (Some(_), Some(last)) => last.end = end,
                _ => {
                    ranges.push(i..end);
                    open = Some(i);
                }
            }
        } else if c.is_alphanumeric() {
            open = None;
        } else if let (Some(_), Some(last)) = (open, ranges.last_mut()) {
            // Neutral: extend the open run; a later letter of another
            // script closes it (below) without removing what it covered.
            last.end = end;
        }
    }
    ranges
}

/// Right-to-left scripts: Hebrew, Arabic, Syriac, Thaana, N'Ko, their
/// presentation forms and the supplementary right-to-left planes.
fn is_rtl(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF
        | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}
