//! Commands that make sticks: build an image, write it to a USB stick.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ck_admin_core::{
    EnrolmentBuild, HardwareId, HubImage, HubImageStep, Profile, Registry, SignSettings, Wallpaper,
    build_enrolment_image, build_thin_image, enrolment_config_sha256, folder_of, import_image,
    local_now, mint_enrolment_code, record_config_hash, record_stick_write,
    requires_signed_settings, retire_stick_credential, sign_settings, stick_config_sha256,
    supports_enrolment,
};
use ck_protocol::{StickId, WifiPassword};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::hub::{HUB_URL, HubState};
use crate::profiles::{message, registry_of};

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
    phase: &'static str,
    /// How far the stick is checked. The checking goes on while the writing does.
    checked: u64,
}

#[tauri::command]
pub async fn import_base_image(
    app: AppHandle,
    archive: PathBuf,
    out: PathBuf,
    sha256: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = base_progress(&app);
        import_image(&archive, &out, &sha256, |done, total| {
            emit("importing", done, total);
        })
        .map_err(|error| message(&error.into()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn list_hub_images(app: AppHandle) -> Result<Vec<HubImage>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<HubState>()
            .as_admin(|session| ck_admin_core::list_hub_images(HUB_URL, session))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn download_hub_image(app: AppHandle, id: String, out: PathBuf) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let download = app
            .state::<HubState>()
            .as_admin(|session| ck_admin_core::prepare_hub_image_download(HUB_URL, session, &id))?;
        let mut emit = base_progress(&app);
        ck_admin_core::download_hub_image(download, &out, |step, done, total| {
            let phase = match step {
                HubImageStep::Download => "downloading",
                HubImageStep::Import => "importing",
            };
            emit(phase, done, total);
        })
        .map_err(|error| message(&error))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn base_progress(app: &AppHandle) -> impl FnMut(&'static str, u64, u64) + '_ {
    let mut last = Instant::now();
    let mut previous_phase = "";
    move |phase, done, total| {
        if phase != previous_phase || done == total || last.elapsed() >= Duration::from_millis(250)
        {
            last = Instant::now();
            previous_phase = phase;
            let _ = app.emit(
                "progress",
                Progress {
                    done,
                    total,
                    phase,
                    checked: 0,
                },
            );
        }
    }
}

/// Whether sticks made from this base image only accept settings the Hub signed, so the admin has
/// to be signed in to the Hub to write them.
#[tauri::command]
pub async fn signature_required(base: PathBuf) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        requires_signed_settings(&base).map_err(|e| message(&e))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn enrolment_supported(base: PathBuf) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || supports_enrolment(&base).map_err(|e| message(&e)))
        .await
        .map_err(|error| error.to_string())?
}

/// A stick Forge made: what the Hub is told about it once its write is finished.
#[derive(Clone)]
struct MadeStick {
    id: StickId,
    /// The SHA-256 of the settings file on the stick.
    config_sha256: String,
    /// The disk it goes to, for a stick that already has its key. A stick that enrols learns its
    /// disk at the Hub when it trades its code.
    disk: Option<HardwareId>,
}

/// Makes one stick image. First-boot enrolment keeps the signing key on the stick from birth.
/// `hardware` is the disk the image goes to, when it is known, so the Hub can count rewrites.
fn build_stick(
    app: &AppHandle,
    base: &Path,
    out: &Path,
    profile_path: &Path,
    wifi_password: Option<&WifiPassword>,
    first_boot_enrolment: bool,
    hardware: Option<&HardwareId>,
) -> Result<MadeStick, String> {
    let mut profile = Profile::load(profile_path).map_err(|e| message(&e))?;
    // Typed in the window for this write only: it is not in the profile file.
    profile.set_wifi_password(wifi_password.cloned());
    let hub = app.state::<HubState>();
    // The address in the profile file is not used: it is the sign-in that decides.
    profile.settings.api.url = if hub.signed_in() {
        HUB_URL.to_owned()
    } else {
        String::new()
    };
    let wallpaper = Wallpaper::read(&profile.stick_settings(), folder_of(profile_path))
        .map_err(|e| message(&e))?;
    let registry = registry_of(profile_path);
    let now = local_now();
    let mut emit = base_progress(app);
    let progress = |done, total| emit("building", done, total);
    // An image that only accepts settings the Hub signed gets them signed, by the signed-in admin.
    let signer = |bytes: &[u8]| {
        hub.as_admin(|session| sign_settings(HUB_URL, session, bytes))
            .map_err(ck_admin_core::Error::Hub)
    };
    let sign: Option<SignSettings<'_>> = hub.signed_in().then_some(&signer);
    if first_boot_enrolment {
        if !hub.signed_in() {
            return Err("sign in to the Hub to use first-boot enrolment".to_owned());
        }
        let options = EnrolmentBuild {
            base,
            out,
            profile: &profile,
            wallpaper: wallpaper.as_ref(),
            registry: &registry,
            now,
            sign,
        };
        let config = build_enrolment_image(
            &options,
            |settings, machine_number, scope| {
                hub.as_admin(|session| {
                    mint_enrolment_code(HUB_URL, session, settings, machine_number, scope, hardware)
                })
                .map(|minted| minted.code)
                .map_err(ck_admin_core::Error::Hub)
            },
            progress,
        )
        .map_err(|e| message(&e))?;
        return Ok(MadeStick {
            config_sha256: enrolment_config_sha256(&config),
            id: config.stick_id,
            disk: None,
        });
    }
    let built = build_thin_image(
        base,
        out,
        &profile,
        wallpaper.as_ref(),
        &registry,
        now,
        sign,
        progress,
    )
    .map_err(|e| message(&e))?;
    // The next successful push includes this stick if the Hub is unavailable now.
    let _ = hub.push(&registry);
    Ok(MadeStick {
        config_sha256: stick_config_sha256(&built.config),
        id: built.config.stick_id,
        disk: hardware.cloned(),
    })
}

/// Tells the Hub the settings hash of each stick written, and which disk the sticks that already
/// had their key were written to. This runs only after the writer reports that everything was
/// written and checked, so a cancelled permission prompt or a failed disk never counts. Not telling
/// the Hub never changes what was written: that stick is then only not checked, or not counted as
/// rewritten.
fn record_written(app: &AppHandle, written: &[MadeStick]) {
    let hub = app.state::<HubState>();
    for stick in written {
        let _ = hub.as_admin(|session| {
            record_config_hash(HUB_URL, session, &stick.id, &stick.config_sha256)
        });
        if let Some(disk) = &stick.disk {
            let _ = hub.as_admin(|session| record_stick_write(HUB_URL, session, &stick.id, disk));
        }
    }
}

#[derive(Serialize)]
pub struct DiskView {
    number: u32,
    name: String,
    serial: String,
    /// The start of the disk's fingerprint, the same id the Hub shows for a stick written to it.
    hardware_id: Option<String>,
    /// `Disk::identity_token` as listed. The window sends it back with the write, and the helper
    /// erases only a disk that still has it.
    token: String,
    size: String,
    bus: String,
    /// Why this disk is never written, or nothing when it may be.
    refused: Option<String>,
    /// The disk is smaller than the image, so the window can say so and not just "protected".
    too_small: bool,
}

#[tauri::command]
pub async fn list_disks(base: PathBuf, allow_virtual: bool) -> Result<Vec<DiskView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let size = ck_writer::thin::image_len(&base).map_err(|e| e.to_string())?;
        let disks = ck_writer::list_disks().map_err(|e| e.to_string())?;
        Ok(disks
            .iter()
            .map(|disk| {
                let refusal = disk.check(size, allow_virtual).err();
                DiskView {
                    number: disk.number,
                    name: disk.friendly_name.clone().unwrap_or_default(),
                    serial: disk
                        .serial_number
                        .clone()
                        .unwrap_or_default()
                        .trim()
                        .to_owned(),
                    hardware_id: HardwareId::of(disk)
                        .map(|id| id.fingerprint.chars().take(8).collect()),
                    token: disk.identity_token(),
                    size: disk.size_text(),
                    bus: disk.bus_type.clone(),
                    refused: refusal.as_ref().map(ToString::to_string),
                    too_small: matches!(refusal, Some(ck_writer::Error::TooSmall { .. })),
                }
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Where `ck-forge.exe` may be: next to this program, and for a development build also the
/// repository's build folder. That folder is a path of the machine the program was built on, so a
/// released program must never start something from it with administrator rights.
fn helper_places(here: &Path, development: bool) -> Vec<PathBuf> {
    let mut places = vec![here.with_file_name("ck-forge.exe")];
    if development {
        places.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/ck-forge.exe"));
    }
    places
}

fn forge_exe() -> Result<PathBuf, String> {
    let here = std::env::current_exe().map_err(|e| e.to_string())?;
    helper_places(&here, cfg!(debug_assertions))
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "ck-forge.exe was not found next to this program".to_owned())
}

#[derive(Deserialize)]
struct HelperState {
    state: String,
    #[serde(default)]
    done: u64,
    #[serde(default)]
    checked: u64,
    #[serde(default)]
    total: u64,
    sha256: Option<String>,
    message: Option<String>,
    /// Which sticks (by position) the helper could not write. Empty when it stopped before or
    /// without saying, so none can be trusted.
    #[serde(default)]
    failed: Vec<usize>,
}

fn read_state(file: &Path) -> Option<HelperState> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// Puts a path in quotes for the argument string of `Start-Process`. The helper runs with
/// administrator rights, so a path that could break out of its quotes is refused.
fn quote(path: &Path) -> Result<String, String> {
    let text = path.display().to_string();
    if text.contains('"') || text.ends_with('\\') {
        return Err(format!("{text} cannot be used as a file name here"));
    }
    Ok(format!("\"{text}\""))
}

/// The token the admin confirmed for each disk, in order of `disks`. A disk without a valid one
/// is refused, so nothing is built or erased for it.
fn confirmed_tokens<'a>(
    disks: &[u32],
    confirmed: &'a [String],
) -> Result<Vec<(u32, &'a str)>, String> {
    disks
        .iter()
        .map(|&disk| {
            let prefix = format!("{disk}=");
            confirmed
                .iter()
                .find_map(|pair| pair.strip_prefix(&prefix))
                .filter(|token| !token.is_empty() && token.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(|token| (disk, token))
                .ok_or_else(|| format!("disk {disk} was not confirmed; list the disks again"))
        })
        .collect()
}

/// The argument string of the elevated `ck-forge stick write`. Every disk must come with the identity
/// token the admin confirmed, so the helper refuses a stick that took its number in the meantime;
/// without one nothing is started.
fn helper_arguments(
    images: &[PathBuf],
    progress_file: &Path,
    disks: &[u32],
    confirmed: &[String],
    allow_virtual: bool,
) -> Result<String, String> {
    let tokens = confirmed_tokens(disks, confirmed)?;
    let mut arguments = vec!["stick".to_owned(), "write".to_owned()];
    for image in images {
        arguments.push(quote(image)?);
    }
    arguments.extend([
        "--yes".to_owned(),
        "--progress-file".to_owned(),
        quote(progress_file)?,
    ]);
    for (disk, token) in tokens {
        arguments.extend(["--disk".to_owned(), disk.to_string()]);
        arguments.extend(["--confirmed".to_owned(), format!("{disk}={token}")]);
    }
    if allow_virtual {
        arguments.push("--allow-virtual".to_owned());
    }
    Ok(arguments.join(" "))
}

/// Makes one image for each stick from `base` and the saved profile, so every stick has its own
/// identity, and writes them to `disks` all at the same time. Or writes the first image to the file
/// `to_file`. Raw disk access needs administrator rights, so the write runs in `ck-forge.exe`
/// started through the Windows permission prompt, and this window stays unelevated. The helper
/// reports in a file, which is read here to show progress.
///
/// Returns the SHA-256 of what was written and read back.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn write_stick(
    app: AppHandle,
    base: PathBuf,
    profile_path: PathBuf,
    disks: Vec<u32>,
    to_file: Option<PathBuf>,
    confirmed: Vec<String>,
    allow_virtual: bool,
    wifi_password: Option<String>,
    first_boot_enrolment: Option<bool>,
) -> Result<String, String> {
    let wifi_password = wifi_password
        .filter(|text| !text.is_empty())
        .map(WifiPassword::new);
    tauri::async_runtime::spawn_blocking(move || {
        if to_file.is_none() && disks.is_empty() {
            return Err("choose a disk or a file to write to".to_owned());
        }
        // Every stick gets an image of its own, so every stick has its own identity. The images
        // are small (only the start of the base is copied) and are removed when the write ends.
        let work = std::env::temp_dir().join(format!("ck-sticks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work)
            .map_err(|e| format!("could not make {}: {e}", work.display()))?;
        let target = Target {
            disks: &disks,
            confirmed: &confirmed,
            to_file: to_file.as_deref(),
            allow_virtual,
            wifi_password: wifi_password.as_ref(),
            first_boot_enrolment: first_boot_enrolment.unwrap_or(false),
        };
        let result = make_and_write(&app, &base, &profile_path, &work, &target);
        let _ = std::fs::remove_dir_all(&work);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Where the sticks go and what is typed for this write only.
struct Target<'a> {
    disks: &'a [u32],
    /// `NUMBER=TOKEN` of each disk as the admin saw it when confirming.
    confirmed: &'a [String],
    to_file: Option<&'a Path>,
    allow_virtual: bool,
    wifi_password: Option<&'a WifiPassword>,
    first_boot_enrolment: bool,
}

/// The id of each disk that is about to be written, in the order of `disks`. A disk whose id cannot
/// be read, or all of them when the list cannot be read, give none: the stick is written as before.
fn hardware_of(disks: &[u32], to_file: bool) -> Vec<Option<HardwareId>> {
    if to_file || disks.is_empty() {
        return Vec::new();
    }
    let Ok(listed) = ck_writer::list_disks() else {
        return Vec::new();
    };
    disks
        .iter()
        .map(|number| {
            listed
                .iter()
                .find(|disk| disk.number == *number)
                .and_then(HardwareId::of)
        })
        .collect()
}

/// The images made for one write, and the sticks they hold. The Hub is told about those sticks once
/// the write has finished.
struct Made {
    images: Vec<PathBuf>,
    written: Vec<MadeStick>,
}

/// Builds one image per stick in `work`.
fn build_images(
    app: &AppHandle,
    base: &Path,
    profile_path: &Path,
    work: &Path,
    target: &Target<'_>,
) -> Result<Made, String> {
    let hardware = hardware_of(target.disks, target.to_file.is_some());
    let mut made = Made {
        images: Vec::new(),
        written: Vec::new(),
    };
    for n in 1..=target.disks.len().max(1) {
        let image = work.join(format!("stick-{n}.img"));
        let disk = hardware.get(n - 1).and_then(Option::as_ref);
        let stick = build_stick(
            app,
            base,
            &image,
            profile_path,
            target.wifi_password,
            target.first_boot_enrolment,
            disk,
        )?;
        made.written.push(stick);
        made.images.push(image);
    }
    Ok(made)
}

/// Builds one image per stick in `work`, then writes them to the disks, or the first to a file.
fn make_and_write(
    app: &AppHandle,
    base: &Path,
    profile_path: &Path,
    work: &Path,
    target: &Target<'_>,
) -> Result<String, String> {
    let Target {
        disks,
        confirmed,
        to_file,
        allow_virtual,
        ..
    } = *target;
    // Before any stick is built: a refusal here must not leave keys in the registry or on the Hub.
    if to_file.is_none() {
        confirmed_tokens(disks, confirmed)?;
    }
    let Made { images, written } = build_images(app, base, profile_path, work, target)?;
    // A file needs no administrator rights, so it is written here: a test without a stick.
    if let Some(file) = to_file {
        return write_to_file(app, &images[0], file);
    }
    let exe = forge_exe()?;
    let progress_file = std::env::temp_dir().join(format!("ck-write-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&progress_file);
    let arguments = helper_arguments(&images, &progress_file, disks, confirmed, allow_virtual)?;
    // The program and its arguments go in environment variables, so nothing chosen in the
    // window is ever part of the PowerShell command text.
    let mut powershell = Command::new("powershell.exe");
    powershell
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Process -FilePath $env:CK_EXE -ArgumentList $env:CK_ARGS -Verb RunAs \
                 -WindowStyle Hidden -Wait",
        ])
        .env("CK_EXE", &exe)
        .env("CK_ARGS", arguments);
    // The elevated helper starts hidden, but PowerShell itself would still show a console.
    let mut child = ck_writer::without_console_window(&mut powershell)
        .spawn()
        .map_err(|e| format!("could not start the writer: {e}"))?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if let Some(state) = read_state(&progress_file)
            && state.total > 0
        {
            let phase = if state.state == "checking" {
                "checking"
            } else {
                "writing"
            };
            let _ = app.emit(
                "progress",
                Progress {
                    done: state.done,
                    total: state.total,
                    phase,
                    checked: state.checked,
                },
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let finished = read_state(&progress_file);
    let _ = std::fs::remove_file(&progress_file);
    let failed = finished
        .as_ref()
        .map_or_else(Vec::new, |state| state.failed.clone());
    match finished {
        Some(HelperState {
            state,
            sha256: Some(hash),
            ..
        }) if state == "done" => {
            record_written(app, &written);
            Ok(hash)
        }
        other => {
            let why = match other {
                Some(HelperState {
                    message: Some(why), ..
                }) => why,
                _ if !status.success() => {
                    "the administrator permission was refused, so nothing was written".to_owned()
                }
                _ => "the writer stopped without saying why; nothing on the stick can be trusted"
                    .to_owned(),
            };
            Err(settle_failure(app, profile_path, &written, &failed, why))
        }
    }
}

/// What follows a write that did not finish: the sticks that were written stay and are recorded, the
/// others leave the registry file and the Hub. Returns the reason, with what could not be cleaned up.
fn settle_failure(
    app: &AppHandle,
    profile_path: &Path,
    written: &[MadeStick],
    failed: &[usize],
    why: String,
) -> String {
    let (kept, unwritten) = split_written(written, failed);
    let kept: Vec<MadeStick> = kept.into_iter().cloned().collect();
    record_written(app, &kept);
    let hub = app.state::<HubState>();
    let registry = registry_of(profile_path);
    let problems = clean_up_unwritten(
        &unwritten,
        |id| {
            Registry::remove(&registry, id)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
        |id| {
            if !hub.signed_in() {
                return Ok(());
            }
            hub.as_admin(
                |session| match retire_stick_credential(HUB_URL, session, id.as_str()) {
                    Err(ck_admin_core::Error::NoSuchStick) => Ok(()),
                    other => other,
                },
            )
        },
    );
    if problems.is_empty() {
        why
    } else {
        format!("{why}. {}", problems.join(". "))
    }
}

/// The sticks whose write the helper confirmed, and those that must not stay: when it names no
/// failed stick it stopped as a whole (refused, cancelled, died), so none can be trusted.
fn split_written<'a>(
    made: &'a [MadeStick],
    failed: &[usize],
) -> (Vec<&'a MadeStick>, Vec<&'a MadeStick>) {
    if failed.is_empty() {
        return (Vec::new(), made.iter().collect());
    }
    let mut parts = (Vec::new(), Vec::new());
    for (index, stick) in made.iter().enumerate() {
        if failed.contains(&index) {
            parts.1.push(stick);
        } else {
            parts.0.push(stick);
        }
    }
    parts
}

/// The registry file is written when an image is built, and every push sends the whole file; the Hub
/// is told at the same time. A stick that was not written does not exist, so it leaves both: it must
/// never reach the Hub later as a live stick. A failure does not stop the others; each is named.
fn clean_up_unwritten(
    unwritten: &[&MadeStick],
    mut remove: impl FnMut(&StickId) -> Result<(), String>,
    mut retire: impl FnMut(&StickId) -> Result<(), String>,
) -> Vec<String> {
    let mut not_removed = Vec::new();
    let mut not_retired = Vec::new();
    for stick in unwritten {
        if remove(&stick.id).is_err() {
            not_removed.push(stick.id.to_string());
        }
        if retire(&stick.id).is_err() {
            not_retired.push(stick.id.to_string());
        }
    }
    let mut problems = Vec::new();
    if !not_removed.is_empty() {
        problems.push(format!(
            "these sticks could not be taken out of the registry file, so delete them from it before the next push: {}",
            not_removed.join(", ")
        ));
    }
    if !not_retired.is_empty() {
        problems.push(format!(
            "these sticks could not be retired on the Hub, so retire them there: {}",
            not_retired.join(", ")
        ));
    }
    problems
}

fn write_to_file(app: &AppHandle, image: &Path, file: &Path) -> Result<String, String> {
    let mut last = Instant::now();
    // How far the file is written and how far it is checked, which go on together.
    let (mut written, mut checked) = (0_u64, 0_u64);
    ck_writer::write_image(image, file, |phase, done, total| {
        match phase {
            ck_writer::Phase::Writing => written = done,
            ck_writer::Phase::Verifying => checked = done,
        }
        // Like the image build: no event for every block, but always one for the end of a phase.
        if done != total && last.elapsed() < Duration::from_millis(250) {
            return;
        }
        last = Instant::now();
        let phase = if written < total {
            "writing"
        } else {
            "checking"
        };
        let _ = app.emit(
            "progress",
            Progress {
                done: written,
                total,
                phase,
                checked,
            },
        );
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_cannot_add_arguments_to_the_elevated_helper() {
        assert_eq!(
            quote(Path::new(r"C:\My Images\stick.img")).as_deref(),
            Ok(r#""C:\My Images\stick.img""#)
        );
        // A quote would end the argument early and start new ones, such as `--to <any file>`.
        assert!(quote(Path::new(r#"x.img" --to "C:\Windows\win.ini"#)).is_err());
        // A backslash before the closing quote would make the quote part of the argument.
        assert!(quote(Path::new(r"C:\folder\")).is_err());
    }

    #[test]
    fn a_disk_is_written_only_with_the_token_the_admin_confirmed() {
        let image = [PathBuf::from(r"C:\w\stick-1.img")];
        let progress = Path::new(r"C:\w\p.json");
        let ok = helper_arguments(&image, progress, &[3], &["3=ab12".to_owned()], false).unwrap();
        assert!(ok.contains("--disk 3 --confirmed 3=ab12"), "{ok}");
        for bad in [vec![], vec!["4=ab12".to_owned()], vec!["3=zz".to_owned()]] {
            assert!(helper_arguments(&image, progress, &[3], &bad, false).is_err());
        }
    }

    #[test]
    fn a_failed_write_keeps_only_the_sticks_the_helper_confirmed() {
        let stick = |id: &str| MadeStick {
            id: StickId::parse(id).unwrap(),
            config_sha256: String::new(),
            disk: None,
        };
        let made = [stick("stick-a"), stick("stick-b"), stick("stick-c")];
        // Stopped as a whole: refused, cancelled or died. Nothing can be trusted.
        let (kept, gone) = split_written(&made, &[]);
        assert!(kept.is_empty() && gone.len() == 3);
        // One stick failed: the others were written and checked.
        let (kept, gone) = split_written(&made, &[1]);
        assert_eq!(kept.len(), 2);
        assert_eq!(gone[0].id.as_str(), "stick-b");

        // Every unwritten stick leaves the registry and the Hub; one failing does not stop the rest.
        let (mut removed, mut retired) = (Vec::new(), Vec::new());
        let problems = clean_up_unwritten(
            &gone,
            |id| {
                removed.push(id.to_string());
                Ok(())
            },
            |id| {
                retired.push(id.to_string());
                Err("down".to_owned())
            },
        );
        assert_eq!(
            (removed, retired),
            (vec!["stick-b".to_owned()], vec!["stick-b".to_owned()])
        );
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("stick-b"));
    }

    #[test]
    fn only_a_development_build_looks_for_the_helper_in_the_build_folder() {
        let here = Path::new(r"C:\Program Files\Keeper Forge\keeper-forge.exe");
        let beside = PathBuf::from(r"C:\Program Files\Keeper Forge\ck-forge.exe");
        assert_eq!(helper_places(here, false), std::slice::from_ref(&beside));
        let development = helper_places(here, true);
        assert_eq!(development.len(), 2);
        assert_eq!(development[0], beside);
    }
}
