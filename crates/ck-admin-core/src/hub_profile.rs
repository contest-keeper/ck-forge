use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::file::folder_of;
use crate::hub::{
    HubProfile, HubSession, check_profile_id, fetch_profile, new_profile_id, upload_profile,
};
use crate::profile::{ContestBookmark, Profile};
use crate::wallpaper::Wallpaper;

/// Where sticks report and where admins sign in. Every build from develop, a pull request or a
/// local checkout uses the develop Hub; only the release workflow sets `CK_HUB_URL` to the
/// production Hub when it compiles.
pub const HUB_URL: &str = match option_env!("CK_HUB_URL") {
    Some(url) => url,
    None => "https://hub.dev.contestkeeper.com",
};

/// A profile saved from the Hub, and the file it was saved to.
#[derive(Debug)]
pub struct LoadedProfile {
    pub profile: Profile,
    pub path: PathBuf,
}

/// Downloads a profile of the organization into `<folder>/<id>/`, with its wallpaper beside it, so
/// it opens like any saved profile.
///
/// # Errors
/// [`Error::Hub`] for an id or a wallpaper name that cannot be a file name, or any error of the
/// Hub request or of writing the files.
pub fn load_hub_profile(
    hub: &str,
    session: &HubSession,
    id: &str,
    folder: &Path,
) -> Result<LoadedProfile, Error> {
    // The id becomes a folder name, so it must not be able to leave `folder`.
    check_profile_id(id)?;
    let remote = fetch_profile(hub, session, id)?;
    let folder = folder.join(id);
    fs::create_dir_all(&folder)
        .map_err(|e| Error::io(format!("could not make {}", folder.display()), e))?;
    let mut settings = remote.settings;
    if let Some(wallpaper) = &remote.wallpaper {
        // A name with a path in it would write outside the profile's folder.
        if wallpaper.name.is_empty()
            || wallpaper.name.contains(['/', '\\', ':'])
            || wallpaper.name == ".."
        {
            return Err(Error::Hub(
                "the Hub gave a wallpaper name that is not a file name".to_owned(),
            ));
        }
        settings.branding.wallpaper = Some(wallpaper.name.clone());
        let target = folder.join(&wallpaper.name);
        fs::write(&target, &wallpaper.bytes)
            .map_err(|e| Error::io(format!("could not save {}", target.display()), e))?;
    }
    let profile = Profile {
        name: remote.name,
        settings,
        hub_id: Some(id.to_owned()),
        stick_key_window: remote.stick_key_window,
        // The Hub keeps the settings, not what the form remembers about the bookmark.
        contest_bookmark: ContestBookmark::default(),
    };
    let path = folder.join("contest-profile.json");
    profile.save(&path)?;
    Ok(LoadedProfile { profile, path })
}

/// Saves the profile to the Hub, with its wallpaper, and remembers the Hub's id in the local file.
///
/// # Errors
/// Any error of the profile's checks, of reading its wallpaper, of the Hub request or of saving
/// the file.
pub fn save_hub_profile(
    hub: &str,
    session: &HubSession,
    path: &Path,
    mut profile: Profile,
) -> Result<Profile, Error> {
    profile.credential_scope()?;
    let id = profile.hub_id.clone().unwrap_or_else(new_profile_id);
    let wallpaper = Wallpaper::read(&profile.settings, folder_of(path))?;
    let upload = HubProfile {
        name: profile.name.clone(),
        settings: profile.settings.clone(),
        stick_key_window: profile.stick_key_window.clone(),
        wallpaper,
    };
    upload_profile(hub, session, &id, &upload)?;
    profile.hub_id = Some(id);
    profile.save(path)?;
    Ok(profile)
}
