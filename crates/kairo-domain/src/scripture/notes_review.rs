//! Sermon notes review helpers, from `src/lib/sermon-notes-review.ts`.
//!
//! The source works on a TipTap (ProseMirror) document. This port keeps only
//! the data model the offsets depend on, [`SermonNotesNode`], with
//! ProseMirror's position rules; the editor that displays it is prompt 079's.

use super::{SermonReferenceMatch, js};
use serde::{Deserialize, Serialize};

/// Must match the block separator the notes text is scanned with.
pub const SERMON_NOTES_BLOCK_SEPARATOR: &str = "\n";

/// The node type whose text is a line break (`hardBreak` in StarterKit).
pub const HARD_BREAK: &str = "hardBreak";

fn comparison_key(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

/// A canonical label only when the source notation needs interpretation:
/// `ps 2 3` gets `Psalms 2:3`, `Mark 4:2 NLT` gets nothing.
pub fn normalized_reference_label(found: &SermonReferenceMatch) -> Option<String> {
    let source = comparison_key(&found.text);
    let canonical = comparison_key(&found.reference);
    (!source.starts_with(&canonical)).then(|| found.reference.clone())
}

pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Plain text to editor HTML that survives HTML whitespace collapsing: one
/// paragraph per line, tabs as four spaces, edge and repeated spaces as `&nbsp;`.
pub fn plain_text_to_editor_html(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    normalized
        .split('\n')
        .map(|line| {
            let mut escaped = escape_html(&line.replace('\t', "    "));
            if let Some(rest) = escaped.strip_prefix(' ') {
                escaped = format!("&nbsp;{rest}");
            }
            if let Some(rest) = escaped.strip_suffix(' ') {
                escaped = format!("{rest}&nbsp;");
            }
            let escaped = nbsp_space_runs(&escaped);
            let body = if escaped.is_empty() { "<br>" } else { &escaped };
            format!("<p>{body}</p>")
        })
        .collect()
}

/// `replace(/ {2,}/g, spaces => '&nbsp;'.repeat(spaces.length))`.
fn nbsp_space_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = 0;
    let flush = |out: &mut String, run: usize| {
        if run >= 2 {
            out.push_str(&"&nbsp;".repeat(run));
        } else if run == 1 {
            out.push(' ');
        }
    };
    for c in text.chars() {
        if c == ' ' {
            run += 1;
        } else {
            flush(&mut out, run);
            run = 0;
            out.push(c);
        }
    }
    flush(&mut out, run);
    out
}

/// A notes document node with ProseMirror's sizing rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SermonNotesNode {
    /// Inline text. Its size is its UTF-16 length.
    Text { text: String },
    /// Any other node. A leaf (`hardBreak`, a horizontal rule) has size 1;
    /// otherwise its size is its content plus 2, even when empty.
    Element {
        #[serde(rename = "type")]
        type_name: String,
        block: bool,
        leaf: bool,
        #[serde(default)]
        content: Vec<SermonNotesNode>,
    },
}

impl SermonNotesNode {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn block(type_name: impl Into<String>, content: Vec<SermonNotesNode>) -> Self {
        Self::Element {
            type_name: type_name.into(),
            block: true,
            leaf: false,
            content,
        }
    }

    pub fn hard_break() -> Self {
        Self::Element {
            type_name: HARD_BREAK.into(),
            block: false,
            leaf: true,
            content: Vec::new(),
        }
    }

    pub fn node_size(&self) -> usize {
        match self {
            Self::Text { text } => js::utf16_len(text),
            Self::Element { leaf: true, .. } => 1,
            Self::Element { .. } => self.content_size() + 2,
        }
    }

    pub fn content(&self) -> &[SermonNotesNode] {
        match self {
            Self::Text { .. } => &[],
            Self::Element { content, .. } => content,
        }
    }

    pub fn content_size(&self) -> usize {
        self.content().iter().map(Self::node_size).sum()
    }

    fn is_block(&self) -> bool {
        matches!(self, Self::Element { block: true, .. })
    }

    fn is_hard_break(&self) -> bool {
        matches!(self, Self::Element { type_name, .. } if type_name == HARD_BREAK)
    }
}

/// ProseMirror `Fragment.nodesBetween`. `f` gets `(node, pos)` and returns
/// whether to descend.
fn nodes_between(
    children: &[SermonNotesNode],
    from: usize,
    to: usize,
    node_start: usize,
    f: &mut impl FnMut(&SermonNotesNode, usize) -> bool,
) {
    let mut pos = 0;
    for child in children {
        if pos >= to {
            break;
        }
        let end = pos + child.node_size();
        if end > from && f(child, node_start + pos) && child.content_size() > 0 {
            let start = pos + 1;
            nodes_between(
                child.content(),
                from.saturating_sub(start),
                child.content_size().min(to.saturating_sub(start)),
                node_start + start,
                f,
            );
        }
        pos = end;
    }
}

/// The document's text as TipTap `getText({ blockSeparator })` gives it with
/// StarterKit: a separator before every block after the start, `\n` for a
/// hard break. Offsets into this text are what [`map_text_range_to_pos`] maps.
pub fn notes_plain_text(doc: &SermonNotesNode, block_separator: &str) -> String {
    let mut text = String::new();
    nodes_between(doc.content(), 0, doc.content_size(), 0, &mut |node, pos| {
        if node.is_block() && pos > 0 {
            text.push_str(block_separator);
        }
        if node.is_hard_break() {
            text.push('\n');
            return false;
        }
        if let SermonNotesNode::Text { text: t } = node {
            text.push_str(t);
        }
        true
    });
    text
}

/// A document position range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocRange {
    pub from: usize,
    pub to: usize,
}

/// Maps a range of [`notes_plain_text`] (UTF-16 offsets) onto document
/// positions. Naive `offset + 1` only works for one text block: every block
/// boundary shifts positions. `None` for a negative, reversed, empty or
/// out-of-range range.
pub fn map_text_range_to_pos(
    doc: &SermonNotesNode,
    start: f64,
    end: f64,
    block_separator: &str,
) -> Option<DocRange> {
    // `!(end >= start)` also rejects NaN, which the source rejects later.
    if start < 0.0 || end < start {
        return None;
    }
    let separator = js::utf16_len(block_separator) as f64;
    let mut chars = 0.0;
    let mut from_pos: Option<usize> = None;
    let mut to_pos: Option<usize> = None;
    nodes_between(doc.content(), 0, doc.content_size(), 0, &mut |node, pos| {
        if to_pos.is_some() {
            return false;
        }
        // Same rule as TipTap getTextBetween: a separator before every block after the start.
        if node.is_block() && pos > 0 {
            chars += separator;
        }
        // StarterKit's hard break serialises as "\n" and stops descent.
        if node.is_hard_break() {
            if from_pos.is_none() && chars == start {
                from_pos = Some(pos);
            }
            chars += 1.0;
            if chars == end {
                to_pos = Some(pos + node.node_size());
            }
            return false;
        }
        if let SermonNotesNode::Text { text } = node {
            for i in 0..js::utf16_len(text) {
                let doc_pos = pos + i;
                if from_pos.is_none() && chars == start {
                    from_pos = Some(doc_pos);
                }
                chars += 1.0;
                if chars == end {
                    to_pos = Some(doc_pos + 1);
                    return false;
                }
            }
        }
        true
    });
    match (from_pos, to_pos) {
        (Some(from), Some(to)) if from < to => Some(DocRange { from, to }),
        _ => None,
    }
}
