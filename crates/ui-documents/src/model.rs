//! The page's rules, apart from drawing: slide size, the import and
//! converter dialogs' wording, the slideshow's next step and the grid's
//! shape.

use kairo_domain::documents::{
    DocumentFormat, DocumentPlaybackStatus, SlideConverterState, SlideConverterStatus,
};
use serde_json::Value;

use crate::backend::ImportStep;

/// `THUMB_KEY`: the slide size, per viewer.
pub const THUMB_KEY: &str = "kairo.documents.slideWidth";
pub const THUMB_MIN: f32 = 140.;
pub const THUMB_MAX: f32 = 420.;
pub const THUMB_DEFAULT: f32 = 240.;
pub const THUMB_STEP: f32 = 10.;
/// The grid's gaps (`gap-x-3 gap-y-4`) and padding (`p-3`).
pub const GAP_X: f32 = 12.;
pub const GAP_Y: f32 = 16.;
pub const GRID_PAD: f32 = 12.;
/// The bar under each slide with its number, as on the Bible cards.
pub const CAPTION: f32 = 34.;
/// The slideshow presets.
pub const PRESETS: [f64; 5] = [5., 10., 15., 30., 60.];
/// How often the live slide's videos are read (`setTimeout(poll, 350)`).
pub const PLAYBACK_POLL_MS: u64 = 350;
/// A push in flight when the dwell ends waits this long, then tries again.
pub const PUSH_RETRY_MS: u64 = 250;

/// The saved slide size, or the default when missing or out of range.
pub fn saved_thumb(value: Option<&Value>) -> f32 {
    let saved = match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    };
    match saved {
        Some(v) if v >= THUMB_MIN as f64 && v <= THUMB_MAX as f64 => v as f32,
        _ => THUMB_DEFAULT,
    }
}

/// A slider value snapped to the range and step.
pub fn snap_thumb(value: f32) -> f32 {
    ((value / THUMB_STEP).round() * THUMB_STEP).clamp(THUMB_MIN, THUMB_MAX)
}

/// `repeat(auto-fill, minmax(thumb, 1fr))`: how many columns, and each
/// column's width.
pub fn grid_columns(width: f32, thumb: f32) -> (usize, f32) {
    let inner = (width - GRID_PAD * 2.).max(thumb);
    let columns = (((inner + GAP_X) / (thumb + GAP_X)).floor() as usize).max(1);
    let column = (inner - GAP_X * (columns as f32 - 1.)) / columns as f32;
    (columns, column)
}

/// The slide's aspect ratio (width over height): its size, else 16:9.
pub fn aspect(size: Option<&kairo_domain::overlay::Size>) -> f32 {
    match size {
        Some(s) if s.width > 0. && s.height > 0. => (s.width / s.height) as f32,
        _ => 16. / 9.,
    }
}

/// `Page` for a PDF, `Slide` for a deck.
pub fn unit(format: Option<&DocumentFormat>) -> &'static str {
    match format {
        Some(DocumentFormat::Pdf) => "Page",
        _ => "Slide",
    }
}

/// The import dialog's state (`DocumentImportProgress`).
#[derive(Clone, Debug, PartialEq)]
pub struct ImportProgress {
    pub name: String,
    pub format: Option<DocumentFormat>,
    pub step: ImportStep,
    /// Set when the import failed: the dialog offers Choose file again.
    pub error: Option<String>,
}

impl ImportProgress {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            format: None,
            step: ImportStep::Preparing {
                name: String::new(),
                format: None,
            },
            error: None,
        }
    }

    pub fn advance(&mut self, step: ImportStep) {
        if let ImportStep::Preparing { name, format } = &step {
            if !name.is_empty() {
                self.name = name.clone();
            }
            if format.is_some() {
                self.format = *format;
            }
        }
        self.step = step;
    }

    pub fn failed(&self) -> bool {
        self.error.is_some()
    }

    pub fn finishing(&self) -> bool {
        self.step == ImportStep::Finishing
    }

    /// The bar has a width only while pages render.
    pub fn percent(&self) -> Option<u32> {
        match self.step {
            ImportStep::Rendering { completed, total } if total > 0 => {
                Some((completed as f64 / total as f64 * 100.).floor() as u32)
            }
            _ => None,
        }
    }

    pub fn title(&self) -> &'static str {
        if self.failed() {
            "Couldn’t import"
        } else {
            "Importing"
        }
    }

    /// The failure in plain words: a missing system library or other
    /// low-level detail never reaches the operator verbatim.
    pub fn error_line(&self) -> Option<String> {
        let error = self.error.as_deref()?;
        let low = error.to_ascii_lowercase();
        Some(
            if low.contains("pdf rendering is unavailable")
                || low.contains("loadlibraryerror")
                || low.contains("dlopen")
            {
                "Kairo couldn’t load its PDF renderer. Reinstall Kairo, then import the file again."
                    .into()
            } else if error.len() > 220 || error.contains('{') {
                "Kairo couldn’t read this file. Check that it opens on this computer, then try again."
                    .into()
            } else {
                error.to_owned()
            },
        )
    }

    pub fn name_line(&self) -> &str {
        if self.name.is_empty() {
            "PDF or PowerPoint"
        } else {
            &self.name
        }
    }

    /// The line under the bar.
    pub fn line(&self, canceling: bool) -> String {
        if canceling {
            return "Canceling…".into();
        }
        match self.step {
            ImportStep::Rendering { completed, total } if total > 0 => format!(
                "{} {} of {total}",
                unit(self.format.as_ref()),
                (completed + 1).min(total)
            ),
            ImportStep::Finishing => "Almost done…".into(),
            _ => "Preparing…".into(),
        }
    }
}

impl Default for ImportProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// `formatMB`.
pub fn format_mb(bytes: f64) -> String {
    format!("{} MB", (bytes / (1024. * 1024.)).round())
}

/// What the converter dialog shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConverterView {
    /// Download and import, or the simplified version.
    Offer,
    /// Downloading or setting up.
    Busy,
    /// No build for this computer.
    Unsupported,
    /// The download this dialog started failed.
    Failed,
}

pub fn converter_view(status: Option<&SlideConverterStatus>, started: bool) -> ConverterView {
    match status.map(|s| &s.state) {
        Some(SlideConverterState::Downloading | SlideConverterState::Installing) => {
            ConverterView::Busy
        }
        Some(SlideConverterState::Unsupported) => ConverterView::Unsupported,
        Some(SlideConverterState::Error) if started => ConverterView::Failed,
        _ => ConverterView::Offer,
    }
}

pub fn converter_title(view: ConverterView) -> &'static str {
    match view {
        ConverterView::Failed => "The download didn’t finish",
        ConverterView::Busy => "Getting the slide converter",
        ConverterView::Unsupported => "Install LibreOffice to continue",
        ConverterView::Offer => "Keep every slide exactly as designed",
    }
}

/// The download's percent, 0 until the size is known.
pub fn converter_percent(status: &SlideConverterStatus) -> u32 {
    if status.size_bytes > 0. {
        ((status.received_bytes / status.size_bytes * 100.).floor() as u32).min(100)
    } else {
        0
    }
}

/// The offer's size line.
pub fn converter_size(status: Option<&SlideConverterStatus>) -> String {
    match status {
        Some(s) if s.size_bytes > 0. => format_mb(s.size_bytes),
        _ => "about 300 MB".into(),
    }
}

/// What the slideshow does when a page's dwell ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advance {
    Push(usize),
    Stop,
}

pub fn advance(page: usize, pages: usize, looping: bool) -> Advance {
    let last = pages.saturating_sub(1);
    if page >= last {
        if looping && pages > 1 {
            Advance::Push(0)
        } else {
            Advance::Stop
        }
    } else {
        Advance::Push(page + 1)
    }
}

/// A slide with videos holds the slideshow until they end; a video error
/// stops it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoGate {
    Wait,
    Advance,
    Stop,
}

pub fn video_gate(playback: Option<&DocumentPlaybackStatus>) -> VideoGate {
    let Some(p) = playback else {
        return VideoGate::Wait;
    };
    if p.videos.iter().any(|v| v.error.is_some()) {
        VideoGate::Stop
    } else if !p.videos.is_empty() && p.videos.iter().all(|v| v.ended) {
        VideoGate::Advance
    } else {
        VideoGate::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slide_size_reads_back_within_range() {
        assert_eq!(saved_thumb(None), THUMB_DEFAULT);
        assert_eq!(saved_thumb(Some(&json!(300))), 300.);
        assert_eq!(saved_thumb(Some(&json!("180"))), 180.);
        assert_eq!(saved_thumb(Some(&json!(100))), THUMB_DEFAULT);
        assert_eq!(saved_thumb(Some(&json!(500))), THUMB_DEFAULT);
        assert_eq!(saved_thumb(Some(&json!("x"))), THUMB_DEFAULT);
        assert_eq!(snap_thumb(263.), 260.);
        assert_eq!(snap_thumb(10.), THUMB_MIN);
    }

    #[test]
    fn columns_fill_the_width() {
        assert_eq!(grid_columns(800., 240.).0, 3);
        assert_eq!(grid_columns(100., 240.).0, 1);
        let (n, w) = grid_columns(1000., 140.);
        assert_eq!(n, 6);
        assert!(w >= 140.);
    }

    #[test]
    fn import_wording_follows_the_stage() {
        let mut p = ImportProgress::new();
        assert_eq!(p.line(false), "Preparing…");
        assert_eq!(p.name_line(), "PDF or PowerPoint");
        p.advance(ImportStep::Preparing {
            name: "Easter".into(),
            format: Some(DocumentFormat::Pdf),
        });
        p.advance(ImportStep::Rendering {
            completed: 2,
            total: 4,
        });
        assert_eq!(p.line(false), "Page 3 of 4");
        assert_eq!(p.percent(), Some(50));
        assert_eq!(p.name_line(), "Easter");
        p.advance(ImportStep::Rendering {
            completed: 4,
            total: 4,
        });
        assert_eq!(p.line(false), "Page 4 of 4");
        p.advance(ImportStep::Finishing);
        assert_eq!(p.line(false), "Almost done…");
        assert_eq!(p.line(true), "Canceling…");
        assert_eq!(p.percent(), None);
        p.error = Some("Bad".into());
        assert_eq!(p.title(), "Couldn’t import");
    }

    #[test]
    fn slideshow_advances_loops_and_stops() {
        assert_eq!(advance(0, 3, false), Advance::Push(1));
        assert_eq!(advance(2, 3, false), Advance::Stop);
        assert_eq!(advance(2, 3, true), Advance::Push(0));
        assert_eq!(advance(0, 1, true), Advance::Stop);
    }

    #[test]
    fn converter_states() {
        let status = |state| SlideConverterStatus {
            state,
            version: String::new(),
            size_bytes: 300. * 1024. * 1024.,
            received_bytes: 150. * 1024. * 1024.,
            error: None,
            other_converter: None,
        };
        let s = status(SlideConverterState::Downloading);
        assert_eq!(converter_view(Some(&s), false), ConverterView::Busy);
        assert_eq!(converter_percent(&s), 50);
        assert_eq!(converter_size(Some(&s)), "300 MB");
        assert_eq!(converter_size(None), "about 300 MB");
        let e = status(SlideConverterState::Error);
        assert_eq!(converter_view(Some(&e), false), ConverterView::Offer);
        assert_eq!(converter_view(Some(&e), true), ConverterView::Failed);
        assert_eq!(
            converter_view(Some(&status(SlideConverterState::Unsupported)), false),
            ConverterView::Unsupported
        );
    }
}
