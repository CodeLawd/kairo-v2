//! PDFs for tests, and the development PDFium.

use kairo_documents::pdf::set_pdfium_library_dir;
use std::path::Path;

/// The development copy of PDFium, `.cache/pdfium/lib` (git-ignored), or
/// `PDFIUM_DYNAMIC_LIB_PATH`. `false` when neither exists.
pub fn require_pdfium() -> bool {
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/pdfium/lib");
    let lib = |dir: &Path| {
        dir.join(if cfg!(windows) {
            "pdfium.dll"
        } else if cfg!(target_os = "macos") {
            "libpdfium.dylib"
        } else {
            "libpdfium.so"
        })
    };
    let ok = if lib(&dev).is_file() {
        set_pdfium_library_dir(dev);
        true
    } else {
        std::env::var_os("PDFIUM_DYNAMIC_LIB_PATH").is_some_and(|d| lib(Path::new(&d)).is_file())
    };
    if !ok {
        assert!(
            std::env::var_os("KAIRO_REQUIRE_PDFIUM").is_none(),
            "KAIRO_REQUIRE_PDFIUM is set but PDFium was not found"
        );
        eprintln!("PDFium not found: PDF cases skipped");
    }
    ok
}

/// A PDF whose pages have the given point sizes, each with a red square in
/// the lower-left corner.
pub fn pdf(pages: &[(u32, u32)]) -> Vec<u8> {
    let n = pages.len();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!(
            "<< /Type /Pages /Count {n} /Kids [{}] >>",
            (0..n)
                .map(|i| format!("{} 0 R", 3 + 2 * i))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ];
    let content = "1 0 0 rg 0 0 100 100 re f";
    for (i, (w, h)) in pages.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents {} 0 R >>",
            4 + 2 * i
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

/// A PDF from raw object bodies; object 1 must be the catalog. A body may be
/// bytes (streams with binary data).
pub fn objects(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            bodies.len() + 1
        )
        .bytes(),
    );
    out
}

/// A stream object around `data`, with extra dictionary entries.
pub fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend(data);
    out.extend(b"\nendstream");
    out
}
