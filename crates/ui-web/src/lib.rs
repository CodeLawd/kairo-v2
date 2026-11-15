//! Web-view windows for the operator's side tools, on `gpui-webview`.
//!
//! [`WebPage`] is the Web workspace (prompt 089), with its preview on a
//! `gpui-webview` page.
//!
//! [`SlidoWebHost`] opens the Slido sign-in and reading window for
//! `kairo-audience`. Wry's views are not `Send`, so each window lives on the
//! GPUI thread and the host talks to it through a channel.

pub mod page;
mod permissions;
mod platform;
mod slido;

pub use page::{NativePreview, PreviewFactory, WebBackend, WebPage};
pub use slido::{HostOptions, SlidoWebHost};
