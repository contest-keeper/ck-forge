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
        if let Some(entry) = entry()
            && entry.set_password(&value).is_err()
        {
            // Not being able to save only means signing in again next time. But the entry must not
            // keep the account of an earlier sign-in, which the next start would restore.
            let _ = entry.delete_credential();
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

    pub fn save(_: &HubSession) -> bool {
        false
    }
    pub fn load() -> Option<HubSession> {
        None
    }
    pub fn forget() {}
}

use std::sync::Mutex;

/// What this run knows of the saved session: not asked yet (`None`), or the answer of the vault, as
/// the account and the secret. Asking the macOS Keychain shows a permission question for each read
/// to a program that is not signed by a developer, so the vault is read once and not for every
/// request to the Hub.
type Known = Option<Option<(String, String)>>;
static KNOWN: Mutex<Known> = Mutex::new(None);

/// The saved session, read from the vault the first time and then kept in memory.
pub fn load() -> Option<HubSession> {
    load_with(&KNOWN, store::load)
}

fn load_with(
    known: &Mutex<Known>,
    read: impl FnOnce() -> Option<HubSession>,
) -> Option<HubSession> {
    let mut known = known.lock().ok()?;
    let held = known.get_or_insert_with(|| {
        read().map(|session| (session.email.clone(), session.token().to_owned()))
    });
    held.as_ref()
        .map(|(email, token)| HubSession::restore(token.clone(), email.clone()))
}

/// Saves the session in the vault and keeps it in memory: a vault that refuses it still leaves the
/// admin signed in until the program ends.
pub fn save(session: &HubSession) {
    store::save(session);
    if let Ok(mut known) = KNOWN.lock() {
        *known = Some(Some((session.email.clone(), session.token().to_owned())));
    }
}

/// Forgets the session, in the vault and in memory.
pub fn forget() {
    store::forget();
    if let Ok(mut known) = KNOWN.lock() {
        *known = Some(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vault_is_asked_once_and_the_answer_is_kept() {
        let known = Mutex::new(None);
        let mut asked = 0;
        for _ in 0..3 {
            let session = load_with(&known, || {
                asked += 1;
                Some(HubSession::restore("secret".to_owned(), "a@b.c".to_owned()))
            });
            assert_eq!(session.expect("session").email, "a@b.c");
        }
        assert_eq!(asked, 1);
    }

    #[test]
    fn an_empty_vault_is_asked_only_once_too() {
        let known = Mutex::new(None);
        let mut asked = 0;
        for _ in 0..3 {
            assert!(
                load_with(&known, || {
                    asked += 1;
                    None
                })
                .is_none()
            );
        }
        assert_eq!(asked, 1);
    }
}
