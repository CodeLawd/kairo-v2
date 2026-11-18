//! Local script extraction on a blocking worker.

use std::{
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use roxmltree::Node;

use crate::DocumentError;

const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_XML_BYTES: usize = 8 * 1024 * 1024;
const MAX_NESTING: usize = 128;
use kairo_domain::teleprompter::MAX_SCRIPT_BYTES;
const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const STRICT_WORD: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const COMPAT: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const WORD_SHAPE: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";

#[derive(Debug)]
pub struct ScriptImport {
    pub text: String,
    pub warnings: Vec<String>,
}

pub async fn import_script(path: PathBuf) -> Result<ScriptImport, DocumentError> {
    tokio::task::spawn_blocking(move || extract_script(&path))
        .await
        .map_err(|error| {
            DocumentError::Message(format!("The script import worker stopped: {error}"))
        })?
}

fn extract_script(path: &Path) -> Result<ScriptImport, DocumentError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "docx" | "pdf") {
        return Err(message("Choose a Word (.docx) or PDF file."));
    }
    let bytes = read_bounded(
        File::open(path)?,
        MAX_FILE_BYTES,
        "Choose a file smaller than 64 MiB.",
    )?;
    if extension == "pdf" {
        return crate::pdf::extract_script(&bytes);
    }
    Ok(ScriptImport {
        text: extract_word(&bytes)?,
        warnings: Vec::new(),
    })
}

fn read_bounded(reader: impl Read, limit: usize, error: &str) -> Result<Vec<u8>, DocumentError> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(message(error));
    }
    Ok(bytes)
}

fn message(text: &str) -> DocumentError {
    DocumentError::Message(text.into())
}

fn extract_word(bytes: &[u8]) -> Result<String, DocumentError> {
    let invalid_word = |error: &dyn std::fmt::Display| {
        DocumentError::Message(format!("Could not read the Word document: {error}"))
    };
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| invalid_word(&e))?;
    let document = archive
        .by_name("word/document.xml")
        .map_err(|e| invalid_word(&e))?;
    let xml = read_bounded(
        document,
        MAX_XML_BYTES,
        "The Word document body exceeds 8 MiB.",
    )?;
    let xml = std::str::from_utf8(&xml).map_err(|e| invalid_word(&e))?;
    let document = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            nodes_limit: 500_000,
            ..Default::default()
        },
    )
    .map_err(|e| invalid_word(&e))?;
    let body = document
        .root_element()
        .children()
        .find(|node| word(*node) == Some("body"))
        .ok_or_else(|| message("The Word document has no body."))?;
    let mut output = String::new();
    extract_node(body, &mut output, &mut Vec::new(), 0)?;
    if output.trim().is_empty() {
        return Err(message("The Word document contains no editable text."));
    }
    if output.ends_with('\n') {
        output.pop();
    }
    Ok(output)
}

fn word<'a, 'input>(node: Node<'a, 'input>) -> Option<&'input str> {
    let name = node.tag_name();
    matches!(name.namespace(), Some(WORD | STRICT_WORD)).then(|| name.name())
}

fn extract_node(
    node: Node<'_, '_>,
    output: &mut String,
    fields: &mut Vec<bool>,
    depth: usize,
) -> Result<(), DocumentError> {
    if depth > MAX_NESTING {
        return Err(message(
            "The Word document contains too many nested elements.",
        ));
    }
    if node.has_tag_name((COMPAT, "AlternateContent")) {
        let branch = node
            .children()
            .find(|child| {
                child.has_tag_name((COMPAT, "Choice"))
                    && child.attribute("Requires").is_some_and(|requires| {
                        let mut prefixes = requires.split_whitespace().peekable();
                        prefixes.peek().is_some()
                            && prefixes.all(|p| {
                                matches!(
                                    child.lookup_namespace_uri(Some(p)),
                                    Some(WORD | STRICT_WORD | WORD_SHAPE)
                                )
                            })
                    })
            })
            .or_else(|| {
                node.children()
                    .find(|child| child.has_tag_name((COMPAT, "Fallback")))
            });
        if let Some(branch) = branch {
            extract_node(branch, output, fields, depth + 1)?;
        }
        return Ok(());
    }
    match word(node) {
        Some("del" | "moveFrom" | "instrText" | "delInstrText" | "object" | "sectPr") => {
            return Ok(());
        }
        Some("fldChar") => {
            match node
                .attribute((WORD, "fldCharType"))
                .or_else(|| node.attribute((STRICT_WORD, "fldCharType")))
            {
                Some("begin") => {
                    if fields.len() >= MAX_NESTING {
                        return Err(message(
                            "The Word document contains too many nested fields.",
                        ));
                    }
                    fields.push(false);
                }
                Some("separate") => {
                    if let Some(field) = fields.last_mut() {
                        *field = true;
                    }
                }
                Some("end") => {
                    fields.pop();
                }
                _ => {}
            }
        }
        Some("t" | "tab" | "br" | "cr" | "softHyphen" | "noBreakHyphen")
            if fields.contains(&false) => {}
        Some("t") => output.push_str(node.text().unwrap_or_default()),
        Some("tab") => output.push('\t'),
        Some("br" | "cr") => output.push('\n'),
        Some("softHyphen") => output.push('\u{ad}'),
        Some("noBreakHyphen") => output.push('\u{2011}'),
        kind => {
            let start = output.len();
            for child in node.children().filter(Node::is_element) {
                extract_node(child, output, fields, depth + 1)?;
            }
            match kind {
                Some("p") => output.push('\n'),
                Some("tc") => {
                    if output.len() > start && output.ends_with('\n') {
                        output.pop();
                    }
                    output.push('\t');
                }
                Some("tr") => {
                    if output.len() > start && output.ends_with('\t') {
                        output.pop();
                    }
                    output.push('\n');
                }
                _ => {}
            }
        }
    }
    if output.len() > MAX_SCRIPT_BYTES {
        return Err(message("The extracted script exceeds 1 MiB."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn word_file(body: &str) -> Vec<u8> {
        let xml = format!(
            r#"<w:document xmlns:w="{WORD}" xmlns:mc="{COMPAT}" xmlns:wps="{WORD_SHAPE}"><w:body>{body}</w:body></w:document>"#
        );
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(
                "word/document.xml",
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        archive.write_all(xml.as_bytes()).unwrap();
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn imports_body_paragraphs_tables_and_unicode() {
        let file = word_file(
            r#"
            <w:p><w:r><w:t xml:space="preserve">Olá </w:t><w:tab/><w:t>世界</w:t><w:softHyphen/><w:br/><w:t>next</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>first</w:t></w:r></w:p></w:tc>
            <w:tc><w:p><w:r><w:t>second</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            <w:p><w:r><w:t>end</w:t></w:r></w:p>
        "#,
        );
        assert_eq!(
            extract_word(&file).unwrap(),
            "Olá \t世界\u{ad}\nnext\nfirst\tsecond\nend"
        );
    }

    #[test]
    fn extracts_one_text_box_and_only_visible_field_results() {
        let file = word_file(
            r#"
            <w:p>
                <mc:AlternateContent>
                    <mc:Choice Requires="wps"><wps:wsp><w:txbxContent><w:p><w:r><w:t>Box</w:t></w:r></w:p></w:txbxContent></wps:wsp></mc:Choice>
                    <mc:Fallback><w:r><w:t>Duplicate</w:t></w:r></mc:Fallback>
                </mc:AlternateContent>
            </w:p>
            <w:p>
                <w:r><w:fldChar w:fldCharType="begin"/><w:instrText>DATE</w:instrText></w:r>
                <w:r><w:fldChar w:fldCharType="separate"/><w:t>November</w:t><w:fldChar w:fldCharType="end"/></w:r>
                <w:del><w:r><w:t>Deleted</w:t></w:r></w:del>
            </w:p>
        "#,
        );
        assert_eq!(extract_word(&file).unwrap(), "Box\n\nNovember");
    }

    #[test]
    fn rejects_corrupt_oversized_and_empty_word_documents() {
        assert!(extract_word(b"invalid zip").is_err());
        assert!(extract_word(&word_file("<w:p/>")).is_err());
        assert!(
            extract_word(&word_file(&" ".repeat(MAX_XML_BYTES)))
                .unwrap_err()
                .to_string()
                .contains("8 MiB")
        );
        let body = format!(
            "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
            "a".repeat(MAX_SCRIPT_BYTES + 1)
        );
        assert!(
            extract_word(&word_file(&body))
                .unwrap_err()
                .to_string()
                .contains("1 MiB")
        );
    }

    #[tokio::test]
    async fn saved_import_reopens_after_source_is_removed() {
        let profile = tempfile::tempdir().unwrap();
        let file = profile.path().join("Talk.DOCX");
        std::fs::write(&file, word_file("<w:p><w:r><w:t>Draft</w:t></w:r></w:p>")).unwrap();
        let preview = import_script(file.clone()).await.unwrap();
        let library = kairo_store::teleprompter::ScriptLibrary::new(profile.path());
        let saved = library
            .save(None, "Talk".into(), preview.text.replace("Draft", "Edited"))
            .unwrap();
        std::fs::remove_file(file).unwrap();
        assert_eq!(library.load(&saved.id).unwrap().body, "Edited");
    }
}
