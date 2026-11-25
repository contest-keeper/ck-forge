//! The profiles the organization keeps on the Hub: list them, load one into the editor and save
//! the one being edited. Every request runs on a worker thread, like the other screens.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use ck_admin_core::{
    HUB_URL, LoadedProfile, Profile, ProfileSummary, list_profiles, load_hub_profile,
    save_hub_profile, vault,
};

/// What a worker thread reports.
enum Event {
    Listed(Result<Vec<ProfileSummary>, String>),
    Loaded(Result<LoadedProfile, String>),
    Saved(Result<Profile, String>),
}

/// What the profile editor has to do after a request ended.
pub enum Outcome {
    /// A profile was downloaded into the Forge folder: edit it, from its file.
    Loaded(Box<LoadedProfile>),
    /// The profile was saved on the Hub, which gave it this id.
    Saved(Option<String>),
}

pub struct Panel {
    profiles: Vec<ProfileSummary>,
    /// Whether a request is running, so the buttons wait for it.
    busy: bool,
    message: String,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Panel {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            profiles: Vec::new(),
            busy: false,
            message: String::new(),
            events,
            sender,
        }
    }
}

/// Runs a Hub request as the signed-in admin, or says to sign in. The session is read from the
/// vault each time, so a sign-out in the Hub screen is respected.
fn as_admin<T>(
    request: impl FnOnce(&ck_admin_core::HubSession) -> Result<T, ck_admin_core::Error>,
) -> Result<T, String> {
    let session = vault::load().ok_or_else(|| "sign in to the Hub first".to_owned())?;
    request(&session).map_err(|e| e.to_string())
}

impl Panel {
    /// Shows the Hub's profiles and the buttons. `current` is the profile being edited and the
    /// file it is saved in, when there is one.
    pub fn show(
        &mut self,
        ui: &mut eframe::egui::Ui,
        current: Option<(&Profile, &str)>,
    ) -> Option<Outcome> {
        let outcome = self.receive();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, eframe::egui::Button::new("List Hub profiles"))
                .clicked()
            {
                self.start(ui.ctx(), |sender| {
                    Event::Listed(as_admin(|session| list_profiles(HUB_URL, session))).send(sender);
                });
            }
            if let Some((profile, path)) = current
                && ui
                    .add_enabled(!self.busy, eframe::egui::Button::new("Save on the Hub"))
                    .clicked()
            {
                self.save(ui.ctx(), profile.clone(), path);
            }
        });
        let mut chosen = None;
        for summary in &self.profiles {
            ui.horizontal(|ui| {
                ui.label(&summary.name);
                if ui
                    .add_enabled(!self.busy, eframe::egui::Button::new("Load"))
                    .clicked()
                {
                    chosen = Some(summary.id.clone());
                }
            });
        }
        if let Some(id) = chosen {
            self.load(ui.ctx(), id);
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        outcome
    }

    fn receive(&mut self) -> Option<Outcome> {
        let mut outcome = None;
        while let Ok(event) = self.events.try_recv() {
            self.busy = false;
            match event {
                Event::Listed(Ok(profiles)) => {
                    self.message = format!("{} profiles on the Hub", profiles.len());
                    self.profiles = profiles;
                }
                Event::Loaded(Ok(loaded)) => outcome = Some(Outcome::Loaded(Box::new(loaded))),
                Event::Saved(Ok(profile)) => {
                    "Saved on the Hub".clone_into(&mut self.message);
                    outcome = Some(Outcome::Saved(profile.hub_id));
                }
                Event::Listed(Err(why)) | Event::Loaded(Err(why)) | Event::Saved(Err(why)) => {
                    self.message = why;
                }
            }
        }
        outcome
    }

    /// Marks the panel busy and runs `work` on a thread, which sends the answer and wakes the window.
    fn start(
        &mut self,
        ctx: &eframe::egui::Context,
        work: impl FnOnce(&Sender<Event>) + Send + 'static,
    ) {
        self.busy = true;
        self.message.clear();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            work(&sender);
            ctx.request_repaint();
        });
    }

    fn load(&mut self, ctx: &eframe::egui::Context, id: String) {
        let Some(folder) = crate::folder::data_dir().map(|data| data.join("profiles")) else {
            "Could not find the Forge folder".clone_into(&mut self.message);
            return;
        };
        self.start(ctx, move |sender| {
            let result = as_admin(|session| load_hub_profile(HUB_URL, session, &id, &folder));
            Event::Loaded(result).send(sender);
        });
    }

    fn save(&mut self, ctx: &eframe::egui::Context, profile: Profile, path: &str) {
        let path = PathBuf::from(path.trim());
        if path.as_os_str().is_empty() {
            "Save the profile to a file first: the Hub copy takes its wallpaper from beside it"
                .clone_into(&mut self.message);
            return;
        }
        self.start(ctx, move |sender| {
            let result = as_admin(|session| save_hub_profile(HUB_URL, session, &path, profile));
            Event::Saved(result).send(sender);
        });
    }
}

impl Event {
    fn send(self, sender: &Sender<Self>) {
        // The window may be gone by now; then nobody needs the answer.
        let _ = sender.send(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_without_a_file_does_not_start() {
        let mut panel = Panel::default();
        panel.save(&eframe::egui::Context::default(), Profile::template(), "  ");
        assert!(!panel.busy);
        assert!(panel.message.contains("Save the profile to a file first"));
    }

    #[test]
    fn a_finished_request_frees_the_buttons_and_reports_the_hub_id() {
        let mut panel = Panel {
            busy: true,
            ..Panel::default()
        };
        let mut profile = Profile::template();
        profile.hub_id = Some("abc".to_owned());
        Event::Saved(Ok(profile)).send(&panel.sender);
        let outcome = panel.receive();
        assert!(!panel.busy);
        assert!(matches!(outcome, Some(Outcome::Saved(Some(id))) if id == "abc"));
        Event::Listed(Err("sign in to the Hub first".to_owned())).send(&panel.sender);
        assert!(panel.receive().is_none());
        assert_eq!(panel.message, "sign in to the Hub first");
    }
}
