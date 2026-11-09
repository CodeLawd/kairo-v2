//! Presentation settings and live program state, from `src/lib/program.ts`.

use crate::audience::PollView;
use crate::serde_util::{Extra, required, string_enum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

string_enum!(
    pub enum TransitionKind {
        Cut = "cut",
        Fade = "fade",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramTransition {
    pub kind: TransitionKind,
    pub duration_ms: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum JoinTag {
        Join = "join",
    }
);

/// `ProgramQuestion`: an audience question, or the join card.
/// Untagged in TypeScript; the join card is told apart by `kind: 'join'`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProgramQuestion {
    Join {
        kind: JoinTag,
        code: String,
        url: String,
        #[serde(rename = "qrDataUrl")]
        qr_data_url: String,
    },
    Question {
        text: String,
        #[serde(deserialize_with = "required")]
        author: Option<String>,
    },
}

string_enum!(
    pub enum ProgramQuestionFont {
        Sans = "sans",
        Serif = "serif",
        Mono = "mono",
    }
);
string_enum!(
    pub enum QuestionPosition {
        Top = "top",
        Center = "center",
        Bottom = "bottom",
    }
);
string_enum!(
    pub enum QuestionAlign {
        Left = "left",
        Center = "center",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramQuestionStyle {
    pub position: QuestionPosition,
    pub align: QuestionAlign,
    pub font_family: ProgramQuestionFont,
    pub font_size_px: f64,
    pub color: String,
    pub background: String,
    pub background_opacity: f64,
    pub border_radius_px: f64,
    pub width_pct: f64,
    pub show_author: bool,
    pub join_headline: String,
    pub qr_size_px: f64,
    pub show_join_url: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum TopOrBottom {
        Top = "top",
        Bottom = "bottom",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramMessageStyle {
    pub position: TopOrBottom,
    pub font_size_px: f64,
    pub color: String,
    pub background: String,
    pub scroll: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramTimerStyle {
    pub rollover: bool,
    pub color: String,
    pub overrun_color: String,
    pub backdrop: bool,
}

string_enum!(
    pub enum PropPosition {
        TopLeft = "top-left",
        TopRight = "top-right",
        BottomLeft = "bottom-left",
        BottomRight = "bottom-right",
        Center = "center",
    }
);
string_enum!(
    pub enum PropAlign {
        Left = "left",
        Center = "center",
        Right = "right",
    }
);
string_enum!(
    pub enum PropShape {
        Rectangle = "rectangle",
        Ellipse = "ellipse",
    }
);
string_enum!(
    pub enum PropImageFit {
        Contain = "contain",
        Cover = "cover",
    }
);

/// Fields every prop object has: `PropObjectCommon` (`PropBox` plus id and opacity).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropFrame {
    pub id: String,
    pub opacity: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `PropObject`, tagged by `kind`. Unknown keys are kept in each variant's `extra`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PropObject {
    #[serde(rename_all = "camelCase")]
    Text {
        #[serde(flatten)]
        frame: PropFrame,
        text: String,
        font_size_px: f64,
        color: String,
        bold: bool,
        align: PropAlign,
        #[serde(flatten)]
        extra: Extra,
    },
    #[serde(rename_all = "camelCase")]
    Timer {
        #[serde(flatten)]
        frame: PropFrame,
        font_size_px: f64,
        color: String,
        overrun_color: String,
        bold: bool,
        align: PropAlign,
        #[serde(flatten)]
        extra: Extra,
    },
    #[serde(rename_all = "camelCase")]
    Shape {
        #[serde(flatten)]
        frame: PropFrame,
        shape: PropShape,
        fill: String,
        radius_px: f64,
        #[serde(flatten)]
        extra: Extra,
    },
    #[serde(rename_all = "camelCase")]
    Image {
        #[serde(flatten)]
        frame: PropFrame,
        media_path: String,
        fit: PropImageFit,
        #[serde(flatten)]
        extra: Extra,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgramProp {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "required")]
    pub background: Option<String>,
    pub objects: Vec<PropObject>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramLogo {
    pub media_path: String,
    pub background: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageDisplayConfig {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    #[serde(deserialize_with = "required")]
    pub display_id: Option<f64>,
    pub display_label: String,
    #[serde(deserialize_with = "required")]
    pub display_size: Option<crate::overlay::Size>,
    pub show_next: bool,
    pub show_clock: bool,
    pub show_timer: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramAudioSettings {
    pub enabled: bool,
    pub volume: f64,
    pub output_label: String,
    pub ndi: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum VideoLayout {
        Full = "full",
        Pip = "pip",
        Split = "split",
    }
);
string_enum!(
    pub enum Side {
        Left = "left",
        Right = "right",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoBox {
    pub x_pct: f64,
    pub y_pct: f64,
    pub width_pct: f64,
    pub height_pct: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSettings {
    pub layout: VideoLayout,
    #[serde(rename = "box")]
    pub frame: VideoBox,
    pub radius_px: f64,
    pub side: Side,
    pub split_pct: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgramSavedMessage {
    pub id: String,
    pub text: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimerTheme {
    #[serde(deserialize_with = "required")]
    pub background: Option<String>,
    pub objects: Vec<PropObject>,
    #[serde(flatten)]
    pub extra: Extra,
}

string_enum!(
    pub enum TimerKind {
        Countdown = "countdown",
        CountdownTo = "countdownTo",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramTimerDef {
    pub id: String,
    pub name: String,
    pub kind: TimerKind,
    pub duration_sec: f64,
    /// 24h `HH:MM`, used by `countdownTo`.
    pub target_time: String,
    pub overrun: bool,
    pub theme: TimerTheme,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `AppSettings.presentation`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationSettings {
    pub transition: ProgramTransition,
    pub question: ProgramQuestionStyle,
    pub message: ProgramMessageStyle,
    pub messages: Vec<ProgramSavedMessage>,
    pub props: Vec<ProgramProp>,
    pub logo: ProgramLogo,
    pub stage_displays: Vec<StageDisplayConfig>,
    pub audio: ProgramAudioSettings,
    pub video: VideoSettings,
    pub timers: Vec<ProgramTimerDef>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `defaultTimerTheme(color, overrunColor)`.
pub fn default_timer_theme(color: &str, overrun_color: &str) -> TimerTheme {
    TimerTheme {
        background: None,
        objects: vec![PropObject::Timer {
            frame: PropFrame {
                id: "timer-readout".into(),
                opacity: 1.0,
                x: 0.3,
                y: 0.05,
                w: 0.4,
                h: 0.12,
            },
            font_size_px: 96.0,
            color: color.into(),
            overrun_color: overrun_color.into(),
            bold: true,
            align: PropAlign::Center,
            extra: Extra::new(),
        }],
        extra: Extra::new(),
    }
}

/// `DEFAULT_VIDEO_SETTINGS`.
impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            layout: VideoLayout::Full,
            frame: VideoBox {
                x_pct: 68.0,
                y_pct: 68.0,
                width_pct: 30.0,
                height_pct: 30.0,
                extra: Extra::new(),
            },
            radius_px: 12.0,
            side: Side::Right,
            split_pct: 50.0,
            extra: Extra::new(),
        }
    }
}

/// `DEFAULT_TIMER`.
impl Default for ProgramTimerDef {
    fn default() -> Self {
        Self {
            id: "timer-main".into(),
            name: "Countdown".into(),
            kind: TimerKind::Countdown,
            duration_sec: 300.0,
            target_time: "10:00".into(),
            overrun: true,
            theme: default_timer_theme("#5eead4", "#f87171"),
            extra: Extra::new(),
        }
    }
}

/// `DEFAULT_PRESENTATION_SETTINGS`.
impl Default for PresentationSettings {
    fn default() -> Self {
        Self {
            transition: ProgramTransition {
                kind: TransitionKind::Cut,
                duration_ms: 400.0,
                extra: Extra::new(),
            },
            question: ProgramQuestionStyle {
                position: QuestionPosition::Center,
                align: QuestionAlign::Left,
                font_family: ProgramQuestionFont::Sans,
                font_size_px: 64.0,
                color: "#ffffff".into(),
                background: "#18212d".into(),
                background_opacity: 95.0,
                border_radius_px: 24.0,
                width_pct: 85.0,
                show_author: true,
                join_headline: "Ask a question".into(),
                qr_size_px: 280.0,
                show_join_url: true,
                extra: Extra::new(),
            },
            message: ProgramMessageStyle {
                position: TopOrBottom::Bottom,
                font_size_px: 44.0,
                color: "#ffffff".into(),
                background: "rgba(0,0,0,0.72)".into(),
                scroll: false,
                extra: Extra::new(),
            },
            messages: Vec::new(),
            props: Vec::new(),
            logo: ProgramLogo {
                media_path: String::new(),
                background: "#000000".into(),
                extra: Extra::new(),
            },
            stage_displays: Vec::new(),
            audio: ProgramAudioSettings {
                enabled: true,
                volume: 1.0,
                output_label: String::new(),
                ndi: false,
                extra: Extra::new(),
            },
            video: VideoSettings::default(),
            timers: vec![ProgramTimerDef::default()],
            extra: Extra::new(),
        }
    }
}

/// Which kinds of content an output shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputShowFilter {
    pub scripture: bool,
    pub lyrics: bool,
    pub documents: bool,
    pub backgrounds: bool,
    pub poll: bool,
    pub question: bool,
    pub messages: bool,
    pub overlays: bool,
    pub countdown: bool,
    pub clock: bool,
    pub stage_message: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `DEFAULT_SHOW_FILTER`.
impl Default for OutputShowFilter {
    fn default() -> Self {
        Self {
            scripture: true,
            lyrics: true,
            documents: true,
            backgrounds: true,
            poll: true,
            question: true,
            messages: true,
            overlays: true,
            countdown: false,
            clock: false,
            stage_message: false,
            extra: Extra::new(),
        }
    }
}

string_enum!(
    pub enum ConfidenceSize {
        Small = "small",
        Medium = "medium",
        Large = "large",
    }
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceLayout {
    pub position: TopOrBottom,
    pub size: ConfidenceSize,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `DEFAULT_CONFIDENCE_LAYOUT`.
impl Default for ConfidenceLayout {
    fn default() -> Self {
        Self {
            position: TopOrBottom::Top,
            size: ConfidenceSize::Medium,
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgramSlideInfo {
    pub reference: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramTimerState {
    pub duration_sec: f64,
    #[serde(deserialize_with = "required")]
    pub ends_at: Option<f64>,
    pub remaining_sec: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgramWeb {
    pub url: String,
    pub reload: f64,
}

/// What is on screen now. Pushed from main to every renderer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramState {
    #[serde(deserialize_with = "required")]
    pub poll: Option<PollView>,
    #[serde(deserialize_with = "required")]
    pub question: Option<ProgramQuestion>,
    #[serde(deserialize_with = "required")]
    pub message: Option<String>,
    #[serde(deserialize_with = "required")]
    pub stage_message: Option<String>,
    pub active_prop_ids: Vec<String>,
    pub logo: bool,
    #[serde(deserialize_with = "required")]
    pub camera: Option<String>,
    #[serde(deserialize_with = "required")]
    pub camera_audio: Option<String>,
    #[serde(deserialize_with = "required")]
    pub web: Option<ProgramWeb>,
    pub timer: ProgramTimerState,
    pub timers: BTreeMap<String, ProgramTimerState>,
    #[serde(deserialize_with = "required")]
    pub screen_timer_id: Option<String>,
    pub countdown_on_screens: bool,
    pub clock_on_screens: bool,
    #[serde(deserialize_with = "required")]
    pub current: Option<ProgramSlideInfo>,
    #[serde(deserialize_with = "required")]
    pub next: Option<ProgramSlideInfo>,
}

/// `EMPTY_PROGRAM_STATE`.
impl Default for ProgramState {
    fn default() -> Self {
        Self {
            poll: None,
            question: None,
            message: None,
            stage_message: None,
            active_prop_ids: Vec::new(),
            logo: false,
            camera: None,
            camera_audio: None,
            web: None,
            timer: ProgramTimerState {
                duration_sec: 300.0,
                ends_at: None,
                remaining_sec: 300.0,
            },
            timers: BTreeMap::new(),
            screen_timer_id: None,
            countdown_on_screens: false,
            clock_on_screens: false,
            current: None,
            next: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropTimerView {
    pub duration_sec: f64,
    #[serde(deserialize_with = "required")]
    pub ends_at: Option<f64>,
    pub remaining_sec: f64,
    pub rollover: bool,
    pub now: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfidenceSizes {
    pub bar_px: f64,
    pub message_px: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfidenceInfo {
    pub clock: bool,
    #[serde(deserialize_with = "required")]
    pub stage_message: Option<String>,
    pub layout: ConfidenceLayout,
    pub sizes: ConfidenceSizes,
    pub style: ProgramTimerStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StageDisplayStatus {
    pub id: String,
    pub ready: bool,
    pub reason: String,
}

/// `normalizeConfidenceLayout` (prompt 013 needs it for outputs; 016 owns
/// the rest of `program.ts`).
pub fn normalize_confidence_layout(raw: Option<&serde_json::Value>) -> ConfidenceLayout {
    use crate::normalize::{as_object, safe_enum};
    let r = as_object(raw);
    ConfidenceLayout {
        position: safe_enum(r.get("position"), TopOrBottom::Top),
        size: safe_enum(r.get("size"), ConfidenceSize::Medium),
        extra: Extra::new(),
    }
}

/// `normalizeShowFilter` (prompt 013, as above).
pub fn normalize_show_filter(raw: Option<&serde_json::Value>) -> OutputShowFilter {
    use crate::normalize::{as_object, safe_bool};
    let r = as_object(raw);
    let d = OutputShowFilter::default();
    OutputShowFilter {
        scripture: safe_bool(r.get("scripture"), d.scripture),
        lyrics: safe_bool(r.get("lyrics"), d.lyrics),
        documents: safe_bool(r.get("documents"), d.documents),
        backgrounds: safe_bool(r.get("backgrounds"), d.backgrounds),
        poll: safe_bool(r.get("poll"), d.poll),
        question: safe_bool(r.get("question"), d.question),
        messages: safe_bool(r.get("messages"), d.messages),
        overlays: safe_bool(r.get("overlays"), d.overlays),
        countdown: safe_bool(r.get("countdown"), d.countdown),
        clock: safe_bool(r.get("clock"), d.clock),
        stage_message: safe_bool(r.get("stageMessage"), d.stage_message),
        extra: Extra::new(),
    }
}
