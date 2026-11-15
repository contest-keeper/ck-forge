//! What the admin does: contest profiles, the stick registry and new stick identities.
//!
//! The command line and the admin window both use this crate, so they behave the same.

mod error;
mod file;
mod hub;
mod image;
mod profile;
mod registry;
mod stick;
mod wallpaper;

pub use error::Error;
pub use file::folder_of;
pub use hub::{
    HubSession, Pushed, new_state, push_registry, push_registry_signed_in, sign_in_url,
    sign_in_with_code, sign_out, wait_for_code,
};
pub use image::{
    Built, build_image, build_thin_image, check_one_stick, configure_image, show_image,
};
pub use profile::Profile;
pub use registry::{Registry, RegistryEntry};
pub use stick::make_stick_config;
pub use wallpaper::{MAX_WALLPAPER_BYTES, Wallpaper};
