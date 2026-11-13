use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::Error;

/// Disks only take whole sectors, so the last block is padded with zeros to this size.
const SECTOR: usize = 512;
/// Large blocks keep a slow stick busy; the size is a whole number of sectors.
const BLOCK: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Writing,
    Verifying,
}

/// The path that opens disk `number` as a raw device. The two leading backslashes matter: with one,
/// Windows takes it for a file named `PhysicalDriveN` on the current drive.
#[must_use]
pub fn disk_path(number: u32) -> PathBuf {
    PathBuf::from(format!(r"\\.\PhysicalDrive{number}"))
}

/// Copies `image` to `target`, then reads `target` back and compares. Returns the image's SHA-256
/// as hex. `progress` gets the phase, the bytes done and the total.
///
/// `target` can be a device (see [`disk_path`]), which must already be prepared by
/// [`crate::prepare_disk`], or a file, which is created if it does not exist. The last block is
/// padded to a whole sector. A device may be larger than the image; only the image's bytes are
/// written and checked.
pub fn write_image(
    image: &Path,
    target: &Path,
    mut progress: impl FnMut(Phase, u64, u64),
) -> Result<String, Error> {
    let mut source =
        File::open(image).map_err(|e| Error::io(format!("opening {}", image.display()), e))?;
    let total = source
        .metadata()
        .map_err(|e| Error::io(format!("reading the size of {}", image.display()), e))?
        .len();
    if total == 0 {
        return Err(Error::EmptyImage);
    }
    let mut out = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(target)
        .map_err(|e| Error::io(format!("opening {} for writing", target.display()), e))?;

    let written = copy(&mut source, &mut out, total, &mut progress, target)?;
    out.sync_all()
        .map_err(|e| Error::io(format!("flushing {}", target.display()), e))?;
    drop(out);

    let read_back = read_back(target, total, &mut progress)?;
    if read_back != written {
        return Err(Error::Mismatch { written, read_back });
    }
    Ok(written)
}

fn copy(
    source: &mut File,
    out: &mut File,
    total: u64,
    progress: &mut impl FnMut(Phase, u64, u64),
    target: &Path,
) -> Result<String, Error> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; BLOCK];
    let mut done = 0_u64;
    while done < total {
        let want = usize::try_from(total - done).map_or(BLOCK, |left| left.min(BLOCK));
        source
            .read_exact(&mut buffer[..want])
            .map_err(|e| Error::io("reading the image", e))?;
        hash.update(&buffer[..want]);
        let padded = want.next_multiple_of(SECTOR);
        buffer[want..padded].fill(0);
        out.write_all(&buffer[..padded])
            .map_err(|e| Error::io(format!("writing to {}", target.display()), e))?;
        done += want as u64;
        progress(Phase::Writing, done, total);
    }
    Ok(hex(&hash.finalize()))
}

fn read_back(
    target: &Path,
    total: u64,
    progress: &mut impl FnMut(Phase, u64, u64),
) -> Result<String, Error> {
    let mut file =
        File::open(target).map_err(|e| Error::io(format!("reopening {}", target.display()), e))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; BLOCK];
    let mut done = 0_u64;
    while done < total {
        let want = usize::try_from(total - done).map_or(BLOCK, |left| left.min(BLOCK));
        let padded = want.next_multiple_of(SECTOR);
        file.read_exact(&mut buffer[..padded])
            .map_err(|e| Error::io(format!("reading back {}", target.display()), e))?;
        hash.update(&buffer[..want]);
        done += want as u64;
        progress(Phase::Verifying, done, total);
    }
    Ok(hex(&hash.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
