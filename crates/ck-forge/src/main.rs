//! `ck-forge`: the admin command line. It only parses commands and prints results; the work is
//! done by `ck-admin-core`.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ck_admin_core::{
    Error, Profile, Registry, Wallpaper, build_image, configure_image, folder_of, show_image,
};
use ck_protocol::{LocalDateTime, StickConfig};
use clap::{Args, Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;

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
    /// List USB disks and write an image to a stick
    #[command(subcommand)]
    Stick(StickCommand),
}

#[derive(Subcommand)]
enum StickCommand {
    /// List the disks of this PC and say which ones may be written
    List,
    /// Write a stick image to a USB stick, then read it back to check it.
    /// Writing a disk needs an administrator terminal and erases the whole stick
    Write {
        /// The stick image to write
        image: PathBuf,
        /// The number of the disk to write (see `stick list`)
        #[arg(long, conflicts_with = "to", required_unless_present = "to")]
        disk: Option<u32>,
        /// Write to this file instead of a disk: tests the writer without a stick
        #[arg(long)]
        to: Option<PathBuf>,
        /// Also allow a virtual disk (a VHD), for testing the writer without a stick
        #[arg(long)]
        allow_virtual: bool,
        /// Do not ask for the confirmation (for scripts and tests)
        #[arg(long)]
        yes: bool,
        /// Keep a small JSON file up to date with the progress, for the admin window that starts
        /// this command with administrator rights and cannot read its output
        #[arg(long)]
        progress_file: Option<PathBuf>,
    },
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
    /// Send the registry to the Hub so it knows the sticks. Asks for the password at a prompt
    Push {
        /// The Hub's address, like `https://hub.example.org`
        #[arg(long)]
        hub: String,
        /// The admin account to log in with
        #[arg(long)]
        user: String,
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
        Command::Image(ImageCommand::Build {
            base,
            out,
            profile,
            registry,
        }) => image_build(&base, &out, &profile, &registry.registry),
        Command::Image(ImageCommand::Configure {
            image,
            profile,
            registry,
        }) => {
            let loaded = Profile::load(&profile)?;
            let wallpaper = Wallpaper::read(&loaded.stick_settings(), folder_of(&profile))?;
            let config = configure_image(&image, &loaded, wallpaper.as_ref(), &registry.registry)?;
            warn(&loaded);
            Ok(format!(
                "Updated {}\n{}",
                image.display(),
                config_text(&config)
            ))
        }
        Command::Image(ImageCommand::Show { image }) => Ok(config_text(&show_image(&image)?)),
        Command::Registry(RegistryCommand::List { registry }) => registry_list(&registry.registry),
        Command::Registry(RegistryCommand::Push {
            hub,
            user,
            registry,
        }) => registry_push(&hub, &user, &registry.registry),
        Command::Stick(StickCommand::List) => stick_list(),
        Command::Stick(StickCommand::Write {
            image,
            disk,
            to,
            allow_virtual,
            yes,
            progress_file,
        }) => {
            let progress = progress_file.as_deref();
            let result = stick_write(&image, disk, to, allow_virtual, yes, progress);
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

fn profile_new(file: &Path) -> Result<String, Error> {
    if file.exists() {
        return Err(Error::io(
            format!("{} already exists; choose another name", file.display()),
            io::Error::from(io::ErrorKind::AlreadyExists),
        ));
    }
    Profile::template().save(file)?;
    Ok(format!(
        "Wrote {0}\nEdit it, then run: ck-forge profile check {0}\n",
        file.display()
    ))
}

fn profile_check(file: &Path) -> Result<String, Error> {
    let profile = Profile::load(file)?;
    // The wallpaper is part of what the profile needs, so a missing picture is found here too.
    Wallpaper::read(&profile.stick_settings(), folder_of(file))?;
    warn(&profile);
    Ok(format!("{} is fine: {}\n", file.display(), profile.name))
}

fn warn(profile: &Profile) {
    for warning in profile.warnings() {
        eprintln!("warning: {warning}");
    }
}

fn image_build(
    base: &Path,
    image: &Path,
    profile: &Path,
    registry: &Path,
) -> Result<String, Error> {
    let profile_file = profile;
    let profile = Profile::load(profile_file)?;
    let wallpaper = Wallpaper::read(&profile.stick_settings(), folder_of(profile_file))?;
    warn(&profile);
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
    Ok(format!(
        "Made {}\nRegistered in {}\n{}",
        image.display(),
        registry.display(),
        config_text(&built.config)
    ))
}

/// The settings of a stick for the admin to read. The secret is never part of it.
fn config_text(config: &StickConfig) -> String {
    let s = &config.settings;
    let mut text = format!(
        "Stick:    {}\nCreated:  {}\nContest:  {} ({:?})\nComputer: {} (machine {})\n",
        config.stick_id,
        config.created_at,
        s.contest.name,
        s.contest.platform,
        config.computer_name(),
        config.machine_number.unwrap_or(1)
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
    text
}

fn registry_list(path: &Path) -> Result<String, Error> {
    let registry = Registry::load(path)?;
    if registry.list().is_empty() {
        return Ok(format!("No sticks in {} yet\n", path.display()));
    }
    let mut text = String::new();
    for entry in registry.list() {
        let _ = writeln!(
            text,
            "{}  {}  #{}  {} / {}  {}  {}",
            entry.stick_id,
            entry.created_at,
            entry.machine_number.unwrap_or(1),
            entry.team,
            entry.site,
            entry.contest,
            entry.image
        );
    }
    Ok(text)
}

fn stick_list() -> Result<String, Error> {
    let mut text = String::new();
    for disk in ck_writer::list_disks()? {
        let verdict = match disk.check(0, true) {
            Ok(()) => "can be written".to_owned(),
            Err(error) => format!("refused: {error}"),
        };
        let _ = writeln!(
            text,
            "disk {}  {}  {}  {}  serial {}  [{}]",
            disk.number,
            disk.friendly_name.as_deref().unwrap_or("?"),
            disk.size_text(),
            disk.bus_type,
            disk.serial_number.as_deref().unwrap_or("?").trim(),
            verdict
        );
    }
    Ok(text)
}

fn stick_write(
    image: &Path,
    disk: Option<u32>,
    to: Option<PathBuf>,
    allow_virtual: bool,
    yes: bool,
    progress: Option<&Path>,
) -> Result<String, Error> {
    let size = std::fs::metadata(image)
        .map_err(|e| Error::io(format!("reading {}", image.display()), e))?
        .len();
    let target = match (disk, to) {
        (_, Some(file)) => file,
        (Some(number), None) => {
            let chosen = ck_writer::list_disks()?
                .into_iter()
                .find(|d| d.number == number)
                .ok_or(ck_writer::Error::NoSuchDisk(number))?;
            chosen.check(size, allow_virtual)?;
            if !yes {
                confirm(&chosen)?;
            }
            ck_writer::prepare_disk(&chosen, size, allow_virtual).map_err(admin_hint)?;
            ck_writer::disk_path(number)
        }
        (None, None) => unreachable!("clap requires --disk or --to"),
    };
    let bar = ProgressBar::new(size);
    bar.set_style(
        ProgressStyle::with_template("{msg:>10} {bar:40} {bytes}/{total_bytes} ({eta})")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    let mut last_report = Instant::now();
    let written = ck_writer::write_image(image, &target, |phase, done, total| {
        let name = match phase {
            ck_writer::Phase::Writing => "writing",
            ck_writer::Phase::Verifying => "checking",
        };
        bar.set_length(total);
        bar.set_position(done);
        bar.set_message(name);
        // A file rewritten for every 4 MiB block would be needless work.
        if done == total || last_report.elapsed() >= Duration::from_millis(500) {
            last_report = Instant::now();
            report_progress(
                progress,
                &json!({ "state": name, "done": done, "total": total }),
            );
        }
    });
    bar.finish_and_clear();
    let hash = written.map_err(admin_hint)?;
    report_progress(progress, &json!({ "state": "done", "sha256": hash }));
    Ok(format!(
        "Wrote {} to {}\nRead back and checked, SHA-256 {hash}\n",
        image.display(),
        target.display()
    ))
}

/// Asks the admin to type the disk number, because this erases the whole disk.
fn confirm(disk: &ck_writer::Disk) -> Result<(), Error> {
    let mut out = io::stdout();
    write!(
        out,
        "This ERASES disk {}: {} ({}, serial {}).\nType erase {} to go on: ",
        disk.number,
        disk.friendly_name.as_deref().unwrap_or("?"),
        disk.size_text(),
        disk.serial_number.as_deref().unwrap_or("?").trim(),
        disk.number
    )
    .and_then(|()| out.flush())
    .map_err(|e| Error::io("writing the output", e))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| Error::io("reading the answer", e))?;
    if answer.trim() == format!("erase {}", disk.number) {
        Ok(())
    } else {
        Err(Error::io(
            "cancelled, nothing was written",
            io::Error::from(io::ErrorKind::Interrupted),
        ))
    }
}

/// Writing a disk without administrator rights fails with "access denied"; say what to do.
fn admin_hint(error: ck_writer::Error) -> Error {
    if let ck_writer::Error::Io { source, .. } = &error
        && source.kind() == io::ErrorKind::PermissionDenied
    {
        return Error::io(
            "access denied: open a terminal as administrator and run the command again",
            io::Error::from(io::ErrorKind::PermissionDenied),
        );
    }
    error.into()
}

fn registry_push(hub: &str, user: &str, registry: &Path) -> Result<String, Error> {
    // Without echo, and never from an argument or the environment, so it is not left in a history.
    let password = rpassword::prompt_password(format!("Password for {user} on {hub}: "))
        .map_err(|e| Error::io("reading the password", e))?;
    let pushed = ck_admin_core::push_registry(hub, user, &password, registry)?;
    let text = format!(
        "Hub: {} added, {} already known\n",
        pushed.added, pushed.already_known
    );
    if pushed.conflicts.is_empty() {
        return Ok(text);
    }
    io::stdout()
        .write_all(text.as_bytes())
        .map_err(|e| Error::io("writing the output", e))?;
    Err(Error::Hub(format!(
        "the Hub already has a different secret for: {}; they were left as they were",
        pushed.conflicts.join(", ")
    )))
}

/// Keeps the progress file of `stick write` up to date, when one was asked for. A failure to write it
/// is ignored: the write itself must not fail because the window stopped listening.
fn report_progress(file: Option<&Path>, state: &serde_json::Value) {
    if let Some(path) = file {
        let _ = std::fs::write(path, state.to_string());
    }
}
