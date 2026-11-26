//! Looks for a newer release when the window starts and again every few hours, so a window left
//! open still hears about it. The Svelte page shows the offer (themed like the rest of the window);
//! this file only finds, remembers and installs.
//!
//! The pipeline writes `min_version` into the update feed. A copy older than that must update before
//! it can be used; any other update can be put off with "Later".

use std::{sync::Mutex, time::Duration};

use semver::Version;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const EVENT: &str = "update-available";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// A stalled feed or download ends in an error instead of hanging the checks or the dialog.
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const RELEASES: &str = "https://github.com/contest-keeper/ck-forge/releases/tag/v";

/// The update that was found, kept for the page to ask for when it opens and for the install.
#[derive(Default)]
pub struct Found(Mutex<Option<Update>>);

#[derive(Serialize, Clone)]
pub struct Offer {
    version: String,
    /// This build is too old to be used until it has updated: the page offers no "Later".
    mandatory: bool,
}

/// Checks in the background, so a slow or missing network never delays the window.
pub fn check_on_start(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            // Offline or no release yet: not worth bothering the user, the next round tries again.
            if let Err(error) = tauri::async_runtime::block_on(look(&app)) {
                eprintln!("update check failed: {error}");
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

async fn look(app: &AppHandle) -> tauri_plugin_updater::Result<()> {
    let Some(update) = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        .build()?
        .check()
        .await?
    else {
        return Ok(());
    };
    let offer = offer_of(&update);
    *app.state::<Found>().0.lock().expect("update lock") = Some(update);
    let _ = app.emit(EVENT, offer);
    Ok(())
}

fn offer_of(update: &Update) -> Offer {
    Offer {
        version: update.version.clone(),
        mandatory: is_mandatory(update),
    }
}

/// Only an update that is older than the feed's `min_version` is mandatory. A missing or unreadable
/// value never locks anyone out.
fn is_mandatory(update: &Update) -> bool {
    let Some(minimum) = update
        .raw_json
        .get("min_version")
        .and_then(|value| value.as_str())
    else {
        return false;
    };
    match (
        Version::parse(&update.current_version),
        Version::parse(minimum),
    ) {
        (Ok(current), Ok(minimum)) => current < minimum,
        _ => false,
    }
}

/// The release notes: the feed's own `release_notes_url` when it has an https one, else the
/// GitHub release of that version.
fn notes_url(update: &Update) -> String {
    update
        .raw_json
        .get("release_notes_url")
        .and_then(|value| value.as_str())
        .filter(|url| url.starts_with("https://"))
        .map_or_else(|| format!("{RELEASES}{}", update.version), str::to_owned)
}

/// For a page that opened after the check found something.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri commands take State by value
pub fn pending_update(found: State<Found>) -> Option<Offer> {
    found.0.lock().expect("update lock").as_ref().map(offer_of)
}

/// The page cannot name the address: the one the feed gave is opened, nothing else.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn open_release_notes(found: State<Found>) -> Result<(), String> {
    let url = found
        .0
        .lock()
        .expect("update lock")
        .as_ref()
        .map(notes_url)
        .ok_or("there is no update")?;
    open::that(url).map_err(|e| format!("could not open the browser: {e}"))
}

#[tauri::command]
pub async fn install_update(app: AppHandle, found: State<'_, Found>) -> Result<(), String> {
    let mut update = found
        .0
        .lock()
        .expect("update lock")
        .clone()
        .ok_or("there is no update")?;
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("could not install the update: {e}"))?;
    app.restart();
}
