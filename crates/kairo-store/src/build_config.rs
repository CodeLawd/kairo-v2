//! Build-time configuration, replacing `MAIN_VITE_*` from `src/main/env.d.ts`
//! and `.env.example`. Set by the release job as environment variables when
//! compiling:
//!
//! | Variable | Electron | Default (dev) |
//! |---|---|---|
//! | `KAIRO_API_URL` | `MAIN_VITE_API_URL` | `http://localhost:3000` |
//! | `KAIRO_WEB_URL` | `MAIN_VITE_WEB_URL` | `http://localhost:3001` |
//! | `KAIRO_AUDIENCE_API_URL` | `MAIN_VITE_AUDIENCE_API_URL` | derived from the API URL |
//! | `KAIRO_LICENSE_PUBLIC_KEY` | `MAIN_VITE_LICENSE_PUBLIC_KEY` | none |
//!
//! These are compiled into the binary, so they are public: URLs and keys
//! meant to be public only, never a secret. An empty value counts as unset,
//! as `||` does in TypeScript.
//!
//! A release build (`debug_assertions` off) must point at real servers: it
//! fails to compile if an API, web or audience URL is local or not `https`.
//! To profile a release build locally, set the URLs to a non-local address.

const fn non_empty(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

const fn or(value: Option<&'static str>, fallback: &'static str) -> &'static str {
    match non_empty(value) {
        Some(v) => v,
        None => fallback,
    }
}

/// The Kairo cloud API base URL.
pub const API_URL: &str = or(option_env!("KAIRO_API_URL"), "http://localhost:3000");
/// The web app, for links opened in the system browser.
pub const WEB_URL: &str = or(option_env!("KAIRO_WEB_URL"), "http://localhost:3001");
/// The audience API override, when set.
pub const AUDIENCE_API_URL_OVERRIDE: Option<&str> =
    non_empty(option_env!("KAIRO_AUDIENCE_API_URL"));
/// The licence verification public key, when set.
pub const LICENSE_PUBLIC_KEY: Option<&str> = non_empty(option_env!("KAIRO_LICENSE_PUBLIC_KEY"));

/// The production API, which has a second address to fall back to.
pub const PRODUCTION_API_URL: &str = "https://api.kairopresenter.com";
const PRODUCTION_API_FALLBACK_URL: &str = "https://kairo-api.alexander-joshua2018.workers.dev";

const fn eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn contains(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.len() > h.len() {
        return false;
    }
    let mut start = 0;
    while start + n.len() <= h.len() {
        let mut i = 0;
        while i < n.len() && (h[start + i] | 0x20) == (n[i] | 0x20) {
            i += 1;
        }
        if i == n.len() {
            return true;
        }
        start += 1;
    }
    false
}

const fn starts_with(text: &str, prefix: &str) -> bool {
    let (t, p) = (text.as_bytes(), prefix.as_bytes());
    if p.len() > t.len() {
        return false;
    }
    let mut i = 0;
    while i < p.len() {
        if t[i] != p[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether `url` may ship in a release: `https` and not a local address.
pub const fn is_release_url(url: &str) -> bool {
    starts_with(url, "https://")
        && !contains(url, "localhost")
        && !contains(url, "127.0.0.1")
        && !contains(url, "[::1]")
        && !contains(url, "0.0.0.0")
}

#[cfg(not(debug_assertions))]
const _: () = {
    assert!(
        is_release_url(API_URL),
        "KAIRO_API_URL must be an https, non-local URL in a release build"
    );
    assert!(
        is_release_url(WEB_URL),
        "KAIRO_WEB_URL must be an https, non-local URL in a release build"
    );
    if let Some(url) = AUDIENCE_API_URL_OVERRIDE {
        assert!(
            is_release_url(url),
            "KAIRO_AUDIENCE_API_URL must be an https, non-local URL in a release build"
        );
    }
};

/// `API_FALLBACK_URL`: the second address of the production API.
pub const fn api_fallback_url() -> Option<&'static str> {
    api_fallback_for(API_URL)
}

/// The production backup, only when the configured API is the production origin.
pub const fn api_fallback_for(api: &str) -> Option<&'static str> {
    if eq(api, PRODUCTION_API_URL) {
        Some(PRODUCTION_API_FALLBACK_URL)
    } else {
        None
    }
}

/// `AUDIENCE_API_BASE_URL`.
pub fn audience_api_base_url() -> String {
    kairo_domain::audience::audience_api_base_url(API_URL, AUDIENCE_API_URL_OVERRIDE)
}

/// `AUDIENCE_API_FALLBACK_URL`: the API fallback, when audience rooms use the API itself.
pub fn audience_api_fallback_url() -> Option<&'static str> {
    if audience_api_base_url() == API_URL {
        api_fallback_url()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_urls_must_be_https_and_remote() {
        assert!(is_release_url("https://api.kairopresenter.com"));
        for bad in [
            "http://api.kairopresenter.com",
            "https://localhost:3000",
            "https://LOCALHOST",
            "https://127.0.0.1:3000",
            "https://[::1]:3000",
            "https://0.0.0.0",
            "",
        ] {
            assert!(!is_release_url(bad), "{bad}");
        }
    }

    #[test]
    fn dev_defaults_match_env_example() {
        // Tests build with debug assertions; the dev defaults apply unless set.
        if option_env!("KAIRO_API_URL").is_none() {
            assert_eq!(API_URL, "http://localhost:3000");
            assert_eq!(audience_api_base_url(), "http://localhost:8787");
            assert_eq!(api_fallback_url(), None);
            assert_eq!(audience_api_fallback_url(), None);
        }
        if option_env!("KAIRO_WEB_URL").is_none() {
            assert_eq!(WEB_URL, "http://localhost:3001");
        }
    }

    #[test]
    fn helpers() {
        assert!(eq("abc", "abc") && !eq("abc", "abd") && !eq("a", "ab"));
        assert!(contains("xLocalHostx", "localhost") && !contains("lo", "localhost"));
        assert_eq!(or(Some(""), "d"), "d");
        assert_eq!(or(Some("v"), "d"), "v");
        assert_eq!(non_empty(None), None);
    }
}
