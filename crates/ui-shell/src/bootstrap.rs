//! Launch rules and state (\`bootstrap/bootstrap-state.ts\`,
//! \`useBootstrapStore.ts\` and the splash logic in \`App.tsx\`).
//!
//! [\`Launch\`] is the launch screen's state machine. It holds no clock: the
//! shell feeds it the bootstrap's progress and result and the splash timers,
//! and asks it what to show.

use kairo_domain::scripture::ApiBibleAuthorizationState;
use kairo_domain::status::{
    AppBootstrapSnapshot, BootstrapProgress, BootstrapResource, BootstrapResourceError,
};
use kairo_ui_kit::launch_mark::LOCKUP_INTRO_MS;
use std::sync::Arc;

/// The branded splash shows on every launch for at least this long.
/// Bootstrap (local disk reads) is done well inside it, so the rest gives
/// background integrations a head start.
pub const SPLASH_MIN_VISIBLE_MS: u64 = 2_500;
/// The beat after the logo intro before the splash may close.
pub const INTRO_BEAT_MS: u64 = 450;
/// The splash closes by this time even if the intro never reports
/// finishing (a hidden window draws no frames).
pub const SPLASH_FALLBACK_MS: u64 = if SPLASH_MIN_VISIBLE_MS > LOCKUP_INTRO_MS + 2_000 {
    SPLASH_MIN_VISIBLE_MS
} else {
    LOCKUP_INTRO_MS + 2_000
};

/// The first step shown before main reports any progress.
pub const INITIAL_STEP: &str = "Getting everything ready…";

/// Progress before the first report.
pub fn initial_progress() -> BootstrapProgress {
    BootstrapProgress {
        completed: 0.0,
        total: 1.0,
        step: INITIAL_STEP.into(),
    }
}

/// Where startup is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapPhase {
    /// Not started.
    Idle,
    /// Waiting for the snapshot.
    Loading,
    /// Every resource loaded.
    Ready,
    /// Open, with a warning naming what failed.
    ReadyWithWarnings,
}

/// A failed resource opens the app with a warning; it never blocks entry.
pub fn bootstrap_phase(errors: &[BootstrapResourceError]) -> BootstrapPhase {
    if errors.is_empty() {
        BootstrapPhase::Ready
    } else {
        BootstrapPhase::ReadyWithWarnings
    }
}

/// The operator's name for a resource (\`RESOURCE_LABELS\`).
pub fn resource_label(resource: BootstrapResource) -> &'static str {
    match resource {
        BootstrapResource::Settings => "settings",
        BootstrapResource::Orchestrator => "automation status",
        BootstrapResource::Transcription => "transcript history",
        BootstrapResource::Translations => "Bible translations",
        BootstrapResource::SermonPlans => "scripture playlists",
        BootstrapResource::LivePlan => "live playlist",
        BootstrapResource::Lyrics => "song library",
        BootstrapResource::Onboarding => "setup progress",
        BootstrapResource::Account => "account",
    }
}

/// The warning bar's text, or empty when nothing failed.
pub fn describe_bootstrap_warning(errors: &[BootstrapResourceError]) -> String {
    if errors.is_empty() {
        return String::new();
    }
    let names: Vec<&str> = errors.iter().map(|e| resource_label(e.resource)).collect();
    format!(
        "Some data could not be loaded at startup: {}.",
        names.join(", ")
    )
}

/// Whole percent, clamped to 0–100; 0 when the total is not positive.
pub fn bootstrap_percent(progress: &BootstrapProgress) -> u32 {
    // NaN fails the comparison too, as in the original.
    if progress.total.is_nan() || progress.total <= 0.0 {
        return 0;
    }
    let percent = (progress.completed / progress.total * 100.0).round();
    if percent.is_nan() {
        return 0;
    }
    percent.clamp(0.0, 100.0) as u32
}

/// Scripture stays quiet about the API.Bible key until authorization is
/// actually known: a warning during startup or offline is noise.
pub fn should_show_api_bible_warning(
    state: ApiBibleAuthorizationState,
    has_key: bool,
    settings_known: bool,
) -> bool {
    // Before startup delivers settings, "no key" is an assumption.
    if !settings_known {
        return false;
    }
    if !has_key {
        return true;
    }
    state == ApiBibleAuthorizationState::Unauthorized
}

/// A refusal is definitive; anything else (no network, DNS) is not
/// (\`isAuthorizationFailure\`).
pub fn is_authorization_failure(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "401",
        "403",
        "unauthor",
        "not authorized",
        "invalid api key",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// The launch screen's state.
#[derive(Clone, Debug)]
pub struct Launch {
    phase: BootstrapPhase,
    progress: BootstrapProgress,
    errors: Vec<BootstrapResourceError>,
    warning_dismissed: bool,
    splash_held: bool,
    intro_done: bool,
    generation: u64,
    snapshot: Option<Arc<AppBootstrapSnapshot>>,
    openings: u64,
}

impl Default for Launch {
    fn default() -> Self {
        Self::new()
    }
}

impl Launch {
    /// Before startup: the splash shows.
    pub fn new() -> Self {
        Self {
            phase: BootstrapPhase::Idle,
            progress: initial_progress(),
            errors: Vec::new(),
            warning_dismissed: false,
            splash_held: false,
            intro_done: false,
            generation: 0,
            snapshot: None,
            openings: 0,
        }
    }

    /// No launch screen at all: open straight away with nothing loaded.
    pub fn skipped() -> Self {
        Self {
            phase: BootstrapPhase::Ready,
            splash_held: true,
            intro_done: true,
            openings: 1,
            ..Self::new()
        }
    }

    /// Starts once. Returns the request generation to load, or \`None\` if
    /// already started.
    pub fn start(&mut self) -> Option<u64> {
        if self.phase != BootstrapPhase::Idle {
            return None;
        }
        self.phase = BootstrapPhase::Loading;
        self.generation += 1;
        Some(self.generation)
    }

    /// Loads again after a failure, from the warning bar. Returns the new
    /// generation; earlier requests' reports are ignored from now on.
    pub fn retry(&mut self) -> u64 {
        self.phase = BootstrapPhase::Loading;
        self.errors.clear();
        self.warning_dismissed = false;
        self.progress = initial_progress();
        self.generation += 1;
        self.generation
    }

    /// A progress report. Ignored unless it belongs to the load in flight.
    pub fn progress(&mut self, generation: u64, progress: BootstrapProgress) -> bool {
        if generation != self.generation || self.phase != BootstrapPhase::Loading {
            return false;
        }
        self.progress = progress;
        true
    }

    /// The load's result. A failed request opens the app with what it had
    /// rather than trap the operator on a screen they cannot leave.
    pub fn finish(
        &mut self,
        generation: u64,
        result: Result<AppBootstrapSnapshot, String>,
    ) -> bool {
        if generation != self.generation || self.phase != BootstrapPhase::Loading {
            return false;
        }
        match result {
            Ok(snapshot) => {
                self.phase = bootstrap_phase(&snapshot.errors);
                self.errors = snapshot.errors.clone();
                self.snapshot = Some(Arc::new(snapshot));
            }
            Err(message) => {
                self.phase = BootstrapPhase::ReadyWithWarnings;
                self.errors = vec![BootstrapResourceError {
                    resource: BootstrapResource::Settings,
                    message,
                }];
            }
        }
        if self.ready() {
            self.openings += 1;
        }
        true
    }

    /// The splash has been up long enough.
    pub fn hold_splash(&mut self) -> bool {
        if self.splash_held {
            return false;
        }
        let was_ready = self.ready();
        self.splash_held = true;
        if !was_ready && self.ready() {
            self.openings += 1;
        }
        true
    }

    /// The logo intro has played out. True the first time.
    pub fn mark_intro_done(&mut self) -> bool {
        !std::mem::replace(&mut self.intro_done, true)
    }

    /// Whether the intro has played out.
    pub fn intro_done(&self) -> bool {
        self.intro_done
    }

    /// The phase.
    pub fn phase(&self) -> BootstrapPhase {
        self.phase
    }

    /// The latest progress.
    pub fn current_progress(&self) -> &BootstrapProgress {
        &self.progress
    }

    /// The failures from the last load.
    pub fn errors(&self) -> &[BootstrapResourceError] {
        &self.errors
    }

    /// The last snapshot that arrived, kept across a failed retry.
    pub fn snapshot(&self) -> Option<&Arc<AppBootstrapSnapshot>> {
        self.snapshot.as_ref()
    }

    /// The load in flight, or the last one.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// How many times the app has opened from the splash; the app fades in
    /// each time.
    pub fn openings(&self) -> u64 {
        self.openings
    }

    /// The snapshot (or its failure) has arrived.
    pub fn bootstrapped(&self) -> bool {
        matches!(
            self.phase,
            BootstrapPhase::Ready | BootstrapPhase::ReadyWithWarnings
        )
    }

    /// The app may show: loaded, and the splash has had its time.
    pub fn ready(&self) -> bool {
        self.bootstrapped() && self.splash_held
    }

    /// Progress is only worth showing once loading outlasts the brand beat.
    pub fn show_progress(&self) -> bool {
        self.splash_held && !self.bootstrapped()
    }

    /// The warning bar's text while it should show.
    pub fn warning(&self) -> Option<String> {
        (self.ready() && !self.errors.is_empty() && !self.warning_dismissed)
            .then(|| describe_bootstrap_warning(&self.errors))
    }

    /// Hides the warning bar until the next load.
    pub fn dismiss_warning(&mut self) {
        self.warning_dismissed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(resource: BootstrapResource) -> BootstrapResourceError {
        BootstrapResourceError {
            resource,
            message: "boom".into(),
        }
    }

    fn snapshot(errors: Vec<BootstrapResourceError>) -> AppBootstrapSnapshot {
        AppBootstrapSnapshot {
            errors,
            ..kairo_core::bootstrap::neutral_snapshot()
        }
    }

    #[test]
    fn warning_names_resources_in_order() {
        assert_eq!(describe_bootstrap_warning(&[]), "");
        assert_eq!(
            describe_bootstrap_warning(&[
                error(BootstrapResource::Lyrics),
                error(BootstrapResource::Orchestrator)
            ]),
            "Some data could not be loaded at startup: song library, automation status."
        );
        assert_eq!(bootstrap_phase(&[]), BootstrapPhase::Ready);
        assert_eq!(
            bootstrap_phase(&[error(BootstrapResource::Account)]),
            BootstrapPhase::ReadyWithWarnings
        );
    }

    #[test]
    fn percent_rounds_and_clamps() {
        let p = |completed, total| BootstrapProgress {
            completed,
            total,
            step: String::new(),
        };
        assert_eq!(bootstrap_percent(&p(0.0, 0.0)), 0);
        assert_eq!(bootstrap_percent(&p(3.0, -1.0)), 0);
        assert_eq!(bootstrap_percent(&p(1.0, f64::NAN)), 0);
        assert_eq!(bootstrap_percent(&p(1.0, 9.0)), 11);
        assert_eq!(bootstrap_percent(&p(1.0, 8.0)), 13);
        assert_eq!(bootstrap_percent(&p(12.0, 9.0)), 100);
        assert_eq!(bootstrap_percent(&p(-1.0, 9.0)), 0);
    }

    #[test]
    fn api_bible_warning_waits_for_facts() {
        use ApiBibleAuthorizationState::*;
        assert!(!should_show_api_bible_warning(Unchecked, false, false));
        assert!(should_show_api_bible_warning(Unchecked, false, true));
        assert!(!should_show_api_bible_warning(Offline, true, true));
        assert!(!should_show_api_bible_warning(Checking, true, true));
        assert!(should_show_api_bible_warning(Unauthorized, true, true));
        assert!(is_authorization_failure("HTTP 401"));
        assert!(is_authorization_failure("Invalid API key"));
        assert!(is_authorization_failure("Unauthorized"));
        assert!(!is_authorization_failure("getaddrinfo ENOTFOUND"));
    }

    #[test]
    fn splash_timings_match_the_source() {
        assert_eq!(SPLASH_FALLBACK_MS, 4_550);
        assert_eq!(LOCKUP_INTRO_MS + INTRO_BEAT_MS, 3_000);
    }

    #[test]
    fn opens_only_when_loaded_and_held() {
        let mut launch = Launch::new();
        assert!(!launch.ready());
        assert_eq!(launch.start(), Some(1));
        assert_eq!(launch.start(), None, "one bootstrap per window");
        assert!(launch.finish(1, Ok(snapshot(vec![]))));
        assert!(launch.bootstrapped() && !launch.ready());
        assert!(!launch.show_progress());
        assert!(launch.hold_splash());
        assert!(launch.ready());
        assert_eq!(launch.openings(), 1);
        assert_eq!(launch.warning(), None);
    }

    #[test]
    fn slow_load_shows_progress_after_the_hold() {
        let mut launch = Launch::new();
        launch.start();
        launch.hold_splash();
        assert!(launch.show_progress());
        let step = BootstrapProgress {
            completed: 4.0,
            total: 9.0,
            step: "Loading scriptures…".into(),
        };
        assert!(launch.progress(1, step.clone()));
        assert!(!launch.progress(7, step.clone()), "other loads are ignored");
        assert_eq!(launch.current_progress(), &step);
        assert!(launch.finish(1, Ok(snapshot(vec![]))));
        assert!(launch.ready());
        assert!(!launch.progress(1, step), "late progress is ignored");
    }

    #[test]
    fn failure_opens_with_a_settings_warning_then_retry_reloads() {
        let mut launch = Launch::new();
        launch.start();
        launch.hold_splash();
        assert!(launch.finish(1, Err("The startup request did not finish.".into())));
        assert_eq!(launch.phase(), BootstrapPhase::ReadyWithWarnings);
        assert_eq!(
            launch.warning().as_deref(),
            Some("Some data could not be loaded at startup: settings.")
        );
        assert_eq!(
            launch.errors()[0].message,
            "The startup request did not finish."
        );

        launch.dismiss_warning();
        assert_eq!(launch.warning(), None);

        let generation = launch.retry();
        assert_eq!(generation, 2);
        assert!(
            !launch.ready() && launch.show_progress(),
            "retry shows the progress screen"
        );
        assert_eq!(launch.current_progress(), &initial_progress());
        assert!(
            !launch.finish(1, Ok(snapshot(vec![]))),
            "the old load is stale"
        );
        assert!(launch.finish(2, Ok(snapshot(vec![error(BootstrapResource::Lyrics)]))));
        assert_eq!(
            launch.warning().as_deref(),
            Some("Some data could not be loaded at startup: song library.")
        );
        assert_eq!(launch.openings(), 2);
        assert!(launch.snapshot().is_some());
    }

    #[test]
    fn failed_retry_keeps_the_last_snapshot() {
        let mut launch = Launch::new();
        launch.start();
        launch.hold_splash();
        launch.finish(1, Ok(snapshot(vec![error(BootstrapResource::Account)])));
        let generation = launch.retry();
        launch.finish(generation, Err("gone".into()));
        assert!(launch.snapshot().is_some());
        assert_eq!(launch.errors()[0].resource, BootstrapResource::Settings);
    }

    #[test]
    fn skipped_is_open_with_nothing_loaded() {
        let launch = Launch::skipped();
        assert!(launch.ready());
        assert_eq!(launch.warning(), None);
        assert!(launch.snapshot().is_none());
    }
}
