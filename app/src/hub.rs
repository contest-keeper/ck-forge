//! Signing in to the Hub through the browser, and sending it the registry once signed in.
//!
//! The session lives in memory only: closing the window signs out, and nothing about it is saved,
//! printed or sent to the page except the admin's email.

// Tauri hands each command its arguments by value.
#![allow(clippy::needless_pass_by_value)]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use ck_admin_core::{HubSession, Pushed};
use serde::Serialize;
use tauri::State;

use crate::profiles::{message, registry_of};

/// Where sticks report and where admins sign in. Sticks only get this address when the admin who
/// made them was signed in. Release builds use the production Hub; the develop build sets
/// `CK_HUB_URL` to the develop Hub when it compiles.
pub const HUB_URL: &str = match option_env!("CK_HUB_URL") {
    Some(url) => url,
    None => "https://hub.contestkeeper.com",
};

const SIGN_IN_WAIT: Duration = Duration::from_secs(180);

#[derive(Default)]
pub struct HubState(Mutex<Option<HubSession>>);

impl HubState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<HubSession>> {
        self.0
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

#[tauri::command]
pub fn hub_status(state: State<'_, HubState>) -> Option<String> {
    state.lock().as_ref().map(|session| session.email.clone())
}

#[tauri::command]
pub async fn hub_sign_out(state: State<'_, HubState>) -> Result<(), String> {
    let session = state.lock().take();
    if let Some(session) = session {
        tauri::async_runtime::spawn_blocking(move || ck_admin_core::sign_out(HUB_URL, &session))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
