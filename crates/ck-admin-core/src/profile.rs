use std::path::Path;

use ck_protocol::{Bookmark, Platform, Settings, UploadMode, WifiPassword};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

use crate::error::Error;
use crate::file::{read_json, write_json_atomically};
use crate::registry::CredentialScope;

const DEFAULT_BOOKMARKS: [(&str, &str); 2] = [
    ("Codeforces", "https://codeforces.com"),
    ("C++ reference", "https://en.cppreference.com"),
];

/// The sites of the default bookmarks, which is all a new profile lets contestants reach.
const DEFAULT_ALLOWED_DOMAINS: [&str; 2] = [".codeforces.com", ".cppreference.com"];

/// Codeforces and `VJudge` number their contests with digits only.
fn is_contest_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())
}

/// Where a contest of a judge is opened, or `None` for a platform with no such page or an id that
/// is not a number.
#[must_use]
pub fn contest_url(platform: Platform, id: &str) -> Option<String> {
    let id = id.trim();
    if !is_contest_id(id) {
        return None;
    }
    match platform {
        Platform::Codeforces => Some(format!("https://codeforces.com/contest/{id}")),
        Platform::Vjudge => Some(format!("https://vjudge.net/contest/{id}")),
        Platform::Kilonova | Platform::Other => None,
    }
}

/// The site contestants must reach to open a contest of the platform.
fn platform_domain(platform: Platform) -> Option<&'static str> {
    match platform {
        Platform::Codeforces => Some(".codeforces.com"),
        Platform::Vjudge => Some(".vjudge.net"),
        Platform::Kilonova | Platform::Other => None,
    }
}

/// Keeps the contest's bookmark, the home page and the allowed site in step with the platform and
/// contest id the admin chose; `was_platform` and `was_id` are what they were before the change.
///
/// - A valid id adds a "Contest" bookmark first, and makes it the home page when there is none.
/// - A new id or platform moves that bookmark (and the home page, if it was the contest) with it,
///   and an id that is no longer valid takes it away.
/// - A bookmark the admin changed, or removed, is never touched or brought back, and a home page
///   the admin chose stays.
/// - The platform's site is added to the allowed sites when the contest has one.
pub fn follow_contest(settings: &mut Settings, was_platform: Platform, was_id: &str) {
    let before = contest_url(was_platform, was_id);
    let now = contest_url(settings.contest.platform, &settings.contest.contest_id);
    if before == now {
        return;
    }
    let branding = &mut settings.branding;
    if let Some(old) = &before {
        // The bookmark still is the one this added, and it follows; otherwise it is the admin's.
        if let Some(at) = branding.bookmarks.iter().position(|b| &b.url == old) {
            match &now {
                Some(url) => branding.bookmarks[at].url.clone_from(url),
                None => {
                    branding.bookmarks.remove(at);
                }
            }
        }
        if branding.home_page.as_ref() == Some(old) {
            branding.home_page.clone_from(&now);
        }
    } else if let Some(url) = &now {
        if !branding.bookmarks.iter().any(|b| &b.url == url) {
            branding.bookmarks.insert(
                0,
                Bookmark {
                    title: "Contest".to_owned(),
                    url: url.clone(),
                },
            );
        }
        if branding.home_page.is_none() {
            branding.home_page = Some(url.clone());
        }
    }
    if now.is_some() {
        allow_platform_site(settings);
    }
}

/// Lets contestants reach the site of the platform the contest is on, when it is not allowed yet.
fn allow_platform_site(settings: &mut Settings) {
    if let Some(domain) = platform_domain(settings.contest.platform)
        && !settings.network.allowed_domains.iter().any(|d| d == domain)
    {
        settings.network.allowed_domains.push(domain.to_owned());
    }
}

/// Explicit UTC dates keep a saved contest independent of the organizer's time zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StickKeyWindow {
    pub valid_from: String,
    pub valid_until: String,
}

/// The admin's settings for one contest, saved as a JSON file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub settings: Settings,
    /// The id of this profile on the Hub, once it was saved there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stick_key_window: Option<StickKeyWindow>,
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
            stick_key_window: None,
        }
    }

    pub fn credential_scope(&self) -> Result<Option<CredentialScope>, Error> {
        let Some(window) = &self.stick_key_window else {
            return Ok(None);
        };
        let contest_id = self.settings.contest.contest_id.trim();
        if contest_id.is_empty()
            || contest_id.len() > 200
            || contest_id.chars().any(char::is_control)
        {
            return Err(Error::InvalidStickKeyWindow("enter a contest ID"));
        }
        let valid_from = parse_utc(&window.valid_from)?;
        let valid_until = parse_utc(&window.valid_until)?;
        if valid_from >= valid_until {
            return Err(Error::InvalidStickKeyWindow(
                "the end must be after the start",
            ));
        }
        Ok(Some(CredentialScope {
            contest_id: contest_id.to_owned(),
            valid_from,
            valid_until,
        }))
    }

    /// The settings to put on a stick. The profile's name becomes the contest name unless the
    /// settings already give one, so a stick always shows which contest it is for.
    #[must_use]
    pub fn stick_settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        if settings.contest.name.is_empty() {
            settings.contest.name.clone_from(&self.name);
        }
        if let Some(url) = contest_url(settings.contest.platform, &settings.contest.contest_id) {
            // The contest itself comes first, so contestants find it before the reference sites.
            // One the admin already has, under any title, is left as it is.
            if !settings.branding.bookmarks.iter().any(|b| b.url == url) {
                settings.branding.bookmarks.insert(
                    0,
                    Bookmark {
                        title: "Contest".to_owned(),
                        url,
                    },
                );
            }
            // A bookmark to a site the stick does not allow would open nothing.
            allow_platform_site(&mut settings);
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
        let numbered = matches!(
            self.settings.contest.platform,
            Platform::Codeforces | Platform::Vjudge
        );
        if numbered && !contest_id.is_empty() && !is_contest_id(contest_id) {
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
            self.credential_scope()?;
            Ok(())
        } else {
            Err(Error::InvalidProfile(problems))
        }
    }
}

fn parse_utc(value: &str) -> Result<OffsetDateTime, Error> {
    OffsetDateTime::parse(value.trim(), &Rfc3339)
        .ok()
        .filter(|timestamp| timestamp.offset() == UtcOffset::UTC)
        .ok_or(Error::InvalidStickKeyWindow(
            "enter dates in UTC, for example 2026-11-19T08:00:00Z",
        ))
}
