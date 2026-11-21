//! Verse numbering that differs from the standard canon, from
//! `src/lib/bible-versification.ts`. Reference numbers only: omitted verses
//! are never filled or renumbered.

use super::versification_data::{ABSENT_BOOKS, GENERATED_OVERRIDES};

/// A chapter whose numbering differs in one translation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerseNumberingOverride {
    pub last_verse: u32,
    pub omitted: &'static [u32],
}

/// `(translation, book id, chapter, override)`, verified against the AMPC source edition.
const OVERRIDES: &[(&str, u32, u32, VerseNumberingOverride)] = &[
    (
        "AMPC",
        41,
        9,
        VerseNumberingOverride {
            last_verse: 50,
            omitted: &[44, 46],
        },
    ),
    (
        "AMPC",
        64,
        1,
        VerseNumberingOverride {
            last_verse: 15,
            omitted: &[],
        },
    ),
];

pub fn has_custom_pack_versification(translation: &str) -> bool {
    let upper = translation.to_uppercase();
    OVERRIDES
        .iter()
        .chain(GENERATED_OVERRIDES)
        .any(|(id, ..)| *id == upper)
        || ABSENT_BOOKS.iter().any(|(id, _)| *id == upper)
}

/// Every chapter of an untranslated book holds no verses.
const ABSENT_CHAPTER: VerseNumberingOverride = VerseNumberingOverride {
    last_verse: 0,
    omitted: &[],
};

/// Takes JavaScript numbers: the source builds the key `${bookId}:${chapter}`,
/// which equals the registry key exactly when the numbers are equal.
pub fn get_verse_numbering_override(
    translation: &str,
    book_id: f64,
    chapter: f64,
) -> Option<VerseNumberingOverride> {
    let upper = translation.to_uppercase();
    let absent = ABSENT_BOOKS
        .iter()
        .any(|(id, books)| *id == upper && books.iter().any(|b| f64::from(*b) == book_id));
    if absent {
        return Some(ABSENT_CHAPTER);
    }
    OVERRIDES
        .iter()
        .chain(GENERATED_OVERRIDES)
        .find(|(id, book, ch, _)| {
            *id == upper && f64::from(*book) == book_id && f64::from(*ch) == chapter
        })
        .map(|(.., value)| *value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ampc_mark_nine_omits_two_verses() {
        let mark = get_verse_numbering_override("ampc", 41.0, 9.0).unwrap();
        assert_eq!(mark.omitted, [44, 46]);
        assert_eq!(mark.last_verse, 50);
        assert!(get_verse_numbering_override("KJV", 41.0, 9.0).is_none());
        assert!(has_custom_pack_versification("Ampc"));
        assert!(!has_custom_pack_versification("NKJV"));
    }

    #[test]
    fn generated_numbering_covers_merged_verses_and_absent_books() {
        // GNT prints Genesis 1:6-7 as verse 6.
        let gnt = get_verse_numbering_override("GNT", 1.0, 1.0).unwrap();
        assert_eq!((gnt.last_verse, gnt.omitted), (31, &[7][..]));
        let exodus = get_verse_numbering_override("tpt", 2.0, 3.0).unwrap();
        assert_eq!((exodus.last_verse, exodus.omitted), (0, &[][..]));
        assert!(get_verse_numbering_override("TPT", 1.0, 2.0).is_none());
        assert!(has_custom_pack_versification("easy"));
        assert!(!has_custom_pack_versification("MSG"));
    }
}
