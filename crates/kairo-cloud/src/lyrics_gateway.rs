//! The managed gateway for online lyrics: web search, page review and
//! translation all go through Kairo's server with the session's licence.

use std::{future::Future, pin::Pin};

use kairo_domain::cloud::ManagedRequest;
use kairo_lyrics::online::http::{GatewayError, ManagedGateway};
use serde_json::Value;

use crate::licensing::LicensingService;

impl ManagedGateway for LicensingService {
    fn request(
        &self,
        request: ManagedRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Value, GatewayError>> + Send + '_>> {
        Box::pin(async move {
            LicensingService::request::<Value>(self, &request)
                .await
                .map_err(|error| GatewayError {
                    status: error.status,
                    message: error.message,
                })
        })
    }
}
