//! Display message extracted from errors carried over Electron IPC.

use regex::Regex;
use std::sync::LazyLock;

static REMOTE_WRAPPER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Error invoking remote method '[^']*':\s*").unwrap());
static STACK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*at\s+").unwrap());
static LEADING_ERROR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z]*Error:\s*").unwrap());

pub fn clean_ipc_error(raw: &str) -> &str {
    let message = REMOTE_WRAPPER
        .find(raw)
        .map_or(raw, |prefix| &raw[prefix.end()..]);
    let message = STACK
        .find(message)
        .map_or(message, |stack| &message[..stack.start()]);
    let mut message = crate::js::trim(message);
    while let Some(prefix) = LEADING_ERROR.find(message) {
        message = crate::js::trim(&message[prefix.end()..]);
    }
    if message.is_empty() {
        "Something went wrong."
    } else {
        message
    }
}
