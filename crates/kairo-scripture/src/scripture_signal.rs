//! Scripture cues used at the boundary between local resolution and model analysis.

use std::sync::LazyLock;

use kairo_domain::scripture::query::BOOKS;
use regex::Regex;

const EVERYDAY_BOOKS: &str = "john|james|mark|luke|matthew|peter|jude|job|acts|numbers|ruth|judges|kings|daniel|samuel|timothy|titus|joel|amos|esther|jonah|revelation|philemon";

static SIGNAL: LazyLock<Regex> = LazyLock::new(|| {
    let mut distinct_books = Vec::new();
    for (name, _) in BOOKS {
        let name = name.trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == ' ');
        let name = name.to_ascii_lowercase();
        if !EVERYDAY_BOOKS.split('|').any(|book| book == name) && !distinct_books.contains(&name) {
            distinct_books.push(name);
        }
    }

    let distinct_books = distinct_books.join("|");
    let pattern = format!(
        r"(?i-u:\b(?:jesus|god|the lord|paul|peter|moses|david|john|solomon|isaiah|the prophet|the psalmist|the apostle|the scriptures?|the bible|the word|his word)\s+(?:said|says|saith|wrote|writes|declares|tells us|is written|was written)\b
        |\b(?:it is written|scripture|the word of god|verse|chapter)\b
        |\b(?:jesus|christ|the lord|lord god|holy spirit|paul|moses|abraham|isaac|jacob|joseph|noah|elijah|elisha|pharaoh|the disciples|the apostles?|pharisees|israel(?:ites)?|jerusalem|galilee|nazareth|bethlehem|egypt|samaria)\b
        |\b(?:thee|thou|thy|thine|ye|hath|doth|saith|unto|shalt|shall|verily|behold|begat|blessed|whosoever|wherefore|lo)\b
        |\b(?:{distinct_books})\b
        |\b(?:book|gospel|letter|epistle)\s+of\s+(?:{EVERYDAY_BOOKS})\b
        |\b(?:{EVERYDAY_BOOKS})\s+(?:chapter\s+)?(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve)\b)"
    );
    // JavaScript's word boundaries, digits and /i are ASCII for these patterns,
    // but its whitespace class includes non-ASCII spaces and the BOM.
    let pattern = pattern.lines().map(str::trim).collect::<String>().replace(
        r"\s",
        r"(?u:[\x09-\x0D\x20\u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}])",
    );
    Regex::new(&pattern).expect("valid scripture signal patterns")
});

/// Whether speech the local rules could not place should be considered for model analysis.
pub fn sounds_like_scripture(text: &str) -> bool {
    SIGNAL.is_match(text)
}
