//! Web-view windows for the operator's side tools, on `gpui-webview`.
//!
//! [`SlidoWebHost`] opens the Slido sign-in and reading window for
//! `kairo-audience`. Wry's views are not `Send`, so each window lives on the
//! GPUI thread and the host talks to it through a channel.

mod permissions;
mod platform;
mod slido;

pub use slido::{HostOptions, SlidoWebHost};
