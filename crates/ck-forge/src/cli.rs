//! The command line the admin types: commands and their options.

use std::path::PathBuf;

use ck_admin_core::{Error, Profile};
use ck_protocol::{Wifi, WifiPassword};
use clap::{Args, Parser, Subcommand};

pub(crate) const DEFAULT_REGISTRY: &str = "ck-registry.json";

#[derive(Parser)]
#[command(
    name = "ck-forge",
    version,
    about = "Makes and checks Contest Keeper sticks"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
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
    /// Sign in to Keeper Hub and use the profiles, sticks and images of the organization
    #[command(subcommand)]
    Hub(HubCommand),
}

#[derive(Subcommand)]
pub(crate) enum HubCommand {
    /// Sign in through the browser; the session is kept in the Windows Credential Manager
    SignIn,
    /// Sign out and forget the saved session
    SignOut,
    /// Say who is signed in
    Status,
    /// List the profiles of the organization
    Profiles,
    /// Download a profile into a folder, with its wallpaper
    Pull {
        /// The id shown by `hub profiles`
        id: String,
        /// The folder to put the profile's own folder in
        #[arg(long, default_value = ".")]
        folder: PathBuf,
    },
    /// Save a profile file to the Hub, with its wallpaper
    Push {
        /// The profile to save
        file: PathBuf,
    },
    /// List the sticks of the organization
    Sticks,
    /// Retire the key of a stick, so it can no longer report
    Retire {
        /// The stick id shown by `hub sticks`
        id: String,
    },
    /// List the base images the Hub offers
    Images,
    /// Download a base image from the Hub, verify it and expand it to a raw .img
    Download {
        /// The id shown by `hub images`
        id: String,
        /// Where to save the raw .img (must not exist)
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
pub(crate) enum StickCommand {
    /// List the disks of this computer and say which ones may be written
    List,
    /// Write a stick image to USB sticks, then read them back to check them.
    /// Writing a disk needs an administrator terminal (`sudo` on macOS) and erases the whole stick. Several sticks
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
        /// Also allow a virtual disk (a VHD, or a disk image on macOS), for testing the writer without a stick
        #[arg(long)]
        allow_virtual: bool,
        /// Do not ask for the confirmation (for scripts and tests)
        #[arg(long)]
        yes: bool,
        /// Keep a small JSON file up to date with the progress, for the admin window that starts
        /// this command with administrator rights and cannot read its output
        #[arg(long)]
        progress_file: Option<PathBuf>,
        /// Stop writing when this file exists: the admin window, which cannot reach this process
        /// once it has administrator rights, creates it to cancel. The sticks are then only partly
        /// written and must be written again
        #[arg(long)]
        cancel_file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub(crate) enum ProfileCommand {
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
    /// Copy a picture next to a profile and make it the profile's wallpaper
    Wallpaper {
        /// The profile to change
        file: PathBuf,
        /// A PNG, JPG, WEBP or BMP picture
        picture: PathBuf,
    },
}

#[derive(Subcommand)]
pub(crate) enum ImageCommand {
    /// Verify a downloaded Keeper OS .img.zst archive and expand it to a raw base image
    Import {
        /// The downloaded archive
        archive: PathBuf,
        /// The SHA-256 shown beside the download on Keeper Hub
        #[arg(long)]
        sha256: String,
        /// Where to save the raw .img (must not exist)
        #[arg(long)]
        out: PathBuf,
    },
    /// Identify a raw image by SHA-256 or by os-release on its Linux filesystem
    Detect {
        /// Raw disk image to inspect
        image: PathBuf,
    },
    /// Make the image of a stick: the base image with its own identity and the profile's settings.
    /// By default the new image holds only the start of the base (through its settings partition) and
    /// `stick write` takes the rest from the base, which takes a second instead of minutes
    Build {
        /// The raw base image (expand an .img.zst download with `image import` first)
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
        #[command(flatten)]
        extras: ExtraSettings,
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
        #[command(flatten)]
        extras: ExtraSettings,
    },
    /// List the base images Keeper can make for you
    Catalog,
    /// Make a base image from an official distribution image, when you have no contest image.
    /// Downloads it (about 600 MB), boots it once in QEMU to install a desktop and the contest
    /// tools (15 to 40 minutes), then applies the Keeper OS patch. Needs QEMU and 30 GB free
    Create {
        /// Which image to make (see `image catalog`)
        #[arg(long)]
        distro: String,
        /// The base image to create (must not exist)
        #[arg(long)]
        out: PathBuf,
        /// The release's ck-os-patch.tar.gz. Without it the image is a plain desktop, not Keeper OS.
        /// Applying it needs WSL
        #[arg(long)]
        patch: Option<PathBuf>,
        /// Where downloads are kept (default: a folder under %LOCALAPPDATA%)
        #[arg(long)]
        cache: Option<PathBuf>,
        /// The QEMU folder (default: C:\Program Files\qemu)
        #[arg(long)]
        qemu: Option<PathBuf>,
        /// QEMU acceleration: whpx on Windows, kvm on Linux, tcg works everywhere but is slow
        #[arg(long, default_value = "whpx")]
        accel: String,
        /// Give up on the installation after this many minutes
        #[arg(long, default_value_t = 90)]
        timeout_minutes: u64,
    },
    /// Say whether a base image needs Hub-signed settings and supports first-boot enrolment
    Support {
        /// The base image to read
        base: PathBuf,
    },
    /// Print the settings on a stick image (the secret is not shown)
    Show {
        /// The stick image to read
        image: PathBuf,
    },
}

#[derive(Subcommand)]
pub(crate) enum RegistryCommand {
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

/// Settings that are given for one build, on top of the profile.
#[derive(Args)]
pub(crate) struct ExtraSettings {
    /// Block Bluetooth on the stick, whatever the profile says
    #[arg(long)]
    block_bluetooth: bool,
    /// Block USB storage devices other than the stick itself, whatever the profile says
    #[arg(long)]
    block_usb_storage: bool,
    /// The Wi-Fi network the stick joins (replaces the profile's)
    #[arg(long)]
    wifi_ssid: Option<String>,
    /// Ask for the Wi-Fi password at a prompt. The password is only written on the stick: it is not
    /// saved in the profile, not printed and not sent to the Hub
    #[arg(long)]
    ask_wifi_password: bool,
}

impl ExtraSettings {
    /// The profile with these settings applied. The password is read without echo and never from an
    /// argument or the environment, so it is not left in a history.
    pub(crate) fn apply(&self, profile: &mut Profile) -> Result<(), Error> {
        if self.block_bluetooth {
            profile.settings.block_bluetooth = true;
        }
        if self.block_usb_storage {
            profile.settings.block_usb_storage = true;
        }
        if let Some(ssid) = &self.wifi_ssid {
            profile.settings.wifi = Some(Wifi {
                ssid: ssid.clone(),
                password: None,
            });
        }
        if self.ask_wifi_password {
            if profile.settings.wifi.is_none() {
                return Err(Error::NoWifiNetwork);
            }
            let password = rpassword::prompt_password("Wi-Fi password: ")
                .map_err(|e| Error::io("reading the password", e))?;
            profile.set_wifi_password(Some(WifiPassword::new(password)));
        }
        let problems = profile.problems();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(Error::InvalidProfile(problems))
        }
    }
}

#[derive(Args)]
pub(crate) struct RegistryFile {
    /// The registry file. It holds every stick's secret, so keep it private. A file with `.lock`
    /// added to its name stays next to it, which is how two runs at once take turns
    #[arg(long, default_value = DEFAULT_REGISTRY)]
    pub(crate) registry: PathBuf,
}
