//! Rebuilds the TypeScript oracle cache. Development and CI tool only.

use std::{fs, path::Path, process::Command, process::ExitCode};

fn regenerate(root: &Path) -> Result<(), String> {
    let cache = kairo_oracle::cache_dir();
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(&cache).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    // Cargo runs test binaries one at a time, and within one binary the
    // shared oracle serialises cache writes, so tests can stay parallel.
    let status = Command::new("cargo")
        .args(["test", "--workspace"])
        .env(kairo_oracle::MODE_ENV, "refresh")
        .current_dir(root)
        .status()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("cargo test failed: {status}"))
}

fn main() -> ExitCode {
    if !std::env::args().any(|arg| arg == "--regenerate") {
        eprintln!("usage: ts-oracle --regenerate");
        return ExitCode::FAILURE;
    }
    match regenerate(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
