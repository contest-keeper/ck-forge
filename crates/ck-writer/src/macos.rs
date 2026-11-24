//! Disks on macOS, through `diskutil`. The plists it prints are read here and nothing else is
//! parsed, so what is judged about a disk is tested with recorded plists and no real disk.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

use crate::backend::DiskBackend;
use crate::{Disk, Error};

const DISKUTIL: &str = "/usr/sbin/diskutil";
/// The sector size of a stick. The writer pads to whole sectors of 512 bytes, which a raw device
/// with larger sectors would refuse in the middle of a write.
const SECTOR_SIZE: u64 = 512;

pub(crate) struct Macos;

impl DiskBackend for Macos {
    fn list(&self) -> Result<Vec<Disk>, Error> {
        list_with(&mut diskutil)
    }

    /// Unmounts every volume of `chosen`. A raw write needs no more: the old partition table is
    /// simply overwritten.
    fn prepare(&self, chosen: &Disk) -> Result<(), String> {
        prepare_with(&mut diskutil, chosen)
    }

    /// The raw device. `/dev/diskN` goes through the cache and is much slower.
    fn raw_path(&self, number: u32) -> PathBuf {
        PathBuf::from(format!("/dev/rdisk{number}"))
    }
}

/// Runs `diskutil` and returns what it printed, or what it complained about.
fn diskutil(args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new(DISKUTIL)
        .args(args)
        .output()
        .map_err(|e| format!("could not start diskutil: {e}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

type Run<'a> = &'a mut dyn FnMut(&[&str]) -> Result<Vec<u8>, String>;

fn list_with(run: Run<'_>) -> Result<Vec<Disk>, Error> {
    let listing = run(&["list", "-plist"]).map_err(Error::Listing)?;
    let whole: DiskList = read(&listing)?;
    let volume = run(&["info", "-plist", "/"]).map_err(Error::Listing)?;
    let boot = boot_disks(&read(&volume)?);
    let mut disks = Vec::new();
    for identifier in &whole.whole_disks {
        let info = run(&["info", "-plist", identifier]).map_err(Error::Listing)?;
        disks.push(disk_from(&read(&info)?, &boot)?);
    }
    Ok(disks)
}

fn prepare_with(run: Run<'_>, chosen: &Disk) -> Result<(), String> {
    // A disk that is not the one that was listed is never touched.
    let now = list_with(run).map_err(|e| e.to_string())?;
    if !now.iter().any(|disk| disk == chosen) {
        return Err("the disk changed since it was listed; list the disks again".to_owned());
    }
    let node = format!("/dev/disk{}", chosen.number);
    run(&["unmountDisk", &node]).map(|_| ())
}

fn read<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    plist::from_bytes(bytes).map_err(|e| Error::Listing(format!("unreadable diskutil answer: {e}")))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DiskList {
    whole_disks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Info {
    device_identifier: String,
    device_node: Option<String>,
    parent_whole_disk: Option<String>,
    #[serde(default)]
    internal: bool,
    #[serde(default)]
    removable_media: bool,
    bus_protocol: Option<String>,
    virtual_or_physical: Option<String>,
    #[serde(default)]
    size: u64,
    device_block_size: Option<u64>,
    media_name: Option<String>,
    #[serde(rename = "IORegistryEntryName")]
    registry_name: Option<String>,
    #[serde(rename = "APFSPhysicalStores", default)]
    physical_stores: Vec<PhysicalStore>,
}

#[derive(Deserialize)]
struct PhysicalStore {
    #[serde(rename = "APFSPhysicalStore")]
    partition: String,
}

/// The whole disks the running system lives on: the one of the volume `/`, and when that is an
/// APFS container (always, today) the disk its container is stored on.
fn boot_disks(volume: &Info) -> Vec<String> {
    let mut disks = Vec::new();
    disks.extend(volume.parent_whole_disk.clone());
    for store in &volume.physical_stores {
        disks.push(whole_disk_of(&store.partition));
    }
    disks
}

/// `disk0s2` is a partition of `disk0`.
fn whole_disk_of(partition: &str) -> String {
    let digits_end = partition.strip_prefix("disk").map_or(0, |rest| {
        rest.chars().take_while(char::is_ascii_digit).count()
    });
    partition[..4 + digits_end].to_owned()
}

fn disk_number(identifier: &str) -> Option<u32> {
    identifier.strip_prefix("disk")?.parse().ok()
}

fn disk_from(info: &Info, boot: &[String]) -> Result<Disk, Error> {
    let number = disk_number(&info.device_identifier).ok_or_else(|| {
        Error::Listing(format!("unexpected disk name {}", info.device_identifier))
    })?;
    let protocol = info.bus_protocol.as_deref().unwrap_or("Unknown");
    let on_boot_disk = boot.contains(&info.device_identifier);
    Ok(Disk {
        number,
        friendly_name: info
            .media_name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| info.registry_name.clone()),
        // diskutil does not print a serial number.
        serial_number: None,
        device_path: info.device_node.clone(),
        size: info.size,
        // A mounted disk image is how the writer is tried without a stick, like a VHD on Windows.
        bus_type: if protocol == "Disk Image" {
            "File Backed Virtual".to_owned()
        } else {
            protocol.to_owned()
        },
        is_system: on_boot_disk,
        is_boot: on_boot_disk,
        blocked: blocked(info),
    })
}

/// Why a disk is never written whatever its bus: only an external, physical stick of removable
/// media qualifies (a mounted disk image does too, when virtual disks are allowed).
fn blocked(info: &Info) -> Option<String> {
    let disk_image = info.bus_protocol.as_deref() == Some("Disk Image");
    let reason = if info.internal {
        "an internal disk"
    } else if !info.physical_stores.is_empty() || (!disk_image && is_virtual(info)) {
        "an APFS container, not a physical disk"
    } else if !disk_image && !info.removable_media {
        "an external drive, not removable media like a stick"
    } else if info
        .device_block_size
        .is_some_and(|size| size != SECTOR_SIZE)
    {
        "a disk with sectors that are not 512 bytes"
    } else {
        return None;
    };
    Some(reason.to_owned())
}

fn is_virtual(info: &Info) -> bool {
    info.virtual_or_physical.as_deref() == Some("Virtual")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &[u8] = include_bytes!("../fixtures/diskutil/list.plist");
    const BOOT: &[u8] = include_bytes!("../fixtures/diskutil/boot-volume.plist");
    const DISKS: [(&str, &[u8]); 8] = [
        ("disk0", include_bytes!("../fixtures/diskutil/disk0.plist")),
        ("disk3", include_bytes!("../fixtures/diskutil/disk3.plist")),
        (
            "disk4",
            include_bytes!("../fixtures/diskutil/disk4-disk-image.plist"),
        ),
        (
            "disk5",
            include_bytes!("../fixtures/diskutil/disk5-usb-stick.plist"),
        ),
        (
            "disk6",
            include_bytes!("../fixtures/diskutil/disk6-usb-ssd.plist"),
        ),
        (
            "disk7",
            include_bytes!("../fixtures/diskutil/disk7-usb-apfs-container.plist"),
        ),
        (
            "disk8",
            include_bytes!("../fixtures/diskutil/disk8-usb-stick-small.plist"),
        ),
        (
            "disk9",
            include_bytes!("../fixtures/diskutil/disk9-usb-4k-sectors.plist"),
        ),
    ];

    /// A diskutil that answers from the recorded plists and notes what it was asked.
    fn recorded(asked: &mut Vec<Vec<String>>) -> impl FnMut(&[&str]) -> Result<Vec<u8>, String> {
        move |args| {
            asked.push(args.iter().map(|arg| (*arg).to_owned()).collect());
            match args {
                ["list", "-plist"] => Ok(LIST.to_vec()),
                ["info", "-plist", "/"] => Ok(BOOT.to_vec()),
                ["info", "-plist", id] => DISKS
                    .iter()
                    .find(|(name, _)| name == id)
                    .map(|(_, plist)| plist.to_vec())
                    .ok_or_else(|| format!("Could not find disk: {id}")),
                ["unmountDisk", _] => Ok(b"Unmount of all volumes was successful".to_vec()),
                other => Err(format!("unexpected diskutil call {other:?}")),
            }
        }
    }

    fn listed() -> Vec<Disk> {
        list_with(&mut recorded(&mut Vec::new())).expect("recorded plists are readable")
    }

    fn disk(number: u32) -> Disk {
        listed()
            .into_iter()
            .find(|disk| disk.number == number)
            .expect("disk is in the recording")
    }

    #[test]
    fn a_stick_is_read_from_its_plist() {
        let stick = disk(5);
        assert_eq!(
            stick.friendly_name.as_deref(),
            Some("Kingston DataTraveler 3.0")
        );
        assert_eq!(
            (stick.bus_type.as_str(), stick.size),
            ("USB", 31_914_983_424)
        );
        assert_eq!(stick.device_path.as_deref(), Some("/dev/disk5"));
        assert_eq!(stick.size_text(), "31.9 GB");
        assert!(stick.check(29_884_416_000, false).is_ok());
        assert_eq!(listed().len(), 8);
    }

    #[test]
    fn the_boot_disk_and_the_disk_under_its_apfs_container_are_both_the_system() {
        // `/` lives on disk3, an APFS container stored on a partition of disk0.
        for number in [0, 3] {
            let error = disk(number).check(1, true).unwrap_err();
            assert!(
                matches!(&error, Error::Refused { reason, .. } if reason.contains("runs from")),
                "disk{number}: {error}"
            );
        }
        assert_eq!(boot_disks(&read(BOOT).unwrap()), ["disk3", "disk0"]);
    }

    #[test]
    fn only_a_removable_external_physical_usb_disk_may_be_written() {
        // (disk, image size, virtual disks allowed, what the refusal says, or None when allowed)
        let cases = [
            (5, 29_884_416_000, false, None),
            (6, 1, true, Some("not removable media")),
            (7, 1, true, Some("APFS container")),
            (9, 1, true, Some("not 512 bytes")),
            (4, 1, false, Some("virtual disk")),
            (4, 1, true, None),
        ];
        for (number, image_size, allow_virtual, refusal) in cases {
            let result = disk(number).check(image_size, allow_virtual);
            match (result, refusal) {
                (Ok(()), None) => {}
                (Err(Error::Refused { reason, .. }), Some(text)) => {
                    assert!(reason.contains(text), "disk{number}: {reason}");
                }
                (result, refusal) => panic!("disk{number}: {result:?}, expected {refusal:?}"),
            }
        }
    }

    #[test]
    fn an_internal_disk_is_refused_even_when_it_would_pass_every_other_test() {
        let mut internal = disk(5);
        internal.blocked = blocked(&Info {
            internal: true,
            ..info_of(DISKS[3].1)
        });
        let error = internal.check(1, true).unwrap_err();
        assert!(matches!(&error, Error::Refused { reason, .. } if reason == "an internal disk"));
        // The recorded internal disk says the same.
        assert!(matches!(
            disk(0).blocked.as_deref(),
            Some("an internal disk")
        ));
    }

    fn info_of(plist: &[u8]) -> Info {
        read(plist).unwrap()
    }

    #[test]
    fn a_stick_that_is_too_small_is_refused() {
        let error = disk(8).check(29_884_416_000, false).unwrap_err();
        assert!(matches!(error, Error::TooSmall { number: 8, .. }));
    }

    #[test]
    fn preparing_unmounts_the_chosen_disk_and_only_that_one() {
        let mut asked = Vec::new();
        prepare_with(&mut recorded(&mut asked), &disk(5)).unwrap();
        let unmounts: Vec<_> = asked
            .iter()
            .filter(|a| a[0] != "info" && a[0] != "list")
            .collect();
        assert_eq!(
            unmounts,
            [&["unmountDisk".to_owned(), "/dev/disk5".to_owned()]]
        );
    }

    #[test]
    fn a_disk_that_changed_since_it_was_listed_is_not_unmounted() {
        let mut replaced = disk(5);
        replaced.size += 512;
        let mut asked = Vec::new();
        let error = prepare_with(&mut recorded(&mut asked), &replaced).unwrap_err();
        assert!(error.contains("changed since it was listed"), "{error}");
        assert!(asked.iter().all(|call| call[0] != "unmountDisk"));
    }

    #[test]
    fn the_raw_device_is_the_unbuffered_one() {
        assert_eq!(Macos.raw_path(5), PathBuf::from("/dev/rdisk5"));
    }

    #[test]
    fn partitions_belong_to_their_whole_disk() {
        for (partition, whole) in [
            ("disk0s2", "disk0"),
            ("disk12s1s1", "disk12"),
            ("disk3", "disk3"),
        ] {
            assert_eq!(whole_disk_of(partition), whole);
        }
    }

    #[test]
    fn nonsense_from_diskutil_is_an_error_and_not_a_disk() {
        let mut nonsense = |_: &[&str]| Ok(b"not a plist".to_vec());
        assert!(matches!(list_with(&mut nonsense), Err(Error::Listing(_))));
        let mut failing = |_: &[&str]| Err("diskutil failed".to_owned());
        assert!(matches!(list_with(&mut failing), Err(Error::Listing(_))));
    }
}
