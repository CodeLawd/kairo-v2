//! Section hotkeys. Port of `src/lib/lyrics-hotkeys.ts`.

use super::js_text::{len16, trim};
use super::{LyricsSectionType, LyricsSongSection};
use std::collections::HashSet;

/// A valid section hotkey (one letter or digit), upper-cased.
pub fn normalize_hotkey(raw: Option<&str>) -> Option<String> {
    let key = trim(raw?).to_uppercase();
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_uppercase() || c.is_ascii_digit() => Some(key),
        _ => None,
    }
}

/// The hotkey a pressed `KeyboardEvent.key` stands for. Keys longer than one
/// UTF-16 code unit (`Enter`, an emoji) are never hotkeys.
pub fn hotkey_from_event(key: &str) -> Option<String> {
    if len16(key) == 1 {
        normalize_hotkey(Some(key))
    } else {
        None
    }
}

/// What the hotkey helpers need from a section.
pub trait HotkeySection: Clone {
    fn section_type(&self) -> LyricsSectionType;
    fn hotkey(&self) -> Option<&str>;
    fn set_hotkey(&mut self, hotkey: Option<String>);
}

impl HotkeySection for LyricsSongSection {
    fn section_type(&self) -> LyricsSectionType {
        self.kind
    }
    fn hotkey(&self) -> Option<&str> {
        self.hotkey.as_deref()
    }
    fn set_hotkey(&mut self, hotkey: Option<String>) {
        self.hotkey = hotkey;
    }
}

/// A hotkey as JavaScript tests it for truth: an empty string is unset.
fn set_key<S: HotkeySection>(section: &S) -> Option<&str> {
    section.hotkey().filter(|k| !k.is_empty())
}

/// Index of the section a key belongs to (TypeScript returns -1 for none).
pub fn section_for_hotkey<S: HotkeySection>(sections: &[S], key: &str) -> Option<usize> {
    sections.iter().position(|s| s.hotkey() == Some(key))
}

fn type_key(kind: LyricsSectionType) -> Option<&'static str> {
    match kind {
        LyricsSectionType::Chorus => Some("C"),
        LyricsSectionType::Bridge => Some("B"),
        LyricsSectionType::PreChorus => Some("P"),
        LyricsSectionType::Tag => Some("T"),
        LyricsSectionType::Intro => Some("I"),
        LyricsSectionType::Outro => Some("O"),
        LyricsSectionType::Ending => Some("E"),
        LyricsSectionType::Verse => None,
    }
}

/// Suggested hotkeys, ProPresenter-style: Verse 1–9 → 1–9, Chorus → C,
/// Bridge → B, Pre-chorus → P, Tag → T, Intro → I, Outro → O, Ending → E.
/// Keys already set are kept; a section whose suggestion is taken stays
/// unassigned.
pub fn auto_assign_hotkeys<S: HotkeySection>(sections: &[S]) -> Vec<S> {
    let mut used: HashSet<String> = sections
        .iter()
        .filter_map(|s| set_key(s).map(str::to_string))
        .collect();
    let mut verse = 0;
    sections
        .iter()
        .map(|section| {
            if section.section_type() == LyricsSectionType::Verse {
                verse += 1;
            }
            if set_key(section).is_some() {
                return section.clone();
            }
            let suggestion = if section.section_type() == LyricsSectionType::Verse {
                (verse <= 9).then(|| verse.to_string())
            } else {
                type_key(section.section_type()).map(str::to_string)
            };
            match suggestion {
                Some(key) if !used.contains(&key) => {
                    used.insert(key.clone());
                    let mut next = section.clone();
                    next.set_hotkey(Some(key));
                    next
                }
                _ => section.clone(),
            }
        })
        .collect()
}

/// Gives `key` to section `index`, taking it off any other section that had
/// it. An invalid or missing key clears the section's hotkey.
pub fn set_section_hotkey<S: HotkeySection>(
    sections: &[S],
    index: usize,
    key: Option<&str>,
) -> Vec<S> {
    let next = normalize_hotkey(key);
    sections
        .iter()
        .enumerate()
        .map(|(i, section)| {
            let mut section = section.clone();
            if i == index {
                section.set_hotkey(next.clone());
            } else if next.is_some() && section.hotkey() == next.as_deref() {
                section.set_hotkey(None);
            }
            section
        })
        .collect()
}
