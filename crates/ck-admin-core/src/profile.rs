use std::path::Path;

use ck_protocol::{Bookmark, Platform, RecordingMode, Settings, UploadMode, WifiPassword};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

use crate::error::Error;
use crate::file::{read_json, write_json_atomically};
use crate::registry::CredentialScope;

const DEFAULT_BOOKMARKS: [(&str, &str); 1] = [("C++ reference", "https://en.cppreference.com")];

/// The sites of the default bookmarks. The judge is added when the admin picks the platform.
const DEFAULT_ALLOWED_DOMAINS: [&str; 1] = [".cppreference.com"];

/// Sites every stick reaches, because judges put their anti-bot check there and without it the
/// judge does not load. They only serve that check: no search, no code, no answers.
pub const ALWAYS_ALLOWED_DOMAINS: [&str; 2] = ["challenges.cloudflare.com", ".hcaptcha.com"];

/// What a blank team or site becomes on a stick, so the computer name is never empty.
const DEFAULT_TEAM: &str = "default team";
const DEFAULT_SITE: &str = "default site";

/// How the platform is called on a bookmark.
fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Codeforces => "Codeforces",
        Platform::Vjudge => "VJudge",
        Platform::Kilonova => "Kilonova",
        Platform::Other => "Contest",
    }
}

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

/// Whether the bookmark the form added has been removed or changed by the admin since.
fn taken_over(state: &ContestBookmark, settings: &Settings) -> bool {
    state.admin_owned
        || state
            .managed
            .as_ref()
            .is_some_and(|url| !settings.branding.bookmarks.iter().any(|b| &b.url == url))
}

/// Keeps the contest's bookmark, the home page and the allowed site in step with the platform and
/// contest id the admin chose; `was_platform` and `was_id` are what they were before the change.
///
/// - A valid id adds a "Contest" bookmark first, and makes it the home page when there is none.
/// - A new id or platform moves that bookmark (and the home page, if it was the contest) with it,
///   and an id that is no longer valid takes it away.
/// - Once the admin removes or changes that bookmark it is theirs: the form never adds, moves or
///   brings one back, whatever ids follow, and a home page the admin chose stays.
/// - The platform's site is added to the allowed sites when the contest has one.
pub fn follow_contest(profile: &mut Profile, was_platform: Platform, was_id: &str) {
    let settings = &mut profile.settings;
    let state = &mut profile.contest_bookmark;
    if taken_over(state, settings) {
        state.managed = None;
        state.admin_owned = true;
    }
    let before = contest_url(was_platform, was_id);
    let now = contest_url(settings.contest.platform, &settings.contest.contest_id);
    if before == now {
        return;
    }
    let branding = &mut settings.branding;
    if let Some(old) = &before {
        // The bookmark the form added follows; one the admin took over is left alone.
        if state.managed.as_ref() == Some(old)
            && let Some(at) = branding.bookmarks.iter().position(|b| &b.url == old)
        {
            if let Some(url) = &now {
                branding.bookmarks[at].url.clone_from(url);
                state.managed.clone_from(&now);
            } else {
                branding.bookmarks.remove(at);
                state.managed = None;
            }
        }
        if branding.home_page.as_ref() == Some(old) {
            branding.home_page.clone_from(&now);
        }
    } else if let Some(url) = &now {
        if !state.admin_owned && !branding.bookmarks.iter().any(|b| &b.url == url) {
            branding.bookmarks.insert(
                0,
                Bookmark {
                    title: platform_name(settings.contest.platform).to_owned(),
                    url: url.clone(),
                },
            );
            state.managed = Some(url.clone());
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

/// What the form remembers about the contest bookmark it keeps, so the admin's own choices last.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContestBookmark {
    /// The address of the bookmark the form added, while it is still there as it was added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed: Option<String>,
    /// The admin removed or changed that bookmark. The form then never adds or moves one again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub admin_owned: bool,
}

impl ContestBookmark {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
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
    #[serde(default, skip_serializing_if = "ContestBookmark::is_default")]
    pub contest_bookmark: ContestBookmark,
}

impl Profile {
    /// A profile with sensible defaults for the admin to edit.
    #[must_use]
    pub fn template() -> Self {
        let mut settings = Settings::default();
        // Nothing is prefilled that the admin has not chosen: no judge, no team, no recording.
        settings.team.clear();
        settings.site.clear();
        settings.recording.mode = RecordingMode::Off;
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
            contest_bookmark: ContestBookmark::default(),
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
        if settings.team.trim().is_empty() {
            DEFAULT_TEAM.clone_into(&mut settings.team);
        }
        if settings.site.trim().is_empty() {
            DEFAULT_SITE.clone_into(&mut settings.site);
        }
        for domain in ALWAYS_ALLOWED_DOMAINS {
            if !settings.network.allowed_domains.iter().any(|d| d == domain) {
                settings.network.allowed_domains.push(domain.to_owned());
            }
        }
        if let Some(url) = contest_url(settings.contest.platform, &settings.contest.contest_id) {
            // The contest itself comes first, so contestants find it before the reference sites.
            // One the admin already has, under any title, is left as it is, and one the admin
            // removed or changed is not put back.
            let theirs = taken_over(&self.contest_bookmark, &settings);
            if !theirs && !settings.branding.bookmarks.iter().any(|b| b.url == url) {
                settings.branding.bookmarks.insert(
                    0,
                    Bookmark {
                        title: platform_name(settings.contest.platform).to_owned(),
                        url,
                    },
                );
            }
            // A bookmark to a site the stick does not allow would open nothing.
            allow_platform_site(&mut settings);
        }
        settings
    }

    /// The settings without the Wi-Fi password. This is what is saved in a profile
    /// file: the password is kept encrypted in the Hub (see `sync_wifi_password`), never on disk.
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
        // As it is built: a blank team or site is not a problem, it gets a default name.
        self.stick_settings().validate()
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
