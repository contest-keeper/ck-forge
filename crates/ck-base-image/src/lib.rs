//! Makes a bootable contest base image from an official distribution image, so an admin does not
//! need a contest image to start with.
//!
//! Steps: download and check the official cloud image, turn it into a raw disk, boot it once in
//! QEMU so a script can install the desktop and tools, optionally apply the Keeper OS patch, and
//! write a record of what the image was made from.

mod catalog;
mod detect;
mod download;
mod error;
mod import;
mod patch;
mod provision;
mod seed;
mod tools;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

pub use catalog::{Entry, catalog, find};
pub use detect::{
    DetectedImage, detect_image, requires_signed_settings, supports_enrolment_schema,
};
pub use download::sha256_file;
pub use error::Error;
pub use import::import_image;
pub use tools::Qemu;

/// How many times a crashed provisioning machine is tried again from the start.
const ATTEMPTS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Download,
    Convert,
    Provision,
    Patch,
    Hash,
}

pub struct Options {
    pub entry: Entry,
    /// The image to make. It must not exist.
    pub out: PathBuf,
    /// Where downloads are kept between builds.
    pub cache: PathBuf,
    pub qemu_dir: Option<PathBuf>,
    pub accel: String,
    pub memory_mb: u32,
    pub timeout: Duration,
    /// The release's `ck-os-patch.tar.gz`. Without it the image is not a Keeper OS image yet.
    pub patch: Option<PathBuf>,
}

/// What a base image was made from, written next to it as `<image>.base.json`.
#[derive(Debug, Serialize)]
pub struct Record {
    pub distro: String,
    pub source_url: String,
    pub source_sha256: String,
    pub script_sha256: String,
    pub patched: bool,
    pub size_bytes: u64,
    pub image_sha256: String,
}

pub fn record_path(image: &Path) -> PathBuf {
    let mut name = image.as_os_str().to_owned();
    name.push(".base.json");
    PathBuf::from(name)
}

fn work_dir(out: &Path) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(".work");
    PathBuf::from(name)
}

/// Builds the image. `progress` gets the step, the amount done and the total (0 when unknown).
/// A failed build leaves no image behind; its logs stay in `<out>.work`.
pub fn create_base_image(
    options: &Options,
    mut progress: impl FnMut(Step, u64, u64),
) -> Result<Record, Error> {
    if options.out.exists() {
        return Err(Error::Exists(options.out.display().to_string()));
    }
    let qemu = Qemu::new(options.qemu_dir.clone());
    let source = download::fetch(&options.entry, &options.cache, |done, total| {
        progress(Step::Download, done, total);
    })?;

    let work = work_dir(&options.out);
    fs::create_dir_all(&work).map_err(|e| Error::io("creating the work folder", e))?;
    let partial = options.out.with_extension("partial");
    let _ = fs::remove_file(&partial);

    let record = match build(options, &qemu, &source, &partial, &work, &mut progress) {
        Ok(record) => record,
        Err(error) => {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
    };
    publish(&partial, &options.out)?;
    let text = serde_json::to_string_pretty(&record)?;
    fs::write(record_path(&options.out), text).map_err(|e| Error::io("writing the record", e))?;
    // The logs are only for a failed build.
    let _ = fs::remove_dir_all(&work);
    Ok(record)
}

/// Gives the finished image its name without replacing a file that appeared there since the start
/// of the build: a hard link fails when `out` exists, where a rename would silently replace it.
fn publish(partial: &Path, out: &Path) -> Result<(), Error> {
    fs::hard_link(partial, out).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => Error::Exists(out.display().to_string()),
        _ => Error::io("naming the finished image", e),
    })?;
    fs::remove_file(partial).map_err(|e| Error::io("removing the unfinished image name", e))
}

fn build(
    options: &Options,
    qemu: &Qemu,
    source: &Path,
    image: &Path,
    work: &Path,
    progress: &mut impl FnMut(Step, u64, u64),
) -> Result<Record, Error> {
    let machine = provision::Machine {
        qemu,
        accel: &options.accel,
        memory_mb: options.memory_mb,
        timeout: options.timeout,
    };
    // A crashed machine leaves a half-installed disk, so each attempt starts from the download.
    for attempt in 1..=ATTEMPTS {
        progress(Step::Convert, 0, 0);
        qemu.make_raw(source, image, options.entry.disk_gb)?;
        let result = provision::provision(&machine, &options.entry, image, work, |done, total| {
            progress(Step::Provision, done, total);
        });
        match result {
            Err(Error::MachineStopped(_)) if attempt < ATTEMPTS => {
                fs::remove_file(image).map_err(|e| Error::io("removing the failed disk", e))?;
            }
            other => {
                other?;
                break;
            }
        }
    }

    if let Some(patch) = &options.patch {
        progress(Step::Patch, 0, 0);
        patch::apply(image, patch)?;
    }

    progress(Step::Hash, 0, 0);
    let size_bytes = fs::metadata(image)
        .map_err(|e| Error::io("reading the image size", e))?
        .len();
    let script_hash = Sha256::digest(options.entry.script()?.as_bytes());
    Ok(Record {
        distro: options.entry.id.clone(),
        source_url: options.entry.url.clone(),
        source_sha256: options.entry.sha256.clone(),
        script_sha256: download::hex(&script_hash),
        patched: options.patch.is_some(),
        size_bytes,
        image_sha256: sha256_file(image)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_image_never_replaces_a_file_that_appeared_meanwhile() {
        let dir = tempfile::tempdir().expect("folder");
        let (partial, out) = (dir.path().join("a.partial"), dir.path().join("a.img"));
        fs::write(&partial, b"new").expect("partial");
        fs::write(&out, b"mine").expect("existing");
        assert!(matches!(publish(&partial, &out), Err(Error::Exists(_))));
        assert_eq!(fs::read(&out).expect("kept"), b"mine");
        fs::remove_file(&out).expect("remove");
        publish(&partial, &out).expect("publish");
        assert_eq!(fs::read(&out).expect("image"), b"new");
        assert!(!partial.exists());
    }
}
