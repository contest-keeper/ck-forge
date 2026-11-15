//! Commands that make sticks: build an image, write it to a USB stick.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ck_admin_core::{Profile, Wallpaper, build_thin_image as build, folder_of};
use ck_protocol::LocalDateTime;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::hub::{HUB_URL, HubState};
use crate::profiles::{message, registry_of};

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
    /// What is going on: `building`, `writing` or `checking`.
    phase: &'static str,
}

#[derive(Serialize)]
pub struct Built {
    stick_id: String,
    computer_name: String,
    team: String,
    registry: String,
    /// What happened when the registry was sent to the Hub; nothing when nobody is signed in.
    hub: Option<String>,
}

/// Makes the image of one stick, from the profile at `profile_path`, and registers the stick. When
/// the admin is signed in to the Hub the stick reports to it and the registry is sent; otherwise the
/// stick has no server address and keeps its events on the stick.
fn build_stick(
    app: &AppHandle,
    base: &Path,
    out: &Path,
    profile_path: &Path,
) -> Result<Built, String> {
    let mut profile = Profile::load(profile_path).map_err(|e| message(&e))?;
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
    let now = LocalDateTime::from(chrono::Local::now().naive_local());
    let mut last = Instant::now();
    let built = build(
        base,
        out,
        &profile,
        wallpaper.as_ref(),
        &registry,
        now,
        |done, total| {
            if done == total || last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                let _ = app.emit(
                    "progress",
                    Progress {
                        done,
                        total,
                        phase: "building",
                    },
                );
            }
        },
    )
    .map_err(|e| message(&e))?;
    let hub = hub.push(&registry).map(|sent| match sent {
        Ok(text) => format!("Sent to the Hub: {text}"),
        // The stick is made either way; the next registry that is sent includes it.
        Err(why) => format!("The registry was not sent to the Hub: {why}"),
    });
    Ok(Built {
        stick_id: built.config.stick_id.to_string(),
        computer_name: built.config.computer_name(),
        team: built.config.settings.team.clone(),
        registry: registry.display().to_string(),
        hub,
    })
}

#[derive(Serialize)]
pub struct DiskView {
    number: u32,
    name: String,
    serial: String,
    size: String,
    bus: String,
    /// Why this disk is never written, or nothing when it may be.
    refused: Option<String>,
}

#[tauri::command]
pub async fn list_disks(base: PathBuf, allow_virtual: bool) -> Result<Vec<DiskView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let size = ck_writer::thin::image_len(&base).map_err(|e| e.to_string())?;
        let disks = ck_writer::list_disks().map_err(|e| e.to_string())?;
        Ok(disks
            .iter()
            .map(|disk| DiskView {
                number: disk.number,
                name: disk.friendly_name.clone().unwrap_or_default(),
                serial: disk
                    .serial_number
                    .clone()
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                size: disk.size_text(),
                bus: disk.bus_type.clone(),
                refused: disk.check(size, allow_virtual).err().map(|e| e.to_string()),
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
    total: u64,
    sha256: Option<String>,
    message: Option<String>,
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

/// Makes one image for each stick from `base` and the saved profile, so every stick has its own
/// identity, and writes them to `disks` all at the same time. Or writes the first image to the file
/// `to_file`. Raw disk access needs administrator rights, so the write runs in `ck-forge.exe`
/// started through the Windows permission prompt, and this window stays unelevated. The helper
/// reports in a file, which is read here to show progress.
///
/// Returns the SHA-256 of what was written and read back.
#[tauri::command]
pub async fn write_stick(
    app: AppHandle,
    base: PathBuf,
    profile_path: PathBuf,
    disks: Vec<u32>,
    to_file: Option<PathBuf>,
    allow_virtual: bool,
) -> Result<String, String> {
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
        let result = make_and_write(
            &app,
            &base,
            &profile_path,
            &work,
            &disks,
            to_file.as_deref(),
            allow_virtual,
        );
        let _ = std::fs::remove_dir_all(&work);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Builds one image per stick in `work`, then writes them to the disks, or the first to a file.
fn make_and_write(
    app: &AppHandle,
    base: &Path,
    profile_path: &Path,
    work: &Path,
    disks: &[u32],
    to_file: Option<&Path>,
    allow_virtual: bool,
) -> Result<String, String> {
    let mut images = Vec::new();
    for n in 1..=disks.len().max(1) {
        let image = work.join(format!("stick-{n}.img"));
        build_stick(app, base, &image, profile_path)?;
        images.push(image);
    }
    // A file needs no administrator rights, so it is written here: a test without a stick.
    if let Some(file) = to_file {
        return write_to_file(app, &images[0], file);
    }
    let exe = forge_exe()?;
    let progress_file = std::env::temp_dir().join(format!("ck-write-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&progress_file);
    let mut arguments = vec!["stick".to_owned(), "write".to_owned()];
    for image in &images {
        arguments.push(quote(image)?);
    }
    arguments.extend([
        "--yes".to_owned(),
        "--progress-file".to_owned(),
        quote(&progress_file)?,
    ]);
    for disk in disks {
        arguments.push("--disk".to_owned());
        arguments.push(disk.to_string());
    }
    if allow_virtual {
        arguments.push("--allow-virtual".to_owned());
    }
    // The program and its arguments go in environment variables, so nothing chosen in the
    // window is ever part of the PowerShell command text.
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Process -FilePath $env:CK_EXE -ArgumentList $env:CK_ARGS -Verb RunAs \
                 -WindowStyle Hidden -Wait",
        ])
        .env("CK_EXE", &exe)
        .env("CK_ARGS", arguments.join(" "))
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
                },
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let finished = read_state(&progress_file);
    let _ = std::fs::remove_file(&progress_file);
    match finished {
        Some(HelperState {
            state,
            sha256: Some(hash),
            ..
        }) if state == "done" => Ok(hash),
        Some(HelperState {
            message: Some(why), ..
        }) => Err(why),
        _ if !status.success() => {
            Err("the administrator permission was refused, so nothing was written".to_owned())
        }
        _ => Err(
            "the writer stopped without saying why; nothing on the stick can be trusted".to_owned(),
        ),
    }
}

fn write_to_file(app: &AppHandle, image: &Path, file: &Path) -> Result<String, String> {
    let mut last = Instant::now();
    ck_writer::write_image(image, file, |phase, done, total| {
        // Like the image build: no event for every block, but always one for the end of a phase.
        if done != total && last.elapsed() < Duration::from_millis(250) {
            return;
        }
        last = Instant::now();
        let phase = match phase {
            ck_writer::Phase::Writing => "writing",
            ck_writer::Phase::Verifying => "checking",
        };
        let _ = app.emit("progress", Progress { done, total, phase });
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
    fn only_a_development_build_looks_for_the_helper_in_the_build_folder() {
        let here = Path::new(r"C:\Program Files\Keeper Forge\keeper-forge.exe");
        let beside = PathBuf::from(r"C:\Program Files\Keeper Forge\ck-forge.exe");
        assert_eq!(helper_places(here, false), std::slice::from_ref(&beside));
        let development = helper_places(here, true);
        assert_eq!(development.len(), 2);
        assert_eq!(development[0], beside);
    }
}
