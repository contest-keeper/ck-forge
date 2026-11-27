//! The profile editor: the settings every stick of a contest gets. It shows what `ck-admin-core`
//! says is wrong with the profile, as `ContestPage.svelte` of the earlier web window does.

use std::path::{Path, PathBuf};

use ck_admin_core::{HUB_URL, Profile, ProfileSummary, add_wallpaper, folder_of};
use eframe::egui::{self, Ui};

use crate::contest::forms::{self, Typed};
use crate::contest::hub_profiles::{Outcome, Panel};
use crate::contest::preview::{self, Asked, Picture};
use crate::folder::Local;
use crate::state::AppState;
use crate::theme::tokens;
use crate::widgets::{self, Btn, Tone};

const PICTURES: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];

#[derive(Clone, PartialEq, Eq)]
enum PendingDiscard {
    New,
    Open(PathBuf),
}

type DeleteTarget = (Option<PathBuf>, Option<String>);

#[derive(Default)]
pub struct Screen {
    /// The profile that the typed texts were last read from.
    revision: Option<u64>,
    typed: Typed,
    picture: Picture,
    hub: Panel,
    /// The profiles on the Hub, listed when the admin signs in and after every save.
    /// None until the Hub has answered, so a failed request is not read as an empty list.
    pub(super) hub_profiles: Option<Vec<ProfileSummary>>,
    /// The profiles in the Forge folder, read again when the open profile or its file changes.
    local: Vec<Local>,
    local_key: Option<(u64, Option<PathBuf>, bool)>,
    /// The account the Hub's list was asked for.
    listed_for: Option<String>,
    /// Confirmation state before replacing unsaved edits or deleting a saved profile.
    pending_discard: Option<PendingDiscard>,
    confirm_delete: Option<DeleteTarget>,
    deleting_active: bool,
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

/// `add_wallpaper`, but a picture that is already in place is not copied onto itself: it may be
/// the chosen file under another name (a link), which copying would empty.
fn copy_wallpaper(profile: &Path, source: &Path) -> Result<String, String> {
    let there = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            let name = format!("wallpaper.{}", extension.to_ascii_lowercase());
            folder_of(profile).join(name)
        })
        .filter(|there| same_picture(there, source));
    add_wallpaper(profile, there.as_deref().unwrap_or(source)).map_err(|e| e.to_string())
}

fn same_picture(first: &Path, second: &Path) -> bool {
    let size = |path| std::fs::metadata(path).map(|meta| meta.len()).ok();
    // The sizes first, so that a huge file is not read to compare it.
    size(first).is_some()
        && size(first) == size(second)
        && std::fs::read(first).ok() == std::fs::read(second).ok()
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

/// A saved profile, once, with the places it is kept.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    name: String,
    /// Its file on this computer.
    file: Option<PathBuf>,
    /// The id the Hub knows it by.
    hub_id: Option<String>,
}

/// One entry for each profile: the ones saved here and on the Hub are the same profile when the Hub
/// id matches, and the rest are kept in one place only. By name.
fn merged(local: &[Local], hub: Option<&[ProfileSummary]>) -> Vec<Entry> {
    let known = hub.unwrap_or_default();
    let mut entries: Vec<Entry> = local
        .iter()
        .map(|item| Entry {
            name: item.name.clone(),
            file: Some(item.path.clone()),
            hub_id: item
                .hub_id
                .clone()
                // Without the Hub's answer an id cannot be told wrong, so it stays.
                .filter(|id| hub.is_none() || known.iter().any(|on_hub| &on_hub.id == id)),
        })
        .collect();
    for summary in known {
        if !entries
            .iter()
            .any(|entry| entry.hub_id.as_ref() == Some(&summary.id))
        {
            entries.push(Entry {
                name: summary.name.clone(),
                file: None,
                hub_id: Some(summary.id.clone()),
            });
        }
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowAction {
    Open,
    AskDelete,
    ConfirmDelete,
    CancelDelete,
}

/// A saved profile as one row: its name, where it is kept and the actions for it.
fn profile_row(ui: &mut Ui, entry: &Entry, open: bool, confirming: bool) -> Option<RowAction> {
    let kept = match (&entry.file, &entry.hub_id) {
        (Some(_), Some(_)) => "On this computer & Hub",
        (Some(_), None) => "On this computer only",
        (None, _) => "On the Hub only",
    };
    let mut action = None;
    widgets::path_row(ui, &entry.name, &[kept], |ui| {
        if confirming {
            if Btn::new("Delete").danger().small().show(ui).clicked() {
                action = Some(RowAction::ConfirmDelete);
            }
            if Btn::new("Cancel").small().show(ui).clicked() {
                action = Some(RowAction::CancelDelete);
            }
            return;
        }
        if open {
            widgets::pill(ui, Tone::Ok, "Open now");
        } else {
            let label = if entry.file.is_some() {
                "Open"
            } else {
                "Download"
            };
            if Btn::new(label).small().show(ui).clicked() {
                action = Some(RowAction::Open);
            }
        }
        if Btn::new("Delete\u{2026}").small().show(ui).clicked() {
            action = Some(RowAction::AskDelete);
        }
    });
    action
}

fn profile_files() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Contest profile", &["json"])
}

impl Screen {
    /// Called every frame, whatever page is shown: takes what the Hub answered, follows the shared
    /// state and says whether the profile may be saved now.
    pub fn poll(&mut self, ctx: &egui::Context, state: &mut AppState) {
        if let Some(outcome) = self.hub.receive() {
            self.take(ctx, outcome, state);
        }
        self.follow(ctx, state);
        if self.listed_for != state.hub_email && !self.hub.busy() {
            self.listed_for.clone_from(&state.hub_email);
            self.hub_profiles = None;
            if state.hub_email.is_some() {
                self.hub.list(ctx);
            }
        }
        // A save now would write the file a Hub request is writing.
        state.blocked = self
            .hub
            .busy()
            .then(|| "Wait until the Hub request has ended.".to_owned());
    }

    fn take(&mut self, ctx: &egui::Context, outcome: Outcome, state: &mut AppState) {
        match outcome {
            Outcome::Listed(profiles) => self.hub_profiles = Some(profiles),
            Outcome::Loaded(loaded) => {
                self.status = format!("Loaded {} from the Hub", loaded.profile.name);
                state.use_profile(loaded.profile, Some(loaded.path));
            }
            Outcome::Saved(profile, path) => {
                self.status = format!("Saved {} here and on the Hub", profile.name);
                state.use_profile(*profile, Some(path));
                // The Hub's list now has this profile.
                self.hub.list(ctx);
            }
            Outcome::Deleted => {
                "Deleted profile.".clone_into(&mut self.status);
                if self.deleting_active {
                    self.deleting_active = false;
                    state.use_profile(Profile::template(), None);
                }
                self.refresh_local(state);
                if state.hub_email.is_some() {
                    self.hub.list(ctx);
                }
            }
            Outcome::Failed(why) => {
                self.deleting_active = false;
                self.failure = why;
            }
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
        // A save changes `is_unsaved` even when the file and the profile stay the same ones.
        let key = (
            state.profile_revision,
            state.profile_path.clone(),
            state.is_unsaved(),
        );
        if self.local_key.as_ref() != Some(&key) {
            self.local_key = Some(key);
            self.refresh_local(state);
        }
        self.picture.follow(ctx, state);
    }

    fn refresh_local(&mut self, state: &AppState) {
        self.local = state
            .data()
            .map(crate::folder::list_local)
            .unwrap_or_default();
    }

    /// The page "Contest profile".
    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        // A request to the Hub is running: nothing here can change until it has ended.
        if self.hub.busy() {
            ui.disable();
        }
        self.toolbar(ui, state);
        self.profiles_card(ui, state);
        self.status_notes(ui);
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
        forms::contest(ui, profile);
        widgets::card(ui, |ui| self.desktop(ui, state));
        if let Some(profile) = &mut state.profile {
            forms::team_and_site(ui, &mut profile.settings);
        }
        self.other_places(ui, state);
    }

    /// The page "Connectivity".
    pub fn show_connectivity(&mut self, ui: &mut Ui, state: &mut AppState) {
        if self.hub.busy() {
            ui.disable();
        }
        self.status_notes(ui);
        self.follow(ui.ctx(), state);
        if let Some(profile) = &mut state.profile {
            forms::connectivity(ui, &mut profile.settings, &mut self.typed);
        }
    }

    /// The page "Screen recording".
    pub fn show_recording(&mut self, ui: &mut Ui, state: &mut AppState) {
        if self.hub.busy() {
            ui.disable();
        }
        self.status_notes(ui);
        if let Some(profile) = &mut state.profile {
            forms::recording_page(ui, &mut profile.settings);
        }
    }

    /// Why the last save or Hub request failed, for pages that do not show the notes themselves.
    pub fn failure(&self) -> &str {
        &self.failure
    }

    fn status_notes(&self, ui: &mut Ui) {
        widgets::note(ui, Tone::Ok, &self.status);
        widgets::note(ui, Tone::Error, &self.failure);
    }

    fn toolbar(&mut self, ui: &mut Ui, state: &mut AppState) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = tokens(ui.ctx()).s.gap;
            let ready = state.profile.is_some();
            if Btn::new("Save").primary().enabled(ready).show(ui).clicked() {
                self.pending_discard = None;
                self.save(ui.ctx(), state);
            }
            if state.hub_email.is_some() && Btn::new("Refresh Hub list").show(ui).clicked() {
                self.clear_messages();
                self.hub.list(ui.ctx());
            }
            if Btn::new("New profile").show(ui).clicked() {
                if state.is_unsaved() {
                    self.pending_discard = Some(PendingDiscard::New);
                } else {
                    self.create(state);
                }
            }
            if Btn::new("Open a file\u{2026}").show(ui).clicked()
                && let Some(path) = profile_files().pick_file()
            {
                if state.is_unsaved() {
                    self.pending_discard = Some(PendingDiscard::Open(path));
                } else {
                    self.open(state, path);
                }
            }
        });
        if let Some(pending) = self.pending_discard.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = tokens(ui.ctx()).s.gap;
                widgets::hint(ui, "Unsaved changes will be lost.");
                if Btn::new("Discard").danger().small().show(ui).clicked() {
                    self.pending_discard = None;
                    match pending {
                        PendingDiscard::New => self.create(state),
                        PendingDiscard::Open(path) => self.open(state, path),
                    }
                }
                if Btn::new("Cancel").small().show(ui).clicked() {
                    self.pending_discard = None;
                }
            });
        }
    }

    /// The saved profiles, from this computer and, when signed in, from the Hub, one row each.
    fn profiles_card(&mut self, ui: &mut Ui, state: &mut AppState) {
        let entries = merged(&self.local, self.hub_profiles.as_deref());
        if entries.is_empty() {
            self.confirm_delete = None;
            return;
        }
        let mut picked = None;
        widgets::card(ui, |ui| {
            widgets::title(ui, "Saved profiles");
            for (at, entry) in entries.iter().enumerate() {
                let here = match &entry.file {
                    Some(file) => state.profile_path.as_ref() == Some(file),
                    None => {
                        state.profile.as_ref().and_then(|p| p.hub_id.as_ref())
                            == entry.hub_id.as_ref()
                    }
                };
                let key = (entry.file.clone(), entry.hub_id.clone());
                let confirming = self.confirm_delete.as_ref() == Some(&key);
                let action = ui
                    .push_id(("saved", at), |ui| profile_row(ui, entry, here, confirming))
                    .inner;
                if let Some(action) = action {
                    picked = Some((at, here, action));
                }
            }
        });
        let Some((at, here, action)) = picked else {
            return;
        };
        let Some(entry) = entries.into_iter().nth(at) else {
            return;
        };
        let key = (entry.file.clone(), entry.hub_id.clone());
        match action {
            RowAction::AskDelete => self.confirm_delete = Some(key),
            RowAction::CancelDelete => self.confirm_delete = None,
            RowAction::ConfirmDelete => {
                self.confirm_delete = None;
                self.clear_messages();
                if let (Some(id), true) = (entry.hub_id, state.hub_email.is_some()) {
                    self.deleting_active = here;
                    self.hub.delete(
                        ui.ctx(),
                        id,
                        entry.file,
                        state.data().map(Path::to_path_buf),
                    );
                } else if let Some(path) = entry.file {
                    match crate::folder::delete_local(state.data(), &path) {
                        Ok(()) => {
                            "Deleted profile.".clone_into(&mut self.status);
                            if here {
                                state.use_profile(Profile::template(), None);
                            }
                            self.refresh_local(state);
                        }
                        Err(why) => self.failure = why,
                    }
                }
            }
            RowAction::Open => {
                self.confirm_delete = None;
                match (entry.file, entry.hub_id) {
                    (Some(path), _) => self.open(state, path),
                    (None, Some(id)) => {
                        self.clear_messages();
                        match state.data() {
                            Some(data) => self.hub.load(ui.ctx(), id, data.join("profiles")),
                            None => "Could not find the Forge folder".clone_into(&mut self.failure),
                        }
                    }
                    (None, None) => {}
                }
            }
        }
    }

    /// The card with the wallpaper, the welcome text and the bookmarks.
    fn desktop(&mut self, ui: &mut Ui, state: &mut AppState) {
        ui.spacing_mut().item_spacing.y = tokens(ui.ctx()).s.gap;
        widgets::title(ui, "Desktop");
        let Some(profile) = &mut state.profile else {
            return;
        };
        let name = profile.settings.branding.wallpaper.as_deref();
        match preview::picker(ui, &self.picture, name, &self.wallpaper_failure) {
            Some(Asked::Remove) => profile.settings.branding.wallpaper = None,
            Some(Asked::Choose) => {
                self.wallpaper_failure.clear();
                let picked = rfd::FileDialog::new()
                    .add_filter("Picture", &PICTURES)
                    .pick_file();
                if let Some(source) = picked {
                    self.use_wallpaper(ui.ctx(), state, &source);
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

    /// Saves the file, and when signed in the Hub's copy too, so the two never differ.
    pub fn save(&mut self, ctx: &egui::Context, state: &mut AppState) {
        self.clear_messages();
        match state.save_profile() {
            Ok(path) => {
                self.status = format!("Saved {}", path.display());
                self.sync_hub(ctx, state, path);
            }
            Err(why) => self.failure = why,
        }
    }

    /// Sends the saved profile to the Hub when signed in. The Hub copy takes its wallpaper from
    /// beside the file, so the file is saved first.
    pub fn sync_hub(&mut self, ctx: &egui::Context, state: &mut AppState, path: PathBuf) {
        let (Some(_), Some(profile)) = (&state.hub_email, state.profile.clone()) else {
            return;
        };
        match self.hub.save(ctx, profile, path) {
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
        // The picture goes first: if it cannot be copied, the editor stays on the old file, which
        // has it. Nothing is copied while saving is blocked.
        let mut before = None;
        if let Some(source) = &wallpaper
            && state.blocked.is_none()
        {
            match copy_wallpaper(path, source) {
                Ok(name) => {
                    if let Some(profile) = &mut state.profile {
                        before = profile.settings.branding.wallpaper.replace(name);
                        state.wallpaper_revision += 1;
                    }
                }
                Err(why) => {
                    self.failure = why;
                    return;
                }
            }
        }
        match state.save_profile_to(path.to_owned()) {
            Ok(()) => self.status = format!("Saved {}", path.display()),
            Err(why) => {
                // The old file is still the one edited, so its picture keeps its name.
                if let (Some(profile), Some(name)) = (&mut state.profile, before) {
                    profile.settings.branding.wallpaper = Some(name);
                }
                self.failure = why;
            }
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
    fn use_wallpaper(&mut self, ctx: &egui::Context, state: &mut AppState, source: &Path) {
        let added = state
            .save_profile()
            .and_then(|path| copy_wallpaper(&path, source));
        match added {
            Ok(name) => {
                if let Some(profile) = &mut state.profile {
                    profile.settings.branding.wallpaper = Some(name);
                    state.wallpaper_revision += 1;
                }
                match state.save_profile() {
                    Ok(path) => self.sync_hub(ctx, state, path),
                    Err(why) => self.wallpaper_failure = why,
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
    fn a_renamed_profile_shows_its_new_name_in_the_saved_list_after_a_save() {
        let (_data, mut state, mut screen) = sample();
        let ctx = egui::Context::default();
        screen.save(&ctx, &mut state);
        screen.poll(&ctx, &mut state);
        "Beta".clone_into(&mut state.profile.as_mut().expect("profile").name);
        screen.poll(&ctx, &mut state);
        screen.save(&ctx, &mut state);
        screen.poll(&ctx, &mut state);
        assert_eq!(screen.local.len(), 1);
        assert_eq!(screen.local[0].name, "Beta");
    }

    #[test]
    fn a_profile_kept_here_and_on_the_hub_is_listed_once() {
        let local = |name: &str, hub_id: Option<&str>| Local {
            name: name.to_owned(),
            path: PathBuf::from(format!("{name}.json")),
            hub_id: hub_id.map(str::to_owned),
        };
        let hub = |id: &str, name: &str| ProfileSummary {
            id: id.to_owned(),
            name: name.to_owned(),
        };
        let entries = merged(
            &[
                local("beta", Some("b1")),
                local("Alpha", Some("gone")),
                local("solo", None),
            ],
            Some(&[hub("b1", "beta"), hub("h2", "Hub only")]),
        );
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "Hub only", "solo"]);
        // An id the Hub no longer lists is not a copy on the Hub.
        assert_eq!(entries[0].hub_id, None);
        assert!(entries[1].file.is_some() && entries[1].hub_id.is_some());
        assert!(entries[2].file.is_none());
        // Before the Hub has answered, a local profile keeps its id and nothing is added.
        let before = merged(&[local("beta", Some("b1"))], None);
        assert_eq!(before.len(), 1);
        assert!(before[0].hub_id.is_some());
    }

    #[test]
    fn a_saved_profile_opens_again() {
        let (data, mut state, mut screen) = sample();
        state.profile.as_mut().expect("profile").name = "Finals".to_owned();
        screen.save(&egui::Context::default(), &mut state);
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
        screen.use_wallpaper(&egui::Context::default(), &mut state, &source);
        assert_eq!(screen.wallpaper_failure, "");
        let path = state.profile_path.clone().expect("saved");
        assert!(folder_of(&path).join("wallpaper.png").is_file());
        assert!(
            !state.is_unsaved(),
            "the chosen wallpaper is saved to the profile file"
        );
        let reloaded = Profile::load(&path).expect("reloaded");
        assert_eq!(
            reloaded.settings.branding.wallpaper.as_deref(),
            Some("wallpaper.png")
        );
    }

    #[test]
    fn save_as_takes_the_wallpaper_along_and_replaces_no_other() {
        let (data, mut state, mut screen) = sample();
        let ctx = egui::Context::default();
        let source = data.path().join("source.png");
        picture(&source, 1);
        screen.use_wallpaper(&ctx, &mut state, &source);
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
    fn a_failed_wallpaper_copy_leaves_the_editor_on_the_old_file() {
        let (data, mut state, mut screen) = sample();
        let ctx = egui::Context::default();
        let source = data.path().join("source.png");
        picture(&source, 1);
        screen.use_wallpaper(&ctx, &mut state, &source);
        let first = state.profile_path.clone().expect("saved");
        std::fs::remove_file(folder_of(&first).join("wallpaper.png")).expect("removed");

        let free = tempfile::tempdir().expect("folder");
        screen.save_as(&mut state, &free.path().join("p.json"));
        assert_ne!(screen.failure, "");
        assert!(!free.path().join("p.json").exists());
        assert_eq!(state.profile_path, Some(first));
    }

    #[test]
    fn choosing_a_link_to_the_wallpaper_keeps_the_picture() {
        let (data, mut state, mut screen) = sample();
        let ctx = egui::Context::default();
        let source = data.path().join("source.png");
        picture(&source, 1);
        screen.use_wallpaper(&ctx, &mut state, &source);
        let folder = folder_of(state.profile_path.as_ref().expect("saved")).to_owned();
        let link = folder.join("link.png");
        std::fs::hard_link(folder.join("wallpaper.png"), &link).expect("link");

        screen.use_wallpaper(&ctx, &mut state, &link);
        assert_eq!(screen.wallpaper_failure, "");
        assert_eq!(
            std::fs::read(folder.join("wallpaper.png")).expect("picture"),
            std::fs::read(&source).expect("picture")
        );
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
