//! Hand-built PowerPoint decks for tests.

use std::io::{Cursor, Write};

/// One slide: a full-bleed rectangle of `fill` (hex, no `#`), and when
/// `picture` is set a PNG in the top-left quarter.
pub struct Slide {
    pub fill: &'static str,
    pub picture: Option<Vec<u8>>,
}

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn slide_xml(slide: &Slide, cx: u64, cy: u64) -> String {
    let picture = if slide.picture.is_some() {
        format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="3" name="Picture"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
            cx / 2,
            cy / 2
        )
    } else {
        String::new()
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Fill"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="{}"/></a:solidFill></p:spPr></p:sp>{picture}</p:spTree></p:cSld></p:sld>"#,
        slide.fill
    )
}

/// A `.pptx` from raw parts: each slide's XML and `.rels`, then any other
/// files (layouts, masters, themes, media) by package path.
pub fn package(
    cx: u64,
    cy: u64,
    slides: &[(String, String)],
    extra: &[(&str, Vec<u8>)],
) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, bytes: &[u8]| {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    };
    let ids: String = (0..slides.len())
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, i + 1))
        .collect();
    put(
        "ppt/presentation.xml",
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:presentation {NS}><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/></p:presentation>"#).as_bytes(),
    );
    let rels: String = (0..slides.len())
        .map(|i| {
            format!(
                r#"<Relationship Id="rId{}" Type="{REL}/slide" Target="slides/slide{}.xml"/>"#,
                i + 1,
                i + 1
            )
        })
        .collect();
    put(
        "ppt/_rels/presentation.xml.rels",
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#).as_bytes(),
    );
    for (i, (xml, rels)) in slides.iter().enumerate() {
        put(&format!("ppt/slides/slide{}.xml", i + 1), xml.as_bytes());
        if !rels.is_empty() {
            put(
                &format!("ppt/slides/_rels/slide{}.xml.rels", i + 1),
                relationships(rels).as_bytes(),
            );
        }
    }
    for (name, bytes) in extra {
        put(name, bytes);
    }
    zip.finish().unwrap().into_inner()
}

/// A `.rels` part around `body`, where `{REL}` is the relationship namespace.
pub fn relationships(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}</Relationships>"#,
        body.replace("{REL}", REL)
    )
}

/// Slide XML with `tree` inside `p:spTree`, and `before` ahead of `p:cSld`'s tree (a background).
pub fn slide(root: &str, before: &str, tree: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:{root} {NS}><p:cSld>{before}<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{tree}</p:spTree></p:cSld></p:{root}>"#
    )
}

/// A `.pptx` of `slides` at `cx`×`cy` EMU.
pub fn pptx(slides: &[Slide], cx: u64, cy: u64) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, bytes: &[u8]| {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    };
    put("[Content_Types].xml", br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/></Types>"#);
    let ids: String = (0..slides.len())
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, i + 1))
        .collect();
    put(
        "ppt/presentation.xml",
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:presentation {NS}><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/></p:presentation>"#).as_bytes(),
    );
    let rels: String = (0..slides.len())
        .map(|i| {
            format!(
                r#"<Relationship Id="rId{}" Type="{REL}/slide" Target="slides/slide{}.xml"/>"#,
                i + 1,
                i + 1
            )
        })
        .collect();
    put(
        "ppt/_rels/presentation.xml.rels",
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#).as_bytes(),
    );
    for (i, slide) in slides.iter().enumerate() {
        let n = i + 1;
        put(
            &format!("ppt/slides/slide{n}.xml"),
            slide_xml(slide, cx, cy).as_bytes(),
        );
        if let Some(png) = &slide.picture {
            put(&format!("ppt/media/image{n}.png"), png);
            put(
                &format!("ppt/slides/_rels/slide{n}.xml.rels"),
                format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="{REL}/image" Target="../media/image{n}.png"/></Relationships>"#).as_bytes(),
            );
        }
    }
    zip.finish().unwrap().into_inner()
}

/// A solid PNG of `rgb`.
pub fn png(rgb: [u8; 3]) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(64, 64, image::Rgb(rgb));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}
