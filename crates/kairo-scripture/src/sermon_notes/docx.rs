//! A DOCX reader that replaces `mammoth` for sermon notes: the raw text
//! `extractRawText` gives (each paragraph followed by a blank line, tabs and
//! line breaks kept, deletions dropped) and HTML in the shape `convertToHtml`
//! emits with its default style map: `h1`–`h6` for Heading 1–6, `ul`/`ol`
//! lists nested by level, `strong`, `em`, `s`, `sup`, `sub`, links, tables
//! and `<br />`. Empty paragraphs are left out of the HTML, as mammoth does.

use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::io::Read;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const OFFICE_DOCUMENT_STRICT: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
const HYPERLINK: &str = "/hyperlink";

/// A part larger than this is refused rather than inflated (zip bombs).
const MAX_PART_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
pub struct DocxContent {
    pub text: String,
    pub html: String,
}

struct Archive {
    zip: zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
}

impl Archive {
    fn part(&mut self, name: &str) -> Result<Option<String>, String> {
        let name = name.trim_start_matches('/');
        let mut file = match self.zip.by_name(name) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        if file.size() > MAX_PART_BYTES {
            return Err(format!("{name} is too large"));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_PART_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_PART_BYTES {
            return Err(format!("{name} is too large"));
        }
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    }
}

fn relationships(xml: Option<&str>) -> Vec<(String, String, String, bool)> {
    let Some(xml) = xml else { return Vec::new() };
    let Ok(doc) = Document::parse(xml) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|n| n.has_tag_name("Relationship") || n.tag_name().name() == "Relationship")
        .map(|n| {
            (
                n.attribute("Id").unwrap_or_default().to_owned(),
                n.attribute("Type").unwrap_or_default().to_owned(),
                n.attribute("Target").unwrap_or_default().to_owned(),
                n.attribute("TargetMode") == Some("External"),
            )
        })
        .collect()
}

fn resolve(base_dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_owned();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

pub fn read(bytes: &[u8]) -> Result<DocxContent, String> {
    let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))
        .map_err(|e| format!("not a DOCX file ({e})"))?;
    let mut archive = Archive { zip };

    let root_rels = archive.part("_rels/.rels")?;
    let document_path = relationships(root_rels.as_deref())
        .into_iter()
        .find(|(_, kind, _, _)| kind == OFFICE_DOCUMENT || kind == OFFICE_DOCUMENT_STRICT)
        .map(|(_, _, target, _)| resolve("", &target))
        .unwrap_or_else(|| "word/document.xml".to_owned());
    let document_xml = archive
        .part(&document_path)?
        .ok_or_else(|| "the document has no main body (word/document.xml)".to_owned())?;
    let dir = document_path
        .rsplit_once('/')
        .map_or("", |(d, _)| d)
        .to_owned();
    let file = document_path
        .rsplit_once('/')
        .map_or(document_path.as_str(), |(_, f)| f);
    let rels_xml = archive.part(&format!("{dir}/_rels/{file}.rels"))?;
    let rels = relationships(rels_xml.as_deref());

    let mut links = HashMap::new();
    let mut styles_path = format!("{dir}/styles.xml");
    let mut numbering_path = format!("{dir}/numbering.xml");
    for (id, kind, target, external) in &rels {
        if kind.ends_with(HYPERLINK) {
            links.insert(
                id.clone(),
                if *external {
                    target.clone()
                } else {
                    resolve(&dir, target)
                },
            );
        } else if kind.ends_with("/styles") {
            styles_path = resolve(&dir, target);
        } else if kind.ends_with("/numbering") {
            numbering_path = resolve(&dir, target);
        }
    }
    let styles_xml = archive.part(&styles_path)?;
    let numbering_xml = archive.part(&numbering_path)?;

    let styles = Styles::parse(styles_xml.as_deref());
    let numbering = Numbering::parse(numbering_xml.as_deref());
    let doc =
        Document::parse(&document_xml).map_err(|e| format!("the document XML is damaged ({e})"))?;
    let body = doc
        .descendants()
        .find(|n| is_w(n, "body"))
        .ok_or_else(|| "the document has no body".to_owned())?;

    let mut converter = Converter {
        styles: &styles,
        numbering: &numbering,
        links: &links,
        text: String::new(),
        html: Html::default(),
    };
    converter.blocks(body);
    converter.html.close_lists(0);
    Ok(DocxContent {
        text: converter.text,
        html: converter.html.out,
    })
}

fn is_w(node: &Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace() == Some(W)
}

fn w_attr<'a>(node: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attribute((W, name)).or_else(|| node.attribute(name))
}

fn child<'a, 'i>(node: &Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| is_w(c, name))
}

/// `<w:b/>` is on; `w:val` of false, 0, off or none turns it off.
fn toggle(props: Option<Node>, name: &str) -> bool {
    props
        .and_then(|p| child(&p, name))
        .is_some_and(|n| !matches!(w_attr(&n, "val"), Some("false" | "0" | "off" | "none")))
}

#[derive(Default)]
struct Styles {
    paragraph_names: HashMap<String, String>,
    paragraph_numbering: HashMap<String, (String, u32)>,
    character_names: HashMap<String, String>,
}

impl Styles {
    fn parse(xml: Option<&str>) -> Self {
        let mut styles = Self::default();
        let Some(Ok(doc)) = xml.map(Document::parse) else {
            return styles;
        };
        for style in doc.descendants().filter(|n| is_w(n, "style")) {
            let Some(id) = w_attr(&style, "styleId") else {
                continue;
            };
            let name = child(&style, "name")
                .and_then(|n| w_attr(&n, "val"))
                .unwrap_or(id)
                .to_owned();
            match w_attr(&style, "type") {
                Some("paragraph") => {
                    if let Some(num) = child(&style, "pPr").and_then(|p| child(&p, "numPr"))
                        && let Some(num_id) = child(&num, "numId").and_then(|n| w_attr(&n, "val"))
                    {
                        let level = child(&num, "ilvl")
                            .and_then(|n| w_attr(&n, "val"))
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        styles
                            .paragraph_numbering
                            .insert(id.to_owned(), (num_id.to_owned(), level));
                    }
                    styles.paragraph_names.insert(id.to_owned(), name);
                }
                Some("character") => {
                    styles.character_names.insert(id.to_owned(), name);
                }
                _ => {}
            }
        }
        styles
    }
}

#[derive(Default)]
struct Numbering {
    /// numId to abstractNumId.
    nums: HashMap<String, String>,
    /// (abstractNumId, level) to "is ordered".
    levels: HashMap<(String, u32), bool>,
}

impl Numbering {
    fn parse(xml: Option<&str>) -> Self {
        let mut numbering = Self::default();
        let Some(Ok(doc)) = xml.map(Document::parse) else {
            return numbering;
        };
        for abs in doc.descendants().filter(|n| is_w(n, "abstractNum")) {
            let Some(abs_id) = w_attr(&abs, "abstractNumId") else {
                continue;
            };
            for lvl in abs.children().filter(|n| is_w(n, "lvl")) {
                let level = w_attr(&lvl, "ilvl")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let format = child(&lvl, "numFmt")
                    .and_then(|n| w_attr(&n, "val"))
                    .unwrap_or("bullet");
                numbering
                    .levels
                    .insert((abs_id.to_owned(), level), format != "bullet");
            }
        }
        for num in doc.descendants().filter(|n| is_w(n, "num")) {
            if let (Some(id), Some(abs)) = (
                w_attr(&num, "numId"),
                child(&num, "abstractNumId").and_then(|n| w_attr(&n, "val")),
            ) {
                numbering.nums.insert(id.to_owned(), abs.to_owned());
            }
        }
        numbering
    }

    fn is_ordered(&self, num_id: &str, level: u32) -> bool {
        self.nums
            .get(num_id)
            .and_then(|abs| self.levels.get(&(abs.clone(), level)))
            .copied()
            .unwrap_or(false)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct RunStyle {
    bold: bool,
    italic: bool,
    strike: bool,
    sup: bool,
    sub: bool,
}

impl RunStyle {
    /// Outermost first: mammoth wraps strike, sub/sup, em, then strong around them.
    fn tags(self) -> Vec<&'static str> {
        let mut tags = Vec::new();
        if self.bold {
            tags.push("strong");
        }
        if self.italic {
            tags.push("em");
        }
        if self.sup {
            tags.push("sup");
        } else if self.sub {
            tags.push("sub");
        }
        if self.strike {
            tags.push("s");
        }
        tags
    }
}

#[derive(Clone, PartialEq)]
enum Inline {
    Text(RunStyle, Option<String>, String),
    Break(RunStyle, Option<String>),
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(text: &str) -> String {
    escape_text(text).replace('"', "&quot;")
}

#[derive(Default)]
struct Html {
    out: String,
    /// Open lists, outermost first: is ordered.
    lists: Vec<bool>,
}

impl Html {
    fn close_lists(&mut self, keep: usize) {
        while self.lists.len() > keep {
            let ordered = self.lists.pop().expect("open list");
            self.out.push_str("</li>");
            self.out.push_str(if ordered { "</ol>" } else { "</ul>" });
        }
    }

    fn list_item(&mut self, level: usize, ordered: bool) {
        let depth = level + 1;
        if self.lists.len() > depth {
            self.close_lists(depth);
        }
        if self.lists.len() == depth && self.lists[depth - 1] != ordered {
            self.close_lists(depth - 1);
        }
        if self.lists.len() == depth {
            self.out.push_str("</li><li>");
            return;
        }
        while self.lists.len() < depth {
            self.out
                .push_str(if ordered { "<ol><li>" } else { "<ul><li>" });
            self.lists.push(ordered);
        }
    }

    fn inlines(&mut self, inlines: &[Inline]) {
        let mut open_link: Option<&str> = None;
        let mut open_tags: Vec<&'static str> = Vec::new();
        for inline in inlines {
            let (style, link) = match inline {
                Inline::Text(s, l, _) | Inline::Break(s, l) => (*s, l.as_deref()),
            };
            let tags = style.tags();
            if open_link != link {
                for tag in open_tags.drain(..).rev() {
                    self.out.push_str(&format!("</{tag}>"));
                }
                if open_link.is_some() {
                    self.out.push_str("</a>");
                }
                if let Some(href) = link {
                    self.out
                        .push_str(&format!("<a href=\"{}\">", escape_attr(href)));
                }
                open_link = link;
            }
            if open_tags != tags {
                let common = open_tags
                    .iter()
                    .zip(&tags)
                    .take_while(|(a, b)| a == b)
                    .count();
                for tag in open_tags.drain(common..).rev() {
                    self.out.push_str(&format!("</{tag}>"));
                }
                for tag in &tags[common..] {
                    self.out.push_str(&format!("<{tag}>"));
                    open_tags.push(tag);
                }
            }
            match inline {
                Inline::Text(_, _, text) => self.out.push_str(&escape_text(text)),
                Inline::Break(..) => self.out.push_str("<br />"),
            }
        }
        for tag in open_tags.drain(..).rev() {
            self.out.push_str(&format!("</{tag}>"));
        }
        if open_link.is_some() {
            self.out.push_str("</a>");
        }
    }
}

struct Converter<'a> {
    styles: &'a Styles,
    numbering: &'a Numbering,
    links: &'a HashMap<String, String>,
    text: String,
    html: Html,
}

impl Converter<'_> {
    fn blocks(&mut self, parent: Node) {
        for node in parent.children().filter(Node::is_element) {
            if is_w(&node, "p") {
                self.paragraph(node);
            } else if is_w(&node, "tbl") {
                self.html.close_lists(0);
                self.table(node);
            } else if is_w(&node, "sdt") {
                if let Some(content) = child(&node, "sdtContent") {
                    self.blocks(content);
                }
            } else if is_w(&node, "customXml") || is_w(&node, "ins") || is_w(&node, "smartTag") {
                self.blocks(node);
            } else if node.tag_name().name() == "AlternateContent"
                && node.tag_name().namespace() == Some(MC)
                && let Some(fallback) = node.children().find(|c| c.tag_name().name() == "Fallback")
            {
                self.blocks(fallback);
            }
        }
    }

    fn table(&mut self, table: Node) {
        self.html.out.push_str("<table>");
        for row in table.children().filter(|n| is_w(n, "tr")) {
            let header = child(&row, "trPr").is_some_and(|p| child(&p, "tblHeader").is_some());
            self.html.out.push_str("<tr>");
            for cell in row.children().filter(|n| is_w(n, "tc")) {
                let tag = if header { "th" } else { "td" };
                let span = child(&cell, "tcPr")
                    .and_then(|p| child(&p, "gridSpan"))
                    .and_then(|g| w_attr(&g, "val"))
                    .and_then(|v| v.parse::<u32>().ok())
                    .filter(|v| *v > 1);
                match span {
                    Some(span) => self
                        .html
                        .out
                        .push_str(&format!("<{tag} colspan=\"{span}\">")),
                    None => self.html.out.push_str(&format!("<{tag}>")),
                }
                self.blocks(cell);
                self.html.close_lists(0);
                self.html.out.push_str(&format!("</{tag}>"));
            }
            self.html.out.push_str("</tr>");
        }
        self.html.out.push_str("</table>");
    }

    fn paragraph(&mut self, p: Node) {
        let props = child(&p, "pPr");
        let style_id = props
            .and_then(|pp| child(&pp, "pStyle"))
            .and_then(|s| w_attr(&s, "val"));
        let style_name = style_id
            .and_then(|id| self.styles.paragraph_names.get(id))
            .map(String::as_str)
            .or(style_id)
            .unwrap_or("");
        let direct_num = props.and_then(|pp| child(&pp, "numPr")).and_then(|num| {
            let id = child(&num, "numId")
                .and_then(|n| w_attr(&n, "val"))?
                .to_owned();
            let level = child(&num, "ilvl")
                .and_then(|n| w_attr(&n, "val"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            Some((id, level))
        });
        let num = direct_num
            .or_else(|| style_id.and_then(|id| self.styles.paragraph_numbering.get(id).cloned()))
            .filter(|(id, _)| id != "0");

        let mut inlines = Vec::new();
        self.inlines(p, RunStyle::default(), None, &mut inlines);
        for inline in &inlines {
            match inline {
                Inline::Text(_, _, t) => self.text.push_str(t),
                Inline::Break(..) => self.text.push('\n'),
            }
        }
        self.text.push_str("\n\n");

        if inlines.is_empty() {
            return;
        }
        let heading = style_name
            .strip_prefix("heading ")
            .or_else(|| style_name.strip_prefix("Heading "))
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=6).contains(n));
        match (&num, heading) {
            (Some((id, level)), None) => {
                let ordered = self.numbering.is_ordered(id, *level);
                self.html.list_item(*level as usize, ordered);
                self.html.inlines(&inlines);
            }
            (_, Some(level)) => {
                self.html.close_lists(0);
                self.html.out.push_str(&format!("<h{level}>"));
                self.html.inlines(&inlines);
                self.html.out.push_str(&format!("</h{level}>"));
            }
            (None, None) => {
                self.html.close_lists(0);
                self.html.out.push_str("<p>");
                self.html.inlines(&inlines);
                self.html.out.push_str("</p>");
            }
        }
    }

    fn inlines(&self, parent: Node, _style: RunStyle, link: Option<&str>, out: &mut Vec<Inline>) {
        for node in parent.children().filter(Node::is_element) {
            if is_w(&node, "r") {
                self.run(node, link, out);
            } else if is_w(&node, "hyperlink") {
                let href = node
                    .attribute((R, "id"))
                    .and_then(|id| self.links.get(id))
                    .cloned()
                    .or_else(|| w_attr(&node, "anchor").map(|a| format!("#{a}")));
                let href = href.as_deref().or(link);
                self.inlines(node, _style, href, out);
            } else if is_w(&node, "ins")
                || is_w(&node, "smartTag")
                || is_w(&node, "fldSimple")
                || is_w(&node, "customXml")
            {
                self.inlines(node, _style, link, out);
            } else if is_w(&node, "sdt") {
                if let Some(content) = child(&node, "sdtContent") {
                    self.inlines(content, _style, link, out);
                }
            } else if node.tag_name().name() == "AlternateContent"
                && node.tag_name().namespace() == Some(MC)
                && let Some(fallback) = node.children().find(|c| c.tag_name().name() == "Fallback")
            {
                self.inlines(fallback, _style, link, out);
            }
            // w:del, w:pPr, bookmarks and comments carry no text.
        }
    }

    fn run(&self, run: Node, link: Option<&str>, out: &mut Vec<Inline>) {
        let props = child(&run, "rPr");
        let character_style = props
            .and_then(|p| child(&p, "rStyle"))
            .and_then(|s| w_attr(&s, "val"))
            .map(|id| {
                self.styles
                    .character_names
                    .get(id)
                    .map_or(id, String::as_str)
            });
        let vertical = props
            .and_then(|p| child(&p, "vertAlign"))
            .and_then(|v| w_attr(&v, "val"));
        let style = RunStyle {
            bold: toggle(props, "b") || character_style.is_some_and(|s| s == "Strong"),
            italic: toggle(props, "i"),
            strike: toggle(props, "strike"),
            sup: vertical == Some("superscript"),
            sub: vertical == Some("subscript"),
        };
        let link = link.map(str::to_owned);
        let push_text = |out: &mut Vec<Inline>, text: &str| {
            if let Some(Inline::Text(s, l, existing)) = out.last_mut()
                && *s == style
                && *l == link
            {
                existing.push_str(text);
                return;
            }
            out.push(Inline::Text(style, link.clone(), text.to_owned()));
        };
        for node in run.children().filter(Node::is_element) {
            if is_w(&node, "t") {
                push_text(out, node.text().unwrap_or_default());
            } else if is_w(&node, "tab") {
                push_text(out, "\t");
            } else if is_w(&node, "noBreakHyphen") {
                push_text(out, "-");
            } else if is_w(&node, "softHyphen") {
                push_text(out, "\u{AD}");
            } else if is_w(&node, "br") || is_w(&node, "cr") {
                // Page and column breaks are not line breaks.
                if matches!(w_attr(&node, "type"), None | Some("textWrapping")) {
                    out.push(Inline::Break(style, link.clone()));
                }
            } else if node.tag_name().name() == "AlternateContent"
                && node.tag_name().namespace() == Some(MC)
                && let Some(fallback) = node.children().find(|c| c.tag_name().name() == "Fallback")
            {
                self.run(fallback, link.as_deref(), out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    pub fn build(document: &str, numbering: Option<&str>, styles: Option<&str>) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: &str| {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        add(
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        );
        add(
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rL" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/?a=1&amp;b=2" TargetMode="External"/></Relationships>"#,
        );
        add("word/document.xml", document);
        if let Some(n) = numbering {
            add("word/numbering.xml", n);
        }
        if let Some(s) = styles {
            add("word/styles.xml", s);
        }
        zip.finish().unwrap().into_inner()
    }

    fn body(inner: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>{inner}</w:body></w:document>"#
        )
    }

    #[test]
    fn text_html_and_lists() {
        let numbering = format!(
            r#"<w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
        );
        let styles = format!(
            r#"<w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:styleId="H1"><w:name w:val="heading 1"/></w:style></w:styles>"#
        );
        let doc = body(
            r#"<w:p><w:pPr><w:pStyle w:val="H1"/></w:pPr><w:r><w:t>Grace</w:t></w:r></w:p>
            <w:p/>
            <w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">John 3:16 </w:t></w:r><w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>&amp; more</w:t></w:r><w:r><w:br/><w:t>line</w:t><w:tab/></w:r><w:del><w:r><w:t>gone</w:t></w:r></w:del></w:p>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>one</w:t></w:r></w:p>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:hyperlink r:id="rL"><w:r><w:t>link</w:t></w:r></w:hyperlink></w:p>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>two</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let out = read(&build(&doc, Some(&numbering), Some(&styles))).unwrap();
        assert_eq!(
            out.text,
            "Grace\n\n\n\nJohn 3:16 & more\nline\t\n\none\n\nlink\n\ntwo\n\ncell\n\n"
        );
        assert_eq!(
            out.html,
            "<h1>Grace</h1><p><strong>John 3:16 <em>&amp; more</em></strong><br />line\t</p>\
             <ol><li>one<ul><li><a href=\"https://example.com/?a=1&amp;b=2\">link</a></li></ul></li><li>two</li></ol>\
             <table><tr><td><p>cell</p></td></tr></table>"
        );
    }

    #[test]
    fn damaged_files_error() {
        assert!(read(b"not a zip").is_err());
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("x.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(read(&bytes).unwrap_err().contains("word/document.xml"));
        assert!(read(&build("<w:document", None, None)).is_err());
    }

    #[test]
    fn empty_document_has_empty_html() {
        let out = read(&build(&body(""), None, None)).unwrap();
        assert_eq!(out.text, "");
        assert_eq!(out.html, "");
    }
}
