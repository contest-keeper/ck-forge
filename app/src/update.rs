//! Looks for a newer release when the window starts and offers to install it.
//!
//! The pipeline writes `min_version` into the update feed. A copy older than that must update before
//! it can be used; any other update can be installed, put off, or skipped for good.

use std::{fs, path::PathBuf};

use semver::Version;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogResult};
use tauri_plugin_updater::{Update, UpdaterExt};

const INSTALL: &str = "Install";
const SKIP: &str = "Skip this version";
const LATER: &str = "Later";

/// Checks in the background, so a slow or missing network never delays the window.
pub fn check_on_start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Offline or no release yet: not worth bothering the user, the next start tries again.
        if let Err(error) = offer_update(&app).await {
            eprintln!("update check failed: {error}");
        }
    });
}

async fn offer_update(app: &AppHandle) -> tauri_plugin_updater::Result<()> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(());
    };
    if is_mandatory(&update) {
        return require_update(app, &update).await;
    }
    if skipped_version(app).as_deref() == Some(update.version.as_str()) {
        return Ok(());
    }
    let answer = app
        .dialog()
        .message(format!(
            "Keeper Forge {} is available. Install it now? The window closes and reopens.",
            update.version
        ))
        .title("Update available")
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            INSTALL.into(),
            SKIP.into(),
            LATER.into(),
        ))
        .blocking_show_with_result();
    match answer {
        MessageDialogResult::Custom(button) if button == INSTALL => install(app, &update).await,
        MessageDialogResult::Custom(button) if button == SKIP => {
            remember_skipped(app, &update.version);
            Ok(())
        }
        _ => Ok(()),
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

/// There is no "Later": the window closes unless the update is installed.
async fn require_update(app: &AppHandle, update: &Update) -> tauri_plugin_updater::Result<()> {
    let accepted = app
        .dialog()
        .message(format!(
            "This version of Keeper Forge is no longer supported. Version {} must be installed to continue.",
            update.version
        ))
        .title("Update required")
        .buttons(MessageDialogButtons::OkCustom(INSTALL.into()))
        .blocking_show();
    let result = if accepted {
        install(app, update).await
    } else {
        Ok(())
    };
    // A successful install restarts the program. Reaching this line means it did not.
    app.exit(0);
    result
}

async fn install(app: &AppHandle, update: &Update) -> tauri_plugin_updater::Result<()> {
    update.download_and_install(|_, _| {}, || {}).await?;
    app.restart();
}

fn skipped_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|folder| folder.join("skipped-update.txt"))
}

fn skipped_version(app: &AppHandle) -> Option<String> {
    let text = fs::read_to_string(skipped_file(app)?).ok()?;
    Some(text.trim().to_string())
}

fn remember_skipped(app: &AppHandle, version: &str) {
    let Some(file) = skipped_file(app) else {
        return;
    };
    if let Some(folder) = file.parent() {
        let _ = fs::create_dir_all(folder);
    }
    if let Err(error) = fs::write(&file, version) {
        eprintln!("could not remember the skipped version: {error}");
    }
}
