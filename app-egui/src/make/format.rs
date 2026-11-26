//! The words for sizes, times and sticks, as `format.ts` of the earlier window had them.

use std::collections::BTreeSet;
use std::path::Path;

use ck_admin_core::HubStick;

/// Decimal units, like the box of a USB stick: 29884416000 is "29.9 GB".
pub fn bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    // Only for showing a size: a precision loss in the last digits does not matter.
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A time span as minutes and seconds (`4:12`), or with hours (`1:02:05`).
pub fn duration(seconds: u64) -> String {
    let (hours, minutes, rest) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
    }
}

/// A whole percent from 0 to 100; 0 when the total is not known yet.
pub fn percent(done: u64, total: u64) -> u64 {
    (done.min(total).saturating_mul(100))
        .checked_div(total)
        .unwrap_or(0)
}

/// [`percent`] as the part of a bar that is filled.
pub fn fraction(done: u64, total: u64) -> f32 {
    f32::from(u8::try_from(percent(done, total)).unwrap_or(100)) / 100.0
}

/// The name of a file without its folders.
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The text the admin types to confirm erasing disks: "erase 3", or "erase 3 4" for two.
pub fn erase_confirmation(disks: &BTreeSet<u32>) -> String {
    let numbers: Vec<String> = disks.iter().map(ToString::to_string).collect();
    format!("erase {}", numbers.join(" "))
}

/// What an admin calls a stick: its team, site and machine number, never its id.
pub fn stick_label(stick: &HubStick) -> String {
    let machine = stick
        .machine_number
        .map(|number| format!("machine {number}"));
    let parts: Vec<&str> = [Some(&stick.team), Some(&stick.site), machine.as_ref()]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .filter(|part| !part.trim().is_empty())
        .collect();
    if parts.is_empty() {
        "A stick without a name".to_owned()
    } else {
        parts.join(" \u{b7} ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_times_read_like_the_earlier_window() {
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(29_884_416_000), "29.9 GB");
        assert_eq!(duration(252), "4:12");
        assert_eq!(duration(3725), "1:02:05");
        assert_eq!((percent(5, 0), percent(1, 3), percent(9, 3)), (0, 33, 100));
    }

    #[test]
    fn the_confirmation_names_every_disk_in_order() {
        assert_eq!(erase_confirmation(&BTreeSet::from([4, 1])), "erase 1 4");
    }

    #[test]
    fn a_stick_is_called_by_team_site_and_machine() {
        let mut stick: HubStick = serde_json::from_str(
            r#"{"stick_id":"03c9f556","team":"T","site":" ","machine_number":2}"#,
        )
        .expect("stick");
        assert_eq!(stick_label(&stick), "T \u{b7} machine 2");
        stick.team.clear();
        stick.machine_number = None;
        assert_eq!(stick_label(&stick), "A stick without a name");
    }
}
