//! Slide text as the browser reads it: whitespace collapsed per paragraph,
//! `<br>` for each newline, `text-transform` applied last.

use crate::color::sanitize_overlay_color;
use crate::scene::{ColoredLine, Paragraph};
use kairo_domain::overlay::OverlayTextTransform;
use unicode_segmentation::UnicodeSegmentation;

/// `white-space: normal`: runs of space, tab, CR, form feed and LF become
/// one space and the ends are trimmed. A no-break space stays.
pub fn collapse(line: &str) -> String {
    line.split([' ', '\t', '\r', '\u{c}', '\n'])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text-transform`. `capitalize` upper-cases the first letter of each word.
pub fn transform(text: &str, how: OverlayTextTransform) -> String {
    match how {
        OverlayTextTransform::None => text.to_owned(),
        OverlayTextTransform::Uppercase => text.to_uppercase(),
        OverlayTextTransform::Lowercase => text.to_lowercase(),
        OverlayTextTransform::Capitalize => text
            .split_word_bounds()
            .map(
                |word| match word.char_indices().find(|(_, c)| c.is_alphabetic()) {
                    Some((index, first)) => format!(
                        "{}{}{}",
                        &word[..index],
                        first.to_uppercase(),
                        &word[index + first.len_utf8()..]
                    ),
                    None => word.to_owned(),
                },
            )
            .collect(),
    }
}

/// The paragraphs of a text body. Plain text splits at each `\n` (the
/// template turned those into `<br>`); coloured lines are one paragraph
/// each, and a `\n` inside one is only whitespace, because only the plain
/// path converts newlines. A trailing `<br>` makes no extra line, so a final
/// empty paragraph is dropped; blank ones between others stay as blank lines.
pub fn paragraphs_for(
    text: &str,
    how: OverlayTextTransform,
    colored: Option<&[ColoredLine]>,
) -> Vec<Paragraph> {
    let mut paragraphs: Vec<Paragraph> = match colored {
        Some(lines) => lines
            .iter()
            .map(|line| Paragraph {
                text: transform(&collapse(&line.text), how),
                color: sanitize_overlay_color(line.color.as_deref()),
            })
            .collect(),
        None => text
            .split('\n')
            .map(|p| Paragraph {
                text: transform(&collapse(p), how),
                color: None,
            })
            .collect(),
    };
    if paragraphs.last().is_some_and(|p| p.text.is_empty()) {
        paragraphs.pop();
    }
    paragraphs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(p: &[Paragraph]) -> Vec<&str> {
        p.iter().map(|p| p.text.as_str()).collect()
    }

    #[test]
    fn whitespace_collapses_and_a_trailing_break_adds_no_line() {
        let p = paragraphs_for(
            "  Faith   hope\tand love.\n\nlast \n",
            OverlayTextTransform::None,
            None,
        );
        assert_eq!(texts(&p), ["Faith hope and love.", "", "last"]);
        // One `<br>` is one empty line; nothing at all is none.
        assert_eq!(
            texts(&paragraphs_for(" \n ", OverlayTextTransform::None, None)),
            [""]
        );
        assert!(paragraphs_for("", OverlayTextTransform::None, None).is_empty());
        // No-break space survives.
        assert_eq!(collapse("a\u{a0}\u{a0}b"), "a\u{a0}\u{a0}b");
    }

    #[test]
    fn transforms_follow_css() {
        assert_eq!(
            transform("the lord's word", OverlayTextTransform::Capitalize),
            "The Lord's Word"
        );
        assert_eq!(
            transform("Straße", OverlayTextTransform::Uppercase),
            "STRASSE"
        );
        assert_eq!(transform("ÀB", OverlayTextTransform::Lowercase), "àb");
        assert_eq!(
            transform("1 john", OverlayTextTransform::Capitalize),
            "1 John"
        );
    }

    #[test]
    fn coloured_lines_keep_newlines_inside_a_line_as_spaces() {
        let lines = [
            ColoredLine {
                text: "a\nb".into(),
                color: Some("#5eead4".into()),
            },
            ColoredLine {
                text: "c".into(),
                color: Some("red".into()),
            },
        ];
        let p = paragraphs_for("ignored", OverlayTextTransform::None, Some(&lines));
        assert_eq!(texts(&p), ["a b", "c"]);
        assert!(p[0].color.is_some());
        assert!(p[1].color.is_none());
    }
}
