//! Keeps the Hub sign-in between runs, in the Windows Credential Manager or the macOS Keychain.
//! Other systems keep nothing: the admin signs in again each time. The window and the command line
//! share the entry.

use crate::hub::HubSession;

#[cfg(any(windows, target_os = "macos"))]
mod store {
    use super::HubSession;

    fn entry() -> Option<keyring::Entry> {
        keyring::Entry::new("Keeper Forge", "hub-session").ok()
    }

    pub fn save(session: &HubSession) {
        let value = format!("{}\n{}", session.email, session.token());
        if let Some(entry) = entry() {
            // Not being able to save only means signing in again next time.
            let _ = entry.set_password(&value);
        }
    }

    pub fn load() -> Option<HubSession> {
        let value = entry()?.get_password().ok()?;
        let (email, token) = value.split_once('\n')?;
        Some(HubSession::restore(token.to_owned(), email.to_owned()))
    }

    pub fn forget() {
        if let Some(entry) = entry() {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod store {
    use super::HubSession;

    pub fn save(_: &HubSession) {}
    pub fn load() -> Option<HubSession> {
        None
    }
    pub fn forget() {}
}

pub use store::{forget, load, save};
