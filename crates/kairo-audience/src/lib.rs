//! Live Q&A and polls, ported from `src/main/services/audience/` and the
//! audience and Engage handlers in `src/main/ipc/index.ts`.
//!
//! - [`AudienceService`] owns the question queue and what is on screen.
//! - [`KairoAudienceSource`] is the operator's WebSocket to an Engage room,
//!   with Electron's reconnect, heartbeat and request-ack rules.
//! - [`SampleAudienceSource`] gives demo questions in development only.
//! - [`EngageFlows`] are the Engage REST flows (list, create, resume,
//!   reopen, delete, invite, revoke) and `audience:start`.
//! - [`qr`] draws the join card's QR code as Electron's `qrcode` does.
//!
//! - [`SlidoAudienceSource`] reads a Slido event through a web view window
//!   that `ui-web` provides (prompt 047).

mod engage;
mod kairo;
pub mod qr;
mod sample;
mod service;
mod slido;
mod source;

pub use engage::{EngageApi, EngageFlows, EngageMethod, sample_allowed};
pub use kairo::{KairoAudienceSource, RoomFetcher};
pub use sample::SampleAudienceSource;
pub use service::{AudienceService, QuestionOutput};
pub use slido::{
    LOAD_TIMEOUT_MS, SLIDO_PROFILE, SLIDO_WINDOW_SIZE, SLIDO_WINDOW_TITLE, STALE_AFTER_MS,
    SlidoAudienceSource, SlidoEventSink, SlidoWindow, SlidoWindowConfig, SlidoWindowEvent,
    SlidoWindowFactory, allowed_slido_frame_navigation, allowed_slido_navigation,
    allowed_slido_window, page_script, slido_read_stale,
};
pub use source::{AudienceSource, AudienceUpdate, BoxFuture, UpdateSink};

/// Every failure carries the operator-facing message Electron shows.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct AudienceError(pub String);

impl AudienceError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

pub type Result<T> = std::result::Result<T, AudienceError>;

pub(crate) use kairo_domain::js::{lock, now_ms, slice_utf16};
