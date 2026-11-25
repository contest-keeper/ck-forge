//! `ck-forge image` and the settings readers.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::cli::ExtraSettings;
use ck_admin_core::{
    CreateOptions, CreateStep, Error, ImageConfiguration, Profile, Wallpaper, build_image,
    build_thin_image, catalog, create_base_image, folder_of, image_configuration, import_image,
    local_now, requires_signed_settings, supports_enrolment,
};
use ck_protocol::{LocalDateTime, Settings, StickConfig, StickId, render_computer_name};
use indicatif::{ProgressBar, ProgressStyle};

pub(crate) fn image_catalog() -> Result<String, Error> {
    let mut text = String::new();
    for entry in catalog()? {
        let _ = writeln!(text, "{}\t{} ({} GB)", entry.id, entry.label, entry.disk_gb);
    }
    Ok(text)
}

/// Downloads under the user's local data folder (the caches folder on macOS), so a second build
/// does not download again.
pub(crate) fn default_cache() -> PathBuf {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(base).join("ck-forge").join("cache");
    }
    if cfg!(target_os = "macos")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join("Library/Caches/ck-forge");
    }
    PathBuf::from("ck-forge-cache")
}

pub(crate) fn image_import(archive: &Path, out: &Path, sha256: &str) -> Result<String, Error> {
    let bar = ProgressBar::new(0);
    bar.set_message("Verifying and expanding the download");
    import_image(archive, out, sha256, |done, total| {
        bar.set_length(total);
        bar.set_position(done);
    })?;
    bar.finish_and_clear();
    Ok(format!("Verified base image saved to {}\n", out.display()))
}

pub(crate) fn image_create(options: &CreateOptions) -> Result<String, Error> {
    let bar = ProgressBar::new(0);
    let out = options.out.clone();
    let record = create_base_image(options, |step, done, total| {
        let (label, unit) = match step {
            CreateStep::Download => ("downloading", 1_000_000),
            CreateStep::Convert => ("converting to a raw disk", 1),
            CreateStep::Provision => ("installing the desktop and tools (seconds)", 1),
            CreateStep::Patch => ("applying the Keeper OS patch", 1),
            CreateStep::Hash => ("checking the finished image", 1),
        };
        bar.set_length(total / unit);
        bar.set_position(done / unit);
        bar.set_message(label);
    })?;
    bar.finish_and_clear();
    let mut text = format!("Made {}\n", out.display());
    let _ = writeln!(text, "  SHA-256: {}", record.image_sha256);
    if !record.patched {
        text.push_str(
            "  Not Keeper OS yet: run again with --patch ck-os-patch.tar.gz (needs WSL) to add the agent.\n",
        );
    }
    Ok(text)
}

pub(crate) fn warn(profile: &Profile) {
    for warning in profile.warnings() {
        eprintln!("warning: {warning}");
    }
}

pub(crate) fn image_build(
    base: &Path,
    image: &Path,
    profile: &Path,
    registry: &Path,
    full_copy: bool,
    extras: &ExtraSettings,
) -> Result<String, Error> {
    let profile_file = profile;
    let mut profile = Profile::load(profile_file)?;
    extras.apply(&mut profile)?;
    let wallpaper = Wallpaper::read(&profile.stick_settings(), folder_of(profile_file))?;
    warn(&profile);
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template("{bar:40} {bytes}/{total_bytes} ({eta})")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    let now = local_now();
    let progress = |done, total| {
        bar.set_length(total);
        bar.set_position(done);
    };
    let result = if full_copy {
        build_image(
            base,
            image,
            &profile,
            wallpaper.as_ref(),
            registry,
            now,
            progress,
        )
    } else {
        build_thin_image(
            base,
            image,
            &profile,
            wallpaper.as_ref(),
            registry,
            now,
            None,
            progress,
        )
    };
    bar.finish_and_clear();
    let built = result?;
    Ok(format!(
        "Made {}\nRegistered in {}\n{}",
        image.display(),
        registry.display(),
        config_text(&built.config)
    ))
}

/// The settings of a stick for the admin to read. The secret is never part of it.
pub(crate) fn config_text(config: &StickConfig) -> String {
    describe_config(
        &config.stick_id,
        config.created_at,
        config.machine_number,
        &config.settings,
    )
}

pub(crate) fn show_support(base: &Path) -> Result<String, Error> {
    let yes = |value: bool| if value { "yes" } else { "no" };
    Ok(format!(
        "Settings must be signed by the Hub: {}\nFirst-boot enrolment: {}\n",
        yes(requires_signed_settings(base)?),
        yes(supports_enrolment(base)?)
    ))
}

pub(crate) fn show_config(image: &Path) -> Result<String, Error> {
    let text = match image_configuration(image)? {
        ImageConfiguration::Registered(config) => config_text(&config),
        ImageConfiguration::Enrolment(config) => {
            let mut text = describe_config(
                &config.stick_id,
                config.created_at,
                config.machine_number,
                &config.settings,
            );
            text.push_str("Provisioning: first-boot enrolment\n");
            text
        }
    };
    Ok(text)
}

pub(crate) fn describe_config(
    id: &StickId,
    created: LocalDateTime,
    number: Option<u32>,
    s: &Settings,
) -> String {
    let number = number.unwrap_or(1);
    let name = render_computer_name(&s.branding.computer_name_pattern, &s.team, &s.site, number);
    let mut text = format!(
        "Stick:    {id}\nCreated:  {created}\nContest:  {} ({:?})\nComputer: {name} (machine {number})\n",
        s.contest.name, s.contest.platform,
    );
    if let Some(wallpaper) = &s.branding.wallpaper {
        let _ = writeln!(text, "Wallpaper: {wallpaper}");
    }
    let _ = writeln!(
        text,
        "Team:     {}\nSite:     {}\nServer:   {}\nRecording: {:?}\nAllowed sites: {}",
        s.team,
        s.site,
        s.api.url,
        s.recording.mode,
        s.network.allowed_domains.join(", ")
    );
    let _ = writeln!(
        text,
        "Bluetooth: {}",
        if s.block_bluetooth {
            "blocked"
        } else {
            "not blocked"
        }
    );
    let _ = writeln!(
        text,
        "USB storage: {}",
        if s.block_usb_storage {
            "blocked"
        } else {
            "not blocked"
        }
    );
    // Only the network name: the password stays on the stick.
    if let Some(wifi) = &s.wifi {
        let _ = writeln!(
            text,
            "Wi-Fi:    {}{}",
            wifi.ssid,
            if wifi.password.is_some() {
                " (password set)"
            } else {
                " (open)"
            }
        );
    }
    text
}
