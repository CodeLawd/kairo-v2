//! Rebuilds the TypeScript oracle cache. Development and CI tool only.

use std::{fs, path::Path, process::Command, process::ExitCode};

fn regenerate(root: &Path) -> Result<(), String> {
    let cache = root.join("tests/oracle-cache");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(&cache).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    // Serial, as test binaries that share a function would otherwise race on its file.
    let status = Command::new("cargo")
        .args(["test", "--workspace", "--", "--test-threads=1"])
        .env("KAIRO_ORACLE_MODE", "refresh")
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    match regenerate(&root) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
