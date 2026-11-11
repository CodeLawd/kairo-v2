//! Auto-fit. One implementation, replacing `estimateAutoFitVerseFontPx` in
//! `overlay-template.ts`, `largestFontThatFits` in `overlay-fit.ts` and the
//! copy inside `overlay.html`.
//!
//! The size is the largest whole pixel size from 12 to a cap at which the
//! text fits, decided by shaping the real text, not by estimating widths.
//! The cap is the smart-fill rule: no larger than the size the typical verse
//! ([`SMART_FILL_REFERENCE`]) would get in the same box, and no larger than
//! the size the text itself would get, so short lines stay verse-sized.

use kairo_domain::js;
use kairo_domain::overlay::OverlayTheme;

pub const MIN_AUTO_FIT_PX: f32 = 12.0;
/// Safety ceiling; smart fill caps well below it.
pub const MAX_AUTO_FIT_PX: f32 = 240.0;
/// Typical projected verse: short lines grow only up to the size this would
/// use in the same box.
pub const SMART_FILL_REFERENCE: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";

/// Average advance as a share of the font size, in the cap's width rule.
const AVG_CHAR_WIDTH_FACTOR: f64 = 0.55;

/// `estimateBoxFillFontPx` for `text` in the verse box: the cap rule's own
/// arithmetic, which is a bound on the size and never the size itself.
fn box_fill_px(text: &str, theme: &OverlayTheme, frame: (u32, u32)) -> f64 {
    let verse = &theme.verse;
    let b = &verse.frame;
    let padding = theme.layout.padding_px;
    let usable_w = (f64::from(frame.0) * b.width_pct / 100.0 - padding * 2.0).max(80.0);
    let usable_h = (f64::from(frame.1) * b.height_pct / 100.0 - padding * 2.0).max(48.0);
    let paragraphs: Vec<&str> = text
        .split('\n')
        .map(js::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if paragraphs.is_empty() {
        return verse.font_size_px;
    }
    let longest_word = paragraphs
        .join(" ")
        .split(js::is_whitespace)
        .filter(|w| !w.is_empty())
        .map(js::utf16_len)
        .fold(1, usize::max) as f64;
    let lengths: Vec<f64> = paragraphs.iter().map(|p| js::utf16_len(p) as f64).collect();
    let fits = |size: f64| -> bool {
        let char_w = size * AVG_CHAR_WIDTH_FACTOR + verse.letter_spacing_px.abs() * 0.35;
        if longest_word * char_w > usable_w {
            return false;
        }
        let per_line = (usable_w / char_w).floor().max(1.0);
        let lines: f64 = lengths.iter().map(|l| (l / per_line).ceil().max(1.0)).sum();
        lines * size * verse.line_height <= usable_h
    };
    let mut lo = f64::from(MIN_AUTO_FIT_PX);
    let mut hi = f64::from(MAX_AUTO_FIT_PX);
    if !fits(lo) {
        return lo;
    }
    while lo < hi {
        let mid = ((lo + hi + 1.0) / 2.0).floor();
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1.0;
        }
    }
    lo
}

/// `data-max-font-px`: the smart-fill cap for `text` in this theme's verse
/// box, as `overlay.html` reads it (`parseFloat(...) || 240`).
pub fn smart_fill_cap(text: &str, theme: &OverlayTheme, frame: (u32, u32)) -> f32 {
    let cap = box_fill_px(text, theme, frame).min(box_fill_px(SMART_FILL_REFERENCE, theme, frame));
    if cap.is_finite() && cap > 0.0 {
        cap as f32
    } else {
        MAX_AUTO_FIT_PX
    }
}

/// The fit search from `overlay.html` (`fitVerse`): `overflows(px)` says the
/// text at `px` does not fit. Returns the least size when even it overflows.
pub fn largest_that_fits(cap: f32, mut overflows: impl FnMut(f32) -> bool) -> f32 {
    let min = MIN_AUTO_FIT_PX;
    if overflows(min) {
        return min;
    }
    let (mut lo, mut hi, mut best) = (min, cap.floor().max(min - 1.0), min);
    // `lo <= hi` with whole-number midpoints, as in the page's script.
    while lo <= hi {
        let mid = ((lo + hi) / 2.0).floor();
        if overflows(mid) {
            hi = mid - 1.0;
        } else {
            best = mid;
            lo = mid + 1.0;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_returns_the_largest_fitting_size_or_the_floor() {
        assert_eq!(largest_that_fits(100.0, |px| px > 37.0), 37.0);
        assert_eq!(largest_that_fits(100.0, |px| px > 12.0), 12.0);
        assert_eq!(largest_that_fits(100.0, |_| true), 12.0);
        assert_eq!(largest_that_fits(30.0, |_| false), 30.0);
        // A cap below the floor still answers the floor.
        assert_eq!(largest_that_fits(5.0, |_| false), 12.0);
        // A fractional cap is a whole-number ceiling.
        assert_eq!(largest_that_fits(40.5, |_| false), 40.0);
    }
}
