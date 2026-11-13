//! Which output shows what (prompt 065).
//!
//! A port of Electron's `surface-manager.ts`, `lobby-player.ts`,
//! `program-service.ts` and `dispatch.ts`. The engine process draws and
//! places windows; this crate decides which surfaces exist, what each one
//! may show, and what each one is told:
//!
//! - [`SurfaceManager`] is a core actor ([`actor`]). Its mailbox serializes
//!   reconciles, so a settings write landing during a display change cannot
//!   open two surfaces for one output.
//! - [`LobbyPlayer`] runs a screen's or card's own playlist, isolated from
//!   the service.
//! - [`ProgramService`] holds the program state and hands every change to
//!   the manager.

pub mod actor;
pub mod displays;
#[cfg(feature = "fake")]
pub mod fake;
pub mod lobby;
pub mod manager;
pub mod ports;
pub mod program;

pub use actor::{SurfaceActor, spawn_surface_manager};
pub use lobby::LobbyPlayer;
pub use manager::{OutputState, SurfaceManager};
pub use ports::{LobbyMedia, OutputSettings, RoutingSink};
pub use program::ProgramService;
