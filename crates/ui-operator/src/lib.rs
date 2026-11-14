//! The Operator workspace frame: the live rail with its monitor, clear-layer
//! strip and video transport, the transcript panel, and the pinned scrolling
//! the panels use (prompt 076). The panels' contents arrive in 077 onward.

pub mod backend;
pub mod booth;
pub mod layers;
pub mod layout;
pub mod rail;
pub mod scroll;
pub mod transport;
pub mod workspace;

pub use backend::{LiveSnapshot, OperatorBackend, OperatorCommand};
pub use booth::{Booth, BoothBackend, BoothEvent, BoothViews};
pub use layers::Layer;
pub use layout::UiState;
pub use rail::{LiveRail, RailEvent};
pub use workspace::{OperatorEvent, OperatorWorkspace};
