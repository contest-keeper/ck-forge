//! Commands for contest profiles: the settings page opens, checks and saves them.

// Tauri hands each command its arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use ck_admin_core::{Error, MAX_WALLPAPER_BYTES, Profile, folder_of};
use serde::Serialize;
use tauri::{AppHandle, Manager};

/// The picture types a wallpaper may be, with the type a page needs to show them.
const WALLPAPER_TYPES: [(&str, &str); 5] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
];

/// A bigger picture is still used as the wallpaper, but is not shown as a preview in the window.
const PREVIEW_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Plain words for the window: one line per problem with a profile, one line for anything else.
pub fn message(error: &Error) -> String {
    match error {
        Error::InvalidProfile(problems) => {
            let lines: Vec<String> = problems.iter().map(|p| format!("- {p}")).collect();
            format!("the profile has problems:\n{}", lines.join("\n"))
        }
        other => other.to_string(),
    }
}

/// The registry file that goes with a profile file.
pub fn registry_of(profile: &Path) -> PathBuf {
    folder_of(profile).join("ck-registry.json")
}

#[derive(Serialize)]
pub struct Checked {
    problems: Vec<String>,
    warnings: Vec<String>,
}

#[tauri::command]
pub fn new_profile() -> Profile {
    Profile::template()
}

#[tauri::command]
pub fn open_profile(path: PathBuf) -> Result<Profile, String> {
    Profile::load(&path).map_err(|e| message(&e))
}

#[tauri::command]
pub fn save_profile(path: PathBuf, profile: Profile) -> Result<(), String> {
    profile.save(&path).map_err(|e| message(&e))
}

/// What is wrong with a profile being edited, so the page can say so next to the field.
#[tauri::command]
pub fn check_profile(mut profile: Profile) -> Checked {
    // The server address is not set in the window: a stick gets it when the admin is signed in.
    crate::hub::HUB_URL.clone_into(&mut profile.settings.api.url);
    let mut problems: Vec<_> = profile.problems().iter().map(ToString::to_string).collect();
    if let Err(error) = profile.credential_scope() {
        problems.push(message(&error));
    }
    Checked {
        problems,
        warnings: profile.warnings(),
    }
}

/// The name of a profile's own folder: its name in plain letters, then a number so two profiles
/// with the same name never share a folder (and so a registry with machine numbers).
fn folder_name(name: &str, unique: u128) -> String {
    let plain: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let words: Vec<&str> = plain.split('-').filter(|word| !word.is_empty()).collect();
    let joined: String = words.join("-").chars().take(40).collect();
    let slug = joined.trim_end_matches('-');
    let slug = if slug.is_empty() { "profile" } else { slug };
    format!("{slug}-{:08x}", unique % 0xFFFF_FFFF)
}

/// Saves a profile that has no file yet in the program's own folder, in a folder of its own
/// (next to its registry and its wallpaper), and says where. A profile that has problems is not
/// saved and leaves nothing behind.
#[tauri::command]
pub fn save_profile_in_forge_folder(app: AppHandle, profile: Profile) -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let folder = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("profiles")
        .join(folder_name(&profile.name, nanos));
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    let path = folder.join("contest-profile.json");
    if let Err(error) = profile.save(&path) {
        // Only an empty folder is removed, so nothing that was there is touched.
        let _ = std::fs::remove_dir(&folder);
        return Err(message(&error));
    }
    Ok(path)
}

/// The type name of a wallpaper picture, or nothing when the file is not one of the types.
fn wallpaper_extension(path: &Path) -> Option<&'static str> {
    let extension = path.extension().and_then(OsStr::to_str)?.to_ascii_lowercase();
    WALLPAPER_TYPES
        .iter()
        .map(|(name, _)| *name)
        .find(|name| *name == extension)
}

/// Copies the chosen picture into the profile's folder as `wallpaper.<type>` and returns that file
/// name, which is what the profile names. A picture of another type left from before is removed.
#[tauri::command]
pub fn add_wallpaper(profile_path: PathBuf, source: PathBuf) -> Result<String, String> {
    let extension = wallpaper_extension(&source)
        .ok_or_else(|| "choose a PNG, JPG, WEBP or BMP picture".to_owned())?;
    let meta = std::fs::metadata(&source)
        .map_err(|e| format!("could not read {}: {e}", source.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", source.display()));
    }
    if meta.len() > MAX_WALLPAPER_BYTES {
        return Err(format!(
            "the picture is larger than {} MB",
            MAX_WALLPAPER_BYTES / (1024 * 1024)
        ));
    }
    let folder = folder_of(&profile_path);
    let name = format!("wallpaper.{extension}");
    let target = folder.join(&name);
    if source != target {
        std::fs::copy(&source, &target)
            .map_err(|e| format!("could not copy the picture to {}: {e}", target.display()))?;
    }
    for (other, _) in WALLPAPER_TYPES {
        if other != extension {
            let _ = std::fs::remove_file(folder.join(format!("wallpaper.{other}")));
        }
    }
    Ok(name)
}

/// The wallpaper as a small picture the window can show, or nothing when it is missing, not a
/// picture or too big to preview.
#[tauri::command]
pub fn wallpaper_preview(profile_path: PathBuf, name: String) -> Option<String> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return None;
    }
    let path = folder_of(&profile_path).join(&name);
    let extension = wallpaper_extension(&path)?;
    let mime = WALLPAPER_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, mime)| *mime)?;
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > PREVIEW_MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_folder_name_is_plain_and_ends_in_a_number() {
        assert_eq!(
            folder_name("Regional Finals 2026!", 0x1234_5678),
            "regional-finals-2026-12345678"
        );
        assert_eq!(folder_name("  ", 1), "profile-00000001");
        assert_eq!(folder_name("../../Windows", 2), "windows-00000002");
        assert_ne!(folder_name("Same", 1), folder_name("Same", 2));
        let long = folder_name(&"a".repeat(100), 3);
        assert!(long.len() <= 40 + 9, "{long}");
    }

    #[test]
    fn only_pictures_are_wallpapers() {
        for good in ["a.png", "a.PNG", "x/y.jpeg", "b.webp", "c.bmp", "d.jpg"] {
            assert!(wallpaper_extension(Path::new(good)).is_some(), "{good}");
        }
        for bad in ["a.exe", "a", "a.png.exe", "a.svg", ".png"] {
            assert!(wallpaper_extension(Path::new(bad)).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_new_profile_checks_clean_in_the_window() {
        let checked = check_profile(new_profile());
        assert!(checked.problems.is_empty(), "{:?}", checked.problems);
        assert!(checked.warnings.is_empty(), "{:?}", checked.warnings);
    }
}
