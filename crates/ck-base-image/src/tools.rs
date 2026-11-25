//! QEMU programs the build runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

/// The name of the program that makes and resizes disk images, which tells where QEMU is.
const IMG: &str = if cfg!(windows) {
    "qemu-img.exe"
} else {
    "qemu-img"
};

/// The firmware QEMU boots a PC with, and the variables it starts from.
const FIRMWARE_CODE: &str = "edk2-x86_64-code.fd";
const FIRMWARE_VARS: &str = "edk2-i386-vars.fd";

/// Where QEMU is installed: the given folder, else the usual place on this system, else the PATH.
pub struct Qemu {
    dir: Option<PathBuf>,
}

/// The folders QEMU's programs usually are in: on Windows its installer's folder, on macOS the two
/// places Homebrew uses (Apple silicon, then Intel). A program started from the Finder has no
/// Homebrew in its PATH, so the folders are looked at by name.
fn usual_program_folders() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]
    } else {
        vec![PathBuf::from(r"C:\Program Files\qemu")]
    }
}

/// The first of `folders` that holds `program`. On Windows the folder is the one of the installer,
/// which also holds `share`; the test is the same for both.
fn first_with(folders: &[PathBuf], program: &str) -> Option<PathBuf> {
    folders
        .iter()
        .find(|folder| folder.join(program).is_file())
        .cloned()
}

/// The folders that may hold the firmware of the QEMU whose programs are in `dir`: `share` of the
/// Windows installer, and `../share/qemu` of Homebrew (`/opt/homebrew/bin` has
/// `/opt/homebrew/share/qemu` beside it).
fn firmware_folders(dir: &Path) -> Vec<PathBuf> {
    let mut folders = vec![dir.join("share")];
    if let Some(prefix) = dir.parent() {
        folders.push(prefix.join("share").join("qemu"));
    }
    folders
}

/// The firmware and its variables from the first of `folders` that has both.
fn firmware_in(folders: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    folders.iter().find_map(|folder| {
        let (code, vars) = (folder.join(FIRMWARE_CODE), folder.join(FIRMWARE_VARS));
        (code.is_file() && vars.is_file()).then_some((code, vars))
    })
}

impl Qemu {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let dir = dir.or_else(|| first_with(&usual_program_folders(), IMG));
        Self { dir }
    }

    pub fn program(&self, name: &str) -> PathBuf {
        match &self.dir {
            Some(dir) => dir.join(name),
            None => PathBuf::from(name),
        }
    }

    /// Firmware and its variables ship with QEMU: in `share` on Windows, in `share/qemu` of the
    /// prefix on macOS with Homebrew.
    pub fn firmware(&self) -> Result<(PathBuf, PathBuf), Error> {
        let Some(dir) = &self.dir else {
            return Err(Error::Tool {
                tool: "QEMU".to_owned(),
                detail: "no QEMU folder found; pass --qemu".to_owned(),
            });
        };
        let folders = firmware_folders(dir);
        firmware_in(&folders).ok_or_else(|| Error::Tool {
            tool: "QEMU".to_owned(),
            detail: format!(
                "no {FIRMWARE_CODE} and {FIRMWARE_VARS} in {}",
                folders
                    .iter()
                    .map(|folder| folder.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        })
    }

    /// Turns the downloaded qcow2 into a raw disk of `gigabytes`, the format sticks are made from.
    pub fn make_raw(&self, qcow: &Path, raw: &Path, gigabytes: u32) -> Result<(), Error> {
        let img = self.program("qemu-img");
        run(
            Command::new(&img)
                .args(["convert", "-O", "raw"])
                .arg(qcow)
                .arg(raw),
            "qemu-img convert",
        )?;
        run(
            Command::new(&img)
                .args(["resize", "-f", "raw"])
                .arg(raw)
                .arg(format!("{gigabytes}G")),
            "qemu-img resize",
        )
    }
}

pub fn run(command: &mut Command, name: &str) -> Result<(), Error> {
    let output = ck_writer::without_console_window(command)
        .output()
        .map_err(|e| Error::io(format!("starting {name}"), e))?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::Tool {
        tool: name.to_owned(),
        detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(path, b"x").expect("file");
    }

    #[test]
    fn the_firmware_of_the_windows_installer_is_found_in_share() {
        let qemu = tempfile::tempdir().expect("folder");
        touch(&qemu.path().join("share").join(FIRMWARE_CODE));
        touch(&qemu.path().join("share").join(FIRMWARE_VARS));
        let (code, vars) = Qemu::new(Some(qemu.path().to_owned()))
            .firmware()
            .expect("found");
        assert_eq!(code, qemu.path().join("share").join(FIRMWARE_CODE));
        assert_eq!(vars, qemu.path().join("share").join(FIRMWARE_VARS));
    }

    #[test]
    fn the_firmware_of_homebrew_is_found_beside_the_bin_folder() {
        // /opt/homebrew/bin has its firmware in /opt/homebrew/share/qemu.
        let prefix = tempfile::tempdir().expect("folder");
        let bin = prefix.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        touch(&prefix.path().join("share/qemu").join(FIRMWARE_CODE));
        touch(&prefix.path().join("share/qemu").join(FIRMWARE_VARS));
        let (code, _) = Qemu::new(Some(bin)).firmware().expect("found");
        assert_eq!(code, prefix.path().join("share/qemu").join(FIRMWARE_CODE));
    }

    #[test]
    fn a_folder_with_only_one_of_the_two_files_is_not_taken_and_the_error_names_the_places() {
        let prefix = tempfile::tempdir().expect("folder");
        let bin = prefix.path().join("bin");
        // `bin/share` is looked at, and holds the code but not the variables.
        touch(&bin.join("share").join(FIRMWARE_CODE));
        let error = Qemu::new(Some(bin)).firmware().unwrap_err().to_string();
        assert!(error.contains(FIRMWARE_VARS), "{error}");
        assert!(error.contains("share"), "{error}");
    }

    #[test]
    fn the_usual_folder_is_the_first_that_holds_the_program() {
        let (first, second) = (
            tempfile::tempdir().expect("first"),
            tempfile::tempdir().expect("second"),
        );
        touch(&second.path().join("qemu-img"));
        let folders = [first.path().to_owned(), second.path().to_owned()];
        assert_eq!(
            first_with(&folders, "qemu-img"),
            Some(second.path().to_owned())
        );
        assert_eq!(first_with(&folders, "qemu-system-x86_64"), None);
    }
}
