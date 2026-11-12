//! `ck-forge`: the admin command line. It only parses commands and prints results; the work is
//! done by `ck-admin-core`.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use ck_admin_core::{
    Error, Profile, Registry, Wallpaper, build_image, configure_image, show_image,
};
use ck_protocol::{LocalDateTime, StickConfig};
use clap::{Args, Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};

const DEFAULT_REGISTRY: &str = "ck-registry.json";

#[derive(Parser)]
#[command(
    name = "ck-forge",
    version,
    about = "Makes and checks Contest Keeper sticks"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Work with contest profiles (the settings for one contest)
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Make and inspect stick images
    #[command(subcommand)]
    Image(ImageCommand),
    /// See the sticks that were made
    #[command(subcommand)]
    Registry(RegistryCommand),
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// Write a new profile with default settings, ready to edit
    New {
        /// Where to write the profile (an existing file is never replaced)
        file: PathBuf,
    },
    /// Check a profile and list its problems
    Check {
        /// The profile to check
        file: PathBuf,
    },
}

#[derive(Subcommand)]
enum ImageCommand {
    /// Copy the base image and give the copy its own identity and the profile's settings
    Build {
        /// The base image (only read, never changed)
        #[arg(long)]
        base: PathBuf,
        /// The new image to create (must not exist)
        #[arg(long)]
        out: PathBuf,
        /// The contest profile
        #[arg(long)]
        profile: PathBuf,
        #[command(flatten)]
        registry: RegistryFile,
    },
    /// Replace the settings on a stick image, keeping its identity
    Configure {
        /// The stick image to change
        image: PathBuf,
        /// The contest profile
        #[arg(long)]
        profile: PathBuf,
        #[command(flatten)]
        registry: RegistryFile,
    },
    /// Print the settings on a stick image (the secret is not shown)
    Show {
        /// The stick image to read
        image: PathBuf,
    },
}

#[derive(Subcommand)]
enum RegistryCommand {
    /// List the sticks that were made
    List {
        #[command(flatten)]
        registry: RegistryFile,
    },
}

#[derive(Args)]
struct RegistryFile {
    /// The registry file. It holds every stick's secret, so keep it private. A file with `.lock`
    /// added to its name stays next to it, which is how two runs at once take turns
    #[arg(long, default_value = DEFAULT_REGISTRY)]
    registry: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command, &mut io::stdout()) {
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

fn run(command: Command, out: &mut impl Write) -> Result<(), Error> {
    match command {
        Command::Profile(ProfileCommand::New { file }) => profile_new(&file, out),
        Command::Profile(ProfileCommand::Check { file }) => profile_check(&file, out),
        Command::Image(ImageCommand::Build {
            base,
            out: image,
            profile,
            registry,
        }) => image_build(&base, &image, &profile, &registry.registry, out),
        Command::Image(ImageCommand::Configure {
            image,
            profile,
            registry,
        }) => {
            let loaded = Profile::load(&profile)?;
            let wallpaper = Wallpaper::read(&loaded.stick_settings(), folder_of(&profile))?;
            let config = configure_image(&image, &loaded, wallpaper.as_ref(), &registry.registry)?;
            warn(&loaded);
            say(out, format_args!("Updated {}", image.display()))?;
            show_config(&config, out)
        }
        Command::Image(ImageCommand::Show { image }) => show_config(&show_image(&image)?, out),
        Command::Registry(RegistryCommand::List { registry }) => {
            registry_list(&registry.registry, out)
        }
    }
}

fn say(out: &mut impl Write, text: std::fmt::Arguments) -> Result<(), Error> {
    writeln!(out, "{text}").map_err(|e| Error::io("writing the output", e))
}

fn profile_new(file: &std::path::Path, out: &mut impl Write) -> Result<(), Error> {
    if file.exists() {
        return Err(Error::io(
            format!("{} already exists; choose another name", file.display()),
            io::Error::from(io::ErrorKind::AlreadyExists),
        ));
    }
    Profile::template().save(file)?;
    say(out, format_args!("Wrote {}", file.display()))?;
    say(
        out,
        format_args!(
            "Edit it, then run: ck-forge profile check {}",
            file.display()
        ),
    )
}

fn profile_check(file: &std::path::Path, out: &mut impl Write) -> Result<(), Error> {
    let profile = Profile::load(file)?;
    // The wallpaper is part of what the profile needs, so a missing picture is found here too.
    Wallpaper::read(&profile.stick_settings(), folder_of(file))?;
    warn(&profile);
    say(
        out,
        format_args!("{} is fine: {}", file.display(), profile.name),
    )
}

/// The folder a profile file is in, where the pictures it names are looked for.
fn folder_of(file: &std::path::Path) -> &std::path::Path {
    match file.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => std::path::Path::new("."),
    }
}

fn warn(profile: &Profile) {
    for warning in profile.warnings() {
        eprintln!("warning: {warning}");
    }
}

fn image_build(
    base: &std::path::Path,
    image: &std::path::Path,
    profile: &std::path::Path,
    registry: &std::path::Path,
    out: &mut impl Write,
) -> Result<(), Error> {
    let loaded = Profile::load(profile)?;
    let wallpaper = Wallpaper::read(&loaded.stick_settings(), folder_of(profile))?;
    warn(&loaded);
    let profile = loaded;
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template("{bar:40} {bytes}/{total_bytes} ({eta})")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    let now = LocalDateTime::from(chrono::Local::now().naive_local());
    let built = build_image(
        base,
        image,
        &profile,
        wallpaper.as_ref(),
        registry,
        now,
        |done, total| {
            bar.set_length(total);
            bar.set_position(done);
        },
    );
    bar.finish_and_clear();
    let built = built?;
    say(out, format_args!("Made {}", image.display()))?;
    say(out, format_args!("Registered in {}", registry.display()))?;
    show_config(&built.config, out)
}

fn show_config(config: &StickConfig, out: &mut impl Write) -> Result<(), Error> {
    let s = &config.settings;
    say(out, format_args!("Stick:    {}", config.stick_id))?;
    say(out, format_args!("Created:  {}", config.created_at))?;
    say(
        out,
        format_args!("Contest:  {} ({:?})", s.contest.name, s.contest.platform),
    )?;
    say(
        out,
        format_args!(
            "Computer: {} (machine {})",
            config.computer_name(),
            config.machine_number.unwrap_or(1)
        ),
    )?;
    if let Some(wallpaper) = &s.branding.wallpaper {
        say(out, format_args!("Wallpaper: {wallpaper}"))?;
    }
    say(out, format_args!("Team:     {}", s.team))?;
    say(out, format_args!("Site:     {}", s.site))?;
    say(out, format_args!("Server:   {}", s.api.url))?;
    say(out, format_args!("Recording: {:?}", s.recording.mode))?;
    say(
        out,
        format_args!("Allowed sites: {}", s.network.allowed_domains.join(", ")),
    )
}

fn registry_list(path: &std::path::Path, out: &mut impl Write) -> Result<(), Error> {
    let registry = Registry::load(path)?;
    if registry.list().is_empty() {
        return say(out, format_args!("No sticks in {} yet", path.display()));
    }
    for entry in registry.list() {
        say(
            out,
            format_args!(
                "{}  {}  #{}  {} / {}  {}  {}",
                entry.stick_id,
                entry.created_at,
                entry.machine_number.unwrap_or(1),
                entry.team,
                entry.site,
                entry.contest,
                entry.image
            ),
        )?;
    }
    Ok(())
}
