//! Boots the downloaded image once and lets a script turn it into a contest desktop.

use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::seed::Seed;
use crate::tools::Qemu;
use crate::{Entry, Error};

pub struct Machine<'a> {
    pub qemu: &'a Qemu,
    pub accel: &'a str,
    pub memory_mb: u32,
    pub timeout: Duration,
}

const OK: &str = "CK-PROVISION-OK";
const FAILED: &str = "CK-PROVISION-FAILED";

/// Runs the entry's script in a VM that writes to `image`. The VM powers itself off when the
/// script is done. `progress` gets the seconds passed and the seconds allowed.
/// The serial log is kept in `work`, so a failed build can be looked at.
pub fn provision(
    machine: &Machine,
    entry: &Entry,
    image: &Path,
    work: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), Error> {
    let (code, vars_template) = machine.qemu.firmware()?;
    let vars = work.join("OVMF_VARS.fd");
    fs::copy(&vars_template, &vars).map_err(|e| Error::io("copying the firmware variables", e))?;
    let serial_log = work.join("provision-serial.log");
    let seed = Seed::start(entry.script()?.to_owned())?;
    let qemu_log = File::create(work.join("provision-qemu.err"))
        .map_err(|e| Error::io("creating the QEMU log", e))?;

    let mut qemu = Command::new(machine.qemu.program("qemu-system-x86_64"));
    qemu.args(["-machine", "q35", "-accel", machine.accel, "-cpu", "qemu64"])
        .args(["-smp", "2", "-m", &machine.memory_mb.to_string()])
        .arg("-drive")
        .arg(format!(
            "if=pflash,format=raw,readonly=on,file={}",
            code.display()
        ))
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,file={}", vars.display()))
        .arg("-drive")
        .arg(format!(
            "file={},format=raw,if=none,id=d0,cache=unsafe,aio=threads",
            image.display()
        ))
        .args(["-device", "virtio-blk-pci,drive=d0"])
        .args([
            "-netdev",
            "user,id=n0",
            "-device",
            "virtio-net-pci,netdev=n0",
        ])
        // cloud-init reads its instructions from this address; 10.0.2.2 is this computer.
        .args([
            "-smbios",
            &format!("type=1,serial=ds=nocloud;s=http://10.0.2.2:{}/", seed.port),
        ])
        .args(["-display", "none", "-serial"])
        .arg(format!("file:{}", serial_log.display()))
        .stdout(Stdio::null())
        .stderr(qemu_log);
    let mut child = ck_writer::without_console_window(&mut qemu)
        .spawn()
        .map_err(|e| Error::io("starting QEMU", e))?;

    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| Error::io("waiting for QEMU", e))?
        {
            if !status.success() {
                return Err(Error::MachineStopped(format!(
                    "QEMU stopped with {status}; see provision-qemu.err"
                )));
            }
            break;
        }
        if started.elapsed() > machine.timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Provision(format!(
                "no result after {} minutes; the log is {}",
                machine.timeout.as_secs() / 60,
                serial_log.display()
            )));
        }
        progress(started.elapsed().as_secs(), machine.timeout.as_secs());
        thread::sleep(Duration::from_secs(2));
    }

    let log = fs::read(&serial_log)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    check_log(&log, &serial_log)
}

fn check_log(log: &str, path: &Path) -> Result<(), Error> {
    let mut succeeded = false;
    let mut failed = false;
    for line in log.lines().map(str::trim) {
        match cloud_init_message(line).unwrap_or(line) {
            OK => succeeded = true,
            FAILED => failed = true,
            _ => {}
        }
    }
    if succeeded && !failed {
        return Ok(());
    }
    let reason = if failed {
        "the script failed"
    } else {
        "the machine stopped before the script finished"
    };
    Err(Error::Provision(format!(
        "{reason}; the log is {}",
        path.display()
    )))
}

fn cloud_init_message(line: &str) -> Option<&str> {
    // Cloud-init can forward the guest's exact line with a kernel timestamp and its PID.
    let (time, rest) = line.strip_prefix('[')?.split_once("] cloud-init[")?;
    let (seconds, fraction) = time.trim().split_once('.')?;
    let (pid, message) = rest.split_once("]: ")?;
    seconds.parse::<u64>().ok()?;
    fraction.parse::<u32>().ok()?;
    pid.parse::<u32>().ok()?;
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_ok_marker_passes() {
        let path = Path::new("x.log");
        assert!(check_log("boot\nCK-PROVISION-OK\n", path).is_ok());
        assert!(check_log("CK-PROVISION-FAILED", path).is_err());
        assert!(check_log("", path).is_err());
        assert!(check_log("boot printed CK-PROVISION-OK as an example\n", path).is_err());
        assert!(check_log("CK-PROVISION-OK\nCK-PROVISION-FAILED\n", path).is_err());
        assert!(check_log("[  365.110898] cloud-init[1105]: CK-PROVISION-OK\n", path).is_ok());
        assert!(check_log("[bad] cloud-init[1105]: CK-PROVISION-OK\n", path).is_err());
        assert!(
            check_log(
                "[  365.110898] cloud-init[1105]: echo CK-PROVISION-OK\n",
                path
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_guest_command_reports_failure() {
        let script = include_str!("../data/ubuntu-24.04.sh");
        let (_, function) = script
            .split_once("\nprovision() {\n")
            .expect("guest function");
        let (body, footer) = function.split_once("\n}\n").expect("guest footer");
        let (setup, _) = body.split_once("\n  apt-get update").expect("guest setup");
        // Run the actual setup and footer with a controlled guest command and inert shutdown.
        for (command, marker) in [(":", OK), ("false", FAILED)] {
            let fixture = format!(
                "sync() {{ :; }}\npoweroff() {{ :; }}\nprovision() {{\n{setup}\n  {command}\n  :\n}}\n{footer}"
            );
            let output = Command::new("bash")
                .args(["-c", &fixture])
                .output()
                .expect("run shell fixture");
            assert!(output.status.success());
            let log = String::from_utf8(output.stdout).expect("serial output");
            assert_eq!(log.trim(), marker);
        }
    }
}
