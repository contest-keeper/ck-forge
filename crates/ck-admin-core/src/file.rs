use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tempfile::NamedTempFile;

use crate::error::Error;

/// The folder a file is in: where a profile's pictures and its registry are looked for.
#[must_use]
pub fn folder_of(file: &Path) -> &Path {
    match file.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    }
}

/// `None` when the file does not exist.
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(format!("reading {}", path.display()), e)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|source| Error::Json {
            path: path.display().to_string(),
            source,
        })
}

/// Writes to a uniquely named temporary file next to `path`, flushes it to disk and renames it, so
/// a crash or power loss never leaves a half-written file.
///
/// On Linux and macOS the file is readable by its owner only, because the registry holds the
/// sticks' secrets. On Windows it inherits the folder's permissions, so keep the registry in the
/// admin's own profile folder.
pub(crate) fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    let mut text = serde_json::to_string_pretty(value).map_err(|source| Error::Json {
        path: path.display().to_string(),
        source,
    })?;
    text.push('\n');

    let folder = folder_of(path);
    let mut temporary = NamedTempFile::new_in(folder).map_err(|e| {
        Error::io(
            format!("creating a temporary file in {}", folder.display()),
            e,
        )
    })?;
    temporary
        .write_all(text.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|e| Error::io(format!("writing {}", path.display()), e))?;
    temporary
        .persist(path)
        .map_err(|e| Error::io(format!("replacing {}", path.display()), e.error))?;
    Ok(())
}

/// Holds an exclusive lock on `<path>.lock` until dropped, so two programs (the command line and
/// the admin window) cannot change the same file at once and lose each other's changes.
pub(crate) struct Lock {
    _file: File,
}

impl Lock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, Error> {
        let mut name = path.as_os_str().to_owned();
        name.push(".lock");
        let lock_path = Path::new(&name);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)
            .map_err(|e| Error::io(format!("opening {}", lock_path.display()), e))?;
        file.lock()
            .map_err(|e| Error::io(format!("locking {}", lock_path.display()), e))?;
        Ok(Self { _file: file })
    }
}
