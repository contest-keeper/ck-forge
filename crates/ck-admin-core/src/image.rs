use std::fs;
use std::path::Path;

use ck_image::{copy_image, read_file, write_file};
use ck_protocol::{ESP_CONFIG_DIR, ESP_CONFIG_FILE, LocalDateTime, Settings, StickConfig};

use crate::error::Error;
use crate::profile::Profile;
use crate::registry::{Registry, RegistryEntry};
use crate::stick::make_stick_config;
use crate::wallpaper::Wallpaper;

/// A stick image that was just made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub config: StickConfig,
    /// SHA-256 of the copy of the base image, before the settings were written.
    pub base_sha256: String,
}

fn config_path() -> String {
    format!("{ESP_CONFIG_DIR}/{ESP_CONFIG_FILE}")
}

/// The wallpaper must be the one the settings name, or the stick would point at a missing file.
fn check_wallpaper(settings: &Settings, wallpaper: Option<&Wallpaper>) -> Result<(), Error> {
    let named = settings.branding.wallpaper.as_deref();
    if named == wallpaper.map(|w| w.name.as_str()) {
        Ok(())
    } else {
        Err(Error::Wallpaper(
            "the picture given is not the one the profile names".to_owned(),
        ))
    }
}

fn write_wallpaper(image: &Path, wallpaper: Option<&Wallpaper>) -> Result<(), Error> {
    if let Some(wallpaper) = wallpaper {
        write_file(
            image,
            &format!("{ESP_CONFIG_DIR}/{}", wallpaper.name),
            &wallpaper.bytes,
        )?;
    }
    Ok(())
}

fn to_json(config: &StickConfig) -> Vec<u8> {
    serde_json::to_vec_pretty(config).expect("a stick config always serializes")
}

/// Copies `base` to `out`, gives the copy a new stick identity and the profile's settings, and
/// adds the stick to the registry file at `registry`.
///
/// `wallpaper` is the picture the profile names, if any, put on the stick next to the settings.
///
/// The profile is checked before the (large) copy starts. When anything fails after the copy, the
/// new image is removed, so a half-made stick is never left around. `base` is only read.
///
/// # Errors
/// Fails when the profile has problems, `out` already exists, the image cannot be read or written,
/// or the registry cannot be updated.
pub fn build_image(
    base: &Path,
    out: &Path,
    profile: &Profile,
    wallpaper: Option<&Wallpaper>,
    registry: &Path,
    now: LocalDateTime,
    progress: impl FnMut(u64, u64),
) -> Result<Built, Error> {
    let settings = profile.stick_settings();
    check_wallpaper(&settings, wallpaper)?;
    // Counted before the copy, so a profile that cannot be used stops before anything is written.
    let number = Registry::load(registry)?.next_machine_number(&settings.contest.name);
    let config = make_stick_config(&settings, number, now)?;
    let base_sha256 = copy_image(base, out, progress)?;
    let result = write_file(out, &config_path(), &to_json(&config))
        .map_err(Error::from)
        .and_then(|()| write_wallpaper(out, wallpaper))
        .and_then(|()| {
            Registry::register(
                registry,
                RegistryEntry {
                    stick_id: config.stick_id.clone(),
                    token: config.token.clone(),
                    team: config.settings.team.clone(),
                    site: config.settings.site.clone(),
                    contest: config.settings.contest.name.clone(),
                    image: out
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                    created_at: config.created_at,
                    machine_number: config.machine_number,
                },
            )
        });
    if let Err(error) = result {
        let _ = fs::remove_file(out);
        return Err(error);
    }
    Ok(Built {
        config,
        base_sha256,
    })
}

/// Replaces the settings on an image with the profile's, keeping the stick's id, secret and
/// creation time. Returns the new file. When the stick is in the registry file at `registry`, its
/// team, site and contest are updated too.
///
/// # Errors
/// Fails when the profile has problems, the image has no settings yet, or it cannot be written.
pub fn configure_image(
    image: &Path,
    profile: &Profile,
    wallpaper: Option<&Wallpaper>,
    registry: &Path,
) -> Result<StickConfig, Error> {
    let problems = profile.problems();
    if !problems.is_empty() {
        return Err(Error::InvalidProfile(problems));
    }
    let mut config = show_image(image)?;
    config.settings = profile.stick_settings();
    check_wallpaper(&config.settings, wallpaper)?;
    write_wallpaper(image, wallpaper)?;
    write_file(image, &config_path(), &to_json(&config))?;
    Registry::update_labels(
        registry,
        &config.stick_id,
        &config.settings.team,
        &config.settings.site,
        &config.settings.contest.name,
    )?;
    Ok(config)
}

/// Reads the settings from an image.
///
/// # Errors
/// Fails when the image cannot be read, has no settings, or they are not valid.
pub fn show_image(image: &Path) -> Result<StickConfig, Error> {
    let bytes = match read_file(image, &config_path()) {
        Ok(bytes) => bytes,
        Err(ck_image::Error::FileNotFound(_)) => return Err(Error::ImageNotConfigured),
        Err(other) => return Err(other.into()),
    };
    serde_json::from_slice(&bytes).map_err(Error::ImageSettingsUnreadable)
}
