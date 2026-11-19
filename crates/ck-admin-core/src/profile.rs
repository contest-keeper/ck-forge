use std::path::Path;

use ck_protocol::{Bookmark, Platform, Settings, UploadMode, WifiPassword};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::file::{read_json, write_json_atomically};

const DEFAULT_BOOKMARKS: [(&str, &str); 2] = [
    ("Codeforces", "https://codeforces.com"),
    ("C++ reference", "https://en.cppreference.com"),
];

/// The sites of the default bookmarks, which is all a new profile lets contestants reach.
const DEFAULT_ALLOWED_DOMAINS: [&str; 2] = [".codeforces.com", ".cppreference.com"];

fn is_codeforces_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())
}

/// The admin's settings for one contest, saved as a JSON file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub settings: Settings,
    /// The id of this profile on the Hub, once it was saved there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_id: Option<String>,
}

impl Profile {
    /// A profile with sensible defaults for the admin to edit.
    #[must_use]
    pub fn template() -> Self {
        let mut settings = Settings::default();
        settings.contest.platform = Platform::Codeforces;
        settings.branding.bookmarks = DEFAULT_BOOKMARKS
            .iter()
            .map(|(title, url)| Bookmark {
                title: (*title).to_owned(),
                url: (*url).to_owned(),
            })
            .collect();
        settings.network.allowed_domains = DEFAULT_ALLOWED_DOMAINS
            .iter()
            .map(|domain| (*domain).to_owned())
            .collect();
        Self {
            name: "My contest".to_owned(),
            settings,
            hub_id: None,
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
        let contest_id = settings.contest.contest_id.trim();
        if is_codeforces_id(contest_id) {
            // The contest itself comes first, so contestants find it before the reference sites.
            let url = format!("https://codeforces.com/contest/{contest_id}");
            settings.branding.bookmarks.retain(|b| b.url != url);
            settings.branding.bookmarks.insert(
                0,
                Bookmark {
                    title: "Contest".to_owned(),
                    url,
                },
            );
        }
        settings
    }

    /// The settings without the Wi-Fi password. This is what is saved in a profile file and sent
    /// to the Hub: the password belongs only on the stick, so it is typed again for each build.
    #[must_use]
    pub fn settings_without_secrets(&self) -> Settings {
        let mut settings = self.settings.clone();
        if let Some(wifi) = &mut settings.wifi {
            wifi.password = None;
        }
        settings
    }

    /// Sets the Wi-Fi password for this build. It does nothing when the profile has no Wi-Fi
    /// network, so a password is never added to settings that have no place for it.
    pub fn set_wifi_password(&mut self, password: Option<WifiPassword>) {
        if let Some(wifi) = &mut self.settings.wifi {
            wifi.password = password;
        }
    }

    /// Things that are allowed but probably not what the admin wants. They do not stop a build.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let contest_id = self.settings.contest.contest_id.trim();
        if !contest_id.is_empty() && !is_codeforces_id(contest_id) {
            warnings.push(
                "the contest ID is not a number, so contestants get no bookmark for the contest"
                    .to_owned(),
            );
        }
        if self.settings.api.url.is_empty() {
            warnings.push(
                "no server address (api.url) is set, so sticks made from this profile cannot report to a Hub"
                    .to_owned(),
            );
        }
        if self.settings.recording.upload == UploadMode::AfterContest
            && self.settings.contest.end.is_none()
        {
            warnings
                .push("set the contest end time to upload recordings after the contest".to_owned());
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
        let mut without_secrets = self.clone();
        without_secrets.settings = self.settings_without_secrets();
        write_json_atomically(path, &without_secrets)
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
