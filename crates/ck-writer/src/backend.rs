//! What differs between operating systems: how disks are listed, released and opened.

use std::path::PathBuf;

use crate::{Disk, Error};

/// The operating system's way to find disks and get one ready for a raw write. The rules about
/// which disk may be written live in [`Disk::check`], the same on every system.
pub(crate) trait DiskBackend {
    /// Every disk of this computer. Needs no administrator rights.
    fn list(&self) -> Result<Vec<Disk>, Error>;

    /// Releases `chosen` so that it can be opened as a raw device. Already checked by the caller.
    /// Returns what went wrong, if anything.
    fn prepare(&self, chosen: &Disk) -> Result<(), String>;

    /// The path that opens disk `number` as a raw device.
    fn raw_path(&self, number: u32) -> PathBuf;
}

/// The backend of the system this runs on.
pub(crate) fn system() -> &'static dyn DiskBackend {
    #[cfg(windows)]
    {
        &crate::windows::Windows
    }
    #[cfg(not(windows))]
    {
        &Unsupported
    }
}

/// Systems that have no way to write a stick yet.
#[cfg(not(windows))]
struct Unsupported;

#[cfg(not(windows))]
impl DiskBackend for Unsupported {
    fn list(&self) -> Result<Vec<Disk>, Error> {
        Err(Error::Listing(
            "writing sticks is not supported on this system".to_owned(),
        ))
    }

    fn prepare(&self, _chosen: &Disk) -> Result<(), String> {
        Err("writing sticks is not supported on this system".to_owned())
    }

    fn raw_path(&self, number: u32) -> PathBuf {
        PathBuf::from(format!("/dev/disk{number}"))
    }
}

/// A backend for tests: disks held in memory, whose raw device of disk `N` is the file `<raw>N`.
#[cfg(test)]
pub(crate) mod fake {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::{Disk, DiskBackend, Error};

    pub(crate) struct Fake {
        raw: PathBuf,
        disks: Mutex<Vec<Disk>>,
        prepared: Mutex<Vec<u32>>,
    }

    impl Fake {
        pub(crate) fn new(raw: impl AsRef<Path>, disks: Vec<Disk>) -> Self {
            Self {
                raw: raw.as_ref().to_path_buf(),
                disks: Mutex::new(disks),
                prepared: Mutex::new(Vec::new()),
            }
        }

        pub(crate) fn prepared(&self) -> Vec<u32> {
            self.prepared.lock().unwrap().clone()
        }
    }

    impl DiskBackend for Fake {
        fn list(&self) -> Result<Vec<Disk>, Error> {
            Ok(self.disks.lock().unwrap().clone())
        }

        fn prepare(&self, chosen: &Disk) -> Result<(), String> {
            self.prepared.lock().unwrap().push(chosen.number);
            Ok(())
        }

        fn raw_path(&self, number: u32) -> PathBuf {
            let mut name = self.raw.clone().into_os_string();
            name.push(number.to_string());
            PathBuf::from(name)
        }
    }
}
