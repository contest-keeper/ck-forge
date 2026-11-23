use ck_crypto::CONFIG_SIGNATURE_FILE;
use std::fs;
use std::path::Path;

use ck_image::{
    copy_image, has_config_partition, read_config_file, read_file, write_config_file, write_file,
};
use ck_protocol::{
    CONFIG_FILE, ENROLMENT_SCHEMA, ESP_CONFIG_DIR, ESP_CONFIG_FILE, EnrolmentCode, EnrolmentConfig,
    LocalDateTime, Problem, Settings, StickConfig, StickId, render_computer_name,
};
use uuid::Uuid;

use crate::error::Error;
use crate::file::Lock;
use crate::profile::Profile;
use crate::registry::{CredentialScope, Registry, RegistryEntry};
use crate::stick::make_stick_config;
use crate::wallpaper::Wallpaper;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub config: StickConfig,
    /// SHA-256 of the copy of the base image, before the settings were written. Only a full copy has
    /// one: a thin image does not read the whole base.
    pub base_sha256: Option<String>,
}

/// Signs the exact bytes of a settings file and returns the text of its signature file. The Hub does
/// it; Forge gives it to the builders so they can write the signature next to the settings.
pub type SignSettings<'a> = &'a dyn Fn(&[u8]) -> Result<String, Error>;

pub struct EnrolmentBuild<'a> {
    pub base: &'a Path,
    pub out: &'a Path,
    pub profile: &'a Profile,
    pub wallpaper: Option<&'a Wallpaper>,
    pub registry: &'a Path,
    pub now: LocalDateTime,
    /// Needed when the base image only accepts settings the Hub signed.
    pub sign: Option<SignSettings<'a>>,
}

/// Whether sticks made from this image only accept settings the Hub signed.
pub fn requires_signed_settings(image: &Path) -> Result<bool, Error> {
    Ok(ck_base_image::requires_signed_settings(image)?)
}

/// Whether the settings written for a stick of `image` need a signature: when the image carries the
/// key list, or when a stick image that holds only the start of a base (a thin image) already has
/// one. Stops early, before anything is made or a code is minted, when a signature is needed and no
/// signer was given.
fn signature_needed(image: &Path, sign: Option<SignSettings<'_>>) -> Result<bool, Error> {
    require_signer(requires_signed_settings(image)?, sign)
}

/// The same for a stick image that may hold only the start of its base: there the key list is not
/// there to be read, but a signature file next to the settings says the image is a signed one.
fn signature_needed_for_stick_image(
    image: &Path,
    sign: Option<SignSettings<'_>>,
) -> Result<bool, Error> {
    let signed_before = read_config_file(image, CONFIG_SIGNATURE_FILE).is_ok();
    let needed = signed_before || requires_signed_settings(image).unwrap_or(false);
    require_signer(needed, sign)
}

fn require_signer(needed: bool, sign: Option<SignSettings<'_>>) -> Result<bool, Error> {
    if needed && sign.is_none() {
        return Err(Error::SignedSettingsRequired);
    }
    Ok(needed)
}

/// The signature file for the settings bytes, when one is needed.
fn signature_of(
    needed: bool,
    sign: Option<SignSettings<'_>>,
    bytes: &[u8],
) -> Result<Option<String>, Error> {
    match (needed, sign) {
        (true, Some(sign)) => sign(bytes).map(Some),
        (true, None) => Err(Error::SignedSettingsRequired),
        (false, _) => Ok(None),
    }
}

/// Writes the settings file and, when there is one, its signature, next to each other.
fn write_settings(image: &Path, bytes: &[u8], signature: Option<&str>) -> Result<(), Error> {
    write_beside_settings(image, CONFIG_FILE, bytes)?;
    if let Some(signature) = signature {
        write_beside_settings(image, CONFIG_SIGNATURE_FILE, signature.as_bytes())?;
    }
    Ok(())
}

pub enum ImageConfiguration {
    Registered(StickConfig),
    Enrolment(EnrolmentConfig),
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

fn enrolment_json(config: &EnrolmentConfig) -> Vec<u8> {
    serde_json::to_vec_pretty(config).expect("an enrolment config always serializes")
}

/// The SHA-256 of the settings file Forge writes for a stick, the exact bytes that go on the config
/// partition. The stick reports the hash of the file it read, so the Hub can tell the file was
/// changed. The picture is not part of it.
#[must_use]
pub fn stick_config_sha256(config: &StickConfig) -> String {
    ck_crypto::sha256_hex(&to_json(config))
}

/// Like [`stick_config_sha256`], for a stick that enrols at its first boot.
#[must_use]
pub fn enrolment_config_sha256(config: &EnrolmentConfig) -> String {
    ck_crypto::sha256_hex(&enrolment_json(config))
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
        None,
        progress,
    )
}

/// Like [`build_image`], but `out` holds only the start of the base image, through its config
/// partition (its EFI partition on an older image), and a manifest `out` + `.thin.json` names `base` for the rest. That is a copy of a
/// few hundred megabytes and not of 30 GB, and a stick written from it is the same stick. The
/// result is not an image that runs by itself: `ck-forge stick write` reads the rest from `base`,
/// and refuses it when `base` is not the image it was made from. Needs the EFI partition to be
/// the last part of the image that the settings touch, which is the case for every stick image.
/// The settings are signed with `sign` when the base image only accepts signed ones.
#[allow(clippy::too_many_arguments)]
pub fn build_thin_image(
    base: &Path,
    out: &Path,
    profile: &Profile,
    wallpaper: Option<&Wallpaper>,
    registry: &Path,
    now: LocalDateTime,
    sign: Option<SignSettings<'_>>,
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
        sign,
        progress,
    )
}

pub fn supports_enrolment(base: &Path) -> Result<bool, Error> {
    Ok(has_config_partition(base)?
        && ck_base_image::supports_enrolment_schema(base, ENROLMENT_SCHEMA)?)
}

/// Makes a thin image whose agent gets its credential at first boot. The mint callback is called
/// only after checking that the base and settings can use enrolment.
pub fn build_enrolment_image(
    options: &EnrolmentBuild<'_>,
    mint: impl FnOnce(&Settings, u32, Option<&CredentialScope>) -> Result<EnrolmentCode, Error>,
    progress: impl FnMut(u64, u64),
) -> Result<EnrolmentConfig, Error> {
    if !supports_enrolment(options.base)? {
        return Err(Error::EnrolmentUnsupported);
    }
    let signed = signature_needed(options.base, options.sign)?;
    if options.out.exists() {
        return Err(ck_base_image::Error::Exists(options.out.display().to_string()).into());
    }
    let scope = options.profile.credential_scope()?;
    let settings = options.profile.stick_settings();
    check_wallpaper(&settings, options.wallpaper)?;
    crate::hub::checked_base(&settings.api.url)?;
    let mut problems = settings.validate();
    if render_computer_name(
        &settings.branding.computer_name_pattern,
        &settings.team,
        &settings.site,
        1,
    )
    .is_empty()
    {
        problems.push(Problem::ComputerNameUnusable);
    }
    if !problems.is_empty() {
        return Err(Error::InvalidProfile(problems));
    }
    let end = settings_end(options.base)?;
    let id = StickId::parse(&Uuid::new_v4().to_string()).expect("a UUID is a valid stick id");
    // The number belongs to the code, so keep the reservation if minting or writing fails.
    let machine_number =
        Registry::reserve_enrolment(options.registry, &id, &settings.contest.name)?;
    let code = mint(&settings, machine_number, scope.as_ref())?;
    let config = EnrolmentConfig::new(id, options.now, Some(machine_number), code, settings);
    let bytes = enrolment_json(&config);
    let signature = signature_of(signed, options.sign, &bytes)?;
    ck_writer::thin::create(options.base, options.out, end, progress)?;
    let result = write_settings(options.out, &bytes, signature.as_deref())
        .and_then(|()| write_wallpaper(options.out, options.wallpaper));
    if let Err(error) = result {
        ck_writer::thin::remove(options.out);
        return Err(error);
    }
    Ok(config)
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
    sign: Option<SignSettings<'_>>,
    progress: impl FnMut(u64, u64),
) -> Result<Built, Error> {
    let signed = signature_needed(base, sign)?;
    let credential_scope = profile.credential_scope()?;
    let settings = profile.stick_settings();
    check_wallpaper(&settings, wallpaper)?;
    // A concurrent enrolment must see this machine number before it asks the Hub for a code.
    let _registry_lock = Lock::acquire(registry)?;
    let mut entries = Registry::load(registry)?;
    let number = entries.next_machine_number(&settings.contest.name);
    let config = make_stick_config(&settings, number, now)?;
    let bytes = to_json(&config);
    let signature = signature_of(signed, sign, &bytes)?;
    let base_sha256 = match copy {
        Copy::Full => Some(copy_image(base, out, progress)?),
        Copy::Thin => {
            ck_writer::thin::create(base, out, settings_end(base)?, progress)?;
            None
        }
    };
    let result = write_settings(out, &bytes, signature.as_deref())
        .and_then(|()| write_wallpaper(out, wallpaper))
        .and_then(|()| {
            entries.add(RegistryEntry {
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
            })?;
            entries.save(registry)
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
/// The new settings are signed with `sign` when the image only accepts signed ones.
pub fn configure_image(
    image: &Path,
    profile: &Profile,
    wallpaper: Option<&Wallpaper>,
    registry: &Path,
    sign: Option<SignSettings<'_>>,
) -> Result<StickConfig, Error> {
    let signed = signature_needed_for_stick_image(image, sign)?;
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
    let bytes = to_json(&config);
    let signature = signature_of(signed, sign, &bytes)?;
    write_wallpaper(image, wallpaper)?;
    write_settings(image, &bytes, signature.as_deref())?;
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
        && let Ok(bytes) = image_settings_bytes(image)
    {
        let id = match parse_image_configuration(&bytes)? {
            ImageConfiguration::Registered(config) => config.stick_id,
            ImageConfiguration::Enrolment(config) => config.stick_id,
        };
        return Err(Error::ImageForOneStick(id.to_string()));
    }
    Ok(())
}

/// The settings on an image, secret included: callers decide what to show.
pub fn show_image(image: &Path) -> Result<StickConfig, Error> {
    match image_configuration(image)? {
        ImageConfiguration::Registered(config) => Ok(config),
        ImageConfiguration::Enrolment(_) => Err(Error::EnrolmentImage),
    }
}

pub fn image_configuration(image: &Path) -> Result<ImageConfiguration, Error> {
    parse_image_configuration(&image_settings_bytes(image)?)
}

fn image_settings_bytes(image: &Path) -> Result<Vec<u8>, Error> {
    let read = if has_config_partition(image)? {
        read_config_file(image, CONFIG_FILE)
    } else {
        read_file(image, &esp_config_path())
    };
    match read {
        Ok(bytes) => Ok(bytes),
        Err(ck_image::Error::FileNotFound(_)) => Err(Error::ImageNotConfigured),
        Err(other) => Err(other.into()),
    }
}

fn parse_image_configuration(bytes: &[u8]) -> Result<ImageConfiguration, Error> {
    let json: serde_json::Value =
        serde_json::from_slice(bytes).map_err(Error::ImageSettingsUnreadable)?;
    if json.get("schema").and_then(serde_json::Value::as_u64) == Some(u64::from(ENROLMENT_SCHEMA)) {
        return serde_json::from_slice(bytes)
            .map(ImageConfiguration::Enrolment)
            .map_err(Error::ImageSettingsUnreadable);
    }
    if json.get("enrolment_code").is_some() {
        return Err(Error::AmbiguousImageConfig);
    }
    serde_json::from_slice(bytes)
        .map(ImageConfiguration::Registered)
        .map_err(Error::ImageSettingsUnreadable)
}
