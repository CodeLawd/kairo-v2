//! What the shell shows, fed by the app, and what it asks the app to do.
//!
//! The services behind these values are composed by prompt 067. Until a
//! value arrives it stays at its "not loaded" default, and the item that
//! depends on it renders nothing, exactly as the Electron components do
//! before their first status (`UpdatePill` while idle, `ActivationChip`
//! before the licence loads, `KeychainNotice` while signed out).

use crate::route::Route;
use gpui_kit::SharedString;
use kairo_domain::import_menu::ImportKind;
use kairo_domain::settings::ShortcutBindings;
use std::path::PathBuf;

/// The updater's state (`UpdateStatus`).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateState {
    /// Nothing to do (also dev and unpackaged builds).
    #[default]
    Idle,
    /// Checking the feed.
    Checking,
    /// A newer version can be downloaded.
    Available {
        /// The version.
        version: SharedString,
    },
    /// Downloading.
    Downloading {
        /// Percent, when known.
        percent: Option<u8>,
    },
    /// Downloaded; installs on the next quit.
    Downloaded {
        /// The version.
        version: SharedString,
    },
    /// The last check or download failed.
    Error,
}

/// The signed-in account, as far as the shell needs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountView {
    /// Signed in (any state but signed out).
    pub signed_in: bool,
    /// Display name.
    pub name: Option<SharedString>,
    /// Email.
    pub email: Option<SharedString>,
    /// `false` when this launch's sign-in will not be remembered.
    pub credentials_persisted: Option<bool>,
}

/// A rendered output (screen or NDI) and whether it is on the air.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputReadiness {
    /// Output name.
    pub name: SharedString,
    /// Ready, or `None` when the engine has not reported it.
    pub ready: Option<bool>,
    /// Why it is not ready.
    pub reason: Option<SharedString>,
}

/// One line for the header's Screens button (`useScreensStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreensLevel {
    /// Every enabled rendered output is live.
    Ready,
    /// At least one is not.
    Warning,
    /// None set up.
    Offline,
}

/// The Screens status for the enabled rendered outputs.
pub fn screens_status(outputs: &[OutputReadiness]) -> (String, ScreensLevel) {
    if outputs.is_empty() {
        return (
            "No screens set up — click to add one".into(),
            ScreensLevel::Offline,
        );
    }
    match outputs.iter().find(|o| o.ready != Some(true)) {
        None if outputs.len() == 1 => (format!("{} is live", outputs[0].name), ScreensLevel::Ready),
        None => (format!("{} live", outputs.len()), ScreensLevel::Ready),
        Some(first) => {
            let reason = first.reason.as_deref().unwrap_or("checking");
            (format!("{}: {reason}", first.name), ScreensLevel::Warning)
        }
    }
}

/// A house track playing from Tracks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HouseTrack {
    /// Track name.
    pub name: SharedString,
    /// Paused or ended.
    pub paused: bool,
}

/// Everything the shell draws that comes from services.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShellState {
    /// What is on screen (`liveOutputLabel`).
    pub live_label: Option<SharedString>,
    /// The transcript pipeline is running.
    pub transcribing: bool,
    /// Capture is recording.
    pub recording: bool,
    /// The updater.
    pub update: UpdateState,
    /// The account.
    pub account: AccountView,
    /// This computer is activated; `None` before the licence loads.
    pub activated: Option<bool>,
    /// Enabled rendered outputs; `None` before settings load.
    pub outputs: Option<Vec<OutputReadiness>>,
    /// The house track, if one is live.
    pub house_track: Option<HouseTrack>,
    /// The operator's shortcut bindings (`display.shortcuts`).
    pub shortcuts: ShortcutBindings,
    /// `whatsNew.lastSeenVersion`; `None` before settings load.
    pub whats_new_last_seen: Option<SharedString>,
}

impl ShellState {
    /// A session is live: transcribing, or something is on screen.
    pub fn is_live(&self) -> bool {
        self.transcribing
            || self
                .live_label
                .as_deref()
                .is_some_and(|l| !l.trim().is_empty())
    }

    /// The Live button's label and detail.
    pub fn live_text(&self) -> (SharedString, SharedString) {
        let label = self
            .live_label
            .as_ref()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty());
        let short: SharedString = match (label, self.transcribing) {
            (Some(l), _) => l.to_owned().into(),
            (None, true) => "Listening".into(),
            (None, false) => "Live".into(),
        };
        let detail: SharedString = match (label, self.transcribing) {
            (Some(l), _) => l.to_owned().into(),
            (None, true) => "Listening for scripture".into(),
            (None, false) => "Nothing on screen".into(),
        };
        (short, detail)
    }
}

/// The kind of feedback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FeedbackKind {
    /// An idea.
    #[default]
    Idea,
    /// A problem.
    Problem,
    /// Praise.
    Praise,
}

impl FeedbackKind {
    /// The stored name.
    pub fn as_str(self) -> &'static str {
        match self {
            FeedbackKind::Idea => "idea",
            FeedbackKind::Problem => "problem",
            FeedbackKind::Praise => "praise",
        }
    }
}

/// What the shell asks the app to do. Each is handled by the service that
/// owns it once prompt 067 composes them.
#[derive(Clone, Debug, PartialEq)]
pub enum ShellRequest {
    /// The window gained focus: pull org secrets (throttled), wake the
    /// sermon uploader and usage.
    WindowFocused,
    /// Clear text and the dock background.
    ClearLive,
    /// Check for updates.
    CheckForUpdates,
    /// Download the available update.
    DownloadUpdate,
    /// Quit and install the downloaded update.
    InstallUpdate,
    /// Pause or resume the house track.
    SetHouseTrackPaused(bool),
    /// Send feedback; answer with [`crate::Shell::feedback_result`].
    SendFeedback {
        /// Kind.
        kind: FeedbackKind,
        /// Message, trimmed and capped.
        message: String,
        /// The route it was sent from.
        context: Route,
    },
    /// Record `whatsNew.lastSeenVersion`.
    MarkWhatsNewSeen(SharedString),
    /// An Import menu item; the route it needs is already showing.
    Import(ImportKind),
    /// `.kairo` files to open, delivered once the UI is ready.
    OpenKairoFiles(Vec<PathBuf>),
    /// Open a link in the system browser.
    OpenExternal(SharedString),
    /// A feature panicked and was contained; record it for crash reports.
    FeaturePanicked {
        /// The route.
        route: Route,
        /// The panic message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(name: &str, ready: Option<bool>, reason: Option<&str>) -> OutputReadiness {
        OutputReadiness {
            name: name.into(),
            ready,
            reason: reason.map(Into::into),
        }
    }

    #[test]
    fn screens_status_lines() {
        assert_eq!(screens_status(&[]).1, ScreensLevel::Offline);
        assert_eq!(
            screens_status(&[out("Audience", Some(true), None)]).0,
            "Audience is live"
        );
        assert_eq!(
            screens_status(&[out("A", Some(true), None), out("B", Some(true), None)]).0,
            "2 live"
        );
        assert_eq!(
            screens_status(&[out("A", Some(true), None), out("Stage", None, None)]),
            ("Stage: checking".into(), ScreensLevel::Warning)
        );
        assert_eq!(
            screens_status(&[out("Stage", Some(false), Some("display unplugged"))]).0,
            "Stage: display unplugged"
        );
    }

    #[test]
    fn live_follows_transcription_and_output() {
        let mut s = ShellState::default();
        assert!(!s.is_live());
        assert_eq!(s.live_text().1, "Nothing on screen");
        s.live_label = Some("   ".into());
        assert!(!s.is_live());
        s.transcribing = true;
        assert_eq!(
            s.live_text(),
            ("Listening".into(), "Listening for scripture".into())
        );
        s.live_label = Some(" John 3:16 ".into());
        assert_eq!(s.live_text().0, "John 3:16");
    }
}
