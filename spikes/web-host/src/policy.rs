//! The Slido navigation rule from `allowedSlidoNavigation`.

const HOSTS: [&str; 5] = [
    "sli.do",
    "slido.com",
    "accounts.google.com",
    "cisco.com",
    "webex.com",
];

/// True for https addresses on an allowed host or its subdomains. `test_origin`
/// admits one exact `http://host:port` fixture origin for local proofs.
pub fn allowed(address: &str, test_origin: Option<&str>) -> bool {
    if let Some(origin) = test_origin
        && (address == origin || address.starts_with(&format!("{origin}/")))
    {
        return true;
    }
    let Some(rest) = address.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = authority
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

#[cfg(test)]
mod tests {
    use super::allowed;

    #[test]
    fn matches_the_electron_rule() {
        assert!(allowed("https://app.sli.do/event/x", None));
        assert!(allowed("https://accounts.google.com/o", None));
        assert!(allowed("https://id.webex.com", None));
        assert!(!allowed("http://app.sli.do/", None));
        assert!(!allowed("https://evilsli.do/", None));
        assert!(!allowed("https://sli.do.evil.com/", None));
        assert!(!allowed("https://sli.do@evil.com/", None));
        assert!(!allowed("file:///etc/passwd", None));
        assert!(!allowed("", None));
    }

    #[test]
    fn test_origin_is_exact() {
        let origin = Some("http://127.0.0.1:8765");
        assert!(allowed("http://127.0.0.1:8765/a", origin));
        assert!(!allowed("http://127.0.0.1:87650/a", origin));
    }
}
