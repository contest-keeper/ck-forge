use std::fs;
use std::path::Path;

use ck_protocol::Settings;

use crate::error::Error;

/// The agent ignores a bigger picture, so the admin tool refuses it before it is put on a stick.
pub const MAX_WALLPAPER_BYTES: u64 = 16 * 1024 * 1024;

/// The wallpaper picture a profile names, read from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wallpaper {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Wallpaper {
    /// Reads the picture the settings name from `folder`, which is where the profile file is.
    /// `None` when the settings name no wallpaper. A missing, oversized or non-file picture is an
    /// error.
    pub fn read(settings: &Settings, folder: &Path) -> Result<Option<Self>, Error> {
        let Some(name) = &settings.branding.wallpaper else {
            return Ok(None);
        };
        // The settings check makes `name` a plain file name, so it stays inside `folder`.
        let path = folder.join(name);
        let meta = fs::metadata(&path)
            .map_err(|e| Error::Wallpaper(format!("{}: {e}", path.display())))?;
        if !meta.is_file() {
            return Err(Error::Wallpaper(format!(
                "{} is not a file",
                path.display()
            )));
        }
        if meta.len() > MAX_WALLPAPER_BYTES {
            return Err(Error::Wallpaper(format!(
                "{} is larger than {} MB",
                path.display(),
                MAX_WALLPAPER_BYTES / (1024 * 1024)
            )));
        }
        let bytes =
            fs::read(&path).map_err(|e| Error::io(format!("reading {}", path.display()), e))?;
        Ok(Some(Self {
            name: name.clone(),
            bytes,
        }))
    }
}
