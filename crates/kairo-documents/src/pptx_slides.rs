//! PowerPoint slide XML to HTML for the built-in renderer. From
//! `src/main/services/documents/pptx-slides.ts`, regex for regex, so the
//! generated HTML is byte-identical to Electron's.

use fancy_regex::Regex as FancyRegex;
use kairo_domain::js::number_to_string as num;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub const SLIDE_W: f64 = 1920.0;
pub const DEFAULT_CX: f64 = 12_192_000.0;
pub const DEFAULT_CY: f64 = 6_858_000.0;

pub type Colors = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlideSize {
    pub cx: f64,
    pub cy: f64,
}

impl Default for SlideSize {
    fn default() -> Self {
        Self {
            cx: DEFAULT_CX,
            cy: DEFAULT_CY,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlideBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlideLine {
    pub text: String,
    pub color: String,
    pub size_pt: f64,
    pub font_family: String,
    pub bold: bool,
    pub italic: bool,
    pub before_pt: f64,
    pub after_pt: f64,
    pub line_height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub t: f64,
    pub b: f64,
    pub l: f64,
    pub r: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SlideElement {
    Text {
        frame: SlideBox,
        align: &'static str,
        size_pt: f64,
        lines: Vec<SlideLine>,
        anchor: &'static str,
        insets: Insets,
    },
    Shape {
        frame: SlideBox,
        fill: String,
        border: String,
        border_width: f64,
        geometry: String,
    },
    Table {
        frame: SlideBox,
        header: bool,
        rows: Vec<Vec<String>>,
    },
    Image {
        frame: SlideBox,
        file: String,
        crop: Option<Crop>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedSlide {
    pub background: String,
    pub elements: Vec<SlideElement>,
}

macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pattern).expect("valid pattern"));
    };
}

re!(SLD_SZ, r"<p:sldSz\b[^>]*>");
re!(LAST_CLR, r"lastClr=\x22([0-9A-Fa-f]{6})\x22");
re!(VAL_HEX, r"val=\x22([0-9A-Fa-f]{6})\x22");
re!(
    XFRM,
    r"<a:xfrm\b[\s\S]*?</a:xfrm>|<p:xfrm\b[\s\S]*?</p:xfrm>"
);
re!(OFF, r"<(?:a:)?off\b[^>]*>");
re!(EXT, r"<(?:a:)?ext\b[^>]*>");
re!(A_T, r"<a:t(?:\s[^>]*)?>([\s\S]*?)</a:t>");
re!(WS, r"\s+");
re!(PARA, r"<a:p\b[\s\S]*?</a:p>");
re!(SZ, r"\bsz=\x22(\d+)\x22");
re!(ALGN, r"\balgn=\x22(ctr|r|l)\x22");
re!(TR, r"<a:tr\b[\s\S]*?</a:tr>");
re!(TC, r"<a:tc\b[\s\S]*?</a:tc>");
re!(FIRST_ROW, r"<a:tblPr\b[^>]*\bfirstRow=\x221\x22");
re!(
    SOLID_HEX,
    r"<a:solidFill>\s*<a:srgbClr\s+val=\x22([0-9A-Fa-f]{6})\x22"
);
re!(
    SOLID_SCHEME,
    r"<a:solidFill>\s*<a:schemeClr\s+val=\x22([^\x22]+)\x22"
);
re!(LST_STYLE, r"<a:lstStyle\b[\s\S]*?</a:lstStyle>");
re!(LATIN, r"<a:latin\b[^>]*typeface=\x22([^\x22]+)\x22");
re!(SPC_BEF, r"<a:spcBef>\s*<a:spcPts\b[^>]*val=\x22(\d+)\x22");
re!(SPC_AFT, r"<a:spcAft>\s*<a:spcPts\b[^>]*val=\x22(\d+)\x22");
re!(LN_SPC, r"<a:lnSpc>\s*<a:spcPct\b[^>]*val=\x22(\d+)\x22");
re!(BOLD, r"\bb=\x221\x22");
re!(ITALIC, r"\bi=\x221\x22");
re!(SRC_RECT, r"<a:srcRect\b[^>]*>");
re!(
    CHILD_OPEN,
    r"<(p:sp|p:pic|p:graphicFrame|p:grpSp|p:cxnSp)\b"
);
re!(
    REL_TARGETS,
    r"Id=\x22(rId[^\x22]+)\x22[^>]*Target=\x22([^\x22]+)\x22|Target=\x22([^\x22]+)\x22[^>]*Id=\x22(rId[^\x22]+)\x22"
);
re!(GRP_OPEN, r"^<p:grpSp\b[^>]*>");
re!(GRP_CLOSE, r"</p:grpSp>$");
re!(PH, r"<p:ph\b[^>]*>");
re!(SP_PR, r"<p:spPr\b[\s\S]*?</p:spPr>");
re!(LN, r"<a:ln\b[\s\S]*?</a:ln>");
re!(PRST, r"<a:prstGeom\b[^>]*prst=\x22([^\x22]+)\x22");
re!(BODY_PR, r"<a:bodyPr\b[^>]*>");
re!(TBL, r"<a:tbl\b[\s\S]*?</a:tbl>");
re!(BLIP, r"<a:blip\b[^>]*>");
re!(SHOW_MASTER, r"\bshowMasterSp=\x220\x22");
re!(BG, r"<p:bg\b[\s\S]*?</p:bg>");
re!(SCHEME_VAL, r"<a:schemeClr\s+val=\x22([^\x22]+)\x22");
re!(SRGB_VAL, r"<a:srgbClr\s+val=\x22([0-9A-Fa-f]{6})\x22");
re!(NEWLINE, r"\n");

static THEME_COLOR: LazyLock<FancyRegex> = LazyLock::new(|| {
    FancyRegex::new(r"<a:(dk1|lt1|dk2|lt2|accent[1-6]|hlink|folHlink)>[\s\S]*?</a:\1>")
        .expect("valid pattern")
});

fn decode_xml(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `xml.match(new RegExp(`\\b${name}="([^"]+)"`))?.[1]`.
pub fn attr<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let mut from = 0;
    while let Some(at) = xml[from..].find(&needle) {
        let start = from + at;
        let boundary = start == 0 || !is_word(xml[..start].chars().next_back().unwrap_or(' '));
        let value_start = start + needle.len();
        if boundary
            && let Some(end) = xml[value_start..].find('"')
            && end > 0
        {
            return Some(&xml[value_start..value_start + end]);
        }
        from = start + 1;
    }
    None
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// JavaScript `Number(text)`, or 0 when absent: `Number(attr(...)) || fallback`.
fn number(value: Option<&str>) -> f64 {
    value.map_or(f64::NAN, kairo_domain::js::string_to_number)
}

fn or(value: f64, fallback: f64) -> f64 {
    if value.is_nan() || value == 0.0 {
        fallback
    } else {
        value
    }
}

/// `Number.prototype.toFixed`: ties round away from zero on the exact value.
pub fn to_fixed(value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return num(value);
    }
    if value.abs() >= 1e21 {
        return num(value);
    }
    let scale = 10f64.powi(digits as i32);
    let scaled = value.abs() * scale;
    let floor = scaled.floor();
    let exact_tie = scaled - floor == 0.5 && (floor + 0.5) / scale == value.abs();
    let text = if exact_tie {
        let n = floor + 1.0;
        let whole = format!("{n:.0}");
        let padded = format!("{whole:0>width$}", width = digits + 1);
        if digits == 0 {
            padded
        } else {
            let (int, frac) = padded.split_at(padded.len() - digits);
            format!("{int}.{frac}")
        }
    } else {
        format!("{:.*}", digits, value.abs())
    };
    if value < 0.0 {
        format!("-{text}")
    } else {
        text
    }
}

pub fn slide_size(presentation_xml: &str) -> SlideSize {
    let Some(m) = SLD_SZ.find(presentation_xml) else {
        return SlideSize::default();
    };
    SlideSize {
        cx: or(number(attr(m.as_str(), "cx")), DEFAULT_CX),
        cy: or(number(attr(m.as_str(), "cy")), DEFAULT_CY),
    }
}

pub fn theme_colors(theme_xml: &str) -> Colors {
    let mut colors = Colors::new();
    for m in THEME_COLOR.captures_iter(theme_xml).flatten() {
        let whole = m.get(0).map_or("", |m| m.as_str());
        let hex = LAST_CLR
            .captures(whole)
            .or_else(|| VAL_HEX.captures(whole))
            .map(|c| c[1].to_owned());
        if let Some(hex) = hex {
            colors.insert(m[1].to_owned(), format!("#{hex}"));
        }
    }
    let get = |c: &Colors, k: &str, d: &str| c.get(k).cloned().unwrap_or_else(|| d.into());
    let bg1 = get(&colors, "lt1", "#FFFFFF");
    let bg2 = get(&colors, "lt2", "#EBEBEB");
    let tx1 = get(&colors, "dk1", "#111111");
    let tx2 = get(&colors, "dk2", "#1E5155");
    colors.insert("bg1".into(), bg1);
    colors.insert("bg2".into(), bg2);
    colors.insert("tx1".into(), tx1);
    colors.insert("tx2".into(), tx2);
    colors
}

fn emu_box(xml: &str, cx: f64, cy: f64) -> Option<SlideBox> {
    let xfrm = XFRM.find(xml)?.as_str();
    let off = OFF.find(xfrm)?.as_str();
    let ext = EXT.find(xfrm)?.as_str();
    let height = SLIDE_W * cy / cx;
    Some(SlideBox {
        x: or(number(attr(off, "x")), 0.0) / cx * SLIDE_W,
        y: or(number(attr(off, "y")), 0.0) / cy * height,
        w: or(number(attr(ext, "cx")), 0.0) / cx * SLIDE_W,
        h: or(number(attr(ext, "cy")), 0.0) / cy * height,
    })
}

fn paragraph_text(xml: &str) -> String {
    A_T.captures_iter(xml)
        .map(|c| kairo_domain::js::trim(&WS.replace_all(&decode_xml(&c[1]), " ")).to_owned())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn block_lines(xml: &str) -> Vec<String> {
    PARA.find_iter(xml)
        .map(|m| paragraph_text(m.as_str()))
        .filter(|t| !t.is_empty())
        .collect()
}

fn font_pt(xml: &str, fallback: f64) -> f64 {
    SZ.captures(xml).map_or(fallback, |c| {
        kairo_domain::js::string_to_number(&c[1]) / 100.0
    })
}

fn align(xml: &str) -> &'static str {
    match ALGN
        .captures(xml)
        .as_ref()
        .map(|c| c.get(1).map_or("", |m| m.as_str()))
    {
        Some("ctr") => "center",
        Some("r") => "right",
        _ => "left",
    }
}

fn parse_table(xml: &str, frame: SlideBox) -> SlideElement {
    let rows = TR
        .find_iter(xml)
        .map(|row| {
            TC.find_iter(row.as_str())
                .map(|cell| block_lines(cell.as_str()).join("\n"))
                .collect()
        })
        .collect();
    SlideElement::Table {
        frame,
        header: FIRST_ROW.is_match(xml),
        rows,
    }
}

fn resolve_color(xml: &str, colors: &Colors, fallback: &str) -> String {
    if let Some(c) = SOLID_HEX.captures(xml) {
        return format!("#{}", &c[1]);
    }
    if let Some(c) = SOLID_SCHEME.captures(xml)
        && let Some(color) = colors.get(&c[1])
    {
        return color.clone();
    }
    fallback.to_owned()
}

fn text_lines(xml: &str, colors: &Colors, inherited: &str) -> Vec<SlideLine> {
    let defaults = LST_STYLE.find(xml).map_or("", |m| m.as_str());
    let base = if defaults.is_empty() {
        inherited
    } else {
        defaults
    };
    let tx1 = colors
        .get("tx1")
        .cloned()
        .unwrap_or_else(|| "#111111".into());
    let fallback = resolve_color(base, colors, &tx1);
    let both = format!("{defaults}{inherited}");
    let mut out = Vec::new();
    for m in PARA.find_iter(xml) {
        let paragraph = m.as_str();
        let text = paragraph_text(paragraph);
        if text.is_empty() {
            continue;
        }
        let font_family = LATIN
            .captures(paragraph)
            .or_else(|| LATIN.captures(&both))
            .map_or_else(|| "Arial".to_owned(), |c| c[1].to_owned());
        let before = SPC_BEF.captures(paragraph).map(|c| c[1].to_owned());
        let after = SPC_AFT.captures(paragraph).map(|c| c[1].to_owned());
        let spacing = SPC_PCT(paragraph);
        let combined = format!("{paragraph}{defaults}{inherited}");
        out.push(SlideLine {
            text,
            color: resolve_color(paragraph, colors, &fallback),
            size_pt: font_pt(paragraph, font_pt(base, 24.0)),
            font_family,
            bold: BOLD.is_match(&combined),
            italic: ITALIC.is_match(paragraph),
            before_pt: before.map_or(0.0, |v| kairo_domain::js::string_to_number(&v)) / 100.0,
            after_pt: after.map_or(0.0, |v| kairo_domain::js::string_to_number(&v)) / 100.0,
            line_height: spacing.map_or(100_000.0, |v| kairo_domain::js::string_to_number(&v))
                / 100_000.0,
        });
    }
    out
}

#[allow(non_snake_case)]
fn SPC_PCT(paragraph: &str) -> Option<String> {
    LN_SPC.captures(paragraph).map(|c| c[1].to_owned())
}

fn image_crop(xml: &str) -> Option<Crop> {
    let rect = SRC_RECT.find(xml)?.as_str();
    let part = |name: &str| {
        let v = attr(rect, name).map_or(0.0, kairo_domain::js::string_to_number);
        (if v.is_nan() { 0.0 } else { v }) / 100_000.0
    };
    let crop = Crop {
        t: part("t"),
        b: part("b"),
        l: part("l"),
        r: part("r"),
    };
    if crop.t == 0.0 && crop.b == 0.0 && crop.l == 0.0 && crop.r == 0.0 {
        return None;
    }
    Some(crop)
}

/// The element starting at `start`, balanced on its own tag name.
fn slice_tagged<'a>(xml: &'a str, start: usize, tag: &str) -> Option<&'a str> {
    // The tag set is small (`p:sp`, `p:pic`, ...), so compile each pair once.
    static PATTERNS: LazyLock<std::sync::Mutex<std::collections::HashMap<String, (Regex, Regex)>>> =
        LazyLock::new(Default::default);
    let (self_closing, token) = {
        let mut cache = kairo_domain::js::lock(&PATTERNS);
        if !cache.contains_key(tag) {
            let pair = (
                Regex::new(&format!(r"^<{tag}\b[^>]*/>")).ok()?,
                Regex::new(&format!(r"<({tag}\b[^>]*/>|/?{tag}\b[^>]*>)")).ok()?,
            );
            cache.insert(tag.to_owned(), pair);
        }
        cache[tag].clone()
    };
    let rest = &xml[start..];
    if let Some(m) = self_closing.find(rest) {
        return Some(m.as_str());
    }
    let mut depth: i64 = 0;
    for c in token.captures_iter(rest) {
        let whole = c.get(0)?;
        let inner = c.get(1)?.as_str();
        if inner.starts_with('/') {
            depth -= 1;
        } else if inner.ends_with("/>") {
            if depth == 0 {
                return Some(&rest[..whole.end()]);
            }
        } else {
            depth += 1;
        }
        if depth == 0 {
            return Some(&rest[..whole.end()]);
        }
    }
    None
}

fn each_child(xml: &str) -> Vec<(String, &str)> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(c) = CHILD_OPEN.captures_at(xml, at) {
        let m = c.get(0).expect("whole match");
        let tag = c[1].to_owned();
        match slice_tagged(xml, m.start(), &tag) {
            Some(sliced) => {
                at = m.start() + sliced.len();
                found.push((tag, sliced));
            }
            None => at = m.end(),
        }
        if at > xml.len() {
            break;
        }
    }
    found
}

fn rel_targets(rels: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for c in REL_TARGETS.captures_iter(rels) {
        let id = c.get(1).or(c.get(4)).map(|m| m.as_str());
        let target = c.get(2).or(c.get(3)).map(|m| m.as_str());
        if let (Some(id), Some(target)) = (id, target) {
            let file = target.rsplit_once('/').map_or(target, |(_, f)| f);
            out.insert(id.to_owned(), file.to_owned());
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn collect_elements(
    xml: &str,
    size: SlideSize,
    embeds: &BTreeMap<String, String>,
    colors: &Colors,
    elements: &mut Vec<SlideElement>,
    inherited: &str,
    skip_placeholders: bool,
) {
    for (tag, child) in each_child(xml) {
        if tag == "p:grpSp" {
            let inner = GRP_OPEN.replace(child, "");
            let inner = GRP_CLOSE.replace(&inner, "");
            collect_elements(
                &inner,
                size,
                embeds,
                colors,
                elements,
                inherited,
                skip_placeholders,
            );
            continue;
        }
        if skip_placeholders && PH.is_match(child) {
            continue;
        }
        let ph = PH.find(child).map(|m| m.as_str());
        let inherited_shape = ph
            .and_then(|ph| {
                each_child(inherited)
                    .into_iter()
                    .find(|(_, item)| {
                        PH.find(item).is_some_and(|other| {
                            let other = other.as_str();
                            attr(ph, "idx").unwrap_or("0") == attr(other, "idx").unwrap_or("0")
                                && attr(ph, "type").unwrap_or("obj")
                                    == attr(other, "type").unwrap_or("obj")
                        })
                    })
                    .map(|(_, x)| x)
            })
            .unwrap_or("");
        let Some(frame) =
            emu_box(child, size.cx, size.cy).or_else(|| emu_box(inherited_shape, size.cx, size.cy))
        else {
            continue;
        };
        if tag == "p:sp" || tag == "p:cxnSp" {
            let properties = SP_PR.find(child).map_or("", |m| m.as_str());
            let line = LN.find(properties).map_or("", |m| m.as_str());
            let fill = resolve_color(&properties.replacen(line, "", 1), colors, "transparent");
            let border = resolve_color(line, colors, "transparent");
            if fill != "transparent" || border != "transparent" {
                elements.push(SlideElement::Shape {
                    frame,
                    fill,
                    border,
                    border_width: number(Some(attr(line, "w").unwrap_or("9525"))) * SLIDE_W
                        / size.cx,
                    geometry: PRST
                        .captures(properties)
                        .map_or_else(|| "rect".to_owned(), |c| c[1].to_owned()),
                });
            }
            let lines = text_lines(child, colors, inherited_shape);
            if lines.is_empty() {
                continue;
            }
            let body = BODY_PR.find(child).map_or("", |m| m.as_str());
            let px =
                |name: &str| number(Some(attr(body, name).unwrap_or("91440"))) * SLIDE_W / size.cx;
            elements.push(SlideElement::Text {
                frame,
                align: align(child),
                size_pt: lines[0].size_pt,
                anchor: match attr(body, "anchor") {
                    Some("b") => "bottom",
                    Some("ctr") => "center",
                    _ => "top",
                },
                insets: Insets {
                    top: px("tIns"),
                    right: px("rIns"),
                    bottom: px("bIns"),
                    left: px("lIns"),
                },
                lines,
            });
            continue;
        }
        if tag == "p:graphicFrame" {
            if let Some(table) = TBL.find(child) {
                elements.push(parse_table(table.as_str(), frame));
            }
            continue;
        }
        let blip = BLIP.find(child).map_or("", |m| m.as_str());
        let Some(file) = attr(blip, "r:embed").and_then(|e| embeds.get(e)) else {
            continue;
        };
        elements.push(SlideElement::Image {
            frame,
            file: file.clone(),
            crop: image_crop(child),
        });
    }
}

/// Layout and master parts a slide inherits from.
#[derive(Clone, Debug, Default)]
pub struct Inherited {
    pub master: String,
    pub layout: String,
    pub master_rels: String,
    pub layout_rels: String,
}

pub fn parse_slide(
    xml: &str,
    rels: &str,
    size: SlideSize,
    colors: &Colors,
    inherited: &Inherited,
) -> ParsedSlide {
    let mut elements = Vec::new();
    if !SHOW_MASTER.is_match(xml) {
        collect_elements(
            &inherited.master,
            size,
            &rel_targets(&inherited.master_rels),
            colors,
            &mut elements,
            "",
            true,
        );
        collect_elements(
            &inherited.layout,
            size,
            &rel_targets(&inherited.layout_rels),
            colors,
            &mut elements,
            "",
            true,
        );
    }
    let parent = format!("{}{}", inherited.layout, inherited.master);
    collect_elements(
        xml,
        size,
        &rel_targets(rels),
        colors,
        &mut elements,
        &parent,
        false,
    );
    let background = [xml, &inherited.layout, &inherited.master]
        .iter()
        .find_map(|part| BG.find(part).map(|m| m.as_str().to_owned()))
        .unwrap_or_default();
    let scheme = SCHEME_VAL.captures(&background).map(|c| c[1].to_owned());
    let hex = SRGB_VAL.captures(&background).map(|c| c[1].to_owned());
    let background = match hex {
        Some(hex) => format!("#{hex}"),
        None => colors
            .get(scheme.as_deref().unwrap_or("bg2"))
            .or(colors.get("bg2"))
            .cloned()
            .unwrap_or_else(|| "#EBEBEB".into()),
    };
    ParsedSlide {
        background,
        elements,
    }
}

fn box_style(b: &SlideBox) -> String {
    format!(
        "left:{}px;top:{}px;width:{}px;height:{}px",
        to_fixed(b.x, 1),
        to_fixed(b.y, 1),
        to_fixed(b.w, 1),
        to_fixed(b.h, 1)
    )
}

fn render_element(
    element: &SlideElement,
    media_href: &dyn Fn(&str) -> String,
    px_per_pt: f64,
) -> String {
    match element {
        SlideElement::Shape {
            frame,
            fill,
            border,
            border_width,
            geometry,
        } => {
            let rounded = match geometry.as_str() {
                "ellipse" => "border-radius:50%;",
                "roundRect" => "border-radius:12%;",
                _ => "",
            };
            let snip = if geometry == "snip2DiagRect" {
                "clip-path:polygon(0 0,50% 0,100% 50%,100% 100%,50% 100%,0 50%);"
            } else {
                ""
            };
            format!(
                "<div class=\"shape\" style=\"{};background:{};border:{}px solid {};{rounded}{snip}\"></div>",
                box_style(frame),
                escape_html(fill),
                num(*border_width),
                escape_html(border)
            )
        }
        SlideElement::Image { frame, file, crop } => {
            let src = escape_html(&media_href(file));
            let Some(crop) = crop else {
                return format!(
                    "<img class=\"pic\" style=\"{}\" src=\"{src}\" alt=\"\">",
                    box_style(frame)
                );
            };
            let width = 100.0 / (1.0 - crop.l - crop.r).max(0.01);
            let height = 100.0 / (1.0 - crop.t - crop.b).max(0.01);
            format!(
                "<div class=\"pic crop\" style=\"{}\"><img src=\"{src}\" alt=\"\" style=\"width:{}%;height:{}%;left:{}%;top:{}%\"></div>",
                box_style(frame),
                num(width),
                num(height),
                to_fixed(-crop.l * width, 2),
                to_fixed(-crop.t * height, 2)
            )
        }
        SlideElement::Text {
            frame,
            align,
            lines,
            anchor,
            insets,
            ..
        } => {
            let lines: String = lines
                .iter()
                .map(|line| {
                    format!(
                        "<div style=\"color:{};font-size:{}px;font-family:&quot;{}&quot;,Arial,sans-serif;font-weight:{};font-style:{};line-height:{};margin-top:{}px;margin-bottom:{}px\">{}</div>",
                        escape_html(&line.color),
                        to_fixed(line.size_pt * px_per_pt, 2),
                        escape_html(&line.font_family),
                        if line.bold { 700 } else { 400 },
                        if line.italic { "italic" } else { "normal" },
                        num(line.line_height),
                        num(line.before_pt * px_per_pt),
                        num(line.after_pt * px_per_pt),
                        escape_html(&line.text)
                    )
                })
                .collect();
            let justify = match *anchor {
                "bottom" => "flex-end",
                "center" => "center",
                _ => "flex-start",
            };
            format!(
                "<div class=\"text\" style=\"{};text-align:{align};justify-content:{justify};padding:{}px {}px {}px {}px\">{lines}</div>",
                box_style(frame),
                num(insets.top),
                num(insets.right),
                num(insets.bottom),
                num(insets.left)
            )
        }
        SlideElement::Table {
            frame,
            header,
            rows,
        } => {
            let rows: String = rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let tag = if *header && index == 0 { "th" } else { "td" };
                    let cells: String = row
                        .iter()
                        .map(|cell| {
                            format!(
                                "<{tag}>{}</{tag}>",
                                NEWLINE.replace_all(&escape_html(cell), "<br>")
                            )
                        })
                        .collect();
                    format!("<tr>{cells}</tr>")
                })
                .collect();
            format!(
                "<div class=\"table\" style=\"{}\"><table>{rows}</table></div>",
                box_style(frame)
            )
        }
    }
}

/// The printable HTML document: one `.slide` section per slide.
pub fn slide_html(
    slides: &[ParsedSlide],
    media_href: &dyn Fn(&str) -> String,
    colors: &Colors,
    size: SlideSize,
) -> String {
    let height = SLIDE_W * size.cy / size.cx;
    let accent = colors.get("accent1").map_or("#B01513", String::as_str);
    let ink = colors.get("tx1").map_or("#111111", String::as_str);
    let px_per_pt = 12700.0 * SLIDE_W / size.cx;
    let pages: String = slides
        .iter()
        .map(|slide| {
            let body: String = slide
                .elements
                .iter()
                .map(|e| render_element(e, media_href, px_per_pt))
                .collect();
            format!(
                "<section class=\"slide\" style=\"background:{}\">{body}</section>",
                escape_html(&slide.background)
            )
        })
        .collect();
    let w = num(SLIDE_W);
    let h = num(height);
    let accent = escape_html(accent);
    let ink = escape_html(ink);
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8">
<style>
  @page {{ size: {w}px {h}px; margin: 0 }}
  html, body {{ margin: 0; padding: 0; background: #000 }}
  .slide {{
    width: {w}px;
    height: {h}px;
    position: relative;
    overflow: hidden;
    page-break-after: always;
    break-after: page;
    font-family: "Calibri", "Segoe UI", system-ui, sans-serif;
  }}
  .text, .pic, .table, .shape {{ position: absolute; box-sizing: border-box }}
  .text {{ display: flex; flex-direction: column; overflow:hidden }}
  .text > div {{ flex-shrink: 0; white-space:pre-wrap }}
  .pic {{ object-fit: cover }}
  .pic.crop {{ overflow: hidden }}
  .pic.crop img {{ position: absolute; max-width: none }}
  .table {{ padding: 0 }}
  table {{ width: 100%; height: 100%; border-collapse: collapse; table-layout: fixed }}
  th, td {{
    border: 1px solid {accent};
    padding: 10px 14px;
    font-size: 22px;
    line-height: 1.25;
    color: {ink};
    background: #fff;
    vertical-align: middle;
    word-wrap: break-word;
  }}
  th {{
    background: {accent};
    color: #fff;
    font-size: 18px;
    letter-spacing: .04em;
    text-transform: uppercase;
  }}
  tr:nth-child(even) td {{ background: #f4f4f4 }}
</style></head><body>{pages}</body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_matches_javascript() {
        assert_eq!(to_fixed(0.25, 1), "0.3");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(-0.04, 1), "-0.0");
        assert_eq!(to_fixed(-0.0, 1), "0.0");
        assert_eq!(to_fixed(-1.25, 1), "-1.3");
        assert_eq!(to_fixed(12.0, 2), "12.00");
        assert_eq!(to_fixed(0.0, 1), "0.0");
    }

    #[test]
    fn attributes_need_a_word_boundary() {
        assert_eq!(attr(r#"<a idx="2" xidx="9">"#, "idx"), Some("2"));
        assert_eq!(attr(r#"<a xidx="9">"#, "idx"), None);
        assert_eq!(attr(r#"<a idx="">"#, "idx"), None);
    }

    #[test]
    fn theme_and_size_defaults() {
        let colors = theme_colors(
            r#"<a:accent1><a:srgbClr val="FF0000"/></a:accent1><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>"#,
        );
        assert_eq!(colors["accent1"], "#FF0000");
        assert_eq!(colors["tx1"], "#000000");
        assert_eq!(colors["bg1"], "#FFFFFF");
        assert_eq!(slide_size(""), SlideSize::default());
    }
}
