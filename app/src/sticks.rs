//! Commands that make sticks: build an image, write it to a USB stick, tell the Hub.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ck_admin_core::{Profile, Wallpaper, build_image as build};
use ck_protocol::LocalDateTime;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::profiles::{folder_of, message, registry_of};

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
}

/// Copies the base image, gives the copy its identity and the profile's settings, and registers it.
#[tauri::command]
pub async fn build_image(
    app: AppHandle,
    base: PathBuf,
    out: PathBuf,
    profile_path: PathBuf,
) -> Result<Built, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = Profile::load(&profile_path).map_err(|e| message(&e))?;
        let wallpaper = Wallpaper::read(&profile.stick_settings(), &folder_of(&profile_path))
            .map_err(|e| message(&e))?;
        let registry = registry_of(&profile_path);
        let now = LocalDateTime::from(chrono::Local::now().naive_local());
        let mut last = Instant::now();
        let built = build(
            &base,
            &out,
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
        Ok(Built {
            stick_id: built.config.stick_id.to_string(),
            computer_name: built.config.computer_name(),
            team: built.config.settings.team.clone(),
            registry: registry.display().to_string(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
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

/// The disks of this PC, each with the reason it is refused, if it is.
#[tauri::command]
pub async fn list_disks(image: PathBuf, allow_virtual: bool) -> Result<Vec<DiskView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let size = std::fs::metadata(&image)
            .map_err(|e| format!("cannot read {}: {e}", image.display()))?
            .len();
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

/// Where `ck-forge.exe` is: next to this program once installed, or in the repository's build folder
/// while developing.
fn forge_exe() -> Result<PathBuf, String> {
    let here = std::env::current_exe().map_err(|e| e.to_string())?;
    let beside = here.with_file_name("ck-forge.exe");
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/ck-forge.exe");
    [beside, dev]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "ck-forge.exe was not found next to this program".to_owned())
}

/// What the helper left in its progress file.
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

/// Puts a path in quotes for the argument string of `Start-Process`.
fn quote(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// Writes `image` to disk `disk`, or to the file `to_file`. Raw disk access needs administrator rights, so the write runs in
/// `ck-forge.exe` started through the Windows permission prompt, and this window stays unelevated.
/// The helper reports in a file, which is read here to show progress.
///
/// Returns the SHA-256 of what was written and read back.
#[tauri::command]
pub async fn write_stick(
    app: AppHandle,
    image: PathBuf,
    disk: Option<u32>,
    to_file: Option<PathBuf>,
    allow_virtual: bool,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // A file needs no administrator rights, so it is written here: a test without a stick.
        if let Some(file) = to_file {
            return write_to_file(&app, &image, &file);
        }
        let disk = disk.ok_or("choose a disk or a file to write to")?;
        let exe = forge_exe()?;
        let progress_file =
            std::env::temp_dir().join(format!("ck-write-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&progress_file);
        let mut arguments = vec![
            "stick".to_owned(),
            "write".to_owned(),
            quote(&image),
            "--disk".to_owned(),
            disk.to_string(),
            "--yes".to_owned(),
            "--progress-file".to_owned(),
            quote(&progress_file),
        ];
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
                "the writer stopped without saying why; nothing on the stick can be trusted"
                    .to_owned(),
            ),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn write_to_file(app: &AppHandle, image: &Path, file: &Path) -> Result<String, String> {
    ck_writer::write_image(image, file, |phase, done, total| {
        let phase = match phase {
            ck_writer::Phase::Writing => "writing",
            ck_writer::Phase::Verifying => "checking",
        };
        let _ = app.emit("progress", Progress { done, total, phase });
    })
    .map_err(|e| e.to_string())
}

/// Sends the registry to the Hub. The password is used for one login and then dropped.
#[tauri::command]
pub async fn push_registry(
    hub: String,
    user: String,
    password: String,
    profile_path: PathBuf,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let registry = registry_of(&profile_path);
        let pushed = ck_admin_core::push_registry(&hub, &user, &password, &registry)
            .map_err(|e| message(&e))?;
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
        Ok(text)
    })
    .await
    .map_err(|e| e.to_string())?
}
