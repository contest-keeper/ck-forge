//! What the admin does: contest profiles, the stick registry and new stick identities.
//!
//! The command line and the admin window both use this crate, so they behave the same.

mod error;
mod file;
mod profile;
mod registry;
mod stick;

pub use error::Error;
pub use profile::Profile;
pub use registry::{Registry, RegistryEntry};
pub use stick::make_stick_config;
