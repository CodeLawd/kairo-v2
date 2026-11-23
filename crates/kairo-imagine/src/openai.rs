//! The OpenAI images API behind [`ImageTransport`], so the managed Kairo
//! proxy can stand in for the operator's own key later without UI changes.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ImagineError;

pub const API: &str = "https://api.openai.com/v1";
/// 16:9 and divisible by 16, so a result fills the output without bars.
pub const OUTPUT_SIZE: &str = "1536x864";
/// Generation can take up to two minutes on OpenAI's side.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Which OpenAI image model makes and edits images. Stored by its model id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageModel {
    /// Fast iteration.
    #[default]
    #[serde(rename = "gpt-image-2.6-flare")]
    Flare,
    /// Detailed visual work.
    #[serde(rename = "gpt-image-2.6-sunburst")]
    Sunburst,
}

impl ImageModel {
    pub const ALL: [ImageModel; 2] = [ImageModel::Flare, ImageModel::Sunburst];

    /// The API model id.
    pub fn id(self) -> &'static str {
        match self {
            ImageModel::Flare => "gpt-image-2.6-flare",
            ImageModel::Sunburst => "gpt-image-2.6-sunburst",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ImageModel::Flare => "Fast",
            ImageModel::Sunburst => "Best quality",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Low, Quality::Medium, Quality::High];

    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Low => "low",
            Quality::Medium => "medium",
            Quality::High => "high",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
        }
    }
}

/// Rough US dollars for `images` landscape images at `quality`.
/// shortcut: per-image figures from OpenAI's earlier gpt-image price table;
/// the 2.6 models bill per token and publish no per-image figure. Replace
/// with token-based cost once the managed tier reports real usage.
pub fn estimate_usd(quality: Quality, images: usize) -> f64 {
    let each = match quality {
        Quality::Low => 0.006,
        Quality::Medium => 0.05,
        Quality::High => 0.2,
    };
    each * images as f64
}

/// One image to make. With `reference`, the image is an edit of it.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRequest {
    pub model: ImageModel,
    pub prompt: String,
    pub quality: Quality,
    pub reference: Option<Vec<u8>>,
}

/// Where image requests go: the operator's own key now, Kairo's managed
/// proxy later.
pub trait ImageTransport: Send + Sync {
    /// The image bytes exactly as OpenAI returned them.
    fn image(&self, request: ImageRequest) -> BoxFuture<'_, Result<Vec<u8>, ImagineError>>;

    /// A `/v1/responses` call, for the assistant's plan.
    fn respond(&self, body: Value) -> BoxFuture<'_, Result<Value, ImagineError>> {
        let _ = body;
        Box::pin(async { Err(ImagineError::Api("this connection can't plan".into())) })
    }

    /// The same call streamed: `thought` hears reasoning-summary text as it
    /// arrives; the answer is the completed response. Without streaming,
    /// this is [`ImageTransport::respond`].
    fn respond_streaming<'a>(
        &'a self,
        body: Value,
        thought: &'a (dyn Fn(&str) + Send + Sync),
    ) -> BoxFuture<'a, Result<Value, ImagineError>> {
        let _ = thought;
        self.respond(body)
    }
}

/// Splits server-sent events out of `buffer` (which keeps any partial
/// event) and returns each complete event's JSON `data`. Bytes, not text:
/// a character can be split across two network chunks, so only whole
/// events are decoded.
pub fn take_events(buffer: &mut Vec<u8>) -> Vec<Value> {
    let mut events = Vec::new();
    while let Some(end) = buffer.windows(2).position(|w| w == b"\n\n") {
        let raw: Vec<u8> = buffer.drain(..end + 2).collect();
        let block = String::from_utf8_lossy(&raw);
        let data: String = block
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if let Ok(value) = serde_json::from_str(&data) {
            events.push(value);
        }
    }
    events
}

/// What one streamed event means for the caller.
#[derive(Debug, PartialEq)]
pub enum StreamEvent {
    Thought(String),
    Done(Value),
    Failed(ImagineError),
    Other,
}

pub fn classify_event(event: &Value) -> StreamEvent {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    let message = |v: &Value| {
        v.pointer("/response/error/message")
            .or_else(|| v.pointer("/error/message"))
            .or_else(|| v.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("the assistant stopped")
            .to_owned()
    };
    if kind.ends_with("reasoning_summary_part.added")
        && event
            .get("summary_index")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
    {
        // A new section of the summary: keep it apart from the last.
        StreamEvent::Thought("\n\n".into())
    } else if kind.ends_with("reasoning_summary_text.delta") {
        let delta = event.get("delta").and_then(Value::as_str).unwrap_or("");
        StreamEvent::Thought(delta.to_owned())
    } else if kind == "response.completed" {
        StreamEvent::Done(event.get("response").cloned().unwrap_or(Value::Null))
    } else if kind == "error" || kind == "response.failed" || kind == "response.incomplete" {
        StreamEvent::Failed(ImagineError::Api(message(event)))
    } else {
        StreamEvent::Other
    }
}

/// The `/v1/images/generations` or `/v1/images/edits` body.
pub fn request_body(request: &ImageRequest) -> Value {
    let mut body = json!({
        "model": request.model.id(),
        "prompt": request.prompt,
        "size": OUTPUT_SIZE,
        "quality": request.quality.as_str(),
        "n": 1,
    });
    if let Some(png) = &request.reference {
        body["images"] = json!([{
            "image_url": format!("data:image/png;base64,{}", STANDARD.encode(png))
        }]);
    }
    body
}

pub fn endpoint(request: &ImageRequest) -> String {
    let path = if request.reference.is_some() {
        "images/edits"
    } else {
        "images/generations"
    };
    format!("{API}/{path}")
}

/// The first image in a successful response, base64-decoded and otherwise
/// untouched.
pub fn parse_image(body: &str) -> Result<Vec<u8>, ImagineError> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| ImagineError::Api("unreadable response".into()))?;
    let b64 = value
        .pointer("/data/0/b64_json")
        .and_then(Value::as_str)
        .ok_or_else(|| ImagineError::Api("the response had no image".into()))?;
    STANDARD
        .decode(b64)
        .map_err(|_| ImagineError::Api("the image data was corrupt".into()))
}

/// Calls OpenAI with the operator's own API key.
pub struct OwnKeyTransport {
    key: String,
    http: reqwest::Client,
}

impl OwnKeyTransport {
    pub fn new(key: String) -> Self {
        let http = kairo_net::transport::client_builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self { key, http }
    }

    /// Checks the key with `GET /v1/models`.
    pub async fn verify(&self) -> Result<(), ImagineError> {
        let response = self
            .http
            .get(format!("{API}/models"))
            .bearer_auth(&self.key)
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(ImagineError::from_response(status, &body))
    }
}

fn transport_error(e: reqwest::Error) -> ImagineError {
    if e.is_timeout() {
        ImagineError::Timeout
    } else {
        ImagineError::Network
    }
}

impl ImageTransport for OwnKeyTransport {
    fn image(&self, request: ImageRequest) -> BoxFuture<'_, Result<Vec<u8>, ImagineError>> {
        Box::pin(async move {
            let response = self
                .http
                .post(endpoint(&request))
                .bearer_auth(&self.key)
                .json(&request_body(&request))
                .send()
                .await
                .map_err(transport_error)?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(transport_error)?;
            if !(200..300).contains(&status) {
                return Err(ImagineError::from_response(status, &body));
            }
            parse_image(&body)
        })
    }

    fn respond(&self, body: Value) -> BoxFuture<'_, Result<Value, ImagineError>> {
        Box::pin(async move {
            let response = self
                .http
                .post(format!("{API}/responses"))
                .bearer_auth(&self.key)
                .json(&body)
                .send()
                .await
                .map_err(transport_error)?;
            let status = response.status().as_u16();
            let text = response.text().await.map_err(transport_error)?;
            if !(200..300).contains(&status) {
                return Err(ImagineError::from_response(status, &text));
            }
            serde_json::from_str(&text).map_err(|_| ImagineError::Api("unreadable response".into()))
        })
    }

    fn respond_streaming<'a>(
        &'a self,
        mut body: Value,
        thought: &'a (dyn Fn(&str) + Send + Sync),
    ) -> BoxFuture<'a, Result<Value, ImagineError>> {
        Box::pin(async move {
            use futures_util::StreamExt;
            body["stream"] = Value::Bool(true);
            let response = self
                .http
                .post(format!("{API}/responses"))
                .bearer_auth(&self.key)
                .json(&body)
                .send()
                .await
                .map_err(transport_error)?;
            let status = response.status().as_u16();
            if !(200..300).contains(&status) {
                let text = response.text().await.unwrap_or_default();
                return Err(ImagineError::from_response(status, &text));
            }
            let mut chunks = response.bytes_stream();
            let mut buffer: Vec<u8> = Vec::new();
            while let Some(chunk) = chunks.next().await {
                let chunk = chunk.map_err(transport_error)?;
                buffer.extend_from_slice(&chunk);
                for event in take_events(&mut buffer) {
                    match classify_event(&event) {
                        StreamEvent::Thought(text) => thought(&text),
                        StreamEvent::Done(response) => return Ok(response),
                        StreamEvent::Failed(error) => return Err(error),
                        StreamEvent::Other => {}
                    }
                }
            }
            Err(ImagineError::Api(
                "the assistant's answer was cut off".into(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_has_no_reference_and_no_response_format() {
        let request = ImageRequest {
            model: ImageModel::default(),
            prompt: "sunrise".into(),
            quality: Quality::High,
            reference: None,
        };
        let body = request_body(&request);
        assert_eq!(
            endpoint(&request),
            "https://api.openai.com/v1/images/generations"
        );
        assert_eq!(body["quality"], "high");
        assert_eq!(body["size"], OUTPUT_SIZE);
        assert!(body.get("images").is_none());
        assert!(body.get("response_format").is_none());
        assert_eq!(body["model"], "gpt-image-2.6-flare");
    }

    #[test]
    fn models_have_their_api_ids_and_flare_is_the_default() {
        assert_eq!(ImageModel::default(), ImageModel::Flare);
        assert_eq!(ImageModel::Sunburst.id(), "gpt-image-2.6-sunburst");
        for m in ImageModel::ALL {
            assert_eq!(ImageModel::from_id(m.id()), Some(m));
            assert_eq!(serde_json::to_value(m).unwrap(), m.id());
        }
        assert_eq!(ImageModel::from_id("gpt-image-2"), None);
    }

    #[test]
    fn refinement_is_an_edit_of_the_reference() {
        let request = ImageRequest {
            model: ImageModel::Sunburst,
            prompt: "warmer".into(),
            quality: Quality::Medium,
            reference: Some(vec![1, 2, 3]),
        };
        assert_eq!(endpoint(&request), "https://api.openai.com/v1/images/edits");
        assert_eq!(request_body(&request)["model"], "gpt-image-2.6-sunburst");
        assert_eq!(
            request_body(&request)["images"][0]["image_url"],
            "data:image/png;base64,AQID"
        );
    }

    #[test]
    fn parses_the_image_bytes_verbatim() {
        assert_eq!(
            parse_image(r#"{"data":[{"b64_json":"AQID"}]}"#).unwrap(),
            vec![1, 2, 3]
        );
        assert!(parse_image(r#"{"data":[]}"#).is_err());
    }

    #[test]
    fn streamed_events_are_split_and_classified() {
        let mut buffer = Vec::from(
            "event: response.reasoning_summary_text.delta\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"Planning\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\"}}\n\ndata: {\"type\":\"resp",
        );
        let events = take_events(&mut buffer);
        assert_eq!(events.len(), 2);
        assert_eq!(buffer, b"data: {\"type\":\"resp", "a partial event waits");
        // A character split across two network chunks arrives whole.
        let quote =
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"I’m\"}\n\n"
                .as_bytes();
        let cut = quote.iter().position(|b| *b == 0xE2).unwrap() + 1;
        let mut split = quote[..cut].to_vec();
        assert!(take_events(&mut split).is_empty());
        split.extend_from_slice(&quote[cut..]);
        assert_eq!(
            classify_event(&take_events(&mut split)[0]),
            StreamEvent::Thought("I’m".into())
        );
        // A new summary section starts on its own paragraph.
        assert_eq!(
            classify_event(
                &json!({"type": "response.reasoning_summary_part.added", "summary_index": 1})
            ),
            StreamEvent::Thought("\n\n".into())
        );
        assert_eq!(
            classify_event(&events[0]),
            StreamEvent::Thought("Planning".into())
        );
        assert_eq!(
            classify_event(&events[1]),
            StreamEvent::Done(json!({"id": "r"}))
        );
        assert!(matches!(
            classify_event(&json!({"type": "error", "message": "boom"})),
            StreamEvent::Failed(ImagineError::Api(m)) if m == "boom"
        ));
        assert_eq!(
            classify_event(&json!({"type": "response.output_text.delta"})),
            StreamEvent::Other
        );
    }

    #[test]
    fn estimate_scales_with_count() {
        assert!(estimate_usd(Quality::High, 2) > estimate_usd(Quality::Medium, 2));
        assert_eq!(estimate_usd(Quality::Low, 0), 0.0);
    }
}
