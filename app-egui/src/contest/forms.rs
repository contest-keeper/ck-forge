//! The cards with the settings: `ContestForm`, `BrandingForm` and `NetworkForm` of the earlier
//! web window, and the limit of the stick key from `ContestPage`.

use ck_admin_core::{Profile, StickKeyWindow};
use ck_protocol::{
    Bookmark, Contest, LocalDateTime, Platform, RecordingMode, Settings, UploadMode, Wifi,
};
use eframe::egui::{self, Align, Layout, TextFormat, Ui, text::LayoutJob, vec2};

use crate::contest::datetime;
use crate::theme::{Weight, font, tokens};
use crate::widgets::{self, Btn, Input, field};

const BAD_TIME: &str = "Use a date and time like 11/15/2026 09:00 AM";
const NO_SUCH_TIME: &str =
    "This PC's clock does not have this time (skipped for daylight saving, or out of range).";
pub const BAD_END: &str = "Fix the contest end first: use a date and time like 11/15/2026 09:00 AM";

/// Text as it was typed, which is not always what the profile holds.
#[derive(Default)]
pub struct Typed {
    /// One site name per line.
    domains: String,
    pub contest_end: String,
    valid_from: String,
    valid_until: String,
    /// The password is not here: it is typed for each write and never saved in the profile.
    ssid: String,
    home_page: String,
}

impl Typed {
    /// Takes the texts from a profile that was just taken in.
    pub fn read(profile: &Profile) -> Self {
        let settings = &profile.settings;
        let window = profile.stick_key_window.as_ref();
        Self {
            domains: settings.network.allowed_domains.join("\n"),
            contest_end: settings
                .contest
                .end
                .map(|end| datetime::shown(end.as_primitive()))
                .unwrap_or_default(),
            valid_from: window.map_or_else(String::new, |w| datetime::utc_to_shown(&w.valid_from)),
            valid_until: window
                .map_or_else(String::new, |w| datetime::utc_to_shown(&w.valid_until)),
            ssid: settings
                .wifi
                .as_ref()
                .map(|wifi| wifi.ssid.clone())
                .unwrap_or_default(),
            home_page: settings.branding.home_page.clone().unwrap_or_default(),
        }
    }

    /// Whether the typed contest end is empty (none) or a date and time.
    pub fn end_is_valid(&self) -> bool {
        self.contest_end.trim().is_empty() || datetime::parse(&self.contest_end).is_some()
    }
}

/// The sites as the profile keeps them: one per non-empty line.
fn parse_domains(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn optional_text(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The Wi-Fi settings for a typed name: none when it is empty. The password is never set here.
fn wifi_for(ssid: &str) -> Option<Wifi> {
    optional_text(ssid).map(|ssid| Wifi {
        ssid,
        password: None,
    })
}

/// A required number: a whole number of at least `min`, otherwise `min`.
fn whole_number(text: &str, min: u64) -> u64 {
    text.parse().ok().filter(|n| *n >= min).unwrap_or(min)
}

/// An optional number: none when blank, zero or too large.
fn optional_count<N: std::str::FromStr + Default + PartialEq>(text: &str) -> Option<N> {
    text.parse().ok().filter(|n| *n != N::default())
}

/// Cuts typed text to `characters` (the `maxlength` of the earlier window).
fn limit(text: &mut String, characters: usize) {
    if let Some((at, _)) = text.char_indices().nth(characters) {
        text.truncate(at);
    }
}

/// Which recording settings matter for a mode, so only those are shown. Hidden settings keep their
/// values: switching the mode back brings them back as they were.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RecordingField {
    Mode,
    StartDelay,
    SnapshotInterval,
    Upload,
    Retention,
}

fn recording_fields(mode: RecordingMode) -> Vec<RecordingField> {
    use RecordingField as F;
    match mode {
        RecordingMode::Off => vec![F::Mode],
        RecordingMode::Snapshots => vec![
            F::Mode,
            F::StartDelay,
            F::SnapshotInterval,
            F::Upload,
            F::Retention,
        ],
        _ => vec![F::Mode, F::StartDelay, F::Upload, F::Retention],
    }
}

fn mode_name(mode: RecordingMode) -> &'static str {
    match mode {
        RecordingMode::Off => "Off",
        RecordingMode::Snapshots => "Snapshots",
        RecordingMode::Low => "Low",
        RecordingMode::Standard => "Standard",
    }
}

fn mode_label(mode: RecordingMode) -> &'static str {
    match mode {
        RecordingMode::Off => "Off: no recording",
        RecordingMode::Snapshots => "Snapshots: a picture every few seconds",
        RecordingMode::Low => "Low: video at 1 frame a second, half size",
        RecordingMode::Standard => "Standard: video at 2 frames a second, full size",
    }
}

/// As many columns of at least 240 points as fit and there are items for, filled row by row (the
/// CSS grid with `auto-fit`).
fn grid<T: Copy>(ui: &mut Ui, items: &[T], mut add: impl FnMut(&mut Ui, T)) {
    let gap = tokens(ui.ctx()).s.gap;
    let width = ui.available_width();
    let mut columns = 1_u8;
    while usize::from(columns) < items.len()
        && f32::from(columns + 1) * (240.0 + gap) - gap <= width
    {
        columns += 1;
    }
    let cell = (width - gap * f32::from(columns - 1)) / f32::from(columns);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(gap, gap);
        for row in items.chunks(usize::from(columns)) {
            ui.horizontal_top(|ui| {
                for item in row {
                    let top_down = Layout::top_down(Align::Min);
                    ui.allocate_ui_with_layout(vec2(cell, 0.0), top_down, |ui| {
                        ui.set_width(cell);
                        add(ui, *item);
                    });
                }
            });
        }
    });
}

/// A labelled line of text.
fn text_field(ui: &mut Ui, label: &str, hint: Option<&str>, text: &mut String) -> egui::Response {
    field(ui, label, hint, None, |ui, id| {
        Input::new(text).show(ui).labelled_by(id)
    })
}

/// A number field. While it has the keyboard it keeps what is typed; afterwards it shows what the
/// profile holds, so what is seen is what gets saved.
fn number_field(ui: &mut Ui, label: &str, hint: &str, held: String, apply: impl FnOnce(&str)) {
    let key = ui.make_persistent_id(("number", label));
    let mut text = ui.data(|data| data.get_temp(key)).unwrap_or(held);
    let response = text_field(ui, label, Some(hint), &mut text);
    if response.changed() {
        text.retain(|c| c.is_ascii_digit());
        apply(&text);
    }
    // Asked first: the context cannot be read while its data is being written.
    let focused = response.has_focus();
    ui.data_mut(|data| {
        if focused {
            data.insert_temp(key, text);
        } else {
            data.remove::<String>(key);
        }
    });
}

/// The card "Contest".
pub fn contest(ui: &mut Ui, settings: &mut Settings, typed: &mut Typed) {
    widgets::card(ui, |ui| {
        widgets::title(ui, "Contest");
        grid(ui, &[0, 1, 2], |ui, at| match at {
            0 => {
                let hint = "Shown on the stick. Empty uses the profile name.";
                text_field(ui, "Contest name", Some(hint), &mut settings.contest.name);
            }
            1 => contest_id(ui, &mut settings.contest),
            _ => contest_end(ui, &mut settings.contest, typed),
        });
    });
}

fn contest_id(ui: &mut Ui, contest: &mut Contest) {
    let id = contest.contest_id.trim();
    // Codeforces numbers its contests with digits only, and a wrong number gives a stick that opens
    // a page that is not there.
    let wrong = contest.platform == Platform::Codeforces && !id.chars().all(|c| c.is_ascii_digit());
    let error = wrong
        .then_some("Use digits only, like 1985. It is the number in codeforces.com/contest/1985.");
    let hint = "The number in codeforces.com/contest/\u{2026}";
    let name = "Codeforces contest ID";
    field(ui, name, Some(hint), error, |ui, label| {
        Input::new(&mut contest.contest_id)
            .invalid(wrong)
            .show(ui)
            .labelled_by(label);
    });
}

fn contest_end(ui: &mut Ui, contest: &mut Contest, typed: &mut Typed) {
    let valid = typed.end_is_valid();
    let hint = "Use the stick's local time for uploads after the contest.";
    let error = (!valid).then_some(BAD_TIME);
    field(ui, "Contest end", Some(hint), error, |ui, label| {
        let text = &mut typed.contest_end;
        if datetime::input(ui, label, "the contest end", text, !valid) {
            if text.trim().is_empty() {
                contest.end = None;
            } else if let Some(end) = datetime::parse(text) {
                contest.end = Some(LocalDateTime::from(end));
            }
        }
    });
}

/// The closed card "Team and site (optional)".
pub fn team_and_site(ui: &mut Ui, settings: &mut Settings) {
    widgets::details(ui, "Team and site (optional)", |ui| {
        grid(ui, &[0, 1], |ui, at| {
            if at == 0 {
                let hint = "Becomes {team} in the computer name.";
                text_field(ui, "Team", Some(hint), &mut settings.team);
            } else {
                let hint = "Becomes {site} in the computer name.";
                text_field(ui, "Site", Some(hint), &mut settings.site);
            }
        });
    });
}

/// The card that limits when the stick key works.
pub fn key_window(ui: &mut Ui, profile: &mut Profile, typed: &mut Typed) {
    widgets::card(ui, |ui| {
        let mut limited = profile.stick_key_window.is_some();
        let label = "Limit when the stick key works";
        if widgets::check_suffixed(ui, &mut limited, label, " (optional)").changed() {
            // Turning the limit on starts from now to a day later, so there is something to change.
            profile.stick_key_window = limited.then(|| {
                let now = time::OffsetDateTime::now_utc();
                let (valid_from, valid_until) = datetime::default_window(now);
                typed.valid_from = datetime::utc_to_shown(&valid_from);
                typed.valid_until = datetime::utc_to_shown(&valid_until);
                StickKeyWindow {
                    valid_from,
                    valid_until,
                }
            });
        }
        widgets::hint(
            ui,
            "Off: the key of each stick works until you retire it. On: it only works between the two times below, which needs the contest ID above.",
        );
        if let Some(window) = &mut profile.stick_key_window {
            grid(ui, &[0, 1], |ui, at| {
                if at == 0 {
                    let from = &mut window.valid_from;
                    key_time(ui, "Valid from", &mut typed.valid_from, from);
                } else {
                    let until = &mut window.valid_until;
                    key_time(ui, "Valid until", &mut typed.valid_until, until);
                }
            });
            stored_times(ui, window);
        }
        widgets::hint(
            ui,
            "To re-key a stick, make a new stick with the new contest profile.",
        );
    });
}

fn key_time(ui: &mut Ui, label: &str, typed: &mut String, stored: &mut String) {
    let error = if typed.trim().is_empty() || datetime::shown_to_utc(typed).is_some() {
        None
    } else if datetime::parse(typed).is_none() {
        Some(BAD_TIME)
    } else {
        Some(NO_SUCH_TIME)
    };
    field(ui, label, None, error, |ui, id| {
        if datetime::input(ui, id, label, typed, error.is_some()) {
            // Text that is no time leaves the field empty, which the check of the profile refuses:
            // an older time is never saved in its place.
            *stored = datetime::shown_to_utc(typed).unwrap_or_default();
        }
    });
}

/// The hint with the two UTC times in the font of file names.
fn stored_times(ui: &mut Ui, window: &StickKeyWindow) {
    let t = tokens(ui.ctx());
    let or_dots = |text: &str| {
        if text.is_empty() {
            "\u{2026}".to_owned()
        } else {
            text.to_owned()
        }
    };
    let format = |weight, size| TextFormat {
        font_id: font(weight, size),
        color: t.c.muted,
        valign: Align::BOTTOM,
        ..TextFormat::default()
    };
    let plain = format(Weight::Regular, t.s.hint);
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let lead = format!(
        "Times are on this PC's clock ({}). The key stores them as UTC: ",
        datetime::clock_name()
    );
    job.append(&lead, 0.0, plain.clone());
    let from = or_dots(&window.valid_from);
    let times = format!("{from} to {}", or_dots(&window.valid_until));
    job.append(&times, 0.0, format(Weight::Mono, t.rem(0.85)));
    job.append(".", 0.0, plain);
    ui.add(egui::Label::new(job).wrap());
}

/// The first part of "What contestants see", above the wallpaper.
pub fn names(ui: &mut Ui, settings: &mut Settings, typed: &mut Typed) {
    let branding = &mut settings.branding;
    grid(ui, &[0, 1, 2, 3], |ui, at| match at {
        0 => {
            text_field(ui, "Display name", None, &mut branding.display_name);
        }
        1 => {
            let hint = "Placeholders: {team} {site} {n}";
            let name = &mut branding.computer_name_pattern;
            text_field(ui, "Computer name", Some(hint), name);
        }
        2 => {
            let hint = "At most 11 characters.";
            if text_field(ui, "USB label", Some(hint), &mut branding.usb_label).changed() {
                limit(&mut branding.usb_label, 11);
            }
        }
        _ => {
            if text_field(ui, "Home page", None, &mut typed.home_page).changed() {
                branding.home_page = optional_text(&typed.home_page);
            }
        }
    });
}

/// The part of "What contestants see" below the wallpaper.
pub fn welcome_and_bookmarks(ui: &mut Ui, settings: &mut Settings) {
    let branding = &mut settings.branding;
    let hint = "Shown at login.";
    field(ui, "Welcome text", Some(hint), None, |ui, id| {
        Input::new(&mut branding.welcome_text)
            .multiline(3)
            .show(ui)
            .labelled_by(id);
    });
    ui.add_space(4.0);
    widgets::subtitle(ui, "Bookmarks");
    let mut remove = None;
    for (at, bookmark) in branding.bookmarks.iter_mut().enumerate() {
        ui.push_id(("bookmark", at), |ui| {
            if bookmark_row(ui, bookmark) {
                remove = Some(at);
            }
        });
    }
    if let Some(at) = remove {
        branding.bookmarks.remove(at);
    }
    if Btn::new("Add a bookmark").small().show(ui).clicked() {
        // Empty, for the admin to fill in; the check reports it until it has a title and a link.
        branding.bookmarks.push(Bookmark {
            title: String::new(),
            url: String::new(),
        });
    }
}

/// Title and address in the proportion 1 to 2, then the button. True to remove the bookmark.
fn bookmark_row(ui: &mut Ui, bookmark: &mut Bookmark) -> bool {
    let t = tokens(ui.ctx());
    let remove = ui.painter().layout_no_wrap(
        "Remove".to_owned(),
        font(Weight::Semibold, t.rem(0.85)),
        t.c.text,
    );
    // The padding of a small button.
    let button = remove.size().x + 24.0;
    let third = (ui.available_width() - button - 2.0 * t.s.gap) / 3.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.s.gap;
        let title = Input::new(&mut bookmark.title)
            .placeholder("Title")
            .width(third)
            .show(ui);
        name_for_readers(ui, &title, "Bookmark title");
        let url = Input::new(&mut bookmark.url)
            .placeholder("https://\u{2026}")
            .width(2.0 * third)
            .show(ui);
        name_for_readers(ui, &url, "Bookmark address");
        Btn::new("Remove").small().show(ui).clicked()
    })
    .inner
}

/// The name of a field that has no label on the screen.
fn name_for_readers(ui: &Ui, response: &egui::Response, name: &str) {
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_label(name));
}

/// The inside of "Network, Hub and screen recording".
pub fn network(ui: &mut Ui, settings: &mut Settings, typed: &mut Typed) {
    widgets::title(ui, "Network and Hub");
    let hint = "One per line, such as kilonova.ro. Empty allows no extra site.";
    field(ui, "Allowed sites", Some(hint), None, |ui, id| {
        let sites = Input::new(&mut typed.domains)
            .multiline(4)
            .show(ui)
            .labelled_by(id);
        if sites.changed() {
            settings.network.allowed_domains = parse_domains(&typed.domains);
        }
    });
    let held = settings.api.heartbeat_s.to_string();
    let hint = "Sticks made while signed in to the Hub send heartbeats to it.";
    number_field(ui, "Heartbeat every (seconds)", hint, held, |text| {
        settings.api.heartbeat_s = whole_number(text, 1);
    });
    between(ui);
    widgets::title(ui, "Bluetooth and Wi-Fi");
    let label = "Block Bluetooth on the sticks";
    widgets::check(ui, &mut settings.block_bluetooth, label);
    let label = "Block other USB storage devices on the sticks";
    widgets::check(ui, &mut settings.block_usb_storage, label);
    let hint = "Sticks join this network by themselves at every boot, with no login on the stick. It works for a network with a WPA2 password or an open one, not for a network that asks for a user name or a web page login. Empty means none. The password is asked on the Make page and is never saved in the profile.";
    if text_field(ui, "Wi-Fi name (SSID)", Some(hint), &mut typed.ssid).changed() {
        limit(&mut typed.ssid, 32);
        settings.wifi = wifi_for(&typed.ssid);
    }
    between(ui);
    widgets::title(ui, "Screen recording");
    recording(ui, settings);
}

/// 22 points between two sections, with a line 18 points above the next title.
fn between(ui: &mut Ui) {
    let gap = tokens(ui.ctx()).s.gap;
    ui.add_space(22.0 - gap);
    widgets::hairline(ui);
    ui.add_space(18.0 - gap);
}

fn recording(ui: &mut Ui, settings: &mut Settings) {
    let recording = &mut settings.recording;
    let fields = recording_fields(recording.mode);
    grid(ui, &fields, |ui, shown| match shown {
        RecordingField::Mode => {
            let options = RecordingMode::ALL.map(|mode| (mode, mode_name(mode)));
            let hint = mode_label(recording.mode);
            field(ui, "Mode", Some(hint), None, |ui, id| {
                widgets::select(ui, "recording-mode", &mut recording.mode, &options)
                    .labelled_by(id);
            });
        }
        RecordingField::StartDelay => {
            let held = recording.start_delay_s.to_string();
            let hint = "Keeps boot and login fast.";
            number_field(ui, "Start after (seconds)", hint, held, |text| {
                recording.start_delay_s = whole_number(text, 0);
            });
        }
        RecordingField::SnapshotInterval => {
            let held = recording.snapshot_interval_s.map(|n| n.to_string());
            let held = held.unwrap_or_default();
            let hint = "Empty uses 10.";
            number_field(ui, "Snapshot every (seconds)", hint, held, |text| {
                recording.snapshot_interval_s = optional_count(text);
            });
        }
        RecordingField::Upload => {
            let options = [
                (UploadMode::Keep, "Keep on the stick"),
                (UploadMode::AfterContest, "Upload after the contest"),
                (UploadMode::DuringContest, "Upload during the contest"),
            ];
            field(ui, "Recording upload", None, None, |ui, id| {
                widgets::select(ui, "recording-upload", &mut recording.upload, &options)
                    .labelled_by(id);
            });
        }
        RecordingField::Retention => {
            let held = recording.retention_days.map(|n| n.to_string());
            let held = held.unwrap_or_default();
            let hint = "Leave empty to keep recordings until you collect them.";
            number_field(ui, "Keep on the stick (days)", hint, held, |text| {
                recording.retention_days = optional_count(text);
            });
        }
    });
    if recording.mode == RecordingMode::Off {
        widgets::hint(ui, "Nothing is recorded, so there is nothing else to set.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sites_are_one_per_line_without_blanks() {
        assert_eq!(
            parse_domains(" codeforces.com \n\n  example.org\n"),
            ["codeforces.com", "example.org"]
        );
    }

    #[test]
    fn only_the_recording_settings_of_the_mode_are_shown() {
        assert_eq!(recording_fields(RecordingMode::Off).len(), 1);
        let snapshot = RecordingField::SnapshotInterval;
        assert!(recording_fields(RecordingMode::Snapshots).contains(&snapshot));
        let low = recording_fields(RecordingMode::Low);
        assert!(low.len() == 4 && !low.contains(&snapshot));
    }

    #[test]
    fn an_empty_wifi_name_means_no_wifi_and_the_password_is_never_set() {
        assert_eq!(wifi_for("  "), None);
        let wifi = wifi_for(" venue ").expect("a network");
        assert_eq!(wifi.ssid, "venue");
        assert!(wifi.password.is_none());
    }

    #[test]
    fn numbers_fall_back_and_text_is_cut_at_its_length() {
        assert_eq!(whole_number("", 1), 1);
        assert_eq!(whole_number("0", 1), 1);
        assert_eq!(whole_number("90", 1), 90);
        assert_eq!(optional_count::<u32>("0"), None);
        assert_eq!(optional_count::<u32>("7"), Some(7));
        let mut label = "\u{103}".repeat(12);
        limit(&mut label, 11);
        assert_eq!(label.chars().count(), 11);
    }

    #[test]
    fn the_contest_end_is_empty_or_a_date_and_time() {
        let mut typed = Typed::default();
        assert!(typed.end_is_valid());
        typed.contest_end = "2026-11-15T09:00".to_owned();
        assert!(typed.end_is_valid());
        typed.contest_end = "tomorrow".to_owned();
        assert!(!typed.end_is_valid());
    }
}
