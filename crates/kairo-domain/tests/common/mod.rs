//! Helpers shared by the kairo-domain integration tests.
#![allow(dead_code)]

use kairo_domain::json::to_js_value;
use kairo_oracle::compare;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// A path under `tests/fixtures`.
pub fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

/// The `.json` files in `tests/fixtures/<dir>`, sorted.
pub fn fixtures(dir: &str) -> Vec<PathBuf> {
    let root = fixture(dir);
    let mut files: Vec<_> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

pub fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Parses `json` as `T` and checks it writes back unchanged. Returns the value.
pub fn round_trip<T: Serialize + DeserializeOwned>(label: &str, json: &Value) -> T {
    let parsed: T = serde_json::from_value(json.clone()).unwrap_or_else(|e| {
        panic!(
            "{label} does not parse as {}: {e}",
            std::any::type_name::<T>()
        )
    });
    let written = to_js_value(&parsed).unwrap();
    if &written != json {
        let differences = compare(json, &written);
        panic!(
            "{label} changed in a round trip ({} differences, by value; number text differs if none):\n  {}",
            differences.len(),
            differences
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
    parsed
}
