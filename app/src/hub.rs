//! Signing in to the Hub through the browser, and sending it the registry once signed in.
//!
//! The session is kept in memory and in the Windows Credential Manager, so the admin is still
//! signed in after closing the program, until the Hub ends it. Nothing about it is written to a
//! file, printed or sent to the page except the admin's email.

// Tauri hands each command its arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ck_admin_core::{HubSession, PairingEnd, Profile, ProfileSummary, Pushed};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::profiles::{message, registry_of};

pub use ck_admin_core::HUB_URL;

const SIGN_IN_WAIT: Duration = Duration::from_secs(600);
const POLL_EVERY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct HubState {
    /// Shared, so a request runs without the lock: a slow Hub must not block every other command.
    session: Mutex<Option<Arc<HubSession>>>,
    /// Whether the Hub has said yes to the session saved by an earlier run.
    checked: AtomicBool,
    /// Set by the Cancel button to stop a sign-in that is waiting for the browser.
    cancel: AtomicBool,
}

impl HubState {
    /// Starts signed in when an earlier run saved a session.
    pub fn restored() -> Self {
        Self {
            session: Mutex::new(ck_admin_core::vault::load().map(Arc::new)),
            checked: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Arc<HubSession>>> {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn current(&self) -> Option<Arc<HubSession>> {
        self.lock().clone()
    }

    pub fn signed_in(&self) -> bool {
        self.lock().is_some()
    }

    /// Sends the registry as the signed-in admin. `None` when nobody is signed in.
    pub fn push(&self, registry: &Path) -> Option<Result<String, String>> {
        let session = self.current()?;
        Some(
            ck_admin_core::push_registry_signed_in(HUB_URL, &session, registry)
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
pub struct SignInEnd {
    /// `approved`, `denied`, `expired` or `cancelled`.
    status: &'static str,
    email: Option<String>,
    /// What happened to the registry that was sent right after signing in, if there was one.
    registry: Option<String>,
}

/// Shows the admin a pairing code, opens the Hub in the browser and waits for the admin to agree
/// there. When the profile already has a registry, it is sent at once, so the Hub knows the sticks
/// made before signing in.
#[tauri::command]
pub async fn hub_sign_in(
    app: AppHandle,
    state: State<'_, HubState>,
    profile_path: Option<PathBuf>,
) -> Result<SignInEnd, String> {
    state.cancel.store(false, Ordering::Relaxed);
    let started = Instant::now();
    let end = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || sign_in(&app)
    })
    .await
    .map_err(|e| e.to_string())??;
    let status = match end {
        PairingEnd::Approved(session) => {
            let email = session.email.clone();
            ck_admin_core::vault::save(&session);
            state.checked.store(true, Ordering::Relaxed);
            // Taken out of the lock first: signing out is a slow request.
            let old = state.lock().replace(Arc::new(session));
            if let Some(old) = old {
                ck_admin_core::sign_out(HUB_URL, &old);
            }
            let registry = profile_path
                .map(|path| registry_of(&path))
                .filter(|registry| registry.is_file())
                .and_then(|registry| state.push(&registry))
                .map(|sent| sent.unwrap_or_else(|why| format!("not sent: {why}")));
            return Ok(SignInEnd {
                status: "approved",
                email: Some(email),
                registry,
            });
        }
        PairingEnd::Cancelled => "cancelled",
        // The Hub says "gone" for a refusal and for an end alike; its 10 minutes tell them apart.
        PairingEnd::Gone if started.elapsed() < Duration::from_secs(590) => "denied",
        PairingEnd::Gone | PairingEnd::TimedOut => "expired",
    };
    Ok(SignInEnd {
        status,
        email: None,
        registry: None,
    })
}

/// Stops the sign-in that is waiting for the browser.
#[tauri::command]
pub fn hub_sign_in_cancel(state: State<'_, HubState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

/// Asks the Hub for a pairing code, shows it to the window and opens the Hub page that shows the
/// same one, then waits for the admin to agree there.
fn sign_in(app: &AppHandle) -> Result<PairingEnd, String> {
    let pairing = ck_admin_core::start_pairing(HUB_URL).map_err(|e| message(&e))?;
    let _ = app.emit("hub-pairing-code", &pairing.code);
    // The code is on screen too, so a browser that does not open is not the end of it.
    let _ = open::that(&pairing.url);
    let state = app.state::<HubState>();
    ck_admin_core::watch_pairing(HUB_URL, &pairing, SIGN_IN_WAIT, POLL_EVERY, &state.cancel)
        .map_err(|e| message(&e))
}

/// The signed-in admin's email. The first time after a run that restored a saved session, the Hub is
/// asked whether it still stands; a Hub that cannot be reached is not an answer, so it is kept.
#[tauri::command]
pub async fn hub_status(state: State<'_, HubState>) -> Result<Option<String>, String> {
    if !state.checked.swap(true, Ordering::Relaxed)
        && let Some(session) = state.current()
        && matches!(
            ck_admin_core::list_profiles(HUB_URL, &session),
            Err(ck_admin_core::Error::SignedOut(_))
        )
    {
        // Only the session that was asked: a sign-in may have replaced it meanwhile.
        let mut current = state.lock();
        if current
            .as_ref()
            .is_some_and(|now| Arc::ptr_eq(now, &session))
        {
            current.take();
            ck_admin_core::vault::forget();
        }
    }
    Ok(state.lock().as_ref().map(|session| session.email.clone()))
}

#[tauri::command]
pub async fn hub_sign_out(state: State<'_, HubState>) -> Result<(), String> {
    let session = state.lock().take();
    ck_admin_core::vault::forget();
    if let Some(session) = session {
        tauri::async_runtime::spawn_blocking(move || ck_admin_core::sign_out(HUB_URL, &session))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The Hub's sticks, so a disk about to be written can be matched with the stick it held before.
#[tauri::command]
pub async fn hub_list_sticks(
    state: State<'_, HubState>,
) -> Result<Vec<ck_admin_core::HubStick>, String> {
    state.as_admin(|session| ck_admin_core::list_sticks(HUB_URL, session))
}

#[tauri::command]
pub async fn hub_retire_stick(state: State<'_, HubState>, id: String) -> Result<(), String> {
    state.as_admin(|session| ck_admin_core::retire_stick_credential(HUB_URL, session, &id))
}

impl HubState {
    /// Runs a Hub request as the signed-in admin, or says to sign in.
    pub(crate) fn as_admin<T>(
        &self,
        request: impl FnOnce(&HubSession) -> Result<T, ck_admin_core::Error>,
    ) -> Result<T, String> {
        let session = self
            .current()
            .ok_or_else(|| "sign in to the Hub first".to_owned())?;
        request(&session).map_err(|e| message(&e))
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
    let folder = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("profiles");
    let loaded = state
        .as_admin(|session| ck_admin_core::load_hub_profile(HUB_URL, session, &id, &folder))?;
    Ok(Loaded {
        profile: loaded.profile,
        path: loaded.path,
    })
}

/// Saves the profile to the Hub, with its wallpaper, and remembers the Hub's id in the local file.
#[tauri::command]
pub async fn hub_save_profile(
    state: State<'_, HubState>,
    path: PathBuf,
    profile: Profile,
) -> Result<Profile, String> {
    state.as_admin(|session| ck_admin_core::save_hub_profile(HUB_URL, session, &path, profile))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hub_request_does_not_hold_the_session_lock() {
        let state = Arc::new(HubState::default());
        *state.lock() = Some(Arc::new(HubSession::restore(
            "token".to_owned(),
            "a@example.test".to_owned(),
        )));
        let (done, result) = std::sync::mpsc::channel();
        let again = Arc::clone(&state);
        std::thread::spawn(move || {
            // Another command asks while this request is still running.
            let seen = again.as_admin(|_| Ok(again.signed_in()));
            let _ = done.send(seen);
        });
        assert_eq!(result.recv_timeout(Duration::from_secs(5)), Ok(Ok(true)));
    }
}
