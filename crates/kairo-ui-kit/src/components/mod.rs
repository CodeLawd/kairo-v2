//! Kairo's shared controls. Kit components (`Button`, `Input`, `Select`,
//! `Slider`, `Switch`, `Tooltip`, menus, dialogs) are used directly; the
//! theme maps Kairo's tokens onto them. The controls here are the ones Kit
//! does not have, or that Kairo draws its own way.

pub mod pref_group;
pub mod segmented;
pub mod selection_bar;
pub mod traffic_lights;

pub use pref_group::PrefGroup;
pub use segmented::{SegmentOption, SegmentedControl};
pub use selection_bar::{SelectionAction, SelectionBar};
pub use traffic_lights::TrafficLights;
pub mod number_field;
pub use number_field::{NumberField, NumberFieldChanged, NumberModel};
pub mod code_field;
pub mod zoom_control;
pub use code_field::{CodeField, CodeFieldEvent};
pub use zoom_control::{ZoomChanged, ZoomControl, ZoomRange};
pub mod dialogs;
pub use dialogs::{ConfirmRequest, alert, confirm, messages};
pub mod modal;
pub use modal::{Frost, FrostedPanel, MovableDialog, MovableDialogEvent};
pub mod drag;
pub mod library_section;
pub mod marquee;
pub use drag::DragPill;
pub use library_section::{
    AddItem, LibraryDragItem, LibraryKind, LibraryRow, LibrarySection, LibrarySectionEvent,
};
pub use marquee::{Marquee, MarqueeBand, MarqueeStep, PressTarget};
pub mod rail;
pub use rail::RailItem;
