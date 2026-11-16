//! Signing in to the Hub through the browser, and sending it the registry once signed in.
//!
//! The session is kept in memory and in the Windows Credential Manager, so the admin is still
//! signed in after closing the program, until the Hub ends it. Nothing about it is written to a
//! file, printed or sent to the page except the admin's email.

// Tauri hands each command its arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ck_admin_core::{
    HubProfile, HubSession, Profile, ProfileSummary, Pushed, Wallpaper, folder_of,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::profiles::{message, registry_of};

/// Where sticks report and where admins sign in. Sticks only get this address when the admin who
/// made them was signed in. Every build from develop, a pull request or a local checkout uses the
/// develop Hub; only the release workflow sets `CK_HUB_URL` to the production Hub when it compiles.
pub const HUB_URL: &str = match option_env!("CK_HUB_URL") {
    Some(url) => url,
    None => "https://hub.dev.contestkeeper.com",
};

const SIGN_IN_WAIT: Duration = Duration::from_secs(180);

#[derive(Default)]
pub struct HubState {
    session: Mutex<Option<HubSession>>,
    /// Whether the Hub has said yes to the session saved by an earlier run.
    checked: AtomicBool,
}

impl HubState {
    /// Starts signed in when an earlier run saved a session.
    pub fn restored() -> Self {
        Self {
            session: Mutex::new(crate::vault::load()),
            checked: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<HubSession>> {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn signed_in(&self) -> bool {
        self.lock().is_some()
    }

    /// Sends the registry as the signed-in admin. `None` when nobody is signed in.
    pub fn push(&self, registry: &Path) -> Option<Result<String, String>> {
        let guard = self.lock();
        let session = guard.as_ref()?;
        Some(
            ck_admin_core::push_registry_signed_in(HUB_URL, session, registry)
                .map(|pushed| describe(&pushed))
                .map_err(|e| message(&e)),
        )
    }
}

fn describe(pushed: &Pushed) -> String {
    let mut text = format!(
        "{} added, {} already known",
        pushed.added, pushed.already_known
    );
    if !pushed.conflicts.is_empty() {
        text = format!(
            "{text}. The Hub has a different secret for {}; they were left as they were",
            pushed.conflicts.join(", ")
        );
    }
    text
}

#[derive(Serialize)]
pub struct SignedIn {
    email: String,
    /// What happened to the registry that was sent right after signing in, if there was one.
    registry: Option<String>,
}

/// Opens the Hub in the browser and waits for the admin to sign in there. When the profile already
/// has a registry, it is sent at once, so the Hub knows the sticks made before signing in.
#[tauri::command]
pub async fn hub_sign_in(
    state: State<'_, HubState>,
    profile_path: Option<PathBuf>,
) -> Result<SignedIn, String> {
    let session = tauri::async_runtime::spawn_blocking(sign_in)
        .await
        .map_err(|e| e.to_string())??;
    let email = session.email.clone();
    crate::vault::save(&session);
    state.checked.store(true, Ordering::Relaxed);
    if let Some(old) = state.lock().replace(session) {
        ck_admin_core::sign_out(HUB_URL, &old);
    }
    let registry = profile_path
        .map(|path| registry_of(&path))
        .filter(|registry| registry.is_file())
        .and_then(|registry| state.push(&registry))
        .map(|sent| sent.unwrap_or_else(|why| format!("not sent: {why}")));
    Ok(SignedIn { email, registry })
}

fn sign_in() -> Result<HubSession, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let state = ck_admin_core::new_state();
    let url = ck_admin_core::sign_in_url(HUB_URL, port, &state).map_err(|e| message(&e))?;
    open::that(&url).map_err(|e| format!("could not open the browser: {e}"))?;
    let code =
        ck_admin_core::wait_for_code(&listener, &state, SIGN_IN_WAIT).map_err(|e| message(&e))?;
    ck_admin_core::sign_in_with_code(HUB_URL, &code).map_err(|e| message(&e))
}

/// The signed-in admin's email. The first time after a run that restored a saved session, the Hub is
/// asked whether it still stands; a Hub that cannot be reached is not an answer, so it is kept.
#[tauri::command]
pub async fn hub_status(state: State<'_, HubState>) -> Result<Option<String>, String> {
    if !state.checked.swap(true, Ordering::Relaxed) && state.signed_in() {
        let ended = state
            .as_admin(|session| ck_admin_core::list_profiles(HUB_URL, session))
            .err()
            .is_some_and(|why| why.contains("sign in again"));
        if ended {
            state.lock().take();
            crate::vault::forget();
        }
    }
    Ok(state.lock().as_ref().map(|session| session.email.clone()))
}

#[tauri::command]
pub async fn hub_sign_out(state: State<'_, HubState>) -> Result<(), String> {
    let session = state.lock().take();
    crate::vault::forget();
    if let Some(session) = session {
        tauri::async_runtime::spawn_blocking(move || ck_admin_core::sign_out(HUB_URL, &session))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

impl HubState {
    /// Runs a Hub request as the signed-in admin, or says to sign in.
    fn as_admin<T>(
        &self,
        request: impl FnOnce(&HubSession) -> Result<T, ck_admin_core::Error>,
    ) -> Result<T, String> {
        let guard = self.lock();
        let session = guard
            .as_ref()
            .ok_or_else(|| "sign in to the Hub first".to_owned())?;
        request(session).map_err(|e| message(&e))
    }
}

#[tauri::command]
pub async fn hub_list_profiles(state: State<'_, HubState>) -> Result<Vec<ProfileSummary>, String> {
    state.as_admin(|session| ck_admin_core::list_profiles(HUB_URL, session))
}

#[derive(Serialize)]
pub struct Loaded {
    profile: Profile,
    path: PathBuf,
}

/// Downloads a profile of the organization into this program's own folder, with its wallpaper
/// beside it, so it opens like any saved profile.
#[tauri::command]
pub async fn hub_load_profile(
    app: AppHandle,
    state: State<'_, HubState>,
    id: String,
) -> Result<Loaded, String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("the Hub gave a profile id that cannot be used as a folder name".to_owned());
    }
    let remote = state.as_admin(|session| ck_admin_core::fetch_profile(HUB_URL, session, &id))?;
    let folder = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("profiles")
        .join(&id);
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    let mut settings = remote.settings;
    if let Some(wallpaper) = &remote.wallpaper {
        // The name is checked with the settings when the profile is saved, before it is trusted.
        settings.branding.wallpaper = Some(wallpaper.name.clone());
        if wallpaper.name.contains(['/', '\\']) || wallpaper.name == ".." {
            return Err("the Hub gave a wallpaper name that is not a file name".to_owned());
        }
        std::fs::write(folder.join(&wallpaper.name), &wallpaper.bytes)
            .map_err(|e| format!("could not save the wallpaper: {e}"))?;
    }
    let profile = Profile {
        name: remote.name,
        settings,
        hub_id: Some(id),
    };
    let path = folder.join("contest-profile.json");
    profile.save(&path).map_err(|e| message(&e))?;
    Ok(Loaded { profile, path })
}

/// Saves the profile to the Hub, with its wallpaper, and remembers the Hub's id in the local file.
#[tauri::command]
pub async fn hub_save_profile(
    state: State<'_, HubState>,
    path: PathBuf,
    mut profile: Profile,
) -> Result<Profile, String> {
    let id = profile
        .hub_id
        .clone()
        .unwrap_or_else(ck_admin_core::new_profile_id);
    let wallpaper =
        Wallpaper::read(&profile.settings, folder_of(&path)).map_err(|e| message(&e))?;
    let upload = HubProfile {
        name: profile.name.clone(),
        settings: profile.settings.clone(),
        wallpaper,
    };
    state.as_admin(|session| ck_admin_core::upload_profile(HUB_URL, session, &id, &upload))?;
    profile.hub_id = Some(id);
    profile.save(&path).map_err(|e| message(&e))?;
    Ok(profile)
}
