//! The Forge folder: where a profile that has no file yet is saved. It is the folder the Svelte
//! window uses (`org.contest-keeper.forge` in the user's application data), so profiles saved by
//! either window show up in both.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ck_admin_core::{Profile, add_wallpaper};

const IDENTIFIER: &str = "org.contest-keeper.forge";

/// The application data folder of Forge, which is where the Svelte window keeps its files too.
pub fn data_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support")
    } else if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(data)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".local/share")
    };
    Some(base.join(IDENTIFIER))
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

/// Makes a folder of its own for a profile called `name` under `data/profiles`. The folder is
/// created exclusively, so a name that happens to repeat never reuses (and overwrites) the folder
/// of another profile: the next number is tried instead.
fn new_folder(data: &Path, name: &str) -> Result<PathBuf, String> {
    let profiles = data.join("profiles");
    std::fs::create_dir_all(&profiles)
        .map_err(|e| format!("could not make {}: {e}", profiles.display()))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    for attempt in 0..100 {
        let folder = profiles.join(folder_name(name, nanos + attempt));
        match std::fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("could not make {}: {e}", folder.display())),
        }
    }
    Err(format!(
        "could not find a free folder in {}",
        profiles.display()
    ))
}

/// Saves `profile` in a folder of its own under `data/profiles`, next to where its registry and
/// wallpaper go, and says where. `wallpaper` is the picture file the profile names now, which is
/// copied along so the saved profile keeps its wallpaper. A profile that has problems is not
/// saved and leaves nothing behind. Also returns the profile as saved: its wallpaper has the name
/// of the copy, which may differ from the name it had.
pub fn save_in(
    data: &Path,
    profile: &Profile,
    wallpaper: Option<&Path>,
) -> Result<(PathBuf, Profile), String> {
    let folder = new_folder(data, &profile.name)?;
    let path = folder.join("contest-profile.json");
    match save_with_wallpaper(&path, profile, wallpaper) {
        Ok(saved) => Ok((path, saved)),
        Err(why) => {
            // The folder was made just now by this call, so nothing else is in it.
            let _ = std::fs::remove_dir_all(&folder);
            Err(why)
        }
    }
}

fn save_with_wallpaper(
    path: &Path,
    profile: &Profile,
    wallpaper: Option<&Path>,
) -> Result<Profile, String> {
    let mut profile = profile.clone();
    if let Some(source) = wallpaper {
        let name = add_wallpaper(path, source).map_err(|e| e.to_string())?;
        profile.settings.branding.wallpaper = Some(name);
    }
    profile.save(path).map_err(|e| e.to_string())?;
    Ok(profile)
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
    fn a_profile_is_saved_in_a_folder_of_its_own() {
        let data = tempfile::tempdir().expect("folder");
        let (path, _) = save_in(data.path(), &Profile::template(), None).expect("saved");
        assert!(path.starts_with(data.path().join("profiles")));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("contest-profile.json")
        );
        assert!(Profile::load(&path).is_ok());
    }

    #[test]
    fn two_profiles_with_one_name_never_share_a_folder() {
        let data = tempfile::tempdir().expect("folder");
        let first = new_folder(data.path(), "Same").expect("first");
        let second = new_folder(data.path(), "Same").expect("second");
        assert_ne!(first, second);
    }

    #[test]
    fn a_profile_with_problems_leaves_no_folder() {
        let data = tempfile::tempdir().expect("folder");
        let mut profile = Profile::template();
        profile
            .settings
            .branding
            .bookmarks
            .push(ck_protocol::Bookmark {
                title: String::new(),
                url: String::new(),
            });
        assert!(save_in(data.path(), &profile, None).is_err());
        let left = std::fs::read_dir(data.path().join("profiles")).map_or(0, Iterator::count);
        assert_eq!(left, 0);
    }

    #[test]
    fn the_wallpaper_goes_along_to_the_new_folder() {
        let data = tempfile::tempdir().expect("folder");
        let old = tempfile::tempdir().expect("old folder");
        let picture = old.path().join("wallpaper.png");
        std::fs::write(&picture, b"not really a picture").expect("picture");
        let mut profile = Profile::template();
        profile.settings.branding.wallpaper = Some("wallpaper.png".to_owned());
        // The picture is named "banner.png" beside the old profile; the copy is called wallpaper.png.
        profile.settings.branding.wallpaper = Some("banner.png".to_owned());
        let picture = old.path().join("banner.png");
        std::fs::write(&picture, b"not really a picture").expect("picture");
        let (path, kept) = save_in(data.path(), &profile, Some(&picture)).expect("saved");
        let saved = Profile::load(&path).expect("opens");
        let name = saved.settings.branding.wallpaper.clone().expect("named");
        assert!(path.with_file_name(&name).is_file());
        // What the editor keeps names the copy too, so a later save finds its picture.
        assert_eq!(kept.settings.branding.wallpaper, Some(name));
    }
}
