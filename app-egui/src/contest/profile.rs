//! The profile editor: the settings every stick of a contest gets. It shows what `ck-admin-core`
//! says is wrong with the profile, as `ContestPage.svelte` of the earlier web window does.

use std::path::{Path, PathBuf};

use ck_admin_core::{HUB_URL, Profile, ProfileSummary, add_wallpaper, folder_of};
use eframe::egui::{self, Ui};

use crate::contest::forms::{self, Typed};
use crate::contest::hub_profiles::{Outcome, Panel};
use crate::contest::preview::{self, Asked, Picture};
use crate::state::AppState;
use crate::theme::tokens;
use crate::widgets::{self, Btn, Input, Tone, field};

const PICTURES: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];

#[derive(Default)]
pub struct Screen {
    /// The profile that the typed texts were last read from.
    revision: Option<u64>,
    typed: Typed,
    picture: Picture,
    hub: Panel,
    /// The profiles on the Hub, while their card is shown.
    pub(super) hub_profiles: Vec<ProfileSummary>,
    /// What the last action did, and why one failed.
    status: String,
    failure: String,
    wallpaper_failure: String,
}

/// What is wrong with a profile and what is odd about it.
fn checked(profile: &Profile) -> (Vec<String>, Vec<String>) {
    let mut profile = profile.clone();
    // The server address is not set in the window: a stick gets it when the admin is signed in.
    HUB_URL.clone_into(&mut profile.settings.api.url);
    let mut problems: Vec<String> = profile.problems().iter().map(ToString::to_string).collect();
    if let Err(error) = profile.credential_scope() {
        problems.push(error.to_string());
    }
    (problems, profile.warnings())
}

/// Whether a wallpaper other than `source` lies in `folder`, where copying `source` would replace
/// or remove it.
fn holds_another_wallpaper(folder: &Path, source: &Path) -> bool {
    let ours = std::fs::read(source).ok();
    PICTURES.iter().any(|extension| {
        std::fs::read(folder.join(format!("wallpaper.{extension}")))
            .is_ok_and(|there| Some(there) != ours)
    })
}

fn profile_files() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Contest profile", &["json"])
}

impl Screen {
    /// Called every frame, whatever page is shown: takes what the Hub answered, follows the shared
    /// state and says whether the profile may be saved now.
    pub fn poll(&mut self, ctx: &egui::Context, state: &mut AppState) {
        if let Some(outcome) = self.hub.receive() {
            self.take(outcome, state);
        }
        self.follow(ctx, state);
        // A save now would write the file a Hub request is writing, or leave typed text out.
        state.blocked = if self.hub.busy() {
            Some("Wait until the Hub request has ended.".to_owned())
        } else {
            (!self.typed.end_is_valid()).then(|| forms::BAD_END.to_owned())
        };
    }

    fn take(&mut self, outcome: Outcome, state: &mut AppState) {
        match outcome {
            Outcome::Listed(profiles) => {
                if profiles.is_empty() {
                    "The Hub has no profiles yet. Save one to it.".clone_into(&mut self.status);
                }
                self.hub_profiles = profiles;
            }
            Outcome::Loaded(loaded) => {
                self.status = format!("Loaded {} from the Hub", loaded.profile.name);
                self.hub_profiles.clear();
                state.use_profile(loaded.profile, Some(loaded.path));
            }
            Outcome::Saved(profile, path) => {
                self.status = format!("Saved {} to the Hub", profile.name);
                state.use_profile(*profile, Some(path));
            }
            Outcome::Failed(why) => self.failure = why,
        }
    }

    /// Reads the typed texts again when another profile is being edited, and the wallpaper when it
    /// changed.
    fn follow(&mut self, ctx: &egui::Context, state: &AppState) {
        if self.revision != Some(state.profile_revision) {
            self.revision = Some(state.profile_revision);
            self.typed = state.profile.as_ref().map(Typed::read).unwrap_or_default();
            self.wallpaper_failure.clear();
        }
        self.picture.follow(ctx, state);
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        // A request to the Hub is running: nothing here can change until it has ended.
        if self.hub.busy() {
            ui.disable();
        }
        self.toolbar(ui, state);
        self.hub_card(ui, state);
        widgets::note(ui, Tone::Ok, &self.status);
        widgets::note(ui, Tone::Error, &self.failure);
        // An action above may have taken another profile in.
        self.follow(ui.ctx(), state);
        let Some(profile) = &mut state.profile else {
            widgets::card(ui, |ui| {
                widgets::text(ui, "Start with a new profile, or open one you saved.");
            });
            return;
        };
        let (problems, warnings) = checked(profile);
        widgets::messages(ui, &problems, &warnings);
        widgets::card(ui, |ui| {
            let hint = "Only for you. Contestants never see it.";
            field(ui, "Profile name", Some(hint), None, |ui, id| {
                Input::new(&mut profile.name).show(ui).labelled_by(id);
            });
        });
        forms::contest(ui, &mut profile.settings, &mut self.typed);
        forms::team_and_site(ui, &mut profile.settings);
        preview::contestant(ui, &self.picture, profile);
        forms::key_window(ui, profile, &mut self.typed);
        widgets::details(ui, "What contestants see", |ui| self.branding(ui, state));
        widgets::details(ui, "Network, Hub and screen recording", |ui| {
            ui.spacing_mut().item_spacing.y = tokens(ui.ctx()).s.gap;
            if let Some(profile) = &mut state.profile {
                forms::network(ui, &mut profile.settings, &mut self.typed);
            }
        });
        self.other_places(ui, state);
    }

    fn toolbar(&mut self, ui: &mut Ui, state: &mut AppState) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = tokens(ui.ctx()).s.gap;
            let ready = state.profile.is_some();
            if Btn::new("New profile").show(ui).clicked() {
                self.create(state);
            }
            if Btn::new("Open\u{2026}").show(ui).clicked()
                && let Some(path) = profile_files().pick_file()
            {
                self.open(state, path);
            }
            if Btn::new("Save").primary().enabled(ready).show(ui).clicked() {
                self.save(state);
            }
            if state.hub_email.is_some() {
                if Btn::new("Load from Hub\u{2026}").show(ui).clicked() {
                    self.clear_messages();
                    self.hub.list(ui.ctx());
                }
                if Btn::new("Save to Hub").enabled(ready).show(ui).clicked() {
                    self.save_to_hub(ui.ctx(), state);
                }
            }
        });
    }

    fn hub_card(&mut self, ui: &mut Ui, state: &AppState) {
        if self.hub_profiles.is_empty() {
            return;
        }
        let mut chosen = None;
        widgets::card(ui, |ui| {
            widgets::title(ui, "Profiles on the Hub");
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for summary in &self.hub_profiles {
                    ui.horizontal(|ui| {
                        let t = tokens(ui.ctx());
                        ui.spacing_mut().item_spacing.x = t.s.gap;
                        // The name is centred on the button.
                        ui.spacing_mut().interact_size.y = t.s.button_small;
                        ui.label(&summary.name);
                        if Btn::new("Open").small().show(ui).clicked() {
                            chosen = Some(summary.id.clone());
                        }
                    });
                }
            });
        });
        if let Some(id) = chosen {
            self.clear_messages();
            match state.data() {
                Some(data) => self.hub.load(ui.ctx(), id, data.join("profiles")),
                None => "Could not find the Forge folder".clone_into(&mut self.failure),
            }
        }
    }

    /// The inside of "What contestants see".
    fn branding(&mut self, ui: &mut Ui, state: &mut AppState) {
        ui.spacing_mut().item_spacing.y = tokens(ui.ctx()).s.gap;
        let Some(profile) = &mut state.profile else {
            return;
        };
        forms::names(ui, &mut profile.settings, &mut self.typed);
        let name = profile.settings.branding.wallpaper.as_deref();
        match preview::picker(ui, &self.picture, name, &self.wallpaper_failure) {
            Some(Asked::Remove) => profile.settings.branding.wallpaper = None,
            Some(Asked::Choose) => {
                self.wallpaper_failure.clear();
                let picked = rfd::FileDialog::new()
                    .add_filter("Picture", &PICTURES)
                    .pick_file();
                if let Some(source) = picked {
                    self.use_wallpaper(state, &source);
                }
            }
            None => {}
        }
        if let Some(profile) = &mut state.profile {
            forms::welcome_and_bookmarks(ui, &mut profile.settings);
        }
    }

    /// Saving a profile that has a file to somewhere else. The earlier web window had neither
    /// button, so they stand apart at the end of the page.
    fn other_places(&mut self, ui: &mut Ui, state: &mut AppState) {
        let Some(path) = &state.profile_path else {
            return;
        };
        let outside = state.data().is_some_and(|data| !path.starts_with(data));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = tokens(ui.ctx()).s.gap;
            if Btn::new("Save as\u{2026}").small().show(ui).clicked()
                && let Some(path) = profile_files()
                    .set_file_name("contest-profile.json")
                    .save_file()
            {
                self.save_as(state, &path);
            }
            let copy = "Save a copy in the Forge folder";
            if outside && Btn::new(copy).small().show(ui).clicked() {
                self.save_in_forge_folder(state);
            }
        });
    }

    fn clear_messages(&mut self) {
        self.status.clear();
        self.failure.clear();
    }

    fn create(&mut self, state: &mut AppState) {
        self.clear_messages();
        state.use_profile(Profile::template(), None);
        "A new profile. Save it to build sticks.".clone_into(&mut self.status);
    }

    fn open(&mut self, state: &mut AppState, path: PathBuf) {
        self.clear_messages();
        match Profile::load(&path) {
            Ok(profile) => {
                self.status = format!("Opened {}", path.display());
                state.use_profile(profile, Some(path));
            }
            Err(error) => self.failure = crate::state::message(&error),
        }
    }

    fn save(&mut self, state: &mut AppState) {
        self.clear_messages();
        match state.save_profile() {
            Ok(path) => self.status = format!("Saved {}", path.display()),
            Err(why) => self.failure = why,
        }
    }

    /// Saves the file first: the Hub copy takes its wallpaper from beside it.
    fn save_to_hub(&mut self, ctx: &egui::Context, state: &mut AppState) {
        self.clear_messages();
        let started = state.save_profile().and_then(|path| {
            let profile = state
                .profile
                .clone()
                .ok_or_else(|| "There is no profile to save.".to_owned())?;
            self.hub.save(ctx, profile, path)
        });
        match started {
            Ok(assigned) => {
                if let Some(profile) = &mut state.profile
                    && profile.hub_id.is_none()
                {
                    profile.hub_id = assigned;
                }
            }
            Err(why) => self.failure = why,
        }
    }

    /// Saves to another file, taking the wallpaper along.
    fn save_as(&mut self, state: &mut AppState, path: &Path) {
        self.clear_messages();
        let wallpaper = state.profile_path.as_ref().and_then(|old| {
            let branding = &state.profile.as_ref()?.settings.branding;
            let name = branding.wallpaper.as_ref()?;
            (folder_of(old) != folder_of(path)).then(|| folder_of(old).join(name))
        });
        // The folder may belong to another profile, whose picture must not be replaced.
        if let Some(source) = &wallpaper
            && holds_another_wallpaper(folder_of(path), source)
        {
            "That folder already holds another wallpaper. Choose another folder."
                .clone_into(&mut self.failure);
            return;
        }
        let saved = state.save_profile_to(path.to_owned()).and_then(|()| {
            let Some(source) = &wallpaper else {
                return Ok(());
            };
            let name = add_wallpaper(path, source).map_err(|e| e.to_string())?;
            state.wallpaper_revision += 1;
            match &mut state.profile {
                // The copy may have another name than the picture had.
                Some(profile) if profile.settings.branding.wallpaper.as_ref() != Some(&name) => {
                    profile.settings.branding.wallpaper = Some(name);
                    state.save_profile_to(path.to_owned())
                }
                _ => Ok(()),
            }
        });
        match saved {
            Ok(()) => self.status = format!("Saved {}", path.display()),
            Err(why) => self.failure = why,
        }
    }

    /// Saves a copy of the profile in the Forge folder and makes that file the one being edited.
    fn save_in_forge_folder(&mut self, state: &mut AppState) {
        self.clear_messages();
        // The same refusals as every other save.
        if let Some(why) = &state.blocked {
            self.failure.clone_from(why);
            return;
        }
        let (Some(profile), Some(data)) = (&state.profile, state.data()) else {
            "Could not find the Forge folder".clone_into(&mut self.failure);
            return;
        };
        // The wallpaper lies beside the file the profile was opened from: it goes along.
        let wallpaper = profile.settings.branding.wallpaper.as_ref().map(|name| {
            state
                .profile_path
                .as_ref()
                .map_or_else(|| PathBuf::from(name), |file| folder_of(file).join(name))
        });
        match crate::folder::save_in(data, profile, wallpaper.as_deref()) {
            Ok((path, saved)) => {
                self.status = format!("Saved {}", path.display());
                // The wallpaper of the copy may have another name than the one the editor had.
                state.use_profile(saved, Some(path));
            }
            Err(why) => self.failure = why,
        }
    }

    /// Copies a picture next to the profile, which is saved first so that it has a place.
    fn use_wallpaper(&mut self, state: &mut AppState, source: &Path) {
        let added = state
            .save_profile()
            .and_then(|path| add_wallpaper(&path, source).map_err(|e| e.to_string()));
        match added {
            Ok(name) => {
                if let Some(profile) = &mut state.profile {
                    profile.settings.branding.wallpaper = Some(name);
                    state.wallpaper_revision += 1;
                }
            }
            Err(why) => self.wallpaper_failure = why,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(path: &Path, shade: u8) {
        image::RgbImage::from_pixel(4, 4, image::Rgb([shade; 3]))
            .save(path)
            .expect("picture");
    }

    /// A window state with a Forge folder of its own.
    fn sample() -> (tempfile::TempDir, AppState, Screen) {
        let data = tempfile::tempdir().expect("folder");
        let state = AppState::load(Some(data.path().to_owned()));
        (data, state, Screen::default())
    }

    #[test]
    fn a_profile_is_not_saved_with_a_contest_end_that_does_not_parse() {
        let (data, mut state, mut screen) = sample();
        let ctx = egui::Context::default();
        screen.poll(&ctx, &mut state);
        screen.typed.contest_end = "tomorrow".to_owned();
        screen.poll(&ctx, &mut state);
        screen.save(&mut state);
        assert_eq!(screen.failure, forms::BAD_END);
        screen.save_in_forge_folder(&mut state);
        assert_eq!(screen.failure, forms::BAD_END);
        assert!(!data.path().join("profiles").exists());
    }

    #[test]
    fn a_saved_profile_opens_again() {
        let (data, mut state, mut screen) = sample();
        state.profile.as_mut().expect("profile").name = "Finals".to_owned();
        screen.save(&mut state);
        let path = state.profile_path.clone().expect("saved");
        assert_eq!(screen.status, format!("Saved {}", path.display()));
        screen.create(&mut state);
        assert!(state.profile_path.is_none() && state.is_unsaved());
        screen.open(&mut state, path.clone());
        assert_eq!(state.profile.as_ref().expect("profile").name, "Finals");
        assert!(!state.is_unsaved());
        screen.open(&mut state, data.path().join("missing.json"));
        assert_ne!(screen.failure, "");
        assert_eq!(state.profile_path, Some(path));
    }

    #[test]
    fn choosing_a_wallpaper_saves_the_profile_first() {
        let (data, mut state, mut screen) = sample();
        let source = data.path().join("source.png");
        picture(&source, 1);
        screen.use_wallpaper(&mut state, &source);
        assert_eq!(screen.wallpaper_failure, "");
        let path = state.profile_path.clone().expect("saved");
        assert!(folder_of(&path).join("wallpaper.png").is_file());
        assert!(state.is_unsaved(), "the new name is not in the file yet");
    }

    #[test]
    fn save_as_takes_the_wallpaper_along_and_replaces_no_other() {
        let (data, mut state, mut screen) = sample();
        let source = data.path().join("source.png");
        picture(&source, 1);
        screen.use_wallpaper(&mut state, &source);
        let first = state.profile_path.clone();

        let other = tempfile::tempdir().expect("folder");
        let theirs = other.path().join("wallpaper.png");
        picture(&theirs, 2);
        let before = std::fs::read(&theirs).expect("picture");
        screen.save_as(&mut state, &other.path().join("p.json"));
        assert!(screen.failure.contains("another wallpaper"));
        assert!(!other.path().join("p.json").exists());
        assert_eq!(std::fs::read(&theirs).expect("picture"), before);
        assert_eq!(state.profile_path, first);

        let free = tempfile::tempdir().expect("folder");
        screen.save_as(&mut state, &free.path().join("p.json"));
        assert_eq!(screen.failure, "");
        assert!(free.path().join("wallpaper.png").is_file());
        assert!(!state.is_unsaved());
    }

    #[test]
    fn the_check_does_not_ask_for_a_server_address_and_reports_the_key_window() {
        let mut profile = Profile::template();
        assert_eq!(checked(&profile), (Vec::new(), Vec::new()));
        profile.stick_key_window = Some(ck_admin_core::StickKeyWindow {
            valid_from: String::new(),
            valid_until: String::new(),
        });
        assert_eq!(checked(&profile).0.len(), 1);
    }
}
