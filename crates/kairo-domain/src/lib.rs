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
//! - Nothing is normalised on read. The normalisers (prompt 013) are
//!   separate functions: [`overlay_defaults`], [`theme_library`] and
//!   [`pp_retirement`]. JavaScript number and string semantics they rely on
//!   are in [`js`].

pub mod audience;
pub mod brand;
pub mod capture;
pub mod cloud;
pub mod collections;
pub mod displays;
pub mod documents;
pub mod import_menu;
pub mod js;
pub mod json;
pub mod live_output;
pub mod lyrics;
pub mod media;
pub mod normalize;
pub mod output_health;
pub mod output_settings;
pub mod output_test_pattern;
pub mod overlay;
pub mod overlay_boxes;
pub mod overlay_content;
pub mod overlay_defaults;
pub mod overlay_elements;
pub mod overlay_outputs;
pub mod overlay_shapes;
pub mod pp_retirement;
pub mod program;
pub mod prop_geometry;
pub mod scripture;
pub mod scripture_trace;
pub mod serde_util;
pub mod services;
pub mod settings;
pub mod status;
pub mod theme_library;
pub mod transcript;
pub mod usage;
pub mod video_devices;
pub mod web;
pub mod workspace;

pub use settings::{AppSettings, StoredSettings};
