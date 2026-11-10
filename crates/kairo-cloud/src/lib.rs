//! Desktop account session and managed Kairo API requests.

pub mod bible_gateway;
pub mod licensing;
mod lyrics_gateway;
pub mod managed;
pub mod onboarding;
mod profile;
mod scripture_gateway;
pub mod sermon_upload;
pub mod service_records;
mod session;
pub mod telemetry;

pub use kairo_net::ApiError;
pub use session::{ChurchNameSeeded, CloudConfig, CloudSession, OpenBrowser, OrgProfilePatch};
