//! The PowerPoint package: slide order, embedded videos and the static parts
//! the built-in renderer needs. From `pptx-videos.ts` and the relationship
//! lookup in `pptx-builtin.ts`, with `zip` in place of `jszip` and
//! `roxmltree` in place of `cheerio`.

use crate::DocumentError;
use kairo_domain::documents::{DocumentBox, DocumentVideo, format_document_size};
use regex::Regex;
use roxmltree::{Document, Node};
use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const MB: u64 = 1024 * 1024;

type Result<T> = std::result::Result<T, DocumentError>;

fn err<T>(message: impl Into<String>) -> Result<T> {
    Err(DocumentError::Message(message.into()))
}

/// Descendants (not the node itself) whose local name is `name`.
fn tags<'a, 'i>(scope: Node<'a, 'i>, name: &str) -> Vec<Node<'a, 'i>> {
    scope
        .descendants()
        .skip(1)
        .filter(|n| n.is_element() && n.tag_name().name() == name)
        .collect()
}

fn tags_in<'a, 'i>(scopes: &[Node<'a, 'i>], name: &str) -> Vec<Node<'a, 'i>> {
    scopes.iter().flat_map(|s| tags(*s, name)).collect()
}

/// The first attribute whose local name is `name`.
fn attr<'a>(node: Option<Node<'a, '_>>, name: &str) -> Option<&'a str> {
    node?
        .attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

/// `Number(text)` with `undefined` as NaN.
fn number(value: Option<&str>) -> f64 {
    value.map_or(f64::NAN, kairo_domain::js::string_to_number)
}

fn parse_xml(xml: &str) -> Option<Document<'_>> {
    Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            ..Default::default()
        },
    )
    .ok()
}

#[derive(Clone, Debug)]
struct Relationship {
    target: String,
    external: bool,
}

fn relationships(xml: &str) -> HashMap<String, Relationship> {
    let mut out = HashMap::new();
    let Some(doc) = parse_xml(xml) else {
        return out;
    };
    for node in tags(doc.root(), "Relationship") {
        out.insert(
            attr(Some(node), "Id").unwrap_or("").to_owned(),
            Relationship {
                target: attr(Some(node), "Target").unwrap_or("").to_owned(),
                external: attr(Some(node), "TargetMode") == Some("External"),
            },
        );
    }
    out
}

/// `posix.normalize` of a package path.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn package_path(base: &str, target: &str) -> String {
    match target.strip_prefix('/') {
        Some(absolute) => normalize(absolute),
        None => normalize(&format!("{base}/{target}")),
    }
}

static SLIDE_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^ppt/slides/[^/\\]+\.xml$").unwrap());
static VIDEO_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^ppt/media/[^/\\]+\.(mp4|m4v|webm|mov|ogv)$").unwrap());
static ARTWORK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^ppt/media/[^/]+\.(png|jpe?g|gif|svg|bmp|webp|tiff?|emf|wmf)$").unwrap()
});
static XML_PART: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(xml|rels)$").unwrap());
static VIDEO_EXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(mp4|m4v|webm|mov|ogv)$").unwrap());

/// Slide parts in `sldIdLst` order, not the creation-time filenames.
pub fn pptx_slide_paths(xml: &str, rels_xml: &str) -> Result<Vec<String>> {
    let Some(doc) = parse_xml(xml) else {
        return err("Invalid PowerPoint slide relationship.");
    };
    let rels = relationships(rels_xml);
    let mut out = Vec::new();
    for node in tags_in(&tags(doc.root(), "sldIdLst"), "sldId") {
        // sldId also has a numeric unqualified id; select the relationship attribute.
        let id = node
            .attributes()
            .find(|a| a.namespace().is_some() && a.name() == "id")
            .map(|a| a.value());
        let Some(rel) = rels.get(id.unwrap_or("")).filter(|r| !r.external) else {
            return err("Invalid PowerPoint slide relationship.");
        };
        let path = package_path("ppt", &rel.target);
        if !SLIDE_PATH.is_match(&path) {
            return err("Invalid PowerPoint slide path.");
        }
        out.push(path);
    }
    Ok(out)
}

/// An opened PowerPoint archive.
pub struct Package {
    zip: zip::ZipArchive<Cursor<Vec<u8>>>,
}

impl Package {
    pub fn open(data: Vec<u8>) -> Result<Self> {
        let zip = zip::ZipArchive::new(Cursor::new(data))
            .map_err(|e| DocumentError::Message(e.to_string()))?;
        Ok(Self { zip })
    }

    fn names(&self) -> Vec<String> {
        self.zip.file_names().map(str::to_owned).collect()
    }

    /// Bounded decompression: an archive cannot expand without limit.
    fn entry_bytes(&mut self, path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>> {
        let Ok(mut entry) = self.zip.by_name(path) else {
            return Ok(None);
        };
        if entry.is_dir() {
            return Ok(None);
        }
        let declared = entry.size();
        let mut bytes = Vec::new();
        let read = (&mut entry)
            .take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| DocumentError::Message(e.to_string()))?;
        if read as u64 > max_bytes {
            let label = if XML_PART.is_match(path) {
                "PowerPoint metadata"
            } else if VIDEO_EXT.is_match(path) {
                "Embedded video"
            } else {
                "PowerPoint artwork"
            };
            let size = if declared > max_bytes {
                format!(" is {} and", format_document_size(declared as f64))
            } else {
                String::new()
            };
            let name = path.rsplit('/').next().unwrap_or(path);
            return err(format!(
                "{label} “{name}”{size} exceeds the {} MB limit. Reduce its size and import again.",
                (max_bytes as f64 / MB as f64).round()
            ));
        }
        Ok(Some(bytes))
    }

    /// The static fallback needs XML and slide artwork, never arbitrary contents.
    pub fn extract_static(&mut self, destination: &Path) -> Result<()> {
        let mut total = 0u64;
        for name in self.names() {
            if name.ends_with('/') || !name.starts_with("ppt/") {
                continue;
            }
            let safe = self
                .zip
                .by_name(&name)
                .map(|e| e.enclosed_name().is_some())
                .unwrap_or(false);
            if !safe || name.contains('\\') || name.split('/').any(|p| p == ".." || p == ".") {
                return err("Invalid PowerPoint archive path.");
            }
            let xml = XML_PART.is_match(&name);
            if !xml && !ARTWORK.is_match(&name) {
                continue;
            }
            let Some(bytes) = self.entry_bytes(&name, if xml { 5 } else { 20 } * MB)? else {
                continue;
            };
            total += bytes.len() as u64;
            if total > 100 * MB {
                return err("PowerPoint slide artwork exceeds 100 MB.");
            }
            let file: PathBuf = name
                .split('/')
                .fold(destination.to_path_buf(), |p, part| p.join(part));
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(file, bytes)?;
        }
        Ok(())
    }
}

/// What `readPptxVideos` found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PptxVideos {
    pub width: f64,
    pub height: f64,
    pub slides: Vec<Vec<DocumentVideo>>,
    pub media: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<String>,
}

pub fn read_pptx_videos(data: Vec<u8>) -> Result<PptxVideos> {
    let mut package = Package::open(data)?;
    let mut xml_bytes = 0u64;
    let mut xml = |package: &mut Package, path: &str| -> Result<String> {
        let bytes = package.entry_bytes(path, 5 * MB)?;
        xml_bytes += bytes.as_ref().map_or(0, |b| b.len() as u64);
        if xml_bytes > 50 * MB {
            return err("PowerPoint slide metadata exceeds 50 MB.");
        }
        Ok(bytes
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default())
    };
    let presentation = xml(&mut package, "ppt/presentation.xml")?;
    let rels_xml = xml(&mut package, "ppt/_rels/presentation.xml.rels")?;
    let paths = pptx_slide_paths(&presentation, &rels_xml)?;
    if paths.is_empty() || paths.len() > 500 {
        return err("PowerPoint must contain between 1 and 500 slides.");
    }
    let doc = parse_xml(&presentation);
    let size = doc
        .as_ref()
        .and_then(|d| tags(d.root(), "sldSz").into_iter().next());
    let or = |v: f64, d: f64| if v.is_nan() || v == 0.0 { d } else { v };
    let width = or(number(attr(size, "cx")), 12_192_000.0);
    let height = or(number(attr(size, "cy")), 6_858_000.0);
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return err("Invalid PowerPoint slide size.");
    }
    let mut out = PptxVideos {
        width,
        height,
        ..PptxVideos::default()
    };
    let mut total = 0u64;
    for (page, path) in paths.iter().enumerate() {
        let slide_xml = xml(&mut package, path)?;
        let (dir, base) = path.rsplit_once('/').unwrap_or(("", path));
        let rels = relationships(&xml(&mut package, &format!("{dir}/_rels/{base}.rels"))?);
        let mut videos = Vec::new();
        let Some(doc) = parse_xml(&slide_xml) else {
            out.slides.push(videos);
            continue;
        };
        for picture in tags(doc.root(), "pic") {
            let reference = tags(picture, "media").into_iter().next();
            let legacy = tags(picture, "videoFile").into_iter().next();
            if legacy.is_none() {
                continue; // Audio-only shapes are not document videos.
            }
            let key = attr(reference, "embed")
                .or_else(|| attr(legacy, "link"))
                .unwrap_or("");
            let mut warn =
                |reason: &str| out.warnings.push(format!("Slide {}: {reason}", page + 1));
            let Some(relation) = rels.get(key).filter(|r| !r.external) else {
                warn(
                    "Linked or online video cannot be played. Embed the video in PowerPoint and import again.",
                );
                continue;
            };
            let target = package_path(dir, &relation.target);
            if !VIDEO_PATH.is_match(&target) {
                warn(
                    "Video is missing or uses an unsupported format. Use an embedded MP4 or WebM file.",
                );
                continue;
            }
            let xfrm = tags_in(&tags(picture, "spPr"), "xfrm").into_iter().next();
            let grouped = picture
                .ancestors()
                .skip(1)
                .any(|a| a.is_element() && a.tag_name().name() == "grpSp");
            let rot = number(attr(xfrm, "rot"));
            if grouped
                || (rot != 0.0 && !rot.is_nan())
                || attr(xfrm, "flipH") == Some("1")
                || attr(xfrm, "flipV") == Some("1")
            {
                warn("Grouped, flipped or rotated video placement is not supported yet.");
                continue;
            }
            let off = xfrm.and_then(|x| tags(x, "off").into_iter().next());
            let ext = xfrm.and_then(|x| tags(x, "ext").into_iter().next());
            let frame = DocumentBox {
                x: number(attr(off, "x")) / width,
                y: number(attr(off, "y")) / height,
                width: number(attr(ext, "cx")) / width,
                height: number(attr(ext, "cy")) / height,
            };
            if ![frame.x, frame.y, frame.width, frame.height]
                .iter()
                .all(|v| v.is_finite())
                || frame.width <= 0.0
                || frame.height <= 0.0
            {
                warn("Video placement could not be read.");
                continue;
            }
            let overlapping = picture
                .next_siblings()
                .skip(1)
                .filter(|s| s.is_element())
                .any(|sibling| {
                    if !["sp", "pic", "graphicFrame", "grpSp", "cxnSp"]
                        .contains(&sibling.tag_name().name())
                    {
                        return false;
                    }
                    if !tags(sibling, "videoFile").is_empty() {
                        return false;
                    }
                    let transform = tags(sibling, "xfrm").into_iter().next();
                    let offset = transform.and_then(|t| tags(t, "off").into_iter().next());
                    let extent = transform.and_then(|t| tags(t, "ext").into_iter().next());
                    if offset.is_none() || extent.is_none() {
                        return true; // Layout-inherited geometry is unknown.
                    }
                    let x = number(attr(offset, "x")) / width;
                    let y = number(attr(offset, "y")) / height;
                    let w = number(attr(extent, "cx")) / width;
                    let h = number(attr(extent, "cy")) / height;
                    x < frame.x + frame.width
                        && x + w > frame.x
                        && y < frame.y + frame.height
                        && y + h > frame.y
                });
            if overlapping {
                warn(
                    "Video playback appears above overlapping slide text or shapes. Move those objects beside the video for reliable playback.",
                );
            }
            let file = target.rsplit('/').next().unwrap_or(&target).to_owned();
            if !out.media.contains_key(&file) {
                let Some(bytes) = package.entry_bytes(&target, 200 * MB)? else {
                    warn("Embedded video file is missing.");
                    continue;
                };
                total += bytes.len() as u64;
                if total > 500 * MB {
                    return err(
                        "Embedded videos exceed 500 MB. Split this presentation into smaller files.",
                    );
                }
                out.media.insert(file.clone(), bytes);
            }
            let trim = tags(picture, "trim").into_iter().next();
            let st = number(attr(trim, "st")) / 1000.0;
            let start = if st.is_nan() || st == 0.0 { 0.0 } else { st }.max(0.0);
            let end = number(attr(trim, "end")) / 1000.0;
            videos.push(DocumentVideo {
                file,
                frame,
                start,
                end: (end.is_finite() && end > start).then_some(end),
            });
        }
        out.slides.push(videos);
    }
    Ok(out)
}

/// The part a `.rels` relationship of `kind` points to, inside `root`.
pub fn related_part(root: &Path, file: &Path, kind: &str) -> Result<Option<PathBuf>> {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = file.parent().unwrap_or(root);
    let rels =
        std::fs::read_to_string(dir.join("_rels").join(format!("{name}.rels"))).unwrap_or_default();
    let Some(doc) = parse_xml(&rels) else {
        return Ok(None);
    };
    let suffix = format!("/{kind}");
    let Some(relation) = tags(doc.root(), "Relationship").into_iter().find(|n| {
        attr(Some(*n), "Type").is_some_and(|t| t.ends_with(&suffix))
            && attr(Some(*n), "TargetMode") != Some("External")
    }) else {
        return Ok(None);
    };
    let target = attr(Some(relation), "Target").unwrap_or("");
    let joined = match target.strip_prefix('/') {
        Some(absolute) => root.join(absolute),
        None => dir.join(target),
    };
    let resolved = lexical(&joined);
    if !resolved.starts_with(lexical(root)) {
        return err("Invalid PowerPoint style relationship.");
    }
    Ok(Some(resolved))
}

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slide_order_follows_the_id_list() {
        let presentation = r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="257" r:id="rId3"/><p:sldId id="256" r:id="rId2"/></p:sldIdLst></p:presentation>"#;
        let rels = r#"<Relationships><Relationship Id="rId2" Target="slides/slide1.xml"/><Relationship Id="rId3" Target="slides/slide2.xml"/></Relationships>"#;
        assert_eq!(
            pptx_slide_paths(presentation, rels).unwrap(),
            ["ppt/slides/slide2.xml", "ppt/slides/slide1.xml"]
        );
        let bad = r#"<Relationships><Relationship Id="rId2" Target="../../etc/x.xml"/><Relationship Id="rId3" Target="slides/slide2.xml"/></Relationships>"#;
        assert_eq!(
            pptx_slide_paths(presentation, bad).unwrap_err().to_string(),
            "Invalid PowerPoint slide path."
        );
    }

    #[test]
    fn package_paths_normalize() {
        assert_eq!(
            package_path("ppt/slides", "../media/v.mp4"),
            "ppt/media/v.mp4"
        );
        assert_eq!(package_path("ppt", "/ppt/slides/a.xml"), "ppt/slides/a.xml");
    }
}
