//! The profiles the organization keeps on the Hub: list them, load one into the editor and save
//! the one being edited. Every request runs on a worker thread, like the other screens.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use ck_admin_core::{
    HUB_URL, LoadedProfile, Profile, ProfileSummary, list_profiles, load_hub_profile,
    new_profile_id, save_hub_profile, vault,
};

/// What the profile editor has to do after a request ended.
pub enum Outcome {
    Listed(Vec<ProfileSummary>),
    /// A profile was downloaded into the Forge folder: edit it, from its file.
    Loaded(Box<LoadedProfile>),
    /// The profile as it was saved on the Hub and in its file.
    Saved(Box<Profile>, PathBuf),
    Failed(String),
}

pub struct Panel {
    /// Whether a request is running, so the buttons wait for it.
    busy: bool,
    events: Receiver<Outcome>,
    sender: Sender<Outcome>,
}

impl Default for Panel {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            busy: false,
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
    /// Whether a request is running. The editor is locked meanwhile: a save that finishes writes the
    /// profile it was started with to its file, which must not undo a newer edit or a new profile.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Takes what a worker thread reported. Called every frame.
    pub fn receive(&mut self) -> Option<Outcome> {
        let outcome = self.events.try_recv().ok()?;
        self.busy = false;
        Some(outcome)
    }

    /// Marks the panel busy and runs `work` on a thread, which sends the answer and wakes the window.
    fn start(
        &mut self,
        ctx: &eframe::egui::Context,
        work: impl FnOnce() -> Result<Outcome, String> + Send + 'static,
    ) {
        self.busy = true;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(work().unwrap_or_else(Outcome::Failed));
            ctx.request_repaint();
        });
    }

    pub fn list(&mut self, ctx: &eframe::egui::Context) {
        self.start(ctx, || {
            as_admin(|session| list_profiles(HUB_URL, session)).map(Outcome::Listed)
        });
    }

    /// Downloads a profile into a folder of its own under `folder`.
    pub fn load(&mut self, ctx: &eframe::egui::Context, id: String, folder: PathBuf) {
        self.start(ctx, move || {
            as_admin(|session| load_hub_profile(HUB_URL, session, &id, &folder))
                .map(|loaded| Outcome::Loaded(Box::new(loaded)))
        });
    }

    /// Starts a save of the profile saved at `path` and returns the id it runs under when the
    /// profile had none yet. The caller keeps that id even if the save fails, so that a retry
    /// updates the same Hub profile and does not make another one.
    pub fn save(
        &mut self,
        ctx: &eframe::egui::Context,
        mut profile: Profile,
        path: PathBuf,
    ) -> Result<Option<String>, String> {
        // The Hub copy is replaced before the local file is checked, so a profile with problems must
        // never be sent: the same check as a local save.
        if !profile.problems().is_empty() {
            return Err("Fix the problems of the profile first: the Hub must not get settings that cannot be saved".to_owned());
        }
        let assigned = profile.hub_id.is_none().then(new_profile_id);
        if assigned.is_some() {
            profile.hub_id.clone_from(&assigned);
        }
        self.start(ctx, move || {
            as_admin(|session| save_hub_profile(HUB_URL, session, &path, profile))
                .map(|saved| Outcome::Saved(Box::new(saved), path))
        });
        Ok(assigned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_with_problems_is_not_sent_to_the_hub() {
        let mut panel = Panel::default();
        let mut profile = Profile::template();
        profile
            .settings
            .branding
            .bookmarks
            .push(ck_protocol::Bookmark {
                title: String::new(),
                url: String::new(),
            });
        let why = panel
            .save(&eframe::egui::Context::default(), profile, "p.json".into())
            .expect_err("refused");
        assert!(why.contains("Fix the problems"));
        assert!(!panel.busy);
    }

    #[test]
    fn a_finished_request_frees_the_buttons() {
        let mut panel = Panel {
            busy: true,
            ..Panel::default()
        };
        assert!(panel.receive().is_none());
        assert!(panel.busy);
        let _ = panel.sender.send(Outcome::Failed("no".to_owned()));
        assert!(matches!(panel.receive(), Some(Outcome::Failed(why)) if why == "no"));
        assert!(!panel.busy);
    }
}
