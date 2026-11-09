use crate::{HEIGHT, Layer, WIDTH, layout::Layout};
use anyhow::{Context, Result};
use std::time::Instant;
use tiny_skia::{Color, FillRule, Mask, Paint, Pixmap, Stroke, Transform};

/// Milliseconds spent in each raster phase of one layer.
#[derive(Default)]
pub struct Phases {
    pub shadow_ms: f64,
    pub stroke_ms: f64,
    pub fill_ms: f64,
}

fn since(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

pub fn canvas() -> Result<Pixmap> {
    let mut image = Pixmap::new(WIDTH, HEIGHT).context("allocate 1080p canvas")?;
    image.fill(Color::from_rgba8(128, 128, 128, 255));
    Ok(image)
}

pub fn draw(canvas: &mut Pixmap, layer: &Layer, layout: &Layout) -> Result<Phases> {
    let mut phases = Phases::default();
    if layout.paths.is_empty() {
        return Ok(phases);
    }
    let x = layer.bounds.x.floor();
    let y = layer.bounds.y.floor();
    let width = (layer.bounds.x + layer.bounds.width).ceil() - x;
    let height = (layer.bounds.y + layer.bounds.height).ceil() - y;
    let mut box_image = Pixmap::new(width as u32, height as u32).context("allocate text box")?;
    let available = layer.bounds.height - 2.0 * layer.padding - layout.height;
    // A fixed-size verse overflows from the top padding: `.pa-verse` grows with
    // its content. The reference is a direct flex item, so Chromium keeps
    // centering it when it overflows its short box.
    let available = if layer.auto_fit || layer.kind != "verse" {
        available
    } else {
        available.max(0.0)
    };
    let alignment = match layer.vertical.as_str() {
        "flex-start" => 0.0,
        "flex-end" => available,
        _ => available / 2.0,
    };
    let origin = (
        layer.bounds.x - x + layer.padding,
        layer.bounds.y - y + layer.padding + alignment,
    );
    let placement = Transform::from_translate(origin.0, origin.1);
    let started = Instant::now();
    if layer.shadow.enabled {
        draw_shadow(&mut box_image, layer, layout, origin)?;
    }
    phases.shadow_ms = since(started);
    let started = Instant::now();
    if layer.stroke_width > 0.0 {
        let stroke = Stroke {
            width: layer.stroke_width,
            ..Stroke::default()
        };
        let paint = paint(layer.stroke);
        for (path, _) in &layout.paths {
            box_image.stroke_path(path, &paint, &stroke, placement, None);
        }
    }
    phases.stroke_ms = since(started);
    let started = Instant::now();
    for (path, color) in &layout.paths {
        box_image.fill_path(path, &paint(*color), FillRule::Winding, placement, None);
    }
    composite_on_canvas(canvas, &box_image, x as i32, y as i32);
    phases.fill_ms = since(started);
    Ok(phases)
}

fn composite_on_canvas(target: &mut Pixmap, source: &Pixmap, x: i32, y: i32) {
    let target_width = target.width() as usize;
    let source_width = source.width() as usize;
    let left = x.max(0);
    let top = y.max(0);
    let right = (x + source.width() as i32).min(target.width() as i32);
    let bottom = (y + source.height() as i32).min(target.height() as i32);
    if right <= left || bottom <= top {
        return;
    }
    let width = (right - left) as usize;
    for row in top..bottom {
        let source_start = ((row - y) as usize * source_width + (left - x) as usize) * 4;
        let target_start = (row as usize * target_width + left as usize) * 4;
        for (src, dst) in source.data()[source_start..source_start + width * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .zip(
                target.data_mut()[target_start..target_start + width * 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut(),
            )
        {
            if src[3] == 0 {
                continue;
            }
            let inverse = u16::from(255 - src[3]);
            for channel in 0..3 {
                dst[channel] =
                    src[channel] + ((u16::from(dst[channel]) * inverse + 127) / 255) as u8;
            }
        }
    }
}

fn paint([r, g, b, a]: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    paint
}

fn draw_shadow(
    target: &mut Pixmap,
    layer: &Layer,
    layout: &Layout,
    (x, y): (f32, f32),
) -> Result<()> {
    let shadow = &layer.shadow;
    let sigma = shadow.blur_px / 2.0;
    let margin = (sigma * 3.0).ceil() + layer.stroke_width + 2.0;
    let mut left = f32::INFINITY;
    let mut top = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    for (path, _) in &layout.paths {
        let bounds = path.bounds();
        left = left.min(bounds.left());
        top = top.min(bounds.top());
        right = right.max(bounds.right());
        bottom = bottom.max(bounds.bottom());
    }
    let x = x + shadow.x_px;
    let y = y + shadow.y_px;
    let left = (left + x - margin).floor().max(0.0);
    let top = (top + y - margin).floor().max(0.0);
    let right = (right + x + margin).ceil().min(target.width() as f32);
    let bottom = (bottom + y + margin).ceil().min(target.height() as f32);
    if right <= left || bottom <= top {
        return Ok(());
    }
    let scale = if sigma >= 2.0 { 0.5 } else { 1.0 };
    let width = ((right - left) * scale).ceil() as u32;
    let height = ((bottom - top) * scale).ceil() as u32;
    let mut mask = Mask::new(width, height).context("allocate shadow mask")?;
    for (path, _) in &layout.paths {
        mask.fill_path(
            path,
            FillRule::Winding,
            true,
            Transform::from_scale(scale, scale)
                .post_translate((x - left) * scale, (y - top) * scale),
        );
    }
    let mut blurred = mask
        .data()
        .iter()
        .map(|&value| f32::from(value))
        .collect::<Vec<_>>();
    if sigma > 0.0 {
        gaussian_boxes(&mut blurred, width as usize, height as usize, sigma * scale);
    }
    let [r, g, b, _] = crate::hex_rgba(&shadow.color).context("invalid shadow color")?;
    let columns = (0..(right - left) as usize)
        .map(|column| sample_at(column, width as usize, scale))
        .collect::<Vec<_>>();
    let stride = target.width() as usize * 4;
    let target_pixels = target.data_mut();
    for row in 0..(bottom - top) as usize {
        let (top_sample, bottom_sample, y_fraction) = sample_at(row, height as usize, scale);
        let top_row = &blurred[top_sample * width as usize..][..width as usize];
        let bottom_row = &blurred[bottom_sample * width as usize..][..width as usize];
        let start = (top as usize + row) * stride + left as usize * 4;
        for (&(lo, hi, x_fraction), pixel) in columns
            .iter()
            .zip(target_pixels[start..].as_chunks_mut::<4>().0.iter_mut())
        {
            let above = top_row[lo] + (top_row[hi] - top_row[lo]) * x_fraction;
            let below = bottom_row[lo] + (bottom_row[hi] - bottom_row[lo]) * x_fraction;
            let value = above + (below - above) * y_fraction;
            let alpha = (value * shadow.opacity).round().clamp(0.0, 255.0) as u8;
            if alpha != 0 {
                let premultiply =
                    |channel: u8| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8;
                pixel.copy_from_slice(&[premultiply(r), premultiply(g), premultiply(b), alpha]);
            }
        }
    }
    Ok(())
}

fn sample_at(position: usize, size: usize, scale: f32) -> (usize, usize, f32) {
    let coordinate = ((position as f32 + 0.5) * scale - 0.5).clamp(0.0, (size - 1) as f32);
    let low = coordinate.floor() as usize;
    (low, (low + 1).min(size - 1), coordinate - low as f32)
}

// Three separable box filters approximate a Gaussian with the requested sigma.
// Running sums keep the cost linear in the clipped shadow area.
fn gaussian_boxes(data: &mut [f32], width: usize, height: usize, sigma: f32) {
    let ideal = (4.0 * sigma * sigma + 1.0).sqrt();
    let lower = (ideal.floor() as usize).max(1) | 1;
    let lower = if lower as f32 > ideal {
        lower.saturating_sub(2).max(1)
    } else {
        lower
    };
    let upper = lower + 2;
    let low = lower as f32;
    let small = ((12.0 * sigma * sigma - 3.0 * low * low - 12.0 * low - 9.0) / (-4.0 * low - 4.0))
        .round()
        .clamp(0.0, 3.0) as usize;
    let mut scratch = vec![0.0; data.len()];
    for pass in 0..3 {
        let radius = if pass < small { lower / 2 } else { upper / 2 };
        blur_axis(data, &mut scratch, height, width, width, 1, radius);
        blur_axis(&scratch, data, width, height, 1, width, radius);
    }
}

fn blur_axis(
    input: &[f32],
    output: &mut [f32],
    lines: usize,
    length: usize,
    line_stride: usize,
    step: usize,
    radius: usize,
) {
    let divisor = (radius * 2 + 1) as f32;
    for line in 0..lines {
        let start = line * line_stride;
        let mut sum: f32 = (0..=radius.min(length - 1))
            .map(|i| input[start + i * step])
            .sum();
        for i in 0..length {
            output[start + i * step] = sum / divisor;
            if i >= radius {
                sum -= input[start + (i - radius) * step];
            }
            if i + radius + 1 < length {
                sum += input[start + (i + radius + 1) * step];
            }
        }
    }
}
