//! Sign-in, email confirmation, activation, first-run setup and the coach
//! tour (prompt 073): `components/account/` and `components/onboarding/`.

mod account_page;
pub mod activation;
pub mod auth;
pub mod backend;
pub mod coach;
pub mod display_picker;
pub mod flow;
pub mod fluid;
pub mod gate;
pub mod license;
pub mod model;
pub mod ob;
mod setup_state;
pub mod stage;
pub mod style;
pub mod tour;
mod tour_page;
pub mod verify;
pub mod wizard;
mod write_runner;

pub use backend::{AccountBackend, OnboardingWrite, Reply};
pub use flow::{AccountFlow, FlowEvent};
pub use model::{Account, AccountChanged};
