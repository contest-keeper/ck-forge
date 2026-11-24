//! The profile editor: the settings every stick of a contest gets. It edits one profile file and
//! shows what `ck-admin-core` says is wrong with it, as the Svelte window does.

use std::path::PathBuf;

use ck_admin_core::{Profile, add_wallpaper};
use ck_protocol::Bookmark;

#[derive(Default)]
pub struct Screen {
    /// The file the profile is read from and saved to, as typed.
    path: String,
    profile: Option<Profile>,
    /// One site name per line, kept as typed until it is applied to the profile.
    domains: String,
    wallpaper_source: String,
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

impl Screen {
    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        ui.heading("Profile");
        ui.horizontal(|ui| {
            ui.label("File");
            ui.text_edit_singleline(&mut self.path);
        });
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                self.set(Profile::template(), "A new profile");
            }
            if ui.button("Open").clicked() {
                self.open();
            }
            if ui
                .add_enabled(self.profile.is_some(), eframe::egui::Button::new("Save"))
                .clicked()
            {
                self.save();
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        let Some(profile) = &mut self.profile else {
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
        ui.horizontal(|ui| {
            ui.label("Wallpaper picture");
            ui.text_edit_singleline(&mut self.wallpaper_source);
            if ui.button("Use").clicked() {
                self.use_wallpaper();
            }
        });
        if let Some(name) = self
            .profile
            .as_ref()
            .and_then(|profile| profile.settings.branding.wallpaper.as_ref())
        {
            ui.label(format!("Wallpaper: {name}"));
        }
    }

    fn set(&mut self, profile: Profile, message: &str) {
        self.domains = profile.settings.network.allowed_domains.join("\n");
        self.profile = Some(profile);
        message.clone_into(&mut self.message);
    }

    fn open(&mut self) {
        match Profile::load(&PathBuf::from(self.path.trim())) {
            Ok(profile) => self.set(profile, "Opened"),
            Err(error) => self.message = error.to_string(),
        }
    }

    fn save(&mut self) {
        let Some(profile) = &self.profile else {
            return;
        };
        self.message = match profile.save(&PathBuf::from(self.path.trim())) {
            Ok(()) => "Saved".to_owned(),
            Err(error) => error.to_string(),
        };
    }

    fn use_wallpaper(&mut self) {
        let Some(profile) = &mut self.profile else {
            return;
        };
        let file = PathBuf::from(self.path.trim());
        match add_wallpaper(&file, &PathBuf::from(self.wallpaper_source.trim())) {
            Ok(name) => {
                profile.settings.branding.wallpaper = Some(name);
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
    fn a_saved_profile_opens_again_with_its_sites() {
        let dir = std::env::temp_dir().join(format!("ck-egui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("folder");
        let mut screen = Screen {
            path: dir.join("p.json").display().to_string(),
            ..Screen::default()
        };
        screen.set(Profile::template(), "");
        screen.domains = "a.example.org\nb.example.org".to_owned();
        if let Some(profile) = &mut screen.profile {
            profile.settings.network.allowed_domains = parse_domains(&screen.domains);
        }
        screen.save();
        assert_eq!(screen.message, "Saved");

        let mut again = Screen {
            path: screen.path.clone(),
            ..Screen::default()
        };
        again.open();
        assert_eq!(again.domains, "a.example.org\nb.example.org");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
