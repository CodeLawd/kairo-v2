//! What the theme editor reads and asks for. The application implements
//! [`ThemeBackend`] over its settings, dialogs and engine; tests drive a
//! scripted fake.
//!
//! Electron reached these through `window.api.settings`, `ndi.pickOverlayMedia`
//! and the `localStorage`-backed app store, and drew the preview in a
//! hidden `<iframe>`. Here the preview is a private offscreen engine surface
//! (the same renderer the screens use), so what the canvas shows is what a
//! screen draws.

use std::sync::Arc;

use crate::ops::Op;
use kairo_domain::overlay::OverlayTheme;
use kairo_domain::settings::AppSettings;

/// Which file the picker offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

/// A push from the core.
#[derive(Clone, Debug, PartialEq)]
pub enum ThemeFeed {
    /// Settings changed anywhere (Screens, Settings, another write).
    Settings(Box<AppSettings>),
}

pub type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send>;
pub type FeedSink = Arc<dyn Fn(ThemeFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;

/// The sample slide the preview shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub reference: String,
    pub text: String,
}

/// A private, muted engine surface used only by the editor. The owning view
/// holds it for as long as the preview is visible; dropping it destroys the
/// surface and its media players.
pub struct ThemePreview {
    hub: kairo_core::preview::PreviewHub,
    output: String,
    apply: Box<dyn Fn(OverlayTheme, Sample) + Send + Sync>,
    _lifetime: Box<dyn Send>,
}

impl ThemePreview {
    pub fn new(
        hub: kairo_core::preview::PreviewHub,
        output: String,
        apply: impl Fn(OverlayTheme, Sample) + Send + Sync + 'static,
        lifetime: impl Send + 'static,
    ) -> Self {
        Self {
            hub,
            output,
            apply: Box::new(apply),
            _lifetime: Box::new(lifetime),
        }
    }

    pub fn hub(&self) -> kairo_core::preview::PreviewHub {
        self.hub.clone()
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    /// Shows `sample` drawn with `theme`, at once (no transition).
    pub fn show(&self, theme: OverlayTheme, sample: Sample) {
        (self.apply)(theme, sample);
    }
}

pub trait ThemeBackend: Send + Sync + 'static {
    /// The stored settings, read on the UI thread (cheap).
    fn snapshot(&self) -> AppSettings;
    fn listen(&self, sink: FeedSink) -> Listening;
    /// Applies `op` to the stored settings. Enqueues immediately, in call
    /// order; dropping the reply does not cancel the write.
    fn edit(&self, op: Op, reply: Reply<()>);
    /// The picker for a picture or a video; `None` when it is closed.
    fn pick_media(&self, kind: MediaKind, reply: Reply<Option<String>>);
    /// Width over height of a picture or video, `None` when unreadable.
    fn media_aspect(&self, path: String, kind: MediaKind, reply: Reply<Option<f64>>);
    /// A private preview surface. `None` when the engine has none to give.
    fn open_preview(&self) -> Option<ThemePreview> {
        None
    }
}
