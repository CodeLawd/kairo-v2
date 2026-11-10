//! The detector's managed transport over the signed-in licensing session.

use std::{future::Future, pin::Pin};

use kairo_domain::cloud::ManagedRequest;
use kairo_scripture::model_detector::{ManagedDetectorTransport, ModelError};
use serde_json::Value;

use crate::licensing::LicensingService;

impl ManagedDetectorTransport for LicensingService {
    fn request(
        &self,
        request: ManagedRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Value, ModelError>> + Send + '_>> {
        Box::pin(async move {
            self.request(&request)
                .await
                .map_err(|error| ModelError::gateway(error.status, error.message))
        })
    }
}
