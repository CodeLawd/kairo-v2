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
    /// A Syphon server (macOS), named by [`SinkSpec::syphon`].
    Syphon {
        name: String,
    },
    /// A video card port (DeckLink). The engine acquires the port, opens
    /// the frame ring at the mode raster and runs the helper (prompt 062).
    Device {
        port_id: String,
        mode_id: String,
        key_fill: bool,
    },
}

impl SinkSpec {
    pub fn is_display(&self) -> bool {
        matches!(self, Self::Display(_))
    }

    /// `reconcileSyphon`: a Syphon server named after its output ("Kairo
    /// Lower Third"), or "Kairo Syphon n" (n counts enabled Syphon outputs
    /// from 1) when the output has no name. The engine uses the name as
    /// given; a new name restarts the server.
    pub fn syphon(output_name: &str, index: usize) -> Self {
        let name = if output_name.is_empty() {
            format!("Kairo Syphon {}", index + 1)
        } else {
            format!("Kairo {output_name}")
        };
        Self::Syphon { name }
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
    /// Surface-manager eligibility: shows backgrounds AND follows the program.
    #[serde(default = "web_enabled_default")]
    pub web_enabled: bool,
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

fn web_enabled_default() -> bool {
    true
}

impl Default for SurfaceConfig {
    /// `DEFAULT_CONFIG`. `video_layout: None` stands for `DEFAULT_VIDEO_SETTINGS`.
    fn default() -> Self {
        Self {
            web_enabled: true,
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

/// Validated again by the engine; web content cannot send these commands.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum WebInput {
    Move {
        x: f64,
        y: f64,
    },
    Click {
        x: f64,
        y: f64,
    },
    Text {
        text: String,
    },
    /// Enter, Tab, Backspace, Delete, Escape, arrows, Home, End, PageUp/Down.
    Key {
        key: String,
    },
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
    /// Native control-window display (CGDirectDisplayID or Windows HMONITOR).
    ControlDisplay { native_id: Option<u64> },
    /// Three-second labels, one on every connected display.
    IdentifyDisplays,
    /// Forget card faults and enumerate the cards again (`refresh`).
    RefreshVideoDevices,
    /// Live → Start Capture: records surface `output` to a movie file with
    /// `settings` (`CaptureSettings` JSON). `source_name` names the file.
    /// Not replayed after a relaunch: the file of a crashed engine is lost.
    StartCapture {
        output: String,
        source_name: String,
        settings: Json,
    },
    /// Finishes the recording's file. Nothing when none is running.
    StopCapture,
    /// Five-second physical shape card. Frame height is 360..4320.
    DisplayTestPattern {
        display: u32,
        name: String,
        shape: String,
        frame_height: u32,
    },
    /// The independent stage display's complete current view.
    SetStage { output: String, view: Json },
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
    /// Direct input to the shared live page, in its 1920×1080 coordinates.
    WebInput { output: String, input: WebInput },
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
            | WebInput { output, .. }
            | WebScroll { output, .. }
            | SetTestPattern { output, .. }
            | SetStage { output, .. } => Some(output),
            RetainSurfaces { .. }
            | Shutdown
            | ControlDisplay { .. }
            | IdentifyDisplays
            | RefreshVideoDevices
            | StartCapture { .. }
            | StopCapture
            | DisplayTestPattern { .. } => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Query {
    /// Native window geometry and power assertion for output qualification.
    DisplayState,
    /// `__videoEnded`: has the background video finished, or is there none.
    VideoEnded { output: String },
    /// `__documentPlayback`.
    DocumentPlayback { output: String },
    /// Every surface's state, for diagnostics and replay checks.
    State,
    /// The video card snapshot (`VideoDeviceSnapshot`).
    VideoDevices,
    /// `ndiAudioSupported`: can this engine's NDI feeds carry program sound.
    NdiAudioSupported,
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
    DisplayState {
        windows: Vec<WindowInfo>,
        sleep_blocked: bool,
    },
    VideoEnded(bool),
    /// `DocumentPlaybackStatus`, or `None` outside document mode.
    DocumentPlayback(Option<Json>),
    State(Vec<SurfaceState>),
    /// `VideoDeviceSnapshot`.
    VideoDevices(Json),
    NdiAudioSupported(bool),
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
    /// The toolkit/OS identity used for control-window location reporting.
    pub native_id: u64,
    pub primary: bool,
    pub hosts_main_window: bool,
    pub refresh_millihertz: Option<u32>,
}

impl DisplayInfo {
    /// Stored Electron sizes are in display-independent pixels, not framebuffer pixels.
    pub fn logical_size(&self) -> (u32, u32) {
        let scale = if self.scale_factor.is_finite() && self.scale_factor > 0. {
            self.scale_factor
        } else {
            1.
        };
        (
            (self.bounds.width as f64 / scale).round() as u32,
            (self.bounds.height as f64 / scale).round() as u32,
        )
    }
    pub fn description(&self) -> String {
        let label = self.label.trim();
        format!(
            "{} · {}×{}",
            if label.is_empty() { "Display" } else { label },
            self.bounds.width,
            self.bounds.height
        )
    }
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
    /// Persist only when the stored assignment still equals old_id.
    DisplayRebound {
        output: String,
        old_id: u32,
        new_id: u32,
        stage: bool,
    },
    /// A top-level load, redirect or SPA location change of a shared live page.
    WebNavigated {
        output: String,
        url: String,
    },
    Displays(Vec<DisplayInfo>),
    /// The video card snapshot changed (`VideoDeviceSnapshot`).
    VideoDevices(Json),
    SurfaceStatus {
        output: String,
        status: SurfaceStatus,
    },
    /// The surface's frame carrying command `seq` reached its sink.
    FrameCommitted {
        output: String,
        seq: u64,
    },
    /// The answer to `SwapPage` command `seq` (`swapDocumentPage`'s
    /// result). `false`: the surface was not showing a document from that
    /// source, the page could not be decoded, or a newer turn replaced it;
    /// the caller then sends a full `ShowDocument`. Sent before the
    /// `FrameCommitted` that carries a swapped page.
    PageSwapped {
        output: String,
        seq: u64,
        swapped: bool,
    },
    Health(Health),
    Stats(Vec<SinkStats>),
    /// The recording's `CaptureStatus` JSON, on every change.
    Capture(Json),
    Error {
        output: Option<String>,
        message: String,
    },
}

/// Physical windows owned by the engine; no other process/window is inspected.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    Program,
    Stage,
    Identify,
    Pattern,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub output: String,
    pub display: u32,
    pub kind: WindowKind,
    pub rehearsal: bool,
    pub bounds: Rect,
    /// CGWindowID or HWND, only for identifying our own native output in tests.
    pub native_window: u64,
}
