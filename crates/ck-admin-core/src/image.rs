use std::fs;
use std::path::Path;

use ck_image::{
    copy_image, has_config_partition, read_config_file, read_file, write_config_file, write_file,
};
use ck_protocol::{
    CONFIG_FILE, ESP_CONFIG_DIR, ESP_CONFIG_FILE, LocalDateTime, Settings, StickConfig,
};

use crate::error::Error;
use crate::profile::Profile;
use crate::registry::{Registry, RegistryEntry};
use crate::stick::make_stick_config;
use crate::wallpaper::Wallpaper;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub config: StickConfig,
    /// SHA-256 of the copy of the base image, before the settings were written. Only a full copy has
    /// one: a thin image does not read the whole base.
    pub base_sha256: Option<String>,
}

/// How much of the base image a new stick image holds.
#[derive(Clone, Copy)]
enum Copy {
    /// All of it: a file of the size of the base that runs on its own.
    Full,
    /// Only its start, up to the end of the config partition (the EFI partition on an older image),
    /// and a manifest that names the base for
    /// the rest. See `ck_writer::thin`.
    Thin,
}

fn esp_config_path() -> String {
    format!("{ESP_CONFIG_DIR}/{ESP_CONFIG_FILE}")
}

/// Writes a file next to the stick's settings: at the root of the `CKCONFIG` partition when the
/// image has one, in the `ck` folder of the EFI partition on an older image.
fn write_beside_settings(image: &Path, name: &str, content: &[u8]) -> Result<(), Error> {
    if has_config_partition(image)? {
        write_config_file(image, name, content)?;
    } else {
        write_file(image, &format!("{ESP_CONFIG_DIR}/{name}"), content)?;
    }
    Ok(())
}

/// The wallpaper must be the one the settings name, or the stick would point at a missing file.
fn check_wallpaper(settings: &Settings, wallpaper: Option<&Wallpaper>) -> Result<(), Error> {
    let named = settings.branding.wallpaper.as_deref();
    if named != wallpaper.map(|w| w.name.as_str()) {
        return Err(Error::Wallpaper(
            "the picture given is not the one the profile names".to_owned(),
        ));
    }
    // The picture is written next to the settings file, and FAT ignores case.
    if named.is_some_and(|name| name.eq_ignore_ascii_case(ESP_CONFIG_FILE)) {
        return Err(Error::Wallpaper(format!(
            "a picture cannot be named {ESP_CONFIG_FILE}, which is the settings file of the stick"
        )));
    }
    Ok(())
}

fn write_wallpaper(image: &Path, wallpaper: Option<&Wallpaper>) -> Result<(), Error> {
    if let Some(wallpaper) = wallpaper {
        write_beside_settings(image, &wallpaper.name, &wallpaper.bytes)?;
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
/// new image is removed, so a half-made stick is never left around. `base` is only read, and an
/// existing `out` is never replaced.
pub fn build_image(
    base: &Path,
    out: &Path,
    profile: &Profile,
    wallpaper: Option<&Wallpaper>,
    registry: &Path,
    now: LocalDateTime,
    progress: impl FnMut(u64, u64),
) -> Result<Built, Error> {
    build(
        base,
        out,
        profile,
        wallpaper,
        registry,
        now,
        Copy::Full,
        progress,
    )
}

/// Like [`build_image`], but `out` holds only the start of the base image, through its config
/// partition (its EFI partition on an older image), and a manifest `out` + `.thin.json` names `base` for the rest. That is a copy of a
/// few hundred megabytes and not of 30 GB, and a stick written from it is the same stick. The
/// result is not an image that runs by itself: `ck-forge stick write` reads the rest from `base`,
/// and refuses it when `base` is not the image it was made from. Needs the EFI partition to be
/// the last part of the image that the settings touch, which is the case for every stick image.
pub fn build_thin_image(
    base: &Path,
    out: &Path,
    profile: &Profile,
    wallpaper: Option<&Wallpaper>,
    registry: &Path,
    now: LocalDateTime,
    progress: impl FnMut(u64, u64),
) -> Result<Built, Error> {
    build(
        base,
        out,
        profile,
        wallpaper,
        registry,
        now,
        Copy::Thin,
        progress,
    )
}

#[allow(clippy::too_many_arguments)]
fn build(
    base: &Path,
    out: &Path,
    profile: &Profile,
    wallpaper: Option<&Wallpaper>,
    registry: &Path,
    now: LocalDateTime,
    copy: Copy,
    progress: impl FnMut(u64, u64),
) -> Result<Built, Error> {
    let credential_scope = profile.credential_scope()?;
    let settings = profile.stick_settings();
    check_wallpaper(&settings, wallpaper)?;
    // Counted before the copy, so a profile that cannot be used stops before anything is written.
    let number = Registry::load(registry)?.next_machine_number(&settings.contest.name);
    let config = make_stick_config(&settings, number, now)?;
    let base_sha256 = match copy {
        Copy::Full => Some(copy_image(base, out, progress)?),
        Copy::Thin => {
            ck_writer::thin::create(base, out, settings_end(base)?, progress)?;
            None
        }
    };
    let result = write_beside_settings(out, CONFIG_FILE, &to_json(&config))
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
                    credential_scope,
                },
            )
        });
    if let Err(error) = result {
        ck_writer::thin::remove(out);
        return Err(error);
    }
    Ok(Built {
        config,
        base_sha256,
    })
}

/// Where the part of `base` that differs from stick to stick ends: the settings are written inside
/// it. That is the end of the `CKCONFIG` partition, or of the EFI partition on an older image.
fn settings_end(base: &Path) -> Result<u64, Error> {
    let mut file =
        fs::File::open(base).map_err(|e| Error::io(format!("opening {}", base.display()), e))?;
    let partitions = ck_image::read_partitions(&mut file)?;
    Ok(ck_image::settings_end(&partitions)?)
}

/// Replaces the settings on an image with the profile's, keeping the stick's id, secret and
/// creation time. Returns the new file. When the stick is in the registry file at `registry`, its
/// team, site and contest are updated too. An image with no settings yet is refused.
pub fn configure_image(
    image: &Path,
    profile: &Profile,
    wallpaper: Option<&Wallpaper>,
    registry: &Path,
) -> Result<StickConfig, Error> {
    let mut config = show_image(image)?;
    let registry_entry = Registry::load(registry)?;
    let existing_scope = registry_entry
        .find(&config.stick_id)
        .and_then(|entry| entry.credential_scope.as_ref());
    if existing_scope != profile.credential_scope()?.as_ref() {
        return Err(Error::StickRekeyRequired);
    }
    config.settings = profile.stick_settings();
    // The whole file is checked, not only the settings: a name pattern can be fine on its own and
    // still give this stick an empty computer name, which the agent refuses.
    let problems = config.validate();
    if !problems.is_empty() {
        return Err(Error::InvalidProfile(problems));
    }
    check_wallpaper(&config.settings, wallpaper)?;
    write_wallpaper(image, wallpaper)?;
    write_beside_settings(image, CONFIG_FILE, &to_json(&config))?;
    Registry::update_labels(
        registry,
        &config.stick_id,
        &config.settings.team,
        &config.settings.site,
        &config.settings.contest.name,
    )?;
    Ok(config)
}

/// Refuses an image made for one stick when it is about to be written to `sticks` sticks: they
/// would all get its stick id and secret. A plain image with no settings may go to any number.
pub fn check_one_stick(image: &Path, sticks: usize) -> Result<(), Error> {
    if sticks > 1
        && let Ok(config) = show_image(image)
    {
        return Err(Error::ImageForOneStick(config.stick_id.to_string()));
    }
    Ok(())
}

/// The settings on an image, secret included: callers decide what to show.
pub fn show_image(image: &Path) -> Result<StickConfig, Error> {
    let read = if has_config_partition(image)? {
        read_config_file(image, CONFIG_FILE)
    } else {
        read_file(image, &esp_config_path())
    };
    let bytes = match read {
        Ok(bytes) => bytes,
        Err(ck_image::Error::FileNotFound(_)) => return Err(Error::ImageNotConfigured),
        Err(other) => return Err(other.into()),
    };
    serde_json::from_slice(&bytes).map_err(Error::ImageSettingsUnreadable)
}
