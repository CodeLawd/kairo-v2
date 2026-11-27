//! The operator window: rail, header, routed workspaces, menus, shortcuts,
//! quit confirmation, What's New, update and keychain notices, and the
//! contained-panic fallback (prompt 071).

pub mod bootstrap;
mod chrome;
pub mod crash;
pub mod keymap;
pub mod launch;
pub mod menus;
pub mod route;
pub mod shell;
pub mod state;
pub mod ui_scale;
pub mod view;
pub mod whats_new;

pub use route::{APPS, Route, WORKSPACES};
pub use shell::{Overlay, Shell, ShellOptions};
pub use state::{ShellRequest, ShellState};
