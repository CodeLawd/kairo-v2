//! Desktop account session and managed Kairo API requests.

pub mod licensing;
pub mod managed;
pub mod onboarding;
mod profile;
pub mod sermon_upload;
pub mod service_records;
mod session;
pub mod telemetry;

pub use kairo_net::ApiError;
pub use session::{ChurchNameSeeded, CloudConfig, CloudSession, OpenBrowser, OrgProfilePatch};
