//! `ck-forge hub`: the admin's session on the Hub, and what the window does with it.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use crate::cli::HubCommand;
use ck_admin_core::vault;
use ck_admin_core::{Error, HUB_URL, HubImageStep, HubSession, Profile, retire_stick_credential};
use indicatif::{ProgressBar, ProgressStyle};

const SIGN_IN_WAIT: Duration = Duration::from_secs(600);
const POLL_EVERY: Duration = Duration::from_secs(2);

/// The session an earlier `hub sign-in` or the window saved, or a request to sign in.
fn session() -> Result<HubSession, Error> {
    vault::load().ok_or_else(|| {
        Error::Hub("sign in to the Hub first with `ck-forge hub sign-in`".to_owned())
    })
}

/// Drops the saved session when the Hub says it ended, so the next command asks to sign in.
fn forget_ended<T>(result: Result<T, Error>) -> Result<T, Error> {
    if matches!(result, Err(Error::SignedOut(_))) {
        vault::forget();
    }
    result
}

fn hub_sign_in() -> Result<String, Error> {
    let pairing = ck_admin_core::start_pairing(HUB_URL)?;
    eprintln!(
        "Code {}. Sign in to the Hub in the browser and check that it shows the same code: {}",
        pairing.code, pairing.url
    );
    // Not being able to open the browser only means the admin opens the address above.
    let _ = open::that(&pairing.url);
    let session = ck_admin_core::wait_for_pairing(HUB_URL, &pairing, SIGN_IN_WAIT, POLL_EVERY)?;
    vault::save(&session);
    Ok(format!(
        "Signed in as {}
",
        session.email
    ))
}

fn hub_sign_out() -> String {
    if let Some(session) = vault::load() {
        ck_admin_core::sign_out(HUB_URL, &session);
    }
    vault::forget();
    "Signed out\n".to_owned()
}

fn hub_status() -> Result<String, Error> {
    let Some(session) = vault::load() else {
        return Ok("Not signed in\n".to_owned());
    };
    forget_ended(ck_admin_core::list_profiles(HUB_URL, &session))?;
    Ok(format!("Signed in as {}\n", session.email))
}

fn hub_profiles() -> Result<String, Error> {
    let session = session()?;
    let profiles = forget_ended(ck_admin_core::list_profiles(HUB_URL, &session))?;
    let mut text = String::new();
    for profile in profiles {
        let _ = writeln!(text, "{}  {}", profile.id, profile.name);
    }
    Ok(text)
}

fn hub_pull(id: &str, folder: &Path) -> Result<String, Error> {
    let session = session()?;
    let loaded = forget_ended(ck_admin_core::load_hub_profile(
        HUB_URL, &session, id, folder,
    ))?;
    Ok(format!("Saved {}\n", loaded.path.display()))
}

fn hub_push(file: &Path) -> Result<String, Error> {
    let session = session()?;
    let profile = Profile::load(file)?;
    let saved = forget_ended(ck_admin_core::save_hub_profile(
        HUB_URL, &session, file, profile,
    ))?;
    Ok(format!(
        "Saved {} to the Hub as {}\n",
        file.display(),
        saved.hub_id.unwrap_or_default()
    ))
}

fn format_hub_stick(stick: &ck_admin_core::HubStick) -> String {
    let mut parts: Vec<String> = [&stick.stick_id, &stick.team, &stick.site, &stick.contest]
        .into_iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    match (&stick.retired_at, stick.machine_number) {
        (Some(_), _) => parts.push("[retired]".to_owned()),
        (None, Some(number)) => parts.push(format!("machine {number}")),
        (None, None) => {}
    }
    parts.join("  ")
}

fn hub_sticks() -> Result<String, Error> {
    let session = session()?;
    let sticks = forget_ended(ck_admin_core::list_sticks(HUB_URL, &session))?;
    let mut text = String::new();
    for stick in &sticks {
        let _ = writeln!(text, "{}", format_hub_stick(stick));
    }
    Ok(text)
}

fn hub_retire(id: &str) -> Result<String, Error> {
    let session = session()?;
    forget_ended(retire_stick_credential(HUB_URL, &session, id))?;
    Ok(format!("Retired the key of {id}\n"))
}

fn hub_images() -> Result<String, Error> {
    let session = session()?;
    let images = forget_ended(ck_admin_core::list_hub_images(HUB_URL, &session))?;
    let mut text = String::new();
    for image in images {
        let _ = writeln!(
            text,
            "{}  {}  {}  {} MB",
            image.id,
            image.name,
            image.version,
            image.size / (1024 * 1024)
        );
    }
    Ok(text)
}

fn hub_download(id: &str, out: &Path) -> Result<String, Error> {
    let session = session()?;
    let download = forget_ended(ck_admin_core::prepare_hub_image_download(
        HUB_URL, &session, id,
    ))?;
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template("{msg:12} {bar:40} {bytes}/{total_bytes}")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    ck_admin_core::download_hub_image(download, out, |step, done, total| {
        bar.set_message(match step {
            HubImageStep::Download => "downloading",
            HubImageStep::Import => "importing",
        });
        bar.set_length(total);
        bar.set_position(done);
    })?;
    bar.finish_and_clear();
    Ok(format!(
        "Saved the base image to {}
",
        out.display()
    ))
}

pub(crate) fn run_hub(command: HubCommand) -> Result<String, Error> {
    match command {
        HubCommand::SignIn => hub_sign_in(),
        HubCommand::SignOut => Ok(hub_sign_out()),
        HubCommand::Status => hub_status(),
        HubCommand::Profiles => hub_profiles(),
        HubCommand::Pull { id, folder } => hub_pull(&id, &folder),
        HubCommand::Push { file } => hub_push(&file),
        HubCommand::Sticks => hub_sticks(),
        HubCommand::Retire { id } => hub_retire(&id),
        HubCommand::Images => hub_images(),
        HubCommand::Download { id, out } => hub_download(&id, &out),
    }
}

#[cfg(test)]
mod tests {
    use ck_admin_core::HubStick;

    use super::*;

    fn stick(id: &str) -> HubStick {
        serde_json::from_str(&format!(r#"{{"stick_id":"{id}"}}"#)).expect("stick")
    }

    #[test]
    fn unactivated_stick_has_no_blank_field_gaps() {
        assert_eq!(format_hub_stick(&stick("abc123")), "abc123");

        let mut active = stick("def456");
        active.team = "Alpha".to_owned();
        active.site = "Hall A".to_owned();
        active.contest = "Finals".to_owned();
        active.machine_number = Some(3);
        assert_eq!(
            format_hub_stick(&active),
            "def456  Alpha  Hall A  Finals  machine 3"
        );

        active.retired_at = Some("2026-11-27".to_owned());
        assert_eq!(
            format_hub_stick(&active),
            "def456  Alpha  Hall A  Finals  [retired]"
        );
    }
}
