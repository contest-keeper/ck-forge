use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::Error;

/// A disk as Windows reports it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Disk {
    pub number: u32,
    pub friendly_name: Option<String>,
    pub serial_number: Option<String>,
    #[serde(rename = "Path")]
    device_path: Option<String>,
    pub size: u64,
    pub bus_type: String,
    pub is_system: bool,
    pub is_boot: bool,
}

const LISTING: &str = "ConvertTo-Json -InputObject @(Get-Disk | ForEach-Object { \
    [pscustomobject]@{ Number = [int]$_.Number; FriendlyName = $_.FriendlyName; \
    SerialNumber = $_.SerialNumber; Path = $_.Path; Size = [uint64]$_.Size; BusType = [string]$_.BusType; \
    IsSystem = [bool]$_.IsSystem; IsBoot = [bool]$_.IsBoot } }) -Compress";

/// Every disk of this PC. Needs no administrator rights.
pub fn list_disks() -> Result<Vec<Disk>, Error> {
    let text = powershell(LISTING, None).map_err(Error::Listing)?;
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

    pub(crate) fn recheck_identity(&self) -> Result<(), Error> {
        let detail = "the disk changed since it was listed; list the disks again";
        let now = list_disks()?
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

/// Gets `chosen` ready to be written: looks the disk up again, because disk numbers can change
/// when a stick is pulled out, checks that it is still the same disk and still allowed, then
/// removes its partitions. Disks that support offlining are taken offline. Removable USB media
/// must have no partitions left before raw writes are allowed.
///
/// Needs administrator rights. Everything on the disk is lost.
pub fn prepare_disk(chosen: &Disk, image_size: u64, allow_virtual: bool) -> Result<(), Error> {
    chosen.check(image_size, allow_virtual)?;
    powershell(PREPARE, Some(chosen)).map_err(|detail| Error::Prepare {
        number: chosen.number,
        detail,
    })?;
    Ok(())
}

const PREPARE: &str = r"
$ErrorActionPreference = 'Stop'
$expected = ConvertFrom-Json $env:CK_FORGE_SELECTED_DISK
function Read-SelectedDisk {
    # Storage can briefly stop reporting a disk while it is reprobed. A replacement fails immediately.
    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        $matches = @(Get-Disk | Where-Object Number -eq $expected.Number)
        if ($matches.Count -gt 0) {
            $current = $matches[0]
            if ($matches.Count -ne 1 -or [string]::IsNullOrWhiteSpace($expected.Path) -or
                $current.Path -ne $expected.Path -or [string]$current.SerialNumber -cne [string]$expected.SerialNumber -or
                $current.FriendlyName -cne $expected.FriendlyName -or $current.Size -ne $expected.Size -or
                [string]$current.BusType -ne $expected.BusType -or $current.IsBoot -or $current.IsSystem) {
                throw 'the disk changed since it was listed; list the disks again'
            }
            return $current
        }
        if ($attempt -lt 4) { Start-Sleep -Milliseconds 250 }
    }
    throw ('disk ' + $expected.Number + ' is no longer present at its selected number; list the disks again')
}

$disk = Read-SelectedDisk
# USB disks with fixed media still support offlining. WMI capability 7 identifies removable media.
$removable = $expected.BusType -eq 'USB' -and
    (Get-CimInstance -ClassName Win32_DiskDrive -Filter ('Index = ' + $expected.Number)).Capabilities -contains 7
$disk = Read-SelectedDisk
if ($disk.IsOffline) {
    Set-Disk -InputObject $disk -IsOffline $false
    $disk = Read-SelectedDisk
}
if ($disk.IsReadOnly) {
    Set-Disk -InputObject $disk -IsReadOnly $false
    $disk = Read-SelectedDisk
}
if ($disk.IsOffline -or $disk.IsReadOnly) { throw 'the disk is not writable' }
if ($disk.PartitionStyle -ne 'RAW' -and (!$removable -or $disk.NumberOfPartitions -ne 0)) {
    Clear-Disk -InputObject $disk -RemoveData -RemoveOEM -Confirm:$false
    $disk = Read-SelectedDisk
}
if ($disk.NumberOfPartitions -ne 0) { throw 'the disk still has partitions' }
if ($disk.IsOffline -or $disk.IsReadOnly) { throw 'the disk is not writable' }
if (!$removable) {
    Set-Disk -InputObject $disk -IsOffline $true
    $disk = Read-SelectedDisk
    if (!$disk.IsOffline) { throw 'Windows did not take the disk offline' }
}
";

/// Runs a PowerShell command and returns what it printed, or what it complained about.
fn powershell(script: &str, chosen: Option<&Disk>) -> Result<String, String> {
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    if let Some(disk) = chosen {
        let expected =
            serde_json::to_string(disk).map_err(|e| format!("encoding disk identity: {e}"))?;
        command.env("CK_FORGE_SELECTED_DISK", expected);
    }
    let output = command
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
            device_path: Some("selected-device-path".into()),
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

    #[cfg(windows)]
    fn prepare_mock(bus: &str, setup: &str) -> Result<String, String> {
        powershell(
            &format!(
                r"
            $disk = ConvertFrom-Json $env:CK_FORGE_SELECTED_DISK
            $disk | Add-Member NoteProperty PartitionStyle MBR
            $disk | Add-Member NoteProperty NumberOfPartitions 1
            $disk | Add-Member NoteProperty IsReadOnly $false
            $disk | Add-Member NoteProperty IsOffline $false
            $capability = 7
            $remaining = 0
            $script:missing = 0
            $deny_mutation = $false
            {setup}
            function Get-Disk {{
                if ($script:missing -gt 0) {{ $script:missing--; return }}
                $disk
            }}
            function Start-Sleep {{ param($Milliseconds) }}
            function Get-CimInstance {{ param($ClassName, $Filter);
                [pscustomobject]@{{ Capabilities = @($capability) }} }}
            function Set-Disk {{ [CmdletBinding()] param($InputObject, [bool]$IsOffline, [bool]$IsReadOnly)
                if ($deny_mutation -or !$InputObject) {{ throw 'unexpected mutation' }}
                if ($PSBoundParameters.ContainsKey('IsOffline')) {{
                    if ($IsOffline -and $capability -eq 7) {{ throw 'Removable media cannot be set to offline' }}
                    $disk.IsOffline = $IsOffline
                    if ($IsOffline) {{ 'offline' }} else {{ 'online' }}
                }}
                if ($PSBoundParameters.ContainsKey('IsReadOnly')) {{
                    $disk.IsReadOnly = $IsReadOnly
                    'read-write'
                }}
            }}
            function Clear-Disk {{ param($InputObject, [switch]$RemoveData, [switch]$RemoveOEM, [switch]$Confirm)
                if ($deny_mutation -or !$InputObject) {{ throw 'unexpected mutation' }}
                $disk.NumberOfPartitions = $remaining
                'cleared'
            }}
            {PREPARE}
            ",
            ),
            Some(&disk(bus)),
        )
    }

    #[test]
    #[cfg(windows)]
    fn removable_usb_preparation_requires_an_empty_disk_and_skips_offline() {
        let result = prepare_mock("USB", "").expect("removable media can be prepared");
        assert_eq!(result.trim(), "cleared");
        assert!(
            prepare_mock("USB", "$remaining = 1")
                .expect_err("USB with remaining partitions is refused")
                .contains("still has partitions")
        );
    }

    #[test]
    #[cfg(windows)]
    fn already_empty_removable_usb_has_no_storage_mutations() {
        let result = prepare_mock(
            "USB",
            "$disk.NumberOfPartitions = 0; $deny_mutation = $true; $script:missing = 2",
        )
        .expect("an already-empty USB needs only a positive identity check");
        assert_eq!(result.trim(), "");
    }

    #[test]
    #[cfg(windows)]
    fn replaced_or_missing_disks_are_refused_before_mutation() {
        for (setup, message) in [
            ("$disk.Path = 'replacement'", "the disk changed"),
            ("$disk.SerialNumber = 'different'", "the disk changed"),
            ("$disk.IsSystem = $true", "the disk changed"),
            ("$script:missing = 6", "is no longer present"),
        ] {
            let setup = format!("{setup}; $deny_mutation = $true");
            let error = prepare_mock("USB", &setup).expect_err("a fresh match is required");
            assert!(error.contains(message), "{error}");
        }
    }

    #[test]
    #[cfg(windows)]
    fn fixed_usb_and_virtual_disks_still_go_offline() {
        for bus in ["USB", "File Backed Virtual"] {
            let result = prepare_mock(bus, "$capability = 3").expect("disk can go offline");
            assert_eq!(result.lines().collect::<Vec<_>>(), ["cleared", "offline"]);
        }
    }
}
