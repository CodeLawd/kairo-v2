//! Operator-facing copy for online lyrics errors. Port of
//! `src/lib/lyrics-online-error.ts`.

use super::js_text::{WS, len16, re, trim};
use regex::Regex;
use std::sync::LazyLock;

static IPC_WRAPPER: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"^(?i-u:error invoking remote method )'[^']+':{WS}*"
    ))
});
static ERROR_PREFIX: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"^(?i-u:error:){WS}*")));
static NO_LYRICS: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i-u:no lyric text was found|no sections could be parsed|couldn'?t structure)")
});
static NETWORK: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i-u:timeout|etimedout|econn|enotfound|network|fetch failed|socket)"));
static LRCLIB: LazyLock<Regex> = LazyLock::new(|| re(r"(?i-u:lrclib)"));
static BLOCKED: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i-u:403|401|429|blocked|captcha|forbidden|too many requests)"));
static NOT_FOUND: LazyLock<Regex> = LazyLock::new(|| re(r"(?i-u:404|not found)"));

/// Short operator copy for an error message. Never shows the "Error invoking
/// remote method …" wrapper. TypeScript takes the error itself and reads its
/// `message` (or `String(err ?? '')`); callers pass that text here.
pub fn format_online_lyrics_error(message: &str) -> String {
    let msg = IPC_WRAPPER.replace(message, "");
    let msg = ERROR_PREFIX.replace(&msg, "");
    let msg = trim(&msg);
    if NO_LYRICS.is_match(msg) {
        return "This page didn't return usable lyrics.".into();
    }
    if NETWORK.is_match(msg) {
        return if LRCLIB.is_match(msg) {
            "Couldn't reach LRCLIB in time. Try again, or pick another result."
        } else {
            "Couldn't reach the lyrics source."
        }
        .into();
    }
    if BLOCKED.is_match(msg) {
        return "That lyrics page blocked the request.".into();
    }
    if NOT_FOUND.is_match(msg) {
        return "Lyrics weren't found for this result.".into();
    }
    if msg.is_empty() || len16(msg) > 160 {
        return "Couldn't load lyrics from this result.".into();
    }
    msg.to_string()
}
