mod layout;
mod raster;

use anyhow::{Context, Result, ensure};
use layout::Engine;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, time::Instant};
use swash::scale::ScaleContext;
use tiny_skia::Pixmap;
use unicode_segmentation::UnicodeSegmentation;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

#[derive(Deserialize)]
struct Case {
    id: String,
    layers: Vec<Layer>,
}

#[derive(Deserialize)]
struct Bounds {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Deserialize)]
struct ReferenceLine {
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    kind: String,
    #[serde(rename = "box")]
    bounds: Bounds,
    padding: f32,
    source: String,
    transform: String,
    family: String,
    weight: u16,
    font_size: f32,
    line_height: f32,
    letter_spacing: f32,
    align: String,
    vertical: String,
    decoration: String,
    fill: [u8; 4],
    stroke: [u8; 4],
    stroke_width: f32,
    auto_fit: bool,
    lines: Vec<ReferenceLine>,
    shadow: Shadow,
    line_colors: Vec<Option<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Shadow {
    enabled: bool,
    x_px: f32,
    y_px: f32,
    blur_px: f32,
    color: String,
    opacity: f32,
}

impl Layer {
    fn styled_text(&self) -> String {
        let text = self
            .source
            .split('\n')
            .map(|line| {
                line.split([' ', '\t', '\r', '\u{c}'])
                    .filter(|word| !word.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n");
        match self.transform.as_str() {
            "uppercase" => text.to_uppercase(),
            "lowercase" => text.to_lowercase(),
            "capitalize" => text
                .split_word_bounds()
                .map(|word| {
                    if let Some((index, first)) =
                        word.char_indices().find(|(_, c)| c.is_alphabetic())
                    {
                        format!(
                            "{}{}{}",
                            &word[..index],
                            first.to_uppercase(),
                            &word[index + first.len_utf8()..]
                        )
                    } else {
                        word.to_owned()
                    }
                })
                .collect(),
            _ => text,
        }
    }

    fn line_color(&self, index: usize) -> [u8; 4] {
        self.line_colors
            .get(index)
            .and_then(Option::as_deref)
            .and_then(hex_rgba)
            .unwrap_or(self.fill)
    }
}

fn hex_rgba(hex: &str) -> Option<[u8; 4]> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let color = u32::from_str_radix(hex, 16).ok()?;
    Some([(color >> 16) as u8, (color >> 8) as u8, color as u8, 255])
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LayerResult {
    kind: String,
    exact: bool,
    fitted: f32,
    reference: f32,
    auto_fit: bool,
    expected: Vec<String>,
    actual: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultRow {
    engine: &'static str,
    id: String,
    layout_ms: f64,
    raster_ms: f64,
    shadow_ms: f64,
    stroke_ms: f64,
    fill_ms: f64,
    ssim: f64,
    occupied_ssim: f64,
    layers: Vec<LayerResult>,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: kairo-spike-text CAPTURE_DIRECTORY RESULTS_DIRECTORY"
    );
    let capture = Path::new(&args[0]);
    let output = Path::new(&args[1]);
    let cases: Vec<Case> = serde_json::from_slice(&fs::read(capture.join("layout.json"))?)?;
    ensure!(cases.len() >= 300, "capture is missing cases");
    fs::create_dir_all(output)?;
    let mut results = Vec::new();
    for mut engine in [Engine::parley(), Engine::cosmic()] {
        let mut scaler = ScaleContext::new();
        let engine_output = output.join(engine.name());
        fs::create_dir_all(&engine_output)?;
        for case in &cases {
            let chromium = Pixmap::load_png(capture.join("text").join(format!("{}.png", case.id)))
                .with_context(|| format!("open Chromium PNG for {}", case.id))?;
            ensure!(
                chromium.width() == WIDTH && chromium.height() == HEIGHT,
                "unexpected PNG dimensions"
            );
            let started = Instant::now();
            let mut image = raster::canvas()?;
            let mut layers = Vec::new();
            let mut layout_ms = 0.0;
            let (mut shadow_ms, mut stroke_ms, mut fill_ms) = (0.0, 0.0, 0.0);
            let mut raster_ms = started.elapsed().as_secs_f64() * 1000.0;
            for layer in &case.layers {
                let started = Instant::now();
                let width = (layer.bounds.width - 2.0 * layer.padding).max(1.0);
                let height = (layer.bounds.height - 2.0 * layer.padding).max(1.0);
                let text = layer.styled_text();
                let size = fit(&mut engine, layer, &text, width, height)?;
                let layout = engine.shape(layer, &text, size, width, Some(&mut scaler))?;
                layout_ms += started.elapsed().as_secs_f64() * 1000.0;
                let started = Instant::now();
                let phases = raster::draw(&mut image, layer, &layout)?;
                shadow_ms += phases.shadow_ms;
                stroke_ms += phases.stroke_ms;
                fill_ms += phases.fill_ms;
                raster_ms += started.elapsed().as_secs_f64() * 1000.0;
                let expected: Vec<_> = layer.lines.iter().map(|line| line.text.clone()).collect();
                let exact = expected
                    .iter()
                    .map(|line| canonical(line))
                    .eq(layout.lines.iter().map(|line| canonical(line)));
                layers.push(LayerResult {
                    kind: layer.kind.clone(),
                    exact,
                    fitted: size,
                    reference: layer.font_size,
                    auto_fit: layer.auto_fit,
                    expected,
                    actual: layout.lines,
                });
            }
            let (ssim, occupied_ssim) = ssim(&chromium, &image);
            if [
                "-kjv-original-fit",
                "-yoruba-original-fit",
                "-rtl-original-fit",
                "-long-outline-fit",
                "-kjv-right-fit",
            ]
            .iter()
            .any(|suffix| case.id.ends_with(suffix))
            {
                image.save_png(engine_output.join(format!("{}.png", case.id)))?;
            }
            results.push(ResultRow {
                engine: engine.name(),
                id: case.id.clone(),
                layout_ms,
                raster_ms,
                shadow_ms,
                stroke_ms,
                fill_ms,
                ssim,
                occupied_ssim,
                layers,
            });
        }
    }
    fs::write(output.join("results.json"), serde_json::to_vec(&results)?)?;
    summarize(&results);
    Ok(())
}

fn summarize(results: &[ResultRow]) {
    for engine in ["parley", "cosmic-text"] {
        let rows: Vec<_> = results.iter().filter(|row| row.engine == engine).collect();
        let matched = rows
            .iter()
            .filter(|row| row.layers.iter().all(|layer| layer.exact))
            .count();
        let fitted: Vec<_> = rows
            .iter()
            .flat_map(|row| &row.layers)
            .filter(|layer| layer.auto_fit)
            .collect();
        let fitted_matched = fitted
            .iter()
            .filter(|layer| (layer.fitted - layer.reference).abs() <= 1.0)
            .count();
        let summary = |measure: fn(&ResultRow) -> f64| {
            let mut values = rows.iter().map(|row| measure(row)).collect::<Vec<_>>();
            values.sort_by(f64::total_cmp);
            let len = values.len();
            (
                values.iter().sum::<f64>() / len as f64,
                values[0],
                values[len / 2],
                values[(len * 95).div_ceil(100) - 1],
            )
        };
        let (ssim_mean, ssim_min, _, _) = summary(|row| row.ssim);
        let (occupied_mean, occupied_min, _, _) = summary(|row| row.occupied_ssim);
        let (_, _, raster_median, raster_p95) = summary(|row| row.raster_ms);
        let (_, _, _, total_p95) = summary(|row| row.layout_ms + row.raster_ms);
        println!(
            "{engine}: exact={matched}/{} fit={fitted_matched}/{} ssim_mean={ssim_mean:.5} ssim_worst={ssim_min:.5} occupied_mean={occupied_mean:.5} occupied_worst={occupied_min:.5} raster_median_ms={raster_median:.3} raster_p95_ms={raster_p95:.3} total_p95_ms={total_p95:.3}",
            rows.len(),
            fitted.len()
        );
    }
}

fn fit(engine: &mut Engine, layer: &Layer, text: &str, width: f32, height: f32) -> Result<f32> {
    if !layer.auto_fit {
        return Ok(layer.font_size);
    }
    let mut low = 12;
    const SMART_FILL_REFERENCE: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";
    let mut high = estimate_cap(layer, &layer.source, width, height).min(estimate_cap(
        layer,
        SMART_FILL_REFERENCE,
        width,
        height,
    ));
    while low < high {
        let middle = (low + high).div_ceil(2);
        let layout = engine.shape(layer, text, middle as f32, width, None)?;
        if layout.height.round() <= height.round() + 1.0
            && layout.width.round() <= width.round() + 1.0
        {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    Ok(low as f32)
}

fn estimate_cap(layer: &Layer, text: &str, width: f32, height: f32) -> u32 {
    let paragraphs: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if paragraphs.is_empty() {
        return (layer.font_size as u32).clamp(12, 240);
    }
    let longest_word = paragraphs
        .iter()
        .flat_map(|line| line.split_whitespace())
        .map(|word| word.encode_utf16().count())
        .max()
        .unwrap_or(1);
    let paragraph_lengths: Vec<_> = paragraphs
        .iter()
        .map(|line| line.encode_utf16().count())
        .collect();
    let width = f64::from(width.max(80.0));
    let height = f64::from(height.max(48.0));
    let fits = |size: u32| {
        let character = f64::from(size) * 0.55 + f64::from(layer.letter_spacing.abs()) * 0.35;
        let per_line = ((width / character).floor() as usize).max(1);
        let lines = paragraph_lengths
            .iter()
            .map(|length| length.div_ceil(per_line))
            .sum::<usize>();
        longest_word as f64 * character <= width
            && lines as f64 * f64::from(size) * f64::from(layer.line_height) <= height
    };
    let mut low = 12_u32;
    let mut high = 240;
    while low < high {
        let middle = (low + high).div_ceil(2);
        if fits(middle) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

fn canonical(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn ssim(left: &Pixmap, right: &Pixmap) -> (f64, f64) {
    let mut all = 0.0;
    let mut active = 0.0;
    let mut blocks = 0;
    let mut active_blocks = 0;
    let row = WIDTH as usize * 4;
    let first = left.data();
    let second = right.data();
    for y in (0..HEIGHT as usize).step_by(8) {
        for x in (0..WIDTH as usize).step_by(8) {
            let mut a = 0.0;
            let mut b = 0.0;
            let mut aa = 0.0;
            let mut bb = 0.0;
            let mut ab = 0.0;
            let mut occupied = false;
            for yy in y..y + 8 {
                for xx in x..x + 8 {
                    let i = yy * row + xx * 4;
                    let l = &first[i..i + 3];
                    let r = &second[i..i + 3];
                    occupied |= l != [128, 128, 128] || r != [128, 128, 128];
                    let l = luminance(l);
                    let r = luminance(r);
                    a += l;
                    b += r;
                    aa += l * l;
                    bb += r * r;
                    ab += l * r;
                }
            }
            let ma = a / 64.0;
            let mb = b / 64.0;
            let va = (aa - a * ma) / 63.0;
            let vb = (bb - b * mb) / 63.0;
            let cov = (ab - a * mb) / 63.0;
            let c1 = 6.5025;
            let c2 = 58.5225;
            let value = ((2.0 * ma * mb + c1) * (2.0 * cov + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            all += value;
            blocks += 1;
            if occupied {
                active += value;
                active_blocks += 1;
            }
        }
    }
    (
        all / f64::from(blocks),
        if active_blocks == 0 {
            1.0
        } else {
            active / f64::from(active_blocks)
        },
    )
}

fn luminance(rgb: &[u8]) -> f64 {
    0.2126 * f64::from(rgb[0]) + 0.7152 * f64::from(rgb[1]) + 0.0722 * f64::from(rgb[2])
}
