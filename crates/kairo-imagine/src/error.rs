use serde_json::Value;

/// Why a generation or refinement failed, worded for the operator.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ImagineError {
    #[error("Connect your OpenAI account first.")]
    NotConnected,
    #[error("OpenAI didn't accept this API key. Check it at platform.openai.com/api-keys.")]
    BadKey,
    #[error(
        "OpenAI needs your organization verified before it makes images. Verify it in your OpenAI organization settings, then try again."
    )]
    NeedsVerification,
    #[error("OpenAI declined this prompt under its content policy. Try rewording it.")]
    Refused,
    #[error("{0}")]
    RateLimited(String),
    #[error("OpenAI took too long to answer. Try again.")]
    Timeout,
    #[error("Couldn't reach OpenAI. Check the internet connection.")]
    Network,
    #[error("OpenAI: {0}")]
    Api(String),
    #[error("Imagine couldn't save its files: {0}")]
    Storage(String),
    #[error("That image is no longer in Imagine.")]
    Missing,
    #[error("Cancelled.")]
    Cancelled,
}

impl From<std::io::Error> for ImagineError {
    fn from(e: std::io::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

impl ImagineError {
    /// Classifies an OpenAI failure from its status and `{"error": {...}}` body.
    pub fn from_response(status: u16, body: &str) -> Self {
        let error = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v.get("error").cloned())
            .unwrap_or(Value::Null);
        let field = |k: &str| {
            error
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        let (code, kind, message) = (field("code"), field("type"), field("message"));
        let lower = message.to_lowercase();
        if status == 401 || code == "invalid_api_key" {
            return Self::BadKey;
        }
        if code == "moderation_blocked"
            || code == "content_policy_violation"
            || lower.contains("safety system")
        {
            return Self::Refused;
        }
        if status == 403 && lower.contains("verif") {
            return Self::NeedsVerification;
        }
        if status == 429 || code == "insufficient_quota" || kind == "insufficient_quota" {
            return Self::RateLimited(
                if code == "insufficient_quota" || kind == "insufficient_quota" {
                    "Your OpenAI account is out of credit. Add credit at platform.openai.com, then try again.".into()
                } else {
                    "OpenAI is limiting requests right now. Wait a moment and try again.".into()
                },
            );
        }
        Self::Api(if message.is_empty() {
            format!("request failed (HTTP {status})")
        } else {
            message
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ImagineError as E;

    #[test]
    fn classifies_openai_failures() {
        assert_eq!(E::from_response(401, ""), E::BadKey);
        let blocked = r#"{"error":{"code":"moderation_blocked","message":"Your request was rejected by the safety system"}}"#;
        assert_eq!(E::from_response(400, blocked), E::Refused);
        let verify =
            r#"{"error":{"message":"Your organization must be verified to use the model"}}"#;
        assert_eq!(E::from_response(403, verify), E::NeedsVerification);
        let quota =
            r#"{"error":{"code":"insufficient_quota","type":"insufficient_quota","message":"x"}}"#;
        assert!(matches!(E::from_response(429, quota), E::RateLimited(m) if m.contains("credit")));
        assert_eq!(
            E::from_response(500, "oops"),
            E::Api("request failed (HTTP 500)".into())
        );
    }
}
