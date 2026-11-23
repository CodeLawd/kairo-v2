//! Imagine: OpenAI image generation and refinement, kept as drafts.
//!
//! Nothing here publishes. Generated images live in Imagine's own store until
//! the operator saves one to Media (or Slides) through the application.

pub mod agent;
pub mod deck;
pub mod error;
pub mod openai;
pub mod pptx;
pub mod preview;
pub mod store;
pub mod work;

pub use agent::{Action, DeckPlan, Plan, SlidePlan};
pub use deck::{DeckTheme, Layout, Point};
pub use error::ImagineError;
pub use openai::{
    ImageModel, ImageRequest, ImageTransport, OwnKeyTransport, Quality, estimate_usd,
};
pub use store::{ImagineStore, Kind, NewVersion, Project, Slide, Version};
