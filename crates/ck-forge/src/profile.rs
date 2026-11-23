//! `ck-forge profile`.

use std::io::{self};
use std::path::Path;

use crate::image::warn;
use ck_admin_core::{Error, Profile, Wallpaper, folder_of};

pub(crate) fn profile_new(file: &Path) -> Result<String, Error> {
    if file.exists() {
        return Err(Error::io(
            format!("{} already exists; choose another name", file.display()),
            io::Error::from(io::ErrorKind::AlreadyExists),
        ));
    }
    Profile::template().save(file)?;
    Ok(format!(
        "Wrote {0}\nEdit it, then run: ck-forge profile check {0}\n",
        file.display()
    ))
}

pub(crate) fn profile_check(file: &Path) -> Result<String, Error> {
    let profile = Profile::load(file)?;
    // The wallpaper is part of what the profile needs, so a missing picture is found here too.
    Wallpaper::read(&profile.stick_settings(), folder_of(file))?;
    warn(&profile);
    Ok(format!("{} is fine: {}\n", file.display(), profile.name))
}
