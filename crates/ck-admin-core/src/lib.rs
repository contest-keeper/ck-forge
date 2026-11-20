//! What the admin does: contest profiles, the stick registry and new stick identities.
//!
//! The command line and the admin window both use this crate, so they behave the same.

mod error;
mod file;
mod hardware;
mod hub;
mod hub_images;
mod image;
mod profile;
mod registry;
mod stick;
mod wallpaper;

pub use ck_base_image::{
    Entry as CatalogEntry, Options as CreateOptions, Record as BaseRecord, Step as CreateStep,
    catalog, create_base_image, detect_image, find as find_catalog_entry, import_image,
};
pub use error::Error;
pub use file::folder_of;
pub use hardware::HardwareId;
pub use hub::{
    HubProfile, HubSession, MintedEnrolmentCode, ProfileSummary, Pushed, fetch_profile,
    list_profiles, mint_enrolment_code, new_profile_id, new_state, push_registry,
    push_registry_signed_in, record_stick_write, retire_stick_credential, sign_in_url,
    sign_in_with_code, sign_out, upload_profile, wait_for_code,
};
pub use hub_images::{
    HubImage, HubImageDownload, HubImageStep, download_hub_image, list_hub_images,
    prepare_hub_image_download,
};
pub use image::{
    Built, EnrolmentBuild, ImageConfiguration, build_enrolment_image, build_image,
    build_thin_image, check_one_stick, configure_image, image_configuration, show_image,
    supports_enrolment,
};
pub use profile::{Profile, StickKeyWindow};
pub use registry::{CredentialScope, Registry, RegistryEntry};
pub use stick::{local_now, make_stick_config};
pub use wallpaper::{MAX_WALLPAPER_BYTES, Wallpaper};
