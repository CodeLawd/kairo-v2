//! Engage's rules apart from drawing, from the components in
//! `src/renderer/src/components/audience/`.

use base64::Engine as _;
use kairo_domain::audience::{
    AudienceNames, AudienceQuestionState, AudienceRoomSettings, AudienceSourceState,
    AudienceSourceStatus, AudienceSourceStep, AudienceState, AudienceStatusKind, EngagementIndex,
    EngagementStatus, PollView,
};

/// `CACHE_KEY` in `Engagements.tsx`.
pub const CACHE_KEY: &str = "kairo:engagements-cache:v1";
/// `DEFAULT_TITLE` in `NewEngagementModal.tsx`.
pub const DEFAULT_TITLE: &str = "Engagement Title";
/// How long "Link copied" stays.
pub const COPIED_MS: u64 = 1500;
/// When a slow start gets its note.
pub const SLOW_MS: u64 = 8000;
pub const MAX_POLL_OPTIONS: usize = 12;

/// The question tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestionTab {
    Approved,
    Answered,
    Hidden,
}

impl QuestionTab {
    pub const ALL: [QuestionTab; 3] = [
        QuestionTab::Approved,
        QuestionTab::Answered,
        QuestionTab::Hidden,
    ];
    pub fn key(self) -> &'static str {
        match self {
            QuestionTab::Approved => "approved",
            QuestionTab::Answered => "answered",
            QuestionTab::Hidden => "hidden",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            QuestionTab::Approved => "Open",
            QuestionTab::Answered => "Answered",
            QuestionTab::Hidden => "Hidden",
        }
    }
    pub fn from_key(key: &str) -> Self {
        match key {
            "answered" => QuestionTab::Answered,
            "hidden" => QuestionTab::Hidden,
            _ => QuestionTab::Approved,
        }
    }
    /// `EMPTY_COPY`.
    pub fn empty(self) -> (&'static str, &'static str) {
        match self {
            QuestionTab::Approved => (
                "No open questions",
                "Questions from the room appear here ready to show on screen.",
            ),
            QuestionTab::Answered => (
                "Nothing answered yet",
                "Mark a question answered and it moves here.",
            ),
            QuestionTab::Hidden => ("Nothing hidden", "Hidden questions stay out of view."),
        }
    }
    /// `matchesTab`: Open holds questions waiting for approval too.
    pub fn matches(self, state: AudienceQuestionState) -> bool {
        match self {
            QuestionTab::Approved => {
                matches!(
                    state,
                    AudienceQuestionState::Approved | AudienceQuestionState::Review
                )
            }
            QuestionTab::Answered => state == AudienceQuestionState::Answered,
            QuestionTab::Hidden => state == AudienceQuestionState::Hidden,
        }
    }
}

/// `age`: "now", "4m", "2h", "3d".
pub fn age(created_at: f64, now: f64) -> String {
    let minutes = ((now - created_at) / 60_000.0).floor();
    if minutes < 1.0 {
        return "now".into();
    }
    if minutes < 60.0 {
        return format!("{minutes}m");
    }
    let hours = (minutes / 60.0).floor();
    if hours < 24.0 {
        format!("{hours}h")
    } else {
        format!("{}d", (hours / 24.0).floor())
    }
}

/// `updated` on the Slido card.
pub fn updated(at: Option<f64>, now: f64) -> String {
    let Some(at) = at.filter(|a| *a > 0.0) else {
        return "no questions yet".into();
    };
    let seconds = ((now - at) / 1000.0).floor().max(0.0);
    if seconds < 15.0 {
        "updated just now".into()
    } else if seconds < 60.0 {
        format!("updated {seconds}s ago")
    } else {
        format!("updated {} min ago", (seconds / 60.0).floor())
    }
}

/// `settingsSummary`.
pub fn settings_summary(s: &AudienceRoomSettings) -> String {
    format!(
        "{} · {} · {}.",
        if s.review {
            "You approve questions"
        } else {
            "Open to everyone"
        },
        if s.show_questions {
            "People can vote"
        } else {
            "People only ask"
        },
        if s.names == AudienceNames::Optional {
            "Names optional"
        } else {
            "Anonymous"
        },
    )
}

/// One row of `RoomSettings`: label, hint, on.
pub fn room_rows(
    s: &AudienceRoomSettings,
) -> [(&'static str, &'static str, &'static str, bool); 3] {
    [
        (
            "review",
            "Approve questions first",
            if s.review {
                "You see them before anyone else."
            } else {
                "Questions appear for everyone right away."
            },
            s.review,
        ),
        (
            "list",
            "Show questions to everyone",
            if s.show_questions {
                "People can read and vote."
            } else {
                "People can only ask."
            },
            s.show_questions,
        ),
        (
            "names",
            "Ask for names",
            if s.names == AudienceNames::Optional {
                "Optional. Anonymous is fine."
            } else {
                "Every question is anonymous."
            },
            s.names == AudienceNames::Optional,
        ),
    ]
}

/// A room setting switched by its row key.
pub fn set_room(s: &AudienceRoomSettings, key: &str, on: bool) -> AudienceRoomSettings {
    let mut next = s.clone();
    match key {
        "review" => next.review = on,
        "list" => next.show_questions = on,
        _ => {
            next.names = if on {
                AudienceNames::Optional
            } else {
                AudienceNames::Off
            }
        }
    }
    next
}

/// Which source the middle of the page shows (`activeSourceId`): the chosen
/// running one, else the newest running one.
pub fn active_source(state: &AudienceState, chosen: Option<&str>) -> Option<String> {
    let running: Vec<&AudienceSourceStatus> = state
        .sources
        .iter()
        .filter(|s| s.state != AudienceSourceState::Stopped)
        .collect();
    chosen
        .filter(|c| running.iter().any(|s| s.id == *c))
        .map(str::to_owned)
        .or_else(|| running.last().map(|s| s.id.clone()))
}

/// Polls are a Kairo engagement's: shown when Kairo or nothing is chosen.
pub fn polls_available(active: Option<&str>) -> bool {
    active.is_none_or(|a| a == "kairo")
}

/// `loadingQueue`.
pub fn loading_queue(state: &AudienceState, active: Option<&str>, initial: bool) -> bool {
    if initial {
        return true;
    }
    let Some(id) = active else { return false };
    let connecting = state
        .sources
        .iter()
        .any(|s| s.id == id && s.state == AudienceSourceState::Connecting);
    connecting && !state.questions.iter().any(|q| q.source_id == id)
}

/// `loadingPolls`.
pub fn loading_polls(state: &AudienceState, initial: bool) -> bool {
    initial
        || (state
            .sources
            .iter()
            .any(|s| s.id == "kairo" && s.state == AudienceSourceState::Connecting)
            && state.engage.is_none())
}

/// The Kairo card's status word.
pub fn kairo_status(state: &AudienceState, source: &AudienceSourceStatus) -> &'static str {
    let closed = state
        .engage
        .as_ref()
        .is_some_and(|e| e.engagement.status == EngagementStatus::Closed);
    if closed {
        "Closed"
    } else if source.state == AudienceSourceState::Live {
        "Live"
    } else if first_connect(source) {
        "Opening…"
    } else if source.state == AudienceSourceState::Error {
        "Connection problem"
    } else {
        "Reconnecting…"
    }
}

pub fn first_connect(source: &AudienceSourceStatus) -> bool {
    source.state == AudienceSourceState::Connecting && source.error.is_none()
}

/// The Slido card: sign-in step showing.
pub fn slido_sign_in(source: &AudienceSourceStatus) -> bool {
    source.state == AudienceSourceState::Connecting
        && matches!(
            source.step,
            Some(AudienceSourceStep::SignIn | AudienceSourceStep::OpenEvent)
        )
}

/// The Slido card's status word and detail line.
pub fn slido_status(
    source: &AudienceSourceStatus,
    questions: usize,
    now: f64,
) -> (&'static str, String) {
    let sign_in = slido_sign_in(source);
    let label = match source.state {
        AudienceSourceState::Live => "Live",
        AudienceSourceState::Error => "Connection problem",
        _ if sign_in => "Sign in",
        _ => "Connecting…",
    };
    let detail = if source.state == AudienceSourceState::Error {
        source
            .error
            .clone()
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "Not getting questions".into())
    } else if sign_in {
        if source.step == Some(AudienceSourceStep::SignIn) {
            "Sign in to Slido in the window that opened.".into()
        } else {
            "In that window, open your event\u{2019}s Q&A.".into()
        }
    } else if source.state == AudienceSourceState::Connecting {
        match &source.event_name {
            Some(name) => format!("Found {name} · reading questions…"),
            None => "Finding the event…".into(),
        }
    } else {
        format!(
            "{questions} {} · {}",
            if questions == 1 {
                "question"
            } else {
                "questions"
            },
            updated(source.updated_at, now)
        )
    };
    (label, detail)
}

/// `#code` or the event id.
pub fn slido_code(source: &AudienceSourceStatus) -> String {
    match (&source.code, &source.event_id) {
        (Some(c), _) => format!("#{c}"),
        (None, Some(e)) => e.clone(),
        _ => "Slido".into(),
    }
}

/// The link "Copy Slido link" copies.
pub fn slido_link(source: &AudienceSourceStatus) -> String {
    match &source.code {
        Some(c) => format!("https://app.sli.do/?search={c}"),
        None => format!(
            "https://app.sli.do/event/{}",
            source.event_id.clone().unwrap_or_default()
        ),
    }
}

/// What a Slido restart reconnects with.
pub fn slido_restart_code(source: &AudienceSourceStatus) -> String {
    source.code.clone().unwrap_or_else(|| {
        format!(
            "https://app.sli.do/event/{}",
            source.event_id.clone().unwrap_or_default()
        )
    })
}

pub fn is_slido(s: &AudienceSourceStatus) -> bool {
    s.kind == AudienceStatusKind::Slido
}

/// `STATUS` in `Engagements.tsx`.
pub fn engagement_status(status: EngagementStatus) -> &'static str {
    match status {
        EngagementStatus::Open => "Open",
        EngagementStatus::Draft => "Not started",
        EngagementStatus::Closed => "Closed",
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `toLocaleDateString(undefined, { day: 'numeric', month: 'short' })` in
/// the system's zone, as "12 Mar".
pub fn day(time_ms: f64) -> String {
    use chrono::{Datelike, TimeZone};
    match chrono::Local.timestamp_millis_opt(time_ms as i64).single() {
        Some(d) => format!("{} {}", d.day(), MONTHS[d.month0() as usize]),
        None => String::new(),
    }
}

/// `dates`: one day, a range, or nothing.
pub fn dates(item: &EngagementIndex) -> Option<String> {
    match (item.starts_at, item.ends_at) {
        (Some(s), Some(e)) if day(s) != day(e) => Some(format!("{} – {}", day(s), day(e))),
        (Some(s), _) => Some(day(s)),
        _ => None,
    }
}

/// `dayToTime`: a `yyyy-mm-dd` day at local midnight, or its last second.
pub fn day_to_time(value: &str, end_of_day: bool) -> Option<f64> {
    use chrono::{NaiveDate, TimeZone};
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    let time = if end_of_day {
        date.and_hms_opt(23, 59, 59)?
    } else {
        date.and_hms_opt(0, 0, 0)?
    };
    chrono::Local
        .from_local_datetime(&time)
        .earliest()
        .map(|t| t.timestamp_millis() as f64)
}

/// The end is before the start.
pub fn dates_invalid(starts: Option<f64>, ends: Option<f64>) -> bool {
    matches!((starts, ends), (Some(s), Some(e)) if e < s)
}

/// The error line under the engagements list: a sign-in failure is said plainly.
pub fn list_error(error: &str) -> String {
    if error.to_ascii_lowercase().contains("sign in") {
        "Could not load engagements. Try again.".into()
    } else {
        error.into()
    }
}

pub const OFFLINE: &str =
    "Kairo is offline. Your engagements will load when the connection returns.";

/// The cached list (`readCache`): anything but an array is ignored.
pub fn read_cache(raw: Option<&str>) -> Option<Vec<EngagementIndex>> {
    let value: serde_json::Value = serde_json::from_str(raw?).ok()?;
    if !value.is_array() {
        return None;
    }
    serde_json::from_value(value).ok()
}

pub fn write_cache(items: Option<&[EngagementIndex]>) -> Option<String> {
    items.map(|i| serde_json::to_string(i).unwrap_or_else(|_| "[]".into()))
}

/// A poll option's count and whole percentage.
pub fn poll_share(poll: &PollView, option: &str) -> (f64, f64) {
    let count = poll.totals.get(option).copied().unwrap_or(0.0);
    let pct = if poll.voters > 0.0 {
        (100.0 * count / poll.voters).round()
    } else {
        0.0
    };
    (count, pct)
}

/// The most votes any option has.
pub fn poll_most(poll: &PollView) -> f64 {
    poll.poll
        .options
        .iter()
        .map(|o| poll.totals.get(&o.id).copied().unwrap_or(0.0))
        .fold(0.0, f64::max)
}

/// `STATE` in `Polls.tsx`.
pub fn poll_state(state: EngagementStatus) -> &'static str {
    match state {
        EngagementStatus::Draft => "Not open yet",
        EngagementStatus::Open => "Open for votes",
        EngagementStatus::Closed => "Closed",
    }
}

/// `n vote(s)`, `n poll(s)`, `n question(s)`.
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The PNG inside a `data:image/png;base64,` address.
pub fn qr_png(data_url: &str) -> Option<Vec<u8>> {
    let (_, b64) = data_url.split_once(";base64,")?;
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()
}

/// `kairo-<code>-qr.png`.
pub fn qr_file_name(code: &str) -> String {
    format!("kairo-{}-qr.png", code.to_lowercase())
}

/// The join link without its scheme.
pub fn bare_url(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}

/// The first letter shown in a round badge.
pub fn initial(name: Option<&str>) -> String {
    name.map(str::trim)
        .and_then(|n| n.chars().next())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

/// `THEME_PRESETS`: id, label, text, card, opacity.
pub const THEME_PRESETS: [(&str, &str, &str, &str, f64); 6] = [
    ("slate", "Slate", "#ffffff", "#18212d", 95.0),
    ("midnight", "Midnight", "#ffffff", "#0c0c0e", 92.0),
    ("stage-blue", "Stage Blue", "#ffffff", "#17377a", 95.0),
    ("emerald", "Emerald", "#f0fdf4", "#0c3b2e", 95.0),
    ("warm", "Warm", "#fef3c7", "#3b2110", 95.0),
    ("light", "Light", "#111827", "#f8fafc", 98.0),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn status(id: &str, state: AudienceSourceState) -> AudienceSourceStatus {
        AudienceSourceStatus {
            id: id.into(),
            kind: if id.starts_with("slido") {
                AudienceStatusKind::Slido
            } else {
                AudienceStatusKind::Kairo
            },
            label: id.into(),
            state,
            step: None,
            error: None,
            joined: None,
            can_open: None,
            code: None,
            event_id: None,
            event_name: None,
            moderator: None,
            skip_review: None,
            updated_at: None,
        }
    }

    fn empty() -> AudienceState {
        AudienceState {
            engage: None,
            live_poll_id: None,
            sample_available: false,
            questions: vec![],
            sources: vec![],
            live_id: None,
            session: None,
        }
    }

    #[test]
    fn ages_and_updates() {
        assert_eq!(age(0.0, 30_000.0), "now");
        assert_eq!(age(0.0, 4.0 * 60_000.0), "4m");
        assert_eq!(age(0.0, 2.0 * 3_600_000.0), "2h");
        assert_eq!(age(0.0, 3.0 * 86_400_000.0), "3d");
        assert_eq!(updated(None, 0.0), "no questions yet");
        assert_eq!(updated(Some(1000.0), 5000.0), "updated just now");
        assert_eq!(updated(Some(1000.0), 31_000.0), "updated 30s ago");
        assert_eq!(updated(Some(1000.0), 181_000.0), "updated 3 min ago");
    }

    #[test]
    fn tabs_room_settings_and_summary() {
        assert!(QuestionTab::Approved.matches(AudienceQuestionState::Review));
        assert!(!QuestionTab::Answered.matches(AudienceQuestionState::Approved));
        let s = AudienceRoomSettings::default();
        assert_eq!(
            settings_summary(&s),
            "Open to everyone · People can vote · Names optional."
        );
        let s = set_room(&s, "review", true);
        let s = set_room(&s, "names", false);
        assert_eq!(
            settings_summary(&s),
            "You approve questions · People can vote · Anonymous."
        );
        assert_eq!(room_rows(&s)[2].2, "Every question is anonymous.");
    }

    #[test]
    fn the_active_source_is_the_chosen_running_one_or_the_newest() {
        let mut state = empty();
        assert_eq!(active_source(&state, None), None);
        assert!(polls_available(None));
        state.sources = vec![
            status("kairo", AudienceSourceState::Live),
            status("slido-1", AudienceSourceState::Connecting),
            status("slido-2", AudienceSourceState::Stopped),
        ];
        assert_eq!(active_source(&state, None).as_deref(), Some("slido-1"));
        assert_eq!(
            active_source(&state, Some("kairo")).as_deref(),
            Some("kairo")
        );
        assert_eq!(
            active_source(&state, Some("slido-2")).as_deref(),
            Some("slido-1"),
            "a stopped one is not chosen"
        );
        assert!(!polls_available(Some("slido-1")));
        assert!(loading_queue(&state, Some("slido-1"), false));
        assert!(!loading_queue(&state, Some("kairo"), false));
        assert!(loading_queue(&state, None, true));
    }

    #[test]
    fn slido_and_kairo_status_lines() {
        let mut s = status("slido-1", AudienceSourceState::Connecting);
        assert_eq!(
            slido_status(&s, 0, 0.0),
            ("Connecting…", "Finding the event…".into())
        );
        s.event_name = Some("Sunday".into());
        assert_eq!(
            slido_status(&s, 0, 0.0).1,
            "Found Sunday · reading questions…"
        );
        s.step = Some(AudienceSourceStep::SignIn);
        assert_eq!(
            slido_status(&s, 0, 0.0),
            (
                "Sign in",
                "Sign in to Slido in the window that opened.".into()
            )
        );
        s.step = Some(AudienceSourceStep::OpenEvent);
        assert!(slido_status(&s, 0, 0.0).1.starts_with("In that window"));
        s.state = AudienceSourceState::Error;
        assert_eq!(
            slido_status(&s, 0, 0.0),
            ("Connection problem", "Not getting questions".into())
        );
        s.state = AudienceSourceState::Live;
        s.updated_at = Some(1000.0);
        assert_eq!(
            slido_status(&s, 1, 2000.0).1,
            "1 question · updated just now"
        );
        assert_eq!(slido_code(&s), "Slido");
        s.event_id = Some("abc".into());
        assert_eq!(slido_link(&s), "https://app.sli.do/event/abc");
        s.code = Some("1234567".into());
        assert_eq!(slido_code(&s), "#1234567");
        assert_eq!(slido_link(&s), "https://app.sli.do/?search=1234567");

        let state = empty();
        let mut k = status("kairo", AudienceSourceState::Connecting);
        assert_eq!(kairo_status(&state, &k), "Opening…");
        k.error = Some("lost".into());
        assert_eq!(kairo_status(&state, &k), "Reconnecting…");
        k.state = AudienceSourceState::Error;
        assert_eq!(kairo_status(&state, &k), "Connection problem");
    }

    #[test]
    fn dates_cache_and_qr() {
        assert_eq!(day_to_time("", false), None);
        assert_eq!(day_to_time("not a day", false), None);
        let start = day_to_time("2026-03-12", false).unwrap();
        let end = day_to_time("2026-03-12", true).unwrap();
        assert!(end - start > 86_000_000.0);
        assert_eq!(day(start), "12 Mar");
        assert!(dates_invalid(Some(end), Some(start)));
        assert!(!dates_invalid(Some(start), None));
        let item = EngagementIndex {
            id: "i".into(),
            code: "ABC".into(),
            name: "n".into(),
            status: EngagementStatus::Open,
            starts_at: Some(start),
            ends_at: Some(day_to_time("2026-03-14", true).unwrap()),
        };
        assert_eq!(dates(&item).as_deref(), Some("12 Mar – 14 Mar"));
        let raw = write_cache(Some(std::slice::from_ref(&item))).unwrap();
        assert_eq!(read_cache(Some(&raw)), Some(vec![item]));
        assert_eq!(read_cache(Some("{\"a\":1}")), None);
        assert_eq!(read_cache(Some("nope")), None);
        assert_eq!(read_cache(None), None);
        assert_eq!(
            list_error("Please sign in again"),
            "Could not load engagements. Try again."
        );
        assert_eq!(qr_png("data:image/png;base64,AAEC"), Some(vec![0, 1, 2]));
        assert_eq!(qr_png("nope"), None);
        assert_eq!(qr_file_name("2AGXHS"), "kairo-2agxhs-qr.png");
        assert_eq!(bare_url("https://usekairo.com/q/X"), "usekairo.com/q/X");
        assert_eq!(initial(Some(" sarah")), "S");
        assert_eq!(initial(None), "?");
        assert_eq!(count(1, "vote", "votes"), "1 vote");
    }
}
