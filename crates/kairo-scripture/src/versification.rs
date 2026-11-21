//! Pack numbering, from `bible-pack-versification.ts`. Structural only.

use kairo_domain::scripture::versification::get_verse_numbering_override;
use kairo_store::db::BIBLE_VERSE_COUNTS;

pub use kairo_domain::scripture::versification::has_custom_pack_versification;

/// The exact verse numbers a chapter holds in `translation`, in order.
pub fn get_pack_verse_numbers(translation: &str, book_id: u32, chapter: u32) -> Vec<u32> {
    let numbering =
        get_verse_numbering_override(translation, f64::from(book_id), f64::from(chapter));
    let last = numbering.map(|n| n.last_verse).unwrap_or_else(|| {
        (book_id as usize)
            .checked_sub(1)
            .and_then(|book| BIBLE_VERSE_COUNTS.get(book))
            .and_then(|counts| counts.get((chapter as usize).checked_sub(1)?))
            .copied()
            .unwrap_or(0)
    });
    (1..=last)
        .filter(|verse| !numbering.is_some_and(|n| n.omitted.contains(verse)))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackTotals {
    pub books: u64,
    pub chapters: u64,
    pub verses: u64,
}

/// A complete pack's book, chapter and verse counts for `translation`.
pub fn pack_totals_for(translation: &str) -> PackTotals {
    // Untranslated books and chapters hold no verses, so they are not counted.
    let mut books = 0;
    let mut chapters = 0;
    let mut verses = 0;
    for (book_index, counts) in BIBLE_VERSE_COUNTS.iter().enumerate() {
        let before = chapters;
        for chapter in 1..=counts.len() as u32 {
            let count =
                get_pack_verse_numbers(translation, book_index as u32 + 1, chapter).len() as u64;
            chapters += u64::from(count > 0);
            verses += count;
        }
        books += u64::from(chapters > before);
    }
    PackTotals {
        books,
        chapters,
        verses,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_match_the_canon_and_ampc_numbering() {
        let canon = PackTotals {
            books: 66,
            chapters: 1_189,
            verses: 31_102,
        };
        assert_eq!(pack_totals_for("NKJV"), canon);
        assert_eq!(
            pack_totals_for("ampc"),
            PackTotals {
                verses: 31_101,
                ..canon
            }
        );
        let mark = get_pack_verse_numbers("AMPC", 41, 9);
        assert_eq!(&mark[42..46], [43, 45, 47, 48]);
        assert_eq!(get_pack_verse_numbers("AMPC", 64, 1).last(), Some(&15));
        // TPT leaves 15 books (391 chapters) untranslated.
        let tpt = pack_totals_for("TPT");
        assert_eq!((tpt.books, tpt.chapters), (51, 1_189 - 391));
        assert!(get_pack_verse_numbers("TPT", 2, 1).is_empty());
        assert_eq!(get_pack_verse_numbers("KJV", 64, 1).last(), Some(&14));
        assert!(get_pack_verse_numbers("KJV", 67, 1).is_empty());
        assert!(get_pack_verse_numbers("KJV", 1, 0).is_empty());
    }
}
