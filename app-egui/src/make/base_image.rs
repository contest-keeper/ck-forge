//! Making a base image: from the catalog of official images (15 to 40 minutes, so it runs on a
//! worker thread and reports its steps), or from a download that is checked against its SHA-256.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use ck_admin_core::{
    CatalogEntry, CreateOptions, CreateStep, catalog, create_base_image, find_catalog_entry,
    import_image,
};

/// What a worker thread reports.
enum Event {
    Progress(&'static str, u64, u64),
    Made(Result<String, String>),
    Imported(Result<(), String>),
}

pub struct Screen {
    entries: Vec<CatalogEntry>,
    /// The catalog entry chosen, by id.
    chosen: String,
    /// Where the new base image goes, as typed. It must not exist yet.
    out: String,
    /// The release's `ck-os-patch.tar.gz`, as typed; empty makes an image that is not Keeper OS yet.
    patch: String,
    archive: String,
    sha256: String,
    progress: Option<(&'static str, u64, u64)>,
    message: String,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        let (entries, message) = match catalog() {
            Ok(entries) => (entries, String::new()),
            Err(error) => (Vec::new(), error.to_string()),
        };
        Self {
            chosen: entries
                .first()
                .map(|entry| entry.id.clone())
                .unwrap_or_default(),
            entries,
            out: String::new(),
            patch: String::new(),
            archive: String::new(),
            sha256: String::new(),
            progress: None,
            message,
            events,
            sender,
        }
    }
}

/// The installation step counts seconds up to the time limit, not bytes: its "done" is the time
/// that has passed and its "total" the longest it may take.
const PROVISION: &str = "installing the desktop and tools";

/// The words for what a step of making an image is doing.
fn step_name(step: CreateStep) -> &'static str {
    match step {
        CreateStep::Download => "downloading",
        CreateStep::Convert => "converting to a raw disk",
        CreateStep::Provision => PROVISION,
        CreateStep::Patch => "applying the Keeper OS patch",
        CreateStep::Hash => "checking the finished image",
    }
}

/// "downloading: 40%" for a step with a known total, and just the step when the total is not known.
fn progress_text(phase: &str, done: u64, total: u64) -> String {
    if phase == PROVISION {
        return format!(
            "{phase}: {} min so far (at most {} min)",
            done / 60,
            total / 60
        );
    }
    match (done.min(total) * 100).checked_div(total) {
        Some(percent) => format!("{phase}: {percent}%"),
        None => phase.to_owned(),
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

/// `Some` of the path when something was typed.
fn typed(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

impl Screen {
    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        self.receive();
        let busy = self.progress.is_some();
        ui.heading("Make a base image");
        for entry in &self.entries {
            ui.radio_value(
                &mut self.chosen,
                entry.id.clone(),
                format!("{} ({} GB)", entry.label, entry.disk_gb),
            );
        }
        ui.horizontal(|ui| {
            ui.label("New image");
            ui.text_edit_singleline(&mut self.out);
        });
        ui.horizontal(|ui| {
            ui.label("Keeper OS patch (optional)");
            ui.text_edit_singleline(&mut self.patch);
        });
        if ui
            .add_enabled(!busy, eframe::egui::Button::new("Make the base image"))
            .clicked()
        {
            self.make(ui.ctx().clone());
        }
        ui.separator();
        ui.heading("Import a downloaded image");
        ui.horizontal(|ui| {
            ui.label("Download");
            ui.text_edit_singleline(&mut self.archive);
        });
        ui.horizontal(|ui| {
            ui.label("SHA-256");
            ui.text_edit_singleline(&mut self.sha256);
        });
        if ui
            .add_enabled(!busy, eframe::egui::Button::new("Check and import"))
            .clicked()
        {
            self.import(ui.ctx().clone());
        }
        if let Some((phase, done, total)) = self.progress {
            ui.label(progress_text(phase, done, total));
            ui.spinner();
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }

    fn receive(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Progress(phase, done, total) => self.progress = Some((phase, done, total)),
                Event::Made(result) => {
                    self.progress = None;
                    self.message = match result {
                        Ok(hash) => format!("Made the base image, SHA-256 {hash}"),
                        Err(why) => why,
                    };
                }
                Event::Imported(result) => {
                    self.progress = None;
                    self.message = match result {
                        Ok(()) => "Checked and saved the base image".to_owned(),
                        Err(why) => why,
                    };
                }
            }
        }
    }

    fn make(&mut self, ctx: eframe::egui::Context) {
        let Some(out) = typed(&self.out) else {
            "Type where the new base image goes".clone_into(&mut self.message);
            return;
        };
        let entry = match find_catalog_entry(&self.chosen) {
            Ok(entry) => entry,
            Err(error) => {
                self.message = error.to_string();
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
            patch: typed(&self.patch),
        };
        self.progress = Some(("starting", 0, 0));
        self.message.clear();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let mut last = Instant::now();
            let result = create_base_image(&options, |step, done, total| {
                // Four updates a second at most, but the end of a step is always sent.
                if last.elapsed() < Duration::from_millis(250) && done != total {
                    return;
                }
                last = Instant::now();
                let _ = sender.send(Event::Progress(step_name(step), done, total));
                ctx.request_repaint();
            })
            .map(|record| record.image_sha256)
            .map_err(|e| e.to_string());
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Made(result));
            ctx.request_repaint();
        });
    }

    fn import(&mut self, ctx: eframe::egui::Context) {
        let (Some(archive), Some(out)) = (typed(&self.archive), typed(&self.out)) else {
            "Type the download and where the base image goes (New image)"
                .clone_into(&mut self.message);
            return;
        };
        let sha256 = self.sha256.trim().to_owned();
        self.progress = Some(("verifying and expanding", 0, 0));
        self.message.clear();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let mut last = Instant::now();
            let result = import_image(&archive, &out, &sha256, |done, total| {
                if last.elapsed() < Duration::from_millis(250) && done != total {
                    return;
                }
                last = Instant::now();
                let _ = sender.send(Event::Progress("verifying and expanding", done, total));
                ctx.request_repaint();
            })
            .map_err(|e| e.to_string());
            let _ = sender.send(Event::Imported(result));
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_no_path() {
        assert_eq!(typed("  "), None);
        assert_eq!(typed(" a.img "), Some(PathBuf::from("a.img")));
    }

    #[test]
    fn the_installation_shows_the_time_that_passed_and_not_a_percentage() {
        assert_eq!(
            progress_text(PROVISION, 15 * 60, 90 * 60),
            "installing the desktop and tools: 15 min so far (at most 90 min)"
        );
    }

    #[test]
    fn a_step_without_a_total_shows_only_its_name() {
        assert_eq!(progress_text("converting", 0, 0), "converting");
        assert_eq!(progress_text("downloading", 5, 10), "downloading: 50%");
    }

    #[test]
    fn making_without_a_place_for_the_image_starts_nothing() {
        let mut screen = Screen::default();
        screen.make(eframe::egui::Context::default());
        assert!(screen.progress.is_none());
        assert!(screen.message.contains("where the new base image goes"));
    }

    #[test]
    fn importing_a_missing_download_reports_the_error_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("ck-egui-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("folder");
        let out = dir.join("base.img");
        let mut screen = Screen {
            archive: dir.join("missing.zst").display().to_string(),
            out: out.display().to_string(),
            sha256: "00".repeat(32),
            ..Screen::default()
        };
        screen.import(eframe::egui::Context::default());
        // The worker thread answers by itself; wait for it.
        for _ in 0..200 {
            screen.receive();
            if screen.progress.is_none() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(screen.progress.is_none());
        assert_ne!(screen.message, "");
        assert!(!out.exists());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
