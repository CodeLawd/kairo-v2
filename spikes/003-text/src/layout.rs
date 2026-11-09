use crate::Layer;
use anyhow::{Context, Result};
use cosmic_text as cosmic;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, LayoutContext, LineHeight,
    OverflowWrap, PositionedLayoutItem, StyleProperty,
};
use swash::scale::{ScaleContext, Scaler};
use swash::zeno::Verb;
use tiny_skia::{Path, PathBuilder, Rect};

#[derive(Default)]
pub struct Layout {
    pub height: f32,
    pub width: f32,
    pub lines: Vec<String>,
    pub paths: Vec<(Path, [u8; 4])>,
}

pub enum Engine {
    Parley {
        fonts: Box<FontContext>,
        context: Box<LayoutContext<()>>,
    },
    Cosmic(Box<cosmic::FontSystem>),
}

impl Engine {
    pub fn parley() -> Self {
        Self::Parley {
            fonts: Box::new(FontContext::new()),
            context: Box::new(LayoutContext::new()),
        }
    }

    pub fn cosmic() -> Self {
        Self::Cosmic(Box::new(cosmic::FontSystem::new()))
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Parley { .. } => "parley",
            Self::Cosmic(_) => "cosmic-text",
        }
    }

    pub fn shape(
        &mut self,
        layer: &Layer,
        text: &str,
        size: f32,
        width: f32,
        outlines: Option<&mut ScaleContext>,
    ) -> Result<Layout> {
        let mut output = Layout::default();
        let mut outlines = outlines;
        let line_height = layer.line_height * size;
        for (index, paragraph) in text.split('\n').enumerate() {
            let paragraph = paragraph.trim();
            if paragraph.is_empty() {
                output.height += line_height;
                continue;
            }
            let color = layer.line_color(index);
            match self {
                Self::Parley { fonts, context } => {
                    let mut styled = context.ranged_builder(fonts, paragraph, 1.0, true);
                    styled.push_default(StyleProperty::FontFamily(FontFamily::from(
                        layer.family.as_str(),
                    )));
                    styled.push_default(StyleProperty::FontSize(size));
                    styled.push_default(StyleProperty::FontWeight(FontWeight::new(f32::from(
                        layer.weight,
                    ))));
                    styled
                        .push_default(StyleProperty::LineHeight(LineHeight::Absolute(line_height)));
                    styled.push_default(StyleProperty::LetterSpacing(layer.letter_spacing));
                    styled.push_default(StyleProperty::OverflowWrap(OverflowWrap::BreakWord));
                    let mut layout = styled.build(paragraph);
                    layout.break_all_lines(Some(width));
                    layout.align(
                        match layer.align.as_str() {
                            "left" => Alignment::Left,
                            "right" => Alignment::Right,
                            "justify" => Alignment::Justify,
                            _ => Alignment::Center,
                        },
                        AlignmentOptions {
                            align_when_overflowing: true,
                        },
                    );
                    for line in layout.lines() {
                        output.push_line(&paragraph[line.text_range()]);
                        if let Some(scaler) = outlines.as_deref_mut() {
                            let mut builder = PathBuilder::new();
                            for item in line.items() {
                                let PositionedLayoutItem::GlyphRun(run) = item else {
                                    continue;
                                };
                                let font = run.run().font();
                                let font_ref = swash::FontRef::from_index(
                                    font.data.as_ref(),
                                    font.index as usize,
                                )
                                .context("Parley selected a font Swash could not open")?;
                                let mut scaler = scaler
                                    .builder(font_ref)
                                    .size(run.run().font_size())
                                    .normalized_coords(run.run().normalized_coords())
                                    .build();
                                for glyph in run.positioned_glyphs() {
                                    append_outline(
                                        &mut builder,
                                        &mut scaler,
                                        glyph.id as u16,
                                        glyph.x,
                                        output.height + glyph.y,
                                    );
                                }
                            }
                            let metrics = line.metrics();
                            decorate(
                                &mut builder,
                                layer,
                                size,
                                metrics.offset,
                                output.height + metrics.baseline,
                                metrics.advance,
                            );
                            if let Some(path) = builder.finish() {
                                output.paths.push((path, color));
                            }
                        }
                    }
                    output.height += layout.height();
                    output.width = output.width.max(layout.width());
                }
                Self::Cosmic(fonts) => {
                    let font_family = layer
                        .family
                        .split(',')
                        .map(|name| name.trim().trim_matches(['"', '\'']))
                        .find(|name| {
                            fonts
                                .db()
                                .faces()
                                .any(|face| face.families.iter().any(|(family, _)| family == name))
                        });
                    let family =
                        font_family.map_or(cosmic::Family::SansSerif, cosmic::Family::Name);
                    let requested = cosmic::Weight(layer.weight);
                    // Match the nearest face within the CSS family first.
                    // Passing an absent weight directly to Cosmic can pick
                    // an exact weight from an unrelated fallback family.
                    let weight = fonts
                        .db()
                        .query(&cosmic::fontdb::Query {
                            families: &[family],
                            weight: requested,
                            ..Default::default()
                        })
                        .and_then(|id| fonts.db().face(id))
                        .map_or(requested, |face| face.weight);
                    let attrs = cosmic::Attrs::new()
                        .family(family)
                        .weight(weight)
                        .letter_spacing(layer.letter_spacing / size);
                    let alignment = match layer.align.as_str() {
                        "left" => cosmic::Align::Left,
                        "right" => cosmic::Align::Right,
                        "justify" => cosmic::Align::Justified,
                        _ => cosmic::Align::Center,
                    };
                    let mut buffer =
                        cosmic::Buffer::new(fonts, cosmic::Metrics::new(size, line_height));
                    buffer.set_wrap(cosmic::Wrap::WordOrGlyph);
                    buffer.set_size(Some(width), None);
                    buffer.set_text(
                        paragraph,
                        &attrs,
                        cosmic::Shaping::Advanced,
                        Some(alignment),
                    );
                    buffer.shape_until_scroll(fonts, false);
                    let mut height = 0.0;
                    for run in buffer.layout_runs() {
                        if let (Some(start), Some(end)) = (
                            run.glyphs.iter().map(|glyph| glyph.start).min(),
                            run.glyphs.iter().map(|glyph| glyph.end).max(),
                        ) {
                            output.push_line(&run.text[start..end]);
                        }
                        if let Some(scaler) = outlines.as_deref_mut() {
                            let mut builder = PathBuilder::new();
                            for glyph in run.glyphs {
                                let font = fonts
                                    .get_font(glyph.font_id, glyph.font_weight)
                                    .context("Cosmic Text selected an unavailable font")?;
                                let font_ref = font.as_swash();
                                let coords = font_ref.variations().normalized_coords([(
                                    swash::Tag::from_be_bytes(*b"wght"),
                                    f32::from(glyph.font_weight.0),
                                )]);
                                let mut scaler = scaler
                                    .builder(font_ref)
                                    .size(glyph.font_size)
                                    .normalized_coords(coords)
                                    .build();
                                append_outline(
                                    &mut builder,
                                    &mut scaler,
                                    glyph.glyph_id,
                                    glyph.x + glyph.x_offset * glyph.font_size,
                                    output.height + run.line_y + glyph.y
                                        - glyph.y_offset * glyph.font_size,
                                );
                            }
                            let x = run
                                .glyphs
                                .iter()
                                .map(|glyph| glyph.x)
                                .reduce(f32::min)
                                .unwrap_or(0.0);
                            decorate(
                                &mut builder,
                                layer,
                                size,
                                x,
                                output.height + run.line_y,
                                run.line_w,
                            );
                            if let Some(path) = builder.finish() {
                                output.paths.push((path, color));
                            }
                        }
                        height = run.line_top + run.line_height;
                        output.width = output.width.max(run.line_w);
                    }
                    output.height += height;
                }
            }
        }
        Ok(output)
    }
}

impl Layout {
    fn push_line(&mut self, text: &str) {
        let text = text.trim();
        if !text.is_empty() {
            self.lines.push(text.to_owned());
        }
    }
}

fn append_outline(builder: &mut PathBuilder, scaler: &mut Scaler<'_>, glyph: u16, x: f32, y: f32) {
    let Some(outline) = scaler.scale_outline(glyph) else {
        return;
    };
    let mut points = outline.points().iter().map(|p| (p.x + x, y - p.y));
    for verb in outline.verbs() {
        match verb {
            Verb::MoveTo => {
                let (x, y) = points.next().expect("move point");
                builder.move_to(x, y);
            }
            Verb::LineTo => {
                let (x, y) = points.next().expect("line point");
                builder.line_to(x, y);
            }
            Verb::QuadTo => {
                let (cx, cy) = points.next().expect("quadratic control");
                let (x, y) = points.next().expect("quadratic end");
                builder.quad_to(cx, cy, x, y);
            }
            Verb::CurveTo => {
                let (x1, y1) = points.next().expect("cubic control");
                let (x2, y2) = points.next().expect("cubic control");
                let (x, y) = points.next().expect("cubic end");
                builder.cubic_to(x1, y1, x2, y2, x, y);
            }
            Verb::Close => builder.close(),
        }
    }
}

fn decorate(
    builder: &mut PathBuilder,
    layer: &Layer,
    size: f32,
    x: f32,
    baseline: f32,
    width: f32,
) {
    let y = match layer.decoration.as_str() {
        "underline" => baseline + size * 0.08,
        "line-through" => baseline - size * 0.3,
        _ => return,
    };
    if let Some(rect) = Rect::from_xywh(x, y, width, (size * 0.055).max(1.0)) {
        builder.push_rect(rect);
    }
}
