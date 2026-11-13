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

    /// The settings to put on a stick. The profile's name becomes the contest name unless the
    /// settings already give one, so a stick always shows which contest it is for.
    #[must_use]
    pub fn stick_settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        if settings.contest.name.is_empty() {
            settings.contest.name.clone_from(&self.name);
        }
        settings
    }

    /// Things that are allowed but probably not what the admin wants. They do not stop a build.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.settings.api.url.is_empty() {
            warnings.push(
                "no server address (api.url) is set, so sticks made from this profile cannot report to a Hub"
                    .to_owned(),
            );
        }
        warnings
    }

    /// Empty when the profile is fine.
    #[must_use]
    pub fn problems(&self) -> Vec<ck_protocol::Problem> {
        self.settings.validate()
    }

    /// Reads a profile and checks its settings: a profile with problems is an error.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let profile: Self =
            read_json(path)?.ok_or_else(|| Error::ProfileMissing(path.display().to_string()))?;
        profile.check()?;
        Ok(profile)
    }

    /// Checks the settings and writes the profile. An invalid profile is never saved.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        self.check()?;
        write_json_atomically(path, self)
    }

    fn check(&self) -> Result<(), Error> {
        let problems = self.problems();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(Error::InvalidProfile(problems))
        }
    }
}
