//! Decks that exercise the built-in renderer: text, tables, groups, lines,
//! crops, theme colors, inherited masters and layouts, odd sizes, and
//! broken packages.

use super::deck::{Slide, package, png, pptx, slide};

const W: u64 = 12_192_000;
const H: u64 = 6_858_000;

fn sp(id: u32, xfrm: &str, fill: &str, body: &str) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Shape {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{xfrm}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{fill}</p:spPr>{body}</p:sp>"#
    )
}

fn xfrm(x: u64, y: u64, cx: u64, cy: u64) -> String {
    format!(r#"<a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#)
}

fn text_body(paragraphs: &str) -> String {
    format!(
        r#"<p:txBody><a:bodyPr lIns="91440" tIns="45720" rIns="91440" bIns="45720" anchor="ctr"/><a:lstStyle/>{paragraphs}</p:txBody>"#
    )
}

const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements><a:clrScheme name="C"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F2A44"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme></a:themeElements></a:theme>"#;

/// Text: runs with size, weight, italics, underline, scheme and RGB colors,
/// alignment, spacing, bullets-free lists, line breaks and entities.
fn text_deck() -> Vec<u8> {
    let paragraphs = concat!(
        r#"<a:p><a:pPr algn="ctr"><a:lnSpc><a:spcPct val="120000"/></a:lnSpc><a:spcBef><a:spcPts val="600"/></a:spcBef></a:pPr><a:r><a:rPr lang="en-US" sz="4400" b="1"><a:solidFill><a:srgbClr val="C00000"/></a:solidFill><a:latin typeface="Georgia"/></a:rPr><a:t>Amazing Grace</a:t></a:r></a:p>"#,
        r#"<a:p><a:pPr algn="r"><a:spcAft><a:spcPts val="1200"/></a:spcAft></a:pPr><a:r><a:rPr sz="2400" i="1" u="sng"><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></a:rPr><a:t>how sweet &amp; "the" &lt;sound&gt;</a:t></a:r><a:br/><a:r><a:rPr sz="1800"/><a:t xml:space="preserve">  that saved  </a:t></a:r></a:p>"#,
        r#"<a:p><a:r><a:rPr sz="2000"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill></a:rPr><a:t>a wretch like me — ünïcødé ✝</a:t></a:r></a:p>"#,
        r#"<a:p><a:endParaRPr sz="1200"/></a:p>"#,
    );
    let tree = [
        sp(2, &xfrm(457_200, 274_638, 8_229_600, 1_143_000), "", &text_body(paragraphs)),
        sp(
            3,
            &xfrm(609_600, 3_000_000, 4_000_000, 2_000_000),
            r#"<a:solidFill><a:schemeClr val="accent1"/></a:solidFill><a:ln w="38100"><a:solidFill><a:srgbClr val="FFC000"/></a:solidFill></a:ln>"#,
            &text_body(r#"<a:p><a:pPr algn="just"/><a:r><a:rPr sz="1400"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:rPr><a:t>Boxed text inside a filled, outlined shape.</a:t></a:r></a:p>"#),
        ),
    ]
    .concat();
    package(
        W,
        H,
        &[(slide("sld", "", &tree), String::new())],
        &[("ppt/theme/theme1.xml", THEME.as_bytes().to_vec())],
    )
}

/// A table with a header row, merged and colored cells.
fn table_deck() -> Vec<u8> {
    let cell = |text: &str, fill: &str| {
        format!(
            r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr sz="1800" b="1"/><a:t>{text}</a:t></a:r></a:p></a:txBody><a:tcPr>{fill}</a:tcPr></a:tc>"#
        )
    };
    let solid = |c: &str| format!(r#"<a:solidFill><a:srgbClr val="{c}"/></a:solidFill>"#);
    let rows = format!(
        r#"<a:tr h="370840">{}{}{}</a:tr><a:tr h="370840">{}{}{}</a:tr><a:tr h="370840">{}{}{}</a:tr>"#,
        cell("Song", &solid("1F2A44")),
        cell("Key", &solid("1F2A44")),
        cell("Tempo", &solid("1F2A44")),
        cell("Amazing Grace", ""),
        cell("G", ""),
        cell("72", &solid("FFF2CC")),
        cell("How Great", ""),
        cell("A &amp; B", ""),
        cell("", ""),
    );
    let table = format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="4" name="Table"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="838200" y="1825625"/><a:ext cx="10515600" cy="1112520"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="1" bandRow="1"/><a:tblGrid><a:gridCol w="3505200"/><a:gridCol w="3505200"/><a:gridCol w="3505200"/></a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#
    );
    package(W, H, &[(slide("sld", "", &table), String::new())], &[])
}

/// Groups, a cropped picture, a line and a background picture.
fn layered_deck() -> Vec<u8> {
    let picture = r#"<p:pic><p:nvPicPr><p:cNvPr id="5" name="Photo"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="10000" t="5000" r="20000" b="0"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm rot="5400000"><a:off x="6000000" y="1000000"/><a:ext cx="3000000" cy="2000000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#;
    let group = format!(
        r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="6" name="Group"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="100000" y="100000"/><a:ext cx="4000000" cy="2000000"/><a:chOff x="0" y="0"/><a:chExt cx="8000000" cy="4000000"/></a:xfrm></p:grpSpPr>{}{}</p:grpSp>"#,
        sp(
            7,
            &xfrm(0, 0, 4_000_000, 4_000_000),
            r#"<a:solidFill><a:srgbClr val="70AD47"/></a:solidFill>"#,
            ""
        ),
        sp(
            8,
            &xfrm(4_000_000, 0, 4_000_000, 4_000_000),
            r#"<a:solidFill><a:srgbClr val="ED7D31"><a:alpha val="50000"/></a:srgbClr></a:solidFill>"#,
            ""
        ),
    );
    let line = r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="9" name="Line"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr><a:xfrm flipV="1"><a:off x="500000" y="5000000"/><a:ext cx="11000000" cy="0"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:ln w="25400"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln></p:spPr></p:cxnSp>"#;
    let background = r#"<p:bg><p:bgPr><a:blipFill><a:blip r:embed="rId3"/><a:stretch><a:fillRect/></a:stretch></a:blipFill><a:effectLst/></p:bgPr></p:bg>"#;
    let rels = r#"<Relationship Id="rId2" Type="{REL}/image" Target="../media/photo.png"/><Relationship Id="rId3" Type="{REL}/image" Target="../media/bg.png"/>"#;
    package(
        W,
        H,
        &[(
            slide("sld", background, &format!("{group}{picture}{line}")),
            rels.into(),
        )],
        &[
            ("ppt/media/photo.png", png([10, 120, 200])),
            ("ppt/media/bg.png", png([240, 240, 220])),
        ],
    )
}

/// A slide inheriting a master background and layout artwork through a
/// second theme and a remapping `clrMap`; placeholders are not drawn twice.
fn inherited_deck() -> Vec<u8> {
    let master = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:bg><p:bgPr><a:solidFill><a:schemeClr val="bg2"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{}{}</p:spTree></p:cSld><p:clrMap bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2" accent1="accent6" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent1" hlink="hlink" folHlink="folHlink"/></p:sldMaster>"#,
        sp(
            2,
            &xfrm(0, 6_400_000, W, 458_000),
            r#"<a:solidFill><a:schemeClr val="accent1"/></a:solidFill>"#,
            ""
        ),
        r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Title Placeholder"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="838200" y="365125"/><a:ext cx="10515600" cy="1325563"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr/><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp>"#,
    );
    let layout = slide(
        "sldLayout",
        "",
        &sp(
            4,
            &xfrm(11_000_000, 200_000, 900_000, 900_000),
            r#"<a:solidFill><a:schemeClr val="tx2"/></a:solidFill>"#,
            "",
        ),
    );
    let tree = r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Welcome</a:t></a:r></a:p></p:txBody></p:sp>"#;
    let slide_rels = r#"<Relationship Id="rId1" Type="{REL}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>"#;
    let layout_rels = r#"<Relationship Id="rId1" Type="{REL}/slideMaster" Target="../slideMasters/slideMaster1.xml"/>"#;
    let master_rels =
        r#"<Relationship Id="rId9" Type="{REL}/theme" Target="/ppt/theme/theme2.xml"/>"#;
    let theme2 = THEME
        .replace("4472C4", "8E44AD")
        .replace("1F2A44", "102030");
    use super::deck::relationships;
    package(
        W,
        H,
        &[(slide("sld", "", tree), slide_rels.into())],
        &[
            ("ppt/theme/theme1.xml", THEME.as_bytes().to_vec()),
            ("ppt/theme/theme2.xml", theme2.into_bytes()),
            ("ppt/slideMasters/slideMaster1.xml", master.into_bytes()),
            (
                "ppt/slideMasters/_rels/slideMaster1.xml.rels",
                relationships(master_rels).into_bytes(),
            ),
            ("ppt/slideLayouts/slideLayout1.xml", layout.into_bytes()),
            (
                "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
                relationships(layout_rels).into_bytes(),
            ),
        ],
    )
}

/// A master the slide hides with `showMasterSp="0"`, at 4:3.
fn four_by_three_deck() -> Vec<u8> {
    let xml = slide(
        "sld",
        "",
        &sp(
            2,
            &xfrm(0, 0, 4_572_000, 3_429_000),
            r#"<a:solidFill><a:srgbClr val="5B9BD5"/></a:solidFill>"#,
            "",
        ),
    )
    .replace("<p:sld ", r#"<p:sld showMasterSp="0" "#);
    package(9_144_000, 6_858_000, &[(xml, String::new())], &[])
}

/// Every deck by name, `.pptx` bytes. The broken ones end in `!`.
pub fn decks() -> Vec<(&'static str, Vec<u8>)> {
    let solid = pptx(
        &[
            Slide {
                fill: "FF0000",
                picture: Some(png([0, 0, 255])),
            },
            Slide {
                fill: "00FF00",
                picture: None,
            },
        ],
        W,
        H,
    );
    let mut no_presentation = std::io::Cursor::new(Vec::new());
    {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(&mut no_presentation);
        zip.start_file(
            "ppt/slides/slide1.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"<p:sld/>").unwrap();
        zip.finish().unwrap();
    }
    vec![
        ("solid", solid),
        ("text", text_deck()),
        ("table", table_deck()),
        ("layered", layered_deck()),
        ("inherited", inherited_deck()),
        ("four-by-three", four_by_three_deck()),
        ("not-a-zip!", b"PK not really".to_vec()),
        ("no-presentation!", no_presentation.into_inner()),
        ("no-slides!", package(W, H, &[], &[])),
    ]
}
