//! The audience and Engage handlers of `src/main/ipc/index.ts`.

use crate::kairo::operator_post;
use crate::source::BoxFuture;
use crate::{
    AudienceError, AudienceService, KairoAudienceSource, Result, RoomFetcher, SampleAudienceSource,
    slice_utf16,
};
use kairo_domain::audience::{
    AudienceRoomCreated, AudienceSourceOptions, AudienceState, EngagementIndex, EngagementInput,
    EngagementStatus, is_audience_code, normalize_audience_room_settings,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngageMethod {
    Get,
    Post,
}

/// `cloudSession.engageRequest`: the signed-in account's Engage calls.
pub trait EngageApi: Send + Sync {
    fn engage_request(
        &self,
        method: EngageMethod,
        path: &str,
        body: Option<Value>,
    ) -> BoxFuture<'static, Result<Value>>;
}

impl EngageApi for kairo_cloud::CloudSession {
    fn engage_request(
        &self,
        method: EngageMethod,
        path: &str,
        body: Option<Value>,
    ) -> BoxFuture<'static, Result<Value>> {
        let session = self.clone();
        let path = path.to_owned();
        Box::pin(async move {
            let method = match method {
                EngageMethod::Get => reqwest::Method::GET,
                EngageMethod::Post => reqwest::Method::POST,
            };
            session
                .engage_request::<Value>(method, &path, body)
                .await
                .map_err(|error| AudienceError::new(error.message))
        })
    }
}

/// Sample questions: development builds with `KAIRO_AUDIENCE_SAMPLE=1` only.
pub fn sample_allowed(packaged: bool) -> bool {
    !packaged && std::env::var("KAIRO_AUDIENCE_SAMPLE").as_deref() == Ok("1")
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|error| AudienceError::new(error.to_string()))
}

fn valid_code(code: &str) -> Result<()> {
    if is_audience_code(code) {
        Ok(())
    } else {
        Err(AudienceError::new("Choose a valid engagement."))
    }
}

#[derive(Clone)]
pub struct EngageFlows {
    service: AudienceService,
    api: Arc<dyn EngageApi>,
    base_url: String,
    fallback_url: Option<String>,
    sample_available: bool,
    slido: Option<Arc<dyn crate::SlidoWindowFactory>>,
    http: reqwest::Client,
}

impl EngageFlows {
    /// `base_url` is `AUDIENCE_API_BASE_URL`, `fallback_url`
    /// `AUDIENCE_API_FALLBACK_URL` (see `kairo_store::build_config`).
    pub fn new(
        service: AudienceService,
        api: Arc<dyn EngageApi>,
        base_url: impl Into<String>,
        fallback_url: Option<String>,
        sample_available: bool,
    ) -> Self {
        Self {
            service,
            api,
            base_url: base_url.into(),
            fallback_url,
            sample_available,
            slido: None,
            http: reqwest::Client::new(),
        }
    }

    /// The window host Slido sources open their sign-in window with. Without
    /// one, `audience:start` for `slido` reports the source as unavailable.
    pub fn with_slido(mut self, factory: Arc<dyn crate::SlidoWindowFactory>) -> Self {
        self.slido = Some(factory);
        self
    }

    fn resume_fetcher(&self, code: String) -> RoomFetcher {
        let api = self.api.clone();
        Arc::new(move || {
            let request = api.engage_request(
                EngageMethod::Post,
                &format!("/{code}/resume"),
                Some(json!({})),
            );
            Box::pin(async move { decode(request.await?) })
        })
    }

    async fn resume_room(&self, code: &str) -> Result<AudienceRoomCreated> {
        (self.resume_fetcher(code.to_owned()))().await
    }

    /// `audience:engagements`.
    pub async fn list(&self) -> Result<Vec<EngagementIndex>> {
        decode(self.api.engage_request(EngageMethod::Get, "", None).await?)
    }

    /// `audience:createEngagement`.
    pub async fn create(&self, input: &EngagementInput) -> Result<()> {
        let body = serde_json::to_value(input).map_err(|e| AudienceError::new(e.to_string()))?;
        self.api
            .engage_request(EngageMethod::Post, "", Some(body))
            .await?;
        Ok(())
    }

    /// `audience:resume`. A draft opens on resume; `reopen` opens a closed one.
    pub async fn resume(&self, code: &str, reopen: bool) -> Result<AudienceState> {
        valid_code(code)?;
        let room = self.resume_room(code).await?;
        let draft = room
            .engagement
            .as_ref()
            .is_some_and(|e| e.status == EngagementStatus::Draft);
        if reopen || draft {
            let (ok, text) = operator_post(
                &self.http,
                &self.base_url,
                code,
                "open",
                &room.operator_token,
                &json!({}),
            )
            .await?;
            if !ok {
                return Err(AudienceError::new(slice_utf16(&text, 300)));
            }
        }
        self.service.detach_engagement();
        let created: RoomFetcher = Arc::new(move || {
            let room = room.clone();
            Box::pin(async move { Ok(room) })
        });
        self.service
            .start(Arc::new(KairoAudienceSource::new(
                self.base_url.clone(),
                created,
                self.fallback_url.clone(),
                Some(self.resume_fetcher(code.to_owned())),
            )))
            .await
    }

    /// `audience:reopenEngagement`.
    pub async fn reopen(&self) -> Result<AudienceState> {
        self.service.manage("open", json!({})).await
    }

    /// `audience:deleteEngagement`.
    pub async fn delete(&self, code: &str) -> Result<()> {
        valid_code(code)?;
        let room = self.resume_room(code).await?;
        let (ok, _) = operator_post(
            &self.http,
            &self.base_url,
            code,
            "delete",
            &room.operator_token,
            &json!({}),
        )
        .await?;
        if !ok {
            return Err(AudienceError::new("Could not delete the engagement."));
        }
        if self.service.state().session.is_some_and(|s| s.code == code) {
            self.service.detach_engagement();
        }
        Ok(())
    }

    /// `audience:invite`.
    pub async fn invite(&self, email: &str, name: &str) -> Result<AudienceState> {
        self.service
            .manage("moderators", json!({ "email": email, "name": name }))
            .await
    }

    /// `audience:revoke`.
    pub async fn revoke(&self, moderator_id: &str) -> Result<AudienceState> {
        self.service
            .manage(&format!("revoke/{moderator_id}"), json!({}))
            .await
    }

    /// `audience:start`.
    pub async fn start(
        &self,
        kind: &str,
        options: &AudienceSourceOptions,
    ) -> Result<AudienceState> {
        match kind {
            "kairo" => {
                let title = options
                    .title
                    .as_deref()
                    .map(|t| slice_utf16(kairo_domain::js::trim(t), 100))
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "Questions from the room".into());
                let settings = normalize_audience_room_settings(
                    &options
                        .settings
                        .as_ref()
                        .and_then(|s| serde_json::to_value(s).ok())
                        .unwrap_or_default(),
                );
                let mut body = json!({ "title": title, "settings": settings });
                // `JSON.stringify` drops undefined and keeps null.
                if let Some(starts_at) = options.starts_at {
                    body["startsAt"] = json!(starts_at);
                }
                if let Some(ends_at) = options.ends_at {
                    body["endsAt"] = json!(ends_at);
                }
                let created: Arc<Mutex<Option<String>>> = Arc::default();
                let api = self.api.clone();
                let remember = created.clone();
                let create: RoomFetcher = Arc::new(move || {
                    let request = api.engage_request(EngageMethod::Post, "", Some(body.clone()));
                    let remember = remember.clone();
                    Box::pin(async move {
                        let room: AudienceRoomCreated = decode(request.await?)?;
                        *crate::lock(&remember) = Some(room.code.clone());
                        Ok(room)
                    })
                });
                let api = self.api.clone();
                let refresh: RoomFetcher = Arc::new(move || {
                    let code = crate::lock(&created).clone();
                    let request = code.map(|code| {
                        api.engage_request(
                            EngageMethod::Post,
                            &format!("/{code}/resume"),
                            Some(json!({})),
                        )
                    });
                    Box::pin(async move {
                        match request {
                            Some(request) => decode(request.await?),
                            None => Err(AudienceError::new("No engagement to reconnect.")),
                        }
                    })
                });
                self.service
                    .start(Arc::new(KairoAudienceSource::new(
                        self.base_url.clone(),
                        create,
                        self.fallback_url.clone(),
                        Some(refresh),
                    )))
                    .await
            }
            "slido" => {
                let event = kairo_domain::audience::parse_slido_event(
                    options.code.as_deref().unwrap_or(""),
                )
                .ok_or_else(|| {
                    AudienceError::new("That doesn’t look like a Slido code or link.")
                })?;
                let factory = self
                    .slido
                    .clone()
                    .ok_or_else(|| AudienceError::new("That audience source is not available."))?;
                self.service
                    .start(Arc::new(crate::SlidoAudienceSource::new(
                        event,
                        options.moderator == Some(true),
                        options.skip_review == Some(true),
                        factory,
                    )?))
                    .await
            }
            "sample" if self.sample_available => {
                self.service
                    .start(Arc::new(SampleAudienceSource::new()))
                    .await
            }
            "sample" => Err(AudienceError::new(
                "Sample questions require KAIRO_AUDIENCE_SAMPLE=1 in development.",
            )),
            _ => Err(AudienceError::new("That audience source is not available.")),
        }
    }
}
