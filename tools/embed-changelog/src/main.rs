//! Regenerates `crates/ui-shell/data/changelog.json` from the Electron
//! website's `website/src/lib/changelog.ts`, for the What's New sheet.
//!
//! `cargo run -p embed-changelog -- --regenerate` (run by
//! `electron-sync --apply-generated`). The Electron checkout is
//! `$KAIRO_ELECTRON_DIR`, else `../kairo-presenter`. Node 22.6 or later
//! imports the TypeScript file directly, stripping its types. The output is
//! committed; builds never read the sibling repository.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: embed-changelog --regenerate [--root DIR]";
const OUTPUT: &str = "crates/ui-shell/data/changelog.json";
const SOURCE: &str = "website/src/lib/changelog.ts";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut regenerate = false;
    let mut root = PathBuf::from(".");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--regenerate" => regenerate = true,
            "--root" => match args.next() {
                Some(dir) => root = PathBuf::from(dir),
                None => return fail(USAGE),
            },
            _ => return fail(USAGE),
        }
    }
    if !regenerate {
        return fail(USAGE);
    }
    let electron = std::env::var_os("KAIRO_ELECTRON_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("../kairo-presenter"));
    match run(&root, &electron) {
        Ok(count) => {
            println!("embed-changelog: wrote {count} entries to {OUTPUT}");
            ExitCode::SUCCESS
        }
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("embed-changelog: {message}");
    ExitCode::FAILURE
}

fn run(root: &Path, electron: &Path) -> Result<usize, String> {
    let source = electron.join(SOURCE);
    let source = source
        .canonicalize()
        .map_err(|e| format!("cannot find {}: {e}", source.display()))?;
    let script = format!(
        "import({}).then((m) => process.stdout.write(JSON.stringify(m.changelog)))",
        serde_json::to_string(&format!("file://{}", source.display())).map_err(|e| e.to_string())?
    );
    let output = Command::new("node")
        .args(["--no-warnings", "--input-type=module", "-e", &script])
        .output()
        .map_err(|e| format!("cannot run node: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "node failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let entries: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("changelog is not JSON: {e}"))?;
    let count = validate(&entries)?;
    let mut text = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(root.join(OUTPUT), text).map_err(|e| format!("cannot write {OUTPUT}: {e}"))?;
    Ok(count)
}

/// Checks the shape the app reads: an array of entries with string
/// `version`, `date` and `title`, string `notes`, and optional highlights.
fn validate(entries: &Value) -> Result<usize, String> {
    let list = entries.as_array().ok_or("changelog is not an array")?;
    for (ix, entry) in list.iter().enumerate() {
        for key in ["version", "date", "title"] {
            if !entry.get(key).is_some_and(Value::is_string) {
                return Err(format!("entry {ix} has no string `{key}`"));
            }
        }
        let notes = entry.get("notes").and_then(Value::as_array);
        if !notes.is_some_and(|n| n.iter().all(Value::is_string)) {
            return Err(format!("entry {ix} has no string `notes`"));
        }
        if let Some(highlights) = entry.get("highlights") {
            let ok = highlights.as_array().is_some_and(|h| {
                h.iter().all(|item| {
                    ["icon", "title", "body"]
                        .iter()
                        .all(|key| item.get(key).is_some_and(Value::is_string))
                })
            });
            if !ok {
                return Err(format!("entry {ix} has malformed `highlights`"));
            }
        }
    }
    Ok(list.len())
}

#[cfg(test)]
mod tests {
    use super::validate;
    use serde_json::json;

    #[test]
    fn accepts_entries_with_and_without_highlights() {
        let entries = json!([
            {"version": "1.0.1", "date": "2026-01-01", "title": "T", "notes": ["a"]},
            {"version": "1.0.0", "date": "2026-01-01", "title": "T", "notes": [],
             "highlights": [{"icon": "tip", "title": "x", "body": "y"}]}
        ]);
        assert_eq!(validate(&entries), Ok(2));
    }

    #[test]
    fn rejects_missing_fields() {
        assert!(validate(&json!({})).is_err());
        assert!(validate(&json!([{"version": "1"}])).is_err());
        assert!(
            validate(&json!([{"version": "1", "date": "d", "title": "t", "notes": [1]}])).is_err()
        );
        assert!(
            validate(
                &json!([{"version": "1", "date": "d", "title": "t", "notes": [],
                "highlights": [{"icon": "tip"}]}])
            )
            .is_err()
        );
    }
}
