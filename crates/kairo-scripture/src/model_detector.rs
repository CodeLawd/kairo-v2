//! Managed scripture analysis. The queue and synchronous local detector remain
//! available while the network future is in flight.

use std::time::Duration;

use futures_util::future::BoxFuture;
use kairo_domain::{
    cloud::{HttpMethod, ManagedProvider, ManagedPurpose, ManagedRequest},
    js::is_whitespace,
};
use serde_json::{Value, json};
use tokio::{sync::watch, time::Instant};

use crate::{
    citation::{ExplicitCitation, canonical_book},
    live_detector::UnresolvedTranscript,
    verse_bounds::is_valid_scripture_reference,
};

const SYSTEM_PROMPT: &str = include_str!("detector_prompt.txt");
const MAX_RETRIES: u32 = 3;
const SDK_RETRIES: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelProvider {
    Anthropic,
    #[default]
    Deepseek,
}

impl ModelProvider {
    pub const fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-haiku-4-5",
            Self::Deepseek => "deepseek-flash",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Anthropic => Self::Deepseek,
            Self::Deepseek => Self::Anthropic,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub provider: ModelProvider,
    /// An explicit override, including an empty string, is sent as provided.
    pub model: Option<String>,
    pub max_tokens: u32,
    pub timeout: Duration,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: ModelProvider::default(),
            model: None,
            // The answer is a short JSON array; thinking is off (see
            // `model_request`), so no room is needed for a chain of thought.
            max_tokens: 1024,
            timeout: Duration::from_secs(15),
        }
    }
}

pub fn model_request(config: &ModelConfig, text: &str) -> ManagedRequest {
    let model = config
        .model
        .as_deref()
        .unwrap_or(config.provider.default_model());
    let user =
        format!("Detect Bible scripture references in this sermon transcript excerpt:\n\n{text}");
    let message = json!({"role": "user", "content": user});
    let (provider, path, body) = match config.provider {
        ModelProvider::Anthropic => (
            ManagedProvider::Anthropic,
            "/v1/messages",
            json!({
                "model": model, "max_tokens": config.max_tokens, "temperature": 0,
                "system": SYSTEM_PROMPT, "messages": [message],
            }),
        ),
        ModelProvider::Deepseek => (
            ManagedProvider::Deepseek,
            "/chat/completions",
            // Thinking is on by default and costs seconds per call (and
            // ignores `temperature`); spotting a reference does not need it.
            json!({
                "model": model, "max_tokens": config.max_tokens, "temperature": 0,
                "thinking": {"type": "disabled"},
                "messages": [{"role": "system", "content": SYSTEM_PROMPT}, message],
            }),
        ),
    };
    ManagedRequest {
        provider,
        path: path.to_owned(),
        method: HttpMethod::Post,
        body: Some(body),
        query: None,
        purpose: Some(ManagedPurpose::Scripture),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ModelError {
    pub status: u16,
    pub message: String,
}

impl ModelError {
    pub fn gateway(status: Option<u16>, message: String) -> Self {
        Self {
            status: status.filter(|status| *status >= 400).unwrap_or(503),
            message: if message.is_empty() {
                "Kairo could not complete the request".to_owned()
            } else {
                message
            },
        }
    }
}

pub trait ManagedDetectorTransport: Send + Sync {
    fn request(&self, request: ManagedRequest) -> BoxFuture<'_, Result<Value, ModelError>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectionType {
    Explicit,
    Partial,
    Quote,
    Paraphrase,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelDetection {
    pub reference: ExplicitCitation,
    pub detection_type: DetectionType,
    pub confidence: f64,
}

fn parse_response(raw: &str) -> Option<Vec<ModelDetection>> {
    let clean = raw.trim_matches(is_whitespace);
    let start = clean.find('[')?;
    let end = clean.rfind(']')?;
    if end <= start {
        return None;
    }
    let entries: Vec<Value> = serde_json::from_str(&clean[start..=end]).ok()?;
    Some(
        entries
            .iter()
            .filter_map(|entry| {
                let book = entry["book"].as_str()?;
                let chapter = entry["chapter"].as_f64()?;
                let verse_start = entry["verseStart"].as_f64()?;
                let verse_end = match &entry["verseEnd"] {
                    Value::Null => None,
                    end => Some(end.as_f64()?),
                };
                let confidence = entry["confidence"].as_f64()?.clamp(0.0, 1.0);
                let detection_type = match entry["detectionType"].as_str()? {
                    "partial" => DetectionType::Partial,
                    "quote" => DetectionType::Quote,
                    "paraphrase" => DetectionType::Paraphrase,
                    _ => DetectionType::Explicit,
                };
                let source_text =
                    crate::live_detector::utf16_prefix(entry["sourceText"].as_str()?, 150);
                if !is_valid_scripture_reference(book, chapter, verse_start, verse_end) {
                    return None;
                }
                let book = canonical_book(&book.to_ascii_lowercase())?;
                Some(ModelDetection {
                    reference: ExplicitCitation {
                        book,
                        chapter,
                        verse_start,
                        verse_end,
                        source_text,
                    },
                    detection_type,
                    confidence,
                })
            })
            .collect(),
    )
}

fn response_text(provider: ModelProvider, value: &Value) -> Result<String, ModelError> {
    let invalid = || ModelError {
        status: 502,
        message: "Kairo returned an invalid scripture response".to_owned(),
    };
    match provider {
        ModelProvider::Deepseek => {
            let choices = value["choices"].as_array().ok_or_else(invalid)?;
            let content = choices.first().map(|choice| &choice["message"]["content"]);
            match content {
                None | Some(Value::Null) => Ok("[]".to_owned()),
                Some(Value::String(content)) if content.trim().is_empty() => {
                    // Nothing said is "no references", not a broken answer.
                    let finish = choices.first().and_then(|c| c["finish_reason"].as_str());
                    if finish != Some("stop") {
                        tracing::warn!(?finish, "AI detection answered with no text");
                    }
                    Ok("[]".to_owned())
                }
                Some(Value::String(content)) => Ok(content.clone()),
                _ => Err(invalid()),
            }
        }
        ModelProvider::Anthropic => {
            let blocks = value["content"].as_array().ok_or_else(invalid)?;
            let text: String = blocks
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect();
            Ok(if text.trim().is_empty() {
                "[]".to_owned()
            } else {
                text
            })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelWork {
    pub transcript: UnresolvedTranscript,
    pub generation: u64,
}

/// Latest-pending coalescing and rate limiting independent of network execution.
pub struct ModelQueue {
    pending: Option<ModelWork>,
    last_started: Option<u64>,
    processing: bool,
    min_interval_ms: u64,
}

impl Default for ModelQueue {
    fn default() -> Self {
        Self::new(1_500)
    }
}

impl ModelQueue {
    pub fn new(min_interval_ms: u64) -> Self {
        Self {
            pending: None,
            last_started: None,
            processing: false,
            min_interval_ms,
        }
    }

    pub fn submit(&mut self, work: ModelWork) {
        if !work.transcript.text.trim_matches(is_whitespace).is_empty() {
            self.pending = Some(work);
        }
    }

    pub fn next_due_ms(&self) -> Option<u64> {
        if self.processing || self.pending.is_none() {
            return None;
        }
        Some(
            self.last_started
                .map_or(0, |last| last.saturating_add(self.min_interval_ms)),
        )
    }

    pub fn take_ready(&mut self, now: u64) -> Option<ModelWork> {
        if self.next_due_ms()? > now {
            return None;
        }
        self.processing = true;
        self.last_started = Some(now);
        self.pending.take()
    }

    /// Offline fallback drains the due item without recording a model start.
    pub fn take_fallback(&mut self, now: u64) -> Option<ModelWork> {
        if self.next_due_ms()? > now {
            return None;
        }
        self.pending.take()
    }

    pub fn complete(&mut self) {
        self.processing = false;
    }

    pub fn clear(&mut self) {
        self.pending = None;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelStats {
    pub total_calls: u64,
    pub errors: u64,
    latency_sum_ms: u128,
}

impl ModelStats {
    pub fn average_latency_ms(self) -> u128 {
        let calls = u128::from(self.total_calls);
        (self.latency_sum_ms + calls / 2)
            .checked_div(calls)
            .unwrap_or(0)
    }
}

#[derive(Debug, PartialEq)]
pub enum ModelOutcome {
    Parsed(Vec<ModelDetection>),
    InvalidResponse,
    Failed(ModelError),
    Cancelled,
}

pub struct ManagedModelDetector<T> {
    transport: T,
    config: ModelConfig,
    switched_provider: bool,
    stats: ModelStats,
}

impl<T: ManagedDetectorTransport> ManagedModelDetector<T> {
    pub fn new(transport: T, config: ModelConfig) -> Self {
        Self {
            transport,
            config,
            switched_provider: false,
            stats: ModelStats::default(),
        }
    }

    pub fn reconfigure(&mut self, config: ModelConfig) {
        self.config = config;
    }

    pub fn stats(&self) -> ModelStats {
        self.stats
    }

    pub async fn run(
        &mut self,
        work: &ModelWork,
        mut cancel: watch::Receiver<bool>,
    ) -> ModelOutcome {
        tokio::select! {
            biased;
            _ = cancel.wait_for(|cancelled| *cancelled) => ModelOutcome::Cancelled,
            result = self.run_active(&work.transcript.text) => result,
        }
    }

    async fn run_active(&mut self, text: &str) -> ModelOutcome {
        let start = Instant::now();
        for attempt in 0..=MAX_RETRIES {
            match self.call_model(text).await {
                Ok(response) => {
                    self.stats.total_calls += 1;
                    self.stats.latency_sum_ms += start.elapsed().as_millis();
                    if let Some(detections) = parse_response(&response) {
                        return ModelOutcome::Parsed(detections);
                    }
                    tracing::warn!(
                        attempt,
                        answer = crate::live_detector::utf16_prefix(&response, 300),
                        "AI detection answer is not a JSON array"
                    );
                    if attempt == MAX_RETRIES {
                        self.stats.errors += 1;
                        return ModelOutcome::InvalidResponse;
                    }
                }
                Err(error) => {
                    let rate_limited = error.message.to_ascii_lowercase().contains("rate limit")
                        || (self.config.provider == ModelProvider::Anthropic
                            && error.status == 429);
                    if rate_limited && attempt < MAX_RETRIES {
                        tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                    } else {
                        self.stats.errors += 1;
                        return ModelOutcome::Failed(error);
                    }
                }
            }
        }
        unreachable!("last model attempt always returns")
    }

    async fn call_model(&mut self, text: &str) -> Result<String, ModelError> {
        let result = self.call_provider(text).await;
        if result.as_ref().is_err_and(|error| {
            error.status == 503
                && error
                    .message
                    .to_ascii_lowercase()
                    .contains("not been configured")
        }) && !self.switched_provider
        {
            self.switched_provider = true;
            self.config.provider = self.config.provider.other();
            self.config.model = None;
            return self.call_provider(text).await;
        }
        result
    }

    async fn call_provider(&self, text: &str) -> Result<String, ModelError> {
        tokio::time::timeout(self.config.timeout, async {
            for attempt in 0..=SDK_RETRIES {
                let response = self
                    .transport
                    .request(model_request(&self.config, text))
                    .await;
                match response {
                    Ok(value) => return response_text(self.config.provider, &value),
                    Err(error)
                        if attempt < SDK_RETRIES
                            && (matches!(error.status, 408 | 409 | 429) || error.status >= 500) =>
                    {
                        // Both SDKs used 500/1,000 ms exponential delays
                        // with up to 25% downward jitter.
                        let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]);
                        let base = 500 * (1 << attempt);
                        tokio::time::sleep(Duration::from_millis(base - base * jitter / 1024))
                            .await;
                    }
                    Err(error) => return Err(error),
                }
            }
            unreachable!("last provider attempt always returns")
        })
        .await
        .unwrap_or_else(|_| {
            Err(ModelError {
                status: 408,
                message: "Request timed out".to_owned(),
            })
        })
    }
}
