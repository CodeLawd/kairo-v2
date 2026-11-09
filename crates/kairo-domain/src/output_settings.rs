//! The optimistic output settings writer, from `src/lib/output-settings.ts`.
//!
//! The TypeScript is an async closure. Its pure part, the revision
//! bookkeeping that decides what to publish and what to roll back to, is
//! ported here as a state machine. The service that owns persistence
//! (prompt 021) supplies the `read`, `publish` and `persist` side effects:
//!
//! 1. `let write = writer.begin(read(), edit)`; publish `write.next` and
//!    persist `write.patch()`.
//! 2. When persistence finishes, call `writer.settle(write, ok)`. If it
//!    returns outputs and theme, publish `read()` with those restored.

use crate::json::to_js_value;
use crate::overlay::{OverlayContentKind, OverlayOutput, OverlayTheme};
use crate::overlay_defaults::normalize_overlay_settings;
use crate::overlay_outputs::live_overlay_theme;
use crate::settings::OverlaySettings;

/// One write in flight.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingWrite {
    pub version: u64,
    /// The normalised overlay to publish straight away.
    pub next: OverlaySettings,
}

impl PendingWrite {
    /// What to persist: only `outputs` and `theme`, since other overlay
    /// controls own their own fields.
    pub fn patch(&self) -> (&[OverlayOutput], &OverlayTheme) {
        (&self.next.outputs, &self.next.theme)
    }
}

/// The last acknowledged outputs and theme, to restore after a failure.
#[derive(Clone, Debug, PartialEq)]
pub struct Rollback {
    pub outputs: Vec<OverlayOutput>,
    pub theme: OverlayTheme,
}

/// `createOutputSettingsWriter`'s state.
#[derive(Clone, Debug, Default)]
pub struct OutputSettingsWriter {
    revision: u64,
    pending: u32,
    confirmed: Option<OverlaySettings>,
    confirmed_revision: u64,
    latest_failed: bool,
}

impl OutputSettingsWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a write: applies `edit` to the current outputs, normalises, and
    /// points the deprecated `theme` at the live scripture theme.
    pub fn begin(
        &mut self,
        previous: OverlaySettings,
        edit: impl FnOnce(Vec<OverlayOutput>) -> Vec<OverlayOutput>,
    ) -> PendingWrite {
        self.revision += 1;
        let version = self.revision;
        let mut draft = previous.clone();
        draft.outputs = edit(previous.outputs.clone());
        let raw = to_js_value(&draft).expect("overlay settings always serialise");
        let mut next = normalize_overlay_settings(Some(&raw));
        next.theme = live_overlay_theme(&next, OverlayContentKind::Scripture);
        if self.pending == 0 {
            self.confirmed = Some(previous);
            self.confirmed_revision = version - 1;
        }
        self.pending += 1;
        self.latest_failed = false;
        PendingWrite { version, next }
    }

    /// Records how a write's persistence ended. Returns what to restore once
    /// every write in the batch has settled and the newest one failed.
    pub fn settle(&mut self, write: &PendingWrite, ok: bool) -> Option<Rollback> {
        if ok {
            if write.version > self.confirmed_revision {
                self.confirmed = Some(write.next.clone());
                self.confirmed_revision = write.version;
            }
        } else if self.revision == write.version {
            self.latest_failed = true;
        }
        self.pending -= 1;
        if self.pending == 0 && self.latest_failed {
            return self.confirmed.as_ref().map(|c| Rollback {
                outputs: c.outputs.clone(),
                theme: c.theme.clone(),
            });
        }
        None
    }
}
