mod support;

use kairo_documents::{pdf::merge_pages, script_import::import_script};
use support::{pdf::require_pdfium, pdf_corpus::pdfs};

#[tokio::test]
async fn extracts_pdf_pages_and_warns_about_pages_without_text() {
    if !require_pdfium() {
        return;
    }
    let corpus = pdfs();
    let text = &corpus.iter().find(|(name, _)| *name == "text").unwrap().1;
    let image = &corpus
        .iter()
        .find(|(name, _)| *name == "vectors-and-image")
        .unwrap()
        .1;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.pdf");
    std::fs::write(&path, text).unwrap();
    let single = import_script(path.clone()).await.unwrap();
    assert!(single.text.contains("Amazing Grace"));
    assert!(!single.text.contains('\r'));

    let mixed = merge_pages(&[text.clone(), image.clone(), text.clone()]).unwrap();
    std::fs::write(&path, mixed).unwrap();
    let imported = import_script(path.clone()).await.unwrap();
    assert_eq!(imported.text, format!("{}\n\n{}", single.text, single.text));
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("columns"))
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.ends_with("pages 2."))
    );

    std::fs::write(&path, image).unwrap();
    assert!(
        import_script(path.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("text-enabled copy")
    );

    std::fs::write(&path, b"not a PDF").unwrap();
    assert!(
        import_script(path.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("Could not read the PDF")
    );
}

#[tokio::test]
async fn reports_password_protection() {
    if !require_pdfium() {
        return;
    }
    let document_id = "0123456789abcdef0123456789abcdef";
    let password_entries = "ab".repeat(32);
    let mut data = support::pdf::objects(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_vec(),
        format!("<< /Filter /Standard /V 1 /R 2 /O <{password_entries}> /U <{password_entries}> /P -4 >>").into_bytes(),
    ]);
    let trailer = b"/Root 1 0 R";
    let offset = data
        .windows(trailer.len())
        .position(|part| part == trailer)
        .unwrap()
        + trailer.len();
    data.splice(
        offset..offset,
        format!(" /Encrypt 4 0 R /ID [<{document_id}> <{document_id}>]").bytes(),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.pdf");
    std::fs::write(&path, data).unwrap();
    assert!(
        import_script(path.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("password protected")
    );
}
