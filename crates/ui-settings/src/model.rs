//! The rules `Settings.tsx` and its panels apply, kept apart from the view
//! so they can be tested without a window: navigation and history, the
//! level meter, number fields, labels and the text each panel shows.

use kairo_domain::documents::{PowerpointConverterId, SlideConverterState, SlideConverterStatus};
use kairo_domain::keyboard_shortcuts::{SHORTCUT_COMMANDS, command_for_shortcut};
use kairo_domain::scripture::{
    InstalledLocalBiblePack, LocalBibleInstallPhase, LocalBiblePackStatus,
    ScriptureTranslationOption, translations::is_bundled_translation,
};
use kairo_domain::scripture_trace::{
    ScriptureLatencyMetrics, ScriptureResolver, ScriptureTraceRecord, TraceStatus,
};
use kairo_domain::settings::{ShortcutBindings, ShortcutCommand};
use kairo_domain::status::{AudioDevice, AudioLevel, UpdateStatus};
use kairo_domain::workspace::{MediaFolderMigration, WorkspaceSongSync};
use kairo_ui_kit::KairoIcon;

// ── Sections and navigation ─────────────────────────────────────────────

/// A settings pane (`SettingsSectionId`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    /// Theme, fonts, lyrics colour and storage.
    General,
    /// Verses on screen.
    Overlay,
    /// Detection and display.
    Scripture,
    /// Input device and levels.
    Audio,
    /// Licence and devices.
    ApiKeys,
    /// Keyboard shortcuts.
    Shortcuts,
    /// Account.
    Account,
}

impl Section {
    /// The id Electron uses (`general`, `apikeys`, …).
    pub fn id(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::Overlay => "overlay",
            Section::Scripture => "scripture",
            Section::Audio => "audio",
            Section::ApiKeys => "apikeys",
            Section::Shortcuts => "shortcuts",
            Section::Account => "account",
        }
    }

    /// The pane for an Electron id; `None` for an unknown one.
    pub fn from_id(id: &str) -> Option<Section> {
        SECTIONS.into_iter().find(|s| s.id() == id)
    }

    /// The sidebar label and pane title.
    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Overlay => "Verses on screen",
            Section::Scripture => "Scripture",
            Section::Audio => "Audio",
            Section::ApiKeys => "Licence & Devices",
            Section::Shortcuts => "Keyboard Shortcuts",
            Section::Account => "Account",
        }
    }

    /// The hint searched with the label.
    pub fn hint(self) -> &'static str {
        match self {
            Section::General => "Theme, fonts, lyrics color & storage",
            Section::Overlay => "Translation & verse numbers",
            Section::Scripture => "Detection & display",
            Section::Audio => "Input device & levels",
            Section::ApiKeys => "Activation and managed cloud services",
            Section::Shortcuts => "Commands & key bindings",
            Section::Account => "Sign in, team sync & setup",
        }
    }

    /// The sidebar glyph.
    pub fn icon(self) -> KairoIcon {
        match self {
            Section::General | Section::Shortcuts => KairoIcon::SlidersHorizontal,
            Section::Overlay => KairoIcon::MonitorPlay,
            Section::Scripture => KairoIcon::BookOpen,
            Section::Audio => KairoIcon::Mic,
            Section::ApiKeys => KairoIcon::Key,
            Section::Account => KairoIcon::CircleUser,
        }
    }
}

/// `SECTION_NAV`, in sidebar order.
pub const SECTIONS: [Section; 7] = [
    Section::General,
    Section::Overlay,
    Section::Scripture,
    Section::Audio,
    Section::ApiKeys,
    Section::Shortcuts,
    Section::Account,
];

/// The sidebar items for a search. Account sits in the card above the list,
/// so it is listed only while searching.
pub fn nav_items(query: &str) -> Vec<Section> {
    let query = query.trim().to_lowercase();
    SECTIONS
        .into_iter()
        .filter(|s| {
            if query.is_empty() {
                return *s != Section::Account;
            }
            format!("{} {}", s.label(), s.hint())
                .to_lowercase()
                .contains(&query)
        })
        .collect()
}

/// Back and forward through the panes opened, as System Settings does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    items: Vec<Section>,
    index: usize,
}

impl History {
    /// History starting at `section`.
    pub fn new(section: Section) -> Self {
        Self {
            items: vec![section],
            index: 0,
        }
    }

    /// The pane showing.
    pub fn current(&self) -> Section {
        self.items[self.index]
    }

    /// Opens `section`, dropping anything ahead of the current pane.
    pub fn select(&mut self, section: Section) {
        if self.current() == section {
            return;
        }
        self.items.truncate(self.index + 1);
        self.items.push(section);
        self.index = self.items.len() - 1;
    }

    /// There is a pane behind.
    pub fn can_back(&self) -> bool {
        self.index > 0
    }

    /// There is a pane ahead.
    pub fn can_forward(&self) -> bool {
        self.index + 1 < self.items.len()
    }

    /// Steps back (`-1`) or forward (`1`) when it can.
    pub fn go(&mut self, delta: i8) {
        if delta < 0 && self.can_back() {
            self.index -= 1;
        } else if delta > 0 && self.can_forward() {
            self.index += 1;
        }
    }
}

// ── Number fields ───────────────────────────────────────────────────────

/// JavaScript's `parseInt(text)`: leading whitespace, an optional sign and
/// the digits that follow; `None` where it gives `NaN`.
pub fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim_start();
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'-') => (-1, &text[1..]),
        Some(b'+') => (1, &text[1..]),
        _ => (1, text),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok().map(|n| n * sign)
}

/// `Math.max(min, parseInt(text) || fallback)`: 0 and `NaN` both take the
/// fallback, as `||` does.
pub fn int_field(text: &str, min: i64, fallback: i64) -> f64 {
    let value = parse_int(text).filter(|n| *n != 0).unwrap_or(fallback);
    value.max(min) as f64
}

/// Debounce seconds.
pub fn debounce_field(text: &str) -> f64 {
    int_field(text, 1, 8)
}

/// Context window seconds.
pub fn context_window_field(text: &str) -> f64 {
    int_field(text, 10, 90)
}

/// Max verses: 0 sends the whole passage.
pub fn max_verses_field(text: &str) -> f64 {
    int_field(text, 0, 0)
}

/// Shows a stored number as the field's text.
pub fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

// ── Gloss colour ────────────────────────────────────────────────────────

/// The gloss colour when nothing valid is stored.
pub const DEFAULT_GLOSS: &str = "#AABED7";

/// `/^#[0-9a-fA-F]{6}$/`.
pub fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

/// The colour the swatch and preview draw: the stored value while valid.
pub fn gloss_or_default(value: &str) -> &str {
    if is_hex_color(value) {
        value
    } else {
        DEFAULT_GLOSS
    }
}

/// `#RRGGBB` as a colour.
pub fn hex_to_rgb(value: &str) -> Option<u32> {
    is_hex_color(value)
        .then(|| u32::from_str_radix(&value[1..], 16).ok())
        .flatten()
}

// ── Level meter ─────────────────────────────────────────────────────────

/// Segments in the input meter.
pub const METER_SEGMENTS: usize = 18;

/// One meter segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    /// Unlit.
    Off,
    /// Lit, in range.
    Ok,
    /// Lit, getting loud.
    Warn,
    /// Lit, clipping.
    Clip,
}

/// The meter's segments: lit up to the RMS, amber from 14, red from 17.
pub fn meter_segments(level: Option<&AudioLevel>, active: bool) -> [Segment; METER_SEGMENTS] {
    let rms = if active {
        level.map_or(0.0, |l| l.rms.min(1.0))
    } else {
        0.0
    };
    std::array::from_fn(|i| {
        let lit = rms >= (i + 1) as f64 / METER_SEGMENTS as f64;
        if !lit {
            Segment::Off
        } else if i >= 16 {
            Segment::Clip
        } else if i >= 13 {
            Segment::Warn
        } else {
            Segment::Ok
        }
    })
}

/// `aria-valuenow`.
pub fn meter_percent(level: Option<&AudioLevel>, active: bool) -> u32 {
    if !active {
        return 0;
    }
    (level.map_or(0.0, |l| l.rms.min(1.0)) * 100.0).round() as u32
}

/// `resolveCaptureDeviceId`: the saved id while that input is present,
/// else the system default (the first device), else `"default"`.
pub fn resolve_input(devices: &[AudioDevice], saved: &str) -> String {
    if !saved.is_empty() && devices.iter().any(|d| d.id == saved) {
        return saved.to_owned();
    }
    devices
        .iter()
        .find(|d| d.is_default)
        .or(devices.first())
        .map_or_else(|| "default".to_owned(), |d| d.id.clone())
}

/// The output picker's choices: System Default, a saved label that is not
/// connected (marked unavailable), then each connected output.
pub fn output_choices(saved: &str, outputs: &[String]) -> Vec<(String, String)> {
    let mut choices = vec![(String::new(), "System Default".to_owned())];
    if !saved.is_empty() && !outputs.iter().any(|o| o == saved) {
        choices.push((saved.to_owned(), format!("{saved} (unavailable)")));
    }
    choices.extend(outputs.iter().map(|o| (o.clone(), o.clone())));
    choices
}

// ── Scripture ───────────────────────────────────────────────────────────

/// The default-translation picker: only Bibles that work now, with a saved
/// default that no longer reads kept (disabled) so the box is never blank.
pub fn translation_choices(
    translations: &[ScriptureTranslationOption],
    saved: &str,
) -> (
    Option<ScriptureTranslationOption>,
    Vec<ScriptureTranslationOption>,
) {
    let ready: Vec<_> = translations
        .iter()
        .filter(|t| t.available)
        .cloned()
        .collect();
    let missing = if ready.iter().any(|t| t.id == saved) {
        None
    } else {
        translations.iter().find(|t| t.id == saved).cloned()
    };
    (missing, ready)
}

/// Picking `id` needs a download first.
pub fn needs_download(translations: &[ScriptureTranslationOption], id: &str) -> bool {
    translations
        .iter()
        .find(|t| t.id == id)
        .is_some_and(|t| t.downloadable == Some(true) && !t.available)
}

/// A Bible library row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackRow {
    /// The translation id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The registry offers it for download.
    pub downloadable: bool,
    /// `downloadApprox`.
    pub size: Option<String>,
}

/// What the library lists.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Library {
    /// Built in, available offline.
    pub bundled: Vec<(String, String)>,
    /// Downloadable packs, then every other installed pack.
    pub packs: Vec<PackRow>,
    /// Online through API.Bible.
    pub online: Vec<(String, String)>,
}

impl Library {
    /// Downloadable packs not yet installed (`Download all`).
    pub fn pending(&self, installed: &[InstalledLocalBiblePack]) -> Vec<String> {
        self.packs
            .iter()
            .filter(|p| {
                p.downloadable
                    && !installed
                        .iter()
                        .any(|i| i.status.translation.eq_ignore_ascii_case(&p.id))
            })
            .map(|p| p.id.clone())
            .collect()
    }
}

/// `LocalBiblePackManager`'s rows. Before translations load, the registry
/// default (NKJV) keeps the panel useful.
pub fn library(
    translations: &[ScriptureTranslationOption],
    installed: &[InstalledLocalBiblePack],
) -> Library {
    let bundled: Vec<_> = translations
        .iter()
        .filter(|t| is_bundled_translation(&t.id) && t.available)
        .map(|t| (t.id.clone(), t.name.clone()))
        .collect();
    let downloadable: Vec<_> = translations
        .iter()
        .filter(|t| t.downloadable == Some(true))
        .collect();
    let mut packs: Vec<PackRow> = if downloadable.is_empty() {
        vec![PackRow {
            id: "NKJV".into(),
            name: "New King James Version".into(),
            downloadable: true,
            size: None,
        }]
    } else {
        downloadable
            .iter()
            .map(|t| PackRow {
                id: t.id.clone(),
                name: t.name.clone(),
                downloadable: true,
                size: t.download_approx.clone(),
            })
            .collect()
    };
    for pack in installed {
        let id = &pack.status.translation;
        if packs.iter().any(|p| p.id.eq_ignore_ascii_case(id))
            || bundled.iter().any(|(b, _)| b.eq_ignore_ascii_case(id))
        {
            continue;
        }
        packs.push(PackRow {
            id: id.clone(),
            name: pack.name.clone(),
            downloadable: false,
            size: None,
        });
    }
    let online = translations
        .iter()
        .filter(|t| {
            t.available
                && t.requires_api_key
                && t.downloadable != Some(true)
                && !is_bundled_translation(&t.id)
        })
        .map(|t| (t.id.clone(), t.name.clone()))
        .collect();
    Library {
        bundled,
        packs,
        online,
    }
}

/// A pack row's live state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackState {
    /// The last status read or pushed.
    pub status: Option<LocalBiblePackStatus>,
    /// The first status has not arrived.
    pub loading: bool,
    /// A download or removal is running.
    pub busy: Option<PackBusy>,
    /// The last call's error.
    pub error: Option<String>,
}

/// What a pack row is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackBusy {
    /// Downloading.
    Download,
    /// Removing.
    Remove,
}

fn revision(status: Option<&LocalBiblePackStatus>) -> f64 {
    status
        .and_then(|s| s.installation.as_ref())
        .map_or(0.0, |i| i.revision)
}

impl PackState {
    /// Takes a read status unless a newer install revision is already shown.
    pub fn accept(&mut self, next: LocalBiblePackStatus) {
        if revision(self.status.as_ref()) > revision(Some(&next)) {
            return;
        }
        self.status = Some(next);
    }

    /// Applies a pushed install progress for this pack.
    pub fn progress(&mut self, progress: kairo_domain::scripture::LocalBibleInstallProgress) {
        if revision(self.status.as_ref()) > progress.revision {
            return;
        }
        self.error = None;
        let previous = self.status.take();
        self.status = Some(LocalBiblePackStatus {
            translation: progress.translation.clone(),
            verse_count: previous.as_ref().map_or(0.0, |p| p.verse_count),
            chapter_count: previous.as_ref().map_or(0.0, |p| p.chapter_count),
            installed: progress.phase == LocalBibleInstallPhase::Ready
                || previous.as_ref().is_some_and(|p| p.installed),
            installation: Some(progress),
        });
    }

    fn phase(&self) -> Option<LocalBibleInstallPhase> {
        self.status
            .as_ref()
            .and_then(|s| s.installation.as_ref())
            .map(|i| i.phase)
    }

    /// The pack is installed.
    pub fn installed(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.installed)
    }

    /// A download, check or install is running.
    pub fn installing(&self) -> bool {
        use LocalBibleInstallPhase::*;
        match self.phase() {
            Some(Downloading | Verifying | Installing) => true,
            phase => {
                self.busy == Some(PackBusy::Download) && !matches!(phase, Some(Ready | Failed))
            }
        }
    }

    /// The failure shown, from the install or the last call.
    pub fn failure(&self) -> Option<String> {
        let install = (self.phase() == Some(LocalBibleInstallPhase::Failed))
            .then(|| {
                self.status
                    .as_ref()
                    .and_then(|s| s.installation.as_ref())
                    .and_then(|i| i.error.clone())
            })
            .flatten()
            .filter(|e| !e.is_empty());
        install.or_else(|| self.error.clone())
    }

    /// Percent received, when the total is known.
    pub fn percent(&self) -> Option<u32> {
        let i = self.status.as_ref()?.installation.as_ref()?;
        let total = i.total_bytes.filter(|t| *t > 0.0)?;
        Some(((i.received_bytes / total * 100.0).floor() as u32).min(100))
    }

    /// The row's detail line.
    pub fn detail(&self, downloadable: bool, size: Option<&str>) -> String {
        if let Some(failure) = self.failure() {
            return failure;
        }
        if self.installing() {
            let i = self.status.as_ref().and_then(|s| s.installation.as_ref());
            let received = i.map_or(0.0, |i| i.received_bytes);
            let total = i.and_then(|i| i.total_bytes).filter(|t| *t > 0.0);
            return match self.phase() {
                Some(LocalBibleInstallPhase::Verifying) => "Checking download…".into(),
                Some(LocalBibleInstallPhase::Installing) => "Finishing installation…".into(),
                _ if received > 0.0 => match total {
                    Some(total) => format!(
                        "Downloading · {} of {} · {}%",
                        mb(received),
                        mb(total),
                        self.percent().unwrap_or(0)
                    ),
                    None => format!("Downloading · {} received", mb(received)),
                },
                _ => "Connecting to download…".into(),
            };
        }
        if self.loading {
            "Checking…".into()
        } else if self.installed() {
            "Ready offline · No internet needed".into()
        } else if downloadable {
            match size {
                Some(size) => format!(
                    "{} · Download once to use offline",
                    size.strip_prefix("about ")
                        .map_or_else(|| size.to_owned(), |s| format!("~{s}"))
                ),
                None => "Download once to use offline".into(),
            }
        } else {
            "Not installed".into()
        }
    }

    /// A non-downloadable pack that is gone shows nothing.
    pub fn hidden(&self, downloadable: bool) -> bool {
        !self.loading && !self.installed() && !downloadable && self.failure().is_none()
    }
}

fn mb(bytes: f64) -> String {
    format!("{:.1} MB", bytes / 1_048_576.0)
}

// ── Verse timing ────────────────────────────────────────────────────────

/// The latency list's filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceFilter {
    /// All.
    All,
    /// Presented.
    Presented,
    /// Not shown: cancelled, failed or replaced.
    Issues,
}

impl TraceFilter {
    /// Its label.
    pub fn label(self) -> &'static str {
        match self {
            TraceFilter::All => "All",
            TraceFilter::Presented => "Presented",
            TraceFilter::Issues => "Not shown",
        }
    }

    /// The record passes.
    pub fn keeps(self, record: &ScriptureTraceRecord) -> bool {
        match self {
            TraceFilter::All => true,
            TraceFilter::Presented => record.trace.status == TraceStatus::Presented,
            TraceFilter::Issues => matches!(
                record.trace.status,
                TraceStatus::Cancelled | TraceStatus::Failed | TraceStatus::Superseded
            ),
        }
    }
}

/// `ms()`: `—`, whole milliseconds, or seconds to one place from 1 s.
pub fn ms(value: Option<f64>) -> String {
    match value {
        None => "—".into(),
        // `toFixed(1)` takes the larger of two equally near values; Rust's
        // formatter rounds a tie to even.
        Some(v) if v >= 1_000.0 => format!("{:.1}s", (v / 100.0).round() / 10.0),
        Some(v) => format!("{}ms", v.round() as i64),
    }
}

/// The value at percentile `p` (nearest rank, floor).
pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((p / 100.0) * sorted.len() as f64).floor() as usize;
    Some(sorted[index.min(sorted.len() - 1)])
}

/// What the operator waited for: excludes the safety delay.
pub fn headline(record: &ScriptureTraceRecord) -> Option<f64> {
    record
        .metrics
        .operator_visible_ms
        .or(record.metrics.total_to_presenter_request_ms)
}

/// How the verse was found.
pub fn resolver_label(resolver: ScriptureResolver) -> &'static str {
    match resolver {
        ScriptureResolver::Explicit => "Reference spoken",
        ScriptureResolver::QuotationLocal => "Verse quoted",
        ScriptureResolver::SermonPlan => "Sermon plan",
        ScriptureResolver::Ai => "AI match",
    }
}

/// A trace's status label.
pub fn status_label(status: TraceStatus) -> &'static str {
    match status {
        TraceStatus::Active => "In progress",
        TraceStatus::Presented => "Presented",
        TraceStatus::Cancelled => "Cancelled",
        TraceStatus::Superseded => "Replaced",
        TraceStatus::Failed => "Failed",
    }
}

/// A recorded step: label, value, and whether it is the deliberate delay.
pub fn stages(metrics: &ScriptureLatencyMetrics) -> Vec<(&'static str, f64, bool)> {
    [
        ("Detection", metrics.detection_ms, false),
        ("Bible lookup", metrics.bible_lookup_ms, false),
        ("Suggestion publish", metrics.suggestion_publish_ms, false),
        ("On-screen render", metrics.suggestion_render_ms, false),
        ("Safety delay", metrics.countdown_ms, true),
        ("Output request", metrics.presenter_request_ms, false),
        (
            "Output confirmation",
            metrics.presenter_confirmation_ms,
            false,
        ),
    ]
    .into_iter()
    .filter_map(|(label, value, deliberate)| value.map(|v| (label, v, deliberate)))
    .collect()
}

/// The three summary numbers: typical, slow and presented.
pub fn trace_summary(records: &[ScriptureTraceRecord]) -> (String, String, usize) {
    let times: Vec<f64> = records.iter().filter_map(headline).collect();
    let presented = records
        .iter()
        .filter(|r| r.trace.status == TraceStatus::Presented)
        .count();
    (
        ms(percentile(&times, 50.0)),
        ms(percentile(&times, 95.0)),
        presented,
    )
}

// ── General ─────────────────────────────────────────────────────────────

/// The software-update row's hint.
pub fn update_label(status: &UpdateStatus) -> String {
    match status {
        UpdateStatus::Checking => "Checking…".into(),
        UpdateStatus::Available { version, .. } => format!("Version {version} is available"),
        UpdateStatus::Downloading { percent, .. } => match percent {
            Some(p) => format!("Downloading — {}%", number_text(*p)),
            None => "Downloading…".into(),
        },
        UpdateStatus::Downloaded { version, .. } => {
            format!("Version {version} is ready — installs on the next quit")
        }
        UpdateStatus::Error { message } => message.clone(),
        UpdateStatus::Idle { current_version } => match current_version {
            Some(v) if !v.is_empty() => format!("Kairo {v} — up to date"),
            _ => "Up to date".into(),
        },
    }
}

/// `formatMB`.
pub fn format_mb(bytes: f64) -> String {
    format!("{} MB", (bytes / 1_048_576.0).round() as i64)
}

/// The converter row's hint; `None` before the first status.
pub fn converter_hint(status: Option<&SlideConverterStatus>) -> String {
    let Some(status) = status else {
        return "Checking…".into();
    };
    let other = match status.other_converter {
        Some(PowerpointConverterId::PowerPoint) => Some("Microsoft PowerPoint"),
        Some(PowerpointConverterId::Keynote) => Some("Keynote"),
        Some(PowerpointConverterId::LibreOffice) => Some("LibreOffice"),
        _ => None,
    };
    match status.state {
        SlideConverterState::Unsupported => "Install LibreOffice from your software center. Kairo uses it to import PowerPoint slides exactly.".into(),
        SlideConverterState::Ready => format!("Installed · version {}. PowerPoint slides keep every picture, font and effect.", status.version),
        SlideConverterState::Error => status.error.clone().unwrap_or_else(|| "The download didn’t finish.".into()),
        _ => match other {
            Some(other) => format!("{other} already converts your slides. Download this as a backup, or for when {other} isn’t available."),
            None => format!("A free, one-time download of about {} so PowerPoint slides keep every picture, font and effect.", format_mb(status.size_bytes)),
        },
    }
}

/// The converter is downloading or installing.
pub fn converter_busy(status: Option<&SlideConverterStatus>) -> bool {
    status.is_some_and(|s| {
        matches!(
            s.state,
            SlideConverterState::Downloading | SlideConverterState::Installing
        )
    })
}

/// The note after a Songs rescan.
pub fn resync_note(sync: &WorkspaceSongSync) -> String {
    let mut note = format!(
        "Songs folder read — {} in the library",
        number_text(sync.imported)
    );
    if sync.removed != 0.0 {
        note.push_str(&format!(", {} removed", number_text(sync.removed)));
    }
    if sync.exported != 0.0 {
        note.push_str(&format!(", {} written out", number_text(sync.exported)));
    }
    note.push('.');
    note
}

/// The media-migration row's hint.
pub fn migration_hint(migration: &MediaFolderMigration) -> String {
    if migration.file_count >= 0.0 {
        let n = migration.file_count;
        format!(
            "{} — {} file{}",
            migration.current_folder,
            number_text(n),
            if n == 1.0 { "" } else { "s" }
        )
    } else {
        migration.current_folder.clone()
    }
}

// ── Account ─────────────────────────────────────────────────────────────

/// `accountInitials`: two letters from the name, else the email, else none.
pub fn initials(name: Option<&str>, email: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = name.unwrap_or("").split_whitespace().collect();
    let first = |s: &str| s.chars().next().map(String::from).unwrap_or_default();
    match parts.as_slice() {
        [] => {}
        [only] => return Some(only.chars().take(2).collect::<String>().to_uppercase()),
        [a, .., b] => return Some(format!("{}{}", first(a), first(b)).to_uppercase()),
    }
    let local: String = email
        .and_then(|e| e.split('@').next())
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    (!local.is_empty()).then(|| local.chars().take(2).collect::<String>().to_uppercase())
}

/// `relativeTime`, for times within a day; older ones as a date.
pub fn relative_time(at_ms: f64, now_ms: f64) -> String {
    let minutes = ((now_ms - at_ms) / 60_000.0).round();
    if minutes < 1.0 {
        return "Just now".into();
    }
    if minutes < 60.0 {
        return format!("{minutes} min ago");
    }
    let hours = (minutes / 60.0).round();
    if hours < 24.0 {
        return format!("{hours} hr ago");
    }
    short_date(at_ms)
}

/// `toLocaleDateString(undefined, { month: 'short', day: 'numeric' })` in
/// English, from UTC days.
fn short_date(at_ms: f64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Civil-from-days (Howard Hinnant).
    let z = (at_ms / 86_400_000.0).floor() as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{} {day}", MONTHS[(month - 1) as usize])
}

// ── Shortcuts ───────────────────────────────────────────────────────────

/// Replaces `command`'s keys (`[]` clears it).
pub fn with_binding(
    bindings: &ShortcutBindings,
    command: ShortcutCommand,
    keys: Vec<String>,
) -> ShortcutBindings {
    let mut next = bindings.clone();
    let slot = match command {
        ShortcutCommand::Search => &mut next.search,
        ShortcutCommand::Previous => &mut next.previous,
        ShortcutCommand::Next => &mut next.next,
        ShortcutCommand::Approve => &mut next.approve,
        ShortcutCommand::Dismiss => &mut next.dismiss,
        ShortcutCommand::Clear => &mut next.clear,
        ShortcutCommand::Auto => &mut next.auto,
    };
    *slot = Some(keys);
    next
}

/// The label of a command.
pub fn command_label(command: ShortcutCommand) -> &'static str {
    SHORTCUT_COMMANDS
        .iter()
        .find(|c| c.id == command)
        .map_or("", |c| c.label)
}

/// Binds `shortcut` to `command`, or names the command already using it.
pub fn record_shortcut(
    draft: &ShortcutBindings,
    command: ShortcutCommand,
    shortcut: &str,
) -> Result<ShortcutBindings, String> {
    let without = with_binding(draft, command, Vec::new());
    if let Some(conflict) = command_for_shortcut(shortcut, &without) {
        return Err(format!(
            "Already assigned to {}. Clear that binding first.",
            command_label(conflict)
        ));
    }
    Ok(with_binding(draft, command, vec![shortcut.to_owned()]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::keyboard_shortcuts::bindings_for;
    use kairo_domain::scripture::TranslationAccess;

    fn option(id: &str, available: bool, downloadable: Option<bool>) -> ScriptureTranslationOption {
        ScriptureTranslationOption {
            id: id.into(),
            name: format!("{id} name"),
            access: TranslationAccess::Local,
            available,
            requires_api_key: false,
            downloadable,
            download_approx: downloadable.map(|_| "about 4 MB".into()),
        }
    }

    #[test]
    fn search_lists_account_only_while_searching() {
        assert!(!nav_items("").contains(&Section::Account));
        assert_eq!(nav_items("team sync"), vec![Section::Account]);
        assert_eq!(nav_items("  BINDINGS "), vec![Section::Shortcuts]);
        assert!(nav_items("zzz").is_empty());
    }

    #[test]
    fn history_drops_forward_on_a_new_pane() {
        let mut h = History::new(Section::General);
        h.select(Section::Audio);
        h.select(Section::Audio);
        h.select(Section::Scripture);
        h.go(-1);
        assert_eq!(h.current(), Section::Audio);
        assert!(h.can_forward());
        h.select(Section::Overlay);
        assert!(!h.can_forward());
        h.go(-1);
        h.go(-1);
        h.go(-1);
        assert_eq!(h.current(), Section::General);
        assert!(!h.can_back());
    }

    #[test]
    fn number_fields_follow_parse_int_or_fallback() {
        assert_eq!(debounce_field(""), 8.0);
        assert_eq!(debounce_field("0"), 8.0);
        assert_eq!(debounce_field("-4"), 1.0);
        assert_eq!(debounce_field("12abc"), 12.0);
        assert_eq!(context_window_field("5"), 10.0);
        assert_eq!(context_window_field("x"), 90.0);
        assert_eq!(max_verses_field(""), 0.0);
        assert_eq!(max_verses_field("3"), 3.0);
        assert_eq!(max_verses_field("-3"), 0.0);
    }

    #[test]
    fn gloss_colour_falls_back_when_invalid() {
        assert!(is_hex_color("#aabbcc"));
        assert!(!is_hex_color("#abc"));
        assert!(!is_hex_color(" #AABBCC"));
        assert_eq!(gloss_or_default("red"), DEFAULT_GLOSS);
        assert_eq!(hex_to_rgb("#102030"), Some(0x102030));
    }

    #[test]
    fn meter_lights_green_amber_red() {
        let level = |rms| AudioLevel {
            rms,
            peak: rms,
            clipping: false,
            timestamp: 0.0,
        };
        assert!(
            meter_segments(Some(&level(1.0)), false)
                .iter()
                .all(|s| *s == Segment::Off)
        );
        let full = meter_segments(Some(&level(2.0)), true);
        assert_eq!(full[12], Segment::Ok);
        assert_eq!(full[13], Segment::Warn);
        assert_eq!(full[16], Segment::Clip);
        let half = meter_segments(Some(&level(0.5)), true);
        assert_eq!(half.iter().filter(|s| **s != Segment::Off).count(), 9);
        assert_eq!(meter_percent(Some(&level(0.456)), true), 46);
        assert_eq!(meter_percent(None, true), 0);
    }

    #[test]
    fn sections_round_trip_their_ids() {
        for section in SECTIONS {
            assert_eq!(Section::from_id(section.id()), Some(section));
        }
        assert_eq!(Section::from_id("integrations"), None);
    }

    #[test]
    fn a_missing_input_falls_back_to_the_default() {
        use kairo_domain::status::AudioDeviceKind;
        let device = |id: &str, default| AudioDevice {
            id: id.into(),
            label: id.into(),
            kind: AudioDeviceKind::AudioInput,
            is_default: default,
        };
        let devices = vec![device("default", true), device("usb", false)];
        assert_eq!(resolve_input(&devices, "usb"), "usb");
        assert_eq!(resolve_input(&devices, "gone"), "default");
        assert_eq!(resolve_input(&devices, ""), "default");
        assert_eq!(resolve_input(&[], "usb"), "default");
    }

    #[test]
    fn missing_output_stays_listed() {
        let choices = output_choices("Old DAC", &["Speakers".into()]);
        assert_eq!(choices[1].1, "Old DAC (unavailable)");
        assert_eq!(output_choices("Speakers", &["Speakers".into()]).len(), 2);
    }

    #[test]
    fn saved_translation_that_cannot_read_is_kept() {
        let list = vec![option("KJV", true, None), option("NKJV", false, Some(true))];
        let (missing, ready) = translation_choices(&list, "NKJV");
        assert_eq!(missing.unwrap().id, "NKJV");
        assert_eq!(ready.len(), 1);
        assert!(needs_download(&list, "NKJV"));
        assert!(!needs_download(&list, "KJV"));
        assert!(translation_choices(&list, "XYZ").0.is_none());
    }

    #[test]
    fn library_falls_back_to_nkjv_and_lists_stray_packs() {
        let installed = vec![InstalledLocalBiblePack {
            name: "Stray".into(),
            status: LocalBiblePackStatus {
                translation: "ABC".into(),
                verse_count: 1.0,
                chapter_count: 1.0,
                installed: true,
                installation: None,
            },
        }];
        let lib = library(&[], &installed);
        assert_eq!(lib.packs[0].id, "NKJV");
        assert_eq!(lib.packs[1].id, "ABC");
        assert!(!lib.packs[1].downloadable);
        assert_eq!(lib.pending(&installed), vec!["NKJV".to_owned()]);
    }

    #[test]
    fn pack_detail_reads_each_phase() {
        let mut state = PackState {
            loading: true,
            ..Default::default()
        };
        assert_eq!(state.detail(true, None), "Checking…");
        state.loading = false;
        assert_eq!(
            state.detail(true, Some("about 4 MB")),
            "~4 MB · Download once to use offline"
        );
        assert!(state.hidden(false));
        let progress = |phase, received, total: Option<f64>, revision| {
            kairo_domain::scripture::LocalBibleInstallProgress {
                translation: "NKJV".into(),
                phase,
                received_bytes: received,
                total_bytes: total,
                revision,
                error: Some("disk full".into()),
            }
        };
        state.progress(progress(
            LocalBibleInstallPhase::Downloading,
            0.0,
            None,
            1.0,
        ));
        assert_eq!(state.detail(true, None), "Connecting to download…");
        state.progress(progress(
            LocalBibleInstallPhase::Downloading,
            1_048_576.0,
            Some(4_194_304.0),
            2.0,
        ));
        assert_eq!(
            state.detail(true, None),
            "Downloading · 1.0 MB of 4.0 MB · 25%"
        );
        // An older revision is ignored.
        state.progress(progress(LocalBibleInstallPhase::Verifying, 0.0, None, 1.0));
        assert!(state.detail(true, None).starts_with("Downloading"));
        state.progress(progress(LocalBibleInstallPhase::Failed, 0.0, None, 3.0));
        assert_eq!(state.detail(true, None), "disk full");
        assert!(!state.installing());
    }

    #[test]
    fn timings_format_and_summarise() {
        assert_eq!(ms(None), "—");
        assert_eq!(ms(Some(412.4)), "412ms");
        assert_eq!(ms(Some(1_250.0)), "1.3s");
        assert_eq!(percentile(&[5.0, 1.0, 3.0, 2.0], 50.0), Some(3.0));
        assert_eq!(percentile(&[5.0, 1.0, 3.0, 2.0], 95.0), Some(5.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn update_and_converter_text() {
        assert_eq!(
            update_label(&UpdateStatus::Idle {
                current_version: Some("1.2.0".into())
            }),
            "Kairo 1.2.0 — up to date"
        );
        assert_eq!(
            update_label(&UpdateStatus::Downloading {
                version: None,
                notes: None,
                percent: Some(42.0)
            }),
            "Downloading — 42%"
        );
        assert_eq!(converter_hint(None), "Checking…");
        let status = SlideConverterStatus {
            state: SlideConverterState::Missing,
            version: "24".into(),
            size_bytes: 150.0 * 1_048_576.0,
            received_bytes: 0.0,
            error: None,
            other_converter: Some(PowerpointConverterId::Keynote),
        };
        assert!(converter_hint(Some(&status)).starts_with("Keynote already converts"));
        let error = SlideConverterStatus {
            state: SlideConverterState::Error,
            other_converter: None,
            ..status
        };
        assert_eq!(converter_hint(Some(&error)), "The download didn’t finish.");
    }

    #[test]
    fn workspace_notes() {
        let sync = WorkspaceSongSync {
            imported: 12.0,
            removed: 0.0,
            exported: 2.0,
        };
        assert_eq!(
            resync_note(&sync),
            "Songs folder read — 12 in the library, 2 written out."
        );
        let m = MediaFolderMigration {
            current_folder: "/old".into(),
            target_folder: "/new".into(),
            needed: true,
            file_count: 1.0,
        };
        assert_eq!(migration_hint(&m), "/old — 1 file");
        let unknown = MediaFolderMigration {
            file_count: -1.0,
            ..m
        };
        assert_eq!(migration_hint(&unknown), "/old");
    }

    #[test]
    fn initials_and_times() {
        assert_eq!(
            initials(Some("Ada  Byron King"), None).as_deref(),
            Some("AK")
        );
        assert_eq!(initials(Some("ada"), None).as_deref(), Some("AD"));
        assert_eq!(initials(None, Some("j.o@x.org")).as_deref(), Some("JO"));
        assert_eq!(initials(Some(" "), Some("@x")), None);
        assert_eq!(relative_time(0.0, 20_000.0), "Just now");
        assert_eq!(relative_time(0.0, 5.0 * 60_000.0), "5 min ago");
        assert_eq!(relative_time(0.0, 3.0 * 3_600_000.0), "3 hr ago");
        // 2026-11-14 00:00 UTC.
        assert_eq!(
            relative_time(1_794_614_400_000.0, 1_794_900_000_000.0),
            "Nov 14"
        );
    }

    #[test]
    fn shortcut_conflicts_name_the_other_command() {
        let draft = ShortcutBindings::default();
        let err = record_shortcut(&draft, ShortcutCommand::Search, "Escape").unwrap_err();
        assert_eq!(
            err,
            "Already assigned to Dismiss top suggestion. Clear that binding first."
        );
        // Rebinding a command to one of its own keys is not a conflict.
        let next = record_shortcut(&draft, ShortcutCommand::Next, "ArrowRight").unwrap();
        assert_eq!(
            bindings_for(ShortcutCommand::Next, &next),
            vec!["ArrowRight"]
        );
        // A cleared binding frees its key.
        let cleared = with_binding(&draft, ShortcutCommand::Dismiss, Vec::new());
        assert!(record_shortcut(&cleared, ShortcutCommand::Search, "Escape").is_ok());
    }
}
