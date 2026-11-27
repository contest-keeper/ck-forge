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

/// A profile saved on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub path: PathBuf,
    /// The id the Hub knows it by, once it was saved there.
    pub hub_id: Option<String>,
}

/// The profiles in `data/profiles`, by name. A folder that holds no readable profile is left out.
pub fn list_local(data: &Path) -> Vec<Local> {
    let Ok(folders) = std::fs::read_dir(data.join("profiles")) else {
        return Vec::new();
    };
    let mut found: Vec<Local> = folders
        .flatten()
        .filter_map(|folder| {
            let path = folder.path().join("contest-profile.json");
            let profile = Profile::load(&path).ok()?;
            Some(Local {
                name: profile.name,
                path,
                hub_id: profile.hub_id,
            })
        })
        .collect();
    found.sort_by_key(|profile| profile.name.to_lowercase());
    found
}

/// Removes a locally saved profile file and its named wallpaper, preserving sibling files such
/// as `ck-registry.json`. If the profile lived in an otherwise empty `data/profiles/<folder>`,
/// that empty directory is removed too.
pub fn delete_local(data: Option<&Path>, path: &Path) -> Result<(), String> {
    let wallpaper = Profile::load(path)
        .ok()
        .and_then(|profile| profile.settings.branding.wallpaper)
        .filter(|name| Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name));
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("could not delete profile: {e}")),
    }
    if let (Some(folder), Some(name)) = (path.parent(), wallpaper) {
        let _ = std::fs::remove_file(folder.join(name));
    }
    if let Some(root) = data.map(|d| d.join("profiles"))
        && path.starts_with(&root)
        && let Some(folder) = path.parent()
        && folder != root
    {
        // Only removes the directory when it is empty; ignores DirectoryNotEmpty so a registry stays.
        let _ = std::fs::remove_dir(folder);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_profiles_are_listed_by_name_and_a_broken_folder_is_left_out() {
        let data = tempfile::tempdir().expect("folder");
        assert_eq!(list_local(data.path()), []);
        let mut second = Profile::template();
        "beta".clone_into(&mut second.name);
        let mut first = Profile::template();
        "Alpha".clone_into(&mut first.name);
        save_in(data.path(), &second, None).expect("saved");
        let (path, _) = save_in(data.path(), &first, None).expect("saved");
        let broken = data.path().join("profiles").join("broken");
        std::fs::create_dir_all(&broken).expect("folder");
        std::fs::write(broken.join("contest-profile.json"), "{").expect("file");
        let names: Vec<String> = list_local(data.path())
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["Alpha", "beta"]);
        assert_eq!(list_local(data.path())[0].path, path);
    }

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

    #[test]
    fn delete_local_removes_managed_profile_and_wallpaper_while_keeping_registry() {
        let data = tempfile::tempdir().expect("folder");
        let source = tempfile::tempdir().expect("pic");
        let pic = source.path().join("banner.png");
        std::fs::write(&pic, b"pic").expect("pic");
        let mut with_pic = Profile::template();
        with_pic.settings.branding.wallpaper = Some("banner.png".to_owned());
        let (managed_path, _) =
            save_in(data.path(), &with_pic, Some(&pic)).expect("saved with wallpaper");
        let managed_folder = managed_path.parent().expect("folder").to_path_buf();
        let (other_path, _) = save_in(data.path(), &Profile::template(), None).expect("saved");

        // An empty folder without a registry is removed cleanly alongside the profile & wallpaper.
        delete_local(Some(data.path()), &managed_path).expect("deleted managed");
        assert!(!managed_folder.exists());
        assert!(other_path.is_file());

        // When a stick registry exists alongside the profile, the registry is kept intact.
        let other_folder = other_path.parent().expect("other").to_path_buf();
        let registry = other_folder.join("ck-registry.json");
        std::fs::write(&registry, r#"{"sticks":[]}"#).expect("registry");
        delete_local(Some(data.path()), &other_path).expect("deleted profile with registry");
        assert!(!other_path.exists());
        assert!(registry.is_file());

        // A file directly under `profiles/` does not remove `profiles/`.
        let direct = data.path().join("profiles").join("direct.json");
        std::fs::write(&direct, "{}").expect("direct");
        delete_local(Some(data.path()), &direct).expect("deleted direct");
        assert!(!direct.exists());
        assert!(data.path().join("profiles").is_dir());

        // An external file leaves its directory and sibling files intact.
        let external_dir = tempfile::tempdir().expect("external");
        let external_file = external_dir.path().join("contest.json");
        let sibling = external_dir.path().join("keep.txt");
        std::fs::write(&external_file, "{}").expect("external");
        std::fs::write(&sibling, "keep").expect("sibling");
        delete_local(Some(data.path()), &external_file).expect("deleted external");
        assert!(!external_file.exists());
        assert!(sibling.is_file());

        // Deleting an already-missing path succeeds cleanly.
        delete_local(Some(data.path()), &managed_path).expect("missing managed");
        delete_local(None, &external_file).expect("missing external");
    }
}
