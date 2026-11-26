//! The "Make sticks" page. Until its steps are rebuilt, it holds the earlier screens as they were.

pub mod base_image;
pub mod images;
mod stick_job;
pub mod sticks;
pub mod write;

use std::path::PathBuf;

use eframe::egui::{Context, Ui};

use crate::state::AppState;
use crate::widgets::{self, Input, Tone};

#[derive(Default)]
pub struct Page {
    images: images::Screen,
    base_image: base_image::Screen,
    sticks: sticks::Screen,
    write: write::Screen,
    /// The base image as typed.
    base: String,
}

impl Page {
    /// Called every frame, whatever page is shown: takes what the worker threads reported, so a
    /// write that ends while another page is open still counts.
    pub fn poll(&mut self, _ctx: &Context, state: &mut AppState) {
        self.images.receive();
        self.base_image.receive();
        self.sticks.receive();
        self.write.receive();
        state.written |= self.write.written;
        state.busy = self.images.busy() || self.base_image.busy() || self.write.busy();
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        if state.profile_path.is_none() {
            widgets::note(
                ui,
                Tone::Error,
                "Go back to the Contest profile step and continue to save the profile.",
            );
        }
        widgets::card(ui, |ui| {
            let saved = state.base_path().map(|path| path.display().to_string());
            let saved = saved.unwrap_or_default();
            if saved != self.base.trim() {
                self.base = saved;
            }
            widgets::field(ui, "Base image", None, None, |ui, label| {
                let input = Input::new(&mut self.base).show(ui).labelled_by(label);
                if input.changed() {
                    let typed = self.base.trim();
                    state.set_base_path((!typed.is_empty()).then(|| PathBuf::from(typed)));
                }
            });
        });
        widgets::card(ui, |ui| {
            self.write
                .use_files(state.base_path(), state.profile_path.as_deref());
            self.write.show(ui);
        });
        widgets::details(ui, "Download or build a base image", |ui| {
            self.images.show(ui);
            ui.separator();
            self.base_image.show(ui);
        });
        widgets::details(ui, "Sticks on the Hub", |ui| self.sticks.show(ui));
    }
}
