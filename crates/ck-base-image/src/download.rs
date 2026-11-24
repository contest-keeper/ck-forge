//! Fetches the official image and checks it against the catalog.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{Entry, Error};

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

pub fn sha256_file(path: &Path) -> Result<String, Error> {
    let mut file =
        File::open(path).map_err(|e| Error::io(format!("opening {}", path.display()), e))?;
    sha256_reader(&mut file).map_err(|e| Error::io(format!("reading {}", path.display()), e))
}

pub(crate) fn sha256_reader(file: &mut impl Read) -> std::io::Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            return Ok(hex(&hash.finalize()));
        }
        hash.update(&buffer[..count]);
    }
}

/// Returns the downloaded image in `cache`, fetching it when it is not there yet. A cached file
/// is checked again, so a damaged cache is replaced. `progress` gets the bytes done and the total.
pub fn fetch(
    entry: &Entry,
    cache: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, Error> {
    fs::create_dir_all(cache).map_err(|e| Error::io("creating the cache folder", e))?;
    let target = cache.join(format!("{}-{}.qcow2", entry.id, &entry.sha256[..12]));
    if target.exists() && sha256_file(&target)? == entry.sha256 {
        return Ok(target);
    }

    let download_error = |detail: String| Error::Download {
        url: entry.url.clone(),
        detail,
    };
    let response = ureq::get(&entry.url)
        .call()
        .map_err(|e| download_error(e.to_string()))?;
    let total = response.body().content_length().unwrap_or(0);
    let mut reader = response.into_body().into_reader();

    let part = target.with_extension("part");
    let mut file = File::create(&part).map_err(|e| Error::io("creating the download file", e))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|e| download_error(e.to_string()))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        file.write_all(&buffer[..count])
            .map_err(|e| Error::io("writing the download", e))?;
        done += count as u64;
        progress(done, total);
    }
    drop(file);

    let actual = hex(&hash.finalize());
    if actual != entry.sha256 {
        let _ = fs::remove_file(&part);
        return Err(Error::Checksum {
            expected: entry.sha256.clone(),
            actual,
        });
    }
    fs::rename(&part, &target).map_err(|e| Error::io("finishing the download", e))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
}
