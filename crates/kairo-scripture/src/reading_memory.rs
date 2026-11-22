//! Every passage read during a service, where each reading stopped, and
//! which one a resume ("let's continue from verse 65") goes back to.
//!
//! Pure and synchronous: it is consulted on the transcript path, and holds
//! at most [`CAPACITY`] readings.

use crate::citation::ExplicitCitation;
use crate::verse_bounds::is_valid_scripture_reference;

/// Readings kept; older ones drop off.
pub const CAPACITY: usize = 8;
/// A verse this close past a reading's end still belongs to it.
const JUST_PAST: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingKind {
    /// The text being preached from.
    Main,
    /// A single verse cited while a main reading was open.
    Aside,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub book: &'static str,
    pub chapter: f64,
    /// First verse of the reading.
    pub start: f64,
    /// Furthest verse known to belong to it.
    pub end: f64,
    /// Last verse heard read to its end.
    pub last_read: Option<f64>,
    /// Last verse put on screen.
    pub last_shown: f64,
    pub kind: ReadingKind,
    pub touched_at: u64,
}

impl Reading {
    fn has(&self, verse: f64) -> bool {
        is_valid_scripture_reference(self.book, self.chapter, verse, None)
    }

    /// Where a resume picks up: after the last verse read to its end, else
    /// the verse left on screen half-read.
    pub fn resume_verse(&self) -> f64 {
        match self.last_read {
            Some(read) if read >= self.last_shown && self.has(read + 1.0) => read + 1.0,
            Some(read) if read >= self.last_shown => read,
            _ => self.last_shown,
        }
    }

    /// The verse lies in the part read so far, or just past it.
    fn spans(&self, verse: f64) -> bool {
        let reached = self
            .end
            .max(self.last_shown)
            .max(self.last_read.unwrap_or(0.0));
        self.start <= verse && verse <= reached + JUST_PAST
    }

    pub fn citation(&self, verse_start: f64, verse_end: Option<f64>) -> ExplicitCitation {
        let end = verse_end.map_or_else(String::new, |end| format!("-{end}"));
        ExplicitCitation {
            book: self.book,
            chapter: self.chapter,
            verse_start,
            verse_end,
            source_text: format!("{} {}:{verse_start}{end}", self.book, self.chapter),
        }
    }
}

/// A reading picked for a resume; `ambiguous` when another fitted as well.
#[derive(Clone, Debug, PartialEq)]
pub struct Recall<'a> {
    pub reading: &'a Reading,
    pub ambiguous: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadingMemory {
    /// Newest first.
    readings: Vec<Reading>,
}

impl ReadingMemory {
    pub fn readings(&self) -> &[Reading] {
        &self.readings
    }

    pub fn clear(&mut self) {
        self.readings.clear();
    }

    pub fn newest(&self) -> Option<&Reading> {
        self.readings.first()
    }

    pub fn newest_main(&self) -> Option<&Reading> {
        self.readings.iter().find(|r| r.kind == ReadingKind::Main)
    }

    /// A citation was heard. A range, a reading introduced as such ("turn
    /// to"), or the first passage of the service is a main reading; any other
    /// single verse cited while one is open is an aside.
    pub fn cite(&mut self, reference: &ExplicitCitation, introduced: bool, now: u64) {
        let end = reference.verse_end.unwrap_or(reference.verse_start);
        let main = introduced || end > reference.verse_start;
        if let Some(reading) = self.touch(reference.book, reference.chapter, now) {
            reading.start = reading.start.min(reference.verse_start);
            reading.end = reading.end.max(end);
            if main {
                reading.kind = ReadingKind::Main;
            }
            return;
        }
        let kind = if main || self.newest_main().is_none() {
            ReadingKind::Main
        } else {
            ReadingKind::Aside
        };
        self.readings.insert(
            0,
            Reading {
                book: reference.book,
                chapter: reference.chapter,
                start: reference.verse_start,
                end,
                last_read: None,
                last_shown: reference.verse_start,
                kind,
                touched_at: now,
            },
        );
        self.readings.truncate(CAPACITY);
    }

    /// A verse went on screen.
    pub fn shown(&mut self, reference: &ExplicitCitation, now: u64) {
        self.cite(reference, false, now);
        if let Some(reading) = self.touch(reference.book, reference.chapter, now) {
            reading.last_shown = reference.verse_start;
        }
    }

    /// Follow-along heard this verse read to its end. Two verses read make a
    /// reading the main one.
    pub fn read(&mut self, book: &'static str, chapter: f64, verse: f64, now: u64) {
        let Some(reading) = self.touch(book, chapter, now) else {
            return;
        };
        reading.last_read = Some(reading.last_read.map_or(verse, |read| read.max(verse)));
        reading.end = reading.end.max(verse);
        if verse > reading.start {
            reading.kind = ReadingKind::Main;
        }
    }

    /// Moves the reading of this chapter to the front.
    fn touch(&mut self, book: &str, chapter: f64, now: u64) -> Option<&mut Reading> {
        let at = self
            .readings
            .iter()
            .position(|r| r.book == book && r.chapter == chapter)?;
        let mut reading = self.readings.remove(at);
        reading.touched_at = now;
        self.readings.insert(0, reading);
        self.readings.first_mut()
    }

    /// The main reading a resume to `verse` means: one whose read span holds
    /// the verse beats one whose chapter merely has it; newest wins a tie,
    /// which is then ambiguous.
    pub fn main_with(&self, verse: f64) -> Option<Recall<'_>> {
        let fits: Vec<&Reading> = self
            .readings
            .iter()
            .filter(|r| r.kind == ReadingKind::Main && r.has(verse))
            .collect();
        let spanning: Vec<&Reading> = fits.iter().copied().filter(|r| r.spans(verse)).collect();
        let tier = if spanning.is_empty() { fits } else { spanning };
        Some(Recall {
            reading: tier.first()?,
            ambiguous: tier.len() > 1,
        })
    }

    /// A bare verse heard without resume wording: the reading just cited if
    /// its chapter has the verse, else the main reading that does.
    pub fn current_with(&self, verse: f64) -> Option<Recall<'_>> {
        match self.newest() {
            Some(reading) if reading.has(verse) => Some(Recall {
                reading,
                ambiguous: false,
            }),
            _ => self.main_with(verse),
        }
    }

    /// The reference lies in a passage read in the last `within_ms`, not
    /// only the one on screen: words read from it aloud are not a quote.
    pub fn covers(&self, reference: &ExplicitCitation, now: u64, within_ms: u64) -> bool {
        let end = reference.verse_end.unwrap_or(reference.verse_start);
        self.readings.iter().any(|r| {
            r.book == reference.book
                && r.chapter == reference.chapter
                && now.saturating_sub(r.touched_at) <= within_ms
                && r.spans(reference.verse_start)
                && r.spans(end)
        })
    }

    /// The newest reading of a book, if it has the verse (any verse when
    /// `None`).
    pub fn of_book(&self, book: &str, verse: Option<f64>) -> Option<&Reading> {
        self.readings
            .iter()
            .find(|r| r.book == book && verse.is_none_or(|v| r.has(v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cite(book: &'static str, chapter: f64, start: f64, end: Option<f64>) -> ExplicitCitation {
        ExplicitCitation {
            book,
            chapter,
            verse_start: start,
            verse_end: end,
            source_text: String::new(),
        }
    }

    #[test]
    fn a_single_verse_during_a_main_reading_is_an_aside_until_read_on() {
        let mut memory = ReadingMemory::default();
        memory.cite(&cite("Luke", 1.0, 57.0, Some(80.0)), false, 0);
        memory.cite(&cite("Romans", 8.0, 28.0, None), false, 1);
        assert_eq!(memory.newest().unwrap().kind, ReadingKind::Aside);
        assert_eq!(memory.newest_main().unwrap().book, "Luke");
        memory.read("Romans", 8.0, 28.0, 2);
        assert_eq!(memory.newest().unwrap().kind, ReadingKind::Aside);
        memory.read("Romans", 8.0, 29.0, 3);
        assert_eq!(memory.newest().unwrap().kind, ReadingKind::Main);
    }

    #[test]
    fn resume_continues_after_the_last_verse_read() {
        let mut memory = ReadingMemory::default();
        memory.cite(&cite("Luke", 1.0, 57.0, Some(80.0)), true, 0);
        memory.shown(&cite("Luke", 1.0, 65.0, None), 1);
        assert_eq!(memory.newest().unwrap().resume_verse(), 65.0);
        memory.read("Luke", 1.0, 65.0, 2);
        assert_eq!(memory.newest().unwrap().resume_verse(), 66.0);
        memory.read("Luke", 1.0, 80.0, 3);
        memory.shown(&cite("Luke", 1.0, 80.0, None), 4);
        assert_eq!(memory.newest().unwrap().resume_verse(), 80.0, "chapter end");
    }

    #[test]
    fn a_reading_paused_for_an_aside_still_covers_its_verses() {
        let mut memory = ReadingMemory::default();
        memory.cite(&cite("Luke", 1.0, 57.0, Some(80.0)), true, 0);
        memory.cite(&cite("Romans", 8.0, 28.0, None), false, 60_000);
        let window = 15 * 60_000;
        assert!(memory.covers(&cite("Luke", 1.0, 66.0, None), 120_000, window));
        assert!(!memory.covers(&cite("Luke", 2.0, 1.0, None), 120_000, window));
        assert!(
            !memory.covers(&cite("Luke", 1.0, 20.0, None), 120_000, window),
            "before the reading"
        );
        assert!(
            !memory.covers(&cite("Luke", 1.0, 66.0, None), window + 1, window),
            "too long ago"
        );
    }

    #[test]
    fn keeps_at_most_eight_readings() {
        let mut memory = ReadingMemory::default();
        for chapter in 1..=10 {
            memory.cite(&cite("Psalms", chapter.into(), 1.0, None), true, 0);
        }
        assert_eq!(memory.readings().len(), CAPACITY);
        assert_eq!(memory.newest().unwrap().chapter, 10.0);
    }
}
