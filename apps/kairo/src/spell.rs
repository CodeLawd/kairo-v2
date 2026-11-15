//! The system spell checker for the song editor (prompt 082): Electron's
//! `<textarea spellCheck>` used Chromium's checker over the macOS
//! dictionaries; here `NSSpellChecker` reports misspelt ranges, which the
//! editor underlines. The editor calls it off the UI thread: the first
//! check waits for the spell server. Elsewhere nothing is reported.

use std::ops::Range;

/// Converts a UTF-16 range (as AppKit reports) to UTF-8 bytes in `text`.
pub fn utf16_to_bytes(text: &str, start: usize, len: usize) -> Option<Range<usize>> {
    let mut units = 0;
    let (mut from, mut to) = (None, None);
    for (byte, c) in text.char_indices() {
        if units == start {
            from = Some(byte);
        }
        if units == start + len {
            to = Some(byte);
            break;
        }
        units += c.len_utf16();
    }
    if units == start && from.is_none() {
        from = Some(text.len());
    }
    if to.is_none() && units <= start + len {
        to = Some(text.len());
    }
    let (from, to) = (from?, to?);
    (from < to).then_some(from..to)
}

/// Misspelt words in `text`, as byte ranges.
#[cfg(target_os = "macos")]
pub fn misspelled(text: &str) -> Vec<Range<usize>> {
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::NSString;
    if text.trim().is_empty() {
        return Vec::new();
    }
    let checker = NSSpellChecker::sharedSpellChecker();
    let string = NSString::from_str(text);
    let total = text.encode_utf16().count();
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < total {
        let found = checker.checkSpellingOfString_startingAt(&string, at as isize);
        if found.length == 0 || found.location >= total || found.location < at {
            break;
        }
        if let Some(range) = utf16_to_bytes(text, found.location, found.length) {
            out.push(range);
        }
        at = found.location + found.length;
    }
    out
}

#[cfg(not(target_os = "macos"))]
pub fn misspelled(_text: &str) -> Vec<Range<usize>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ranges_map_to_bytes_with_yoruba_and_emoji() {
        let text = "Ẹ ṣé 😀 wrld";
        // "wrld" starts after "Ẹ ṣé 😀 " (8 UTF-16 units: the emoji is 2).
        let r = utf16_to_bytes(text, 8, 4).unwrap();
        assert_eq!(&text[r], "wrld");
        let r = utf16_to_bytes(text, 2, 2).unwrap();
        assert_eq!(&text[r], "ṣé");
        assert_eq!(utf16_to_bytes(text, 0, 0), None);
    }

    /// Talks to the system spell server (slow the first time):
    /// `cargo test -p kairo --bin kairo spell -- --ignored`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn the_system_checker_marks_misspelt_words() {
        let text = "Amazing grace how sweeet the sound wrld";
        let found: Vec<&str> = misspelled(text).into_iter().map(|r| &text[r]).collect();
        assert!(found.contains(&"sweeet"), "{found:?}");
        assert!(!found.contains(&"grace"));
    }
}
