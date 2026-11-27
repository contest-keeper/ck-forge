//! What the admin does: contest profiles, the stick registry and new stick identities.
//!
//! The command line and the admin window both use this crate, so they behave the same.

mod error;
mod file;
mod hardware;
mod hub;
mod hub_images;
mod hub_profile;
mod image;
mod profile;
mod registry;
mod stick;
pub mod vault;
mod wallpaper;

pub use ck_base_image::{
    Entry as CatalogEntry, Options as CreateOptions, Record as BaseRecord, Step as CreateStep,
    catalog, create_base_image, detect_image, find as find_catalog_entry, import_image,
};
pub use error::Error;
pub use file::folder_of;
pub use hardware::HardwareId;
pub use hub::{
    HubProfile, HubSession, HubStick, HubStickDisk, MintedEnrolmentCode, Pairing, PairingEnd,
    ProfileSummary, Pushed, fetch_profile, list_profiles, list_sticks, mint_enrolment_code,
    new_profile_id, push_registry, push_registry_signed_in, record_config_hash, record_stick_write,
    retire_stick_credential, sign_out, sign_settings, start_pairing, upload_profile,
    wait_for_pairing, watch_pairing,
};
pub use hub_images::{
    HubImage, HubImageDownload, HubImageStep, download_hub_image, list_hub_images,
    prepare_hub_image_download,
};
pub use hub_profile::{HUB_URL, LoadedProfile, load_hub_profile, save_hub_profile};
pub use image::{
    Built, EnrolmentBuild, ImageConfiguration, SignSettings, build_enrolment_image, build_image,
    build_thin_image, check_one_stick, configure_image, enrolment_config_sha256,
    image_configuration, requires_signed_settings, show_image, stick_config_sha256,
    supports_enrolment,
};
pub use profile::{ContestBookmark, Profile, StickKeyWindow, contest_url, follow_contest};
pub use registry::{CredentialScope, Registry, RegistryEntry};
pub use stick::{local_now, make_stick_config};
pub use wallpaper::{MAX_WALLPAPER_BYTES, Wallpaper, add_wallpaper, wallpaper_data_url};
