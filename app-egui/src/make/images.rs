//! The image picker: the base images the Hub offers, downloading one, and what a base image
//! supports.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use ck_admin_core::{
    HUB_URL, HubImage, HubImageStep, download_hub_image, list_hub_images,
    prepare_hub_image_download, requires_signed_settings, supports_enrolment, vault,
};

/// What a worker thread reports.
enum Event {
    Images(Result<Vec<HubImage>, String>),
    Progress(&'static str, u64, u64),
    Downloaded(Result<(), String>),
}

pub struct Screen {
    images: Vec<HubImage>,
    /// The raw image to write the chosen Hub image to, as typed.
    out: String,
    /// The base image to ask about, as typed.
    base: String,
    progress: Option<(&'static str, u64, u64)>,
    message: String,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            images: Vec::new(),
            out: String::new(),
            base: String::new(),
            progress: None,
            message: String::new(),
            events,
            sender,
        }
    }
}

/// "12 of 30 MB" for a download in progress.
fn megabytes(done: u64, total: u64) -> String {
    format!("{} of {} MB", done / (1024 * 1024), total / (1024 * 1024))
}

impl Screen {
    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        self.receive();
        ui.heading("Base images");
        ui.horizontal(|ui| {
            if ui.button("Refresh from the Hub").clicked() {
                self.refresh(ui.ctx().clone());
            }
            ui.label("Writes to");
            ui.text_edit_singleline(&mut self.out);
        });
        let busy = self.progress.is_some();
        let mut chosen = None;
        for image in &self.images {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} {} ({} MB)",
                    image.name,
                    image.version,
                    image.size / (1024 * 1024)
                ));
                if ui
                    .add_enabled(!busy, eframe::egui::Button::new("Download"))
                    .clicked()
                {
                    chosen = Some(image.id.clone());
                }
            });
        }
        if let Some(id) = chosen {
            self.download(&id, ui.ctx().clone());
        }
        if let Some((phase, done, total)) = self.progress {
            ui.label(format!("{phase}: {}", megabytes(done, total)));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Base image");
            ui.text_edit_singleline(&mut self.base);
            if ui.button("Check").clicked() {
                self.message = support_text(&PathBuf::from(self.base.trim()));
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }

    fn receive(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Images(Ok(images)) => self.images = images,
                Event::Images(Err(why)) | Event::Downloaded(Err(why)) => {
                    self.progress = None;
                    self.message = why;
                }
                Event::Progress(phase, done, total) => self.progress = Some((phase, done, total)),
                Event::Downloaded(Ok(())) => {
                    self.progress = None;
                    "Saved the base image".clone_into(&mut self.message);
                }
            }
        }
    }

    fn refresh(&self, ctx: eframe::egui::Context) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = vault::load()
                .ok_or_else(|| "sign in to the Hub first".to_owned())
                .and_then(|session| list_hub_images(HUB_URL, &session).map_err(|e| e.to_string()));
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Images(result));
            ctx.request_repaint();
        });
    }

    fn download(&mut self, id: &str, ctx: eframe::egui::Context) {
        let out = PathBuf::from(self.out.trim());
        if self.out.trim().is_empty() {
            "Type where the base image goes".clone_into(&mut self.message);
            return;
        }
        self.progress = Some(("starting", 0, 0));
        let sender = self.sender.clone();
        let id = id.to_owned();
        thread::spawn(move || {
            let result = vault::load()
                .ok_or_else(|| "sign in to the Hub first".to_owned())
                .and_then(|session| {
                    let download = prepare_hub_image_download(HUB_URL, &session, &id)
                        .map_err(|e| e.to_string())?;
                    download_hub_image(download, &out, |step, done, total| {
                        let phase = match step {
                            HubImageStep::Download => "downloading",
                            HubImageStep::Import => "importing",
                        };
                        let _ = sender.send(Event::Progress(phase, done, total));
                        ctx.request_repaint();
                    })
                    .map_err(|e| e.to_string())
                });
            let _ = sender.send(Event::Downloaded(result));
            ctx.request_repaint();
        });
    }
}

fn support_text(base: &std::path::Path) -> String {
    let yes = |value: bool| if value { "yes" } else { "no" };
    match (requires_signed_settings(base), supports_enrolment(base)) {
        (Ok(signed), Ok(enrolment)) => format!(
            "Settings must be signed by the Hub: {}. First-boot enrolment: {}.",
            yes(signed),
            yes(enrolment)
        ),
        (Err(error), _) | (_, Err(error)) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_base_image_is_explained_not_shown_as_support() {
        let text = support_text(std::path::Path::new("no-such-image.img"));
        assert!(!text.contains("signed by the Hub"), "{text}");
    }

    #[test]
    fn sizes_are_shown_in_megabytes() {
        assert_eq!(megabytes(5 * 1024 * 1024, 30 * 1024 * 1024), "5 of 30 MB");
    }
}
