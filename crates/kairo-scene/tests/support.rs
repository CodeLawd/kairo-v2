//! The prompt 003 corpus, rebuilt from `tools/golden-capture/corpus.cjs`,
//! and the comparison helpers shared by the corpus tests.
#![allow(dead_code)]

use kairo_domain::overlay::*;
use kairo_domain::theme_library::BUILT_IN_THEMES;
use kairo_scene::{ColoredLine, SlideScene, TextRaster};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const WIDTH: u32 = 1920;
pub const HEIGHT: u32 = 1080;

pub struct Case {
    pub id: String,
    pub theme: OverlayTheme,
    pub reference: String,
    pub text: String,
    pub colored: Vec<ColoredLine>,
}

impl Case {
    pub fn scene(&self) -> SlideScene {
        SlideScene::build(
            &self.theme,
            &self.reference,
            &self.text,
            (WIDTH, HEIGHT),
            &self.colored,
        )
    }
}

fn passages() -> Vec<(&'static str, String, String)> {
    let long = "Blessed are the undefiled in the way, who walk in the law of the LORD. Blessed are they that keep his testimonies, and that seek him with the whole heart. ".repeat(10);
    vec![
        ("short", "John 11:35".into(), "Jesus wept.".into()),
        ("kjv", "John 3:16 (KJV)".into(), "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.".into()),
        ("bbe", "John 3:16 (BBE)".into(), "For God had such love for the world that he gave his only Son, so that whoever has faith in him may not come to destruction but have eternal life.".into()),
        ("bsb", "John 3:16 (BSB)".into(), "For God so loved the world that He gave His one and only Son, that everyone who believes in Him shall not perish but have eternal life.".into()),
        ("numbered", "Psalm 23:1–3 (KJV)".into(), "1 The LORD is my shepherd; I shall not want.\n2 He maketh me to lie down in green pastures: he leadeth me beside the still waters.\n3 He restoreth my soul: he leadeth me in the paths of righteousness for his name’s sake.".into()),
        ("long", "Psalm 119 (KJV)".into(), long.trim().into()),
        ("yoruba", "Yorùbá / English".into(), "Ìjà dópin, ogun sì tán\nThe strife is over, the battle done\nOlúwa wa ti ṣẹ́gun\nOur Lord has won the victory".into()),
        ("lyrics", "".into(), "Amazing grace! How sweet the sound\nThat saved a wretch like me\nI once was lost, but now am found\nWas blind, but now I see".into()),
        ("rtl", "مزمور ٢٣".into(), "الرَّبُّ رَاعِيَّ فَلَا يُعْوِزُنِي شَيْءٌ.\nThe Lord is my shepherd — 123.".into()),
        ("empty", "".into(), "".into()),
        ("blank-with-reference", "John 3:16".into(), " \n  ".into()),
        ("breakword", "Fixture".into(), format!("  Faith   hope   and love.\n\n{}\n<light> & “mercy”", "uninterrupted".repeat(9))),
    ]
}

type Variant = (&'static str, fn(&mut OverlayTextStyle));

fn variants() -> Vec<Variant> {
    vec![
        ("original", |_| {}),
        ("left", |v| {
            v.align = OverlayTextAlign::Left;
            v.vertical_align = OverlayVerticalAlign::Top;
            v.letter_spacing_px = 1.5;
            v.text_transform = OverlayTextTransform::Uppercase;
        }),
        ("right", |v| {
            v.align = OverlayTextAlign::Right;
            v.vertical_align = OverlayVerticalAlign::Bottom;
            v.letter_spacing_px = -0.4;
            v.text_transform = OverlayTextTransform::Lowercase;
            v.text_decoration = OverlayTextDecoration::LineThrough;
        }),
        ("outline", |v| {
            v.align = OverlayTextAlign::Justify;
            v.text_transform = OverlayTextTransform::Capitalize;
            v.text_decoration = OverlayTextDecoration::Underline;
            v.outline = OverlayTextOutline {
                enabled: true,
                position: OverlayOutlinePosition::Outside,
                width_px: 2.0,
                style: OverlayOutlineStyle::Solid,
                color: "#123040".into(),
                opacity: 0.85,
                extra: Default::default(),
            };
        }),
    ]
}

/// The 480 cases, in `corpus.cjs` order.
pub fn corpus() -> Vec<Case> {
    let mut cases = Vec::new();
    for built in BUILT_IN_THEMES.iter() {
        for (label, reference, text) in passages() {
            for (variant, apply) in variants() {
                for auto_fit in [false, true] {
                    let mut theme = built.theme.clone();
                    apply(&mut theme.verse);
                    theme.layout.auto_fit_text = auto_fit;
                    let colored = if label == "yoruba" {
                        text.split('\n')
                            .enumerate()
                            .map(|(i, line)| ColoredLine {
                                text: line.to_owned(),
                                color: (i % 2 == 1).then(|| "#5eead4".to_owned()),
                            })
                            .collect()
                    } else {
                        vec![]
                    };
                    cases.push(Case {
                        id: format!(
                            "{}-{label}-{variant}-{}",
                            built.id,
                            if auto_fit { "fit" } else { "fixed" }
                        ),
                        theme,
                        reference: reference.clone(),
                        text: text.clone(),
                        colored,
                    });
                }
            }
        }
    }
    cases
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Chromium's layout of every case, committed by prompt 003.
pub fn reference_layout() -> Vec<Value> {
    let path = workspace_root().join("tests/golden/003-text/layout.json");
    serde_json::from_slice(&std::fs::read(path).expect("layout.json")).expect("layout.json parses")
}

/// Where Chromium's flat-background renders of the corpus are cached.
pub fn reference_pngs() -> PathBuf {
    std::env::var_os("KAIRO_TEXT_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join(".cache/golden-capture/003/text"))
}

pub fn canonical(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A raster over Chromium's flat `#808080` page, as RGBA bytes.
pub fn over_gray(raster: Option<&TextRaster>) -> Vec<u8> {
    let mut canvas = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    for px in canvas.as_chunks_mut::<4>().0 {
        *px = [128, 128, 128, 255];
    }
    if let Some(r) = raster {
        for row in 0..r.height {
            for col in 0..r.width {
                let s = ((row * r.width + col) * 4) as usize;
                let d = (((r.y + row) * WIDTH + r.x + col) * 4) as usize;
                let a = u16::from(r.pixels[s + 3]);
                if a == 0 {
                    continue;
                }
                let inv = 255 - a;
                for c in 0..3 {
                    canvas[d + c] = (u16::from(r.pixels[s + c])
                        + (u16::from(canvas[d + c]) * inv + 127) / 255)
                        .min(255) as u8;
                }
            }
        }
    }
    canvas
}

pub fn load_png(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    assert_eq!((info.width, info.height), (WIDTH, HEIGHT));
    match info.color_type {
        png::ColorType::Rgba => Some(buf),
        png::ColorType::Rgb => Some(
            buf.as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
        ),
        other => panic!("unexpected colour type {other:?}"),
    }
}

pub fn save_png(path: &Path, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("png header")
        .write_image_data(rgba)
        .expect("png data");
}

/// Mean SSIM over 8x8 blocks of luminance: the whole frame, and only the
/// blocks where either image differs from the flat background.
pub fn ssim(left: &[u8], right: &[u8]) -> (f64, f64) {
    let (mut all, mut active) = (0.0, 0.0);
    let (mut blocks, mut active_blocks) = (0u32, 0u32);
    let row = WIDTH as usize * 4;
    let luma =
        |p: &[u8]| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
    for y in (0..HEIGHT as usize).step_by(8) {
        for x in (0..WIDTH as usize).step_by(8) {
            let (mut a, mut b, mut aa, mut bb, mut ab) = (0.0, 0.0, 0.0, 0.0, 0.0);
            let mut occupied = false;
            for yy in y..y + 8 {
                for xx in x..x + 8 {
                    let i = yy * row + xx * 4;
                    let l = &left[i..i + 3];
                    let r = &right[i..i + 3];
                    occupied |= l != [128, 128, 128] || r != [128, 128, 128];
                    let (l, r) = (luma(l), luma(r));
                    a += l;
                    b += r;
                    aa += l * l;
                    bb += r * r;
                    ab += l * r;
                }
            }
            let (ma, mb) = (a / 64.0, b / 64.0);
            let (va, vb) = ((aa - a * ma) / 63.0, (bb - b * mb) / 63.0);
            let cov = (ab - a * mb) / 63.0;
            let (c1, c2) = (6.5025, 58.5225);
            let v = ((2.0 * ma * mb + c1) * (2.0 * cov + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            all += v;
            blocks += 1;
            if occupied {
                active += v;
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
