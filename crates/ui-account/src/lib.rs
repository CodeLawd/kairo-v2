//! Sign-in, email confirmation, activation, first-run setup and the coach
//! tour (prompt 073): `components/account/` and `components/onboarding/`.

pub mod activation;
pub mod auth;
pub mod backend;
pub mod coach;
pub mod display_picker;
pub mod flow;
pub mod gate;
pub mod license;
pub mod model;
pub mod ob;
pub mod style;
pub mod tour;
pub mod verify;
pub mod wizard;

pub use backend::{AccountBackend, OnboardingWrite, Reply};
pub use flow::{AccountFlow, FlowEvent};
pub use model::{Account, AccountChanged};
