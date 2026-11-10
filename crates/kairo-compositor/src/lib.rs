//! The engine's GPU compositor: the program layer stack of `overlay.html`,
//! drawn with `wgpu` into one premultiplied frame per output, shown in
//! display windows letterboxed on black.

pub mod camera;
pub mod geometry;
pub mod layers;
pub mod renderer;
pub mod thread;

pub use camera::{Mode, VideoLayout, place, video_layout};
pub use geometry::{FRAME_WIDTH, Rect, Transform, frame_height, letterbox};
pub use layers::{Content, Fill, Node, PIP_SHADOW, Raster, Shadow, Slot, Stack, clamp_transition};
pub use renderer::{FRAME_FORMAT, FrameTarget, Renderer, Scene};
