//! The built-in renderer's HTML against Electron's, byte for byte, through
//! the oracle (`oracle:pptx-html.slidesHtml`).

mod support;

use base64::Engine;
use kairo_documents::pptx_builtin::build_slides_html;
use serde_json::{Value, json};

fn rust(deck: &[u8]) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("deck.pptx");
    std::fs::write(&source, deck).unwrap();
    match build_slides_html(&source, dir.path()) {
        Ok(html) => {
            let text = std::fs::read_to_string(html.path).unwrap();
            let media = url::Url::from_file_path(dir.path().join("pptx/ppt/media")).unwrap();
            json!({ "html": text.replace(media.as_str(), "MEDIA") })
        }
        Err(error) => json!({ "error": error.to_string() }),
    }
}

#[test]
fn slide_html_matches_electron() {
    let decks = support::corpus::decks();
    let encoded: Vec<String> = decks
        .iter()
        .map(|(_, d)| base64::engine::general_purpose::STANDARD.encode(d))
        .collect();
    let expected = kairo_oracle::oracle_value("oracle:pptx-html.slidesHtml", json!([encoded]));
    let mut failures = Vec::new();
    for ((name, deck), want) in decks.iter().zip(expected.as_array().unwrap()) {
        let got = rust(deck);
        assert_eq!(
            name.ends_with('!'),
            got.get("error").is_some(),
            "{name}: {got}"
        );
        if &got != want {
            let (a, b) = (want.to_string(), got.to_string());
            let at = a
                .bytes()
                .zip(b.bytes())
                .position(|(x, y)| x != y)
                .unwrap_or(a.len().min(b.len()));
            let from = at.saturating_sub(80);
            failures.push(format!(
                "{name}: differs at byte {at}\n  electron …{}\n  rust     …{}",
                &a[from..(at + 80).min(a.len())],
                &b[from..(at + 80).min(b.len())]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
