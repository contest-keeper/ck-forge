use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::Error;

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

/// Writes to a temporary file and renames it, so a crash never leaves a half-written file.
pub(crate) fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    let mut text = serde_json::to_string_pretty(value).map_err(|source| Error::Json {
        path: path.display().to_string(),
        source,
    })?;
    text.push('\n');

    let temporary = path.with_extension("tmp");
    fs::write(&temporary, text)
        .map_err(|e| Error::io(format!("writing {}", temporary.display()), e))?;
    fs::rename(&temporary, path).map_err(|e| Error::io(format!("replacing {}", path.display()), e))
}
