//! Looks for a newer build when the window starts and offers to install it. A port of the Tauri
//! updater of `app/`: it reads the same kind of feed (`update.json`), checks the installer against
//! the same minisign key, and honours `min_version`: a copy older than that must update before it
//! can be used; any other update can be installed, put off, or skipped for good.
//!
//! An installer is never run unless its signature checks out against the public key that was built
//! into this program. A build without a key (a development build) does not look for updates.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::Deserialize;

/// The public key the installers are signed with: the text of the minisign `.pub` file, in base64,
/// as Tauri keeps it. Set when the release is built.
const PUBKEY: Option<&str> = option_env!("CK_UPDATE_PUBKEY");

/// Where the newest build is announced.
const FEED: &str = match option_env!("CK_UPDATE_FEED") {
    Some(feed) => feed,
    None => "https://github.com/contest-keeper/ck-forge/releases/latest/download/update.json",
};

/// The version of this build: the release workflow sets it, otherwise it is the crate's.
pub const VERSION: &str = match option_env!("CK_FORGE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// The key of this system in the feed's `platforms`.
const PLATFORM: &str = "windows-x86_64";

/// No installer is larger than this, so a feed cannot make the window fill the disk.
const MAX_INSTALLER_BYTES: u64 = 300 * 1024 * 1024;
const MAX_FEED_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Feed {
    version: String,
    min_version: Option<String>,
    platforms: HashMap<String, Platform>,
}

#[derive(Deserialize)]
struct Platform {
    /// The minisign signature file, in base64.
    signature: String,
    url: String,
}

/// A build that is newer than this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub version: String,
    url: String,
    signature: String,
    /// This build is older than the feed's `min_version`, so it must update.
    pub mandatory: bool,
}

/// Whether this program can check an update at all: only a build with the public key can.
pub fn enabled() -> bool {
    cfg!(windows) && PUBKEY.is_some()
}

/// Reads a feed: the newer build for this system, or none when `current` is up to date or the feed
/// has no build for this system. An unreadable feed is an error and is never mistaken for "up to
/// date" by the caller, which only shows it.
pub fn newer_in(feed: &str, current: &str) -> Result<Option<Available>, String> {
    let feed: Feed = serde_json::from_str(feed).map_err(|e| format!("unreadable feed: {e}"))?;
    let current =
        Version::parse(current).map_err(|e| format!("this version is unreadable: {e}"))?;
    let newest = Version::parse(&feed.version).map_err(|e| format!("unreadable feed: {e}"))?;
    if newest <= current {
        return Ok(None);
    }
    let Some(platform) = feed.platforms.get(PLATFORM) else {
        return Ok(None);
    };
    // Only a secure address, so the installer cannot be swapped on the way. The signature is
    // checked as well, but nothing is downloaded over an open connection.
    if !platform.url.starts_with("https://") {
        return Err("the feed names an installer that is not on an https address".to_owned());
    }
    Ok(Some(Available {
        version: feed.version,
        url: platform.url.clone(),
        signature: platform.signature.clone(),
        mandatory: is_mandatory(&current, feed.min_version.as_deref()),
    }))
}

/// Only a build older than the feed's `min_version` is mandatory. A missing or unreadable value
/// never locks anyone out.
fn is_mandatory(current: &Version, minimum: Option<&str>) -> bool {
    minimum
        .and_then(|minimum| Version::parse(minimum).ok())
        .is_some_and(|minimum| *current < minimum)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(5)
        .timeout_global(Some(Duration::from_secs(900)))
        .build()
        .into()
}

/// Asks the feed whether there is a newer build than this one.
pub fn check() -> Result<Option<Available>, String> {
    let mut answer = agent()
        .get(FEED)
        .call()
        .map_err(|e| format!("could not reach the update feed: {e}"))?;
    let text = answer
        .body_mut()
        .with_config()
        .limit(MAX_FEED_BYTES)
        .read_to_string()
        .map_err(|e| format!("could not read the update feed: {e}"))?;
    newer_in(&text, VERSION)
}

/// Checks `data` against the signature of the feed with the key built into this program.
fn verify(data: &mut impl Read, pubkey_b64: &str, signature_b64: &str) -> Result<(), String> {
    let decode = |text: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    };
    let key = decode(pubkey_b64)
        .and_then(|text| PublicKey::decode(&text).ok())
        .ok_or("the update key built into this program is unreadable")?;
    let signature = decode(signature_b64)
        .and_then(|text| Signature::decode(&text).ok())
        .ok_or("the signature of the update is unreadable")?;
    let mut verifier = key
        .verify_stream(&signature)
        .map_err(|_| "the signature of the update is not by Contest Keeper".to_owned())?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = data
            .read(&mut buffer)
            .map_err(|e| format!("could not read the download: {e}"))?;
        if count == 0 {
            break;
        }
        verifier.update(&buffer[..count]);
    }
    verifier
        .finalize()
        .map_err(|_| "the download does not match its signature".to_owned())
}

/// Deletes the downloaded installer unless it was kept: every failed path leaves nothing behind.
struct Partial {
    path: PathBuf,
    keep: bool,
}

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Downloads the installer of `update` into `folder`, checks it against its signature and returns
/// where it is. A download that does not check out is deleted. `progress` gets the bytes done and
/// the total (0 when not known).
pub fn download(
    update: &Available,
    folder: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let pubkey = PUBKEY.ok_or("this build has no update key")?;
    let answer = agent()
        .get(&update.url)
        .call()
        .map_err(|e| format!("could not download the update: {e}"))?;
    let total = answer.body().content_length().unwrap_or(0);
    if total > MAX_INSTALLER_BYTES {
        return Err("the update is larger than any installer should be".to_owned());
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("could not make the folder: {e}"))?;
    let setup = folder.join("keeper-forge-update-setup.exe");
    let mut reader = answer
        .into_body()
        .into_reader()
        .take(MAX_INSTALLER_BYTES + 1);
    let mut file = File::create(&setup).map_err(|e| format!("could not save the update: {e}"))?;
    let mut guard = Partial {
        path: setup.clone(),
        keep: false,
    };
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut done = 0_u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|e| format!("the download was interrupted: {e}"))?;
        if count == 0 {
            break;
        }
        done += count as u64;
        if done > MAX_INSTALLER_BYTES {
            return Err("the update is larger than any installer should be".to_owned());
        }
        file.write_all(&buffer[..count])
            .map_err(|e| format!("could not save the update: {e}"))?;
        progress(done, total);
    }
    drop(file);
    let checked = File::open(&setup)
        .map_err(|e| format!("could not read the update: {e}"))
        .and_then(|mut file| verify(&mut file, pubkey, &update.signature));
    checked?;
    guard.keep = true;
    Ok(setup)
}

/// Runs the checked installer and starts the program again once it is done. The installer is per
/// machine, so it goes through the Windows permission prompt. This program must close right after:
/// the installer cannot replace a program that is running, so a helper waits a moment first.
pub fn install_and_restart(setup: &Path) -> Result<(), String> {
    let app = std::env::current_exe().map_err(|e| e.to_string())?;
    // The paths go in environment variables, so nothing here is part of the command text.
    let script = "Start-Sleep -Seconds 2; \
        Start-Process -FilePath $env:CK_SETUP -ArgumentList '/S' -Verb RunAs -Wait; \
        Start-Process -FilePath $env:CK_APP";
    let mut powershell = Command::new("powershell.exe");
    powershell
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("CK_SETUP", setup)
        .env("CK_APP", app);
    ck_writer::without_console_window(&mut powershell)
        .spawn()
        .map_err(|e| format!("could not start the installer: {e}"))?;
    Ok(())
}

/// The version the admin chose to skip, kept in the Forge folder.
pub fn skipped_version(folder: &Path) -> Option<String> {
    let text = std::fs::read_to_string(folder.join("skipped-update.txt")).ok()?;
    Some(text.trim().to_owned())
}

pub fn remember_skipped(folder: &Path, version: &str) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    std::fs::write(folder.join("skipped-update.txt"), version).map_err(|e| e.to_string())
}

/// Whether to tell the admin about `update`: a skipped version stays quiet, unless this build must
/// update anyway.
pub fn worth_offering(update: &Available, skipped: Option<&str>) -> bool {
    update.mandatory || skipped != Some(update.version.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(version: &str, min: Option<&str>, url: &str) -> String {
        let min = min.map_or_else(String::new, |min| format!(r#""min_version": "{min}","#));
        format!(
            r#"{{"version":"{version}",{min}"platforms":{{"windows-x86_64":{{"signature":"c2ln","url":"{url}"}}}}}}"#
        )
    }

    const SETUP: &str =
        "https://github.com/contest-keeper/ck-forge/releases/download/dev/setup.exe";

    #[test]
    fn a_newer_preview_is_found_and_the_same_or_older_one_is_not() {
        let found = newer_in(&feed("0.2.0-preview.97", None, SETUP), "0.2.0-preview.9")
            .expect("feed")
            .expect("newer");
        assert_eq!(found.version, "0.2.0-preview.97");
        assert!(!found.mandatory);
        assert_eq!(
            newer_in(&feed("0.2.0-preview.9", None, SETUP), "0.2.0-preview.9"),
            Ok(None)
        );
        assert_eq!(
            newer_in(&feed("0.1.0", None, SETUP), "0.2.0-preview.9"),
            Ok(None)
        );
    }

    #[test]
    fn a_build_older_than_the_minimum_must_update_and_a_bad_minimum_never_locks_anyone_out() {
        let old = "0.1.0";
        let mandatory = newer_in(&feed("0.3.0", Some("0.2.0"), SETUP), old)
            .unwrap()
            .unwrap();
        assert!(mandatory.mandatory);
        let optional = newer_in(&feed("0.3.0", Some("0.1.0"), SETUP), old)
            .unwrap()
            .unwrap();
        assert!(!optional.mandatory);
        let nonsense = newer_in(&feed("0.3.0", Some("soon"), SETUP), old)
            .unwrap()
            .unwrap();
        assert!(!nonsense.mandatory);
    }

    #[test]
    fn an_unreadable_feed_or_an_open_address_is_an_error_and_never_up_to_date() {
        assert!(newer_in("not json", "0.1.0").is_err());
        assert!(newer_in(&feed("later", None, SETUP), "0.1.0").is_err());
        assert!(
            newer_in(
                &feed("0.3.0", None, "http://example.org/setup.exe"),
                "0.1.0"
            )
            .is_err()
        );
    }

    #[test]
    fn a_feed_without_a_build_for_this_system_offers_nothing() {
        let text = r#"{"version":"9.0.0","platforms":{"linux-x86_64":{"signature":"x","url":"https://x"}}}"#;
        assert_eq!(newer_in(text, "0.1.0"), Ok(None));
    }

    #[test]
    fn the_feed_the_tauri_window_reads_is_understood() {
        // The shape `tools/make-update-manifest.sh` writes.
        let text = r#"{"version":"0.3.0","pub_date":"2026-11-26T10:00:00Z","min_version":"0.2.0",
            "platforms":{"windows-x86_64":{"signature":"c2ln",
            "url":"https://github.com/contest-keeper/ck-forge/releases/download/dev/keeper-forge-setup-x64.exe"}}}"#;
        let found = newer_in(text, "0.1.0").expect("feed").expect("newer");
        assert_eq!(found.version, "0.3.0");
        assert!(found.mandatory);
    }

    #[test]
    fn a_skipped_version_stays_quiet_unless_the_update_is_mandatory() {
        let optional = newer_in(&feed("0.3.0", None, SETUP), "0.1.0")
            .unwrap()
            .unwrap();
        assert!(worth_offering(&optional, None));
        assert!(worth_offering(&optional, Some("0.2.0")));
        assert!(!worth_offering(&optional, Some("0.3.0")));
        let mandatory = newer_in(&feed("0.3.0", Some("0.3.0"), SETUP), "0.1.0")
            .unwrap()
            .unwrap();
        assert!(worth_offering(&mandatory, Some("0.3.0")));
    }

    #[test]
    fn the_skipped_version_is_remembered_in_the_folder() {
        let folder = tempfile::tempdir().expect("folder");
        assert_eq!(skipped_version(folder.path()), None);
        remember_skipped(folder.path(), "0.3.0").expect("remembered");
        assert_eq!(skipped_version(folder.path()).as_deref(), Some("0.3.0"));
    }

    /// A key pair, a signed installer and both of them wrapped in base64 the way Tauri keeps them.
    fn signed(data: &[u8]) -> (String, String) {
        let b64 = |text: String| base64::engine::general_purpose::STANDARD.encode(text);
        let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("keys");
        let signature = minisign::sign(
            Some(&pair.pk),
            &pair.sk,
            std::io::Cursor::new(data),
            None,
            None,
        )
        .expect("signed");
        (
            b64(pair.pk.to_box().expect("pk").to_string()),
            b64(signature.to_string()),
        )
    }

    #[test]
    fn an_installer_runs_only_when_its_signature_checks_out_against_the_built_in_key() {
        let setup = b"installer bytes".to_vec();
        let (key, signature) = signed(&setup);
        assert_eq!(verify(&mut setup.as_slice(), &key, &signature), Ok(()));

        // Changed bytes, another key, and garbage all fail.
        let mut changed = setup.clone();
        changed[0] ^= 1;
        assert!(verify(&mut changed.as_slice(), &key, &signature).is_err());
        let (other_key, _) = signed(&setup);
        assert!(verify(&mut setup.as_slice(), &other_key, &signature).is_err());
        assert!(verify(&mut setup.as_slice(), "!!", &signature).is_err());
        assert!(verify(&mut setup.as_slice(), &key, "!!").is_err());
    }
}
