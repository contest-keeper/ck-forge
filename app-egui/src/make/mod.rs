//! The "Make sticks" page: the base image, the sticks, the write and the old keys, as
//! `MakePage.svelte` of the earlier window had them.

pub mod base_image;
mod format;
pub mod images;
mod progress;
mod stick_job;
pub mod sticks;
pub mod write;

use eframe::egui::{Context, Ui};

use crate::state::AppState;
use crate::widgets::{self, Tone};

#[derive(Default)]
pub struct Page {
    images: images::Screen,
    write: write::Screen,
    sticks: sticks::Screen,
    /// The admin the old keys were listed for.
    email: Option<String>,
}

impl Page {
    /// Called every frame, whatever page is shown: takes what the worker threads reported, so a
    /// write that ends while another page is open still counts.
    pub fn poll(&mut self, ctx: &Context, state: &mut AppState) {
        self.images.receive();
        self.sticks.receive();
        self.write.receive(ctx);
        self.follow(ctx, state);
        if let Some((previous, retire, account)) = self.write.take_finished() {
            self.sticks
                .after_write(previous, retire, account.as_deref(), ctx);
        }
        state.written |= self.write.written;
        state.busy = self.locked() || self.images.creator.busy() || self.sticks.busy();
    }

    /// Whether something runs that the steps wait for: a write with its old keys, a download or an
    /// import. A build does not stop a write from an image that is already there.
    fn locked(&self) -> bool {
        self.write.writing() || self.images.busy() || self.sticks.retiring_old()
    }

    /// Passes a new base image to the shared state, and the shared state to the steps.
    fn follow(&mut self, ctx: &Context, state: &mut AppState) {
        if let Some(base) = self.images.take_new_base() {
            state.set_base_path(Some(base));
            self.write.base_replaced();
        }
        if self.email != state.hub_email {
            self.email.clone_from(&state.hub_email);
            self.sticks.forget();
            self.images.forget_account();
        }
        self.write.follow(
            state.base_path(),
            state.profile_path.as_deref(),
            state.hub_email.as_deref(),
            ctx,
        );
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        if state.profile_path.is_none() {
            widgets::note(
                ui,
                Tone::Error,
                "Go back to the Contest profile step and continue to save the profile.",
            );
        }
        let signed_in = state.hub_email.is_some();
        let locked = self.locked();
        widgets::card(ui, |ui| {
            self.images.show(ui, state.base_path(), signed_in, locked);
        });
        // A file chosen just now counts before the next step is drawn.
        self.follow(ui.ctx(), state);
        let profile = state.profile.as_ref();
        let network = profile
            .and_then(|profile| profile.settings.wifi.as_ref())
            .map(|wifi| wifi.ssid.as_str());
        let name = profile.map_or("", |profile| profile.name.as_str());
        let dev = state.dev_tools();
        widgets::card(ui, |ui| self.write.sticks_ui(ui, network, locked, dev));
        widgets::card(ui, |ui| self.write.erase_ui(ui, name, locked));
        widgets::note(ui, Tone::Error, &self.sticks.old_failure);
        self.write.dialog_ui(ui.ctx());
        // Where the window cannot write sticks, this is the only write from the saved profile.
        if dev || !write::CAN_WRITE {
            let title = if dev {
                "Development: test the write into a file"
            } else {
                "Write into an image file"
            };
            widgets::details(ui, title, |ui| self.write.file_ui(ui, locked));
        }
    }

    /// Clears transient state when leaving the Retire sticks page.
    pub fn leave_retire(&mut self) {
        self.sticks.leave();
    }

    /// The page that retires the keys of old sticks.
    pub fn show_retire(&mut self, ui: &mut Ui, state: &AppState) {
        if state.hub_email.is_none() {
            widgets::note(ui, Tone::Error, "Sign in to the Hub to retire old sticks.");
            return;
        }
        widgets::note(ui, Tone::Error, &self.sticks.old_failure);
        widgets::card(ui, |ui| self.sticks.show(ui, self.locked()));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ck_admin_core::{HubImage, HubStick};

    use super::*;
    use crate::app::{Forge, MAKE};
    use crate::make::stick_job::Progress;
    use crate::shots::capture;

    const EMAIL: &str = "admin@example.org";
    const BASE: &str = r"D:\images\keeper-os-0.4.0.img";

    #[test]
    fn a_chosen_image_becomes_the_base_of_the_window_and_of_the_next_write() {
        let mut page = Page::default();
        let mut state = AppState::load(None);
        page.images.method = Some(images::Method::File);
        page.images.sample_pick(PathBuf::from("new.img"));
        page.poll(&Context::default(), &mut state);
        assert_eq!(state.base_path(), Some(std::path::Path::new("new.img")));
        assert_eq!(page.write.base, Some(PathBuf::from("new.img")));
    }

    fn make(forge: &mut Forge, base: bool) {
        forge.step = MAKE;
        forge.state.profile_path = Some(PathBuf::from("contest-profile.json"));
        if let Some(profile) = &mut forge.state.profile {
            "My contest".clone_into(&mut profile.name);
        }
        forge.state.set_base_path(base.then(|| PathBuf::from(BASE)));
        // Already followed, so the first frame asks the sample paths nothing.
        forge.make.write.base = forge.state.base_path().map(Into::into);
    }

    fn sign_in(forge: &mut Forge) {
        forge.hub.state = crate::side::hub_sign_in::State::SignedIn(EMAIL.to_owned());
        forge.state.hub_email = Some(EMAIL.to_owned());
        forge.make.email = Some(EMAIL.to_owned());
        forge.make.write.email = Some(EMAIL.to_owned());
        forge.make.write.support.enrolment = Some(false);
    }

    fn old_stick() -> HubStick {
        serde_json::from_str(
            r#"{"stick_id":"03c9f556","team":"Team Alpha","site":"Hall A","machine_number":7,
                "hardware":{"id":"0a0b0c0d","write_count":2}}"#,
        )
        .expect("stick")
    }

    /// Signed in, four disks listed and `chosen` ticked.
    fn disks(forge: &mut Forge, chosen: &[u32]) {
        use write::tests::disk;
        make(forge, true);
        sign_in(forge);
        let write = &mut forge.make.write;
        write.support.enrolment = Some(true);
        let mut system = disk(0, "Samsung SSD 980", "S64DNF0", 1_000_000_000_000, "NVMe");
        system.is_system = true;
        write.disks = vec![
            disk(1, "SanDisk Ultra", "4C5300012", 30_800_000_000, "USB"),
            disk(
                2,
                "Kingston DataTraveler",
                "60A44C42",
                61_900_000_000,
                "USB",
            ),
            disk(3, "Generic Flash Disk", "0001", 3_800_000_000, "USB"),
            system,
        ];
        write.size = 4_800_000_000;
        write.chosen = chosen.iter().copied().collect();
        write.earlier.insert(1, old_stick());
    }

    /// The Hub picker with one image listed.
    fn offered(forge: &mut Forge) {
        let image: HubImage = serde_json::from_str(
            r#"{"id":"i1","name":"Keeper OS","version":"0.4.0","profile":"standard",
                "size":4820000000,"filename":"keeper-os-0.4.0.img.zst","url":"","sha256":""}"#,
        )
        .expect("image");
        forge.make.images.method = Some(images::Method::Hub);
        forge.make.images.hub = Some(vec![image]);
        forge.make.images.chosen = 0;
    }

    #[test]
    #[ignore = "writes pictures; needs CK_SHOTS_DIR and a graphics adapter"]
    fn shots_of_make() {
        capture("make-nobase", 0.0, |forge| make(forge, false));
        capture("make-nobase-hub", 0.0, |forge| {
            make(forge, false);
            sign_in(forge);
            offered(forge);
        });
        capture("make-nobase-file", 0.0, |forge| {
            make(forge, false);
            forge.make.images.method = Some(images::Method::File);
            forge.make.images.archive = Some(PathBuf::from(r"D:\dl\keeper-os-0.4.0.img.zst"));
        });
        capture("make-nobase-build", 222.0, |forge| {
            make(forge, false);
            forge.make.images.method = Some(images::Method::Build);
        });
        capture("make-default", 0.0, |forge| make(forge, true));
        capture("make-disks", 202.0, |forge| disks(forge, &[1]));
        capture("make-confirm", 4000.0, |forge| disks(forge, &[1, 2]));
        capture("make-retire-dialog", 590.0, |forge| {
            disks(forge, &[1, 2]);
            "erase 1 2".clone_into(&mut forge.make.write.typed);
            forge.make.write.sample_question();
        });
        capture("make-writing", 790.0, |forge| {
            disks(forge, &[1]);
            forge.hub.state = crate::side::hub_sign_in::State::SignedOut;
            forge.state.hub_email = None;
            forge.make.email = None;
            forge.make.write.email = None;
            forge.make.write.earlier.clear();
            "erase 1".clone_into(&mut forge.make.write.typed);
            forge.make.write.sample_job(Progress {
                phase: "writing",
                done: 1_900_000_000,
                total: 4_800_000_000,
                checked: 800_000_000,
            });
        });
        capture("make-done", 4000.0, |forge| {
            disks(forge, &[1, 2]);
            forge.state.written = true;
            forge.make.write.written = true;
            forge.make.write.earlier.clear();
            "Written and read back on 2 sticks in 14:32. The check value (SHA-256) is \
             a3f1c9d2e8b7406f5a1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a."
                .clone_into(&mut forge.make.write.success);
            forge.make.sticks.previous = vec![old_stick()];
        });
        capture("make-failed", 4000.0, |forge| {
            disks(forge, &[1]);
            forge.hub.state = crate::side::hub_sign_in::State::SignedOut;
            forge.state.hub_email = None;
            forge.make.email = None;
            forge.make.write.email = None;
            "disk 1: the stick was removed while it was being written"
                .clone_into(&mut forge.make.write.failure);
        });
        // States the earlier pictures do not have.
        capture("make-download", 0.0, |forge| {
            make(forge, false);
            sign_in(forge);
            offered(forge);
            forge.make.images.sample_download();
        });
        capture("make-retire-other", 4000.0, |forge| {
            disks(forge, &[]);
            forge.make.sticks.previous = vec![old_stick()];
            forge.make.sticks.asking = Some("03c9f556".to_owned());
        });
    }
}
