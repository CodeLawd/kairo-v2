//! The join card's QR code. Electron calls `qrcode.toDataURL(url, { width:
//! 384, margin: 2, errorCorrectionLevel: 'M' })`; this draws the same grid
//! with the same scaling (`qrcode/lib/renderer/utils.js`, `qrToImageData`)
//! into an RGBA PNG, black on white.

use crate::{AudienceError, Result, lock};
use base64::Engine;
use qrcode::bits::Bits;
use qrcode::canvas::{Canvas, MaskPattern};
use qrcode::{Color, EcLevel, Version};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const QR_WIDTH: usize = 384;
pub const QR_MARGIN: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Numeric,
    Alphanumeric,
    Byte,
}

impl Mode {
    /// `Mode.getCharCountIndicator`.
    fn count_bits(self, version: i16) -> usize {
        let band = match version {
            1..=9 => 0,
            10..=26 => 1,
            _ => 2,
        };
        match self {
            Self::Numeric => [10, 12, 14][band],
            Self::Alphanumeric => [9, 11, 13][band],
            Self::Byte => [8, 16, 16][band],
        }
    }

    /// `getSegmentBitsLength` (`NumericData`, `AlphanumericData`, `ByteData`).
    fn data_bits(self, length: usize) -> usize {
        match self {
            Self::Numeric => {
                10 * (length / 3)
                    + if !length.is_multiple_of(3) {
                        (length % 3) * 3 + 1
                    } else {
                        0
                    }
            }
            Self::Alphanumeric => 11 * (length / 2) + 6 * (length % 2),
            Self::Byte => length * 8,
        }
    }
}

/// `{ data, mode, length }`; `length` is UTF-16 units, or UTF-8 bytes for byte mode.
#[derive(Clone, Debug)]
struct Segment {
    data: String,
    mode: Mode,
    length: usize,
}

fn class(c: char) -> Mode {
    match c {
        '0'..='9' => Mode::Numeric,
        'A'..='Z' | ' ' | '$' | '%' | '*' | '+' | '-' | '.' | '/' | ':' => Mode::Alphanumeric,
        _ => Mode::Byte,
    }
}

/// `getSegmentsFromString` with Kanji mode off: maximal runs of each class.
fn raw_split(text: &str) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    for c in text.chars() {
        let mode = class(c);
        match segments.last_mut() {
            Some(last) if last.mode == mode => {
                last.data.push(c);
                last.length += c.len_utf16();
            }
            _ => segments.push(Segment {
                data: c.to_string(),
                mode,
                length: c.len_utf16(),
            }),
        }
    }
    segments
}

/// `Data.getLength()`: characters, or bytes for byte mode.
fn data_length(segment: &Segment) -> usize {
    match segment.mode {
        Mode::Byte => segment.data.len(),
        _ => segment.data.encode_utf16().count(),
    }
}

/// Total data bits for `version` and level M (`Mode.MIXED` capacity).
fn capacity_bits(version: i16) -> usize {
    qrcode::bits::Bits::new(Version::Normal(version))
        .max_len(EcLevel::M)
        .unwrap_or(0)
}

/// `Version.getBestVersionForData`.
fn best_version(segments: &[Segment]) -> Option<i16> {
    match segments {
        [] => Some(1),
        [one] => (1..=40).find(|&v| {
            let usable = capacity_bits(v).saturating_sub(one.mode.count_bits(v) + 4);
            let capacity = match one.mode {
                Mode::Numeric => (usable as f64 / 10.0 * 3.0).floor() as usize,
                Mode::Alphanumeric => (usable as f64 / 11.0 * 2.0).floor() as usize,
                Mode::Byte => usable / 8,
            };
            data_length(one) <= capacity
        }),
        many => (1..=40).find(|&v| {
            let bits: usize = many
                .iter()
                .map(|s| s.mode.count_bits(v) + 4 + s.mode.data_bits(data_length(s)))
                .sum();
            bits <= capacity_bits(v)
        }),
    }
}

/// `Segments.fromString`: the cheapest mix of modes, found exactly as
/// `buildGraph` and `dijkstrajs` find it, ties and quirks included.
fn optimal_segments(text: &str, version: i16) -> Vec<Segment> {
    let raw = raw_split(text);
    // `buildNodes`.
    let groups: Vec<Vec<Segment>> = raw
        .iter()
        .map(|seg| {
            let byte = Segment {
                data: seg.data.clone(),
                mode: Mode::Byte,
                length: seg.data.len(),
            };
            match seg.mode {
                Mode::Numeric => vec![
                    seg.clone(),
                    Segment {
                        mode: Mode::Alphanumeric,
                        ..seg.clone()
                    },
                    byte,
                ],
                Mode::Alphanumeric => vec![seg.clone(), byte],
                Mode::Byte => vec![byte],
            }
        })
        .collect();
    // `buildGraph`. Node 0 is `start`, the last is `end`.
    let mut nodes: Vec<Segment> = vec![Segment {
        data: String::new(),
        mode: Mode::Byte,
        length: 0,
    }];
    let mut last_count: Vec<usize> = vec![0];
    let mut edges: Vec<Vec<(usize, usize)>> = vec![Vec::new()];
    let mut previous = vec![0usize];
    for group in &groups {
        let mut current = Vec::new();
        for node in group {
            let key = nodes.len();
            nodes.push(node.clone());
            last_count.push(0);
            edges.push(Vec::new());
            current.push(key);
            for &prev in &previous {
                // `start` has no table entry, so it always pays the switch cost.
                let cost = if prev != 0 && nodes[prev].mode == node.mode {
                    let cost = node.mode.data_bits(last_count[prev] + node.length)
                        - node.mode.data_bits(last_count[prev]);
                    last_count[prev] += node.length;
                    cost
                } else {
                    if prev != 0 {
                        last_count[prev] = node.length;
                    }
                    node.mode.data_bits(node.length) + 4 + node.mode.count_bits(version)
                };
                edges[prev].push((key, cost));
            }
        }
        previous = current;
    }
    let end = nodes.len();
    edges.push(Vec::new());
    for &prev in &previous {
        edges[prev].push((end, 0));
    }
    // `dijkstrajs`: a queue kept stably sorted by cost, no visited set.
    let mut costs: Vec<Option<usize>> = vec![None; end + 1];
    let mut predecessors: Vec<Option<usize>> = vec![None; end + 1];
    costs[0] = Some(0);
    let mut queue: Vec<(usize, usize)> = vec![(0, 0)];
    while !queue.is_empty() {
        let (u, cost_u) = queue.remove(0);
        for &(v, edge) in &edges[u] {
            let through = cost_u + edge;
            if costs[v].is_none_or(|c| c > through) {
                costs[v] = Some(through);
                queue.push((v, through));
                queue.sort_by_key(|&(_, cost)| cost);
                predecessors[v] = Some(u);
            }
        }
    }
    let mut path = Vec::new();
    let mut at = predecessors[end];
    while let Some(node) = at.filter(|&n| n != 0) {
        path.push(node);
        at = predecessors[node];
    }
    path.reverse();
    // `mergeSegments`.
    let mut merged: Vec<Segment> = Vec::new();
    for node in path {
        let node = nodes[node].clone();
        match merged.last_mut() {
            Some(last) if last.mode == node.mode => last.data.push_str(&node.data),
            _ => merged.push(node),
        }
    }
    merged
}

fn penalty(size: usize, dark: &[bool]) -> usize {
    let get = |row: usize, col: usize| usize::from(dark[row * size + col]);
    let mut points = 0;
    // N1: runs of five or more.
    for row in 0..size {
        let (mut same_col, mut same_row) = (0, 0);
        let (mut last_col, mut last_row) = (None, None);
        for col in 0..size {
            let module = get(row, col);
            if Some(module) == last_col {
                same_col += 1;
            } else {
                if same_col >= 5 {
                    points += 3 + (same_col - 5);
                }
                last_col = Some(module);
                same_col = 1;
            }
            let module = get(col, row);
            if Some(module) == last_row {
                same_row += 1;
            } else {
                if same_row >= 5 {
                    points += 3 + (same_row - 5);
                }
                last_row = Some(module);
                same_row = 1;
            }
        }
        if same_col >= 5 {
            points += 3 + (same_col - 5);
        }
        if same_row >= 5 {
            points += 3 + (same_row - 5);
        }
    }
    // N2: 2×2 blocks.
    let mut blocks = 0;
    for row in 0..size - 1 {
        for col in 0..size - 1 {
            let sum = get(row, col) + get(row, col + 1) + get(row + 1, col) + get(row + 1, col + 1);
            if sum == 4 || sum == 0 {
                blocks += 1;
            }
        }
    }
    points += blocks * 3;
    // N3: finder-like patterns.
    let mut finders = 0;
    for row in 0..size {
        let (mut bits_col, mut bits_row) = (0usize, 0usize);
        for col in 0..size {
            bits_col = ((bits_col << 1) & 0x7FF) | get(row, col);
            if col >= 10 && (bits_col == 0x5D0 || bits_col == 0x05D) {
                finders += 1;
            }
            bits_row = ((bits_row << 1) & 0x7FF) | get(col, row);
            if col >= 10 && (bits_row == 0x5D0 || bits_row == 0x05D) {
                finders += 1;
            }
        }
    }
    points += finders * 40;
    // N4: dark balance.
    let count = dark.iter().filter(|d| **d).count() as f64;
    let k = ((count * 100.0 / dark.len() as f64) / 5.0).ceil() as i64 - 10;
    points + k.unsigned_abs() as usize * 10
}

const MASKS: [MaskPattern; 8] = [
    MaskPattern::Checkerboard,
    MaskPattern::HorizontalLines,
    MaskPattern::VerticalLines,
    MaskPattern::DiagonalLines,
    MaskPattern::LargeCheckerboard,
    MaskPattern::Fields,
    MaskPattern::Diamonds,
    MaskPattern::Meadow,
];

fn qr_error(error: impl std::fmt::Display) -> AudienceError {
    AudienceError::new(error.to_string())
}

/// The module grid as `qrcode.create(text, { errorCorrectionLevel: 'M' })`
/// builds it: its width, and `true` for each dark module, row by row.
pub fn qr_matrix(text: &str) -> Result<(usize, Vec<bool>)> {
    const TOO_BIG: &str = "The amount of data is too big to be stored in a QR Code";
    let estimate = best_version(&raw_split(text)).unwrap_or(40);
    let segments = optimal_segments(text, estimate);
    let version = best_version(&segments).ok_or_else(|| AudienceError::new(TOO_BIG))?;
    let mut bits = Bits::new(Version::Normal(version));
    for segment in &segments {
        let data = segment.data.as_bytes();
        match segment.mode {
            Mode::Numeric => bits.push_numeric_data(data),
            Mode::Alphanumeric => bits.push_alphanumeric_data(data),
            Mode::Byte => bits.push_byte_data(data),
        }
        .map_err(qr_error)?;
    }
    bits.push_terminator(EcLevel::M).map_err(qr_error)?;
    let (data, ec) =
        qrcode::ec::construct_codewords(&bits.into_bytes(), Version::Normal(version), EcLevel::M)
            .map_err(qr_error)?;
    let mut canvas = Canvas::new(Version::Normal(version), EcLevel::M);
    canvas.draw_all_functional_patterns();
    canvas.draw_data(&data, &ec);
    let size = Version::Normal(version).width() as usize;
    // `getBestMask`: the first mask with the lowest penalty.
    let mut best: Option<(usize, Vec<bool>)> = None;
    for mask in MASKS {
        let mut masked = canvas.clone();
        masked.apply_mask(mask);
        let dark: Vec<bool> = masked
            .into_colors()
            .into_iter()
            .map(|c| c == Color::Dark)
            .collect();
        let score = penalty(size, &dark);
        if best.as_ref().is_none_or(|(b, _)| score < *b) {
            best = Some((score, dark));
        }
    }
    let (_, dark) = best.expect("eight masks");
    Ok((size, dark))
}

/// Pixels as `qrToImageData` lays them out: a square of
/// `floor((size + 2 * margin) * scale)` with `scale = width / (size + 2 * margin)`.
pub fn qr_pixels(size: usize, dark: &[bool]) -> (usize, Vec<u8>) {
    let modules = (size + QR_MARGIN * 2) as f64;
    let scale = if QR_WIDTH as f64 >= modules {
        QR_WIDTH as f64 / modules
    } else {
        // `opts.scale` defaults to 4 when the width is too small.
        4.0
    };
    let symbol = (modules * scale).floor() as usize;
    let margin = QR_MARGIN as f64 * scale;
    let mut rgba = vec![255u8; symbol * symbol * 4];
    for i in 0..symbol {
        for j in 0..symbol {
            let (fi, fj) = (i as f64, j as f64);
            let inside = fi >= margin
                && fj >= margin
                && fi < symbol as f64 - margin
                && fj < symbol as f64 - margin;
            if !inside {
                continue;
            }
            let i_src = ((fi - margin) / scale).floor() as usize;
            let j_src = ((fj - margin) / scale).floor() as usize;
            if dark.get(i_src * size + j_src).copied().unwrap_or(false) {
                let at = (i * symbol + j) * 4;
                rgba[at..at + 3].fill(0);
            }
        }
    }
    (symbol, rgba)
}

pub fn qr_png(text: &str) -> Result<Vec<u8>> {
    let (size, dark) = qr_matrix(text)?;
    let (side, rgba) = qr_pixels(size, &dark);
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side as u32, side as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| AudienceError::new(error.to_string()))?;
    writer
        .write_image_data(&rgba)
        .map_err(|error| AudienceError::new(error.to_string()))?;
    writer
        .finish()
        .map_err(|error| AudienceError::new(error.to_string()))?;
    Ok(out)
}

/// `data:image/png;base64,…`, generated once per join URL. A failure is
/// not cached, so the next attempt tries again.
pub fn qr_data_url(text: &str) -> Result<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = lock(cache).get(text) {
        return Ok(hit.clone());
    }
    let url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(qr_png(text)?)
    );
    lock(cache).insert(text.to_owned(), url.clone());
    Ok(url)
}
