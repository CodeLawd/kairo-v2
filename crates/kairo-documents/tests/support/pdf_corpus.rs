//! PDFs for comparing PDFium pages with pdf.js: an embedded TrueType font,
//! gradients, transparency, vector strokes and an image.

use super::pdf::{objects, stream};

const FONT: &[u8] = include_bytes!("../fixtures/fonts/LiberationSans-Regular.ttf");

fn catalog_and_page(width: u32, height: u32, resources: &str) -> Vec<Vec<u8>> {
    vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources {resources} /Contents 4 0 R >>"
        )
        .into_bytes(),
    ]
}

/// WinAnsi widths for 32..=126 in 1/1000 em, from the font's own metrics.
fn widths() -> String {
    let face = ttf_parser::Face::parse(FONT, 0).unwrap();
    let em = f64::from(face.units_per_em());
    (32u8..=126)
        .map(|c| {
            let glyph = face.glyph_index(char::from(c)).unwrap();
            let advance = f64::from(face.glyph_hor_advance(glyph).unwrap());
            format!("{}", (advance * 1000.0 / em).round())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn text() -> Vec<u8> {
    let content = concat!(
        "0.2 0.2 0.2 rg BT /F1 40 Tf 72 690 Td (Amazing Grace, how sweet the sound) Tj ET\n",
        "0.75 0 0 rg BT /F1 22 Tf 72 640 Td (That saved a wretch like me! 0123456789) Tj ET\n",
        "0 0 0.6 rg BT /F1 12 Tf 72 610 Td 14 TL (I once was lost, but now am found;) Tj T* (Was blind, but now I see.) Tj ET\n",
        "0 g BT /F1 8 Tf 72 560 Td (Small print: ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz) Tj ET\n",
        "BT /F1 64 Tf 1 0 0 1 72 420 Tm 2 Tr 1.5 w 0.1 0.5 0.1 RG (Outline) Tj ET\n",
        "BT /F1 30 Tf 0.866 0.5 -0.5 0.866 300 200 Tm 0 Tr (Rotated text) Tj ET\n",
    );
    let mut bodies = catalog_and_page(612, 792, "<< /Font << /F1 5 0 R >> >>");
    bodies.push(stream("", content.as_bytes()));
    bodies.push(
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSans /FirstChar 32 /LastChar 126 /Widths [{}] /FontDescriptor 6 0 R /Encoding /WinAnsiEncoding >>",
            widths()
        )
        .into_bytes(),
    );
    bodies.push(
        b"<< /Type /FontDescriptor /FontName /LiberationSans /Flags 32 /FontBBox [-203 -303 1050 910] /ItalicAngle 0 /Ascent 905 /Descent -212 /CapHeight 716 /StemV 80 /FontFile2 7 0 R >>"
            .to_vec(),
    );
    bodies.push(stream(&format!("/Length1 {}", FONT.len()), FONT));
    objects(&bodies)
}

fn gradients() -> Vec<u8> {
    let content = concat!(
        "q 0 0 960 540 re W n /Sh1 sh Q\n",
        "q 480 270 m 900 270 l 900 520 l 480 520 l h W n /Sh2 sh Q\n",
    );
    let mut bodies = catalog_and_page(960, 540, "<< /Shading << /Sh1 5 0 R /Sh2 6 0 R >> >>");
    bodies.push(stream("", content.as_bytes()));
    bodies.push(
        b"<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 960 540] /Extend [true true] /Function << /FunctionType 2 /Domain [0 1] /C0 [0.05 0.1 0.4] /C1 [0.95 0.6 0.1] /N 1 >> >>"
            .to_vec(),
    );
    bodies.push(
        b"<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [690 395 0 690 395 200] /Extend [false true] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [0.2 0.7 0.3] /N 1 >> >>"
            .to_vec(),
    );
    objects(&bodies)
}

fn transparency() -> Vec<u8> {
    let content = concat!(
        "0.9 0.9 0.95 rg 0 0 800 600 re f\n",
        "/A gs 1 0 0 rg 100 150 300 300 re f\n",
        "/B gs 0 0 1 rg 250 100 300 300 re f\n",
        "/C gs 0 0.7 0 rg 400 200 300 300 re f\n",
        "/D gs 0 g 20 w 1 0 0 RG 80 520 m 720 520 l S\n",
    );
    let mut bodies = catalog_and_page(
        800,
        600,
        "<< /ExtGState << /A << /ca 0.8 >> /B << /ca 0.5 >> /C << /ca 0.3 /BM /Multiply >> /D << /CA 0.4 >> >> >>",
    );
    bodies.push(stream("", content.as_bytes()));
    objects(&bodies)
}

fn vectors_and_image() -> Vec<u8> {
    // A 32×32 RGB checkerboard of red and white, drawn at 200×200 points.
    let mut pixels = Vec::with_capacity(32 * 32 * 3);
    for y in 0..32 {
        for x in 0..32 {
            let red = (x / 4 + y / 4) % 2 == 0;
            pixels.extend(if red { [220, 30, 30] } else { [255, 255, 255] });
        }
    }
    let content = concat!(
        "q 200 0 0 200 360 80 cm /Im1 Do Q\n",
        "0.1 0.3 0.8 RG 6 w 1 J 1 j 60 60 m 120 300 220 300 280 60 c S\n",
        "0.9 0.6 0 rg 0 0 0 RG 2 w 80 420 m 160 560 l 240 420 l h B\n",
        "[12 6] 0 d 3 w 0.4 g 40 380 m 600 380 l S\n",
        "0 0.5 0.5 rg 450 450 m 450 505 495 550 550 550 c 605 550 650 505 650 450 c 650 395 605 350 550 350 c 495 350 450 395 450 450 c f\n",
    );
    let mut bodies = catalog_and_page(700, 620, "<< /XObject << /Im1 5 0 R >> >>");
    bodies.push(stream("", content.as_bytes()));
    bodies.push(stream(
        "/Type /XObject /Subtype /Image /Width 32 /Height 32 /ColorSpace /DeviceRGB /BitsPerComponent 8",
        &pixels,
    ));
    objects(&bodies)
}

/// Every PDF by name.
pub fn pdfs() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("text", text()),
        ("gradients", gradients()),
        ("transparency", transparency()),
        ("vectors-and-image", vectors_and_image()),
    ]
}
