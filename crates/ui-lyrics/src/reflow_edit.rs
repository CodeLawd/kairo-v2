//! The song editor's document (`SongReflowEditor.tsx`), as pure rules tested
//! without a window: sections ("blocks") of slides, typing, the caret split
//! (⌥↵) and join (Backspace at the start), structured paste, section type,
//! label, order and delete, new sections, and the editor's own undo history
//! with typing bursts. Caret offsets are UTF-8 bytes, as Kit inputs report.

use std::sync::LazyLock;

use kairo_domain::lyrics::reflow::{next_section_label, parse_reflow, reflow_header_label};
use kairo_domain::lyrics::{LyricsSectionType, LyricsSong, LyricsSongSection};
use regex::Regex;

/// Keystrokes in one slide closer together than this undo as one step.
pub const TYPING_BURST_MS: u64 = 1000;
pub const MAX_UNDO: usize = 200;

/// A blank line (or a run of them): where one slide ends and the next begins.
static BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n[ \t]*\n+").unwrap());
static TRAILING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+$").unwrap());
static LEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s+").unwrap());

/// `SECTION_TYPES`, in menu order.
pub const SECTION_TYPES: [(LyricsSectionType, &str); 8] = [
    (LyricsSectionType::Verse, "Verse"),
    (LyricsSectionType::PreChorus, "Pre-Chorus"),
    (LyricsSectionType::Chorus, "Chorus"),
    (LyricsSectionType::Bridge, "Bridge"),
    (LyricsSectionType::Tag, "Tag"),
    (LyricsSectionType::Intro, "Intro"),
    (LyricsSectionType::Outro, "Outro"),
    (LyricsSectionType::Ending, "Ending"),
];

/// The longest section name.
pub const MAX_LABEL: usize = 40;

/// One section of the song as the editor holds it (`EditSection`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSection {
    pub kind: LyricsSectionType,
    pub label: String,
    /// The lines, with a blank line between slides.
    pub lines_text: String,
    pub hotkey: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slide {
    pub key: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub key: u64,
    pub kind: LyricsSectionType,
    pub label: String,
    pub hotkey: Option<String>,
    pub slides: Vec<Slide>,
}

/// Where the cursor is: a slide and a byte offset in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caret {
    pub slide: u64,
    pub at: usize,
}

#[derive(Clone, Debug)]
struct Snapshot {
    blocks: Vec<Block>,
    caret: Option<Caret>,
}

/// `songToEdit` sections.
pub fn edit_sections(song: &LyricsSong) -> Vec<EditSection> {
    song.sections
        .iter()
        .map(|s| EditSection {
            kind: s.kind,
            label: s.label.clone(),
            lines_text: s.lines.join("\n"),
            hotkey: s.hotkey.clone(),
        })
        .collect()
}

/// `editToSections`: back to song sections, slide breaks kept.
pub fn song_sections(sections: &[EditSection]) -> Vec<LyricsSongSection> {
    sections
        .iter()
        .map(|s| {
            let raw: Vec<&str> = s.lines_text.split('\n').collect();
            LyricsSongSection {
                kind: s.kind,
                label: s.label.clone(),
                lines: kairo_domain::lyrics::slides::preserve_slide_breaks(&raw),
                line_colors: None,
                hotkey: kairo_domain::lyrics::hotkeys::normalize_hotkey(s.hotkey.as_deref()),
                extra: Default::default(),
            }
        })
        .collect()
}

/// A label that only names its type ("Verse 2", "Chorus"): renamed along
/// with a type change.
pub fn is_stock_label(label: &str, kind: LyricsSectionType) -> bool {
    let name = SECTION_TYPES
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or(kind.as_str(), |(_, n)| n);
    let label = label.trim();
    let Some(rest) = label
        .get(..name.len())
        .filter(|head| head.eq_ignore_ascii_case(name))
        .map(|_| &label[name.len()..])
    else {
        return false;
    };
    rest.is_empty()
        || rest
            .strip_prefix(' ')
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// The editor's document and history.
#[derive(Clone, Debug)]
pub struct ReflowDoc {
    blocks: Vec<Block>,
    next_key: u64,
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    typing: Option<u64>,
    typed_at: u64,
}

impl ReflowDoc {
    /// `toBlocks`: an empty song becomes one empty Verse 1.
    pub fn new(sections: &[EditSection]) -> Self {
        let mut doc = Self {
            blocks: Vec::new(),
            next_key: 1,
            past: Vec::new(),
            future: Vec::new(),
            typing: None,
            typed_at: 0,
        };
        doc.blocks = doc.build_blocks(sections);
        doc
    }

    fn key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key
    }

    fn build_blocks(&mut self, sections: &[EditSection]) -> Vec<Block> {
        let empty = [EditSection {
            kind: LyricsSectionType::Verse,
            label: "Verse 1".into(),
            lines_text: String::new(),
            hotkey: None,
        }];
        let source = if sections.is_empty() {
            &empty[..]
        } else {
            sections
        };
        source
            .iter()
            .map(|s| {
                let slides = BREAK
                    .split(&s.lines_text)
                    .map(|t| TRAILING.replace(t, "").into_owned())
                    .collect::<Vec<_>>();
                let block_key = self.key();
                Block {
                    key: block_key,
                    kind: s.kind,
                    label: s.label.clone(),
                    hotkey: s.hotkey.clone(),
                    slides: slides
                        .into_iter()
                        .map(|text| Slide {
                            key: self.key(),
                            text,
                        })
                        .collect(),
                }
            })
            .collect()
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// `toSections`.
    pub fn sections(&self) -> Vec<EditSection> {
        self.blocks
            .iter()
            .map(|b| {
                let label = b.label.trim();
                EditSection {
                    kind: b.kind,
                    label: if label.is_empty() {
                        "Verse".into()
                    } else {
                        label.to_owned()
                    },
                    // A blank line typed inside a slide is just space: it must
                    // not come back as a slide break.
                    lines_text: b
                        .slides
                        .iter()
                        .map(|s| {
                            TRAILING
                                .replace(&BREAK.replace_all(&s.text, "\n"), "")
                                .into_owned()
                        })
                        .filter(|t| !t.trim().is_empty())
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                    hotkey: b.hotkey.clone(),
                }
            })
            .collect()
    }

    /// Replaces the document from outside (a translation, a reload).
    pub fn reset(&mut self, sections: &[EditSection]) {
        self.blocks = self.build_blocks(sections);
        self.past.clear();
        self.future.clear();
        self.typing = None;
    }

    pub fn first_slide(&self) -> Option<u64> {
        self.blocks.first()?.slides.first().map(|s| s.key)
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    fn find(&self, slide: u64) -> Option<(usize, usize)> {
        self.blocks.iter().enumerate().find_map(|(b, block)| {
            block
                .slides
                .iter()
                .position(|s| s.key == slide)
                .map(|s| (b, s))
        })
    }

    /// The 1-based number of each slide through the song.
    pub fn slide_number(&self, slide: u64) -> Option<usize> {
        let mut n = 0;
        for block in &self.blocks {
            for s in &block.slides {
                n += 1;
                if s.key == slide {
                    return Some(n);
                }
            }
        }
        None
    }

    fn commit(&mut self, next: Vec<Block>, caret: Option<Caret>, typing: Option<u64>, now: u64) {
        let same_burst = typing.is_some()
            && self.typing == typing
            && now.saturating_sub(self.typed_at) < TYPING_BURST_MS;
        if !same_burst {
            self.past.push(Snapshot {
                blocks: self.blocks.clone(),
                caret,
            });
            if self.past.len() > MAX_UNDO {
                self.past.remove(0);
            }
        }
        self.future.clear();
        self.typing = typing;
        self.typed_at = now;
        self.blocks = if next.is_empty() {
            self.build_blocks(&[])
        } else {
            next
        };
    }

    /// Undo; returns where to put the cursor.
    pub fn undo(&mut self, caret: Option<Caret>) -> Option<Option<Caret>> {
        let target = self.past.pop()?;
        self.future.push(Snapshot {
            blocks: std::mem::replace(&mut self.blocks, target.blocks),
            caret,
        });
        self.typing = None;
        Some(target.caret)
    }

    pub fn redo(&mut self, caret: Option<Caret>) -> Option<Option<Caret>> {
        let target = self.future.pop()?;
        self.past.push(Snapshot {
            blocks: std::mem::replace(&mut self.blocks, target.blocks),
            caret,
        });
        self.typing = None;
        Some(target.caret)
    }

    fn with_slides(&self, b: usize, slides: Vec<Slide>) -> Vec<Block> {
        let mut next = self.blocks.clone();
        next[b].slides = slides;
        next
    }

    /// Typing only ever changes this slide; Enter is a new line.
    pub fn edit(&mut self, slide: u64, text: String, caret: Option<Caret>, now: u64) {
        let Some((b, s)) = self.find(slide) else {
            return;
        };
        if self.blocks[b].slides[s].text == text {
            return;
        }
        let mut slides = self.blocks[b].slides.clone();
        slides[s].text = text;
        let next = self.with_slides(b, slides);
        self.commit(next, caret, Some(slide), now);
    }

    /// Replaces one slide with several; returns the caret to place.
    fn replace_slide(
        &mut self,
        b: usize,
        s: usize,
        texts: Vec<String>,
        (focus, at): (usize, usize),
        caret: Option<Caret>,
        now: u64,
    ) -> Caret {
        let first = self.blocks[b].slides[s].key;
        let fresh: Vec<Slide> = texts
            .into_iter()
            .enumerate()
            .map(|(i, text)| Slide {
                key: if i == 0 { first } else { self.key() },
                text,
            })
            .collect();
        let target = Caret {
            slide: fresh[focus].key,
            at,
        };
        let mut slides = self.blocks[b].slides.clone();
        slides.splice(s..=s, fresh);
        let next = self.with_slides(b, slides);
        self.commit(next, caret, None, now);
        target
    }

    /// ⌥↵ (or ⌘↵): everything from the cursor moves to a new slide below.
    pub fn break_at(&mut self, slide: u64, cursor: usize, now: u64) -> Option<Caret> {
        let (b, s) = self.find(slide)?;
        let text = &self.blocks[b].slides[s].text;
        let cursor = floor_char(text, cursor);
        let head = TRAILING.replace(&text[..cursor], "").into_owned();
        let tail = LEADING.replace(&text[cursor..], "").into_owned();
        let before = Some(Caret { slide, at: cursor });
        Some(self.replace_slide(b, s, vec![head, tail], (1, 0), before, now))
    }

    /// Backspace at the very start: this slide joins the end of the one
    /// above. `None` when there is no slide above in the section.
    pub fn join_up(&mut self, slide: u64, now: u64) -> Option<Caret> {
        let (b, s) = self.find(slide)?;
        if s == 0 {
            return None;
        }
        let above = self.blocks[b].slides[s - 1].clone();
        let current = &self.blocks[b].slides[s];
        let joined = [above.text.as_str(), current.text.as_str()]
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let at = if above.text.trim().is_empty() {
            0
        } else {
            above.text.len() + usize::from(!current.text.trim().is_empty())
        };
        let mut slides = self.blocks[b].slides.clone();
        slides.splice(
            s - 1..=s,
            [Slide {
                key: above.key,
                text: joined,
            }],
        );
        let next = self.with_slides(b, slides);
        self.commit(next, Some(Caret { slide, at: 0 }), None, now);
        Some(Caret {
            slide: above.key,
            at,
        })
    }

    /// A paste that carries a song's shape keeps it: blank lines become
    /// slides and `[Chorus]` lines sections. `None`: paste as plain text.
    pub fn paste(
        &mut self,
        slide: u64,
        selection: std::ops::Range<usize>,
        pasted: &str,
        now: u64,
    ) -> Option<Caret> {
        let clean = pasted.replace("\r\n", "\n").replace('\r', "\n");
        let has_header = clean.split('\n').any(|l| reflow_header_label(l).is_some());
        if !has_header && !BREAK.is_match(clean.trim()) {
            return None;
        }
        let (b, s) = self.find(slide)?;
        let current = &self.blocks[b].slides[s].text;
        let start = floor_char(current, selection.start.min(current.len()));
        let end = floor_char(current, selection.end.min(current.len())).max(start);
        let value = format!("{}{}{}", &current[..start], clean, &current[end..]);
        let before = Some(Caret { slide, at: start });
        if has_header {
            return self.paste_song(b, s, &value, before, now);
        }
        let parts: Vec<String> = BREAK
            .split(&value)
            .map(|p| TRAILING.replace(p, "").into_owned())
            .collect();
        let last = parts.len() - 1;
        let at = parts[last].len();
        Some(self.replace_slide(b, s, parts, (last, at), before, now))
    }

    fn labels(blocks: &[Block]) -> Vec<String> {
        blocks.iter().map(|b| b.label.clone()).collect()
    }

    fn paste_song(
        &mut self,
        b: usize,
        s: usize,
        value: &str,
        caret: Option<Caret>,
        now: u64,
    ) -> Option<Caret> {
        let parsed = parse_reflow(value);
        let block = self.blocks[b].clone();
        let mut before: Vec<Slide> = block.slides[..s].to_vec();
        let after: Vec<Slide> = block.slides[s + 1..].to_vec();
        let mut pasted: Vec<Block> = Vec::new();
        for section in parsed {
            let slides: Vec<Slide> = section
                .slides
                .iter()
                .map(|sl| Slide {
                    key: self.key(),
                    text: sl.lines.join("\n"),
                })
                .collect();
            if section.header_line.is_none() {
                // Text above the first [Header] stays in this section.
                before.extend(slides);
                continue;
            }
            let slides = if slides.is_empty() {
                vec![Slide {
                    key: self.key(),
                    text: String::new(),
                }]
            } else {
                slides
            };
            pasted.push(Block {
                key: self.key(),
                kind: section.kind,
                label: section.label,
                hotkey: None,
                slides,
            });
        }
        let head: Vec<Block> = if before.is_empty() {
            Vec::new()
        } else {
            vec![Block {
                slides: before.clone(),
                ..block.clone()
            }]
        };
        let tail: Vec<Block> = if after.is_empty() {
            Vec::new()
        } else {
            let mut all = self.blocks.clone();
            all.extend(pasted.iter().cloned());
            let label = next_section_label(&Self::labels(&all), block.kind);
            vec![Block {
                key: self.key(),
                kind: block.kind,
                label,
                hotkey: None,
                slides: after,
            }]
        };
        let last = pasted
            .last()
            .and_then(|p| p.slides.last())
            .or(before.last())
            .map(|s| Caret {
                slide: s.key,
                at: s.text.len(),
            });
        let mut next = self.blocks[..b].to_vec();
        next.extend(head);
        next.extend(pasted);
        next.extend(tail);
        next.extend(self.blocks[b + 1..].iter().cloned());
        self.commit(next, caret, None, now);
        last
    }

    fn update_block(&mut self, b: usize, f: impl FnOnce(&mut Block), now: u64) {
        let mut next = self.blocks.clone();
        f(&mut next[b]);
        self.commit(next, None, None, now);
    }

    /// The section's name (at most 40 characters).
    pub fn rename(&mut self, b: usize, label: &str, now: u64) {
        if b >= self.blocks.len() {
            return;
        }
        let label: String = label.chars().take(MAX_LABEL).collect();
        if self.blocks[b].label == label {
            return;
        }
        self.update_block(b, |block| block.label = label, now);
    }

    /// A stock label follows the new type ("Verse 2" → "Chorus").
    pub fn set_type(&mut self, b: usize, kind: LyricsSectionType, now: u64) {
        if b >= self.blocks.len() {
            return;
        }
        let block = &self.blocks[b];
        let label = if is_stock_label(&block.label, block.kind) {
            let others: Vec<String> = self
                .blocks
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != b)
                .map(|(_, x)| x.label.clone())
                .collect();
            next_section_label(&others, kind)
        } else {
            block.label.clone()
        };
        self.update_block(
            b,
            |block| {
                block.kind = kind;
                block.label = label;
            },
            now,
        );
    }

    pub fn move_block(&mut self, b: usize, to: usize, now: u64) {
        if b >= self.blocks.len() || to >= self.blocks.len() || b == to {
            return;
        }
        let mut next = self.blocks.clone();
        let moved = next.remove(b);
        next.insert(to, moved);
        self.commit(next, None, None, now);
    }

    pub fn delete_block(&mut self, b: usize, now: u64) {
        if b >= self.blocks.len() {
            return;
        }
        let mut next = self.blocks.clone();
        next.remove(b);
        self.commit(next, None, None, now);
    }

    /// A new section starting at this slide: it and every slide below move in.
    pub fn start_section_at(
        &mut self,
        slide: u64,
        kind: LyricsSectionType,
        now: u64,
    ) -> Option<Caret> {
        let (b, s) = self.find(slide)?;
        let block = self.blocks[b].clone();
        let label = next_section_label(&Self::labels(&self.blocks), kind);
        let moved = Block {
            key: self.key(),
            kind,
            label,
            hotkey: None,
            slides: block.slides[s..].to_vec(),
        };
        let mut next = self.blocks[..b].to_vec();
        if s > 0 {
            next.push(Block {
                slides: block.slides[..s].to_vec(),
                ..block
            });
        }
        let caret = Caret {
            slide: moved.slides[0].key,
            at: 0,
        };
        next.push(moved);
        next.extend(self.blocks[b + 1..].iter().cloned());
        self.commit(next, None, None, now);
        Some(caret)
    }

    pub fn add_section(&mut self, kind: LyricsSectionType, now: u64) -> Caret {
        let slide = Slide {
            key: self.key(),
            text: String::new(),
        };
        let caret = Caret {
            slide: slide.key,
            at: 0,
        };
        let mut next = self.blocks.clone();
        next.push(Block {
            key: self.key(),
            kind,
            label: next_section_label(&Self::labels(&self.blocks), kind),
            hotkey: None,
            slides: vec![slide],
        });
        self.commit(next, None, None, now);
        caret
    }

    /// The last slide of a section takes the section with it.
    pub fn delete_slide(&mut self, slide: u64, now: u64) {
        let Some((b, s)) = self.find(slide) else {
            return;
        };
        let mut slides = self.blocks[b].slides.clone();
        slides.remove(s);
        if slides.is_empty() {
            self.delete_block(b, now);
        } else {
            let next = self.with_slides(b, slides);
            self.commit(next, None, None, now);
        }
    }
}

/// The nearest char boundary at or below `at`.
fn floor_char(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use LyricsSectionType::*;

    fn sec(kind: LyricsSectionType, label: &str, text: &str) -> EditSection {
        EditSection {
            kind,
            label: label.into(),
            lines_text: text.into(),
            hotkey: None,
        }
    }

    fn texts(doc: &ReflowDoc) -> Vec<(String, Vec<String>)> {
        doc.blocks()
            .iter()
            .map(|b| {
                (
                    b.label.clone(),
                    b.slides.iter().map(|s| s.text.clone()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn blank_lines_are_slides_and_round_trip() {
        let doc = ReflowDoc::new(&[sec(Verse, "Verse 1", "a\nb\n\n\nc  \n")]);
        assert_eq!(
            texts(&doc),
            [("Verse 1".into(), vec!["a\nb".into(), "c".into()])]
        );
        assert_eq!(doc.sections()[0].lines_text, "a\nb\n\nc");
        let empty = ReflowDoc::new(&[]);
        assert_eq!(texts(&empty), [("Verse 1".into(), vec![String::new()])]);
    }

    #[test]
    fn split_and_join_at_the_caret_with_yoruba_text() {
        let mut doc = ReflowDoc::new(&[sec(Verse, "Verse 1", "Ẹ ṣé o Olúwa\nỌba mi")]);
        let first = doc.first_slide().unwrap();
        let at = "Ẹ ṣé o Olúwa\n".len();
        let caret = doc.break_at(first, at, 0).unwrap();
        assert_eq!(texts(&doc)[0].1, ["Ẹ ṣé o Olúwa", "Ọba mi"]);
        assert_eq!(caret.at, 0);
        // A caret inside a multibyte character snaps to its start.
        let second = caret.slide;
        let back = doc.join_up(second, 10).unwrap();
        assert_eq!(texts(&doc)[0].1, ["Ẹ ṣé o Olúwa\nỌba mi"]);
        assert_eq!(back.at, "Ẹ ṣé o Olúwa".len() + 1);
        assert!(
            doc.join_up(back.slide, 20).is_none(),
            "nothing above the first slide"
        );
        // Byte 1 is inside "Ẹ": the split snaps to its start.
        doc.break_at(back.slide, 1, 30).unwrap();
        assert_eq!(texts(&doc)[0].1[0], "");
        assert!(texts(&doc)[0].1[1].starts_with("Ẹ ṣé"));
    }

    #[test]
    fn typing_bursts_undo_as_one_step_and_structure_changes_undo_too() {
        let mut doc = ReflowDoc::new(&[sec(Verse, "Verse 1", "a")]);
        let s = doc.first_slide().unwrap();
        doc.edit(s, "ab".into(), None, 0);
        doc.edit(s, "abc".into(), None, 500);
        doc.edit(s, "abcd".into(), None, 2000);
        assert!(doc.undo(None).is_some());
        assert_eq!(texts(&doc)[0].1, ["abc"]);
        doc.undo(None);
        assert_eq!(texts(&doc)[0].1, ["a"]);
        assert!(doc.undo(None).is_none());
        doc.redo(None);
        assert_eq!(texts(&doc)[0].1, ["abc"]);
        doc.break_at(s, 1, 3000);
        assert_eq!(texts(&doc)[0].1, ["a", "bc"]);
        doc.undo(None);
        assert_eq!(texts(&doc)[0].1, ["abc"]);
        assert!(!doc.can_redo() || doc.can_redo());
    }

    #[test]
    fn a_pasted_song_keeps_its_sections() {
        let mut doc = ReflowDoc::new(&[sec(Verse, "Verse 1", "keep\n\nafter")]);
        let first = doc.first_slide().unwrap();
        let caret = doc
            .paste(first, 4..4, "\n\n[Chorus]\nsing\n\nagain", 0)
            .unwrap();
        let t = texts(&doc);
        assert_eq!(t[0], ("Verse 1".into(), vec!["keep".into()]));
        assert_eq!(t[1], ("Chorus".into(), vec!["sing".into(), "again".into()]));
        assert_eq!(t[2].0, "Verse 2");
        assert_eq!(t[2].1, ["after"]);
        assert_eq!(caret.at, "again".len());
        // Plain text without breaks pastes normally.
        assert!(doc.paste(first, 0..0, "just words", 1).is_none());
        // Breaks without headers become slides.
        let s = doc.blocks()[1].slides[0].key;
        doc.paste(s, 4..4, "\nx\n\nmore", 2).unwrap();
        assert_eq!(texts(&doc)[1].1, ["sing\nx", "more", "again"]);
    }

    #[test]
    fn sections_change_type_move_split_add_and_delete() {
        let mut doc =
            ReflowDoc::new(&[sec(Verse, "Verse 1", "a\n\nb"), sec(Chorus, "Chorus", "c")]);
        doc.set_type(0, Bridge, 0);
        assert_eq!(doc.blocks()[0].label, "Bridge");
        doc.rename(0, "My own name", 1);
        doc.set_type(0, Tag, 2);
        assert_eq!(doc.blocks()[0].label, "My own name", "a custom name stays");
        doc.move_block(1, 0, 3);
        assert_eq!(doc.blocks()[0].label, "Chorus");
        let b = doc.blocks()[1].slides[1].key;
        doc.start_section_at(b, Verse, 4).unwrap();
        assert_eq!(texts(&doc)[2], ("Verse 1".into(), vec!["b".into()]));
        doc.add_section(Ending, 5);
        assert_eq!(doc.blocks().last().unwrap().label, "Ending");
        let only = doc.blocks()[2].slides[0].key;
        doc.delete_slide(only, 6);
        assert_eq!(
            doc.blocks().len(),
            3,
            "deleting the last slide deletes the section"
        );
        for _ in 0..3 {
            doc.delete_block(0, 7);
        }
        assert_eq!(
            texts(&doc),
            [("Verse 1".into(), vec![String::new()])],
            "never empty"
        );
        assert!(is_stock_label("chorus 3", Chorus));
        assert!(!is_stock_label("Chorus x", Chorus));
    }
}
