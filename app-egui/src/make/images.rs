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
    /// The list, and the account generation it was asked in.
    Images(u64, Result<Vec<HubImage>, String>),
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
    /// Goes up when the account changes, so a list asked for another account is dropped.
    account: u64,
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
            account: 0,
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

    /// The signed-in account changed: the images listed for one account are not for another.
    pub fn forget_account(&mut self) {
        self.account += 1;
        self.loading = false;
        self.hub = None;
        self.chosen = usize::MAX;
        self.failure.clear();
    }

    /// Takes what the worker threads reported.
    pub fn receive(&mut self) {
        self.creator.receive();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Images(account, _) if account != self.account => {}
                Event::Images(_, result) => {
                    self.loading = false;
                    match result {
                        Ok(mut images) => {
                            images.retain(|image| image.latest);
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
        // A build that is still running, or not yet taken, must not replace this later choice.
        self.creator.superseded();
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
        widgets::step_head(ui, 1, base.is_some(), "Base image", "");
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
        let full = base.display().to_string();
        let below = [full.as_str()];
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
            "The usual way."
        } else {
            "Sign in to the Hub first."
        };
        let choices = [
            (Method::Hub, "Download from Keeper Hub", hub_text),
            (
                Method::File,
                "Use a file from this PC",
                "An image you already have.",
            ),
            (Method::Build, "Build one", "Advanced. 15 to 40 minutes."),
        ];
        // Three equal columns, or one on a narrow window: never two and one left over.
        let across = ui.available_width() + 10.0;
        let columns: u8 = if across >= 540.0 { 3 } else { 1 };
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
            Some(Method::Hub) => widgets::inner(ui, |ui| self.hub_ui(ui, signed_in, locked)),
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

    fn hub_ui(&mut self, ui: &mut Ui, signed_in: bool, locked: bool) {
        if signed_in && self.hub.is_none() && !self.loading && self.failure.is_empty() {
            self.refresh(ui.ctx().clone());
        }
        let ready = !locked && !self.loading;
        let Some(images) = self.hub.as_ref().filter(|images| !images.is_empty()) else {
            let none = self.hub.is_some();
            if self.loading {
                widgets::hint(ui, "Finding images\u{2026}");
            } else {
                if Btn::new("Refresh")
                    .small()
                    .enabled(ready)
                    .show(ui)
                    .clicked()
                {
                    self.refresh(ui.ctx().clone());
                }
                if none {
                    widgets::hint(ui, "The Hub has no images available.");
                }
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
        let mut do_refresh = false;
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
                    if Btn::new("Refresh")
                        .small()
                        .enabled(ready)
                        .show(ui)
                        .clicked()
                    {
                        do_refresh = true;
                    }
                },
            );
        });
        if do_refresh {
            self.refresh(ui.ctx().clone());
        }
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
        let account = self.account;
        thread::spawn(move || {
            let result = vault::load()
                .ok_or_else(|| "sign in to the Hub first".to_owned())
                .and_then(|session| list_hub_images(HUB_URL, &session).map_err(|e| e.to_string()));
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Images(account, result));
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
    fn another_account_forgets_the_images_and_a_late_list_is_dropped() {
        let mut screen = Screen {
            hub: Some(Vec::new()),
            chosen: 0,
            ..Screen::default()
        };
        let late = Event::Images(screen.account, Ok(Vec::new()));
        screen.forget_account();
        assert!(screen.hub.is_none() && screen.chosen == usize::MAX);
        assert!(screen.sender.send(late).is_ok());
        screen.receive();
        assert!(screen.hub.is_none());
    }

    #[test]
    fn only_the_latest_hub_images_are_offered() {
        let entry = |id: &str, latest: Option<bool>| {
            let mut entry = serde_json::json!({
                "id": id, "name": "Keeper OS", "version": "1.0", "profile": "generic",
                "size": 1, "filename": null, "url": "", "sha256": ""
            });
            if let Some(latest) = latest {
                entry["latest"] = latest.into();
            }
            serde_json::from_value::<HubImage>(entry).expect("image")
        };
        let list = vec![
            entry("old", Some(false)),
            entry("new", Some(true)),
            entry("unmarked", None),
        ];
        let mut screen = Screen::default();
        let event = Event::Images(screen.account, Ok(list));
        assert!(screen.sender.send(event).is_ok());
        screen.receive();
        let offered = screen.hub.expect("list");
        let ids: Vec<&str> = offered.iter().map(|image| image.id.as_str()).collect();
        assert_eq!(ids, ["new", "unmarked"]);
    }

    #[test]
    fn an_image_picked_during_a_build_stays_the_base() {
        let mut screen = Screen::default();
        screen.creator.sample_running();
        screen.picked(PathBuf::from("picked.img"));
        assert_eq!(screen.take_new_base(), Some(PathBuf::from("picked.img")));
        screen.creator.sample_finish(PathBuf::from("built.img"));
        screen.receive();
        assert_eq!(screen.take_new_base(), None);
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
