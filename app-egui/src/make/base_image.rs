//! Building a base image from the catalog of official images (15 to 40 minutes, so it runs on a
//! worker thread and reports its steps): `BaseImageCreator.svelte` of the earlier window.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use ck_admin_core::{
    CatalogEntry, CreateOptions, CreateStep, catalog, create_base_image, find_catalog_entry,
};
use eframe::egui::{Context, Ui};

use crate::make::format;
use crate::widgets::{self, Btn, Tone};

/// What a worker thread reports.
enum Event {
    Progress(CreateStep, u64, u64),
    /// Where the image went, and its SHA-256 or why there is none.
    Made(PathBuf, Result<String, String>),
}

pub struct Screen {
    entries: Vec<CatalogEntry>,
    /// The place in `entries` of the chosen system.
    chosen: usize,
    /// Where the new base image goes. It must not exist yet.
    out: Option<PathBuf>,
    /// The release's `ck-os-patch.tar.gz`; without one the image is not Keeper OS yet.
    patch: Option<PathBuf>,
    busy: bool,
    /// The step that runs and how far it is, once the build reported one.
    step: Option<(CreateStep, u64, u64)>,
    failure: String,
    hash: String,
    /// The image that was just built, until the page takes it as the base.
    made: Option<PathBuf>,
    /// Another base image was chosen after this build started.
    outdated: bool,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        let (entries, failure) = match catalog() {
            Ok(entries) => (entries, String::new()),
            Err(error) => (Vec::new(), error.to_string()),
        };
        Self {
            entries,
            chosen: 0,
            out: None,
            patch: None,
            busy: false,
            step: None,
            failure,
            hash: String::new(),
            made: None,
            outdated: false,
            events,
            sender,
        }
    }
}

/// The words for what a step of making an image is doing.
fn step_name(step: CreateStep) -> &'static str {
    match step {
        CreateStep::Download => "Downloading the official image",
        CreateStep::Convert => "Preparing the disk",
        CreateStep::Provision => "Installing the desktop and contest tools (this takes a while)",
        CreateStep::Patch => "Applying the Keeper OS patch",
        CreateStep::Hash => "Checking the finished image",
    }
}

/// "...: 40%" for a step with a known total, and just the step when the total is not known. The
/// installation counts seconds up to its time limit, not bytes, so it says how long it has run.
fn progress_text(step: CreateStep, done: u64, total: u64) -> String {
    let name = step_name(step);
    if step == CreateStep::Provision {
        let (so_far, most) = (done / 60, total / 60);
        return format!("{name} \u{b7} {so_far} min so far (at most {most} min)");
    }
    if total == 0 {
        name.to_owned()
    } else {
        format!("{name} \u{b7} {}%", format::percent(done, total))
    }
}

/// Downloads are kept between builds, so a second image does not download again: under the local
/// data folder on Windows, in the caches folder on macOS (the same places `ck-forge image create`
/// uses), and in the Forge folder elsewhere.
fn cache_folder() -> PathBuf {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(base).join("ck-forge").join("cache");
    }
    if cfg!(target_os = "macos")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join("Library/Caches/ck-forge");
    }
    crate::folder::data_dir()
        .map_or_else(|| PathBuf::from("ck-forge-cache"), |dir| dir.join("cache"))
}

/// The accelerator QEMU uses to install the image: the Windows hypervisor, the macOS one on an Intel
/// Mac (the guest is x86), and plain emulation anywhere else, which is slow but works.
fn accelerator() -> &'static str {
    if cfg!(windows) {
        "whpx"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "hvf"
    } else {
        "tcg"
    }
}

impl Screen {
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// The image that was just built.
    pub fn take_made(&mut self) -> Option<PathBuf> {
        self.made.take()
    }

    /// Another base image was chosen: a build from before keeps its file but is not chosen.
    pub fn superseded(&mut self) {
        self.made = None;
        self.outdated = self.busy;
    }

    pub fn show(&mut self, ui: &mut Ui, locked: bool) {
        let idle = !self.busy && !locked;
        widgets::subtitle(ui, "Build a base image");
        widgets::hint(
            ui,
            "Downloads an official image, installs a desktop and the contest tools in a virtual \
             machine, and adds the Keeper OS patch. Needs about 30 GB free, QEMU, and 15 to 40 \
             minutes. The new image is then used as the base image. Most admins download one \
             instead.",
        );
        let options: Vec<(usize, &str)> = self
            .entries
            .iter()
            .map(|entry| entry.label.as_str())
            .enumerate()
            .collect();
        let chosen = &mut self.chosen;
        widgets::path_box(ui, |ui| {
            widgets::strong(ui, "Which system");
            ui.add_enabled_ui(idle, |ui| {
                ui.set_max_width(320.0);
                widgets::select(ui, "catalog", chosen, &options);
            });
        });
        self.out_ui(ui, idle);
        self.patch_ui(ui, idle);
        widgets::path_box(ui, |ui| self.make_row(ui, idle));
        widgets::note(ui, Tone::Error, &self.failure);
        if !self.hash.is_empty() {
            let chosen = if self.outdated {
                "Base image made, but not chosen because you chose another one meanwhile."
            } else {
                "Base image made and chosen."
            };
            let text = format!("{chosen} Its check value (SHA-256) is {}.", self.hash);
            widgets::note(ui, Tone::Ok, &text);
        }
    }

    fn out_ui(&mut self, ui: &mut Ui, idle: bool) {
        let name = self
            .out
            .as_deref()
            .map_or_else(|| "Where to save the image".to_owned(), format::file_name);
        let full = self.out.as_ref().map(|path| path.display().to_string());
        let below: Vec<&str> = full.iter().map(String::as_str).collect();
        let save = widgets::path_row(ui, &name, &below, |ui| {
            let button = Btn::new("Save as\u{2026}").small();
            button.enabled(idle).show(ui).clicked()
        });
        if save
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Disk image", &["img"])
                .set_file_name("stick.img")
                .save_file()
        {
            self.out = Some(path);
        }
    }

    fn patch_ui(&mut self, ui: &mut Ui, idle: bool) {
        let name = self.patch.as_deref().map_or_else(
            || "Keeper OS patch (ck-os-patch.tar.gz)".to_owned(),
            format::file_name,
        );
        let below =
            ["Without it the image is a plain desktop and cannot report to Keeper Hub. Needs WSL."];
        let choose = widgets::path_row(ui, &name, &below, |ui| {
            let button = Btn::new("Choose\u{2026}").small();
            button.enabled(idle).show(ui).clicked()
        });
        if choose
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Keeper OS patch", &["gz"])
                .pick_file()
        {
            self.patch = Some(path);
        }
    }

    /// The button that starts the build, and beside it how far it is or why it cannot start.
    fn make_row(&mut self, ui: &mut Ui, idle: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let text = if self.busy {
                "Making the image\u{2026}"
            } else {
                "Make the base image\u{2026}"
            };
            let ready = idle && !self.entries.is_empty() && self.out.is_some();
            if Btn::new(text).primary().enabled(ready).show(ui).clicked() {
                self.make(ui.ctx().clone());
            }
            if let Some((step, done, total)) = self.step {
                ui.vertical(|ui| {
                    widgets::hint(ui, &progress_text(step, done, total));
                    widgets::progress_bar(ui, format::fraction(done, total));
                });
            } else if self.busy {
                widgets::hint(ui, "Starting\u{2026}");
            } else if self.out.is_none() {
                widgets::hint(ui, "Choose where to save the image first.");
            } else if self.entries.is_empty() {
                widgets::hint(ui, "No system is available to build from.");
            }
        });
    }

    /// Takes what the worker thread reported.
    pub fn receive(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Progress(step, done, total) => self.step = Some((step, done, total)),
                Event::Made(out, result) => {
                    self.busy = false;
                    self.step = None;
                    match result {
                        Ok(hash) => {
                            self.hash = hash;
                            self.made = (!self.outdated).then_some(out);
                        }
                        Err(why) => self.failure = why,
                    }
                }
            }
        }
    }

    fn make(&mut self, ctx: Context) {
        let Some(out) = self.out.clone() else {
            "Choose where to save the image first.".clone_into(&mut self.failure);
            return;
        };
        let id = self
            .entries
            .get(self.chosen)
            .map_or("", |entry| entry.id.as_str());
        let entry = match find_catalog_entry(id) {
            Ok(entry) => entry,
            Err(error) => {
                self.failure = error.to_string();
                return;
            }
        };
        let options = CreateOptions {
            entry,
            out,
            cache: cache_folder(),
            qemu_dir: None,
            accel: accelerator().to_owned(),
            memory_mb: 4096,
            timeout: Duration::from_secs(90 * 60),
            patch: self.patch.clone(),
        };
        self.busy = true;
        self.outdated = false;
        self.failure.clear();
        self.hash.clear();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let mut last = Instant::now();
            let result = create_base_image(&options, |step, done, total| {
                // Four updates a second at most, but the end of a step is always sent.
                if last.elapsed() < Duration::from_millis(250) && done != total {
                    return;
                }
                last = Instant::now();
                let _ = sender.send(Event::Progress(step, done, total));
                ctx.request_repaint();
            })
            .map(|record| record.image_sha256)
            .map_err(|e| e.to_string());
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Made(options.out.clone(), result));
            ctx.request_repaint();
        });
    }
}

/// States for the tests of the step around this one. Nothing is built.
#[cfg(test)]
impl Screen {
    pub(super) fn sample_running(&mut self) {
        self.busy = true;
    }

    pub(super) fn sample_finish(&mut self, out: PathBuf) {
        let made = Event::Made(out, Ok("abc".to_owned()));
        assert!(self.sender.send(made).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_installation_shows_the_time_that_passed_and_not_a_percentage() {
        let text = progress_text(CreateStep::Provision, 15 * 60, 90 * 60);
        assert!(text.ends_with("15 min so far (at most 90 min)"), "{text}");
    }

    #[test]
    fn a_step_without_a_total_shows_only_its_name() {
        let text = progress_text(CreateStep::Convert, 0, 0);
        assert_eq!(text, "Preparing the disk");
        assert!(progress_text(CreateStep::Download, 5, 10).ends_with("50%"));
    }

    #[test]
    fn a_built_image_becomes_the_base_and_a_failed_build_does_not() {
        let mut screen = Screen::default();
        let out = PathBuf::from("new.img");
        let failed = Event::Made(out.clone(), Err("no QEMU".to_owned()));
        assert!(screen.sender.send(failed).is_ok());
        screen.receive();
        assert_eq!(screen.take_made(), None);
        let made = Event::Made(out.clone(), Ok("abc".to_owned()));
        assert!(screen.sender.send(made).is_ok());
        screen.receive();
        assert_eq!(screen.take_made(), Some(out));
    }

    #[test]
    fn making_without_a_place_for_the_image_starts_nothing() {
        let mut screen = Screen::default();
        screen.make(Context::default());
        assert!(!screen.busy());
        assert!(screen.failure.contains("where to save"));
    }
}
