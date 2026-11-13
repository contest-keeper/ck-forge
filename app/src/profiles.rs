//! Commands for contest profiles: the settings page opens, checks and saves them.

// Tauri hands each command its arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::{Path, PathBuf};

use ck_admin_core::{Error, Profile, folder_of};
use serde::Serialize;

/// Plain words for the window: one line per problem with a profile, one line for anything else.
pub fn message(error: &Error) -> String {
    match error {
        Error::InvalidProfile(problems) => {
            let lines: Vec<String> = problems.iter().map(|p| format!("- {p}")).collect();
            format!("the profile has problems:\n{}", lines.join("\n"))
        }
        other => other.to_string(),
    }
}

/// The registry file that goes with a profile file.
pub fn registry_of(profile: &Path) -> PathBuf {
    folder_of(profile).join("ck-registry.json")
}

#[derive(Serialize)]
pub struct Checked {
    problems: Vec<String>,
    warnings: Vec<String>,
}

#[tauri::command]
pub fn new_profile() -> Profile {
    Profile::template()
}

#[tauri::command]
pub fn open_profile(path: PathBuf) -> Result<Profile, String> {
    Profile::load(&path).map_err(|e| message(&e))
}

#[tauri::command]
pub fn save_profile(path: PathBuf, profile: Profile) -> Result<(), String> {
    profile.save(&path).map_err(|e| message(&e))
}

/// What is wrong with a profile being edited, so the page can say so next to the field.
#[tauri::command]
pub fn check_profile(profile: Profile) -> Checked {
    Checked {
        problems: profile.problems().iter().map(ToString::to_string).collect(),
        warnings: profile.warnings(),
    }
}
