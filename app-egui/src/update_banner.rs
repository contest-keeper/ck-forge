//! The window's part of the updater: the check when the window opens, the offer, the download and
//! the restart (on macOS, the opened disk image). What is decided (is it newer, is it mandatory, is
//! it signed) lives in `update.rs`.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use crate::update::{self, Available};

/// What a worker thread reports.
enum Event {
    Found(Result<Option<Available>, String>),
    Progress(u64, u64),
    /// The download was checked and the installer started or the disk image opened.
    Started(Result<(), String>),
}

enum State {
    /// Nothing to say: no newer build, no key, or the check has not answered.
    Quiet,
    Offer(Available),
    Downloading(u64, u64),
    /// macOS: the disk image is open and the person takes it from here.
    Opened,
    /// The download or the start failed: the update stays, so it can be tried again.
    Failed(String),
}

pub struct Banner {
    state: State,
    /// The update that was offered, kept while it downloads or after it failed: a mandatory one
    /// keeps the window blocked and any one can be tried again.
    pending: Option<Available>,
    started: bool,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Banner {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            state: State::Quiet,
            pending: None,
            started: false,
            events,
            sender,
        }
    }
}

impl Banner {
    /// Whether this build is too old to be used until it has updated.
    pub fn blocks(&self) -> bool {
        !matches!(self.state, State::Quiet) && self.pending.as_ref().is_some_and(|u| u.mandatory)
    }

    /// Starts the check the first time and takes what the worker threads reported. Closes the
    /// window when an update was started.
    pub fn poll(&mut self, ctx: &eframe::egui::Context) {
        if !self.started {
            self.started = true;
            self.check(ctx.clone());
        }
        self.receive(ctx);
    }

    /// Whether there is anything to show.
    pub fn visible(&self) -> bool {
        !matches!(self.state, State::Quiet)
    }

    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        match &self.state {
            State::Quiet => {}
            State::Offer(update) => {
                let update = update.clone();
                self.offer_ui(ui, &update);
            }
            State::Downloading(done, total) => {
                let (done, total) = (done / (1024 * 1024), total / (1024 * 1024));
                ui.label(format!("Downloading the update: {done} of {total} MB"));
                ui.spinner();
            }
            State::Opened => {
                ui.label(
                    "The disk image is open. Close Keeper Forge, then drag the new Keeper Forge to Applications and replace the old one.",
                );
                if ui.button("Close Keeper Forge").clicked() {
                    ui.ctx()
                        .send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                }
                // The disk image may have been ejected or its window lost: Install opens it again.
                if let Some(update) = self.pending.clone()
                    && ui.button("Back").clicked()
                {
                    self.state = State::Offer(update);
                }
            }
            State::Failed(why) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("Update: {why}"));
                if let Some(update) = self.pending.clone()
                    && ui.button("Try again").clicked()
                {
                    self.download(update, ui.ctx().clone());
                }
            }
        }
    }

    fn offer_ui(&mut self, ui: &mut eframe::egui::Ui, update: &Available) {
        let text = if update.mandatory {
            format!(
                "This version of Keeper Forge is no longer supported. Version {} must be installed to continue.",
                update.version
            )
        } else {
            format!("Keeper Forge {} is available.", update.version)
        };
        ui.label(text);
        ui.label(if cfg!(target_os = "macos") {
            "Install downloads the disk image and opens it. You then drag Keeper Forge to Applications."
        } else {
            "The window closes and reopens when it is installed."
        });
        ui.horizontal(|ui| {
            if ui.button("Install").clicked() {
                self.download(update.clone(), ui.ctx().clone());
            }
            if update.mandatory {
                if ui.button("Close").clicked() {
                    ui.ctx()
                        .send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                }
            } else {
                if ui.button("Skip this version").clicked() {
                    if let Some(folder) = crate::folder::data_dir() {
                        let _ = update::remember_skipped(&folder, &update.version);
                    }
                    self.state = State::Quiet;
                    self.pending = None;
                }
                if ui.button("Later").clicked() {
                    self.state = State::Quiet;
                    self.pending = None;
                }
            }
        });
    }

    fn receive(&mut self, ctx: &eframe::egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Found(Ok(Some(update))) => {
                    let skipped = crate::folder::data_dir()
                        .and_then(|folder| update::skipped_version(&folder));
                    if update::worth_offering(&update, skipped.as_deref()) {
                        self.pending = Some(update.clone());
                        self.state = State::Offer(update);
                    }
                }
                // Offline or no feed yet: not worth bothering the admin, the next start tries again.
                Event::Found(Ok(None) | Err(_)) => {}
                Event::Progress(done, total) => self.state = State::Downloading(done, total),
                Event::Started(Ok(())) if cfg!(target_os = "macos") => {
                    self.state = State::Opened;
                }
                Event::Started(Ok(())) => {
                    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                }
                Event::Started(Err(why)) => self.state = State::Failed(why),
            }
        }
    }

    /// Checks in the background, so a slow or missing network never delays the window.
    fn check(&self, ctx: eframe::egui::Context) {
        if !update::enabled() {
            return;
        }
        let sender = self.sender.clone();
        thread::spawn(move || {
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Found(update::check()));
            ctx.request_repaint();
        });
    }

    fn download(&mut self, update: Available, ctx: eframe::egui::Context) {
        self.state = State::Downloading(0, 0);
        let sender = self.sender.clone();
        thread::spawn(move || {
            let folder = std::env::temp_dir().join(format!("ck-update-{}", std::process::id()));
            let result = update::download(&update, &folder, |done, total| {
                let _ = sender.send(Event::Progress(done, total));
                ctx.request_repaint();
            })
            .and_then(|file| update::install(&file));
            let _ = sender.send(Event::Started(result));
            ctx.request_repaint();
        });
    }
}
