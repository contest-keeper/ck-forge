//! The profile editor: the settings every stick of a contest gets. It edits one profile file and
//! shows what `ck-admin-core` says is wrong with it, as the Svelte window does.

use std::path::PathBuf;

use crate::contest::hub_profiles::Outcome;
use crate::state::AppState;
use base64::Engine;
use ck_admin_core::{Profile, add_wallpaper, wallpaper_data_url};
use ck_protocol::{Bookmark, LocalDateTime, RecordingMode, UploadMode, Wifi};

#[derive(Default)]
pub struct Screen {
    /// The file the profile is read from and saved to, as typed.
    path: String,
    /// The profile and the file that the typed texts were last read from.
    revision: Option<u64>,
    file: Option<PathBuf>,
    /// The profile file, picture name and picture revision the preview was made from.
    pictured: Option<(PathBuf, String, u64)>,
    /// One site name per line, kept as typed until it is applied to the profile.
    domains: String,
    /// The contest end as typed, `2026-11-15T09:00`, until it is applied to the profile.
    contest_end: String,
    /// The Wi-Fi name as typed. The password is not here: it is typed for each write and never
    /// saved in the profile.
    ssid: String,
    wallpaper_source: String,
    /// The wallpaper as a picture, made once when it changes and not on every frame.
    preview: Option<eframe::egui::TextureHandle>,
    hub: crate::contest::hub_profiles::Panel,
    /// What the last open, save or picture said.
    message: String,
}

/// The sites as the profile keeps them: one per non-empty line.
fn parse_domains(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Which recording settings matter for a mode, so only those are shown. Hidden settings keep their
/// values: switching the mode back brings them back as they were.
#[derive(Debug, PartialEq, Eq)]
struct RecordingFields {
    timing: bool,
    snapshot_interval: bool,
    keeping: bool,
}

fn recording_fields(mode: RecordingMode) -> RecordingFields {
    RecordingFields {
        timing: mode != RecordingMode::Off,
        snapshot_interval: mode == RecordingMode::Snapshots,
        keeping: mode != RecordingMode::Off,
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

fn upload_label(upload: UploadMode) -> &'static str {
    match upload {
        UploadMode::AfterContest => "Upload after the contest",
        UploadMode::DuringContest => "Upload during the contest",
        _ => "Keep on the stick",
    }
}

/// The Wi-Fi settings for a typed name: none when it is empty. The password is never set here.
fn wifi_for(ssid: &str) -> Option<Wifi> {
    let ssid = ssid.trim();
    (!ssid.is_empty()).then(|| Wifi {
        ssid: ssid.to_owned(),
        password: None,
    })
}

/// A number that may be left out: a box to turn it on, then its value.
fn optional_count(ui: &mut eframe::egui::Ui, label: &str, value: &mut Option<u64>, default: u64) {
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        ui.checkbox(&mut on, label);
        if on {
            let number = value.get_or_insert(default);
            ui.add(eframe::egui::DragValue::new(number).range(1..=u64::from(u32::MAX)));
        } else {
            *value = None;
        }
    });
}

fn recording_ui(ui: &mut eframe::egui::Ui, recording: &mut ck_protocol::Recording) {
    ui.label("Screen recording");
    eframe::egui::ComboBox::from_label("Mode")
        .selected_text(mode_label(recording.mode))
        .show_ui(ui, |ui| {
            for mode in RecordingMode::ALL {
                ui.selectable_value(&mut recording.mode, mode, mode_label(mode));
            }
        });
    let fields = recording_fields(recording.mode);
    if !fields.timing {
        ui.label("Nothing is recorded, so there is nothing else to set.");
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Start after (seconds)");
        ui.add(eframe::egui::DragValue::new(&mut recording.start_delay_s));
    });
    if fields.snapshot_interval {
        optional_count(
            ui,
            "Snapshot every N seconds (10 when off)",
            &mut recording.snapshot_interval_s,
            10,
        );
    }
    if fields.keeping {
        eframe::egui::ComboBox::from_label("Recording upload")
            .selected_text(upload_label(recording.upload))
            .show_ui(ui, |ui| {
                for upload in [
                    UploadMode::Keep,
                    UploadMode::AfterContest,
                    UploadMode::DuringContest,
                ] {
                    ui.selectable_value(&mut recording.upload, upload, upload_label(upload));
                }
            });
        let mut days = recording.retention_days.map(u64::from);
        optional_count(ui, "Delete recordings after N days", &mut days, 30);
        recording.retention_days = days.and_then(|days| u32::try_from(days).ok());
    }
}

fn bookmarks_ui(ui: &mut eframe::egui::Ui, bookmarks: &mut Vec<Bookmark>) {
    let mut remove = None;
    for (at, bookmark) in bookmarks.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut bookmark.title);
            ui.text_edit_singleline(&mut bookmark.url);
            if ui.button("Remove").clicked() {
                remove = Some(at);
            }
        });
    }
    if let Some(at) = remove {
        bookmarks.remove(at);
    }
    if ui.button("Add a bookmark").clicked() {
        bookmarks.push(new_bookmark());
    }
}

/// An empty bookmark for the admin to fill in; the check reports it until it has a title and a link.
fn new_bookmark() -> Bookmark {
    Bookmark {
        title: String::new(),
        url: String::new(),
    }
}

/// The contest end, the Wi-Fi name and the recording settings.
fn contest_end_wifi_recording_ui(
    ui: &mut eframe::egui::Ui,
    profile: &mut Profile,
    contest_end: &mut String,
    ssid: &mut String,
) {
    ui.horizontal(|ui| {
        ui.label("Contest end");
        if ui.text_edit_singleline(contest_end).changed() {
            match contest_end.trim() {
                "" => profile.settings.contest.end = None,
                text => {
                    if let Ok(end) = text.parse::<LocalDateTime>() {
                        profile.settings.contest.end = Some(end);
                    }
                }
            }
        }
        if !end_is_valid(contest_end) {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "use a date and time like 2026-11-15T09:00",
            );
        }
    });
    ui.horizontal(|ui| {
        ui.label("Wi-Fi name (SSID)");
        if ui.text_edit_singleline(ssid).changed() {
            profile.settings.wifi = wifi_for(ssid);
        }
    });
    ui.label(
        "Sticks join this Wi-Fi at every boot. The password is asked when you write sticks and is never saved in the profile.",
    );
    recording_ui(ui, &mut profile.settings.recording);
}

/// The longest side of the preview picture, so a large wallpaper never exceeds a texture size.
const PREVIEW_SIDE: u32 = 1024;

/// The picture inside a `data:<type>;base64,<data>` address, made small enough to show.
fn picture_of(data_url: &str) -> Option<eframe::egui::ColorImage> {
    let (_, encoded) = data_url.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let mut picture = image::load_from_memory(&bytes).ok()?;
    // `thumbnail` also enlarges, which a small picture does not need.
    if picture.width() > PREVIEW_SIDE || picture.height() > PREVIEW_SIDE {
        picture = picture.thumbnail(PREVIEW_SIDE, PREVIEW_SIDE);
    }
    let picture = picture.to_rgba8();
    let size = [picture.width() as usize, picture.height() as usize];
    Some(eframe::egui::ColorImage::from_rgba_unmultiplied(
        size,
        picture.as_raw(),
    ))
}

/// Whether the typed contest end is empty (none) or a date and time.
fn end_is_valid(text: &str) -> bool {
    text.trim().is_empty() || text.parse::<LocalDateTime>().is_ok()
}

const BAD_END: &str = "Fix the contest end first: use a date and time like 2026-11-15T09:00";

impl Screen {
    /// Called every frame, whatever page is shown: takes what the Hub answered, follows the shared
    /// state and says whether the profile may be saved now.
    pub fn poll(&mut self, ctx: &eframe::egui::Context, state: &mut AppState) {
        match self.hub.receive() {
            Some(Outcome::Loaded(loaded)) => {
                state.use_profile(loaded.profile, Some(loaded.path));
                "Loaded from the Hub".clone_into(&mut self.message);
            }
            Some(Outcome::Saved(id)) => {
                if let Some(profile) = &mut state.profile {
                    profile.hub_id = id;
                }
            }
            Some(Outcome::Assigned(_)) | None => {}
        }
        self.follow(ctx, state);
        // A save now would write the file a Hub request is writing, or leave typed text out.
        state.blocked = if self.hub.busy() {
            Some("Wait until the Hub request has ended.".to_owned())
        } else {
            (!end_is_valid(&self.contest_end)).then(|| BAD_END.to_owned())
        };
    }

    /// Reads the typed texts again when another profile or file is being edited.
    fn follow(&mut self, ctx: &eframe::egui::Context, state: &AppState) {
        if self.revision != Some(state.profile_revision) {
            self.revision = Some(state.profile_revision);
            if let Some(profile) = &state.profile {
                self.read(profile);
            }
            self.file = None;
            self.path.clear();
        }
        if self.file != state.profile_path {
            self.file.clone_from(&state.profile_path);
            if let Some(file) = &self.file {
                self.path = file.display().to_string();
            }
        }
        let wanted = state.profile_path.as_ref().and_then(|file| {
            let name = state
                .profile
                .as_ref()?
                .settings
                .branding
                .wallpaper
                .clone()?;
            Some((file.clone(), name, state.wallpaper_revision))
        });
        if self.pictured != wanted {
            self.pictured = wanted;
            self.refresh_preview(ctx);
        }
    }

    fn read(&mut self, profile: &Profile) {
        self.domains = profile.settings.network.allowed_domains.join("\n");
        self.contest_end = profile
            .settings
            .contest
            .end
            .map(|end| end.to_string())
            .unwrap_or_default();
        self.ssid = profile
            .settings
            .wifi
            .as_ref()
            .map(|wifi| wifi.ssid.clone())
            .unwrap_or_default();
    }

    pub fn show(&mut self, ui: &mut eframe::egui::Ui, state: &mut AppState) {
        // A request to the Hub is running: nothing here can change until it has ended.
        if self.hub.busy() {
            ui.disable();
        }
        ui.horizontal(|ui| {
            ui.label("File");
            ui.text_edit_singleline(&mut self.path);
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("New").clicked() {
                state.use_profile(Profile::template(), None);
                "A new profile".clone_into(&mut self.message);
            }
            if ui.button("Open").clicked() {
                self.open(state);
            }
            if ui.button("Save").clicked() {
                self.save(state);
            }
            if ui.button("Save in the Forge folder").clicked() {
                self.save_in_forge_folder(state);
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        self.hub_ui(ui, state);
        self.follow(ui.ctx(), state);
        let Some(profile) = &mut state.profile else {
            return;
        };
        ui.separator();
        eframe::egui::Grid::new("profile")
            .num_columns(2)
            .show(ui, |ui| {
                let settings = &mut profile.settings;
                for (label, value) in [
                    ("Name", &mut profile.name),
                    ("Contest", &mut settings.contest.name),
                    ("Contest ID", &mut settings.contest.contest_id),
                    ("Team", &mut settings.team),
                    ("Site", &mut settings.site),
                    (
                        "Computer name",
                        &mut settings.branding.computer_name_pattern,
                    ),
                    ("USB label", &mut settings.branding.usb_label),
                    ("Server address", &mut settings.api.url),
                ] {
                    ui.label(label);
                    ui.text_edit_singleline(value);
                    ui.end_row();
                }
            });
        let home = profile
            .settings
            .branding
            .home_page
            .get_or_insert_with(String::new);
        ui.horizontal(|ui| {
            ui.label("Home page");
            ui.text_edit_singleline(home);
        });
        if home.trim().is_empty() {
            profile.settings.branding.home_page = None;
        }
        ui.label("Welcome text");
        ui.text_edit_multiline(&mut profile.settings.branding.welcome_text);
        ui.checkbox(
            &mut profile.settings.block_bluetooth,
            "Block Bluetooth on the stick",
        );
        ui.checkbox(
            &mut profile.settings.block_usb_storage,
            "Block other USB storage devices on the stick",
        );
        contest_end_wifi_recording_ui(ui, profile, &mut self.contest_end, &mut self.ssid);
        ui.label("Bookmarks");
        bookmarks_ui(ui, &mut profile.settings.branding.bookmarks);
        ui.label("Allowed sites, one per line");
        if ui.text_edit_multiline(&mut self.domains).changed() {
            profile.settings.network.allowed_domains = parse_domains(&self.domains);
        }
        ui.separator();
        for problem in profile.problems() {
            ui.colored_label(ui.visuals().error_fg_color, problem.to_string());
        }
        for warning in profile.warnings() {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        ui.separator();
        self.wallpaper_ui(ui, state);
    }

    fn wallpaper_ui(&mut self, ui: &mut eframe::egui::Ui, state: &mut AppState) {
        ui.horizontal(|ui| {
            ui.label("Wallpaper picture");
            ui.text_edit_singleline(&mut self.wallpaper_source);
            if ui.button("Use").clicked() {
                self.use_wallpaper(state);
            }
        });
        if let Some(name) = state
            .profile
            .as_ref()
            .and_then(|profile| profile.settings.branding.wallpaper.as_ref())
        {
            ui.label(format!("Wallpaper: {name}"));
        }
        if let Some(preview) = &self.preview {
            ui.add(eframe::egui::Image::new(preview).max_width(320.0));
        }
    }

    /// The Hub's profiles. A started save keeps the id it runs under, even if it fails, so that a
    /// retry updates the same Hub profile and does not make another one.
    fn hub_ui(&mut self, ui: &mut eframe::egui::Ui, state: &mut AppState) {
        let file = state
            .profile_path
            .as_ref()
            .map(|file| file.display().to_string())
            .unwrap_or_default();
        let current = state
            .profile
            .as_ref()
            .map(|profile| (profile, file.as_str()));
        // The same refusal as a local save: the typed end is not applied to the profile.
        let blocked = (!end_is_valid(&self.contest_end)).then_some(BAD_END);
        if let Some(Outcome::Assigned(id)) = self.hub.show(ui, current, blocked)
            && let Some(profile) = &mut state.profile
        {
            profile.hub_id.get_or_insert(id);
        }
    }

    fn open(&mut self, state: &mut AppState) {
        let path = PathBuf::from(self.path.trim());
        match Profile::load(&path) {
            Ok(profile) => {
                state.use_profile(profile, Some(path));
                "Opened".clone_into(&mut self.message);
            }
            Err(error) => self.message = error.to_string(),
        }
    }

    /// Saves a copy of the profile in the Forge folder and makes that file the one being edited.
    fn save_in_forge_folder(&mut self, state: &mut AppState) {
        let Some(profile) = &state.profile else {
            return;
        };
        if !end_is_valid(&self.contest_end) {
            BAD_END.clone_into(&mut self.message);
            return;
        }
        let Some(data) = state.data() else {
            "Could not find the Forge folder".clone_into(&mut self.message);
            return;
        };
        // The wallpaper lies beside the file the profile was opened from: it goes along.
        let wallpaper = profile.settings.branding.wallpaper.as_ref().map(|name| {
            state
                .profile_path
                .as_ref()
                .and_then(|file| file.parent())
                .map_or_else(|| PathBuf::from(name), |folder| folder.join(name))
        });
        match crate::folder::save_in(data, profile, wallpaper.as_deref()) {
            Ok((path, saved)) => {
                self.message = format!("Saved in {}", path.display());
                // The wallpaper of the copy may have another name than the one the editor had.
                state.use_profile(saved, Some(path));
            }
            Err(why) => self.message = why,
        }
    }

    /// Saves to the typed file, or in the Forge folder when none is typed.
    fn save(&mut self, state: &mut AppState) {
        // Otherwise the end that was typed would be left out and an older one saved.
        if !end_is_valid(&self.contest_end) {
            BAD_END.clone_into(&mut self.message);
            return;
        }
        let typed = self.path.trim();
        let result = if typed.is_empty() {
            state.save_profile().map(|_| ())
        } else {
            state.save_profile_to(PathBuf::from(typed))
        };
        self.message = match result {
            Ok(()) => "Saved".to_owned(),
            Err(why) => why,
        };
    }

    /// Makes the picture of the profile's wallpaper, or none when there is no picture or it is too
    /// large or not readable. The texture is replaced, so a new picture is never shown stale.
    fn refresh_preview(&mut self, ctx: &eframe::egui::Context) {
        self.preview = self
            .pictured
            .as_ref()
            .and_then(|(file, name, _)| wallpaper_data_url(file, name))
            .as_deref()
            .and_then(picture_of)
            .map(|picture| {
                ctx.load_texture("wallpaper", picture, eframe::egui::TextureOptions::LINEAR)
            });
    }

    fn use_wallpaper(&mut self, state: &mut AppState) {
        let Some(file) = state.profile_path.clone() else {
            "Save the profile first: the picture is copied next to it"
                .clone_into(&mut self.message);
            return;
        };
        let Some(profile) = &mut state.profile else {
            return;
        };
        match add_wallpaper(&file, &PathBuf::from(self.wallpaper_source.trim())) {
            Ok(name) => {
                profile.settings.branding.wallpaper = Some(name);
                state.wallpaper_revision += 1;
                "The picture was copied next to the profile".clone_into(&mut self.message);
            }
            Err(error) => self.message = error.to_string(),
        }
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
        let off = recording_fields(RecordingMode::Off);
        assert!(!off.timing && !off.snapshot_interval && !off.keeping);
        let snapshots = recording_fields(RecordingMode::Snapshots);
        assert!(snapshots.timing && snapshots.snapshot_interval && snapshots.keeping);
        let low = recording_fields(RecordingMode::Low);
        assert!(low.timing && !low.snapshot_interval && low.keeping);
    }

    #[test]
    fn an_empty_wifi_name_means_no_wifi_and_the_password_is_never_set() {
        assert_eq!(wifi_for("  "), None);
        let wifi = wifi_for(" venue ").expect("a network");
        assert_eq!(wifi.ssid, "venue");
        assert!(wifi.password.is_none());
    }

    #[test]
    fn a_wallpaper_address_becomes_a_picture_and_garbage_does_not() {
        // A 2 by 1 PNG made in memory, then handed over as the data address the engine builds.
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(2, 1)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("png");
        let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
        let picture = picture_of(&format!("data:image/png;base64,{encoded}")).expect("picture");
        assert_eq!(picture.size, [2, 1]);
        assert!(picture_of("data:image/png;base64,@@@").is_none());
        assert!(picture_of("not an address").is_none());
    }

    #[test]
    fn the_contest_end_is_empty_or_a_local_date_and_time() {
        assert!(end_is_valid(""));
        assert!(end_is_valid("2026-11-15T09:00"));
        assert!(!end_is_valid("tomorrow"));
    }

    #[test]
    fn a_new_bookmark_is_reported_until_it_is_filled_in() {
        let mut profile = Profile::template();
        let before = profile.problems().len();
        profile.settings.branding.bookmarks.push(new_bookmark());
        assert!(profile.problems().len() > before);
        let bookmark = profile
            .settings
            .branding
            .bookmarks
            .last_mut()
            .expect("added");
        bookmark.title = "Judge".to_owned();
        bookmark.url = "https://judge.example.org".to_owned();
        assert_eq!(profile.problems(), []);
    }

    #[test]
    fn a_profile_is_not_saved_with_a_contest_end_that_does_not_parse() {
        let dir = tempfile::tempdir().expect("folder");
        let ctx = eframe::egui::Context::default();
        let mut state = AppState::load(None);
        let mut screen = Screen::default();
        screen.poll(&ctx, &mut state);
        screen.path = dir.path().join("p.json").display().to_string();
        screen.contest_end = "tomorrow".to_owned();
        screen.save(&mut state);
        assert_ne!(screen.message, "Saved");
        // The footer's Continue is refused for the same reason.
        screen.poll(&ctx, &mut state);
        assert!(state.save_profile().is_err());
        assert!(!dir.path().join("p.json").exists());
    }

    #[test]
    fn a_saved_profile_opens_again_with_its_sites() {
        let dir = tempfile::tempdir().expect("folder");
        let ctx = eframe::egui::Context::default();
        let mut state = AppState::load(None);
        let mut screen = Screen::default();
        screen.poll(&ctx, &mut state);
        screen.path = dir.path().join("p.json").display().to_string();
        let profile = state.profile.as_mut().expect("profile");
        profile.settings.network.allowed_domains = parse_domains("a.example.org\nb.example.org");
        screen.save(&mut state);
        assert_eq!(screen.message, "Saved");

        let mut state = AppState::load(None);
        let mut again = Screen {
            path: screen.path.clone(),
            ..Screen::default()
        };
        again.open(&mut state);
        again.poll(&ctx, &mut state);
        assert_eq!(again.domains, "a.example.org\nb.example.org");
        assert_eq!(again.path, screen.path);
    }
}
