//! `ck-forge`: the admin command line. It only parses commands and prints results; the work is
//! done by `ck-admin-core`.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ck_admin_core::{
    Error, Profile, Registry, Wallpaper, build_image, build_thin_image, check_one_stick,
    configure_image, folder_of, show_image,
};
use ck_protocol::{LocalDateTime, StickConfig};
use clap::{Args, Parser, Subcommand};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
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
    /// Write a stick image to USB sticks, then read them back to check them.
    /// Writing a disk needs an administrator terminal and erases the whole stick. Several sticks
    /// are written at the same time, so a batch takes about as long as one stick
    Write {
        /// The stick image to write. For several sticks each with its own identity give one image
        /// per stick, in the order of the disks or files; they must be made from the same base
        #[arg(required = true)]
        images: Vec<PathBuf>,
        /// The number of a disk to write (see `stick list`). Repeat the option to write several
        /// sticks at once
        #[arg(long, conflicts_with = "to", required_unless_present = "to")]
        disk: Vec<u32>,
        /// Write to this file instead of a disk: tests the writer without a stick. Repeat the
        /// option to write several files at once
        #[arg(long)]
        to: Vec<PathBuf>,
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
    /// Make the image of a stick: the base image with its own identity and the profile's settings.
    /// By default the new image holds only the start of the base (through its EFI partition) and
    /// `stick write` takes the rest from the base, which takes a second instead of minutes
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
        /// Copy the whole base image (30 GB), so the new image runs without the base
        #[arg(long)]
        full_copy: bool,
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
            full_copy,
            registry,
        }) => image_build(&base, &out, &profile, &registry.registry, full_copy),
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
    full_copy: bool,
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
    let make = if full_copy {
        build_image
    } else {
        build_thin_image
    };
    let result = make(
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
    let built = result?;
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
    images: &[PathBuf],
    disks: &[u32],
    to: Vec<PathBuf>,
    allow_virtual: bool,
    yes: bool,
    progress: Option<&Path>,
) -> Result<String, Error> {
    let count = disks.len().max(to.len());
    let images = images_for(images, count)?;
    // Before anything is erased: the images must fit together, and a base that moved is found now.
    let size = ck_writer::check_images(&images)?;
    let targets = if to.is_empty() {
        prepare_disks(disks, size, allow_virtual, yes)?
    } else {
        to
    };
    let jobs: Vec<(PathBuf, PathBuf)> = images.into_iter().zip(targets).collect();
    let targets: Vec<&PathBuf> = jobs.iter().map(|(_, target)| target).collect();

    let bars = MultiProgress::new();
    let bars: Vec<ProgressBar> = targets
        .iter()
        .map(|target| {
            let bar = bars.add(ProgressBar::new(size));
            bar.set_style(
                ProgressStyle::with_template(
                    "{prefix:>16} {msg:>9} {bar:30} {bytes}/{total_bytes} ({eta})",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
            );
            bar.set_prefix(label(target));
            bar
        })
        .collect();
    // For each target whether it is checking and how far it is; the slowest one is reported.
    let mut state = vec![(false, 0_u64); targets.len()];
    let mut last_report = Instant::now();
    let results = ck_writer::write_images(&jobs, |index, phase, done, total| {
        let checking = phase == ck_writer::Phase::Verifying;
        let name = if checking { "checking" } else { "writing" };
        bars[index].set_length(total);
        bars[index].set_position(done);
        bars[index].set_message(name);
        state[index] = (checking, done);
        // A file rewritten for every 4 MiB block would be needless work.
        if done == total || last_report.elapsed() >= Duration::from_millis(500) {
            last_report = Instant::now();
            let (checking, done) = state.iter().copied().min().unwrap_or_default();
            let name = if checking { "checking" } else { "writing" };
            report_progress(
                progress,
                &json!({ "state": name, "done": done, "total": total }),
            );
        }
    });
    for bar in &bars {
        bar.finish_and_clear();
    }

    let mut text = String::new();
    let mut failures = Vec::new();
    let mut hash = String::new();
    for ((image, target), result) in jobs.iter().zip(results.map_err(admin_hint)?) {
        match result {
            Ok(sha256) => {
                let _ = writeln!(text, "Wrote {} to {}", image.display(), target.display());
                hash = sha256;
            }
            Err(error) => failures.push(format!("{}: {}", label(target), admin_hint(error))),
        }
    }
    if !failures.is_empty() {
        let message = format!(
            "{} of {} sticks failed ({}{})",
            failures.len(),
            targets.len(),
            failures.join("; "),
            if text.is_empty() {
                String::new()
            } else {
                format!("). The others are written and checked, {}", text.trim_end())
            }
        );
        return Err(Error::io("writing the sticks", io::Error::other(message)));
    }
    report_progress(progress, &json!({ "state": "done", "sha256": hash }));
    if jobs.len() == 1 || jobs.windows(2).all(|pair| pair[0].0 == pair[1].0) {
        let _ = writeln!(text, "Read back and checked, SHA-256 {hash}");
    } else {
        let _ = writeln!(
            text,
            "Every stick was read back and checked against its own image"
        );
    }
    Ok(text)
}

/// One image for every target, in order: the one image given, or one image per target. Several
/// sticks written from one image that holds a stick's identity, or from the same image twice,
/// would share an id and secret, so that is refused.
fn images_for(images: &[PathBuf], count: usize) -> Result<Vec<PathBuf>, Error> {
    if images.len() == 1 {
        check_one_stick(&images[0], count)?;
        return Ok(vec![images[0].clone(); count]);
    }
    if images.len() != count {
        return Err(Error::io(
            format!("{} images for {count} sticks", images.len()),
            io::Error::other("give one image for all the sticks, or one image for each stick"),
        ));
    }
    for (index, image) in images.iter().enumerate() {
        if images[..index].contains(image) {
            return Err(Error::io(
                format!("{} is given twice", image.display()),
                io::Error::other("two sticks written from one image would share its id and secret"),
            ));
        }
    }
    Ok(images.to_vec())
}

/// A short name for a target: `disk 3` for `\\.\PhysicalDrive3`, else the file name.
fn label(target: &Path) -> String {
    let name = target.file_name().map_or_else(
        || target.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    match name.strip_prefix("PhysicalDrive") {
        Some(number) => format!("disk {number}"),
        None => name,
    }
}

/// Looks every disk up and checks that it may be written, asks once for all of them, then gets
/// each ready. Nothing is touched before every disk has passed the checks. Returns the device
/// paths.
fn prepare_disks(
    numbers: &[u32],
    size: u64,
    allow_virtual: bool,
    yes: bool,
) -> Result<Vec<PathBuf>, Error> {
    let listed = ck_writer::list_disks()?;
    let mut chosen: Vec<ck_writer::Disk> = Vec::new();
    for &number in numbers {
        if chosen.iter().any(|d| d.number == number) {
            return Err(Error::io(
                format!("disk {number} is given twice"),
                io::Error::from(io::ErrorKind::InvalidInput),
            ));
        }
        let disk = listed
            .iter()
            .find(|d| d.number == number)
            .cloned()
            .ok_or(ck_writer::Error::NoSuchDisk(number))?;
        disk.check(size, allow_virtual)?;
        chosen.push(disk);
    }
    if !yes {
        confirm(&chosen)?;
    }
    for disk in &chosen {
        ck_writer::prepare_disk(disk, size, allow_virtual).map_err(admin_hint)?;
    }
    Ok(chosen
        .iter()
        .map(|d| ck_writer::disk_path(d.number))
        .collect())
}

/// Asks the admin to type the disk numbers, because this erases the whole disks.
fn confirm(disks: &[ck_writer::Disk]) -> Result<(), Error> {
    let mut out = io::stdout();
    let mut numbers = Vec::new();
    for disk in disks {
        writeln!(
            out,
            "This ERASES disk {}: {} ({}, serial {}).",
            disk.number,
            disk.friendly_name.as_deref().unwrap_or("?"),
            disk.size_text(),
            disk.serial_number.as_deref().unwrap_or("?").trim(),
        )
        .map_err(|e| Error::io("writing the output", e))?;
        numbers.push(disk.number.to_string());
    }
    let expected = format!("erase {}", numbers.join(" "));
    write!(out, "Type {expected} to go on: ")
        .and_then(|()| out.flush())
        .map_err(|e| Error::io("writing the output", e))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| Error::io("reading the answer", e))?;
    if answer.trim() == expected {
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
