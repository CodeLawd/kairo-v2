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

/// Remaining whole seconds, including a negative value when a running timer overruns.
/// `now_ms` is supplied by the caller so all outputs can sample the same instant.
pub fn timer_remaining_sec(timer: &ProgramTimerState, now_ms: f64) -> f64 {
    timer.ends_at.map_or(timer.remaining_sec, |ends_at| {
        ((ends_at - now_ms) / 1000.0).ceil()
    })
}

pub fn timer_running(timer: &ProgramTimerState) -> bool {
    timer.ends_at.is_some()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimerReadout {
    pub seconds: f64,
    pub time_up: bool,
    pub paused: bool,
}

pub fn timer_readout(timer: &ProgramTimerState, now_ms: f64, rollover: bool) -> TimerReadout {
    let raw = timer_remaining_sec(timer, now_ms);
    let running = timer_running(timer);
    TimerReadout {
        seconds: if rollover {
            raw
        } else {
            crate::js::math_max(0.0, raw)
        },
        time_up: timer.duration_sec > 0.0 && (raw < 0.0 || (raw == 0.0 && running)),
        paused: !running && raw != timer.duration_sec,
    }
}

pub fn start_timer(timer: &ProgramTimerState, now_ms: f64) -> ProgramTimerState {
    let mut run = timer.clone();
    if run.ends_at.is_none() {
        run.ends_at = Some(now_ms + run.remaining_sec * 1000.0);
    }
    run
}

pub fn pause_timer(timer: &ProgramTimerState, now_ms: f64) -> ProgramTimerState {
    ProgramTimerState {
        duration_sec: timer.duration_sec,
        ends_at: None,
        remaining_sec: timer_remaining_sec(timer, now_ms),
    }
}

pub fn reset_timer(timer: &ProgramTimerState, duration_sec: Option<f64>) -> ProgramTimerState {
    let duration = crate::js::clamp(
        crate::js::math_round(duration_sec.unwrap_or(timer.duration_sec)),
        0.0,
        24.0 * 3600.0,
    );
    ProgramTimerState {
        duration_sec: duration,
        ends_at: None,
        remaining_sec: duration,
    }
}

/// Build the idle run. The caller supplies the wall-clock resolver for its local
/// timezone, including date rollover and daylight-saving transitions.
pub fn idle_timer(
    def: &ProgramTimerDef,
    now_ms: f64,
    next_time_of_day: impl FnOnce(&str, f64) -> f64,
) -> ProgramTimerState {
    let duration_sec = match def.kind {
        TimerKind::Countdown => {
            crate::js::clamp(crate::js::math_round(def.duration_sec), 0.0, 86_400.0)
        }
        TimerKind::CountdownTo => {
            ((next_time_of_day(&def.target_time, now_ms) - now_ms) / 1000.0).ceil()
        }
    };
    ProgramTimerState {
        duration_sec,
        ends_at: None,
        remaining_sec: duration_sec,
    }
}

/// Start or resume. A time-of-day countdown always resolves a fresh deadline.
pub fn start_timer_for(
    def: &ProgramTimerDef,
    run: &ProgramTimerState,
    now_ms: f64,
    next_time_of_day: impl FnOnce(&str, f64) -> f64,
) -> ProgramTimerState {
    if timer_running(run) || def.kind == TimerKind::Countdown {
        return start_timer(run, now_ms);
    }
    let ends_at = next_time_of_day(&def.target_time, now_ms);
    ProgramTimerState {
        duration_sec: ((ends_at - now_ms) / 1000.0).ceil(),
        ends_at: Some(ends_at),
        remaining_sec: 0.0,
    }
}

/// Format a countdown, including the sign during an overrun.
pub fn format_timer(seconds: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    format!("{sign}{}", crate::media::format_media_clock(seconds.abs()))
}

mod normalization;
pub use normalization::*;

fn parse_clock_target(target: &str) -> Option<(u8, u8)> {
    let [h1, h2, b':', m1, m2] = *target.as_bytes() else {
        return None;
    };
    if ![h1, h2, m1, m2].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let hour = (h1 - b'0') * 10 + h2 - b'0';
    let minute = (m1 - b'0') * 10 + m2 - b'0';
    (hour < 24 && minute < 60).then_some((hour, minute))
}

pub fn transition_ms(transition: &ProgramTransition) -> f64 {
    if transition.kind == TransitionKind::Fade {
        transition.duration_ms
    } else {
        0.0
    }
}

static DEFAULT_TIMER: std::sync::LazyLock<ProgramTimerDef> =
    std::sync::LazyLock::new(ProgramTimerDef::default);

pub fn screen_timer_def<'a>(
    timers: &'a [ProgramTimerDef],
    screen_timer_id: Option<&str>,
) -> &'a ProgramTimerDef {
    timers
        .iter()
        .find(|timer| Some(timer.id.as_str()) == screen_timer_id)
        .or_else(|| timers.first())
        .unwrap_or(&DEFAULT_TIMER)
}

pub fn timer_readout_object(def: &ProgramTimerDef) -> Option<&PropObject> {
    def.theme
        .objects
        .iter()
        .find(|object| matches!(object, PropObject::Timer { .. }))
}

pub fn screen_timer_style(
    presentation: &PresentationSettings,
    state: &ProgramState,
) -> ProgramTimerStyle {
    let def = screen_timer_def(&presentation.timers, state.screen_timer_id.as_deref());
    let (color, overrun_color) = match timer_readout_object(def) {
        Some(PropObject::Timer {
            color,
            overrun_color,
            ..
        }) => (color.as_str(), overrun_color.as_str()),
        _ => ("#5eead4", "#f87171"),
    };
    ProgramTimerStyle {
        rollover: def.overrun,
        color: color.into(),
        overrun_color: overrun_color.into(),
        backdrop: false,
    }
}

pub fn timer_view_for(
    def: &ProgramTimerDef,
    run: Option<&ProgramTimerState>,
    now_ms: f64,
    next_time_of_day: impl FnOnce(&str, f64) -> f64,
) -> PropTimerView {
    let idle;
    let run = match run {
        Some(run) => run,
        None => {
            idle = idle_timer(def, now_ms, next_time_of_day);
            &idle
        }
    };
    PropTimerView {
        duration_sec: run.duration_sec,
        ends_at: run.ends_at,
        remaining_sec: run.remaining_sec,
        rollover: def.overrun,
        now: now_ms,
    }
}

pub fn prop_timer_view_for(
    state: &ProgramState,
    presentation: &PresentationSettings,
    now_ms: f64,
    next_time_of_day: impl FnOnce(&str, f64) -> f64,
) -> PropTimerView {
    timer_view_for(
        screen_timer_def(&presentation.timers, state.screen_timer_id.as_deref()),
        Some(&state.timer),
        now_ms,
        next_time_of_day,
    )
}

pub fn confidence_sizes(size: ConfidenceSize) -> ConfidenceSizes {
    let (bar_px, message_px) = match size {
        ConfidenceSize::Small => (48.0, 40.0),
        ConfidenceSize::Medium => (72.0, 60.0),
        ConfidenceSize::Large => (104.0, 84.0),
    };
    ConfidenceSizes { bar_px, message_px }
}

pub fn confidence_for(
    output: &crate::overlay::OverlayOutput,
    state: &ProgramState,
    presentation: &PresentationSettings,
) -> Option<ConfidenceInfo> {
    let clock = output.show.clock && state.clock_on_screens;
    let stage_message = output
        .show
        .stage_message
        .then(|| state.stage_message.clone())
        .flatten();
    if !clock && stage_message.as_ref().is_none_or(String::is_empty) {
        return None;
    }
    Some(ConfidenceInfo {
        clock,
        stage_message,
        layout: output.confidence.clone(),
        sizes: confidence_sizes(output.confidence.size),
        style: screen_timer_style(presentation, state),
    })
}

/// Wall clock display in the caller's timezone.
pub fn format_clock(time: impl chrono::Timelike) -> String {
    let hour = time.hour();
    let clock_hour = (hour + 11) % 12 + 1;
    let suffix = if hour < 12 { "AM" } else { "PM" };
    format!("{clock_hour}:{:02} {suffix}", time.minute())
}

/// Resolve today's or tomorrow's local deadline without reading the system clock.
/// In a fall-back overlap, the earlier instant wins. In a spring-forward gap,
/// advance by the size of the offset change, as JavaScript Date does.
pub fn next_time_of_day<T: chrono::TimeZone>(target: &str, now_ms: f64, zone: &T) -> f64 {
    use chrono::{Duration, Offset};
    let mut parts = target.split(':');
    let hour = parts.next().map_or(f64::NAN, crate::js::string_to_number);
    let minute = parts.next().map_or(f64::NAN, crate::js::string_to_number);
    let minutes = hour.trunc() * 60.0 + minute.trunc();
    if !minutes.is_finite() || minutes.abs() > 2.88e11 {
        return f64::NAN;
    }
    if !now_ms.is_finite() || now_ms.abs() > 8.64e15 {
        return f64::NAN;
    }
    let Some(now) = zone.timestamp_millis_opt(now_ms.trunc() as i64).single() else {
        return f64::NAN;
    };
    let resolve = |local: chrono::NaiveDateTime| {
        if let Some(instant) = zone.from_local_datetime(&local).earliest() {
            return Some(instant);
        }
        let before = zone
            .offset_from_local_datetime(&local.checked_sub_signed(Duration::days(2))?)
            .latest()?;
        let after = zone
            .offset_from_local_datetime(&local.checked_add_signed(Duration::days(2))?)
            .earliest()?;
        let shift =
            i64::from(after.fix().local_minus_utc()) - i64::from(before.fix().local_minus_utc());
        zone.from_local_datetime(&local.checked_add_signed(Duration::seconds(shift))?)
            .earliest()
    };
    let Some(local) = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| midnight.checked_add_signed(Duration::minutes(minutes as i64)))
    else {
        return f64::NAN;
    };
    let Some(today) = resolve(local) else {
        return f64::NAN;
    };
    if today.timestamp_millis() as f64 > now_ms {
        return today.timestamp_millis() as f64;
    }
    // Date.setDate uses the already resolved clock fields if setHours fell in a gap.
    today
        .naive_local()
        .checked_add_signed(Duration::days(1))
        .and_then(resolve)
        .map_or(f64::NAN, |time| time.timestamp_millis() as f64)
}

string_enum!(
    pub enum OutputShowKey {
        Scripture = "scripture",
        Lyrics = "lyrics",
        Documents = "documents",
        Backgrounds = "backgrounds",
        Poll = "poll",
        Question = "question",
        Messages = "messages",
        Overlays = "overlays",
        Countdown = "countdown",
        Clock = "clock",
        StageMessage = "stageMessage",
    }
);

pub const PLAYLIST_SHOW_KEYS: &[OutputShowKey] = &[
    OutputShowKey::Poll,
    OutputShowKey::Question,
    OutputShowKey::Messages,
    OutputShowKey::Overlays,
    OutputShowKey::Countdown,
    OutputShowKey::Clock,
    OutputShowKey::StageMessage,
];

pub fn output_show_label(key: OutputShowKey) -> &'static str {
    match key {
        OutputShowKey::Scripture => "Scripture",
        OutputShowKey::Lyrics => "Lyrics",
        OutputShowKey::Documents => "Slides",
        OutputShowKey::Backgrounds => "Backgrounds",
        OutputShowKey::Poll => "Poll results",
        OutputShowKey::Question => "Audience questions",
        OutputShowKey::Messages => "Messages",
        OutputShowKey::Overlays => "Logo & props",
        OutputShowKey::Countdown => "Countdown",
        OutputShowKey::Clock => "Clock",
        OutputShowKey::StageMessage => "Stage message",
    }
}

/// Rebuild the live program snapshot after a timer update, retaining the on-screen
/// readout when its selected timer has no separate run yet.
pub fn program_state_with_timer_run(
    state: &ProgramState,
    presentation: &PresentationSettings,
    timer_id: String,
    run: ProgramTimerState,
) -> ProgramState {
    let mut next = state.clone();
    next.timers.insert(timer_id, run);
    let screen = screen_timer_def(&presentation.timers, state.screen_timer_id.as_deref());
    if let Some(run) = next.timers.get(&screen.id) {
        next.timer = run.clone();
    }
    next
}
