//! Auto-update from GitHub Releases, ported from `src/main/services/updater/`.
//!
//! Reads the same electron-builder feeds (`latest-mac.yml`, `latest.yml`,
//! `latest-linux.yml`) from `CodeLawd/kairo-presenter`, verifies the SHA-512
//! of every download before it can be installed, and never downloads until
//! the operator asks. See `native-migration/DECISIONS.md` (048).

mod download;
mod errors;
pub mod feed;
pub mod install;
mod service;

pub use download::{client, download_verified};
pub use errors::{Operation, update_error_message};
pub use service::{
    BoxFuture, InstallOutcome, UpdaterConfig, UpdaterService, is_newer, simulation_requested,
};
