use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use base64::Engine;
use ck_protocol::Settings;

use crate::error::Error;
use crate::file::folder_of;

/// The agent ignores a bigger picture, so the admin tool refuses it before it is put on a stick.
pub const MAX_WALLPAPER_BYTES: u64 = 16 * 1024 * 1024;

/// The picture types a wallpaper may be, with the media type a page needs to show them.
const WALLPAPER_TYPES: [(&str, &str); 5] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
];

/// A bigger picture is still used as the wallpaper, but is not shown as a preview.
const PREVIEW_MAX_BYTES: u64 = 4 * 1024 * 1024;

fn wallpaper_type(path: &Path) -> Option<(&'static str, &'static str)> {
    let extension = path
        .extension()
        .and_then(OsStr::to_str)?
        .to_ascii_lowercase();
    WALLPAPER_TYPES
        .iter()
        .find(|(name, _)| *name == extension)
        .copied()
}

/// Copies the chosen picture into the profile's folder as `wallpaper.<type>` and returns that file
/// name, which is what the profile names. A picture of another type left from before is removed.
///
/// # Errors
/// [`Error::Wallpaper`] for a file that is not a PNG, JPG, WEBP or BMP picture, is too big, or
/// cannot be read or copied.
pub fn add_wallpaper(profile_path: &Path, source: &Path) -> Result<String, Error> {
    let (extension, _) = wallpaper_type(source)
        .ok_or_else(|| Error::Wallpaper("choose a PNG, JPG, WEBP or BMP picture".to_owned()))?;
    let meta = fs::metadata(source)
        .map_err(|e| Error::Wallpaper(format!("could not read {}: {e}", source.display())))?;
    if !meta.is_file() {
        return Err(Error::Wallpaper(format!(
            "{} is not a file",
            source.display()
        )));
    }
    if meta.len() > MAX_WALLPAPER_BYTES {
        return Err(Error::Wallpaper(format!(
            "the picture is larger than {} MB",
            MAX_WALLPAPER_BYTES / (1024 * 1024)
        )));
    }
    let folder = folder_of(profile_path);
    let name = format!("wallpaper.{extension}");
    let target = folder.join(&name);
    if source != target {
        fs::copy(source, &target).map_err(|e| {
            Error::Wallpaper(format!(
                "could not copy the picture to {}: {e}",
                target.display()
            ))
        })?;
    }
    for (other, _) in WALLPAPER_TYPES {
        if other != extension {
            let _ = fs::remove_file(folder.join(format!("wallpaper.{other}")));
        }
    }
    Ok(name)
}

/// The wallpaper as a `data:` URL a window can show, or nothing when it is missing, not a picture
/// or too big to preview.
#[must_use]
pub fn wallpaper_data_url(profile_path: &Path, name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return None;
    }
    let path = folder_of(profile_path).join(name);
    let (_, mime) = wallpaper_type(&path)?;
    let meta = fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > PREVIEW_MAX_BYTES {
        return None;
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(fs::read(&path).ok()?);
    Some(format!("data:{mime};base64,{encoded}"))
}

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
