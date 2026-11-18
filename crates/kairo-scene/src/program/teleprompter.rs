//! The teleprompter viewport. Layout is independent of the playback clock.

use super::{SANS, Space, TextLook, text_box};
use crate::color::Rgba;
use crate::raster::draw_scroll_text;
use crate::scene::{Rect, Vertical};
use crate::typeset::{ShapedLine, Typesetter};
use crate::vector::Tile;

pub struct ScriptLayout {
    lines: Vec<ShapedLine>,
    script_len: usize,
    viewport: (u32, u32),
    margin: f32,
    line_height: f32,
}

impl ScriptLayout {
    pub fn new(ts: &mut Typesetter, text: &str, viewport: (u32, u32), font_px: f32) -> Self {
        let font_px = if font_px.is_finite() {
            font_px.clamp(12.0, 240.0)
        } else {
            64.0
        };
        let line_height = font_px * 1.25;
        let margin = viewport.0 as f32 * 0.08;
        let width = (viewport.0 as f32 - margin * 2.0).max(1.0);
        let look = TextLook::new(SANS, font_px, 500.0, 1.25, Rgba::new(255, 255, 255, 255));
        let mut offset = 0;
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let display = if paragraph.trim().is_empty() {
                " "
            } else {
                paragraph
            };
            let b = text_box(
                &look,
                display,
                Space::Pre,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height: line_height,
                },
                0.0,
                Vertical::Top,
            );
            let top = lines.len() as f32 * line_height;
            let shaped = ts.shape(&b, font_px, width, true);
            for mut line in shaped.lines {
                line.byte_start += offset;
                line.baseline += top;
                for glyph in &mut line.glyphs {
                    glyph.y += top;
                }
                lines.push(line);
            }
            offset += paragraph.len() + 1;
        }
        Self {
            lines,
            script_len: text.len(),
            viewport,
            margin,
            line_height,
        }
    }

    pub fn end(&self) -> f64 {
        self.lines.len().saturating_sub(1) as f64
    }

    pub fn position_for_anchor(&self, anchor: usize) -> f64 {
        self.lines
            .partition_point(|line| line.byte_start <= anchor)
            .saturating_sub(1) as f64
    }

    pub fn anchor_at(&self, position: f64) -> usize {
        self.lines
            .get(position.clamp(0.0, self.end()).floor() as usize)
            .map_or(self.script_len, |line| line.byte_start)
    }

    pub fn draw(&self, ts: &mut Typesetter, position: f64) -> Option<Tile> {
        let position = position.clamp(0.0, self.end()) as f32;
        let guide = self.viewport.1 as f32 * 0.32;
        let scroll = position * self.line_height - guide;
        // Include overhangs from the lines immediately outside the viewport.
        let start = ((scroll / self.line_height).floor() as isize - 2).max(0) as usize;
        let end = (((scroll + self.viewport.1 as f32) / self.line_height).ceil() as usize + 2)
            .min(self.lines.len());
        let visible = self.lines.get(start..end).unwrap_or(&[]);
        let image = draw_scroll_text(ts, visible, (self.margin, -scroll), self.viewport, guide)?;
        Some(Tile {
            x: 0,
            y: 0,
            width: image.width(),
            height: image.height(),
            pixels: image.take(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portrait_viewport_wraps_long_script_and_preserves_unicode_anchors_on_reflow() {
        let mut ts = Typesetter::new();
        let paragraph = "Hello world, this is a stage reading. Olá ẹ̀. ".repeat(10);
        let script = format!("{paragraph}\n\n").repeat(125);
        let frame = (360, 640);
        let layout = ScriptLayout::new(&mut ts, &script, frame, 24.0);
        assert!(layout.end() > 1000.0);
        let anchor = layout.anchor_at(800.0);
        assert!(script.is_char_boundary(anchor));
        let reflow = ScriptLayout::new(&mut ts, &script, frame, 36.0);
        let position = reflow.position_for_anchor(anchor);
        assert!(reflow.anchor_at(position) <= anchor);
        assert!(reflow.anchor_at(position + 1.0) > anchor);
        let tile = reflow.draw(&mut ts, position).unwrap();
        assert_eq!((tile.width, tile.height), frame);
        assert!(tile.pixels.as_chunks::<4>().0.contains(&[255; 4]));
    }
}
