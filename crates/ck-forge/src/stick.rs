//! `ck-forge stick`: listing, and writing sticks with their progress report.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ck_admin_core::{Error, check_one_stick};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde_json::json;

pub(crate) fn stick_list() -> Result<String, Error> {
    let mut text = String::new();
    for disk in ck_writer::list_disks()? {
        let verdict = match disk.check(0, true) {
            Ok(()) => "can be written".to_owned(),
            Err(error) => format!("refused: {error}"),
        };
        let _ = writeln!(
            text,
            "disk {}  {}  {}  {}  serial {}  [{}]",
            disk.number,
            disk.friendly_name.as_deref().unwrap_or("?"),
            disk.size_text(),
            disk.bus_type,
            disk.serial_number.as_deref().unwrap_or("?").trim(),
            verdict
        );
    }
    Ok(text)
}

/// What controls a write from outside: the files that report and cancel, and the disks that were
/// confirmed by an admin window that started this command.
pub(crate) struct Control<'a> {
    pub progress: Option<&'a Path>,
    pub cancel: Option<&'a Path>,
    /// `NUMBER=TOKEN` for each confirmed disk.
    pub confirmed: &'a [String],
}

/// Writes the sticks and, when a progress file was asked for, says in it how it ended: an error,
/// or `cancelled` when the admin window asked to stop.
pub(crate) fn stick_write_reported(
    images: &[PathBuf],
    disks: &[u32],
    to: Vec<PathBuf>,
    allow_virtual: bool,
    yes: bool,
    control: &Control<'_>,
) -> Result<String, Error> {
    let mut failed = Vec::new();
    let result = stick_write(images, disks, to, allow_virtual, yes, control, &mut failed);
    if let Err(error) = &result {
        let state = if matches!(error, Error::Writer(ck_writer::Error::Cancelled)) {
            "cancelled"
        } else {
            "error"
        };
        report_progress(
            control.progress,
            // `failed` lets the window tell the sticks that were not written from those that were.
            &json!({ "state": state, "message": error.to_string(), "failed": failed }),
        );
    }
    result
}

fn stick_write(
    images: &[PathBuf],
    disks: &[u32],
    to: Vec<PathBuf>,
    allow_virtual: bool,
    yes: bool,
    control: &Control<'_>,
    failed_targets: &mut Vec<usize>,
) -> Result<String, Error> {
    let count = disks.len().max(to.len());
    let images = images_for(images, count)?;
    // Before anything is erased: the images must fit together, and a base that moved is found now.
    let size = ck_writer::check_images(&images)?;
    let prepared = if to.is_empty() {
        prepare_disks(disks, size, allow_virtual, yes, control.confirmed)?
    } else {
        Vec::new()
    };
    let targets = if prepared.is_empty() {
        to
    } else {
        prepared
            .iter()
            .map(|disk| ck_writer::disk_path(disk.number))
            .collect()
    };
    let jobs: Vec<(PathBuf, PathBuf)> = images.into_iter().zip(targets).collect();
    let targets: Vec<&PathBuf> = jobs.iter().map(|(_, target)| target).collect();

    let bars = progress_bars(&targets, size);
    // For each target how far it is written and how far it is checked; the slowest is reported.
    let mut state = vec![(0_u64, 0_u64); targets.len()];
    let mut last_report = Instant::now();
    let report = |index: usize, phase, done, total| {
        match phase {
            ck_writer::Phase::Writing => state[index].0 = done,
            ck_writer::Phase::Verifying => state[index].1 = done,
        }
        show_target(&bars[index], state[index], total);
        // A file rewritten for every 4 MiB block would be needless work.
        if done == total || last_report.elapsed() >= Duration::from_millis(500) {
            last_report = Instant::now();
            report_slowest(control.progress, &state, total);
        }
    };
    let results = if prepared.is_empty() {
        ck_writer::write_images(&jobs, report)
    } else {
        let cancelled = || control.cancel.is_some_and(Path::exists);
        ck_writer::write_disk_images_cancellable(&jobs, &prepared, &cancelled, report)
    };
    for bar in &bars {
        bar.finish_and_clear();
    }

    let mut text = String::new();
    let mut failures = Vec::new();
    let mut hash = String::new();
    for (index, ((image, target), result)) in
        jobs.iter().zip(results.map_err(admin_hint)?).enumerate()
    {
        match result {
            Ok(ck_writer::Written { sha256, unchanged }) => {
                let _ = write!(text, "Wrote {} to {}", image.display(), target.display());
                if unchanged > 0 {
                    let _ = write!(
                        text,
                        " ({:.1} of {:.1} GB was already on it and was not written again)",
                        gigabytes(unchanged),
                        gigabytes(size),
                    );
                }
                text.push('\n');
                hash = sha256;
            }
            Err(error) => {
                failed_targets.push(index);
                failures.push(format!("{}: {}", label(target), admin_hint(error)));
            }
        }
    }
    if !failures.is_empty() {
        let message = format!(
            "{} of {} sticks failed ({}{})",
            failures.len(),
            targets.len(),
            failures.join("; "),
            if text.is_empty() {
                String::new()
            } else {
                format!("). The others are written and checked, {}", text.trim_end())
            }
        );
        return Err(Error::io("writing the sticks", io::Error::other(message)));
    }
    report_progress(
        control.progress,
        &json!({ "state": "done", "sha256": hash }),
    );
    if jobs.len() == 1 || jobs.windows(2).all(|pair| pair[0].0 == pair[1].0) {
        let _ = writeln!(text, "Read back and checked, SHA-256 {hash}");
    } else {
        let _ = writeln!(
            text,
            "Every stick was read back and checked against its own image"
        );
    }
    Ok(text)
}

/// One progress bar for each target.
fn progress_bars(targets: &[&PathBuf], size: u64) -> Vec<ProgressBar> {
    let bars = MultiProgress::new();
    targets
        .iter()
        .map(|target| {
            let bar = bars.add(ProgressBar::new(size));
            bar.set_style(
                ProgressStyle::with_template(
                    "{prefix:>16} {msg:>9} {bar:30} {bytes}/{total_bytes} ({eta})",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
            );
            bar.set_prefix(label(target));
            bar
        })
        .collect()
}

/// One image for every target, in order: the one image given, or one image per target. Several
/// sticks written from one image that holds a stick's identity, or from the same image twice,
/// would share an id and secret, so that is refused.
pub(crate) fn images_for(images: &[PathBuf], count: usize) -> Result<Vec<PathBuf>, Error> {
    if images.len() == 1 {
        check_one_stick(&images[0], count)?;
        return Ok(vec![images[0].clone(); count]);
    }
    if images.len() != count {
        return Err(Error::io(
            format!("{} images for {count} sticks", images.len()),
            io::Error::other("give one image for all the sticks, or one image for each stick"),
        ));
    }
    for (index, image) in images.iter().enumerate() {
        if images[..index].contains(image) {
            return Err(Error::io(
                format!("{} is given twice", image.display()),
                io::Error::other("two sticks written from one image would share its id and secret"),
            ));
        }
    }
    Ok(images.to_vec())
}

pub(crate) fn gigabytes(bytes: u64) -> f64 {
    // Only for showing; a stick image is far below the range where this loses digits.
    #[allow(clippy::cast_precision_loss)]
    let gigabytes = bytes as f64 / 1e9;
    gigabytes
}

/// A short name for a target: `disk 3` for `\\.\PhysicalDrive3`, else the file name.
pub(crate) fn label(target: &Path) -> String {
    let name = target.file_name().map_or_else(
        || target.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    match name.strip_prefix("PhysicalDrive") {
        Some(number) => format!("disk {number}"),
        None => name,
    }
}

/// Looks every disk up and checks that it may be written, asks once for all of them, then gets
/// each ready. Nothing is touched before every disk has passed the checks. Keeps the selected
/// identities so the writer can match them again before opening the devices.
pub(crate) fn prepare_disks(
    numbers: &[u32],
    size: u64,
    allow_virtual: bool,
    yes: bool,
    confirmed: &[String],
) -> Result<Vec<ck_writer::Disk>, Error> {
    let confirmed = parse_confirmed(confirmed)?;
    let listed = ck_writer::list_disks()?;
    let mut chosen: Vec<ck_writer::Disk> = Vec::new();
    for &number in numbers {
        if chosen.iter().any(|d| d.number == number) {
            return Err(Error::io(
                format!("disk {number} is given twice"),
                io::Error::from(io::ErrorKind::InvalidInput),
            ));
        }
        let disk = listed
            .iter()
            .find(|d| d.number == number)
            .cloned()
            .ok_or(ck_writer::Error::NoSuchDisk(number))?;
        disk.check(size, allow_virtual)?;
        // The admin confirmed one particular stick in the window. If another one took this number
        // since, it is not erased: it was never confirmed.
        if let Some((_, token)) = confirmed.iter().find(|(n, _)| *n == number)
            && *token != disk.identity_token()
        {
            return Err(ck_writer::Error::Prepare {
                number,
                detail: "this is not the disk that was confirmed; list the disks again".to_owned(),
            }
            .into());
        }
        chosen.push(disk);
    }
    check_access(&chosen)?;
    if !yes {
        confirm(&chosen)?;
    }
    for disk in &chosen {
        ck_writer::prepare_disk(disk, size, allow_virtual).map_err(admin_hint)?;
    }
    Ok(chosen)
}

/// `NUMBER=TOKEN` pairs of `--confirmed`. A token is hex, so nothing else is accepted.
fn parse_confirmed(pairs: &[String]) -> Result<Vec<(u32, String)>, Error> {
    pairs
        .iter()
        .map(|pair| {
            pair.split_once('=')
                .and_then(|(number, token)| {
                    let valid = !token.is_empty() && token.bytes().all(|b| b.is_ascii_hexdigit());
                    valid.then(|| number.parse().ok().map(|n| (n, token.to_owned())))?
                })
                .ok_or_else(|| {
                    Error::io(
                        format!("--confirmed {pair}"),
                        io::Error::from(io::ErrorKind::InvalidInput),
                    )
                })
        })
        .collect()
}

/// Asks the admin to type the disk numbers, because this erases the whole disks.
pub(crate) fn confirm(disks: &[ck_writer::Disk]) -> Result<(), Error> {
    let mut out = io::stdout();
    let mut numbers = Vec::new();
    for disk in disks {
        writeln!(
            out,
            "This ERASES disk {}: {} ({}, serial {}).",
            disk.number,
            disk.friendly_name.as_deref().unwrap_or("?"),
            disk.size_text(),
            disk.serial_number.as_deref().unwrap_or("?").trim(),
        )
        .map_err(|e| Error::io("writing the output", e))?;
        numbers.push(disk.number.to_string());
    }
    let expected = format!("erase {}", numbers.join(" "));
    write!(out, "Type {expected} to go on: ")
        .and_then(|()| out.flush())
        .map_err(|e| Error::io("writing the output", e))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| Error::io("reading the answer", e))?;
    if answer.trim() == expected {
        Ok(())
    } else {
        Err(Error::io(
            "cancelled, nothing was written",
            io::Error::from(io::ErrorKind::Interrupted),
        ))
    }
}

/// What to do when a disk may not be opened.
const ADMIN_HINT: &str = if cfg!(target_os = "macos") {
    "access denied: run the command again with sudo"
} else {
    "access denied: open a terminal as administrator and run the command again"
};

/// Writing a disk without administrator rights fails with "access denied"; say what to do.
pub(crate) fn admin_hint(error: ck_writer::Error) -> Error {
    if let ck_writer::Error::Io { source, .. } = &error
        && source.kind() == io::ErrorKind::PermissionDenied
    {
        return Error::io(ADMIN_HINT, io::Error::from(io::ErrorKind::PermissionDenied));
    }
    error.into()
}

/// On macOS a stick's raw device belongs to root, and the disk is unmounted before it is opened.
/// Ask for `sudo` now, so that a stick is not unmounted for a write that cannot happen. A disk
/// image attached by the user belongs to the user and needs no `sudo`.
#[cfg(target_os = "macos")]
fn check_access(disks: &[ck_writer::Disk]) -> Result<(), Error> {
    use std::os::unix::fs::MetadataExt;

    let me = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        .ok_or_else(|| Error::io("finding out who runs this", io::ErrorKind::Other.into()))?;
    if me == 0 {
        return Ok(());
    }
    for disk in disks {
        let device = ck_writer::disk_path(disk.number);
        let owner = std::fs::metadata(&device)
            .map_err(|e| Error::io(format!("looking at {}", device.display()), e))?
            .uid();
        if owner != me {
            return Err(Error::io(
                ADMIN_HINT,
                io::Error::from(io::ErrorKind::PermissionDenied),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_wraps)]
fn check_access(_disks: &[ck_writer::Disk]) -> Result<(), Error> {
    Ok(())
}

/// Shows a target's bar: the writing, then the checking once the writing is over.
pub(crate) fn show_target(bar: &ProgressBar, (written, checked): (u64, u64), total: u64) {
    let writing = written < total;
    bar.set_length(total);
    bar.set_position(if writing { written } else { checked });
    bar.set_message(if writing {
        format!("check {}%", checked * 100 / total)
    } else {
        "checking".to_owned()
    });
}

/// Reports how far the slowest target is written and checked.
pub(crate) fn report_slowest(file: Option<&Path>, state: &[(u64, u64)], total: u64) {
    let written = state
        .iter()
        .map(|(written, _)| *written)
        .min()
        .unwrap_or_default();
    let checked = state
        .iter()
        .map(|(_, checked)| *checked)
        .min()
        .unwrap_or_default();
    let name = if written < total {
        "writing"
    } else {
        "checking"
    };
    report_progress(
        file,
        &json!({ "state": name, "done": written, "checked": checked, "total": total }),
    );
}

/// Keeps the progress file of `stick write` up to date, when one was asked for. A failure to write it
/// is ignored: the write itself must not fail because the window stopped listening.
///
/// This runs with administrator rights and the path is in a folder the person's own programs can
/// write to. A plain `fs::write` follows a symbolic link put there and would overwrite its target as
/// the administrator. So the state is written to a new file that must not exist (`create_new` does
/// not follow links) and then renamed over the path, which replaces a link instead of following it.
// shortcut: a link in a folder above the path is still followed. Closing that needs a folder the
// person's own programs cannot change, which the window cannot make without elevation; upgrade
// when the helper gets its own root-owned folder for progress files.
pub(crate) fn report_progress(file: Option<&Path>, state: &serde_json::Value) {
    let Some(path) = file else {
        return;
    };
    // Named for this process, so it is never a file that belongs to anyone else.
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".{}.new", std::process::id()));
    let temporary = PathBuf::from(temporary);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut out| Write::write_all(&mut out, state.to_string().as_bytes()));
    if written.is_err() || std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

#[cfg(test)]
mod confirmed_tests {
    use super::*;

    #[test]
    fn only_a_number_and_a_hex_token_are_accepted() {
        assert_eq!(
            parse_confirmed(&["3=ab12".to_owned()]).expect("valid"),
            [(3, "ab12".to_owned())]
        );
        for bad in ["3", "x=ab", "3=", "3=zz", "3=ab;calc", "-1=ab"] {
            assert!(parse_confirmed(&[bad.to_owned()]).is_err(), "{bad}");
        }
    }
}

#[cfg(all(test, unix))]
mod progress_tests {
    use super::*;

    #[test]
    fn a_link_at_the_progress_path_is_replaced_and_its_target_is_left_alone() {
        let folder = tempfile::tempdir().expect("folder");
        let target = folder.path().join("precious");
        std::fs::write(&target, "keep me").expect("target");
        let path = folder.path().join("progress.json");
        std::os::unix::fs::symlink(&target, &path).expect("link");
        report_progress(Some(&path), &json!({ "state": "writing" }));
        assert_eq!(std::fs::read_to_string(&target).expect("target"), "keep me");
        assert_eq!(
            std::fs::read_to_string(&path).expect("progress"),
            r#"{"state":"writing"}"#
        );
    }

    #[test]
    fn the_progress_file_is_rewritten_each_time() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("progress.json");
        report_progress(Some(&path), &json!({ "state": "writing", "done": 1 }));
        report_progress(Some(&path), &json!({ "state": "done" }));
        assert_eq!(
            std::fs::read_to_string(&path).expect("progress"),
            r#"{"state":"done"}"#
        );
    }
}
