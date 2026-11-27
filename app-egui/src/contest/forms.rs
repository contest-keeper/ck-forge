//! The cards with the settings: `ContestForm`, `BrandingForm` and `NetworkForm` of the earlier
//! web window, and the limit of the stick key from `ContestPage`.

use ck_admin_core::Profile;
use ck_protocol::{Bookmark, Contest, Platform, RecordingMode, Settings, UploadMode, Wifi};
use eframe::egui::{self, Align, Layout, Ui, vec2};

use crate::theme::{Weight, font, tokens};
use crate::widgets::{self, Btn, Input, field};

/// Text as it was typed, which is not always what the profile holds.
#[derive(Default)]
pub struct Typed {
    /// One site name per line.
    domains: String,
    /// The password is not here: it is typed for each write and never saved in the profile.
    ssid: String,
}

impl Typed {
    /// Takes the texts from a profile that was just taken in.
    pub fn read(profile: &Profile) -> Self {
        let settings = &profile.settings;
        Self {
            domains: settings.network.allowed_domains.join(
                "
",
            ),
            ssid: settings
                .wifi
                .as_ref()
                .map(|wifi| wifi.ssid.clone())
                .unwrap_or_default(),
        }
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
pub fn contest(ui: &mut Ui, settings: &mut Settings) {
    let was = (
        settings.contest.platform,
        settings.contest.contest_id.clone(),
    );
    widgets::card(ui, |ui| {
        widgets::title(ui, "Contest");
        grid(ui, &[0, 1, 2], |ui, at| match at {
            0 => {
                let hint = "Shown on the stick. Empty uses the profile name.";
                text_field(ui, "Contest name", Some(hint), &mut settings.contest.name);
            }
            1 => platform_field(ui, &mut settings.contest),
            _ => contest_id(ui, &mut settings.contest),
        });
    });
    // The contest's page joins the bookmarks and becomes the home page, for the admin to change.
    ck_admin_core::follow_contest(settings, was.0, &was.1);
}

/// The judge the contest is on. A platform an older profile has is kept in the list.
fn platform_field(ui: &mut Ui, contest: &mut Contest) {
    let mut options = vec![
        (Platform::Codeforces, "Codeforces"),
        (Platform::Vjudge, "VJudge"),
    ];
    for (platform, name) in [(Platform::Kilonova, "Kilonova"), (Platform::Other, "Other")] {
        if contest.platform == platform {
            options.push((platform, name));
        }
    }
    let hint = "Adds the contest page to the bookmarks as the home page. You can change both.";
    field(ui, "Platform", Some(hint), None, |ui, id| {
        widgets::select(ui, "contest-platform", &mut contest.platform, &options).labelled_by(id);
    });
}

fn contest_id(ui: &mut Ui, contest: &mut Contest) {
    let id = contest.contest_id.trim();
    // Codeforces and VJudge number their contests with digits only, and a wrong number gives a
    // stick that opens a page that is not there.
    let numbered = matches!(contest.platform, Platform::Codeforces | Platform::Vjudge);
    let wrong = numbered && !id.chars().all(|c| c.is_ascii_digit());
    let (site, name) = match contest.platform {
        Platform::Vjudge => ("vjudge.net/contest/", "VJudge contest ID"),
        _ => ("codeforces.com/contest/", "Codeforces contest ID"),
    };
    let error = wrong.then(|| format!("Use digits only, like 1985. It is the number in {site}1985."));
    let hint = format!("The number in {site}\u{2026}");
    field(ui, name, Some(hint.as_str()), error.as_deref(), |ui, label| {
        Input::new(&mut contest.contest_id)
            .invalid(wrong)
            .show(ui)
            .labelled_by(label);
    });
}

/// The closed card at the end of the page: what makes the name of each computer.
pub fn team_and_site(ui: &mut Ui, settings: &mut Settings) {
    widgets::details(ui, "Team and site (optional)", |ui| {
        grid(ui, &[0, 1, 2], |ui, at| match at {
            0 => {
                let hint = "Becomes {team} in the computer name.";
                text_field(ui, "Team", Some(hint), &mut settings.team);
            }
            1 => {
                let hint = "Becomes {site} in the computer name.";
                text_field(ui, "Site", Some(hint), &mut settings.site);
            }
            _ => {
                let hint = "How the stick names itself: {team} {site} {n}.";
                let name = &mut settings.branding.computer_name_pattern;
                text_field(ui, "Computer name", Some(hint), name);
            }
        });
    });
}

/// The welcome text and the bookmarks. The house beside a bookmark makes it the home page.
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
    widgets::hint(ui, "The house marks the page the browser opens first.");
    let mut remove = None;
    for at in 0..branding.bookmarks.len() {
        let was_home = is_home(branding.home_page.as_deref(), &branding.bookmarks[at].url);
        let before = branding.bookmarks[at].url.clone();
        ui.push_id(("bookmark", at), |ui| {
            let row = bookmark_row(ui, &mut branding.bookmarks[at], was_home);
            remove = remove.or(row.remove.then_some(at));
            if row.toggle_home {
                let url = &branding.bookmarks[at].url;
                branding.home_page = if was_home { None } else { optional_text(url) };
            }
        });
        // Typing in the home bookmark moves the home page with it.
        let url = &branding.bookmarks[at].url;
        if was_home && *url != before {
            branding.home_page = optional_text(url);
        }
    }
    if let Some(at) = remove {
        if is_home(branding.home_page.as_deref(), &branding.bookmarks[at].url) {
            branding.home_page = None;
        }
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

/// Whether a bookmark's address is the home page. An empty address never is.
fn is_home(home: Option<&str>, url: &str) -> bool {
    !url.trim().is_empty() && home == Some(url.trim())
}

/// What a click in a bookmark row asked for.
struct Row {
    remove: bool,
    toggle_home: bool,
}

/// The house button: filled when the bookmark is the home page.
fn house(ui: &mut Ui, on: bool) -> egui::Response {
    let t = tokens(ui.ctx());
    let size = t.s.button_small;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
    let color = if on { t.c.accent } else { t.c.muted };
    let c = rect.center();
    let stroke = egui::Stroke::new(1.6, color);
    let painter = ui.painter();
    let roof = vec![
        c + vec2(-8.0, -1.0),
        c + vec2(0.0, -8.0),
        c + vec2(8.0, -1.0),
    ];
    let body = egui::Rect::from_min_max(c + vec2(-5.5, -1.0), c + vec2(5.5, 7.0));
    if on {
        painter.rect_filled(body, 1.0, color);
    } else {
        painter.rect_stroke(body, 1.0, stroke, egui::StrokeKind::Inside);
    }
    painter.line(roof, stroke);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, "Home page"));
    response.on_hover_text("Home page")
}

/// House, title and address in the proportion 1 to 2, then the button.
fn bookmark_row(ui: &mut Ui, bookmark: &mut Bookmark, home: bool) -> Row {
    let t = tokens(ui.ctx());
    let remove = ui.painter().layout_no_wrap(
        "Remove".to_owned(),
        font(Weight::Semibold, t.rem(0.85)),
        t.c.text,
    );
    // The padding of a small button.
    let button = remove.size().x + 24.0;
    let third = (ui.available_width() - button - t.s.button_small - 3.0 * t.s.gap) / 3.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.s.gap;
        let toggle_home = house(ui, home).clicked();
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
        let remove = Btn::new("Remove").small().show(ui).clicked();
        Row {
            remove,
            toggle_home,
        }
    })
    .inner
}

/// The name of a field that has no label on the screen.
fn name_for_readers(ui: &Ui, response: &egui::Response, name: &str) {
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_label(name));
}

/// The page "Connectivity": the sites, Wi-Fi, Bluetooth and USB storage.
pub fn connectivity(ui: &mut Ui, settings: &mut Settings, typed: &mut Typed) {
    widgets::card(ui, |ui| {
        widgets::title(ui, "Allowed sites");
        let hint = "One per line, such as kilonova.ro. Empty allows no extra site.";
        field(ui, "Sites", Some(hint), None, |ui, id| {
            let sites = Input::new(&mut typed.domains)
                .multiline(4)
                .show(ui)
                .labelled_by(id);
            if sites.changed() {
                settings.network.allowed_domains = parse_domains(&typed.domains);
            }
        });
    });
    widgets::card(ui, |ui| {
        widgets::title(ui, "Wi-Fi");
        let hint = "WPA2 or open networks only. Empty means none. The password is asked when you write sticks and is never saved.";
        if text_field(ui, "Network name (SSID)", Some(hint), &mut typed.ssid).changed() {
            limit(&mut typed.ssid, 32);
            settings.wifi = wifi_for(&typed.ssid);
        }
    });
    widgets::card(ui, |ui| {
        widgets::title(ui, "Devices");
        let label = "Block Bluetooth on the sticks";
        widgets::check(ui, &mut settings.block_bluetooth, label);
        let label = "Block the contestant's own USB drives and disks";
        widgets::check(ui, &mut settings.block_usb_storage, label);
    });
}

/// The page "Screen recording".
pub fn recording_page(ui: &mut Ui, settings: &mut Settings) {
    widgets::card(ui, |ui| {
        widgets::title(ui, "Screen recording");
        recording(ui, settings);
    });
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
    fn only_a_filled_in_bookmark_address_can_be_the_home_page() {
        assert!(is_home(Some("https://a.example"), " https://a.example "));
        assert!(!is_home(Some("https://a.example"), "https://b.example"));
        assert!(!is_home(Some(""), ""));
        assert!(!is_home(None, "https://a.example"));
    }
}
