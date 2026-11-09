//! Pure grid logic, independent of what is on screen. Throwaway spike code.

use std::collections::HashSet;

pub const GAP: f32 = 12.0;
pub const FOOTER: f32 = 24.0;
pub const MIN_ZOOM: f32 = 120.0;
pub const MAX_ZOOM: f32 = 480.0;
pub const ZOOM_STEP: f32 = 40.0;
/// Pointer travel before a press becomes a marquee, as in `MarqueeSelect`.
pub const START_DISTANCE: f32 = 4.0;
pub const EDGE: f32 = 36.0;
pub const SCROLL_STEP: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    pub columns: usize,
    pub card_width: f32,
    pub row_height: f32,
}

/// `repeat(auto-fill, minmax(min(100%, min), 1fr))` with a 16:9 card and footer.
pub fn grid_layout(width: f32, min_width: f32) -> GridLayout {
    let usable = width.max(1.0);
    let columns = (((usable + GAP) / (min_width + GAP)).floor() as usize).max(1);
    let card_width = (usable - GAP * (columns - 1) as f32) / columns as f32;
    GridLayout {
        columns,
        card_width,
        row_height: card_width * 9.0 / 16.0 + FOOTER + GAP,
    }
}

pub fn row_count(count: usize, columns: usize) -> usize {
    count.div_ceil(columns.max(1))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Band {
    pub fn between(a: (f32, f32), b: (f32, f32)) -> Self {
        Self {
            left: a.0.min(b.0),
            top: a.1.min(b.1),
            right: a.0.max(b.0),
            bottom: a.1.max(b.1),
        }
    }
}

/// Positions under a band in content coordinates. It works from geometry, so
/// cards scrolled out of view are still selected. Touching edges count.
pub fn marquee_hits(layout: GridLayout, count: usize, band: Band) -> Vec<usize> {
    let card_height = layout.row_height - GAP;
    let step_x = layout.card_width + GAP;
    let first_row = (band.top / layout.row_height).floor().max(0.0) as usize;
    let last_row = (band.bottom / layout.row_height).floor().max(0.0) as usize;
    let mut hits = Vec::new();
    for row in first_row..=last_row {
        let top = row as f32 * layout.row_height;
        if top > band.bottom || top + card_height < band.top {
            continue;
        }
        for column in 0..layout.columns {
            let index = row * layout.columns + column;
            let left = column as f32 * step_x;
            if index < count && left <= band.right && left + layout.card_width >= band.left {
                hits.push(index);
            }
        }
    }
    hits
}

/// The card position at a content point, or `None` over a gap or empty space.
pub fn card_at(layout: GridLayout, count: usize, x: f32, y: f32) -> Option<usize> {
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let step_x = layout.card_width + GAP;
    let column = (x / step_x).floor() as usize;
    let row = (y / layout.row_height).floor() as usize;
    let in_card = x - column as f32 * step_x <= layout.card_width
        && y - row as f32 * layout.row_height <= layout.row_height - GAP;
    let index = row * layout.columns + column;
    (in_card && column < layout.columns && index < count).then_some(index)
}

/// Moves `moved` ids before `target`, keeping their relative order. Dropping
/// onto a moved card changes nothing.
pub fn reorder(order: &mut Vec<u32>, moved: &HashSet<u32>, target: u32) -> bool {
    if moved.contains(&target) || moved.is_empty() {
        return false;
    }
    let block: Vec<u32> = order
        .iter()
        .copied()
        .filter(|id| moved.contains(id))
        .collect();
    if block.is_empty() {
        return false;
    }
    order.retain(|id| !moved.contains(id));
    let Some(at) = order.iter().position(|id| *id == target) else {
        return false;
    };
    order.splice(at..at, block);
    true
}

#[derive(Debug, Clone, Copy)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

/// The next focused position, stopping at the edges of the grid.
pub fn navigate(index: usize, direction: Move, columns: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let last = count - 1;
    match direction {
        Move::Left => index.saturating_sub(1),
        Move::Right => (index + 1).min(last),
        Move::Up => index.checked_sub(columns).unwrap_or(index),
        Move::Down => {
            if index + columns <= last {
                index + columns
            } else {
                index
            }
        }
    }
}

pub fn zoom(min_width: f32, direction: f32) -> f32 {
    (min_width + ZOOM_STEP * direction).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Port of `kairoPathsIn`, for the `file://` URLs GPUI delivers.
pub fn kairo_paths(urls: &[String], exists: impl Fn(&str) -> bool) -> Vec<String> {
    urls.iter()
        .filter_map(|url| url.strip_prefix("file://"))
        .map(percent_decode)
        .filter(|path| path.to_lowercase().ends_with(".kairo") && exists(path))
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = (bytes[at] == b'%')
            .then(|| {
                text.get(at + 1..at + 3)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            })
            .flatten();
        if let Some(byte) = hex {
            out.push(byte);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_count_changes_with_width_and_never_reaches_zero() {
        assert_eq!(grid_layout(100.0, 240.0).columns, 1);
        assert_eq!(grid_layout(492.0, 240.0).columns, 2);
        assert_eq!(grid_layout(491.0, 240.0).columns, 1);
        let wide = grid_layout(1000.0, 240.0);
        assert_eq!(wide.columns, 4);
        assert!((wide.card_width * 4.0 + GAP * 3.0 - 1000.0).abs() < 0.01);
    }

    fn fixed() -> GridLayout {
        GridLayout {
            columns: 3,
            card_width: 100.0,
            row_height: 100.0,
        }
    }

    #[test]
    fn marquee_selects_by_geometry_and_includes_offscreen_rows() {
        let band = Band::between((10.0, 2010.0), (120.0, 10.0));
        let hits = marquee_hits(fixed(), 5000, band);
        assert_eq!(hits.len(), 2 * 21);
        assert_eq!(&hits[..2], [0, 1]);
    }

    #[test]
    fn marquee_skips_gaps_and_positions_past_the_end() {
        assert!(marquee_hits(fixed(), 9, Band::between((0.0, 90.0), (300.0, 98.0))).is_empty());
        assert_eq!(
            marquee_hits(fixed(), 4, Band::between((0.0, 100.0), (340.0, 150.0))),
            [3]
        );
        assert!(marquee_hits(fixed(), 0, Band::between((0.0, 0.0), (300.0, 300.0))).is_empty());
    }

    #[test]
    fn card_at_separates_cards_from_gaps() {
        assert_eq!(card_at(fixed(), 9, 50.0, 50.0), Some(0));
        assert_eq!(card_at(fixed(), 9, 105.0, 50.0), None);
        assert_eq!(card_at(fixed(), 9, 50.0, 95.0), None);
        assert_eq!(card_at(fixed(), 2, 240.0, 50.0), None);
        assert_eq!(card_at(fixed(), 9, -1.0, 50.0), None);
    }

    #[test]
    fn reorder_moves_a_block_in_order() {
        let mut order = vec![1, 2, 3, 4, 5];
        assert!(reorder(&mut order, &HashSet::from([4, 2]), 1));
        assert_eq!(order, [2, 4, 1, 3, 5]);
        assert!(!reorder(&mut order, &HashSet::from([1]), 1));
        assert!(!reorder(&mut order, &HashSet::new(), 1));
        assert_eq!(order, [2, 4, 1, 3, 5]);
    }

    #[test]
    fn navigation_stops_at_edges() {
        assert_eq!(navigate(0, Move::Left, 4, 10), 0);
        assert_eq!(navigate(9, Move::Right, 4, 10), 9);
        assert_eq!(navigate(2, Move::Up, 4, 10), 2);
        assert_eq!(navigate(5, Move::Up, 4, 10), 1);
        assert_eq!(navigate(5, Move::Down, 4, 10), 9);
        assert_eq!(navigate(7, Move::Down, 4, 10), 7);
        assert_eq!(navigate(0, Move::Down, 4, 0), 0);
    }

    #[test]
    fn zoom_is_clamped() {
        assert_eq!(zoom(MIN_ZOOM, -1.0), MIN_ZOOM);
        assert_eq!(zoom(MAX_ZOOM, 1.0), MAX_ZOOM);
        assert_eq!(zoom(240.0, 1.0), 280.0);
    }

    #[test]
    fn file_urls_are_decoded_and_filtered() {
        let urls = [
            "file:///a/My%20Song.KAIRO",
            "file:///a/b.txt",
            "https://x/y.kairo",
            "file:///gone.kairo",
        ]
        .map(String::from);
        assert_eq!(
            kairo_paths(&urls, |p| p != "/gone.kairo"),
            ["/a/My Song.KAIRO"]
        );
        assert!(kairo_paths(&[], |_| true).is_empty());
    }
}
