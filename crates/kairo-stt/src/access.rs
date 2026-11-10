//! Short-lived transcription grants from the licence server.

use kairo_domain::cloud::{LicensedFeature, TranscriptionGrant};
use kairo_net::ApiError;
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// `TranscriptionAccess` in `deepgram.ts`, plus `assertFeature('transcription')`.
pub trait TranscriptionAccess: Send + Sync + 'static {
    fn transcription(&self) -> BoxFuture<'_, Result<TranscriptionGrant, ApiError>>;
    fn renew_transcription(
        &self,
        id: String,
    ) -> BoxFuture<'_, Result<TranscriptionGrant, ApiError>>;
    fn release_transcription(&self, id: String) -> BoxFuture<'_, Result<(), ApiError>>;
    fn assert_transcription(&self) -> BoxFuture<'_, Result<(), ApiError>>;
}

impl TranscriptionAccess for kairo_cloud::licensing::LicensingService {
    fn transcription(&self) -> BoxFuture<'_, Result<TranscriptionGrant, ApiError>> {
        Box::pin(kairo_cloud::licensing::LicensingService::transcription(
            self,
        ))
    }

    fn renew_transcription(
        &self,
        id: String,
    ) -> BoxFuture<'_, Result<TranscriptionGrant, ApiError>> {
        Box::pin(async move {
            kairo_cloud::licensing::LicensingService::renew_transcription(self, &id).await
        })
    }

    fn release_transcription(&self, id: String) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async move {
            kairo_cloud::licensing::LicensingService::release_transcription(self, &id).await
        })
    }

    fn assert_transcription(&self) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(self.assert_feature(LicensedFeature::Transcription))
    }
}
