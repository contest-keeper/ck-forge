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
    /// A profile with sensible defaults for the admin to edit.
    #[must_use]
    pub fn template() -> Self {
        Self {
            name: "My contest".to_owned(),
            settings: Settings::default(),
        }
    }

    /// The problems with this profile, in plain words. Empty when it is fine.
    #[must_use]
    pub fn problems(&self) -> Vec<ck_protocol::Problem> {
        self.settings.validate()
    }

    /// Reads a profile and checks its settings.
    ///
    /// # Errors
    /// Fails when the file cannot be read, is not a profile, or has problems.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let profile: Self =
            read_json(path)?.ok_or_else(|| Error::ProfileMissing(path.display().to_string()))?;
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
