//! Crash-safe file replacement, shared by every store that writes a file.

use std::path::Path;

/// The start of the temporary file name [`write_atomic`] uses for `file_name`.
fn temp_prefix(file_name: &str) -> String {
    format!(".{file_name}.tmp-")
}

/// Removes temporary files that a killed [`write_atomic`] of `file_name` left
/// in `dir`. Only one Kairo runs per profile, so none is in use.
pub fn remove_stale_temp_files(dir: &Path, file_name: &str) {
    let prefix = temp_prefix(file_name);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Writes `bytes` to `path` crash-safely: a temporary file in the same
/// folder, `fsync`, then a rename over the old file (and, on Unix, an `fsync`
/// of the folder so the rename itself survives power loss). A failure at any
/// point leaves the old file whole.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!("{}{}-{n}", temp_prefix(name), std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        if let Ok(folder) = std::fs::File::open(dir) {
            let _ = folder.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
