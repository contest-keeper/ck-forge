use std::path::Path;

use ck_protocol::Settings;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::file::{read_json, write_json_atomically};

/// The admin's settings for one contest, saved as a JSON file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub settings: Settings,
}

impl Profile {
    /// Reads a profile and checks its settings.
    ///
    /// # Errors
    /// Fails when the file cannot be read, is not a profile, or has problems.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let profile: Self = read_json(path)?.ok_or_else(|| {
            Error::io(
                format!("reading {}", path.display()),
                std::io::Error::new(std::io::ErrorKind::NotFound, "the file does not exist"),
            )
        })?;
        profile.check()?;
        Ok(profile)
    }

    /// Checks the settings and writes the profile. An invalid profile is never saved.
    ///
    /// # Errors
    /// Fails when the settings have problems or the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        self.check()?;
        write_json_atomically(path, self)
    }

    fn check(&self) -> Result<(), Error> {
        let problems = self.settings.validate();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(Error::InvalidProfile(problems))
        }
    }
}
