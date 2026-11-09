//! Where Kairo keeps things and how it starts, logs and fails (prompt 020).
//! Settings, secrets, SQLite and workspace files follow in prompts 021 to 024.
//!
//! - [`paths`]: every folder Electron's `app.getPath` gives, for the same profile.
//! - [`logging`]: `tracing` into electron-log's folder.
//! - [`crash`]: the panic hook, recovery snapshot and local crash ledger.
//! - [`instance`]: one Kairo per profile, forwarding later launches.
//! - [`launch`]: `.kairo` files from the command line and macOS open events.
//! - [`build_config`]: server URLs fixed at build time.
//! - [`os_locale`]: the system locale, as usage reports send it.

pub mod build_config;
pub mod crash;
pub mod instance;
pub mod launch;
pub mod logging;
pub mod paths;

/// `app.getLocale()`: the system's preferred locale as a BCP 47 tag, such as
/// `en-US`. Falls back to `en-US`, Chromium's default, when the system says nothing.
pub fn os_locale() -> String {
    sys_locale::get_locale()
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "en-US".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_locale_is_a_tag() {
        let locale = super::os_locale();
        assert!(!locale.is_empty() && !locale.contains(' '), "{locale}");
    }
}
