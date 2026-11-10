//! Prompt 042: sermon notes import. Differential against Electron's real
//! `readSermonDocument` and `analyzeScriptureReferences` (through
//! `tools/ts-oracle/modules/sermon-notes.ts`) on the corpus in
//! `tests/fixtures/sermon-notes` (written by its `generate.py`; rerun the
//! oracle in `refresh` mode after regenerating it).
//!
//! References are compared, not raw text: PDFium and pdf.js lay text out
//! differently. For the formats both read the same way (TXT, MD, RTF) the
//! text and match offsets must be identical too.

use kairo_scripture::sermon_notes::{
    ImportProgress, ReadProgress, SermonDocumentError, SermonImportError, UNSUPPORTED_DOCUMENT,
    analyze_scripture_references, import_sermon_notes, import_sermon_notes_blocking,
    read_sermon_document, read_sermon_document_with_progress, scan_sermon_notes,
    set_pdfium_library_dir,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const FIXTURES: &str = "crates/kairo-scripture/tests/fixtures/sermon-notes";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    workspace().join(FIXTURES).join(name)
}

/// The development copy of PDFium, `.cache/pdfium/lib` (git-ignored), or
/// `PDFIUM_DYNAMIC_LIB_PATH`. `false` when neither exists.
fn pdfium_available() -> bool {
    let dev = workspace().join(".cache/pdfium/lib");
    let lib = |dir: &Path| {
        dir.join(if cfg!(windows) {
            "pdfium.dll"
        } else if cfg!(target_os = "macos") {
            "libpdfium.dylib"
        } else {
            "libpdfium.so"
        })
    };
    if lib(&dev).is_file() {
        set_pdfium_library_dir(dev);
        return true;
    }
    std::env::var_os("PDFIUM_DYNAMIC_LIB_PATH").is_some_and(|d| lib(Path::new(&d)).is_file())
}

fn require_pdfium() -> bool {
    let ok = pdfium_available();
    if !ok {
        assert!(
            std::env::var_os("KAIRO_REQUIRE_PDFIUM").is_none(),
            "KAIRO_REQUIRE_PDFIUM is set but PDFium was not found"
        );
        eprintln!("PDFium not found: PDF cases skipped (see PROGRESS.md, prompt 042)");
    }
    ok
}

const SERMONS: [&str; 4] = ["grace", "shepherd", "faith", "hope"];
const FORMATS: [&str; 5] = ["docx", "pdf", "txt", "md", "rtf"];
const EDGE: [&str; 12] = [
    "empty.txt",
    "empty.docx",
    "blank-page.pdf",
    "corrupt.pdf",
    "corrupt.docx",
    "zero-bytes.docx",
    "no-document.docx",
    "notes.odt",
    "bom-and-invalid.txt",
    "rtf-markup.txt",
    "SHOUTING.TXT",
    "crlf.md",
];

fn items(v: &Value) -> Vec<(String, String)> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["reference"].as_str().unwrap().into(),
                i["translation"].as_str().unwrap().into(),
            )
        })
        .collect()
}

fn matches(v: &Value) -> Vec<(String, String, Value)> {
    v["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["text"].as_str().unwrap().into(),
                m["reference"].as_str().unwrap().into(),
                m["translations"].clone(),
            )
        })
        .collect()
}

#[test]
fn detected_references_match_electron_for_every_format() {
    let with_pdf = require_pdfium();
    let mut files: Vec<String> = SERMONS
        .iter()
        .flat_map(|s| FORMATS.iter().map(move |f| format!("{s}.{f}")))
        .chain(EDGE.iter().map(|s| s.to_string()))
        .collect();
    files.sort();
    let relative: Vec<String> = files.iter().map(|f| format!("{FIXTURES}/{f}")).collect();
    let expected = kairo_oracle::oracle_value("oracle:sermon-notes.read", json!([relative, "KJV"]));
    let expected = expected.as_array().expect("oracle list");

    let mut compared = 0;
    let mut references = 0;
    for (name, want) in files.iter().zip(expected) {
        let is_pdf = name.to_ascii_lowercase().ends_with(".pdf");
        if is_pdf && !with_pdf {
            continue;
        }
        let got = read_sermon_document(&fixture(name));
        if let Some(message) = want.get("error") {
            let error = got.expect_err(&format!(
                "{name}: Electron failed with {message}, native read it"
            ));
            if message == UNSUPPORTED_DOCUMENT {
                assert_eq!(error.to_string(), UNSUPPORTED_DOCUMENT, "{name}");
            }
            compared += 1;
            continue;
        }
        let content = got.unwrap_or_else(|e| panic!("{name}: {e}"));
        let analysis =
            serde_json::to_value(analyze_scripture_references(&content.text, "KJV")).unwrap();
        assert_eq!(items(&analysis), items(want), "{name}: items");
        assert_eq!(matches(&analysis), matches(want), "{name}: matches");
        let plain = [".txt", ".md", ".rtf"]
            .iter()
            .any(|e| name.to_ascii_lowercase().ends_with(e));
        if plain {
            assert_eq!(content.text, want["text"].as_str().unwrap(), "{name}: text");
            assert_eq!(content.html, want["html"].as_str().unwrap(), "{name}: html");
            let diff = kairo_oracle::compare(&want["matches"], &analysis["matches"]);
            assert!(diff.is_empty(), "{name}: offsets {diff:?}");
        }
        references += items(want).len();
        compared += 1;
    }
    assert!(
        references > 100,
        "corpus too small: {references} references"
    );
    eprintln!("{compared} files compared, {references} references");
}

#[test]
fn docx_text_and_html_match_mammoth_on_the_corpus() {
    let files: Vec<String> = SERMONS
        .iter()
        .map(|s| format!("{FIXTURES}/{s}.docx"))
        .collect();
    let expected = kairo_oracle::oracle_value("oracle:sermon-notes.read", json!([files, "KJV"]));
    for (file, want) in files.iter().zip(expected.as_array().unwrap()) {
        let got = read_sermon_document(&workspace().join(file)).unwrap();
        assert_eq!(got.text, want["text"].as_str().unwrap(), "{file}: text");
        assert_eq!(got.html, want["html"].as_str().unwrap(), "{file}: html");
    }
}

/// Prose, punctuation and notation variants, scanned as text (`scanSermonNotes`).
fn text_corpus() -> Vec<String> {
    let references = [
        "John 3:16",
        "Jn 3:16",
        "1 John 1:9",
        "I John 1:9",
        "First John 1:9",
        "Ps 23:1-6",
        "Psalm 23 1",
        "Song of Songs 2:4",
        "Rev. 21:4",
        "Romans 8:28–30",
        "Rom 8:28 — 39",
        "Gen 1:1,2",
        "Acts 2:14, 41",
        "Acts 2:38; 3:19",
        "Matt 5:3; 6",
        "2Cor 5:17",
        "2 cor 5 : 17",
        "Phil 4:13 (NIV)",
        "NKJV Isaiah 53:5",
        "Heb 11:1 [ESV]",
        "James 1:2-4 NIV/ESV",
        "Luke 4:18 kjv, amp",
        "Obad 1:15",
        "Jude 1:24, 25",
        "1 Cor 13:4",
        "Mark 1:1, 2 Kings 2:11",
        "Mark 1:1, 2 Sam 7:12",
        "John 3:16-17, 18-21, 4:1",
        "Ex 20:1-17; Deut 5:6",
        "Psalm 119:105, 106; 130:1",
        "Eph 2:8 (NASB, NLT)",
        "Joel 2:28,",
        "John 1:1234",
        "Gen 1:1:2",
        "Rom 1:1; 2",
        "Prov 3:5 , 6",
        "Zeph 3:17 MSG",
        "1Pet 5:7",
    ];
    let frames = [
        "{}",
        "Read {} today.",
        "({})",
        "We open with {} and close.",
        "{}\n{}",
        "Text: {}; then prayer.",
        "😀 {} 😀",
        "{}\t{}",
        "  {}  ",
        "Notes:\r\n{}\r\n",
    ];
    let mut out = Vec::new();
    for (i, r) in references.iter().enumerate() {
        for (j, f) in frames.iter().enumerate() {
            let other = references[(i + j + 1) % references.len()];
            out.push(f.replacen("{}", r, 1).replacen("{}", other, 1));
        }
    }
    out.push(String::new());
    out.push("No scripture here, just 10:30 and 3 songs.".into());
    out.push(references.join(" "));
    out
}

#[test]
fn scanned_text_matches_electron_exactly() {
    let corpus = text_corpus();
    let cases: Vec<(Value, Value)> = corpus
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let translation = if i % 3 == 0 { "NIV" } else { "KJV" };
            let mut got = serde_json::to_value(scan_sermon_notes(text, translation)).unwrap();
            // Ids carry `Date.now()`, pinned in the oracle; compare positions only.
            for (n, item) in got["items"].as_array_mut().unwrap().iter_mut().enumerate() {
                item["id"] = json!(format!("verse-{}-1700000000000", n + 1));
            }
            (json!([text, translation]), got)
        })
        .collect();
    let inputs: Vec<Value> = cases.iter().map(|(a, _)| a.clone()).collect();
    let expected = kairo_oracle::oracle_value("oracle:sermon-notes.analyze", json!([inputs]));
    let expected = expected.as_array().unwrap();
    let mut failures = Vec::new();
    for ((args, got), want) in cases.iter().zip(expected) {
        let diff = kairo_oracle::compare(want, got);
        if !diff.is_empty() {
            failures.push(format!("{args}: {}", diff.join("; ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(8)].join("\n")
    );
}

#[test]
fn errors_and_empty_documents() {
    let unsupported = read_sermon_document(&fixture("notes.odt")).unwrap_err();
    assert!(matches!(unsupported, SermonDocumentError::Unsupported));
    assert_eq!(unsupported.to_string(), UNSUPPORTED_DOCUMENT);
    assert!(matches!(
        read_sermon_document(Path::new("no-extension")),
        Err(SermonDocumentError::Unsupported)
    ));
    assert!(matches!(
        read_sermon_document(&fixture("missing.txt")),
        Err(SermonDocumentError::Io { .. })
    ));
    for bad in ["corrupt.docx", "zero-bytes.docx", "no-document.docx"] {
        assert!(
            matches!(
                read_sermon_document(&fixture(bad)),
                Err(SermonDocumentError::Docx(_))
            ),
            "{bad}"
        );
    }
    let empty = read_sermon_document(&fixture("empty.txt")).unwrap();
    assert_eq!(
        (empty.text.as_str(), empty.html.as_str()),
        ("", "<p><br></p>")
    );
    let empty_docx = read_sermon_document(&fixture("empty.docx")).unwrap();
    assert_eq!(
        (empty_docx.text.as_str(), empty_docx.html.as_str()),
        ("", "<p><br></p>")
    );
    if require_pdfium() {
        assert!(matches!(
            read_sermon_document(&fixture("corrupt.pdf")),
            Err(SermonDocumentError::Pdf(_))
        ));
        let blank = read_sermon_document(&fixture("blank-page.pdf")).unwrap();
        assert_eq!(blank.text.trim(), "");
    }
}

#[test]
fn import_builds_a_draft_with_progress() {
    let mut seen = Vec::new();
    let draft =
        import_sermon_notes_blocking(&fixture("grace.txt"), "KJV", &mut |p| seen.push(p)).unwrap();
    assert_eq!(draft.title, "grace");
    assert_eq!(draft.source_file_name, "grace.txt");
    assert!(
        draft
            .items
            .iter()
            .any(|i| i.reference == "Ephesians 2:8–9" && i.translation == "NKJV")
    );
    assert_eq!(
        seen,
        vec![
            ImportProgress::Reading(ReadProgress { done: 1, total: 1 }),
            ImportProgress::Scanning
        ]
    );

    if require_pdfium() {
        let mut pages = Vec::new();
        read_sermon_document_with_progress(&fixture("hope.pdf"), &mut |p| pages.push(p)).unwrap();
        assert_eq!(
            pages,
            vec![
                ReadProgress { done: 1, total: 2 },
                ReadProgress { done: 2, total: 2 }
            ]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn async_import_runs_off_the_caller_and_handles_cancel() {
    assert!(
        import_sermon_notes(None, "KJV".into(), |_| {})
            .await
            .unwrap()
            .is_none()
    );
    let draft = import_sermon_notes(Some(fixture("faith.docx")), "KJV".into(), |_| {})
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.title, "faith");
    assert!(!draft.items.is_empty());
    let error = import_sermon_notes(Some(fixture("notes.odt")), "KJV".into(), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SermonImportError::Document(SermonDocumentError::Unsupported)
    ));
    assert_eq!(error.to_string(), UNSUPPORTED_DOCUMENT);
}

/// Release timing for a large document: `cargo test -p kairo-scripture --release
/// --test sermon_notes large_document_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing, run by hand"]
fn large_document_timing() {
    let mut text = std::fs::read_to_string(fixture("grace.txt")).unwrap();
    for name in ["shepherd.txt", "faith.txt", "hope.txt"] {
        text.push_str(&std::fs::read_to_string(fixture(name)).unwrap());
    }
    let big = text.repeat(2000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.txt");
    std::fs::write(&path, &big).unwrap();
    let start = std::time::Instant::now();
    let draft = import_sermon_notes_blocking(&path, "KJV", &mut |_| {}).unwrap();
    eprintln!(
        "{} bytes, {} matches, {} items: {:?}",
        big.len(),
        draft.matches.len(),
        draft.items.len(),
        start.elapsed()
    );
    if require_pdfium() {
        let start = std::time::Instant::now();
        for _ in 0..100 {
            read_sermon_document(&fixture("hope.pdf")).unwrap();
        }
        eprintln!("hope.pdf x100: {:?}", start.elapsed());
    }
}
