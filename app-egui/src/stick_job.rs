//! What writing sticks does behind the window: one image for each stick, made here, then written
//! by `ck-forge stick write` started with administrator rights, which reports in a JSON file.
//!
//! The window itself never runs elevated. The helper is told only paths and disk numbers: the
//! Wi-Fi password is inside the images, never on its command line or in a file of its own.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use ck_admin_core::{
    EnrolmentBuild, HUB_URL, HardwareId, HubSession, Profile, SignSettings, Wallpaper,
    build_enrolment_image, build_thin_image, enrolment_config_sha256, folder_of, local_now,
    mint_enrolment_code, push_registry_signed_in, record_config_hash, record_stick_write,
    retire_stick_credential, sign_settings, stick_config_sha256, vault,
};
use ck_protocol::{StickId, WifiPassword};
use serde::Deserialize;

/// What the job tells the window.
pub enum Event {
    /// How far the sticks are written and checked.
    Progress(Progress),
    /// The job ended: the SHA-256 of what was written and read back, or what went wrong.
    Done(Result<String, String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// `building`, `writing` or `checking`.
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    /// How far the stick is checked. The checking goes on while the writing does.
    pub checked: u64,
}

/// Where the sticks go and what is typed for this write only.
pub struct Request {
    pub base: PathBuf,
    pub profile: PathBuf,
    /// The disks to erase and write. Empty when writing to a file.
    pub disks: Vec<u32>,
    /// For each disk the admin confirmed: its number and `Disk::identity_token` as it was listed.
    /// The helper refuses a disk that has another token by then.
    pub confirmed: Vec<(u32, String)>,
    /// A file to write the first image to: a test without a stick, which needs no rights.
    pub to_file: Option<PathBuf>,
    pub allow_virtual: bool,
    /// Typed for this write: it is not in the profile file.
    pub wifi_password: Option<WifiPassword>,
    pub first_boot_enrolment: bool,
}

/// A stick the job made: what the Hub is told about it once its write is finished.
struct MadeStick {
    id: StickId,
    /// The SHA-256 of the settings file on the stick.
    config_sha256: String,
    /// The disk it goes to, for a stick that already has its key.
    disk: Option<HardwareId>,
}

/// The registry file that goes with a profile file.
fn registry_of(profile: &Path) -> PathBuf {
    folder_of(profile).join("ck-registry.json")
}

/// The id of each disk that is about to be written, in the order of `disks`. A disk whose id cannot
/// be read, or all of them when the list cannot be read, give none.
fn hardware_of(disks: &[u32]) -> Vec<Option<HardwareId>> {
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

/// Runs the whole write on the calling thread and sends its progress and its end. `cancel` is set
/// by the window when the admin cancels.
pub fn run(request: &Request, cancel: &AtomicBool, events: &Sender<Event>, repaint: &dyn Fn()) {
    let result = write_all(request, cancel, &|progress| {
        // The window may be gone by now; then nobody needs the answer.
        let _ = events.send(Event::Progress(progress));
        repaint();
    });
    let _ = events.send(Event::Done(result));
    repaint();
}

fn write_all(
    request: &Request,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<String, String> {
    if request.to_file.is_none() && request.disks.is_empty() {
        return Err("choose a disk or a file to write to".to_owned());
    }
    let session = vault::load();
    // Every stick gets an image of its own, so every stick has its own identity. The images are
    // small (only the start of the base is copied) and are removed when the write ends.
    let work = std::env::temp_dir().join(format!("ck-sticks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)
        .map_err(|e| format!("could not make {}: {e}", work.display()))?;
    let mut made = Vec::new();
    let result = make_and_write(request, session.as_ref(), cancel, &work, report, &mut made);
    let _ = std::fs::remove_dir_all(&work);
    // The Hub learns of a stick when its image is built, before anything is written. A stick whose
    // write was cancelled or failed does not exist, so it must not stay on the Hub as a live stick.
    if result.is_err()
        && let Some(session) = session.as_ref()
    {
        retire_unwritten(&made, |id| {
            let _ = retire_stick_credential(HUB_URL, session, id);
        });
    }
    result
}

/// Calls `retire` with the id of each stick that was registered but not written.
fn retire_unwritten(made: &[MadeStick], mut retire: impl FnMut(&str)) {
    for stick in made {
        retire(stick.id.as_str());
    }
}

/// `made` gets each stick as soon as its image is built, so the caller can retire them if this fails.
fn make_and_write(
    request: &Request,
    session: Option<&HubSession>,
    cancel: &AtomicBool,
    work: &Path,
    report: &dyn Fn(Progress),
    made: &mut Vec<MadeStick>,
) -> Result<String, String> {
    let images = build_images(request, session, cancel, work, report, made)?;
    // A cancel during the last image must not start the write either.
    if cancel.load(Ordering::Relaxed) {
        return Err(CANCELLED_BEFORE_WRITING.to_owned());
    }
    // A file needs no administrator rights, so it is written here: a test without a stick.
    if let Some(file) = &request.to_file {
        return write_to_file(&images[0], file, cancel, report);
    }
    let hash = write_elevated(&images, request, cancel, report)?;
    // Only now that everything was written and checked, so a refused permission prompt or a failed
    // disk never counts.
    if let Some(session) = session {
        record_written(session, made);
    }
    Ok(hash)
}

/// Builds one image per stick in `work`.
fn build_images(
    request: &Request,
    session: Option<&HubSession>,
    cancel: &AtomicBool,
    work: &Path,
    report: &dyn Fn(Progress),
    made: &mut Vec<MadeStick>,
) -> Result<Vec<PathBuf>, String> {
    let hardware = if request.to_file.is_some() {
        Vec::new()
    } else {
        hardware_of(&request.disks)
    };
    let mut images = Vec::new();
    for n in 1..=request.disks.len().max(1) {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED_BEFORE_WRITING.to_owned());
        }
        let image = work.join(format!("stick-{n}.img"));
        let disk = hardware.get(n - 1).and_then(Option::as_ref);
        made.push(build_stick(request, session, &image, disk, report)?);
        images.push(image);
    }
    Ok(images)
}

const CANCELLED_BEFORE_WRITING: &str = "cancelled: nothing was written";

/// Makes one stick image. First-boot enrolment keeps the signing key on the stick from birth.
/// `hardware` is the disk the image goes to, when it is known, so the Hub can count rewrites.
fn build_stick(
    request: &Request,
    session: Option<&HubSession>,
    out: &Path,
    hardware: Option<&HardwareId>,
    report: &dyn Fn(Progress),
) -> Result<MadeStick, String> {
    let mut profile = Profile::load(&request.profile).map_err(|e| e.to_string())?;
    profile.set_wifi_password(request.wifi_password.clone());
    // The address in the profile file is not used: it is the sign-in that decides.
    profile.settings.api.url = if session.is_some() {
        HUB_URL.to_owned()
    } else {
        String::new()
    };
    let wallpaper = Wallpaper::read(&profile.stick_settings(), folder_of(&request.profile))
        .map_err(|e| e.to_string())?;
    let registry = registry_of(&request.profile);
    let now = local_now();
    let mut last = Instant::now();
    let progress = |done, total| {
        // Four updates a second at most, but the end is always sent.
        if done == total || last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            report(Progress {
                phase: "building",
                done,
                total,
                checked: 0,
            });
        }
    };
    // An image that only accepts settings the Hub signed gets them signed, by the signed-in admin.
    let signer =
        |bytes: &[u8]| session.map_or(Ok(String::new()), |s| sign_settings(HUB_URL, s, bytes));
    let sign: Option<SignSettings<'_>> = session.map(|_| &signer as SignSettings<'_>);
    if request.first_boot_enrolment {
        let Some(session) = session else {
            return Err("sign in to the Hub to use first-boot enrolment".to_owned());
        };
        let options = EnrolmentBuild {
            base: &request.base,
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
                mint_enrolment_code(HUB_URL, session, settings, machine_number, scope, hardware)
                    .map(|minted| minted.code)
            },
            progress,
        )
        .map_err(|e| e.to_string())?;
        return Ok(MadeStick {
            config_sha256: enrolment_config_sha256(&config),
            id: config.stick_id,
            disk: None,
        });
    }
    let built = build_thin_image(
        &request.base,
        out,
        &profile,
        wallpaper.as_ref(),
        &registry,
        now,
        sign,
        progress,
    )
    .map_err(|e| e.to_string())?;
    // The next successful push includes this stick if the Hub is unavailable now.
    if let Some(session) = session {
        let _ = push_registry_signed_in(HUB_URL, session, &registry);
    }
    Ok(MadeStick {
        config_sha256: stick_config_sha256(&built.config),
        id: built.config.stick_id,
        disk: hardware.cloned(),
    })
}

/// Tells the Hub the settings hash of each stick written, and which disk the sticks that already
/// had their key were written to. Not telling the Hub never changes what was written: that stick
/// is then only not checked, or not counted as rewritten.
fn record_written(session: &HubSession, written: &[MadeStick]) {
    for stick in written {
        let _ = record_config_hash(HUB_URL, session, &stick.id, &stick.config_sha256);
        if let Some(disk) = &stick.disk {
            let _ = record_stick_write(HUB_URL, session, &stick.id, disk);
        }
    }
}

fn write_to_file(
    image: &Path,
    file: &Path,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<String, String> {
    let mut last = Instant::now();
    // How far the file is written and how far it is checked, which go on together.
    let (mut written, mut checked) = (0_u64, 0_u64);
    let cancelled = || cancel.load(Ordering::Relaxed);
    ck_writer::write_image_cancellable(image, file, &cancelled, |phase, done, total| {
        match phase {
            ck_writer::Phase::Writing => written = done,
            ck_writer::Phase::Verifying => checked = done,
        }
        if done != total && last.elapsed() < Duration::from_millis(250) {
            return;
        }
        last = Instant::now();
        report(Progress {
            phase: if written < total {
                "writing"
            } else {
                "checking"
            },
            done: written,
            total,
            checked,
        });
    })
    .map_err(|e| match e {
        ck_writer::Error::Cancelled => "cancelled: the file is only partly written".to_owned(),
        other => other.to_string(),
    })
}

/// What the helper writes in its progress file.
#[derive(Debug, Deserialize)]
pub struct HelperState {
    state: String,
    #[serde(default)]
    done: u64,
    #[serde(default)]
    checked: u64,
    #[serde(default)]
    total: u64,
    sha256: Option<String>,
    message: Option<String>,
}

fn read_state(file: &Path) -> Option<HelperState> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// The progress the helper reported, when it has started to write.
fn progress_of(state: &HelperState) -> Option<Progress> {
    (state.total > 0).then_some(Progress {
        phase: if state.state == "checking" {
            "checking"
        } else {
            "writing"
        },
        done: state.done,
        total: state.total,
        checked: state.checked,
    })
}

/// What the end of the helper means: the hash when everything was written and checked. Anything
/// else is a failure, and a stick whose write did not finish must never be used.
fn outcome(finished: Option<HelperState>, permission_given: bool) -> Result<String, String> {
    match finished {
        Some(HelperState {
            state,
            sha256: Some(hash),
            ..
        }) if state == "done" => Ok(hash),
        Some(HelperState { state, .. }) if state == "cancelled" => {
            Err("cancelled: the stick is only partly written and must be written again".to_owned())
        }
        Some(HelperState {
            message: Some(why), ..
        }) => Err(why),
        _ if !permission_given => {
            Err("the administrator permission was refused, so nothing was written".to_owned())
        }
        _ => Err(
            "the writer stopped without saying why; nothing on the stick can be trusted".to_owned(),
        ),
    }
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

/// The arguments of `ck-forge stick write`. Only paths and disk numbers: the images hold the
/// settings, so the Wi-Fi password is never part of the command line.
fn elevated_arguments(
    images: &[PathBuf],
    disks: &[u32],
    confirmed: &[(u32, String)],
    allow_virtual: bool,
    progress_file: &Path,
    cancel_file: &Path,
) -> Result<Vec<String>, String> {
    let mut arguments = vec!["stick".to_owned(), "write".to_owned()];
    for image in images {
        arguments.push(quote(image)?);
    }
    arguments.extend([
        "--yes".to_owned(),
        "--progress-file".to_owned(),
        quote(progress_file)?,
        "--cancel-file".to_owned(),
        quote(cancel_file)?,
    ]);
    for disk in disks {
        arguments.push("--disk".to_owned());
        arguments.push(disk.to_string());
    }
    // Hex only, so a token cannot add anything to the command line.
    for (number, token) in confirmed {
        arguments.push("--confirmed".to_owned());
        arguments.push(format!("{number}={token}"));
    }
    if allow_virtual {
        arguments.push("--allow-virtual".to_owned());
    }
    Ok(arguments)
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

/// Writes `images` to the disks with `ck-forge stick write`, started through the Windows
/// permission prompt, and follows its progress file. When `cancel` is set the cancel file is
/// created, which the helper sees between blocks.
fn write_elevated(
    images: &[PathBuf],
    request: &Request,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<String, String> {
    let exe = forge_exe()?;
    let pid = std::process::id();
    let progress_file = std::env::temp_dir().join(format!("ck-write-{pid}.json"));
    let cancel_file = std::env::temp_dir().join(format!("ck-write-{pid}.cancel"));
    let _ = std::fs::remove_file(&progress_file);
    let _ = std::fs::remove_file(&cancel_file);
    let arguments = elevated_arguments(
        images,
        &request.disks,
        &request.confirmed,
        request.allow_virtual,
        &progress_file,
        &cancel_file,
    )?;
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
        .env("CK_ARGS", arguments.join(" "));
    // The elevated helper starts hidden, but PowerShell itself would still show a console.
    let mut child = ck_writer::without_console_window(&mut powershell)
        .spawn()
        .map_err(|e| format!("could not start the writer: {e}"))?;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = std::fs::write(&cancel_file, b"cancel");
        }
        if let Some(progress) = read_state(&progress_file).as_ref().and_then(progress_of) {
            report(progress);
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let finished = read_state(&progress_file);
    let _ = std::fs::remove_file(&progress_file);
    let _ = std::fs::remove_file(&cancel_file);
    outcome(finished, status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_stick_is_retired_when_the_write_does_not_finish() {
        let made: Vec<MadeStick> = ["stick-a", "stick-b"]
            .iter()
            .map(|name| MadeStick {
                id: StickId::parse(name).expect("id"),
                config_sha256: String::new(),
                disk: None,
            })
            .collect();
        let mut retired = Vec::new();
        retire_unwritten(&made, |id| retired.push(id.to_owned()));
        assert_eq!(retired, ["stick-a", "stick-b"]);
    }

    fn state(text: &str) -> Option<HelperState> {
        serde_json::from_str(text).ok()
    }

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
    fn the_elevated_command_line_holds_paths_and_disk_numbers_and_never_the_wifi_password() {
        let secret = "correct horse battery";
        let request = Request {
            base: PathBuf::from("base.img"),
            profile: PathBuf::from("p.json"),
            disks: vec![3, 4],
            confirmed: vec![(3, "ab12".to_owned()), (4, "cd34".to_owned())],
            to_file: None,
            allow_virtual: true,
            wifi_password: Some(WifiPassword::new(secret)),
            first_boot_enrolment: false,
        };
        let arguments = elevated_arguments(
            &[PathBuf::from("a.img"), PathBuf::from("b.img")],
            &request.disks,
            &request.confirmed,
            request.allow_virtual,
            Path::new("progress.json"),
            Path::new("stop"),
        )
        .expect("arguments");
        let line = arguments.join(" ");
        assert!(!line.contains(secret));
        assert_eq!(
            line,
            r#"stick write "a.img" "b.img" --yes --progress-file "progress.json" --cancel-file "stop" --disk 3 --disk 4 --confirmed 3=ab12 --confirmed 4=cd34 --allow-virtual"#
        );
        // The password type hides itself even when something prints the request's parts.
        assert!(!format!("{:?}", request.wifi_password).contains(secret));
    }

    #[test]
    fn only_a_finished_and_checked_write_is_a_success() {
        let done = state(r#"{"state":"done","sha256":"abc"}"#);
        assert_eq!(outcome(done, true), Ok("abc".to_owned()));
        // Even a zero exit is no success without the helper saying it finished.
        assert!(outcome(state(r#"{"state":"writing","done":1,"total":9}"#), true).is_err());
        assert!(outcome(None, true).is_err());
    }

    #[test]
    fn a_refused_permission_a_cancel_and_an_error_each_say_what_happened() {
        let refused = outcome(None, false).unwrap_err();
        assert!(refused.contains("permission was refused"), "{refused}");
        let cancelled = outcome(state(r#"{"state":"cancelled","message":"x"}"#), false);
        assert!(cancelled.unwrap_err().contains("must be written again"));
        let failed = outcome(
            state(r#"{"state":"error","message":"disk 3 is gone"}"#),
            false,
        );
        assert_eq!(failed, Err("disk 3 is gone".to_owned()));
    }

    #[test]
    fn progress_shows_writing_then_checking_and_nothing_before_a_total_is_known() {
        assert_eq!(progress_of(&state(r#"{"state":"writing"}"#).unwrap()), None);
        let writing = state(r#"{"state":"writing","done":5,"checked":2,"total":10}"#).unwrap();
        let progress = progress_of(&writing).expect("progress");
        assert_eq!(
            (progress.phase, progress.done, progress.checked),
            ("writing", 5, 2)
        );
        let checking = state(r#"{"state":"checking","done":10,"checked":4,"total":10}"#).unwrap();
        assert_eq!(progress_of(&checking).expect("progress").phase, "checking");
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

    #[test]
    fn a_cancel_before_the_first_image_stops_everything_and_writes_nothing() {
        let request = Request {
            base: PathBuf::from("no-such-base.img"),
            profile: PathBuf::from("no-such-profile.json"),
            // A disk number that is only read about, never opened: the job stops first.
            disks: vec![99],
            confirmed: Vec::new(),
            to_file: None,
            allow_virtual: false,
            wifi_password: None,
            first_boot_enrolment: false,
        };
        let cancel = AtomicBool::new(true);
        let work = tempfile::tempdir().expect("folder");
        let result = make_and_write(
            &request,
            None,
            &cancel,
            work.path(),
            &|_| {},
            &mut Vec::new(),
        );
        assert_eq!(result, Err(CANCELLED_BEFORE_WRITING.to_owned()));
    }
}
