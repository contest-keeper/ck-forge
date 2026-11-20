//! QEMU programs the build runs.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

/// Where QEMU is installed: the given folder, else the usual Windows place, else the PATH.
pub struct Qemu {
    dir: Option<PathBuf>,
}

impl Qemu {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let dir = dir.or_else(|| {
            let usual = PathBuf::from(r"C:\Program Files\qemu");
            usual.is_dir().then_some(usual)
        });
        Self { dir }
    }

    pub fn program(&self, name: &str) -> PathBuf {
        match &self.dir {
            Some(dir) => dir.join(name),
            None => PathBuf::from(name),
        }
    }

    /// Firmware and its variables ship with QEMU on Windows.
    pub fn firmware(&self) -> Result<(PathBuf, PathBuf), Error> {
        let Some(dir) = &self.dir else {
            return Err(Error::Tool {
                tool: "QEMU".to_owned(),
                detail: "no QEMU folder found; pass --qemu".to_owned(),
            });
        };
        let share = dir.join("share");
        Ok((
            share.join("edk2-x86_64-code.fd"),
            share.join("edk2-i386-vars.fd"),
        ))
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
