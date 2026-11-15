//! The versioned wire protocol between `kairo` and `kairo-engine`, the
//! surface state both sides keep, and the shared-memory frame ring.
//!
//! These structs and `kairo-scene` are the only code the two processes share
//! (architecture section 5). The engine never reads settings, the network or
//! the database: everything it draws arrives through [`Command`].

pub mod messages;
pub mod preview;
#[cfg(unix)]
pub mod ring;
pub mod surface;
pub mod wire;

pub use messages::*;
pub use surface::{Availability, SurfaceState};
pub use wire::{DecodeError, FrameDecoder, MAX_FRAME_BYTES, encode_frame, read_frame, write_frame};

/// Bumped on any change to the encoding of a message. A client and an engine
/// with different versions refuse each other at the handshake.
pub const PROTOCOL_VERSION: u32 = 9;

/// Environment variable carrying the handshake token to the engine. It is
/// not passed on the command line, where other users' `ps` could read it.
pub const TOKEN_ENV: &str = "KAIRO_ENGINE_TOKEN";
