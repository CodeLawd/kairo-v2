//! Microphone capture and streaming speech recognition (prompt 038).
//!
//! Ports `src/renderer/src/audio`, the PCM worklet, `src/main/services/audio`
//! and `src/main/services/stt` into the core: capture runs on its own thread
//! from cpal, the relay keeps the 5 s cap, and Deepgram runs as one actor.

pub mod access;
pub mod buffer;
pub mod capture;
pub mod deepgram;
pub mod devices;
pub mod keyterms;
pub mod pcm;
pub mod permission;
pub mod recording;
pub mod relay;
pub mod replay;
pub mod resample;
pub mod service;

pub use access::TranscriptionAccess;
pub use deepgram::{DeepgramStt, SttEvent, Timings};
pub use relay::AudioRelay;
pub use service::SttService;
