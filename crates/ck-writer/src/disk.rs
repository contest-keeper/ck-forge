use std::process::Command;

use serde::Deserialize;

use crate::Error;

/// A disk as Windows reports it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct Disk {
    pub number: u32,
    pub friendly_name: Option<String>,
    pub serial_number: Option<String>,
    pub size: u64,
    pub bus_type: String,
    pub is_system: bool,
    pub is_boot: bool,
}

const LISTING: &str = "ConvertTo-Json -InputObject @(Get-Disk | ForEach-Object { \
    [pscustomobject]@{ Number = [int]$_.Number; FriendlyName = $_.FriendlyName; \
    SerialNumber = $_.SerialNumber; Size = [uint64]$_.Size; BusType = [string]$_.BusType; \
    IsSystem = [bool]$_.IsSystem; IsBoot = [bool]$_.IsBoot } }) -Compress";

/// Every disk of this PC. Needs no administrator rights.
pub fn list_disks() -> Result<Vec<Disk>, Error> {
    let text = powershell(LISTING).map_err(Error::Listing)?;
    parse_disks(&text)
}

fn parse_disks(text: &str) -> Result<Vec<Disk>, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(text).map_err(|e| Error::Listing(format!("unreadable answer: {e}")))
}

impl Disk {
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
            return Err(refuse("the disk Windows runs from"));
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
}

/// Gets `chosen` ready to be written: looks the disk up again, because disk numbers can change
/// when a stick is pulled out, checks that it is still the same disk and still allowed, then
/// removes its partitions and takes it offline so Windows lets go of its volumes.
///
/// Needs administrator rights. Everything on the disk is lost.
pub fn prepare_disk(chosen: &Disk, image_size: u64, allow_virtual: bool) -> Result<(), Error> {
    let now = list_disks()?
        .into_iter()
        .find(|disk| disk.number == chosen.number)
        .ok_or(Error::NoSuchDisk(chosen.number))?;
    if now != *chosen {
        return Err(Error::Prepare {
            number: chosen.number,
            detail: "the disk changed since it was listed; list the disks again".to_owned(),
        });
    }
    now.check(image_size, allow_virtual)?;

    let n = now.number;
    let script = format!(
        "$ErrorActionPreference = 'Stop'; \
         Set-Disk -Number {n} -IsOffline $false -ErrorAction SilentlyContinue; \
         Set-Disk -Number {n} -IsReadOnly $false -ErrorAction SilentlyContinue; \
         if ((Get-Disk -Number {n}).PartitionStyle -ne 'RAW') {{ \
             Clear-Disk -Number {n} -RemoveData -RemoveOEM -Confirm:$false }}; \
         Set-Disk -Number {n} -IsOffline $true"
    );
    powershell(&script).map_err(|detail| Error::Prepare { number: n, detail })?;
    Ok(())
}

/// Runs a PowerShell command and returns what it printed, or what it complained about.
fn powershell(script: &str) -> Result<String, String> {
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("could not start PowerShell: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(error.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(bus: &str) -> Disk {
        Disk {
            number: 3,
            friendly_name: Some("Stick".into()),
            serial_number: Some("ABC".into()),
            size: 32_000_000_000,
            bus_type: bus.into(),
            is_system: false,
            is_boot: false,
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

    #[test]
    fn the_listing_reads_one_disk_or_many() {
        let one = r#"[{"Number":1,"FriendlyName":null,"SerialNumber":null,"Size":5,"BusType":"USB","IsSystem":false,"IsBoot":false}]"#;
        assert_eq!(parse_disks(one).unwrap().len(), 1);
        assert_eq!(parse_disks("").unwrap().len(), 0);
        assert!(parse_disks("nonsense").is_err());
    }
}
