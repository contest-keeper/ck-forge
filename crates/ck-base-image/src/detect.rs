use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use ck_image::Guid;
use ext4_view::{Ext4, Ext4Error, Ext4Read};

use crate::Error;
use crate::download::sha256_reader;

// These exact raw fingerprints are recorded in ck-os docs/base-images.md. The provisioning
// catalog and the Hub's download manifest hash their input and compressed archives respectively.
const KNOWN_RAW_IMAGES: &[(&str, &str)] = &[
    (
        "34f4dbaa905b74c80212868053eed75128d30d93d18a2a64ff66c773b0ef8dee",
        "Keeper OS reference image (2026-11-12)",
    ),
    (
        "f924caf11ee693e64316b20c9ef7690d70396371985e677279445b41e451576b",
        "ICPC contest reference image (2026-11-12)",
    ),
];
const SUPERBLOCK_OFFSET: u64 = 1024;
const SUPERBLOCK_PREFIX: usize = 0x154;
const RELEASE_LIMIT: u64 = 64 * 1024;

#[derive(Debug)]
pub struct DetectedImage {
    sha256: String,
    identity: Identity,
}

#[derive(Debug)]
enum Identity {
    Known(&'static str),
    Linux {
        path: &'static str,
        release: OsRelease,
    },
}

#[derive(Debug, Default, PartialEq, Eq)]
struct OsRelease {
    id: Option<String>,
    version: Option<String>,
    name: Option<String>,
    pretty_name: Option<String>,
}

impl fmt::Display for DetectedImage {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "SHA-256: {}", self.sha256)?;
        match &self.identity {
            Identity::Known(name) => write!(out, "\nKnown raw image: {name}"),
            Identity::Linux { path, release } => {
                let name = release.pretty_name.as_deref().or(release.name.as_deref());
                let id = release.id.as_deref().unwrap_or("linux");
                write!(
                    out,
                    "\nLinux distribution: {}\nID: {id}",
                    name.unwrap_or(id)
                )?;
                if let Some(version) = &release.version {
                    write!(out, "\nVersion ID: {version}")?;
                }
                write!(out, "\nRead from: {path}")
            }
        }
    }
}

/// Identifies a raw image by its whole-file SHA-256, or reads os-release directly from a Linux
/// filesystem in its GPT. Opens the image read-only and does not mount it or use its filename.
pub fn detect_image(path: &Path) -> Result<DetectedImage, Error> {
    let mut disk = File::open(path).map_err(|e| Error::io("opening the image", e))?;
    let sha256 = sha256_reader(&mut disk).map_err(|e| Error::io("hashing the image", e))?;
    let identity = identify(known_raw_image(&sha256), &mut disk)?;
    Ok(DetectedImage { sha256, identity })
}

fn known_raw_image(sha256: &str) -> Option<&'static str> {
    KNOWN_RAW_IMAGES
        .iter()
        .find(|(digest, _)| *digest == sha256)
        .map(|(_, name)| *name)
}

fn identify(known: Option<&'static str>, disk: &mut File) -> Result<Identity, Error> {
    if let Some(name) = known {
        return Ok(Identity::Known(name));
    }

    let disk_size = disk
        .metadata()
        .map_err(|e| Error::io("reading image size", e))?
        .len();
    let partitions = ck_image::read_partitions(disk).map_err(Error::DetectionLayout)?;
    for partition in partitions
        .iter()
        .filter(|part| part.type_guid == Guid::LINUX_FILESYSTEM)
    {
        let start = partition.start_byte();
        let end = partition.end_byte();
        if end > disk_size {
            return Err(Error::io(
                "reading the Linux partition",
                io::ErrorKind::UnexpectedEof.into(),
            ));
        }
        let len = end - start;
        if len < SUPERBLOCK_OFFSET + SUPERBLOCK_PREFIX as u64 {
            continue;
        }
        let mut reader = PartitionReader {
            disk: disk
                .try_clone()
                .map_err(|e| Error::io("opening the Linux partition", e))?,
            start,
            len,
        };
        let mut header = [0; SUPERBLOCK_PREFIX];
        reader
            .read_at(SUPERBLOCK_OFFSET, &mut header)
            .map_err(|e| Error::io("identifying the Linux filesystem", e))?;
        if header[0x38..0x3a] != [0x53, 0xef] {
            continue;
        }
        check_geometry(&header, len)?;
        let fs = Ext4::load(Box::new(reader)).map_err(Error::DetectionFilesystem)?;
        if let Some(identity) = read_os_release(&fs)? {
            return Ok(identity);
        }
    }
    Err(Error::UnknownImage)
}

fn check_geometry(header: &[u8; SUPERBLOCK_PREFIX], len: u64) -> Result<(), Error> {
    let word =
        |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().expect("four bytes"));
    let log_block_size = word(0x18);
    let blocks = (u64::from(word(0x150)) << 32) | u64::from(word(0x04));
    let blocks_per_group = u64::from(word(0x20));
    // The reader allocates one entry per block group before reading their descriptors.
    if log_block_size > 6
        || blocks == 0
        || blocks > (len / (1024 << log_block_size))
        || blocks_per_group == 0
        || blocks.div_ceil(blocks_per_group) > 65_536
    {
        return Err(Error::DetectionGeometry);
    }
    Ok(())
}

fn read_os_release(fs: &Ext4) -> Result<Option<Identity>, Error> {
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        let file = match fs.open(path) {
            Ok(file) => file,
            Err(Ext4Error::NotFound) => continue,
            Err(error) => return Err(Error::DetectionFilesystem(error)),
        };
        let mut bytes = Vec::new();
        file.take(RELEASE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::io("reading os-release", e))?;
        if bytes.len() as u64 > RELEASE_LIMIT {
            return Err(Error::OsRelease("file exceeds 64 KiB"));
        }
        let text =
            std::str::from_utf8(&bytes).map_err(|_| Error::OsRelease("file is not UTF-8"))?;
        return Ok(Some(Identity::Linux {
            path,
            release: parse_os_release(text)?,
        }));
    }
    Ok(None)
}

fn parse_os_release(text: &str) -> Result<OsRelease, Error> {
    let mut os = OsRelease::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(Error::OsRelease("expected a key and value"))?;
        let field = match key {
            "ID" => &mut os.id,
            "VERSION_ID" => &mut os.version,
            "NAME" => &mut os.name,
            "PRETTY_NAME" => &mut os.pretty_name,
            _ => continue,
        };
        let value = release_value(value)?;
        *field = (!value.is_empty()).then_some(value);
    }
    if os.id.as_ref().is_some_and(|id| {
        !id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    }) {
        return Err(Error::OsRelease("invalid distribution ID"));
    }
    if os.id.is_none() && os.name.is_none() && os.pretty_name.is_none() {
        return Err(Error::OsRelease("missing distribution identity"));
    }
    Ok(os)
}

fn release_value(text: &str) -> Result<String, Error> {
    let invalid = || Error::OsRelease("malformed quoted or escaped value");
    let quote = text.chars().next().filter(|ch| matches!(ch, '\'' | '"'));
    let inner = if let Some(quote) = quote {
        text.strip_prefix(quote)
            .and_then(|value| value.strip_suffix(quote))
            .ok_or_else(invalid)?
    } else {
        text
    };
    let mut chars = inner.chars();
    let mut value = String::new();
    while let Some(ch) = chars.next() {
        if ch == '\\' && quote != Some('\'') {
            let escaped = chars.next().ok_or_else(invalid)?;
            if quote == Some('"') && !matches!(escaped, '"' | '\\' | '$' | '`') {
                value.push('\\');
            }
            value.push(escaped);
        } else if Some(ch) == quote
            || (quote.is_none() && (ch.is_whitespace() || matches!(ch, '\'' | '"')))
        {
            return Err(invalid());
        } else {
            value.push(ch);
        }
    }
    if value.chars().any(char::is_control) {
        return Err(Error::OsRelease("control characters in a value"));
    }
    Ok(value)
}

struct PartitionReader {
    disk: File,
    start: u64,
    len: u64,
}

impl PartitionReader {
    fn read_at(&mut self, offset: u64, dst: &mut [u8]) -> io::Result<()> {
        if offset > self.len || dst.len() as u64 > self.len - offset {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.disk.seek(SeekFrom::Start(self.start + offset))?;
        self.disk.read_exact(dst)
    }
}

impl Ext4Read for PartitionReader {
    fn read(
        &mut self,
        offset: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.read_at(offset, dst)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_fingerprint_takes_precedence_over_filesystem_reading() {
        let known = known_raw_image(KNOWN_RAW_IMAGES[0].0);
        let mut not_a_disk = tempfile::tempfile().expect("temp file");
        let identity = identify(known, &mut not_a_disk).expect("known raw fingerprint");
        assert!(matches!(identity, Identity::Known(name) if name.starts_with("Keeper OS")));
        assert!(known_raw_image(&"0".repeat(64)).is_none());
    }

    #[test]
    fn os_release_parses_shell_quoting_without_evaluating_it() {
        let os = parse_os_release("# example\nID=debian\nID='ubuntu'\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu \\\"Noble\\\" \\$HOME\"\n").expect("valid release");
        assert_eq!(os.id.as_deref(), Some("ubuntu"));
        assert_eq!(os.pretty_name.as_deref(), Some("Ubuntu \"Noble\" $HOME"));
        assert!(parse_os_release("ID='ubuntu'\"fedora\"").is_err());
        assert!(parse_os_release("ID=ubuntu\nPRETTY_NAME=bad\u{1b}").is_err());
    }

    #[test]
    fn oversized_ext_metadata_is_refused_before_allocating_descriptors() {
        let mut header = [0; SUPERBLOCK_PREFIX];
        header[4..8].copy_from_slice(&65_537_u32.to_le_bytes());
        header[0x20..0x24].copy_from_slice(&1_u32.to_le_bytes());
        assert!(matches!(
            check_geometry(&header, 100 * 1024 * 1024),
            Err(Error::DetectionGeometry)
        ));
        assert!(matches!(
            check_geometry(&header, 1024),
            Err(Error::DetectionGeometry)
        ));
    }
}
