//! Verse and reference text for the overlay, from `src/lib/overlay-content.ts`.

use crate::js;
use crate::scripture::ScriptureVerse;

/// `formatOverlayReference`: the reference, with ` (KJV)` when asked.
pub fn format_overlay_reference(
    reference: &str,
    translation: &str,
    show_translation: bool,
) -> String {
    if show_translation {
        format!("{reference} ({translation})")
    } else {
        reference.to_owned()
    }
}

/// `formatCardReference`: the card shows the translation in its own badge.
pub fn format_card_reference(reference: &str) -> String {
    reference.to_owned()
}

/// `formatOverlayVerseText`: one line per verse, numbered when there is more
/// than one and numbers are on, cut to `max_verses` (0 or `None` for no cap)
/// with `…` after the last kept verse. `None` for no verses.
pub fn format_overlay_verse_text(
    verses: &[ScriptureVerse],
    show_verse_numbers: bool,
    max_verses: Option<f64>,
) -> Option<String> {
    if verses.is_empty() {
        return None;
    }
    let max = max_verses.unwrap_or(0.0);
    let mut selected = verses;
    let mut truncated = false;
    // `selected.length > maxVerses` then `slice(0, maxVerses)`: slice truncates
    // a fractional end towards zero.
    if max > 0.0 && (selected.len() as f64) > max {
        selected = &selected[..max.trunc() as usize];
        truncated = true;
    }
    let show_numbers = show_verse_numbers && selected.len() > 1;
    let mut lines: Vec<String> = selected
        .iter()
        .map(|v| {
            if show_numbers {
                format!("{} {}", js::number_to_string(v.verse), v.text)
            } else {
                v.text.clone()
            }
        })
        .collect();
    if truncated && let Some(last) = lines.last_mut() {
        last.push('…');
    }
    Some(lines.join("\n"))
}

/// `renderOverlayTemplate`: fills `{Reference}` and `{Text}`, then trims.
/// Replacement text follows `String.prototype.replaceAll`, so `$&` and the
/// other `$` patterns in a reference or verse behave as in the Electron app.
pub fn render_overlay_template(template: &str, reference: &str, text: &str) -> String {
    let filled = js::replace_all(template, "{Reference}", reference);
    let filled = js::replace_all(&filled, "{Text}", text);
    js::trim(&filled).to_owned()
}
