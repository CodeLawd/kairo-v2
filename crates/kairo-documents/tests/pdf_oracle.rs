//! PDFium pages against pdf.js's, by SSIM, through the oracle
//! (`oracle:pdf-pages.render`).

mod support;

use base64::Engine;
use kairo_documents::pdf::render_pages;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use support::pdf::require_pdfium;

/// Below this the pages are not the same picture. Measured values are in
/// PROGRESS.md (prompt 045).
const MIN_SSIM: f64 = 0.95;

fn luma(image: &image::RgbaImage) -> Vec<f64> {
    image
        .pixels()
        .map(|p| {
            let [r, g, b, _] = p.0.map(f64::from);
            0.299 * r + 0.587 * g + 0.114 * b
        })
        .collect()
}

/// Mean SSIM over 8×8 blocks of luma, with the usual constants.
fn ssim(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
    let (w, h) = a.dimensions();
    let (la, lb) = (luma(a), luma(b));
    let (c1, c2) = ((0.01f64 * 255.0).powi(2), (0.03f64 * 255.0).powi(2));
    let mut total = 0.0;
    let mut blocks = 0.0;
    for by in (0..h - 7).step_by(8) {
        for bx in (0..w - 7).step_by(8) {
            let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for y in by..by + 8 {
                for x in bx..bx + 8 {
                    let i = (y * w + x) as usize;
                    let (p, q) = (la[i], lb[i]);
                    sa += p;
                    sb += q;
                    saa += p * p;
                    sbb += q * q;
                    sab += p * q;
                }
            }
            let n = 64.0;
            let (ma, mb) = (sa / n, sb / n);
            let (va, vb, cov) = (saa / n - ma * ma, sbb / n - mb * mb, sab / n - ma * mb);
            total += ((2.0 * ma * mb + c1) * (2.0 * cov + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            blocks += 1.0;
        }
    }
    total / blocks
}

#[test]
fn pdfium_pages_look_like_pdf_js() {
    if !require_pdfium() {
        return;
    }
    let pdfs = support::pdf_corpus::pdfs();
    let b64 = base64::engine::general_purpose::STANDARD;
    let encoded: Vec<String> = pdfs.iter().map(|(_, d)| b64.encode(d)).collect();
    let expected = kairo_oracle::oracle_value("oracle:pdf-pages.render", json!([encoded]));
    let mut report = Vec::new();
    let mut worst = 1.0f64;
    for ((name, data), pages) in pdfs.iter().zip(expected.as_array().unwrap()) {
        let dir = tempfile::tempdir().unwrap();
        let ours = render_pages(data, dir.path(), &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        let pages = pages.as_array().unwrap();
        assert_eq!(ours.len(), pages.len(), "{name}: page count");
        for (index, (path, page)) in ours.iter().zip(pages).enumerate() {
            let mine = image::open(path).unwrap().into_rgba8();
            let theirs =
                image::load_from_memory(&b64.decode(page["png"].as_str().unwrap()).unwrap())
                    .unwrap()
                    .into_rgba8();
            assert_eq!(
                mine.dimensions(),
                (
                    page["width"].as_u64().unwrap() as u32,
                    page["height"].as_u64().unwrap() as u32
                ),
                "{name} page {index}: size"
            );
            let score = ssim(&mine, &theirs);
            worst = worst.min(score);
            report.push(format!(
                "{name} p{index} {}×{} SSIM {score:.4}",
                mine.width(),
                mine.height()
            ));
        }
    }
    println!("{}", report.join("\n"));
    assert!(
        worst >= MIN_SSIM,
        "worst SSIM {worst:.4} < {MIN_SSIM}\n{}",
        report.join("\n")
    );
}
