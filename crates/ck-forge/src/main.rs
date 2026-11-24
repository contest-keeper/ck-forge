//! `ck-forge`: the admin command line. It only parses commands and prints results; the work is
//! done by `ck-admin-core`.

mod cli;
mod image;
mod profile;
mod registry;
mod stick;

use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use ck_admin_core::{
    CreateOptions, Error, Profile, Wallpaper, configure_image, detect_image, folder_of,
};
use clap::Parser;
use serde_json::json;

use cli::{Cli, Command, ImageCommand, ProfileCommand, RegistryCommand, StickCommand};
use image::{
    config_text, default_cache, image_build, image_catalog, image_create, image_import,
    show_config, warn,
};
use profile::{profile_check, profile_new, profile_wallpaper};
use registry::{registry_list, registry_push};
use stick::{report_progress, stick_list, stick_write};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = run(cli.command).and_then(|text| {
        io::stdout()
            .write_all(text.as_bytes())
            .map_err(|e| Error::io("writing the output", e))
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error);
            ExitCode::FAILURE
        }
    }
}

/// Plain words for the admin: one line per problem with a profile, one line for anything else.
fn report(error: &Error) {
    if let Error::InvalidProfile(problems) = error {
        eprintln!("error: the profile has problems:");
        for problem in problems {
            eprintln!("  - {problem}");
        }
    } else {
        eprintln!("error: {error}");
    }
}

/// Does the command and returns what to print.
fn run(command: Command) -> Result<String, Error> {
    match command {
        Command::Profile(ProfileCommand::New { file }) => profile_new(&file),
        Command::Profile(ProfileCommand::Check { file }) => profile_check(&file),
        Command::Profile(ProfileCommand::Wallpaper { file, picture }) => {
            profile_wallpaper(&file, &picture)
        }
        Command::Image(ImageCommand::Import {
            archive,
            sha256,
            out,
        }) => image_import(&archive, &out, &sha256),
        Command::Image(ImageCommand::Build {
            base,
            out,
            profile,
            full_copy,
            registry,
            extras,
        }) => image_build(
            &base,
            &out,
            &profile,
            &registry.registry,
            full_copy,
            &extras,
        ),
        Command::Image(ImageCommand::Configure {
            image,
            profile,
            registry,
            extras,
        }) => {
            let mut loaded = Profile::load(&profile)?;
            extras.apply(&mut loaded)?;
            let wallpaper = Wallpaper::read(&loaded.stick_settings(), folder_of(&profile))?;
            let config = configure_image(
                &image,
                &loaded,
                wallpaper.as_ref(),
                &registry.registry,
                None,
            )?;
            warn(&loaded);
            Ok(format!(
                "Updated {}\n{}",
                image.display(),
                config_text(&config)
            ))
        }
        Command::Image(ImageCommand::Catalog) => image_catalog(),
        Command::Image(ImageCommand::Detect { image }) => {
            Ok(format!("{}\n", detect_image(&image)?))
        }
        Command::Image(ImageCommand::Create {
            distro,
            out,
            patch,
            cache,
            qemu,
            accel,
            timeout_minutes,
        }) => image_create(&CreateOptions {
            entry: ck_admin_core::find_catalog_entry(&distro)?,
            out,
            cache: cache.unwrap_or_else(default_cache),
            qemu_dir: qemu,
            accel,
            memory_mb: 4096,
            timeout: Duration::from_secs(timeout_minutes * 60),
            patch,
        }),
        Command::Image(ImageCommand::Show { image }) => show_config(&image),
        Command::Registry(RegistryCommand::List { registry }) => registry_list(&registry.registry),
        Command::Registry(RegistryCommand::Push {
            hub,
            user,
            registry,
        }) => registry_push(&hub, &user, &registry.registry),
        Command::Stick(StickCommand::List) => stick_list(),
        Command::Stick(StickCommand::Write {
            images,
            disk,
            to,
            allow_virtual,
            yes,
            progress_file,
        }) => {
            let progress = progress_file.as_deref();
            let result = stick_write(&images, &disk, to, allow_virtual, yes, progress);
            if let Err(error) = &result {
                report_progress(
                    progress,
                    &json!({ "state": "error", "message": error.to_string() }),
                );
            }
            result
        }
    }
}
