//! Thin stick images.
//!
//! The settings of a stick live in the EFI partition near the start of the image, so an image made
//! for one stick differs from the base image only in its first few hundred megabytes. A thin image
//! is that start (a file that is exactly as long as the part that can differ) and a manifest next
//! to it that names the base image for all the rest. Making one takes a second and 130 MB of disk
//! instead of a copy of 30 GB, and writing it reads the head from the thin image and the rest from
//! the base.
//!
//! The head file is a raw image start, so `image show` and `image configure` work on it. It is not
//! a bootable image by itself: [`open`] refuses it unless its manifest is there and the base is
//! still the one it was made from.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Error;

const CHUNK: usize = 4 * 1024 * 1024;
const SECTOR: u64 = 512;

/// What the manifest says about the base image the thin image was made from.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Manifest {
    /// Absolute path of the base image.
    base: PathBuf,
    base_len: u64,
    /// When the base was last changed, to catch a base that was replaced.
    base_modified_secs: u64,
    base_modified_nanos: u32,
    /// Length of the head file: the bytes that are taken from it and not from the base.
    head_len: u64,
    /// SHA-256 of the first `head_len` bytes of the base as they were when the thin image was made,
    /// which is cheap to read again and shows a different base at once.
    head_sha256: String,
}

/// Where the manifest of the thin image `image` is.
#[must_use]
pub fn manifest_path(image: &Path) -> PathBuf {
    let mut name = image.as_os_str().to_owned();
    name.push(".thin.json");
    PathBuf::from(name)
}

/// The size in bytes of the image `image` once written: the size of the base for a thin image,
/// the size of the file otherwise. Use this and not the size of the file. Refuses a file that is
/// the cut-off start of a disk without its manifest.
pub fn image_len(image: &Path) -> Result<u64, Error> {
    Ok(open(image)?.len())
}

/// Makes the thin image `out` from `base`: the first `head_len` bytes of `base`, and a manifest.
/// `out` and its manifest must not exist. `progress` gets the bytes copied and `head_len`. When
/// anything fails nothing is left behind.
///
/// `head_len` must be at least the end of the last part of the image that will be changed, and a
/// whole number of sectors.
pub fn create(
    base: &Path,
    out: &Path,
    head_len: u64,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), Error> {
    let io_error = |what: String| move |e| Error::io(what.clone(), e);
    let mut source = File::open(base).map_err(io_error(format!("opening {}", base.display())))?;
    let meta = source
        .metadata()
        .map_err(io_error(format!("reading the size of {}", base.display())))?;
    if head_len == 0 || !head_len.is_multiple_of(SECTOR) || head_len > meta.len() {
        return Err(Error::Thin(format!(
            "the start of {} cannot be {head_len} bytes long",
            base.display()
        )));
    }
    let modified = meta
        .modified()
        .and_then(|time| time.duration_since(UNIX_EPOCH).map_err(io::Error::other))
        .map_err(io_error(format!("reading the date of {}", base.display())))?;
    let base = fs::canonicalize(base).map_err(io_error(format!("finding {}", base.display())))?;

    let mut target = create_new(out)?;
    let manifest = manifest_path(out);
    let result =
        copy_head(&mut source, &mut target, head_len, &mut progress).and_then(|head_sha256| {
            let manifest_text = serde_json::to_vec_pretty(&Manifest {
                base: base.clone(),
                base_len: meta.len(),
                base_modified_secs: modified.as_secs(),
                base_modified_nanos: modified.subsec_nanos(),
                head_len,
                head_sha256,
            })
            .map_err(|e| Error::Thin(e.to_string()))?;
            target
                .sync_all()
                .map_err(io_error("flushing the thin image".to_owned()))?;
            let mut file = create_new(&manifest)?;
            io::Write::write_all(&mut file, &manifest_text)
                .map_err(io_error(format!("writing {}", manifest.display())))
        });
    if result.is_err() {
        drop(target);
        remove(out);
    }
    result
}

fn create_new(path: &Path) -> Result<File, Error> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            let what = if e.kind() == io::ErrorKind::AlreadyExists {
                format!("{} exists already", path.display())
            } else {
                format!("creating {}", path.display())
            };
            Error::io(what, e)
        })
}

/// Removes a thin image and its manifest, whatever of them exists.
pub fn remove(image: &Path) {
    let _ = fs::remove_file(image);
    let _ = fs::remove_file(manifest_path(image));
}

/// Copies `head_len` bytes and returns their SHA-256.
fn copy_head(
    source: &mut File,
    target: &mut File,
    head_len: u64,
    progress: &mut impl FnMut(u64, u64),
) -> Result<String, Error> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut done = 0_u64;
    while done < head_len {
        let want = usize::try_from(head_len - done).map_or(CHUNK, |left| left.min(CHUNK));
        source
            .read_exact(&mut buffer[..want])
            .map_err(|e| Error::io("reading the base image", e))?;
        hash.update(&buffer[..want]);
        io::Write::write_all(target, &buffer[..want])
            .map_err(|e| Error::io("writing the thin image", e))?;
        done += want as u64;
        progress(done, head_len);
    }
    Ok(crate::write::hex(&hash.finalize()))
}

/// An image split for writing: its own start, and the rest, which images of one base share.
pub(crate) struct Parts {
    /// The start that belongs to this image alone, from its first byte; none for a plain image.
    pub(crate) head: Option<File>,
    pub(crate) head_len: u64,
    /// The file the rest of the image is read from, and its length: the base of a thin image, or the
    /// whole of a plain image, which has no head of its own.
    pub(crate) tail: PathBuf,
    pub(crate) tail_total: u64,
}

impl Parts {
    /// How many bytes the image makes.
    pub(crate) fn len(&self) -> u64 {
        self.tail_total
    }

    /// Whether `other` is made of the same shared rest: the same file, with the same length and
    /// where the shared part starts. Only then can the two be written from one reading of it.
    pub(crate) fn shares_tail_with(&self, other: &Self) -> bool {
        self.tail == other.tail
            && self.tail_total == other.tail_total
            && self.head_len == other.head_len
    }
}

/// Opens `image` to be written. A thin image is checked against its base first.
pub(crate) fn open(image: &Path) -> Result<Parts, Error> {
    let manifest = manifest_path(image);
    match fs::read(&manifest) {
        Ok(text) => open_thin(image, &manifest, &text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => open_plain(image),
        Err(e) => Err(Error::io(format!("reading {}", manifest.display()), e)),
    }
}

fn open_plain(image: &Path) -> Result<Parts, Error> {
    let mut file =
        File::open(image).map_err(|e| Error::io(format!("opening {}", image.display()), e))?;
    let len = file
        .metadata()
        .map_err(|e| Error::io(format!("reading the size of {}", image.display()), e))?
        .len();
    if is_cut_off(&mut file, len)
        .map_err(|e| Error::io(format!("reading {}", image.display()), e))?
    {
        return Err(Error::Thin(format!(
            "{} is only the start of a disk image and its manifest {} is missing, so it must \
             not be written",
            image.display(),
            manifest_path(image).display()
        )));
    }
    Ok(Parts {
        head: None,
        head_len: 0,
        tail: fs::canonicalize(image)
            .map_err(|e| Error::io(format!("finding {}", image.display()), e))?,
        tail_total: len,
    })
}

/// Whether the file starts with a disk partition table that says the disk goes on past the end of
/// the file. A file with no partition table, like a test's, is not.
fn is_cut_off(file: &mut File, len: u64) -> io::Result<bool> {
    let mut header = [0_u8; 48];
    file.seek(SeekFrom::Start(SECTOR))?;
    if file.read_exact(&mut header).is_err() || &header[..8] != b"EFI PART" {
        return Ok(false);
    }
    let backup_lba = u64::from_le_bytes(header[32..40].try_into().expect("eight bytes"));
    Ok(backup_lba.saturating_add(1).saturating_mul(SECTOR) > len)
}

fn open_thin(image: &Path, manifest_file: &Path, text: &[u8]) -> Result<Parts, Error> {
    let bad = |why: String| Error::Thin(format!("{}: {why}", image.display()));
    let manifest: Manifest = serde_json::from_slice(text)
        .map_err(|e| bad(format!("{} is not readable ({e})", manifest_file.display())))?;

    let head =
        File::open(image).map_err(|e| Error::io(format!("opening {}", image.display()), e))?;
    let head_len = head
        .metadata()
        .map_err(|e| Error::io(format!("reading the size of {}", image.display()), e))?
        .len();
    if head_len != manifest.head_len {
        return Err(bad(format!(
            "is {head_len} bytes long but its manifest says {}",
            manifest.head_len
        )));
    }

    let mut base = File::open(&manifest.base).map_err(|e| {
        Error::io(
            format!(
                "opening the base image {} that {} was made from",
                manifest.base.display(),
                image.display()
            ),
            e,
        )
    })?;
    let meta = base.metadata().map_err(|e| {
        Error::io(
            format!("reading the size of {}", manifest.base.display()),
            e,
        )
    })?;
    let modified = meta
        .modified()
        .and_then(|time| time.duration_since(UNIX_EPOCH).map_err(io::Error::other))
        .map_err(|e| {
            Error::io(
                format!("reading the date of {}", manifest.base.display()),
                e,
            )
        })?;
    if meta.len() != manifest.base_len
        || modified.as_secs() != manifest.base_modified_secs
        || modified.subsec_nanos() != manifest.base_modified_nanos
    {
        return Err(bad(format!(
            "was made from {}, which has changed since; make the image again",
            manifest.base.display()
        )));
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut left = manifest.head_len;
    while left > 0 {
        let want = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
        base.read_exact(&mut buffer[..want])
            .map_err(|e| Error::io("reading the base image", e))?;
        hash.update(&buffer[..want]);
        left -= want as u64;
    }
    if crate::write::hex(&hash.finalize()) != manifest.head_sha256 {
        return Err(bad(format!(
            "was made from a different image than {} now is; make the image again",
            manifest.base.display()
        )));
    }
    Ok(Parts {
        head: Some(head),
        head_len: manifest.head_len,
        tail: manifest.base,
        tail_total: manifest.base_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_image(dir: &Path, len: usize) -> PathBuf {
        let path = dir.join("base.img");
        let bytes: Vec<u8> = (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect();
        fs::write(&path, bytes).unwrap();
        path
    }

    /// The bytes of the image as they are written: its head, then the rest of the tail.
    fn read_all(mut parts: Parts) -> Vec<u8> {
        let mut all = Vec::new();
        if let Some(head) = parts.head.as_mut() {
            head.take(parts.head_len).read_to_end(&mut all).unwrap();
        }
        let mut tail = File::open(&parts.tail).unwrap();
        tail.seek(SeekFrom::Start(parts.head_len)).unwrap();
        tail.read_to_end(&mut all).unwrap();
        all
    }

    #[test]
    fn a_thin_image_reads_as_its_head_followed_by_the_rest_of_the_base() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_image(dir.path(), 10_000);
        let out = dir.path().join("stick.img");
        create(&base, &out, 2048, |_, _| {}).unwrap();
        assert_eq!(fs::metadata(&out).unwrap().len(), 2048);

        // Change the head the way the settings of a stick would.
        let mut head = fs::read(&out).unwrap();
        head[100..110].fill(0xEE);
        fs::write(&out, &head).unwrap();

        let source = open(&out).unwrap();
        let mut expected = fs::read(&base).unwrap();
        expected[100..110].fill(0xEE);
        assert_eq!(source.len(), 10_000);
        assert_eq!(read_all(source), expected);
        assert_eq!(image_len(&out).unwrap(), 10_000);
    }

    #[test]
    fn the_head_may_end_at_the_last_sector_and_at_odd_sizes() {
        let dir = tempfile::tempdir().unwrap();
        for (len, head) in [(1024_usize, 1024_u64), (1000, 512), (4097, 4096)] {
            let base = base_image(dir.path(), len);
            let out = dir.path().join(format!("stick{len}.img"));
            create(&base, &out, head, |_, _| {}).unwrap();
            let source = open(&out).unwrap();
            assert_eq!(read_all(source), fs::read(&base).unwrap(), "{len} {head}");
        }
    }

    #[test]
    fn a_head_with_no_manifest_is_never_taken_for_a_whole_image() {
        let dir = tempfile::tempdir().unwrap();
        // The start of a disk whose partition table says it goes on for 100 sectors.
        let mut head = vec![0_u8; 2048];
        head[512..520].copy_from_slice(b"EFI PART");
        head[512 + 32..512 + 40].copy_from_slice(&99_u64.to_le_bytes());
        let path = dir.path().join("cut.img");
        fs::write(&path, &head).unwrap();
        assert!(matches!(open(&path), Err(Error::Thin(_))));
        assert!(matches!(image_len(&path), Err(Error::Thin(_))));

        // The same bytes with a backup table where it belongs are a whole image.
        let mut whole = head.clone();
        whole.resize(100 * 512, 0);
        fs::write(&path, &whole).unwrap();
        assert_eq!(image_len(&path).unwrap(), 100 * 512);
    }

    #[test]
    fn a_base_that_changed_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_image(dir.path(), 5000);
        let out = dir.path().join("stick.img");
        create(&base, &out, 1024, |_, _| {}).unwrap();

        // Another image in its place: same size, different start.
        let mut other = fs::read(&base).unwrap();
        other[10] ^= 0xFF;
        fs::write(&base, &other).unwrap();
        assert!(
            matches!(open(&out), Err(Error::Thin(why)) if why.contains("changed") || why.contains("different"))
        );

        // A shorter base.
        fs::write(&base, &other[..4096]).unwrap();
        assert!(matches!(open(&out), Err(Error::Thin(_))));

        // A missing base.
        fs::remove_file(&base).unwrap();
        assert!(matches!(open(&out), Err(Error::Io { .. })));
    }

    #[test]
    fn a_head_that_was_changed_in_length_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_image(dir.path(), 5000);
        let out = dir.path().join("stick.img");
        create(&base, &out, 1024, |_, _| {}).unwrap();
        fs::write(&out, vec![0_u8; 2048]).unwrap();
        assert!(matches!(open(&out), Err(Error::Thin(_))));
    }

    #[test]
    fn nothing_is_left_behind_when_making_one_fails() {
        let dir = tempfile::tempdir().unwrap();
        let base = base_image(dir.path(), 1000);
        let out = dir.path().join("stick.img");
        // A head longer than the base.
        assert!(create(&base, &out, 2048, |_, _| {}).is_err());
        assert!(!out.exists() && !manifest_path(&out).exists());
        // An existing image is never replaced.
        fs::write(&out, b"mine").unwrap();
        assert!(create(&base, &out, 512, |_, _| {}).is_err());
        assert_eq!(fs::read(&out).unwrap(), b"mine");
    }
}
