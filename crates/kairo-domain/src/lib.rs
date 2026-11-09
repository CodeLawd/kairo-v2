//! Kairo's data types: everything persisted to disk or exchanged between
//! processes and with the server. A port of the types in the Electron app's
//! `src/lib` (prompt 012).
//!
//! Rules every type follows, recorded in `native-migration/DECISIONS.md`:
//!
//! - JSON names match TypeScript exactly.
//! - Optional fields are absent, never `null`; `T | null` fields must be
//!   present; see [`serde_util`] for the mapping.
//! - Every TypeScript `number` is an `f64`.
//! - Persisted objects keep unknown keys in a flattened `extra` map, so data
//!   written by an older or newer version survives a load and save.
//! - Nothing is normalised on read. Normalisers are prompt 013.

pub mod audience;
pub mod brand;
pub mod capture;
pub mod cloud;
pub mod collections;
pub mod documents;
pub mod import_menu;
pub mod json;
pub mod live_output;
pub mod lyrics;
pub mod media;
pub mod overlay;
pub mod program;
pub mod scripture;
pub mod scripture_trace;
pub mod serde_util;
pub mod services;
pub mod settings;
pub mod status;
pub mod transcript;
pub mod usage;
pub mod video_devices;
pub mod web;
pub mod workspace;

pub use settings::{AppSettings, StoredSettings};
