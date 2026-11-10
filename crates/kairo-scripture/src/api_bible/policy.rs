//! `api-cache-policy.ts` and `offline-download-policy.ts`.

/// API.Bible requires cached content to be refreshed at least every 30 days.
pub const API_CACHE_MAX_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

pub fn is_fresh(fetched_at: i64, now: i64) -> bool {
    now - fetched_at < API_CACHE_MAX_AGE_MS
}

/// The instant a verse fetched at `fetched_at` may no longer be shown.
pub fn api_cache_expiry(fetched_at: i64) -> i64 {
    fetched_at + API_CACHE_MAX_AGE_MS
}

pub const OFFLINE_BIBLE_IDS_ENV_VAR: &str = "KAIRO_OFFLINE_BIBLE_IDS";
pub const LEGACY_OFFLINE_BIBLE_IDS_ENV_VAR: &str = "PROAUTOMATE_OFFLINE_BIBLE_IDS";

/// Stored allowlist plus the deployment variables, trimmed and de-duplicated
/// in first-seen order. `env` reads one variable.
pub fn resolve_offline_download_bible_ids(
    stored: &[String],
    env: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let from_env: Vec<String> = [OFFLINE_BIBLE_IDS_ENV_VAR, LEGACY_OFFLINE_BIBLE_IDS_ENV_VAR]
        .into_iter()
        .flat_map(|name| {
            env(name)
                .unwrap_or_default()
                .split(|c: char| c == ',' || kairo_domain::js::is_whitespace(c))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    let mut ids: Vec<String> = Vec::new();
    for id in stored.iter().chain(&from_env) {
        let id = kairo_domain::js::trim(id);
        if !id.is_empty() && !ids.iter().any(|seen| seen == id) {
            ids.push(id.to_owned());
        }
    }
    ids
}

/// The process environment, for [`resolve_offline_download_bible_ids`].
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// A key alone never permits a whole-translation download: the id must be
/// licensed, or the text must be public domain.
pub fn is_offline_download_permitted(
    bible_id: &str,
    allowlist: &[String],
    copyright: &str,
) -> bool {
    allowlist.iter().any(|id| id == bible_id) || is_public_domain(copyright)
}

/// `/public\s+domain/i`.
fn is_public_domain(copyright: &str) -> bool {
    let lower = copyright.to_lowercase();
    lower.match_indices("public").any(|(i, _)| {
        let rest = &lower[i + "public".len()..];
        let trimmed = rest.trim_start_matches(kairo_domain::js::is_whitespace);
        trimmed.len() < rest.len() && trimmed.starts_with("domain")
    })
}
