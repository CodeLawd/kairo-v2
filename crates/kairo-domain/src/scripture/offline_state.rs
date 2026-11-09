//! Operator-facing states for cached API.Bible content, from
//! `src/lib/scripture-offline-state.ts`, and the offline Bible rows in
//! Settings, from `src/renderer/.../settings/offline-bible-view-model.ts`.

use super::{
    ApiBibleCacheStatus, ApiBibleDownloadProgress, ApiBibleOfflineTranslation,
    ScriptureTranslationOption, TranslationAccess, js,
};
use serde::{Deserialize, Serialize};

/// Error messages the scripture service raises for cached content. The
/// wording operators are trained on lives here only.
pub mod api_cache_messages {
    pub const NOT_AVAILABLE_OFFLINE: &str = "This passage is not available offline.";
    pub const REFRESH_REQUIRED: &str =
        "This Bible must be refreshed before it can be used offline.";
    pub const ACCESS_REVOKED: &str =
        "Your API.Bible account no longer has access to this translation.";
    pub const OFFLINE_DOWNLOAD_NOT_PERMITTED: &str =
        "Offline download for this translation has not been licensed for this app.";
}

crate::serde_util::string_enum!(
    pub enum ScriptureCacheNoticeKind {
        OfflineMissing = "offline-missing",
        RefreshRequired = "refresh-required",
        AccessRevoked = "access-revoked",
        DownloadNotLicensed = "download-not-licensed",
    }
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptureCacheNotice {
    pub kind: ScriptureCacheNoticeKind,
    pub message: String,
    pub hint: String,
}

/// The notice for an error the operator must act on, from the error's message
/// (an `Error`'s message, a thrown string, or `""` for anything else).
///
/// The source looks the message up in a plain object, so a message such as
/// `"toString"` returns an inherited function; that is a bug, and this port
/// returns `None` instead (DECISIONS.md, prompt 015).
pub fn get_scripture_cache_notice(message: &str) -> Option<ScriptureCacheNotice> {
    use api_cache_messages::*;
    let (kind, hint) = match message {
        NOT_AVAILABLE_OFFLINE => (
            ScriptureCacheNoticeKind::OfflineMissing,
            "Restore the internet connection, or download this Bible for offline use in Settings → Scripture.",
        ),
        REFRESH_REQUIRED => (
            ScriptureCacheNoticeKind::RefreshRequired,
            "Cached API.Bible text expires after 30 days. Reconnect and refresh it in Settings → Scripture → Offline Bibles.",
        ),
        ACCESS_REVOKED => (
            ScriptureCacheNoticeKind::AccessRevoked,
            "Check the translations enabled for your key at API.Bible, or choose another translation.",
        ),
        OFFLINE_DOWNLOAD_NOT_PERMITTED => (
            ScriptureCacheNoticeKind::DownloadNotLicensed,
            "Confirm the API.Bible plan and publisher licence before enabling whole-Bible download.",
        ),
        _ => return None,
    };
    Some(ScriptureCacheNotice {
        kind,
        message: message.to_string(),
        hint: hint.to_string(),
    })
}

pub const API_BIBLE_ATTRIBUTION_LABEL: &str = "Scripture provided by API.Bible";
pub const API_BIBLE_ATTRIBUTION_URL: &str = "https://scripture.api.bible";

/// API-derived passages carry their provider attribution; bundled ones do not.
pub fn should_show_api_bible_attribution(option: Option<&ScriptureTranslationOption>) -> bool {
    option.is_some_and(|o| o.access == TranslationAccess::Api)
}

crate::serde_util::string_enum!(
    pub enum OfflineBibleAction {
        Download = "download",
        Resume = "resume",
        Pause = "pause",
        Refresh = "refresh",
        Retry = "retry",
        Remove = "remove",
    }
);

impl OfflineBibleAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Download => "Download",
            Self::Resume => "Resume",
            Self::Pause => "Pause",
            Self::Refresh => "Refresh",
            Self::Retry => "Retry",
            Self::Remove => "Remove",
        }
    }
}

/// What an operator may do with a cached translation. Anything that fetches
/// licensed content needs the confirmed offline licence.
pub fn get_offline_bible_actions(row: &ApiBibleOfflineTranslation) -> Vec<OfflineBibleAction> {
    use ApiBibleCacheStatus as S;
    use OfflineBibleAction as A;
    let fetching = row.offline_download_enabled;
    let gated = |action| {
        if fetching {
            vec![action, A::Remove]
        } else {
            vec![A::Remove]
        }
    };
    match row.status {
        S::Downloading => vec![A::Pause],
        S::Downloaded | S::Unavailable => vec![A::Remove],
        S::Stale => gated(A::Refresh),
        S::Paused | S::Partial => gated(A::Resume),
        S::Failed => gated(A::Retry),
        S::NotDownloaded => {
            if fetching {
                vec![A::Download]
            } else {
                Vec::new()
            }
        }
    }
}

pub fn get_offline_bible_status_label(row: &ApiBibleOfflineTranslation) -> String {
    use ApiBibleCacheStatus as S;
    let n = js::number_to_string;
    match row.status {
        S::Downloading => format!(
            "Downloading {} of {} chapters",
            n(row.cached_chapters),
            n(row.total_chapters)
        ),
        S::Downloaded => "Downloaded".into(),
        S::Paused => format!(
            "Paused at {} of {} chapters",
            n(row.cached_chapters),
            n(row.total_chapters)
        ),
        S::Partial => "Partially cached".into(),
        S::Stale => "Refresh required".into(),
        S::Unavailable => "No longer licensed".into(),
        S::Failed => match row.error.as_deref() {
            Some(error) if !error.is_empty() => format!("Failed — {error}"),
            _ => "Failed".into(),
        },
        S::NotDownloaded => "Not downloaded".into(),
    }
}

/// Whole percent of chapters cached, bounded to 0–100; 0 without a total.
pub fn get_offline_bible_progress_percent(row: &ApiBibleOfflineTranslation) -> f64 {
    if row.total_chapters <= 0.0 {
        return 0.0;
    }
    js::max(
        0.0,
        js::min(
            100.0,
            js::round(row.cached_chapters / row.total_chapters * 100.0),
        ),
    )
}

/// When licensed text stops being displayable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OfflineBibleExpiry {
    /// No expiry recorded; the source shows nothing.
    None,
    Expired,
    /// Expires at this time (milliseconds since the epoch).
    At(f64),
}

/// The expiry state at `now`. The source formats `At` with the system
/// locale's short date (`Expires Aug 26, 2026`); see [`format_offline_bible_expiry`].
pub fn offline_bible_expiry(row: &ApiBibleOfflineTranslation, now: f64) -> OfflineBibleExpiry {
    match row.expires_at {
        None => OfflineBibleExpiry::None,
        Some(at) if at <= now => OfflineBibleExpiry::Expired,
        Some(at) => OfflineBibleExpiry::At(at),
    }
}

/// The source's text: `""`, `"Expired"`, or `"Expires "` and the date as the
/// caller's locale formats it (the view owns locale; prompt 074).
pub fn format_offline_bible_expiry(
    row: &ApiBibleOfflineTranslation,
    now: f64,
    format_date: impl FnOnce(f64) -> String,
) -> String {
    match offline_bible_expiry(row, now) {
        OfflineBibleExpiry::None => String::new(),
        OfflineBibleExpiry::Expired => "Expired".into(),
        OfflineBibleExpiry::At(at) => format!("Expires {}", format_date(at)),
    }
}

/// Applies a pushed progress event to the matching row, leaving others intact.
pub fn merge_offline_download_progress(
    rows: &[ApiBibleOfflineTranslation],
    progress: &ApiBibleDownloadProgress,
) -> Vec<ApiBibleOfflineTranslation> {
    rows.iter()
        .map(|row| {
            if row.bible_id != progress.bible_id {
                return row.clone();
            }
            let mut next = ApiBibleOfflineTranslation {
                status: progress.status,
                cached_chapters: progress.completed_chapters,
                total_chapters: progress.total_chapters,
                cached_verses: progress.cached_verses,
                error: progress.error.clone().filter(|e| !e.is_empty()),
                ..row.clone()
            };
            if progress.cached_verses == 0.0 {
                next.fetched_at = None;
                next.expires_at = None;
            }
            next
        })
        .collect()
}
