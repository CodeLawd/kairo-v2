//! Text typed straight onto a slide. The words are drawn as the slide draws
//! them (the theme's font, size and alignment, wrapping in its frame) and
//! the keys go to a hidden text field; this draws that field's caret and
//! selection over the drawn words and turns a click into a caret position.
//!
//! GPUI's text layout reports positions as if every line were left aligned,
//! so centred and right-aligned rows are offset here by the room each row
//! leaves in the frame.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::{
    App, Bounds, ElementId, HighlightStyle, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Point, Styled, StyledText, TextAlign, TextLayout, Window, canvas, div,
    fill, point, px, size,
};

/// One wrapped row of the laid-out text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    /// Byte range in the whole text.
    pub start: usize,
    pub end: usize,
    /// Where its hard line (between newlines) begins.
    pub line: usize,
    /// Its left edge and width as laid out (left aligned).
    pub x0: f32,
    pub width: f32,
    /// Its top, from the text's top.
    pub top: f32,
}

/// How far a row moves right for `align` in a frame `frame_w` wide.
pub fn align_offset(align: TextAlign, frame_w: f32, row_w: f32) -> f32 {
    let room = (frame_w - row_w).max(0.0);
    match align {
        TextAlign::Left => 0.0,
        TextAlign::Center => room / 2.0,
        TextAlign::Right => room,
    }
}

/// The row holding byte `index` (the later one at a wrap).
pub fn row_of(rows: &[Row], index: usize) -> Option<&Row> {
    rows.iter()
        .rev()
        .find(|r| r.start <= index && index <= r.end)
        .or_else(|| rows.last())
}

/// The row under `y` (from the text's top), clamped to the first and last.
pub fn row_at(rows: &[Row], y: f32, line_height: f32) -> Option<&Row> {
    rows.iter()
        .find(|r| y < r.top + line_height)
        .or_else(|| rows.last())
}

/// The rows of a laid-out text, with each row's start and width.
fn rows(layout: &TextLayout) -> Vec<Row> {
    let line_height = f32::from(layout.line_height());
    let mut out = Vec::new();
    let mut offset = 0;
    let mut top = 0.0;
    for line in layout.line_layouts() {
        let unwrapped = &line.unwrapped_layout;
        let mut starts = vec![0];
        for b in line.wrap_boundaries() {
            if let Some(g) = unwrapped
                .runs
                .get(b.run_ix)
                .and_then(|r| r.glyphs.get(b.glyph_ix))
            {
                starts.push(g.index);
            }
        }
        for (i, &start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(unwrapped.len);
            let x0 = f32::from(unwrapped.x_for_index(start));
            let x1 = f32::from(unwrapped.x_for_index(end));
            out.push(Row {
                start: offset + start,
                end: offset + end,
                line: offset,
                x0,
                width: (x1 - x0).max(0.0),
                top,
            });
            top += line_height;
        }
        offset += unwrapped.len + 1;
    }
    out
}

/// The caret's top-left for byte `index`, in window coordinates.
fn caret_at(layout: &TextLayout, index: usize, align: TextAlign) -> Option<Point<Pixels>> {
    let bounds = layout.bounds();
    let rows = rows(layout);
    let row = row_of(&rows, index)?;
    let line = layout.line_layout_for_index(row.start)?;
    let x = f32::from(line.unwrapped_layout.x_for_index(index - row.line)) - row.x0
        + align_offset(align, f32::from(bounds.size.width), row.width);
    Some(point(bounds.left() + px(x), bounds.top() + px(row.top)))
}

/// The boxes covering `range` on the drawn rows, in window coordinates.
fn selection_rects(
    layout: &TextLayout,
    range: Range<usize>,
    align: TextAlign,
) -> Vec<Bounds<Pixels>> {
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let mut out = Vec::new();
    for row in rows(layout) {
        let (from, to) = (range.start.max(row.start), range.end.min(row.end));
        if from >= to {
            continue;
        }
        let Some(line) = layout.line_layout_for_index(row.start) else {
            continue;
        };
        let x = |i: usize| f32::from(line.unwrapped_layout.x_for_index(i - row.line)) - row.x0;
        let offset = align_offset(align, f32::from(bounds.size.width), row.width);
        let left = offset + x(from);
        let right = offset + x(to.max(from));
        out.push(Bounds::new(
            point(bounds.left() + px(left), bounds.top() + px(row.top)),
            size(px((right - left).max(2.0)), line_height),
        ));
    }
    out
}

/// The byte under a click at `position` (window coordinates).
fn index_at(layout: &TextLayout, position: Point<Pixels>, align: TextAlign) -> Option<usize> {
    let bounds = layout.bounds();
    let rows = rows(layout);
    let line_height = f32::from(layout.line_height());
    let row = row_at(&rows, f32::from(position.y - bounds.top()), line_height)?;
    let line = layout.line_layout_for_index(row.start)?;
    let x = f32::from(position.x - bounds.left())
        - align_offset(align, f32::from(bounds.size.width), row.width)
        + row.x0;
    let index = line.unwrapped_layout.closest_index_for_x(px(x)) + row.line;
    Some(index.clamp(row.start, row.end))
}

/// The range of the word around byte `index` (letters, digits and
/// apostrophes), or just the caret when there is none.
pub fn word_at(text: &str, index: usize) -> Range<usize> {
    let index = index.min(text.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’';
    let start = text[..index]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(index, |(i, _)| i);
    let end = text[index..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(text.len(), |(i, _)| index + i);
    start..end
}

/// The text's layout from its last frame, kept for the arrow keys.
pub type SlideLayout = Rc<RefCell<Option<TextLayout>>>;

/// Where ↑ (`up`) or ↓ moves byte `index` on the drawn slide: the row above
/// or below, at the same place across. `None` at the first or last row.
pub fn move_vertical(
    layout: &SlideLayout,
    index: usize,
    align: TextAlign,
    up: bool,
) -> Option<usize> {
    let layout = layout.borrow().clone()?;
    let at = caret_at(&layout, index, align)?;
    let line_height = layout.line_height();
    let target = if up {
        at.y - line_height / 2.0
    } else {
        at.y + line_height * 1.5
    };
    let bounds = layout.bounds();
    if target < bounds.top() || target > bounds.bottom() {
        return None;
    }
    index_at(&layout, point(at.x, target), align)
}

/// Where a drag began. It lives with the editor, not the element, because
/// the element is built again for every frame of the drag.
pub type DragAnchor = Rc<Cell<Option<usize>>>;

/// Changes the field's selection; an empty range is a caret.
pub type OnSelect = Rc<dyn Fn(Range<usize>, &mut Window, &mut App)>;

/// The words with the field's caret (`selection` empty, drawn while
/// `caret_on`) or selection over them; `suffix` follows them dimmed and
/// takes no caret. Clicks, double-clicks (a word) and drags select as in a
/// text box. The layout is kept in `layout` for the arrow keys.
#[allow(clippy::too_many_arguments)]
pub fn slide_text(
    id: impl Into<ElementId>,
    text: &str,
    suffix: &str,
    selection: Range<usize>,
    focused: bool,
    caret_on: bool,
    align: TextAlign,
    caret: Hsla,
    layout_out: SlideLayout,
    anchor: DragAnchor,
    on_select: OnSelect,
) -> impl IntoElement {
    let full = format!("{text}{suffix}");
    let len = text.len();
    // The selection is painted here, not as a text highlight: GPUI places
    // highlights as if every row were left aligned.
    let mut highlights = Vec::new();
    if !suffix.is_empty() {
        highlights.push((
            len..full.len(),
            HighlightStyle {
                fade_out: Some(0.35),
                ..Default::default()
            },
        ));
    }
    let styled = StyledText::new(full).with_highlights(highlights);
    let layout = styled.layout().clone();
    *layout_out.borrow_mut() = Some(layout.clone());
    let (down_layout, move_layout, select_layout, paint_layout) =
        (layout.clone(), layout.clone(), layout.clone(), layout);
    let selected = (focused && selection.start < selection.end)
        .then(|| selection.start.min(len)..selection.end.min(len));
    let cursor = selection.end.min(len);
    let show_caret = focused && caret_on && selection.start == selection.end;
    let (down_anchor, move_anchor, up_anchor) = (anchor.clone(), anchor.clone(), anchor);
    let words = text.to_owned();
    let (down_select, move_select) = (on_select.clone(), on_select);
    div()
        .id(id)
        .relative()
        .w_full()
        .cursor_text()
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            let Some(index) = index_at(&down_layout, event.position, align) else {
                return;
            };
            let index = index.min(len);
            if event.click_count >= 3 {
                down_anchor.set(None);
                down_select(0..len, window, cx);
            } else if event.click_count == 2 {
                down_anchor.set(None);
                down_select(word_at(&words, index), window, cx);
            } else if event.modifiers.shift {
                let from = selection.start.min(len);
                down_anchor.set(Some(from));
                down_select(from.min(index)..from.max(index), window, cx);
            } else {
                down_anchor.set(Some(index));
                down_select(index..index, window, cx);
            }
        })
        .on_mouse_move(move |event, window, cx| {
            let Some(from) = move_anchor.get() else {
                return;
            };
            if event.pressed_button != Some(MouseButton::Left) {
                move_anchor.set(None);
                return;
            }
            if let Some(index) = index_at(&move_layout, event.position, align) {
                let index = index.min(len);
                move_select(from.min(index)..from.max(index), window, cx);
            }
        })
        .on_mouse_up(MouseButton::Left, move |_, _, _| up_anchor.set(None))
        // Under the words: the selection, row by row where each row is drawn.
        .child(
            canvas(
                |_, _, _| {},
                move |_: Bounds<Pixels>, _, window, _| {
                    let Some(range) = &selected else {
                        return;
                    };
                    for rect in selection_rects(&select_layout, range.clone(), align) {
                        window.paint_quad(fill(rect, caret.opacity(0.35)));
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .child(styled)
        .child(
            canvas(
                |_, _, _| {},
                move |_: Bounds<Pixels>, _, window, _| {
                    if !show_caret {
                        return;
                    }
                    if let Some(at) = caret_at(&paint_layout, cursor, align) {
                        let height = paint_layout.line_height();
                        window.paint_quad(fill(Bounds::new(at, size(px(2.0), height)), caret));
                    }
                },
            )
            .absolute()
            .size_full(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(start: usize, end: usize, width: f32, top: f32) -> Row {
        Row {
            start,
            end,
            line: 0,
            x0: 0.0,
            width,
            top,
        }
    }

    #[test]
    fn rows_shift_by_the_room_they_leave() {
        assert_eq!(align_offset(TextAlign::Left, 100.0, 60.0), 0.0);
        assert_eq!(align_offset(TextAlign::Center, 100.0, 60.0), 20.0);
        assert_eq!(align_offset(TextAlign::Right, 100.0, 60.0), 40.0);
        assert_eq!(
            align_offset(TextAlign::Center, 50.0, 60.0),
            0.0,
            "never left of the frame"
        );
    }

    #[test]
    fn a_double_click_takes_the_word() {
        let text = "the LORD’s word, again";
        assert_eq!(&text[word_at(text, 6)], "LORD’s");
        assert_eq!(&text[word_at(text, 0)], "the");
        assert_eq!(&text[word_at(text, text.len())], "again");
        assert_eq!(word_at(text, 18), 18..18, "between words");
    }

    #[test]
    fn a_wrap_point_belongs_to_the_next_row() {
        let rows = [row(0, 10, 90.0, 0.0), row(10, 18, 70.0, 20.0)];
        assert_eq!(row_of(&rows, 4).unwrap().start, 0);
        assert_eq!(row_of(&rows, 10).unwrap().start, 10);
        assert_eq!(row_of(&rows, 18).unwrap().start, 10);
        assert_eq!(row_at(&rows, 5.0, 20.0).unwrap().start, 0);
        assert_eq!(row_at(&rows, 25.0, 20.0).unwrap().start, 10);
        assert_eq!(
            row_at(&rows, 99.0, 20.0).unwrap().start,
            10,
            "below the last row"
        );
    }
}
