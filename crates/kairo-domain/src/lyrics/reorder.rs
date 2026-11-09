//! Moving, labelling, deleting and editing slides across sections. Port of
//! `src/lib/lyrics-reorder.ts`.
//!
//! Slide indexes are flat across the song. Out-of-range indexes leave the
//! sections unchanged, as in TypeScript.

use super::js_text::{has_text, trim_end};
use super::{LyricsSectionType, LyricsSongSection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug)]
struct SlideBlock {
    source: usize,
    lines: Vec<String>,
    colors: Vec<Option<String>>,
}

/// One block per slide (a blank line is a slide break), remembering its
/// section. A section with no lyrics gives one empty block.
fn slide_blocks(sections: &[LyricsSongSection]) -> Vec<SlideBlock> {
    let mut blocks = Vec::new();
    for (source, section) in sections.iter().enumerate() {
        let empty = || SlideBlock {
            source,
            lines: Vec::new(),
            colors: Vec::new(),
        };
        let mut block = empty();
        for (index, line) in section.lines.iter().enumerate() {
            if !has_text(line) {
                if !block.lines.is_empty() {
                    blocks.push(std::mem::replace(&mut block, empty()));
                }
            } else {
                block.lines.push(line.clone());
                block.colors.push(
                    section
                        .line_colors
                        .as_ref()
                        .and_then(|c| c.get(index).cloned().flatten()),
                );
            }
        }
        if !block.lines.is_empty() || !section.lines.iter().any(|l| has_text(l)) {
            blocks.push(block);
        }
    }
    blocks
}

fn index_in(index: i64, len: usize) -> Option<usize> {
    usize::try_from(index).ok().filter(|&i| i < len)
}

fn with_block(base: &LyricsSongSection, block: &SlideBlock) -> LyricsSongSection {
    LyricsSongSection {
        lines: block.lines.clone(),
        line_colors: Some(block.colors.clone()),
        ..base.clone()
    }
}

fn append(last: &mut LyricsSongSection, block: &SlideBlock) {
    last.lines.push(String::new());
    last.lines.extend(block.lines.iter().cloned());
    let colors = last.line_colors.get_or_insert_with(Vec::new);
    colors.push(None);
    colors.extend(block.colors.iter().cloned());
}

/// Moves a slide block, keeping line colours and section identity.
pub fn reorder_lyric_slide(
    sections: &[LyricsSongSection],
    from: i64,
    to: i64,
) -> Vec<LyricsSongSection> {
    let mut blocks = slide_blocks(sections);
    let (Some(from), Some(to)) = (index_in(from, blocks.len()), index_in(to, blocks.len())) else {
        return sections.to_vec();
    };
    if from == to {
        return sections.to_vec();
    }
    let moved = blocks.remove(from);
    blocks.insert(to, moved);
    let mut result: Vec<LyricsSongSection> = Vec::new();
    let mut previous = None;
    for block in &blocks {
        let last_has_lines = result.last().is_some_and(|l| !l.lines.is_empty());
        if previous != Some(block.source) || block.lines.is_empty() || !last_has_lines {
            result.push(with_block(&sections[block.source], block));
        } else if let Some(last) = result.last_mut() {
            append(last, block);
        }
        previous = Some(block.source);
    }
    unique_hotkeys(result)
}

/// The section a set of slides is relabelled as.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideLabel {
    #[serde(rename = "type")]
    pub kind: LyricsSectionType,
    pub label: String,
}

/// Gives the chosen slides a section label, keeping order and colours.
/// Neighbouring slides that end up with the same type and label merge.
pub fn label_lyric_slides(
    sections: &[LyricsSongSection],
    slide_indexes: impl IntoIterator<Item = i64>,
    target: &SlideLabel,
) -> Vec<LyricsSongSection> {
    let chosen: HashSet<i64> = slide_indexes.into_iter().collect();
    let blocks = slide_blocks(sections);
    if !chosen.iter().any(|&i| index_in(i, blocks.len()).is_some()) {
        return sections.to_vec();
    }
    let mut result: Vec<LyricsSongSection> = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let base = &sections[block.source];
        let picked = chosen.contains(&(index as i64));
        let kind = if picked { target.kind } else { base.kind };
        let label = if picked { &target.label } else { &base.label };
        match result.last_mut() {
            Some(last)
                if last.kind == kind
                    && last.label == *label
                    && !last.lines.is_empty()
                    && !block.lines.is_empty() =>
            {
                append(last, block);
            }
            _ => {
                let mut section = with_block(base, block);
                section.kind = kind;
                section.label = label.clone();
                result.push(section);
            }
        }
    }
    unique_hotkeys(result)
}

/// Removes slides by flat index and rejoins what is left.
pub fn delete_lyric_slides(
    sections: &[LyricsSongSection],
    slide_indexes: impl IntoIterator<Item = i64>,
) -> Vec<LyricsSongSection> {
    let chosen: HashSet<i64> = slide_indexes.into_iter().collect();
    let blocks = slide_blocks(sections);
    if !chosen.iter().any(|&i| index_in(i, blocks.len()).is_some()) {
        return sections.to_vec();
    }
    let kept = blocks
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !chosen.contains(&(*i as i64)))
        .map(|(_, b)| b)
        .collect::<Vec<_>>();
    join_blocks(sections, &kept)
}

/// A slide's own lyric lines (before display wrapping).
pub fn lyric_slide_lines(sections: &[LyricsSongSection], index: i64) -> Option<Vec<String>> {
    let blocks = slide_blocks(sections);
    index_in(index, blocks.len()).map(|i| blocks[i].lines.clone())
}

/// Replaces one slide's lines (Quick Edit). An unchanged line keeps its
/// colour; edited lines go back to automatic. A blank line starts another
/// slide in the same section.
pub fn replace_lyric_slide(
    sections: &[LyricsSongSection],
    index: i64,
    lines: &[String],
) -> Vec<LyricsSongSection> {
    let mut blocks = slide_blocks(sections);
    let Some(index) = index_in(index, blocks.len()) else {
        return sections.to_vec();
    };
    let mapped: Vec<&str> = lines.iter().map(|l| trim_end(l)).collect();
    let mut tidy: Vec<&str> = mapped
        .iter()
        .enumerate()
        .filter(|(i, l)| has_text(l) || (*i > 0 && has_text(mapped[i - 1])))
        .map(|(_, l)| *l)
        .collect();
    while tidy.last().is_some_and(|l| !has_text(l)) {
        tidy.pop();
    }
    if tidy.is_empty() {
        return sections.to_vec();
    }
    let block = &blocks[index];
    let colors = tidy
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if has_text(line) && block.lines.get(i).is_some_and(|l| l == line) {
                block.colors.get(i).cloned().flatten()
            } else {
                None
            }
        })
        .collect();
    blocks[index] = SlideBlock {
        source: block.source,
        lines: tidy.iter().map(|l| l.to_string()).collect(),
        colors,
    };
    join_blocks(sections, &blocks)
}

/// Rebuilds sections from blocks, merging neighbours that share a type and
/// label.
fn join_blocks(sections: &[LyricsSongSection], blocks: &[SlideBlock]) -> Vec<LyricsSongSection> {
    let mut result: Vec<LyricsSongSection> = Vec::new();
    for block in blocks {
        let base = &sections[block.source];
        match result.last_mut() {
            Some(last)
                if last.kind == base.kind
                    && last.label == base.label
                    && !last.lines.is_empty()
                    && !block.lines.is_empty() =>
            {
                append(last, block);
            }
            _ => result.push(with_block(base, block)),
        }
    }
    unique_hotkeys(result)
}

/// Where a moved slide lands relative to its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Before,
    After,
}

/// Moves a slide next to another and makes it part of that slide's section.
/// Sections that end up side by side with the same label merge.
pub fn move_lyric_slide(
    sections: &[LyricsSongSection],
    from: i64,
    target: i64,
    side: Side,
) -> Vec<LyricsSongSection> {
    let blocks = slide_blocks(sections);
    let (Some(from), Some(target)) = (index_in(from, blocks.len()), index_in(target, blocks.len()))
    else {
        return sections.to_vec();
    };
    let insert_at = if side == Side::Before {
        target
    } else {
        target + 1
    };
    if (insert_at == from || insert_at == from + 1) && blocks[from].source == blocks[target].source
    {
        return sections.to_vec();
    }
    let moved = SlideBlock {
        source: blocks[target].source,
        ..blocks[from].clone()
    };
    let mut rest: Vec<SlideBlock> = blocks
        .into_iter()
        .enumerate()
        .filter(|(i, _)| *i != from)
        .map(|(_, b)| b)
        .collect();
    let at = if from < insert_at {
        insert_at - 1
    } else {
        insert_at
    };
    rest.insert(at.min(rest.len()), moved);
    join_blocks(sections, &rest)
}

/// A hotkey stays on the first of the sections that carry it.
fn unique_hotkeys(sections: Vec<LyricsSongSection>) -> Vec<LyricsSongSection> {
    let mut seen = HashSet::new();
    sections
        .into_iter()
        .map(|mut section| {
            if let Some(key) = section.hotkey.as_deref().filter(|k| !k.is_empty())
                && !seen.insert(key.to_string())
            {
                section.hotkey = None;
            }
            section
        })
        .collect()
}
