//! The conductor between transcription, detection and presentation (prompt 039).
//!
//! Ports `src/main/orchestrator.ts`, voice navigation, the live sermon
//! playlist and output dispatch. It sits above the service crates: `kairo-core`
//! cannot depend on `kairo-stt` or `kairo-scripture` without a cycle through
//! `kairo-cloud`.

pub mod clients;
pub mod detector;
pub mod dispatch;
pub mod engine;
pub mod live_plan;
pub mod orchestrator;
pub(crate) mod suggestion;
pub mod voice;
