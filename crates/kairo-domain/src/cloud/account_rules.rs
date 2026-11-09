use super::{
    CloudSessionState, DevicePollOutcome, SessionDescription, SessionSnapshot, SessionTone,
    SignUpInput,
};
use regex::Regex;
use std::sync::LazyLock;

pub fn is_signed_in(session: &SessionSnapshot) -> bool {
    session.state != CloudSessionState::SignedOut
}

pub fn needs_email_confirmation(session: &SessionSnapshot) -> bool {
    is_signed_in(session)
        && session
            .user
            .as_ref()
            .is_some_and(|user| !user.email_verified)
}

pub fn can_enter_app(session: &SessionSnapshot) -> bool {
    is_signed_in(session) && !needs_email_confirmation(session)
}

pub fn should_offer_account_gate(session: &SessionSnapshot) -> bool {
    !can_enter_app(session)
}

pub fn describe_session_state(session: &SessionSnapshot) -> SessionDescription {
    match session.state {
        CloudSessionState::SignedOut => SessionDescription {
            label: "Signed out".into(),
            detail: "Sign in to use Kairo on this machine.".into(),
            tone: SessionTone::Neutral,
        },
        CloudSessionState::Stale => SessionDescription {
            label: "Offline".into(),
            detail: "Signed in, but Kairo cannot be reached. Nothing is interrupted.".into(),
            tone: SessionTone::Warn,
        },
        CloudSessionState::Active => SessionDescription {
            label: session
                .org
                .as_ref()
                .map_or_else(|| "Signed in".into(), |org| org.name.clone()),
            detail: session.user.as_ref().map_or_else(
                || "Signed in".into(),
                |user| format!("Signed in as {}", user.email),
            ),
            tone: SessionTone::Good,
        },
    }
}

pub const MIN_PASSWORD_LENGTH: usize = 10;

pub fn validate_sign_up(input: &SignUpInput) -> Option<&'static str> {
    static EMAIL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[^@\s]+@[^@\s]+\.[^@\s]+$").unwrap());
    if crate::js::trim(&input.name).is_empty() {
        return Some("Enter your name");
    }
    if crate::js::trim(&input.org_name).is_empty() {
        return Some("Enter your church name");
    }
    if !EMAIL.is_match(crate::js::trim(&input.email)) {
        return Some("Enter a valid email address");
    }
    if input.password.encode_utf16().count() < MIN_PASSWORD_LENGTH {
        return Some("Password must be at least 10 characters");
    }
    None
}

pub const USER_CODE_ALPHABET: &str = "BCDFGHJKMNPQRTVWXY2346789";
pub const USER_CODE_LENGTH: usize = 8;
const USER_CODE_PREFIX: &str = "PROA";
pub const DEVICE_POLL_INTERVAL_SEC: f64 = 5.0;

fn stripped_code(raw: &str) -> String {
    raw.to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

pub fn format_user_code(raw: &str) -> String {
    let mut body = stripped_code(raw);
    body.truncate(USER_CODE_LENGTH);
    format!(
        "{USER_CODE_PREFIX}-{}",
        body.strip_prefix(USER_CODE_PREFIX).unwrap_or(&body)
    )
}

pub fn normalize_user_code(input: &str) -> String {
    format_user_code(input)
}

pub fn is_valid_user_code(input: &str) -> bool {
    let stripped = stripped_code(input);
    let body = stripped.strip_prefix(USER_CODE_PREFIX).unwrap_or(&stripped);
    body.len() == USER_CODE_LENGTH - USER_CODE_PREFIX.len()
        && body.chars().all(|c| USER_CODE_ALPHABET.contains(c))
}

pub fn user_code_from_bytes(bytes: [u8; 4]) -> String {
    let alphabet = USER_CODE_ALPHABET.as_bytes();
    let body: String = bytes
        .into_iter()
        .map(|byte| char::from(alphabet[usize::from(byte) % alphabet.len()]))
        .collect();
    format!("{USER_CODE_PREFIX}-{body}")
}

pub fn next_poll_delay_sec(outcome: DevicePollOutcome, current_sec: Option<f64>) -> f64 {
    let current = current_sec.unwrap_or(DEVICE_POLL_INTERVAL_SEC);
    if outcome == DevicePollOutcome::SlowDown {
        crate::js::math_min(current + DEVICE_POLL_INTERVAL_SEC, 30.0)
    } else {
        current
    }
}

pub fn is_terminal_outcome(outcome: DevicePollOutcome) -> bool {
    matches!(
        outcome,
        DevicePollOutcome::Approved | DevicePollOutcome::Denied | DevicePollOutcome::Expired
    )
}

pub fn safe_return_path(value: Option<&str>, fallback: &str) -> String {
    let resolve = || {
        let value = value.filter(|value| {
            value.starts_with('/')
                && !value.starts_with("//")
                && value.encode_utf16().count() <= 1500
        })?;
        let mut bytes = value.as_bytes().iter();
        while let Some(byte) = bytes.next() {
            if *byte == b'%'
                && bytes
                    .by_ref()
                    .take(2)
                    .filter(|byte| byte.is_ascii_hexdigit())
                    .count()
                    != 2
            {
                return None;
            }
        }
        let decoded = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .ok()?;
        if decoded.starts_with("//")
            || decoded.contains('\\')
            || decoded.chars().any(|c| c <= ' ' || c == '\u{7f}')
        {
            return None;
        }
        let base = url::Url::parse("https://kairo.invalid").ok()?;
        let url = base.join(value).ok()?;
        (url.origin() == base.origin()).then(|| url[url::Position::BeforePath..].to_owned())
    };
    resolve().unwrap_or_else(|| fallback.into())
}

fn normalize_separators(value: &str, separators: &[char]) -> String {
    let mut previous = false;
    value
        .chars()
        .filter_map(|c| {
            let separator = separators.contains(&c);
            let output = if separator && previous {
                None
            } else if separator {
                Some(' ')
            } else {
                Some(c)
            };
            previous = separator;
            output
        })
        .collect::<String>()
        .to_lowercase()
}

pub fn normalize_zone(zone: &str) -> String {
    normalize_separators(zone, &['_', '/', '-'])
}

pub fn zone_city(zone: &str) -> String {
    normalize_separators(zone.rsplit('/').next().unwrap_or(zone), &['_', '-'])
}

pub fn match_timezones<'a>(zones: &'a [String], query: &str, limit: usize) -> Vec<&'a str> {
    let query = normalize_zone(query);
    let query = query
        .split(crate::js::is_whitespace)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if query.is_empty() {
        return zones.iter().take(limit).map(String::as_str).collect();
    }
    let mut ranked: Vec<_> = zones
        .iter()
        .filter_map(|zone| {
            if zone_city(zone).starts_with(&query) {
                return Some((0, zone));
            }
            let normalized = normalize_zone(zone);
            if normalized.split(' ').any(|word| word.starts_with(&query)) {
                Some((1, zone))
            } else {
                normalized.contains(&query).then_some((2, zone))
            }
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, zone)| zone.as_str())
        .collect()
}
