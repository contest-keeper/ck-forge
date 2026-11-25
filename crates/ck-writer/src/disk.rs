use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Error;
use crate::backend::{self, DiskBackend};

/// A disk of this computer. The fields are the Windows names; other systems fill them the same way.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Disk {
    pub number: u32,
    pub friendly_name: Option<String>,
    pub serial_number: Option<String>,
    #[serde(rename = "Path")]
    pub(crate) device_path: Option<String>,
    pub size: u64,
    pub bus_type: String,
    pub is_system: bool,
    pub is_boot: bool,
    /// Set by a system that knows more than the bus says: why this disk is never written.
    #[serde(default)]
    pub blocked: Option<String>,
}

/// Every disk of this computer. Needs no administrator rights.
pub fn list_disks() -> Result<Vec<Disk>, Error> {
    backend::system().list()
}

impl Disk {
    /// A fingerprint of this disk as it was listed: its number, name, serial number, size and device
    /// path. A window that asked the admin to confirm this disk hands it to the helper that writes,
    /// which refuses a disk with another fingerprint, so a stick that took the number of the
    /// confirmed one in the meantime is never erased.
    #[must_use]
    pub fn identity_token(&self) -> String {
        let text = format!(
            "{}|{}|{}|{}|{}",
            self.number,
            self.friendly_name.as_deref().unwrap_or(""),
            self.serial_number.as_deref().unwrap_or("").trim(),
            self.size,
            self.device_path.as_deref().unwrap_or("").trim(),
        );
        crate::write::hex(&Sha256::digest(text.as_bytes()))
    }

    /// The size for people, in decimal gigabytes like the box of a stick, e.g. `31.9 GB`.
    #[must_use]
    pub fn size_text(&self) -> String {
        let tenths = self.size / 100_000_000;
        format!("{}.{} GB", tenths / 10, tenths % 10)
    }

    /// Says why this disk must not be written, or nothing if it may be.
    ///
    /// Only USB disks qualify. A virtual disk (a VHD used to test the writer without a stick)
    /// qualifies only when `allow_virtual` is set.
    pub fn check(&self, image_size: u64, allow_virtual: bool) -> Result<(), Error> {
        let refuse = |reason: &str| Error::Refused {
            number: self.number,
            reason: reason.to_owned(),
        };
        if self.is_system || self.is_boot {
            return Err(refuse("the disk this computer runs from"));
        }
        if let Some(reason) = &self.blocked {
            return Err(refuse(reason));
        }
        let virtual_disk = self.bus_type.contains("Virtual");
        if virtual_disk && !allow_virtual {
            return Err(refuse(
                "a virtual disk (pass --allow-virtual to test with one)",
            ));
        }
        if !virtual_disk && self.bus_type != "USB" {
            return Err(refuse(&format!(
                "a {} disk, not a USB stick",
                self.bus_type
            )));
        }
        if self.size < image_size {
            return Err(Error::TooSmall {
                number: self.number,
                disk: self.size,
                image: image_size,
            });
        }
        Ok(())
    }

    pub(crate) fn recheck_identity(&self, backend: &dyn DiskBackend) -> Result<(), Error> {
        let detail = "the disk changed since it was listed; list the disks again";
        let now = backend
            .list()?
            .into_iter()
            .find(|disk| disk.number == self.number)
            .ok_or(Error::NoSuchDisk(self.number))?;
        if now != *self
            || self
                .device_path
                .as_deref()
                .is_none_or(|path| path.trim().is_empty())
        {
            return Err(Error::Prepare {
                number: self.number,
                detail: detail.to_owned(),
            });
        }
        Ok(())
    }
}

/// Gets `chosen` ready to be written: checks that it is still allowed, then lets the system
/// release it (on Windows the partitions are removed and the disk taken offline when it supports
/// that). The writer looks the disk up again before it opens it, because disk numbers can change
/// when a stick is pulled out.
///
/// Needs administrator rights. Everything on the disk is lost.
pub fn prepare_disk(chosen: &Disk, image_size: u64, allow_virtual: bool) -> Result<(), Error> {
    prepare_with(backend::system(), chosen, image_size, allow_virtual)
}

pub(crate) fn prepare_with(
    backend: &dyn DiskBackend,
    chosen: &Disk,
    image_size: u64,
    allow_virtual: bool,
) -> Result<(), Error> {
    chosen.check(image_size, allow_virtual)?;
    backend.prepare(chosen).map_err(|detail| Error::Prepare {
        number: chosen.number,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(bus: &str) -> Disk {
        Disk {
            number: 3,
            friendly_name: Some("Stick".into()),
            serial_number: Some("ABC".into()),
            device_path: Some("selected-device-path".into()),
            size: 32_000_000_000,
            bus_type: bus.into(),
            is_system: false,
            is_boot: false,
            blocked: None,
        }
    }

    #[test]
    fn only_a_usb_disk_that_is_big_enough_and_not_the_system_disk_may_be_written() {
        let mut system = disk("USB");
        system.is_system = true;
        let mut boot = disk("USB");
        boot.is_boot = true;
        // (disk, image size, virtual disks allowed, may be written)
        let cases = [
            (disk("USB"), 29_884_416_000, false, true),
            (system, 1, true, false),
            (boot, 1, true, false),
            (disk("NVMe"), 1, true, false),
            (disk("SATA"), 1, true, false),
            (disk("File Backed Virtual"), 1, false, false),
            (disk("File Backed Virtual"), 1, true, true),
        ];
        for (disk, image_size, allow_virtual, allowed) in cases {
            assert_eq!(
                disk.check(image_size, allow_virtual).is_ok(),
                allowed,
                "{disk:?} with virtual disks allowed: {allow_virtual}"
            );
        }

        let error = disk("USB").check(64_000_000_000, false).unwrap_err();
        assert!(matches!(error, Error::TooSmall { number: 3, .. }));
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    fn stick(number: u32, serial: &str) -> Disk {
        Disk {
            number,
            friendly_name: Some("Stick".into()),
            serial_number: Some(serial.into()),
            device_path: Some("/dev/fake".into()),
            size: 32_000_000_000,
            bus_type: "USB".into(),
            is_system: false,
            is_boot: false,
            blocked: None,
        }
    }

    #[test]
    fn another_stick_with_the_same_number_has_another_token() {
        let a = stick(3, "AAAA1111");
        assert_eq!(a.identity_token(), stick(3, "AAAA1111 ").identity_token());
        assert_ne!(a.identity_token(), stick(3, "BBBB2222").identity_token());
        assert_ne!(a.identity_token(), stick(4, "AAAA1111").identity_token());
        // Only hex, so it can sit in the helper's command line.
        assert!(a.identity_token().bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
