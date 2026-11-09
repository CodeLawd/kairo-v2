//! `.kairo` files handed to Kairo by the operating system, from
//! `src/main/index.ts` ("Opening .kairo files from Finder / Explorer").
//!
//! Windows and Linux put the file on the command line, of this process or of
//! a second launch ([`crate::instance`]). macOS sends it as an open-file event,
//! including the file that launched the app, before any window exists; the
//! [`LaunchQueue`] holds paths until the app is ready to open them.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// `kairoPathsIn`: the arguments that name an existing `.kairo` file (any
/// case). Relative arguments resolve against `cwd`, the folder of the
/// process that received them.
pub fn kairo_paths_in(argv: &[String], cwd: Option<&Path>) -> Vec<PathBuf> {
    argv.iter()
        .filter(|arg| arg.to_lowercase().ends_with(".kairo"))
        .map(|arg| match cwd {
            Some(cwd) if Path::new(arg).is_relative() => cwd.join(arg),
            _ => PathBuf::from(arg),
        })
        .filter(|path| path.exists())
        .collect()
}

/// The `.kairo` files among macOS open-file URLs (`file://…`, percent-encoded).
pub fn kairo_paths_from_urls(urls: &[String]) -> Vec<PathBuf> {
    let args: Vec<String> = urls
        .iter()
        .filter_map(|url| url.strip_prefix("file://"))
        .filter_map(percent_decode)
        .collect();
    kairo_paths_in(&args, None)
}

/// Decodes `%XX` escapes. `None` when the result is not UTF-8.
pub fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let byte = (bytes[at] == b'%')
            .then(|| {
                text.get(at + 1..at + 3)
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            })
            .flatten();
        match byte {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Files that arrive before the app can open them. Paths pushed after
/// [`LaunchQueue::mark_ready`] are returned to the caller to open at once.
#[derive(Default)]
pub struct LaunchQueue {
    state: Mutex<(bool, Vec<PathBuf>)>,
}

impl LaunchQueue {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new((false, Vec::new())),
        }
    }

    /// Holds `paths` until ready, or hands them back if already ready.
    pub fn push(&self, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.0 {
            paths
        } else {
            state.1.extend(paths);
            Vec::new()
        }
    }

    /// Marks the app ready and returns everything held so far.
    pub fn mark_ready(&self) -> Vec<PathBuf> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.0 = true;
        std::mem::take(&mut state.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_kairo_files_are_taken() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("Amazing Grace.kairo");
        let upper = dir.path().join("SET.KAIRO");
        std::fs::write(&song, "").unwrap();
        std::fs::write(&upper, "").unwrap();
        let argv = vec![
            "--inspect".into(),
            song.to_string_lossy().into_owned(),
            dir.path()
                .join("missing.kairo")
                .to_string_lossy()
                .into_owned(),
            "notes.txt".into(),
            "SET.KAIRO".into(),
        ];
        assert_eq!(
            kairo_paths_in(&argv, Some(dir.path())),
            vec![song.clone(), upper]
        );
        assert!(kairo_paths_in(&[], None).is_empty());
    }

    #[test]
    fn file_urls_are_decoded() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("Amazing Grace é.kairo");
        std::fs::write(&song, "").unwrap();
        let url = format!(
            "file://{}",
            song.to_string_lossy()
                .replace(' ', "%20")
                .replace('é', "%C3%A9")
        );
        assert_eq!(
            kairo_paths_from_urls(&[url, "https://example.com/x.kairo".into()]),
            vec![song]
        );
        assert_eq!(percent_decode("%ZZ%41"), Some("%ZZA".into()));
        assert_eq!(percent_decode("%FF"), None);
    }

    #[test]
    fn the_queue_holds_files_until_ready() {
        let queue = LaunchQueue::new();
        assert!(queue.push(vec!["a.kairo".into()]).is_empty());
        assert!(queue.push(vec!["b.kairo".into()]).is_empty());
        assert_eq!(
            queue.mark_ready(),
            vec![PathBuf::from("a.kairo"), PathBuf::from("b.kairo")]
        );
        assert_eq!(
            queue.push(vec!["c.kairo".into()]),
            vec![PathBuf::from("c.kairo")]
        );
        assert!(queue.mark_ready().is_empty());
    }
}
