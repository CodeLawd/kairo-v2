//! OS file clipboard. From `src/main/services/media/clipboard.ts`.
//!
//! macOS reads and writes `NSFilenamesPboardType`, then falls back to
//! `public.file-url` and plain text. Windows uses `CF_HDROP` and Linux
//! `text/uri-list` (X11 and Wayland), both through `arboard`, then plain text.

use std::path::PathBuf;

/// Absolute paths currently on the pasteboard, if any. Never fails: an empty
/// or unreadable clipboard is an empty list.
pub fn read_file_paths() -> Vec<PathBuf> {
    platform::read()
}

/// Puts absolute file paths on the pasteboard the way the file manager expects.
/// Empty strings are dropped; an empty list writes nothing.
pub fn write_file_paths(paths: &[PathBuf]) -> Result<(), String> {
    let absolute: Vec<_> = paths
        .iter()
        .filter(|p| !p.as_os_str().is_empty())
        .cloned()
        .collect();
    if absolute.is_empty() {
        return Ok(());
    }
    platform::write(&absolute)
}

/// Paths inside an `NSFilenamesPboardType` XML property list.
pub fn parse_plist_paths(plist: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = plist;
    while let Some(start) = rest.find("<string>") {
        rest = &rest[start + "<string>".len()..];
        let Some(end) = rest.find('<') else { break };
        if end > 0 && rest[end..].starts_with("</string>") {
            out.push(unescape_xml(&rest[..end]));
        }
        rest = &rest[end..];
    }
    out
}

/// `NSFilenamesPboardType` XML for `paths`.
pub fn plist_for_paths(paths: &[String]) -> String {
    let mut lines = vec![
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_owned(),
        r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#.to_owned(),
        r#"<plist version="1.0">"#.to_owned(),
        "<array>".to_owned(),
    ];
    lines.extend(
        paths
            .iter()
            .map(|p| format!("<string>{}</string>", escape_xml(p))),
    );
    lines.push("</array>".into());
    lines.push("</plist>".into());
    lines.join("\n")
}

/// A `file:` URL, or a bare path that a `file://` prefix turns into one.
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let full = if url.starts_with("file:") {
        url.to_owned()
    } else {
        format!("file://{url}")
    };
    url::Url::parse(&full).ok()?.to_file_path().ok()
}

/// Newline-separated file URLs and absolute paths; anything else is ignored.
pub fn parse_text_paths(text: &str) -> Vec<PathBuf> {
    text.split('\n')
        .map(|line| kairo_domain::js::trim(line.strip_suffix('\r').unwrap_or(line)))
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            if line.starts_with("file:") {
                return file_url_to_path(line);
            }
            let bytes = line.as_bytes();
            let drive = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'\\' | b'/');
            (line.starts_with('/') || drive).then(|| PathBuf::from(line))
        })
        .collect()
}

/// `public.file-url` content: one URL per line.
pub fn parse_url_lines(text: &str) -> Vec<PathBuf> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).trim())
        .filter(|l| !l.is_empty())
        .filter_map(file_url_to_path)
        .collect()
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSArray, NSString};

    const FILENAMES: &str = "NSFilenamesPboardType";

    pub fn read() -> Vec<PathBuf> {
        autoreleasepool(|_| {
            let board = NSPasteboard::generalPasteboard();
            let names = NSString::from_str(FILENAMES);
            if let Some(list) = board.propertyListForType(&names)
                && let Ok(array) = list.downcast::<NSArray<AnyObject>>()
            {
                let paths: Vec<_> = array
                    .iter()
                    .filter_map(|item| item.downcast::<NSString>().ok())
                    .map(|s| PathBuf::from(s.to_string()))
                    .collect();
                if !paths.is_empty() {
                    return paths;
                }
            }
            // Finder sometimes exposes a single file this way.
            if let Some(url) = board.stringForType(&NSString::from_str("public.file-url")) {
                let paths = parse_url_lines(&url.to_string());
                if !paths.is_empty() {
                    return paths;
                }
            }
            board
                .stringForType(&NSString::from_str("public.utf8-plain-text"))
                .map(|text| parse_text_paths(&text.to_string()))
                .unwrap_or_default()
        })
    }

    pub fn write(paths: &[PathBuf]) -> Result<(), String> {
        autoreleasepool(|_| {
            let strings: Vec<Retained<NSString>> = paths
                .iter()
                .map(|p| NSString::from_str(&p.to_string_lossy()))
                .collect();
            let array = NSArray::from_retained_slice(&strings);
            let board = NSPasteboard::generalPasteboard();
            board.clearContents();
            let object: &AnyObject = &array;
            // SAFETY: an NSArray of NSString is a valid property list.
            if unsafe { board.setPropertyList_forType(object, &NSString::from_str(FILENAMES)) } {
                Ok(())
            } else {
                Err("The pasteboard refused the file list.".into())
            }
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    pub fn read() -> Vec<PathBuf> {
        let Ok(mut board) = arboard::Clipboard::new() else {
            return Vec::new();
        };
        if let Ok(files) = board.get().file_list()
            && !files.is_empty()
        {
            return files;
        }
        board
            .get_text()
            .map(|text| parse_text_paths(&text))
            .unwrap_or_default()
    }

    pub fn write(paths: &[PathBuf]) -> Result<(), String> {
        arboard::Clipboard::new()
            .and_then(|mut board| board.set().file_list(paths))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_round_trips_escaped_paths() {
        let paths = vec!["/a/R&D <x>.mp4".to_owned(), "/b/\"q\".png".to_owned()];
        assert_eq!(parse_plist_paths(&plist_for_paths(&paths)), paths);
    }

    #[test]
    fn plain_text_accepts_urls_and_absolute_paths_only() {
        let text = "file:///Users/x/My%20Loop.mp4\r\n  /tmp/a.png \nnotes.txt\nC:\\m\\b.mov\n\n";
        let paths = parse_text_paths(text);
        assert_eq!(paths[0], PathBuf::from("/Users/x/My Loop.mp4"));
        assert_eq!(paths[1], PathBuf::from("/tmp/a.png"));
        assert_eq!(paths[2], PathBuf::from("C:\\m\\b.mov"));
        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn a_bare_file_url_path_gets_a_scheme() {
        assert_eq!(
            parse_url_lines("/Users/x/a.mp4\n"),
            vec![PathBuf::from("/Users/x/a.mp4")]
        );
    }

    #[test]
    fn writing_nothing_is_a_no_op() {
        assert_eq!(write_file_paths(&[PathBuf::new()]), Ok(()));
    }
}
