//! Deepgram keyterm prompting for Bible book names. From
//! `src/main/services/stt/keyterms.ts`.
//!
//! Nova-3 caps keyterms at 500 tokens per request and Deepgram recommends a
//! focused list of 20–50 terms, so this is not every book: names such as John,
//! Mark or Acts are ordinary English the model already hears well. These are
//! the ones that come back misspelled or as other words.

const HARD_BOOK_NAMES: [&str; 31] = [
    "Leviticus",
    "Deuteronomy",
    "Chronicles",
    "Nehemiah",
    "Psalm",
    "Psalms",
    "Proverbs",
    "Ecclesiastes",
    "Song of Solomon",
    "Isaiah",
    "Jeremiah",
    "Lamentations",
    "Ezekiel",
    "Hosea",
    "Obadiah",
    "Micah",
    "Nahum",
    "Habakkuk",
    "Zephaniah",
    "Haggai",
    "Zechariah",
    "Malachi",
    "Corinthians",
    "Galatians",
    "Ephesians",
    "Philippians",
    "Colossians",
    "Thessalonians",
    "Philemon",
    "Hebrews",
    "Revelation",
];

pub const MAX_KEYTERMS: usize = 50;

/// Terms to boost for one session. Books in the live sermon playlist come
/// first. The names are English, so other languages get none.
pub fn scripture_keyterms(language: &str, playlist_books: &[&str]) -> Vec<String> {
    if !is_english(kairo_domain::js::trim(language)) {
        return Vec::new();
    }
    let mut seen = std::collections::HashSet::new();
    let mut terms = Vec::new();
    for book in playlist_books.iter().chain(&HARD_BOOK_NAMES) {
        // "1 Corinthians" is spoken "First Corinthians"; the name needs help.
        let name = kairo_domain::js::trim(strip_ordinal(book));
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        terms.push(name.to_owned());
        if terms.len() == MAX_KEYTERMS {
            break;
        }
    }
    terms
}

/// `/^en(?:-|$)/i`.
fn is_english(language: &str) -> bool {
    let bytes = language.as_bytes();
    bytes.len() >= 2
        && bytes[..2].eq_ignore_ascii_case(b"en")
        && (bytes.len() == 2 || bytes[2] == b'-')
}

/// `/^[123]\s+/`.
fn strip_ordinal(book: &str) -> &str {
    let mut chars = book.char_indices();
    match (chars.next(), chars.next()) {
        (Some((_, '1' | '2' | '3')), Some((_, c))) if kairo_domain::js::is_whitespace(c) => {
            book[1..].trim_start_matches(kairo_domain::js::is_whitespace)
        }
        _ => book,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boosts_hard_book_names_for_english() {
        let terms = scripture_keyterms("en-US", &[]);
        for name in ["Habakkuk", "Philippians", "Ecclesiastes", "Thessalonians"] {
            assert!(terms.iter().any(|t| t == name), "{name}");
        }
        assert!(terms.len() <= MAX_KEYTERMS);
        assert_eq!(scripture_keyterms("en", &[]), terms);
    }

    #[test]
    fn playlist_books_come_first_by_name_without_duplicates() {
        let terms = scripture_keyterms("en-GB", &["John", "1 Corinthians", "Habakkuk", "John"]);
        assert_eq!(&terms[..3], ["John", "Corinthians", "Habakkuk"]);
        assert_eq!(terms.iter().filter(|t| *t == "Corinthians").count(), 1);
        assert_eq!(terms.iter().filter(|t| *t == "Habakkuk").count(), 1);
    }

    #[test]
    fn stays_within_the_cap() {
        let names: Vec<String> = (0..80).map(|i| format!("Book{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(scripture_keyterms("en-US", &refs).len(), MAX_KEYTERMS);
    }

    #[test]
    fn other_languages_get_no_english_names() {
        assert!(scripture_keyterms("fr", &[]).is_empty());
        assert!(scripture_keyterms("es-419", &["Romans"]).is_empty());
        assert!(scripture_keyterms("multi", &[]).is_empty());
        assert!(scripture_keyterms("eng", &[]).is_empty());
        assert!(!scripture_keyterms(" EN ", &[]).is_empty());
    }
}
