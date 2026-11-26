//! Step 1, the base image: one from this PC, one downloaded from the Hub, or one built here
//! (`BaseImageStep.svelte` and `HubImagePicker.svelte` of the earlier window).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use ck_admin_core::{
    HUB_URL, HubImage, HubImageStep, download_hub_image, import_image, list_hub_images,
    prepare_hub_image_download, vault,
};
use eframe::egui::{Context, Ui};

use crate::make::progress::Bar;
use crate::make::stick_job::Progress;
use crate::make::{base_image, format};
use crate::widgets::{self, Btn, Input, Tone};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Method {
    Hub,
    File,
    Build,
}

/// What a worker thread reports.
enum Event {
    Images(Result<Vec<HubImage>, String>),
    Progress(Progress),
    /// A download or an import ended: where the new base image is, or why there is none.
    Finished(Result<PathBuf, String>),
}

pub struct Screen {
    /// How the admin is getting another image, once they asked to.
    pub(super) method: Option<Method>,
    /// The compressed download that was chosen, which is checked before it is used.
    pub(super) archive: Option<PathBuf>,
    sha256: String,
    /// The images the Hub offers, once asked.
    pub(super) hub: Option<Vec<HubImage>>,
    /// The place in `hub` of the chosen image; past the end while none is.
    pub(super) chosen: usize,
    loading: bool,
    /// A download or an import is running.
    importing: bool,
    bar: Bar,
    failure: String,
    /// An image that is to be the base from now on, until the page takes it.
    new_base: Option<PathBuf>,
    pub(super) creator: base_image::Screen,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            method: None,
            archive: None,
            sha256: String::new(),
            hub: None,
            chosen: usize::MAX,
            loading: false,
            importing: false,
            bar: Bar::default(),
            failure: String::new(),
            new_base: None,
            creator: base_image::Screen::default(),
            events,
            sender,
        }
    }
}

/// Whether `text` can be a SHA-256: 64 hex digits.
fn is_sha256(text: &str) -> bool {
    let text = text.trim();
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Where a new base image goes, asked with the system's dialog.
fn choose_base_to_save() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Disk image", &["img"])
        .set_file_name("keeper-os.img")
        .save_file()
}

/// Passes progress on four times a second at most, but always a new step and the end of one.
fn throttled(sender: Sender<Event>, ctx: Context) -> impl FnMut(&'static str, u64, u64) {
    let mut last = Instant::now();
    let mut previous = "";
    move |phase, done, total| {
        if phase == previous && done != total && last.elapsed() < Duration::from_millis(250) {
            return;
        }
        last = Instant::now();
        previous = phase;
        // The window may be gone by now; then nobody needs the answer.
        let _ = sender.send(Event::Progress(Progress {
            phase,
            done,
            total,
            checked: 0,
        }));
        ctx.request_repaint();
    }
}

impl Screen {
    /// Whether a download or an import runs, which the other steps wait for.
    pub fn busy(&self) -> bool {
        self.importing
    }

    /// The image that was just chosen, downloaded, imported or built.
    pub fn take_new_base(&mut self) -> Option<PathBuf> {
        self.new_base.take().or_else(|| self.creator.take_made())
    }

    /// Takes what the worker threads reported.
    pub fn receive(&mut self) {
        self.creator.receive();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Images(result) => {
                    self.loading = false;
                    match result {
                        Ok(images) => {
                            self.chosen = if images.len() == 1 { 0 } else { usize::MAX };
                            self.hub = Some(images);
                        }
                        Err(why) => self.failure = why,
                    }
                }
                Event::Progress(progress) => self.bar.update(progress, Instant::now()),
                Event::Finished(result) => {
                    self.importing = false;
                    self.bar.clear();
                    match result {
                        Ok(out) => self.adopt(out),
                        Err(why) => self.failure = why,
                    }
                }
            }
        }
    }

    /// Makes `path` the base image and closes the choices.
    fn adopt(&mut self, path: PathBuf) {
        self.new_base = Some(path);
        self.archive = None;
        self.sha256.clear();
        self.method = None;
    }

    /// One button for both kinds of file: a raw image is used as it is, a download is checked and
    /// unpacked first.
    fn picked(&mut self, path: PathBuf) {
        self.failure.clear();
        let compressed = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zst"));
        if compressed {
            self.archive = Some(path);
            self.sha256.clear();
        } else {
            self.adopt(path);
        }
    }

    pub fn show(&mut self, ui: &mut Ui, base: Option<&Path>, signed_in: bool, locked: bool) {
        widgets::step_head(
            ui,
            1,
            base.is_some(),
            "Base image",
            "The system every stick is copied from. Choose it once; Forge remembers it.",
        );
        match base {
            Some(base) if self.method.is_none() => self.chosen_ui(ui, base, locked),
            _ => self.choices_ui(ui, base, signed_in, locked),
        }
        if self.importing {
            self.bar.show(ui);
        }
        widgets::note(ui, Tone::Error, &self.failure);
    }

    fn chosen_ui(&mut self, ui: &mut Ui, base: &Path, locked: bool) {
        let below = [
            &base.display().to_string(),
            "Keep this file in place until all sticks are written.",
        ];
        widgets::path_row(ui, &format::file_name(base), &below, |ui| {
            let other = Btn::new("Use another image\u{2026}").small();
            if other.enabled(!locked).show(ui).clicked() {
                self.method = Some(Method::Hub);
            }
            widgets::pill(ui, Tone::Ok, "Ready");
        });
    }

    fn choices_ui(&mut self, ui: &mut Ui, base: Option<&Path>, signed_in: bool, locked: bool) {
        let hub_text = if signed_in {
            "The usual way. Forge checks the file."
        } else {
            "Sign in to the Hub first."
        };
        let choices = [
            (Method::Hub, "Download from Keeper Hub", hub_text),
            (
                Method::File,
                "Use a file from this PC",
                "An image you already downloaded or built.",
            ),
            (
                Method::Build,
                "Build one",
                "Advanced. Takes 15 to 40 minutes.",
            ),
        ];
        // As many columns of at least 210 as fit, as the grid of the style sheet.
        let across = ui.available_width() + 10.0;
        let columns: u8 = match across {
            w if w >= 660.0 => 3,
            w if w >= 440.0 => 2,
            _ => 1,
        };
        let width = (across / f32::from(columns) - 10.0).floor();
        for line in choices.chunks(usize::from(columns)) {
            let height = line
                .iter()
                .map(|(_, name, text)| widgets::choice_height(ui, name, text, width))
                .fold(0.0, f32::max);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                for &(method, name, text) in line {
                    let possible = !locked && (method != Method::Hub || signed_in);
                    ui.add_enabled_ui(possible, |ui| {
                        let on = self.method == Some(method);
                        if widgets::choice_sized(ui, on, name, text, width, height).clicked() {
                            self.method = Some(method);
                        }
                    });
                }
            });
        }
        match self.method {
            Some(Method::Hub) => widgets::inner(ui, |ui| self.hub_ui(ui, locked)),
            Some(Method::File) => widgets::inner(ui, |ui| self.file_ui(ui, locked)),
            Some(Method::Build) => widgets::inner(ui, |ui| self.creator.show(ui, locked)),
            None => {}
        }
        if let Some(base) = base
            && self.method.is_some()
        {
            let keep = format!("Keep {}", format::file_name(base));
            if Btn::new(&keep).small().enabled(!locked).show(ui).clicked() {
                self.method = None;
            }
        }
    }

    fn file_ui(&mut self, ui: &mut Ui, locked: bool) {
        widgets::hint(
            ui,
            "Choose a raw disk image (.img), or the compressed download from Keeper Hub \
             (.img.zst). A download is checked against its SHA-256 and unpacked into a raw image \
             first.",
        );
        let name = self
            .archive
            .as_deref()
            .map_or_else(|| "No file chosen".to_owned(), format::file_name);
        let full = self.archive.as_ref().map(|path| path.display().to_string());
        let below: Vec<&str> = full.iter().map(String::as_str).collect();
        let clicked = widgets::path_row(ui, &name, &below, |ui| {
            let button = Btn::new("Choose image file\u{2026}").small();
            button.enabled(!locked).show(ui).clicked()
        });
        if clicked
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Keeper OS image or download", &["img", "zst"])
                .add_filter("Raw disk image", &["img"])
                .add_filter("Keeper OS download", &["zst"])
                .pick_file()
        {
            self.picked(path);
        }
        let Some(archive) = self.archive.clone() else {
            return;
        };
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let label = widgets::text(ui, "SHA-256 of this download").id;
            ui.add_enabled_ui(!locked, |ui| {
                let input = Input::new(&mut self.sha256).mono().show(ui);
                input.labelled_by(label);
            });
            widgets::hint(
                ui,
                "Copy the 64 characters shown beside this download on Keeper Hub.",
            );
        });
        let ready = !locked && is_sha256(&self.sha256);
        let button = Btn::new("Check and unpack\u{2026}").primary().small();
        if button.enabled(ready).show(ui).clicked()
            && let Some(out) = choose_base_to_save()
        {
            self.import(archive, out, ui.ctx().clone());
        }
    }

    fn hub_ui(&mut self, ui: &mut Ui, locked: bool) {
        let ready = !locked && !self.loading;
        let Some(images) = self.hub.as_ref().filter(|images| !images.is_empty()) else {
            let none = self.hub.is_some();
            let text = if self.loading {
                "Finding images\u{2026}"
            } else {
                "Get Keeper OS from Hub"
            };
            let button = Btn::new(text).small();
            if button.enabled(ready).show(ui).clicked() {
                self.refresh(ui.ctx().clone());
            }
            if none {
                widgets::hint(ui, "The Hub has no images available.");
            }
            return;
        };
        let labels: Vec<String> = images
            .iter()
            .map(|image| format!("{} {} ({})", image.name, image.version, image.profile))
            .collect();
        let mut options = vec![(usize::MAX, "Choose a version")];
        options.extend(labels.iter().map(String::as_str).enumerate());
        let chosen = &mut self.chosen;
        let mut download = None;
        widgets::path_box(ui, |ui| {
            let id = images.get(*chosen).map(|image| image.id.clone());
            let about = images.get(*chosen).map(|image| {
                let size = format::bytes(image.size);
                match &image.filename {
                    Some(filename) => format!("{size} \u{b7} {filename}"),
                    None => size,
                }
            });
            widgets::row(
                ui,
                |ui| {
                    widgets::strong(ui, "Image from Keeper Hub");
                    ui.add_enabled_ui(ready, |ui| {
                        widgets::select(ui, "hub-image", chosen, &options);
                    });
                    if let Some(about) = &about {
                        widgets::hint(ui, about);
                    }
                },
                |ui| {
                    let button = Btn::new("Download and verify\u{2026}").small();
                    if button.enabled(ready && id.is_some()).show(ui).clicked() {
                        download.clone_from(&id);
                    }
                },
            );
        });
        if let Some(id) = download
            && let Some(out) = choose_base_to_save()
        {
            self.download(id, out, ui.ctx().clone());
        }
    }

    fn refresh(&mut self, ctx: Context) {
        self.loading = true;
        self.failure.clear();
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

    fn start(&mut self) {
        self.importing = true;
        self.failure.clear();
        self.bar.clear();
    }

    fn download(&mut self, id: String, out: PathBuf, ctx: Context) {
        self.start();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let mut report = throttled(sender.clone(), ctx.clone());
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
                        report(phase, done, total);
                    })
                    .map_err(|e| e.to_string())
                });
            let _ = sender.send(Event::Finished(result.map(|()| out)));
            ctx.request_repaint();
        });
    }

    fn import(&mut self, archive: PathBuf, out: PathBuf, ctx: Context) {
        self.start();
        let sha256 = self.sha256.trim().to_owned();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let mut report = throttled(sender.clone(), ctx.clone());
            let result = import_image(&archive, &out, &sha256, |done, total| {
                report("importing", done, total);
            })
            .map_err(|e| e.to_string());
            let _ = sender.send(Event::Finished(result.map(|()| out)));
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
impl Screen {
    pub(super) fn sample_pick(&mut self, path: PathBuf) {
        self.picked(path);
    }

    /// A download that is half done, for the pictures.
    pub(super) fn sample_download(&mut self) {
        self.method = Some(Method::Hub);
        self.start();
        let now = Instant::now();
        let mut progress = Progress {
            phase: "downloading",
            done: 0,
            total: 4_820_000_000,
            checked: 0,
        };
        let began = now.checked_sub(Duration::from_secs(20)).unwrap_or(now);
        self.bar.update(progress, began);
        progress.done = 1_900_000_000;
        self.bar.update(progress, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Waits for the worker thread, which answers by itself.
    fn finish(screen: &mut Screen) {
        for _ in 0..200 {
            screen.receive();
            if !screen.busy() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!screen.busy());
    }

    #[test]
    fn a_raw_image_becomes_the_base_and_a_download_is_checked_first() {
        let mut screen = Screen {
            method: Some(Method::File),
            ..Screen::default()
        };
        screen.picked(PathBuf::from("keeper-os.IMG.ZST"));
        assert_eq!(screen.take_new_base(), None);
        assert!(screen.archive.is_some());
        screen.picked(PathBuf::from("keeper-os.img"));
        assert_eq!(screen.take_new_base(), Some(PathBuf::from("keeper-os.img")));
        assert!(screen.archive.is_none() && screen.method.is_none());
    }

    #[test]
    fn only_64_hex_digits_can_be_checked() {
        assert!(is_sha256(&format!(" {} ", "aF".repeat(32))));
        assert!(!is_sha256(&"a".repeat(63)));
        assert!(!is_sha256(&"g".repeat(64)));
    }

    #[test]
    fn a_finished_download_becomes_the_base_and_a_failed_one_does_not() {
        let mut screen = Screen::default();
        screen.start();
        let failed = Event::Finished(Err("no".to_owned()));
        assert!(screen.sender.send(failed).is_ok());
        screen.receive();
        assert_eq!(screen.take_new_base(), None);
        screen.start();
        let out = PathBuf::from("base.img");
        assert!(screen.sender.send(Event::Finished(Ok(out.clone()))).is_ok());
        screen.receive();
        assert_eq!(screen.take_new_base(), Some(out));
    }

    #[test]
    fn importing_a_missing_download_reports_the_error_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("folder");
        let out = dir.path().join("base.img");
        let mut screen = Screen {
            sha256: "00".repeat(32),
            ..Screen::default()
        };
        let archive = dir.path().join("missing.zst");
        screen.import(archive, out.clone(), Context::default());
        finish(&mut screen);
        assert_ne!(screen.failure, "");
        assert_eq!(screen.take_new_base(), None);
        assert!(!out.exists());
    }
}
