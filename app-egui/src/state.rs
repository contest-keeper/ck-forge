//! What the pages share: the profile being edited, the base image and the choices of the window.
//! The same state, with the same rules, as `state.svelte.ts` of the earlier web window.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use ck_admin_core::{Error, Profile};
use serde::{Deserialize, Serialize};

use crate::theme::{Look, Mode, Theme};

/// What is remembered between runs, in `window.json` in the Forge folder.
#[derive(Default, Serialize, Deserialize)]
struct Remembered {
    base_path: Option<PathBuf>,
    theme: Option<Theme>,
    mode: Option<Mode>,
    /// Set by hand in `window.json` on a development machine: shows virtual disks and the write
    /// into a file. The window never offers it, and keeps it when it rewrites the file.
    #[serde(default)]
    dev_tools: bool,
}

const REMEMBERED: &str = "window.json";

// Independent flags of the window, not states of one thing.
#[allow(clippy::struct_excessive_bools)]
pub struct AppState {
    /// The profile being edited. Replace it with `use_profile`, never directly.
    pub profile: Option<Profile>,
    /// Where the profile is saved. Building needs it, because the registry and the wallpaper sit
    /// next to it.
    pub profile_path: Option<PathBuf>,
    /// The profile as it was last saved or loaded, to tell whether it changed since.
    saved: Option<String>,
    /// Goes up when another profile is taken in, so editors read their typed text from it again.
    pub profile_revision: u64,
    /// Why the profile must not be saved now: typed text that is not in the profile yet, or a
    /// request that is still writing the file. The contest page sets it every frame.
    pub blocked: Option<String>,
    base_path: Option<PathBuf>,
    /// A stick was written and read back in this run.
    pub written: bool,
    /// The admin signed in to the Hub: sticks made now report to it.
    pub hub_email: Option<String>,
    /// Something long is running (an import, a download or a write), so other buttons wait.
    pub busy: bool,
    /// Goes up when a wallpaper file is replaced; replacing keeps the name, so previews watch this.
    pub wallpaper_revision: u64,
    look: Look,
    /// Light or dark was chosen; until then the window follows the system.
    mode_chosen: bool,
    /// The Forge folder, or none to remember and save nothing (tests and screenshots).
    data: Option<PathBuf>,
    dev_tools: bool,
}

/// Writes beside `path` and renames, so a window that stops halfway leaves the old file whole.
fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)
}

/// How often a stick tells the Hub it is alive. Not a setting: every profile gets this.
const HEARTBEAT_S: u64 = 20;

/// A profile as this window keeps it: the fixed heartbeat, and no limit on when the stick key works
/// (the window has no way to set one, so an older limit would be a hidden rule).
fn normalised(mut profile: Profile) -> Profile {
    profile.settings.api.heartbeat_s = HEARTBEAT_S;
    profile.stick_key_window = None;
    profile
}

/// What went wrong with a profile, in the words of the earlier window.
pub fn message(error: &Error) -> String {
    match error {
        Error::InvalidProfile(problems) => {
            let lines: Vec<String> = problems.iter().map(|p| format!("- {p}")).collect();
            format!("the profile has problems:\n{}", lines.join("\n"))
        }
        other => other.to_string(),
    }
}

fn text_of(profile: &Profile) -> Option<String> {
    serde_json::to_string(profile).ok()
}

impl AppState {
    /// Starts with a new profile to fill in, and what the last run remembered in `data`.
    pub fn load(data: Option<PathBuf>) -> Self {
        let remembered: Remembered = data
            .as_ref()
            .and_then(|folder| std::fs::read(folder.join(REMEMBERED)).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            profile: Some(normalised(Profile::template())),
            profile_path: None,
            saved: None,
            profile_revision: 0,
            blocked: None,
            base_path: remembered.base_path,
            written: false,
            hub_email: None,
            busy: false,
            wallpaper_revision: 0,
            look: Look {
                theme: remembered.theme.unwrap_or_default(),
                mode: remembered.mode.unwrap_or_default(),
            },
            mode_chosen: remembered.mode.is_some(),
            data,
            dev_tools: remembered.dev_tools,
        }
    }

    /// The Forge folder, when this window may use it.
    pub fn data(&self) -> Option<&Path> {
        self.data.as_deref()
    }

    pub fn dev_tools(&self) -> bool {
        self.dev_tools
    }

    pub fn look(&self) -> Look {
        self.look
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.look.theme = theme;
        self.remember();
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.look.mode = mode;
        self.mode_chosen = true;
        self.remember();
    }

    pub fn follow_system(&mut self, dark: bool) {
        if !self.mode_chosen {
            self.look.mode = if dark { Mode::Dark } else { Mode::Light };
        }
    }

    pub fn base_path(&self) -> Option<&Path> {
        self.base_path.as_deref()
    }

    pub fn set_base_path(&mut self, path: Option<PathBuf>) {
        if self.base_path != path {
            self.base_path = path;
            self.remember();
        }
    }

    fn remember(&self) {
        let Some(folder) = &self.data else {
            return;
        };
        let remembered = Remembered {
            base_path: self.base_path.clone(),
            theme: Some(self.look.theme),
            mode: self.mode_chosen.then_some(self.look.mode),
            dev_tools: self.dev_tools,
        };
        // A choice that cannot be written lasts until the window closes.
        if let Ok(bytes) = serde_json::to_vec_pretty(&remembered) {
            let _ = std::fs::create_dir_all(folder);
            let _ = replace(&folder.join(REMEMBERED), &bytes);
        }
    }

    /// Takes a profile in as the one being edited. `path` is where it came from, none for a new one.
    pub fn use_profile(&mut self, profile: Profile, path: Option<PathBuf>) {
        let profile = normalised(profile);
        self.saved = path.as_ref().and_then(|_| text_of(&profile));
        self.profile = Some(profile);
        self.profile_path = path;
        self.profile_revision += 1;
        self.blocked = None;
    }

    /// Whether the profile has edits that are not saved.
    pub fn is_unsaved(&self) -> bool {
        self.profile
            .as_ref()
            .is_some_and(|profile| text_of(profile) != self.saved)
    }

    /// Saves the profile being edited and returns where it is. A profile that was saved or opened
    /// is saved to the same file. A new one goes into the Forge folder, in a folder of its own,
    /// and keeps that place from then on. The error says why a profile was not saved.
    pub fn save_profile(&mut self) -> Result<PathBuf, String> {
        if let Some(path) = self.profile_path.clone() {
            self.save_profile_to(path.clone())?;
            return Ok(path);
        }
        let profile = self.saveable()?;
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| "Could not find the Forge folder".to_owned())?;
        let (path, saved) = crate::folder::save_in(data, profile, None)?;
        self.saved = text_of(&saved);
        self.profile = Some(saved);
        self.profile_path = Some(path.clone());
        Ok(path)
    }

    /// Saves the profile to `path` and edits that file from then on.
    pub fn save_profile_to(&mut self, path: PathBuf) -> Result<(), String> {
        let profile = self.saveable()?;
        profile.save(&path).map_err(|e| message(&e))?;
        self.saved = text_of(profile);
        self.profile_path = Some(path);
        Ok(())
    }

    /// The profile, when nothing stops it from being saved.
    fn saveable(&self) -> Result<&Profile, String> {
        // Otherwise text that was typed would be left out and an older value saved.
        if let Some(why) = &self.blocked {
            return Err(why.clone());
        }
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| "There is no profile to save.".to_owned())?;
        let problems = profile.problems();
        if problems.is_empty() {
            Ok(profile)
        } else {
            Err(message(&Error::InvalidProfile(problems)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_profile_is_unsaved_until_it_is_saved_in_the_forge_folder() {
        let data = tempfile::tempdir().expect("folder");
        let mut state = AppState::load(Some(data.path().to_owned()));
        assert!(state.is_unsaved());
        let path = state.save_profile().expect("saved");
        assert!(path.starts_with(data.path()));
        assert!(!state.is_unsaved());
        state.profile.as_mut().expect("profile").name = "Other".to_owned();
        assert!(state.is_unsaved());
        assert_eq!(state.save_profile().expect("same file"), path);
        assert!(!state.is_unsaved());
    }

    #[test]
    fn a_profile_taken_in_has_the_fixed_heartbeat_and_no_key_limit() {
        let mut state = AppState::load(None);
        let mut profile = Profile::template();
        profile.settings.api.heartbeat_s = 3;
        profile.stick_key_window = Some(ck_admin_core::StickKeyWindow {
            valid_from: String::new(),
            valid_until: String::new(),
        });
        state.use_profile(profile, None);
        let profile = state.profile.as_ref().expect("profile");
        assert_eq!(profile.settings.api.heartbeat_s, 20);
        assert!(profile.stick_key_window.is_none());
    }

    #[test]
    fn a_blocked_or_invalid_profile_is_not_saved_and_leaves_nothing() {
        let data = tempfile::tempdir().expect("folder");
        let mut state = AppState::load(Some(data.path().to_owned()));
        state.blocked = Some("fix the contest end".to_owned());
        assert_eq!(state.save_profile(), Err("fix the contest end".to_owned()));
        state.blocked = None;
        state
            .profile
            .as_mut()
            .expect("profile")
            .settings
            .branding
            .bookmarks
            .push(ck_protocol::Bookmark {
                title: String::new(),
                url: String::new(),
            });
        let why = state.save_profile().expect_err("invalid");
        assert!(why.starts_with("the profile has problems:\n- "), "{why}");
        assert!(state.profile_path.is_none());
        assert!(!data.path().join("profiles").exists());
    }

    #[test]
    fn a_choice_that_cannot_be_written_leaves_the_remembered_file_whole() {
        let data = tempfile::tempdir().expect("folder");
        let mut state = AppState::load(Some(data.path().to_owned()));
        state.set_base_path(Some(PathBuf::from("base.img")));
        // A folder where the new file would be written makes the writing fail.
        std::fs::create_dir(data.path().join("window.tmp")).expect("in the way");
        state.set_theme(Theme::Classic);
        let again = AppState::load(Some(data.path().to_owned()));
        assert_eq!(again.base_path(), Some(Path::new("base.img")));
        assert_eq!(again.look().theme, Theme::Modern);
    }

    #[test]
    fn the_development_switch_is_only_set_by_hand_and_survives_a_rewrite() {
        let data = tempfile::tempdir().expect("folder");
        assert!(!AppState::load(Some(data.path().to_owned())).dev_tools());
        std::fs::write(data.path().join(REMEMBERED), r#"{"dev_tools":true}"#).expect("file");
        let mut state = AppState::load(Some(data.path().to_owned()));
        assert!(state.dev_tools());
        state.set_theme(Theme::Classic);
        assert!(AppState::load(Some(data.path().to_owned())).dev_tools());
    }

    #[test]
    fn the_look_and_the_base_image_are_remembered() {
        let data = tempfile::tempdir().expect("folder");
        let mut state = AppState::load(Some(data.path().to_owned()));
        state.follow_system(true);
        assert_eq!(state.look().mode, Mode::Dark);
        state.set_base_path(Some(PathBuf::from("base.img")));
        state.set_theme(Theme::Classic);
        state.set_mode(Mode::Light);
        let mut again = AppState::load(Some(data.path().to_owned()));
        again.follow_system(true);
        assert_eq!(again.look(), state.look());
        assert_eq!(again.base_path(), Some(Path::new("base.img")));
    }
}
