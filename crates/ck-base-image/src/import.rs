//! Imports a downloaded Keeper OS archive without changing the original.

use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::Error;
use crate::download::hex;

// Keeper OS fits below this limit, including the older 30 GiB images.
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const MAX_ZSTD_WINDOW_LOG: u32 = 27;

/// Expands a Zstandard download to a raw disk image. The SHA-256 is the hash of the download as
/// published by the Hub. Images up to 32 GiB are supported. Progress counts compressed bytes
/// across the verification and decoding passes.
pub fn import_image(
    archive: &Path,
    out: &Path,
    sha256: &str,
    progress: impl FnMut(u64, u64),
) -> Result<(), Error> {
    let sha256 = sha256.trim();
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidChecksum);
    }
    if out.exists() {
        return Err(Error::Exists(out.display().to_string()));
    }

    let file = File::open(archive).map_err(|e| Error::io("opening the download", e))?;
    let total = file
        .metadata()
        .map_err(|e| Error::io("reading the download size", e))?
        .len();
    let mut archive = ArchiveReader {
        file,
        hash: Sha256::new(),
        done: 0,
        total: total.saturating_mul(2),
        progress,
    };
    // Reject a bad download before it can consume any space in the destination folder.
    io::copy(&mut archive, &mut io::sink()).map_err(|e| Error::io("reading the download", e))?;
    archive.verify_checksum(sha256)?;
    archive
        .file
        .rewind()
        .map_err(|e| Error::io("rewinding the download", e))?;
    let mut decoder =
        zstd::Decoder::new(archive).map_err(|e| Error::io("opening the compressed image", e))?;
    decoder
        .window_log_max(MAX_ZSTD_WINDOW_LOG)
        .map_err(|e| Error::io("setting the decoder window limit", e))?;
    let folder = out
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary =
        NamedTempFile::new_in(folder).map_err(|e| Error::io("creating the temporary image", e))?;
    expand_image(&mut decoder, &mut temporary, MAX_IMAGE_BYTES)?;

    // The file could have changed after the first check. Verify the exact bytes decoded.
    decoder.finish().into_inner().verify_checksum(sha256)?;
    // Without this a power cut after "done" can leave a short file under the final name.
    temporary
        .as_file()
        .sync_all()
        .map_err(|e| Error::io("flushing the verified image", e))?;
    temporary
        .persist_noclobber(out)
        .map_err(|err| match err.error.kind() {
            io::ErrorKind::AlreadyExists => Error::Exists(out.display().to_string()),
            _ => Error::io("saving the verified image", err.error),
        })?;
    // The new name is only durable once the folder is flushed (not possible on Windows this way).
    #[cfg(unix)]
    File::open(folder)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| Error::io("flushing the image folder", e))?;
    Ok(())
}

fn expand_image(reader: &mut impl Read, out: &mut impl Write, limit: u64) -> Result<(), Error> {
    let size = io::copy(&mut reader.by_ref().take(limit), out)
        .map_err(|e| Error::io("expanding the compressed image", e))?;
    if size == 0 {
        return Err(Error::EmptyArchive);
    }
    if size == limit {
        let extra = reader
            .read(&mut [0_u8])
            .map_err(|e| Error::io("checking the expanded image size", e))?;
        if extra != 0 {
            return Err(Error::ImageTooLarge);
        }
    }
    Ok(())
}

struct ArchiveReader<F> {
    file: File,
    hash: Sha256,
    done: u64,
    total: u64,
    progress: F,
}

impl<F> ArchiveReader<F> {
    fn verify_checksum(&mut self, expected: &str) -> Result<(), Error> {
        let actual = hex(&std::mem::take(&mut self.hash).finalize());
        if !expected.eq_ignore_ascii_case(&actual) {
            return Err(Error::Checksum {
                expected: expected.to_ascii_lowercase(),
                actual,
            });
        }
        Ok(())
    }
}

impl<F: FnMut(u64, u64)> Read for ArchiveReader<F> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.file.read(buffer)?;
        if count > 0 {
            self.hash.update(&buffer[..count]);
            self.done += count as u64;
            (self.progress)(self.done, self.total);
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_stops_writing_at_the_limit() {
        for (raw, too_large) in [(&b"abcd"[..], false), (&b"abcde"[..], true)] {
            let bytes = zstd::encode_all(raw, 1).expect("compress image");
            let mut decoder = zstd::Decoder::new(bytes.as_slice()).expect("open archive");
            let mut out = Vec::new();

            let result = expand_image(&mut decoder, &mut out, 4);

            assert_eq!(out, b"abcd");
            if too_large {
                assert!(matches!(result, Err(Error::ImageTooLarge)));
            } else {
                assert!(result.is_ok());
            }
        }
    }
}
