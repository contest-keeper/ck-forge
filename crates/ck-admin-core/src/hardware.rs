//! Which physical USB disk a stick is written to, so the Hub can tell that a disk was written
//! more than once. The id is a bookkeeping aid and not a security check: a disk's serial number can
//! be copied by whoever wants to.

use ck_writer::Disk;
use serde::Serialize;

/// Serial numbers that many cheap sticks share, so they say nothing about one disk.
const JUNK_SERIALS: [&str; 3] = ["0123456789ABCDEF", "123456789ABCDEF0", "FFFFFFFFFFFFFFFF"];

/// What the Hub is told about the disk: a hash of its serial number and size, and the last
/// characters of the serial so a person can match it to a stick in hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HardwareId {
    pub fingerprint: String,
    pub serial_hint: String,
}

impl HardwareId {
    /// The id of a disk, or nothing when its serial number is missing or cannot tell one disk from
    /// another. A disk with no id is written as before and is never counted as rewritten.
    #[must_use]
    pub fn of(disk: &Disk) -> Option<Self> {
        let serial: String = disk.serial_number.as_deref()?.trim().to_ascii_uppercase();
        let letters: String = serial.chars().filter(char::is_ascii_alphanumeric).collect();
        let same_everywhere = letters.chars().all(|c| letters.starts_with(c));
        if letters.len() < 4 || same_everywhere || JUNK_SERIALS.contains(&letters.as_str()) {
            return None;
        }
        let fingerprint =
            ck_crypto::sha256_hex(format!("ck-disk-v1|{letters}|{}", disk.size).as_bytes());
        let serial_hint = letters[letters.len() - 4..].to_owned();
        Some(Self {
            fingerprint,
            serial_hint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(serial: Option<&str>, size: u64) -> Disk {
        // A Disk is made only from the Windows listing, so a test reads it the same way.
        let listing = serde_json::json!({
            "Number": 3, "FriendlyName": null, "SerialNumber": serial, "Path": null,
            "Size": size, "BusType": "USB", "IsSystem": false, "IsBoot": false,
        });
        serde_json::from_value(listing).expect("a disk")
    }

    #[test]
    fn a_disk_has_the_same_id_every_time_whatever_the_case_or_padding() {
        let a = HardwareId::of(&disk(Some("  0a1b2c3d4e5f  "), 32_000_000_000)).expect("an id");
        let b = HardwareId::of(&disk(Some("0A1B2C3D4E5F"), 32_000_000_000)).expect("an id");
        assert_eq!(a, b);
        assert_eq!(a.serial_hint, "4E5F");
        assert_eq!(a.fingerprint.len(), 64);
        assert!(
            a.fingerprint
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
        assert_ne!(
            a,
            HardwareId::of(&disk(Some("0A1B2C3D4E5F"), 16_000_000_000)).expect("an id")
        );
        assert_ne!(
            a,
            HardwareId::of(&disk(Some("0A1B2C3D4E5E"), 32_000_000_000)).expect("an id")
        );
    }

    #[test]
    fn a_missing_or_shared_serial_gives_no_id() {
        for serial in [
            None,
            Some(""),
            Some("   "),
            Some("123"),
            Some("0000000000000000"),
            Some("0123456789abcdef"),
            Some("----"),
        ] {
            assert_eq!(HardwareId::of(&disk(serial, 1)), None, "{serial:?}");
        }
    }
}
