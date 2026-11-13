//! Writes a stick image to a USB stick, safely, and checks the result by reading it back.
//!
//! The target is either a disk (`\.\PhysicalDriveN`) or a plain file. A file needs no
//! administrator rights, which is how the write and the check are tested without a stick.

mod disk;
mod error;
mod write;

pub use disk::{Disk, list_disks, prepare_disk};
pub use error::Error;
pub use write::{Phase, disk_path, write_image};
