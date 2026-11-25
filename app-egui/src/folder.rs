//! The Forge folder: where a profile that has no file yet is saved. It is the folder the Svelte
//! window uses (`org.contest-keeper.forge` in the user's application data), so profiles saved by
//! either window show up in both.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ck_admin_core::Profile;

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

/// Saves `profile` in a folder of its own under `data/profiles`, next to where its registry and
/// wallpaper go, and says where. A profile that has problems is not saved and leaves nothing behind.
pub fn save_in(data: &Path, profile: &Profile) -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let folder = data
        .join("profiles")
        .join(folder_name(&profile.name, nanos));
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    let path = folder.join("contest-profile.json");
    if let Err(error) = profile.save(&path) {
        // Only an empty folder is removed, so nothing that was there is touched.
        let _ = std::fs::remove_dir(&folder);
        return Err(error.to_string());
    }
    Ok(path)
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
        let data = std::env::temp_dir().join(format!("ck-egui-forge-{}", std::process::id()));
        let path = save_in(&data, &Profile::template()).expect("saved");
        assert!(path.starts_with(data.join("profiles")));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("contest-profile.json")
        );
        assert!(Profile::load(&path).is_ok());
        std::fs::remove_dir_all(&data).expect("cleanup");
    }

    #[test]
    fn a_profile_with_problems_leaves_no_folder() {
        let data = std::env::temp_dir().join(format!("ck-egui-forge-bad-{}", std::process::id()));
        let mut profile = Profile::template();
        profile
            .settings
            .branding
            .bookmarks
            .push(ck_protocol::Bookmark {
                title: String::new(),
                url: String::new(),
            });
        assert!(save_in(&data, &profile).is_err());
        let left = std::fs::read_dir(data.join("profiles")).map_or(0, Iterator::count);
        assert_eq!(left, 0);
        let _ = std::fs::remove_dir_all(&data);
    }
}
