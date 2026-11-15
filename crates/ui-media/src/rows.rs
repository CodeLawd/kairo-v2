//! Pure rules of the media dock and inspector (`MediaDock.tsx`,
//! `MediaInspector.tsx`, `TrimTimeline.tsx`), tested without a window.

use kairo_domain::media::{MediaItem, MediaLibrary};
use serde_json::Value;

// ─── Sizing and persisted UI state ──────────────────────────────────────────

pub const MIN_HEIGHT: f32 = 180.;
pub const HARD_MAX_HEIGHT: f32 = 520.;
pub const DEFAULT_HEIGHT: f32 = 288.;
pub const HEIGHT_KEY: &str = "media-dock-height";
pub const ZOOM_KEY: &str = "media-dock-zoom";
pub const DEFAULT_ZOOM: f32 = 100.;
pub const MIN_ZOOM: f32 = 75.;
pub const MAX_ZOOM: f32 = 200.;
/// Room kept for the page above.
pub const MIN_PAGE_HEIGHT: f32 = 300.;
/// Title bar and tab strip above the page.
pub const APP_CHROME_HEIGHT: f32 = 44.;
pub const SIDEBAR_HIDDEN_KEY: &str = "kairo.mediaDock.sidebarHidden";
pub const SIDEBAR_WIDTH_KEY: &str = "kairo.mediaDock.sidebarWidth";
pub const SIDEBAR_MIN: f32 = 150.;
pub const SIDEBAR_MAX: f32 = 360.;
pub const SIDEBAR_DEFAULT: f32 = 200.;
/// Card width at 100 % (`minmax(168px, 1fr)`).
pub const CARD_MIN: f32 = 168.;
pub const GAP_X: f32 = 14.;
pub const GAP_Y: f32 = 12.;
/// The name line under a card.
pub const LABEL_HEIGHT: f32 = 22.;
/// How often paste availability is checked (Finder may copy files).
pub const PASTE_POLL_MS: u64 = 2000;
/// Keyboard resize step.
pub const HEIGHT_STEP: f32 = 24.;
pub const SIDEBAR_STEP: f32 = 16.;

/// Room the window gives the dock once the page keeps its floor.
pub fn available_room(window_height: f32) -> f32 {
    window_height - APP_CHROME_HEIGHT - MIN_PAGE_HEIGHT
}

pub fn max_height(window_height: f32) -> f32 {
    MIN_HEIGHT.max(HARD_MAX_HEIGHT.min(available_room(window_height)))
}

pub fn clamp_height(value: f32, window_height: f32) -> f32 {
    max_height(window_height).min(MIN_HEIGHT.max(value))
}

pub fn clamp_sidebar(width: f32) -> f32 {
    SIDEBAR_MAX.min(SIDEBAR_MIN.max(width)).round()
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `storedHeight`: a positive stored height, clamped; else the default.
pub fn stored_height(value: Option<&Value>, window_height: f32) -> f32 {
    match number(value).filter(|n| n.is_finite() && *n > 0.) {
        Some(n) => clamp_height(n as f32, window_height),
        None => clamp_height(DEFAULT_HEIGHT, window_height),
    }
}

/// `storedZoom`: within 75–200, else the default.
pub fn stored_zoom(value: Option<&Value>) -> f32 {
    number(value)
        .map(|n| n as f32)
        .filter(|n| n.is_finite() && (MIN_ZOOM..=MAX_ZOOM).contains(n))
        .unwrap_or(DEFAULT_ZOOM)
}

pub fn stored_sidebar_width(value: Option<&Value>) -> f32 {
    match number(value).filter(|n| n.is_finite() && *n > 0.) {
        Some(n) => clamp_sidebar(n as f32),
        None => SIDEBAR_DEFAULT,
    }
}

/// The sidebar shows unless the stored flag is the string or bool `true`.
pub fn stored_sidebar_open(value: Option<&Value>) -> bool {
    !matches!(value, Some(Value::Bool(true))) && value.and_then(Value::as_str) != Some("true")
}

// ─── The grid ───────────────────────────────────────────────────────────────

/// What the sidebar points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    All,
    Playlist(String),
}

/// The items the grid shows, and a playlist's ids that no longer resolve
/// (counted, not dropped, so a renamed file is noticed).
pub fn visible_items<'a>(
    library: &'a MediaLibrary,
    selection: Option<&Selection>,
) -> (Vec<&'a MediaItem>, Vec<String>) {
    match selection {
        None | Some(Selection::All) => (library.items.iter().collect(), Vec::new()),
        Some(Selection::Playlist(id)) => {
            let Some(playlist) = library.playlists.iter().find(|p| &p.id == id) else {
                return (Vec::new(), Vec::new());
            };
            let mut shown = Vec::new();
            let mut missing = Vec::new();
            for id in &playlist.item_ids {
                match library.items.iter().find(|i| &i.id == id) {
                    Some(item) => shown.push(item),
                    None => missing.push(id.clone()),
                }
            }
            (shown, missing)
        }
    }
}

pub fn selection_label(selection: Option<&Selection>, library: &MediaLibrary) -> String {
    match selection {
        Some(Selection::Playlist(id)) => library
            .playlists
            .iter()
            .find(|p| &p.id == id)
            .map_or_else(|| "Playlist".into(), |p| p.name.clone()),
        _ => "All backgrounds".into(),
    }
}

/// "Untitled playlist", then "Untitled playlist 2", … (Finder's new folder).
pub fn new_playlist_name(library: &MediaLibrary) -> String {
    let taken = |n: &str| library.playlists.iter().any(|p| p.name == n);
    let mut name = "Untitled playlist".to_owned();
    let mut n = 2;
    while taken(&name) {
        name = format!("Untitled playlist {n}");
        n += 1;
    }
    name
}

pub fn count_label(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// The warning over a playlist with files that are gone.
pub fn missing_note(missing: &[String]) -> String {
    let n = missing.len();
    let mut s = format!(
        "{} in this playlist {} no longer in the folder — {}",
        count_label(n, "item"),
        if n == 1 { "is" } else { "are" },
        missing
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    if n > 3 {
        s.push('…');
    }
    s
}

pub fn delete_item_question(name: &str) -> (String, String) {
    (
        format!("Delete “{name}” from the backgrounds folder?"),
        "This removes the file from disk and cannot be undone.".into(),
    )
}

pub fn delete_items_question(count: usize) -> (String, String) {
    (
        format!(
            "Delete {} from the backgrounds folder?",
            count_label(count, "item")
        ),
        "This removes the files from disk and cannot be undone.".into(),
    )
}

pub fn delete_playlist_question(name: &str) -> String {
    format!("Delete the “{name}” playlist? The files stay on disk.")
}

/// Columns and card width for the grid at `width` and `zoom`
/// (`repeat(auto-fill, minmax(min(100%, 168px × zoom), 1fr))`).
pub fn grid_layout(width: f32, zoom: f32) -> (usize, f32) {
    let width = width.max(1.);
    let min = (CARD_MIN * zoom / 100.).min(width);
    let columns = (((width + GAP_X) / (min + GAP_X)).floor() as usize).max(1);
    let card = (width - GAP_X * (columns as f32 - 1.)) / columns as f32;
    (columns, card)
}

/// One grid row's height for cards `card` wide.
pub fn row_height(card: f32) -> f32 {
    card * 9. / 16. + LABEL_HEIGHT + GAP_Y
}

/// The edge a drop lands on: the left half is before.
pub fn drop_before(x: f32, left: f32, width: f32) -> bool {
    x < left + width / 2.
}

// ─── Inspector and trim ─────────────────────────────────────────────────────

/// The inspector's trim fields, as typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Trim {
    /// The start field (seconds).
    pub start: String,
    /// The end field; empty is the file's end.
    pub end: String,
    pub duration: f64,
}

impl Trim {
    pub fn in_point(&self) -> f64 {
        self.start.trim().parse().unwrap_or(f64::NAN)
    }
    pub fn out_point(&self) -> f64 {
        if self.end.trim().is_empty() {
            self.duration
        } else {
            self.end.trim().parse().unwrap_or(f64::NAN)
        }
    }
    /// A start before the end, within a known duration.
    pub fn valid(&self) -> bool {
        let (i, o) = (self.in_point(), self.out_point());
        self.duration > 0.
            && !self.start.trim().is_empty()
            && i >= 0.
            && i < o
            && o <= self.duration
    }
    /// The smallest gap between the handles.
    pub fn gap(&self) -> f64 {
        0.01_f64.min(self.duration / 2.)
    }
    /// The handles' positions, held inside the clip while the fields are invalid.
    pub fn track(&self) -> (f64, f64) {
        if self.valid() {
            return (self.in_point(), self.out_point());
        }
        let gap = self.gap();
        let i = self.in_point();
        let start = 0_f64.max((self.duration - gap).min(if i.is_nan() { 0. } else { i }));
        let o = self.out_point();
        let end = (start + gap).max(
            self.duration
                .min(if o.is_nan() { self.duration } else { o }),
        );
        (start, end)
    }
    /// The line under the fields.
    pub fn note(&self) -> String {
        if self.valid() {
            format!(
                "Selected duration {:.2}s",
                self.out_point() - self.in_point()
            )
        } else if self.duration > 0. {
            "Choose a start before the end, within the video.".into()
        } else {
            "Trim requires a seekable video with a known duration.".into()
        }
    }
}

/// Which trim handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// `TrimTimeline`'s clamp: the start stays before the end by `gap`.
pub fn clamp_trim(edge: Edge, time: f64, start: f64, end: f64, duration: f64) -> f64 {
    let gap = 0.01_f64.min(duration / 2.);
    match edge {
        Edge::Start => 0_f64.max((end - gap).min(time)),
        Edge::End => duration.min((start + gap).max(time)),
    }
}

/// A drag: snapped to hundredths, the very end kept exact.
pub fn snap_trim(seconds: f64, duration: f64) -> f64 {
    if seconds >= duration {
        duration
    } else {
        (seconds * 100.).round() / 100.
    }
}

/// An arrow key on a handle: ±0.1 s, ±1 s with Shift.
pub fn trim_key(
    edge: Edge,
    key: &str,
    shift: bool,
    start: f64,
    end: f64,
    duration: f64,
) -> Option<f64> {
    let time = if edge == Edge::Start { start } else { end };
    let gap = 0.01_f64.min(duration / 2.);
    let step = if shift { 1. } else { 0.1 };
    let (min, max) = match edge {
        Edge::Start => (0., 0_f64.max(end - gap)),
        Edge::End => (duration.min(start + gap), duration),
    };
    let next = match key {
        "left" | "down" => time - step,
        "right" | "up" => time + step,
        "home" => return Some(min),
        "end" => return Some(max),
        _ => return None,
    };
    Some(clamp_trim(
        edge,
        (next * 100.).round() / 100.,
        start,
        end,
        duration,
    ))
}

/// The end field for a handle time: the clip's end is "file end" (empty).
pub fn end_field(time: f64, duration: f64) -> String {
    if time >= duration {
        String::new()
    } else {
        number_text(time)
    }
}

/// JavaScript's `String(number)` for the fields.
pub fn number_text(n: f64) -> String {
    kairo_domain::js::number_to_string(n)
}

/// `formatDocumentSize`.
pub fn format_size(bytes: f64) -> String {
    kairo_domain::documents::format_document_size(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn library(items: &[&str], playlists: Vec<(&str, &[&str])>) -> MediaLibrary {
        serde_json::from_value(json!({
            "folder": "/m", "folders": [],
            "items": items.iter().map(|id| json!({"id": id, "name": id, "path": format!("/m/{id}.jpg"), "kind": "image", "ext": "jpg", "folder": "", "sizeBytes": 1.0})).collect::<Vec<_>>(),
            "playlists": playlists.iter().map(|(id, ids)| json!({"id": id, "name": format!("P {id}"), "itemIds": ids, "createdAt": 0.0})).collect::<Vec<_>>(),
            "playback": {}, "liveItemId": null, "livePaused": false, "error": null,
        }))
        .unwrap()
    }

    #[test]
    fn stored_state_is_read_and_clamped() {
        assert_eq!(stored_height(None, 1000.), DEFAULT_HEIGHT);
        assert_eq!(stored_height(Some(&json!("900")), 1000.), HARD_MAX_HEIGHT);
        assert_eq!(stored_height(Some(&json!(100)), 1000.), MIN_HEIGHT);
        // A short window caps the dock below 520.
        assert_eq!(stored_height(Some(&json!(500)), 700.), 356.);
        assert_eq!(stored_zoom(Some(&json!(300))), DEFAULT_ZOOM);
        assert_eq!(stored_zoom(Some(&json!("150"))), 150.);
        assert_eq!(stored_sidebar_width(Some(&json!(999))), SIDEBAR_MAX);
        assert_eq!(stored_sidebar_width(Some(&json!("x"))), SIDEBAR_DEFAULT);
        assert!(stored_sidebar_open(None));
        assert!(!stored_sidebar_open(Some(&json!("true"))));
        assert!(stored_sidebar_open(Some(&json!("false"))));
    }

    #[test]
    fn playlists_keep_their_order_and_count_missing_files() {
        let lib = library(
            &["a", "b", "c"],
            vec![("p", &["c", "gone", "a", "x", "y", "z"])],
        );
        let (shown, missing) = visible_items(&lib, Some(&Selection::Playlist("p".into())));
        assert_eq!(
            shown.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["c", "a"]
        );
        assert_eq!(missing, ["gone", "x", "y", "z"]);
        assert_eq!(
            missing_note(&missing),
            "4 items in this playlist are no longer in the folder — gone, x, y…"
        );
        assert_eq!(visible_items(&lib, None).0.len(), 3);
        assert!(
            visible_items(&lib, Some(&Selection::Playlist("nope".into())))
                .0
                .is_empty()
        );
        assert_eq!(
            selection_label(Some(&Selection::Playlist("p".into())), &lib),
            "P p"
        );
        assert_eq!(
            selection_label(Some(&Selection::Playlist("nope".into())), &lib),
            "Playlist"
        );
    }

    #[test]
    fn new_playlist_names_count_up() {
        let mut lib = library(&[], vec![]);
        assert_eq!(new_playlist_name(&lib), "Untitled playlist");
        lib.playlists = serde_json::from_value(json!([
            {"id": "1", "name": "Untitled playlist", "itemIds": [], "createdAt": 0.0},
            {"id": "2", "name": "Untitled playlist 2", "itemIds": [], "createdAt": 0.0}
        ]))
        .unwrap();
        assert_eq!(new_playlist_name(&lib), "Untitled playlist 3");
    }

    #[test]
    fn grid_layout_fills_columns() {
        assert_eq!(grid_layout(1000., 100.).0, 5);
        assert_eq!(grid_layout(1000., 200.).0, 2);
        assert_eq!(grid_layout(100., 100.), (1, 100.));
    }

    #[test]
    fn trim_rules() {
        let t = Trim {
            start: "1".into(),
            end: String::new(),
            duration: 10.,
        };
        assert!(t.valid());
        assert_eq!(t.note(), "Selected duration 9.00s");
        let bad = Trim {
            start: "8".into(),
            end: "3".into(),
            duration: 10.,
        };
        assert!(!bad.valid());
        assert_eq!(bad.track(), (8., 8.01));
        let unknown = Trim {
            start: "0".into(),
            end: String::new(),
            duration: 0.,
        };
        assert_eq!(
            unknown.note(),
            "Trim requires a seekable video with a known duration."
        );
        assert_eq!(clamp_trim(Edge::Start, 9.999, 0., 5., 10.), 4.99);
        assert_eq!(clamp_trim(Edge::End, 0., 2., 5., 10.), 2.01);
        assert_eq!(snap_trim(4.56789, 10.), 4.57);
        assert_eq!(snap_trim(10.5, 10.), 10.);
        assert_eq!(
            trim_key(Edge::Start, "right", false, 1., 5., 10.),
            Some(1.1)
        );
        assert_eq!(trim_key(Edge::End, "left", true, 1., 5., 10.), Some(4.));
        assert_eq!(trim_key(Edge::End, "end", false, 1., 5., 10.), Some(10.));
        assert_eq!(trim_key(Edge::Start, "x", false, 1., 5., 10.), None);
        assert_eq!(end_field(10., 10.), "");
        assert_eq!(end_field(2.5, 10.), "2.5");
    }
}
