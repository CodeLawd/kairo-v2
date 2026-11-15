//! App-owned persistence and output commands. A window holds its event receiver
//! only while open; dropping it releases all topic subscriptions.
use crate::model::Edit;
use futures_channel::mpsc::UnboundedReceiver;
use kairo_domain::{
    media::MediaPlaylist,
    overlay::{DisplayInfo, TestPatternInput},
    program::StageDisplayStatus,
    settings::AppSettings,
    video_devices::{VideoDeviceSnapshot, VideoTestPattern},
};
use std::{collections::BTreeMap, future::Future, pin::Pin};

pub type Reply<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Snapshot {
    pub settings: AppSettings,
    pub displays: Vec<DisplayInfo>,
    pub devices: VideoDeviceSnapshot,
    pub stages: Vec<StageDisplayStatus>,
    pub outputs: BTreeMap<String, (bool, String)>,
    pub playlists: Vec<MediaPlaylist>,
    pub ndi_audio: bool,
}
impl Snapshot {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            displays: vec![],
            devices: VideoDeviceSnapshot {
                ports: vec![],
                providers: vec![],
                outputs: BTreeMap::new(),
            },
            stages: vec![],
            outputs: BTreeMap::new(),
            playlists: vec![],
            ndi_audio: false,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Command {
    Identify,
    Frame(TestPatternInput),
    TestVerse,
    Clear,
    RefreshCards,
    CardPattern {
        id: String,
        pattern: Option<VideoTestPattern>,
    },
}
pub struct Events {
    pub receiver: UnboundedReceiver<Result<Snapshot, String>>,
    pub lifetime: Box<dyn Send>,
}

pub trait ScreensBackend: Send + Sync + 'static {
    /// A private sample session. Dropping it destroys its offscreen surface.
    fn open_preview(&self) -> Option<ThemePreview> {
        None
    }
    fn load(&self) -> Reply<Result<Snapshot, String>>;
    /// Enqueues immediately, in call order. Dropping the reply does not cancel a write.
    fn edit(&self, edit: Edit) -> Reply<Result<AppSettings, String>>;
    fn run(&self, command: Command) -> Reply<Result<String, String>>;
    fn events(&self) -> Events;
    fn read_ui(&self, key: &str) -> Option<String>;
    fn write_ui(&self, key: &str, value: String);
}

/// A private, muted engine surface used only by the Theme tab. The owning
/// view retains this session for as long as the preview is visible.
pub struct ThemePreview {
    hub: kairo_core::preview::PreviewHub,
    output: String,
    apply: Box<dyn Fn(kairo_domain::overlay::OverlayTheme) + Send + Sync>,
    _lifetime: Box<dyn Send>,
}

impl ThemePreview {
    pub fn new(
        hub: kairo_core::preview::PreviewHub,
        output: String,
        apply: impl Fn(kairo_domain::overlay::OverlayTheme) + Send + Sync + 'static,
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
    pub fn set_theme(&self, theme: kairo_domain::overlay::OverlayTheme) {
        (self.apply)(theme);
    }
}
