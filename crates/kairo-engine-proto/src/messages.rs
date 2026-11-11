//! Every message on the engine socket.
//!
//! Postcard encodes by field order, so these types use no `skip`, `flatten`
//! or `untagged` attributes. Domain values whose serde shape relies on them
//! (themes, backgrounds, document slides, playback, confidence info) travel
//! as [`Json`] text and are parsed by the scene builder in the engine.

use crate::surface::SurfaceState;
use serde::{Deserialize, Serialize};

/// A domain value as JSON text, exactly as `JSON.stringify` would carry it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Json(pub String);

impl Json {
    pub fn encode<T: Serialize>(value: &T) -> Self {
        Self(serde_json::to_string(value).expect("domain values serialize"))
    }

    pub fn decode<T: serde::de::DeserializeOwned>(&self) -> serde_json::Result<T> {
        serde_json::from_str(&self.0)
    }

    pub fn value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::from_str(&self.0)
    }

    pub fn from_value(value: &serde_json::Value) -> Self {
        Self(value.to_string())
    }
}

// ─── Handshake ───────────────────────────────────────────────────────────────

/// The first message a client sends. The engine closes the connection
/// without reading further unless the version and token both match.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Hello {
    pub protocol: u32,
    pub token: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Welcome {
    pub protocol: u32,
    pub engine_version: String,
    pub pid: u32,
}

// ─── Client to engine ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ToEngine {
    Hello(Hello),
    /// `seq` grows by one per command; `FrameCommitted` reports the newest
    /// sequence a surface's frame carries.
    Command {
        seq: u64,
        command: Command,
    },
    Query {
        id: u64,
        query: Query,
    },
    /// Heartbeat. Answered from the engine's main loop, so a hung loop
    /// stops answering even while the socket thread is alive.
    Ping {
        nonce: u64,
    },
}

/// How a surface's pixels leave the engine.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SinkSpec {
    /// A window on one physical display.
    Display(DisplaySink),
    Ndi {
        name: String,
    },
    Syphon {
        name: String,
    },
    /// A video card port (DeckLink), through the frame ring.
    Device {
        port_id: String,
        ring: Option<String>,
    },
}

impl SinkSpec {
    pub fn is_display(&self) -> bool {
        matches!(self, Self::Display(_))
    }
}

/// What an output remembers about the display it was pointed at.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DisplayBinding {
    pub display_id: Option<u32>,
    pub label: String,
    pub size: Option<(u32, u32)>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DisplaySink {
    pub binding: DisplayBinding,
    /// The bound display also holds Kairo's controls: a half-size window.
    pub rehearsal: bool,
    /// 360 to 4320; the frame is always 1920 wide.
    pub frame_height: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SurfaceSpec {
    /// The output id. Surface ids are output ids.
    pub output: String,
    /// Shown in logs and window titles.
    pub label: String,
    pub sink: SinkSpec,
}

/// `SurfaceConfig` in `program-surface.ts`, the argument of `__configure`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SurfaceConfig {
    pub transition_ms: f64,
    pub audio: AudioConfig,
    pub camera_audible: bool,
    pub ndi_audio: bool,
    /// `VideoSettings`: how the live video input sits with the slide.
    pub video_layout: Option<Json>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AudioConfig {
    pub enabled: bool,
    pub volume: f64,
    pub output_label: String,
}

impl Default for SurfaceConfig {
    /// `DEFAULT_CONFIG`. `video_layout: None` stands for `DEFAULT_VIDEO_SETTINGS`.
    fn default() -> Self {
        Self {
            transition_ms: 0.0,
            audio: AudioConfig {
                enabled: false,
                volume: 1.0,
                output_label: String::new(),
            },
            camera_audible: false,
            ndi_audio: false,
            video_layout: None,
        }
    }
}

/// `RenderState`: one painted slide.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SlideContent {
    pub reference: String,
    pub text: String,
    /// `OverlayTheme`.
    pub theme: Json,
    /// `OverlayColoredLine[]`.
    pub colored_lines: Option<Json>,
}

/// The markup layers above the slide (`SurfaceLayers` minus camera, web, info).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayerKind {
    Poll,
    Question,
    Message,
    Props,
    Timer,
    Logo,
}

impl LayerKind {
    pub const ALL: [Self; 6] = [
        Self::Poll,
        Self::Question,
        Self::Message,
        Self::Props,
        Self::Timer,
        Self::Logo,
    ];
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CameraLayer {
    pub label: String,
    pub audio_label: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WebLayer {
    pub url: String,
    pub reload: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestPattern {
    Bars,
    Text,
    Motion,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum VideoTransport {
    Pause(bool),
    Seek(f64),
}

/// Architecture section 5.3, one to one with the Electron shell.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Command {
    /// Creates a surface, or retargets an existing one to a new sink.
    OpenSurface(SurfaceSpec),
    /// Destroys every surface not named. Sent first when state is replayed.
    RetainSurfaces { outputs: Vec<String> },
    /// `ManagedWindow.close` (display surfaces park until restored) or
    /// `destroy` (gone for good).
    CloseSurface { output: String, destroy: bool },
    /// `ProgramSurface.restore`: unpark and show what it last showed.
    RestoreSurface { output: String },
    /// `__configure`.
    Configure {
        output: String,
        config: SurfaceConfig,
    },
    /// `__setContent(html, instant)`.
    ShowSlide {
        output: String,
        /// The output whose deck this is; scopes page swaps.
        source: String,
        slide: SlideContent,
        instant: bool,
    },
    /// `setBackground`: the background under what is on screen.
    SetBackground { output: String, background: Json },
    /// `__patchBackground`: loop and colour without restarting the video.
    PatchBackground {
        output: String,
        /// `MediaPlayback`.
        playback: Json,
        /// `mediaFilterCss(playback)`.
        filter: String,
    },
    /// `__clearText`, optionally with a new background (`clearText(background)`).
    ClearText {
        output: String,
        background: Option<Json>,
    },
    /// `__setContent("")`.
    Clear { output: String },
    /// `__setVideoPaused`, `__seekVideo`.
    VideoTransport {
        output: String,
        transport: VideoTransport,
    },
    /// `__markDocument` plus the first page's videos.
    ShowDocument {
        output: String,
        source: String,
        slide: SlideContent,
        document: Option<Json>,
    },
    /// `__swapDocumentPage`.
    SwapPage {
        output: String,
        source: String,
        media_path: String,
        document: Option<Json>,
    },
    /// `__preloadDocumentPage`.
    PreloadPage { output: String, media_path: String },
    /// `__setDocumentVideos`.
    DocumentVideos { output: String, document: Json },
    /// `__controlDocumentVideo`.
    ControlDocumentVideo {
        output: String,
        id: String,
        page: f64,
        video: u32,
        /// `DocumentVideoCommand`.
        command: Json,
    },
    /// `__setPoll`, `__setQuestion`, `__setMessage`, `__setProps`,
    /// `__setTimerDesign`, `__setLogo`: the layer as data (`ProgramLayer`
    /// from `programLayersFor`), `None` to clear it.
    SetLayer {
        output: String,
        kind: LayerKind,
        layer: Option<Json>,
    },
    /// `__setInfo`. `ConfidenceInfo`.
    SetInfo { output: String, info: Option<Json> },
    /// `__setCamera`.
    SetCamera {
        output: String,
        camera: Option<CameraLayer>,
    },
    /// `__setWeb`.
    SetWeb {
        output: String,
        web: Option<WebLayer>,
    },
    /// `__setWebPointer`.
    WebPointer {
        output: String,
        point: Option<(f64, f64)>,
    },
    /// `scrollWeb`.
    WebScroll { output: String, x: f64, y: f64 },
    SetTestPattern {
        output: String,
        pattern: Option<TestPattern>,
    },
    /// Close every window and exit now.
    Shutdown,
}

impl Command {
    /// The surface this command is for, if any.
    pub fn output(&self) -> Option<&str> {
        use Command::*;
        match self {
            OpenSurface(spec) => Some(&spec.output),
            CloseSurface { output, .. }
            | RestoreSurface { output }
            | Configure { output, .. }
            | ShowSlide { output, .. }
            | SetBackground { output, .. }
            | PatchBackground { output, .. }
            | ClearText { output, .. }
            | Clear { output }
            | VideoTransport { output, .. }
            | ShowDocument { output, .. }
            | SwapPage { output, .. }
            | PreloadPage { output, .. }
            | DocumentVideos { output, .. }
            | ControlDocumentVideo { output, .. }
            | SetLayer { output, .. }
            | SetInfo { output, .. }
            | SetCamera { output, .. }
            | SetWeb { output, .. }
            | WebPointer { output, .. }
            | WebScroll { output, .. }
            | SetTestPattern { output, .. } => Some(output),
            RetainSurfaces { .. } | Shutdown => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Query {
    /// `__videoEnded`: has the background video finished, or is there none.
    VideoEnded { output: String },
    /// `__documentPlayback`.
    DocumentPlayback { output: String },
    /// Every surface's state, for diagnostics and replay checks.
    State,
}

// ─── Engine to client ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum FromEngine {
    Welcome(Welcome),
    /// Sent instead of `Welcome`, then the connection closes.
    Rejected {
        reason: String,
    },
    Pong {
        nonce: u64,
    },
    Reply {
        id: u64,
        reply: Reply,
    },
    Event(Event),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Reply {
    VideoEnded(bool),
    /// `DocumentPlaybackStatus`, or `None` outside document mode.
    DocumentPlayback(Option<Json>),
    State(Vec<SurfaceState>),
    /// The query named a surface the engine does not have.
    UnknownSurface,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    pub id: u32,
    pub label: String,
    pub bounds: Rect,
    pub scale_factor: f64,
}

/// Whether a surface can take content now.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum SurfaceStatus {
    Ready,
    /// The display is gone; nothing reopens it on another screen.
    Parked {
        reason: String,
    },
    /// The sink cannot run in this engine (no GPU, no driver, not built).
    Unavailable {
        reason: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct SinkStats {
    pub output: String,
    pub produced: u64,
    pub dropped: u64,
    pub late: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Health {
    pub uptime_ms: u64,
    pub surfaces: u32,
    pub windows: u32,
    pub gpu: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Event {
    Displays(Vec<DisplayInfo>),
    SurfaceStatus {
        output: String,
        status: SurfaceStatus,
    },
    /// The surface's frame carrying command `seq` reached its sink.
    FrameCommitted {
        output: String,
        seq: u64,
    },
    Health(Health),
    Stats(Vec<SinkStats>),
    Error {
        output: Option<String>,
        message: String,
    },
}
