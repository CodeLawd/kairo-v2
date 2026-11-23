//! A drawn deck as a PowerPoint file built from native shapes: every card,
//! badge, bar, ring and text box Kairo designed is a real, editable
//! PowerPoint shape in the deck's fonts and colours; pictures are placed as
//! pictures, and the speaker notes go in the slide's notes.

use std::io::{Cursor, Write};

use kairo_scene::color::Rgba;
use kairo_scene::{Align, Rect, Vertical};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::ImagineError;
use crate::deck::{Frame, HEIGHT, Mark, Picture, WIDTH, picture_png};

/// EMU per slide pixel: 1920 px across a 13.333 in (12192000 EMU) slide.
const EMU: f32 = 12_192_000. / WIDTH as f32;
const SLIDE_CX: i64 = 12_192_000;
const SLIDE_CY: i64 = 6_858_000;

/// One slide to export.
pub struct ExportSlide {
    pub page: Rgba,
    /// The design, back to front.
    pub marks: Vec<Mark>,
    pub background: Option<Vec<u8>>,
    pub art: Option<Vec<u8>>,
    pub notes: String,
}

impl ExportSlide {
    /// A slide that is only a full-bleed picture.
    pub fn picture(png: Vec<u8>, notes: String) -> Self {
        Self {
            page: Rgba::new(0, 0, 0, 255),
            marks: vec![Mark::Picture {
                rect: Rect {
                    x: 0.,
                    y: 0.,
                    width: WIDTH as f32,
                    height: HEIGHT as f32,
                },
                source: Picture::Background,
                radius: 0.,
            }],
            background: Some(png),
            art: None,
            notes,
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}

fn emu(px: f32) -> i64 {
    (px * EMU).round() as i64
}

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const GROUP: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;

/// A colour, with its transparency when it has one.
fn clr(c: Rgba) -> String {
    let hex = format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b);
    if c.a == 255 {
        format!(r#"<a:srgbClr val="{hex}"/>"#)
    } else {
        let alpha = (c.a as u32 * 100_000 / 255).min(100_000);
        format!(r#"<a:srgbClr val="{hex}"><a:alpha val="{alpha}"/></a:srgbClr>"#)
    }
}

fn xfrm(r: Rect) -> String {
    format!(
        r#"<a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
        emu(r.x),
        emu(r.y),
        emu(r.width).max(1),
        emu(r.height).max(1)
    )
}

/// A rectangle, rounded when `radius` is set.
fn geometry(r: Rect, radius: f32) -> String {
    let short = r.width.min(r.height);
    if radius <= 0. || short <= 0. {
        return r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#.into();
    }
    let adj = ((radius / short) * 100_000.).round().min(50_000.) as i64;
    format!(
        r#"<a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val {adj}"/></a:avLst></a:prstGeom>"#
    )
}

fn shape(id: usize, r: Rect, fill: Rgba, radius: f32) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Shape {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}{}<a:solidFill>{}</a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp>"#,
        xfrm(r),
        geometry(r, radius),
        clr(fill)
    )
}

fn outline(id: usize, r: Rect, color: Rgba, width: f32, radius: f32) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Outline {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}{}<a:noFill/><a:ln w="{}"><a:solidFill>{}</a:solidFill></a:ln></p:spPr></p:sp>"#,
        xfrm(r),
        geometry(r, radius),
        emu(width),
        clr(color)
    )
}

fn ring(id: usize, r: Rect, color: Rgba, width: f32) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Ring {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="{}"><a:solidFill>{}</a:solidFill></a:ln></p:spPr></p:sp>"#,
        xfrm(r),
        emu(width),
        clr(color)
    )
}

fn picture(id: usize, rid: usize, r: Rect, radius: f32) -> String {
    format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="Picture {id}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}{}</p:spPr></p:pic>"#,
        xfrm(r),
        geometry(r, radius)
    )
}

fn text_box(id: usize, f: &Frame) -> String {
    let anchor = match f.vertical {
        Vertical::Top => "t",
        Vertical::Middle => "ctr",
        Vertical::Bottom => "b",
    };
    let algn = match f.align {
        Align::Left => "l",
        Align::Center => "ctr",
        Align::Right => "r",
        Align::Justify => "just",
    };
    // Slide pixels to points: 1920 px is 960 pt.
    let size = ((f.size * 0.5).round() as i64).clamp(6, 400) * 100;
    let spc = (f.letter_spacing * 50.).round() as i64;
    let line = (f.line_height * 100_000.).round() as i64;
    let paragraphs: String = f
        .paragraphs
        .iter()
        .map(|p| {
            format!(
                r#"<a:p><a:pPr algn="{algn}"><a:lnSpc><a:spcPct val="{line}"/></a:lnSpc><a:buNone/></a:pPr><a:r><a:rPr lang="en-US" sz="{size}" b="{b}" spc="{spc}" dirty="0"><a:solidFill>{color}</a:solidFill><a:latin typeface="{font}"/><a:cs typeface="{font}"/></a:rPr><a:t>{text}</a:t></a:r></a:p>"#,
                b = u8::from(f.bold()),
                color = clr(f.color),
                font = esc(&f.font),
                text = esc(p),
            )
        })
        .collect();
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Text {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" lIns="0" tIns="0" rIns="0" bIns="0" anchor="{anchor}"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#,
        xfrm(f.rect)
    )
}

/// The slide's XML and its pictures (PNG, in relationship order from rId3).
fn slide_xml(s: &ExportSlide) -> Result<(String, Vec<Vec<u8>>), ImagineError> {
    let mut media = Vec::new();
    let mut tree = String::new();
    for (i, mark) in s.marks.iter().enumerate() {
        let id = i + 2;
        match mark {
            Mark::Rect { rect, fill, radius } => tree.push_str(&shape(id, *rect, *fill, *radius)),
            Mark::Outline {
                rect,
                color,
                width,
                radius,
            } => tree.push_str(&outline(id, *rect, *color, *width, *radius)),
            Mark::Ring { rect, color, width } => tree.push_str(&ring(id, *rect, *color, *width)),
            Mark::Text(f) => tree.push_str(&text_box(id, f)),
            Mark::Picture {
                rect,
                source,
                radius,
            } => {
                let bytes = match source {
                    Picture::Background => &s.background,
                    Picture::Art => &s.art,
                };
                let Some(bytes) = bytes else { continue };
                media.push(picture_png(bytes, *rect)?);
                tree.push_str(&picture(id, media.len() + 2, *rect, *radius));
            }
        }
    }
    let xml = format!(
        r#"{XML}<p:sld {NS}><p:cSld><p:bg><p:bgPr><a:solidFill>{}</a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>{GROUP}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
        clr(Rgba::new(s.page.r, s.page.g, s.page.b, 255))
    );
    Ok((xml, media))
}

fn notes_xml(n: usize, notes: &str) -> String {
    let paragraphs: String = notes
        .lines()
        .map(|l| {
            format!(
                r#"<a:p><a:r><a:rPr lang="en-US" dirty="0"/><a:t>{}</a:t></a:r></a:p>"#,
                esc(l)
            )
        })
        .collect();
    let paragraphs = if paragraphs.is_empty() {
        "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>".to_string()
    } else {
        paragraphs
    };
    let _ = n;
    format!(
        r#"{XML}<p:notes {NS}><p:cSld><p:spTree>{GROUP}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"#
    )
}

const CLR_MAP: &str = r#"<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#;

fn theme_xml() -> String {
    let solid = |c: &str| format!(r#"<a:solidFill><a:schemeClr val="{c}"/></a:solidFill>"#);
    let line = r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#;
    format!(
        r#"{XML}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Imagine"><a:themeElements><a:clrScheme name="Imagine"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1E2430"/></a:dk2><a:lt2><a:srgbClr val="F5F3EE"/></a:lt2><a:accent1><a:srgbClr val="E8B04A"/></a:accent1><a:accent2><a:srgbClr val="5B8DB8"/></a:accent2><a:accent3><a:srgbClr val="7FA36B"/></a:accent3><a:accent4><a:srgbClr val="C9705A"/></a:accent4><a:accent5><a:srgbClr val="8C6BB1"/></a:accent5><a:accent6><a:srgbClr val="4FA3A0"/></a:accent6><a:hlink><a:srgbClr val="5B8DB8"/></a:hlink><a:folHlink><a:srgbClr val="8C6BB1"/></a:folHlink></a:clrScheme><a:fontScheme name="Imagine"><a:majorFont><a:latin typeface="Georgia"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Helvetica Neue"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Imagine"><a:fillStyleLst>{f}{f}{f}</a:fillStyleLst><a:lnStyleLst>{line}{line}{line}</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{f}{f}{f}</a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#,
        f = solid("phClr"),
    )
}

/// The .pptx file.
pub fn write(title: &str, slides: &[ExportSlide]) -> Result<Vec<u8>, ImagineError> {
    if slides.is_empty() {
        return Err(ImagineError::Api("the deck has no slides".into()));
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, bytes: &[u8], o: SimpleFileOptions| -> Result<(), ImagineError> {
        zip.start_file(name, o)
            .map_err(|e| ImagineError::Storage(e.to_string()))?;
        zip.write_all(bytes)?;
        Ok(())
    };
    let n = slides.len();
    let overrides: String = (1..=n)
        .map(|i| {
            format!(
                r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/><Override PartName="/ppt/notesSlides/notesSlide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>"#
            )
        })
        .collect();
    put(
        "[Content_Types].xml",
        format!(
            r#"{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/ppt/theme/theme2.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>{overrides}</Types>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "_rels/.rels",
        format!(
            r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "docProps/core.xml",
        format!(
            r#"{XML}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>{}</dc:title><dc:creator>Kairo Imagine</dc:creator></cp:coreProperties>"#,
            esc(title)
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "docProps/app.xml",
        format!(
            r#"{XML}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Kairo</Application><Slides>{n}</Slides></Properties>"#
        )
        .as_bytes(),
        opts,
    )?;
    let slide_ids: String = (1..=n)
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, i + 3))
        .collect();
    put(
        "ppt/presentation.xml",
        format!(
            r#"{XML}<p:presentation {NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:notesMasterIdLst><p:notesMasterId r:id="rId3"/></p:notesMasterIdLst><p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx="{SLIDE_CX}" cy="{SLIDE_CY}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
        )
        .as_bytes(),
        opts,
    )?;
    let slide_rels: String = (1..=n)
        .map(|i| format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>"#, i + 3))
        .collect();
    put(
        "ppt/_rels/presentation.xml.rels",
        format!(
            r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="notesMasters/notesMaster1.xml"/>{slide_rels}</Relationships>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/slideMasters/slideMaster1.xml",
        format!(
            r#"{XML}<p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>{GROUP}</p:spTree></p:cSld>{CLR_MAP}<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        format!(
            r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/slideLayouts/slideLayout1.xml",
        format!(
            r#"{XML}<p:sldLayout {NS} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{GROUP}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        format!(
            r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/notesMasters/notesMaster1.xml",
        format!(
            r#"{XML}<p:notesMaster {NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>{GROUP}</p:spTree></p:cSld>{CLR_MAP}</p:notesMaster>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        "ppt/notesMasters/_rels/notesMaster1.xml.rels",
        format!(
            r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme2.xml"/></Relationships>"#
        )
        .as_bytes(),
        opts,
    )?;
    put("ppt/theme/theme1.xml", theme_xml().as_bytes(), opts)?;
    put("ppt/theme/theme2.xml", theme_xml().as_bytes(), opts)?;
    for (i, s) in slides.iter().enumerate() {
        let i = i + 1;
        let (xml, media) = slide_xml(s)?;
        let mut rels = String::new();
        for (k, png) in media.iter().enumerate() {
            put(&format!("ppt/media/image{i}_{}.png", k + 1), png, stored)?;
            rels.push_str(&format!(
                r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image{i}_{}.png"/>"#,
                k + 3,
                k + 1
            ));
        }
        put(&format!("ppt/slides/slide{i}.xml"), xml.as_bytes(), opts)?;
        put(
            &format!("ppt/slides/_rels/slide{i}.xml.rels"),
            format!(
                r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide{i}.xml"/>{rels}</Relationships>"#
            )
            .as_bytes(),
            opts,
        )?;
        put(
            &format!("ppt/notesSlides/notesSlide{i}.xml"),
            notes_xml(i, &s.notes).as_bytes(),
            opts,
        )?;
        put(
            &format!("ppt/notesSlides/_rels/notesSlide{i}.xml.rels"),
            format!(
                r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="../notesMasters/notesMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="../slides/slide{i}.xml"/></Relationships>"#
            )
            .as_bytes(),
            opts,
        )?;
    }
    let cursor = zip
        .finish()
        .map_err(|e| ImagineError::Storage(e.to_string()))?;
    let _ = HEIGHT;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deck::{Content, DeckTheme, Layout, Point, compose};

    #[test]
    fn writes_native_shapes_text_and_notes_for_every_slide() {
        let theme = DeckTheme::default();
        let slides: Vec<ExportSlide> = [Layout::Title, Layout::Cards]
            .into_iter()
            .map(|layout| {
                let c = Content {
                    layout,
                    title: "AI for Church Media".into(),
                    lead: "Plan & pray".into(),
                    points: vec![Point {
                        heading: "Caption <every> video".into(),
                        ..Point::default()
                    }],
                    page: 2,
                    pages: 2,
                    deck: "AI".into(),
                    ..Content::default()
                };
                let (page, marks) = compose(&theme, &c);
                ExportSlide {
                    page,
                    marks,
                    background: None,
                    art: None,
                    notes: "Welcome everyone.".into(),
                }
            })
            .collect();
        let bytes = write("AI & media", &slides).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        for name in [
            "[Content_Types].xml",
            "ppt/presentation.xml",
            "ppt/slides/slide2.xml",
            "ppt/notesSlides/notesSlide1.xml",
        ] {
            assert!(zip.by_name(name).is_ok(), "{name}");
        }
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("ppt/slides/slide2.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(
            xml.contains("Caption &lt;every&gt; video"),
            "text is escaped and editable"
        );
        assert!(xml.contains("roundRect"), "cards are native rounded shapes");
        let mut notes = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("ppt/notesSlides/notesSlide2.xml").unwrap(),
            &mut notes,
        )
        .unwrap();
        assert!(notes.contains("Welcome everyone."));
    }

    #[test]
    fn a_picture_slide_embeds_its_picture() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(32, 18, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let bytes = write("Old", &[ExportSlide::picture(png, String::new())]).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(zip.by_name("ppt/media/image1_1.png").is_ok());
    }
}
