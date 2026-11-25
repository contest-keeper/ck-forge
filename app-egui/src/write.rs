//! Write sticks: one image for each stick, written to the chosen disks by `ck-forge stick write`
//! started with administrator rights (the Windows permission prompt). The window stays unelevated,
//! shows the helper's progress and can cancel. Elsewhere it shows the command to run in a terminal.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use ck_protocol::WifiPassword;
use ck_writer::{Disk, list_disks};

use crate::stick_job::{Event, Progress, Request, run};

// The checkboxes and flags of a window are plain state, not a state machine.
#[allow(clippy::struct_excessive_bools)]
pub struct Screen {
    disks: Vec<Disk>,
    /// Whether the disks are being listed, which can take seconds.
    listing: bool,
    /// The base image the sticks are made from, as typed.
    base: String,
    /// The profile the sticks get their settings from, as typed.
    profile: String,
    /// The Wi-Fi password, typed for this write only. It is never saved, logged or shown.
    wifi_password: String,
    first_boot_enrolment: bool,
    allow_virtual: bool,
    /// A file to write to instead of a disk: a test without a stick.
    to_file: String,
    /// The disks the admin ticked.
    chosen: BTreeSet<u32>,
    /// Whether the window that asks to confirm the erase is open.
    confirming: bool,
    /// The image to write on a system where the window cannot write, as typed.
    image: String,
    /// The chosen disk number on such a system, as typed.
    disk: String,
    message: String,
    job: Option<Job>,
    events: Receiver<Result<Vec<Disk>, String>>,
    sender: Sender<Result<Vec<Disk>, String>>,
}

/// A write that is running on its own thread.
struct Job {
    cancel: Arc<AtomicBool>,
    progress: Option<Progress>,
    events: Receiver<Event>,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            disks: Vec::new(),
            listing: false,
            base: String::new(),
            profile: String::new(),
            wifi_password: String::new(),
            first_boot_enrolment: false,
            allow_virtual: false,
            to_file: String::new(),
            chosen: BTreeSet::new(),
            confirming: false,
            image: String::new(),
            disk: String::new(),
            message: String::new(),
            job: None,
            events,
            sender,
        }
    }
}

/// How to start the command line tool with the rights a write needs.
const ADMIN_HINT: &str = if cfg!(target_os = "macos") {
    "Writing a stick needs administrator rights: run the command below in Terminal. It asks for your password (sudo)."
} else {
    "Writing a stick needs administrator rights: run the command below in an administrator PowerShell."
};

/// One line for a disk: its number, name, size, and whether it may be written.
fn describe(disk: &Disk) -> String {
    let verdict = match disk.check(0, true) {
        Ok(()) => "can be written".to_owned(),
        Err(error) => format!("refused: {error}"),
    };
    format!(
        "disk {}  {}  {}  {}  [{verdict}]",
        disk.number,
        disk.friendly_name.as_deref().unwrap_or("?"),
        disk.size_text(),
        disk.bus_type,
    )
}

/// The command that writes `image` to disk `number`, quoted for the shell the hint names: zsh or
/// bash on macOS, PowerShell on Windows.
fn command_line(image: &str, number: &str) -> String {
    let sudo = if cfg!(target_os = "macos") {
        "sudo "
    } else {
        ""
    };
    let image = if cfg!(windows) {
        quote_powershell(image.trim())
    } else {
        quote_posix(image.trim())
    };
    let tool = if cfg!(target_os = "macos") {
        bundled_tool(std::env::current_exe().ok().as_deref())
    } else {
        "ck-forge".to_owned()
    };
    format!("{sudo}{tool} stick write {image} --disk {}", number.trim())
}

/// The command line tool as a terminal can start it. In the macOS app it sits beside this program
/// and is not on the PATH, so its full path is given; otherwise `ck-forge` is expected on the PATH.
fn bundled_tool(here: Option<&std::path::Path>) -> String {
    here.map(|here| here.with_file_name("ck-forge"))
        .filter(|tool| tool.is_file())
        .map_or_else(
            || "ck-forge".to_owned(),
            |tool| quote_posix(&tool.display().to_string()),
        )
}

/// `text` as one single-quoted word of a POSIX shell: only the quote itself is special inside.
fn quote_posix(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `text` as one single-quoted string of PowerShell, which ends the string at any of its four
/// kinds of single quote, so each is doubled.
fn quote_powershell(text: &str) -> String {
    let mut quoted = String::from("'");
    for c in text.chars() {
        quoted.push(c);
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}

/// How far the write is, from 0 to 1: the writing, then the checking once the writing is over.
fn fraction(progress: &Progress) -> f32 {
    if progress.total == 0 {
        return 0.0;
    }
    let done = if progress.phase == "checking" {
        progress.checked
    } else {
        progress.done
    };
    // Only for showing a bar: a precision loss in the last digits does not matter.
    #[allow(clippy::cast_precision_loss)]
    let fraction = done.min(progress.total) as f32 / progress.total as f32;
    fraction
}

/// The words for what the job is doing.
fn progress_text(progress: &Progress) -> String {
    let megabytes = |bytes: u64| bytes / (1024 * 1024);
    match progress.phase {
        "building" => format!(
            "making the stick image: {} of {} MB",
            megabytes(progress.done),
            megabytes(progress.total)
        ),
        "checking" => format!(
            "checking: {} of {} MB",
            megabytes(progress.checked),
            megabytes(progress.total)
        ),
        _ => format!(
            "writing: {} of {} MB (checked {} MB)",
            megabytes(progress.done),
            megabytes(progress.total),
            megabytes(progress.checked)
        ),
    }
}

/// One line of the confirmation for a disk that is about to be erased.
fn erase_line(disk: &Disk) -> String {
    format!(
        "disk {}: {} ({}, serial {})",
        disk.number,
        disk.friendly_name.as_deref().unwrap_or("?"),
        disk.size_text(),
        disk.serial_number.as_deref().unwrap_or("?").trim(),
    )
}

fn typed(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

impl Screen {
    fn list(&mut self, ctx: eframe::egui::Context) {
        self.listing = true;
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = list_disks().map_err(|e| e.to_string());
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn receive(&mut self) {
        while let Ok(result) = self.events.try_recv() {
            self.listing = false;
            match result {
                Ok(disks) => {
                    self.chosen
                        .retain(|number| disks.iter().any(|d| d.number == *number));
                    self.disks = disks;
                    self.message.clear();
                }
                Err(why) => self.message = why,
            }
        }
        let mut ended = None;
        if let Some(job) = &mut self.job {
            while let Ok(event) = job.events.try_recv() {
                match event {
                    Event::Progress(progress) => job.progress = Some(progress),
                    Event::Done(result) => ended = Some(result),
                }
            }
        }
        if let Some(result) = ended {
            self.job = None;
            self.message = match result {
                Ok(hash) => format!("Written and read back, SHA-256 {hash}"),
                Err(why) => why,
            };
        }
    }

    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        ui.heading("Write a stick");
        self.receive();
        if cfg!(windows) {
            self.write_ui(ui);
        } else {
            self.command_ui(ui);
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }

    fn list_ui(&mut self, ui: &mut eframe::egui::Ui, pick: bool) {
        let button = ui.add_enabled(!self.listing, eframe::egui::Button::new("List the disks"));
        if button.clicked() {
            self.list(ui.ctx().clone());
        }
        let size = typed(&self.base)
            .and_then(|base| ck_writer::thin::image_len(&base).ok())
            .unwrap_or(0);
        for disk in &self.disks {
            if pick {
                let allowed = disk.check(size, self.allow_virtual).is_ok();
                let mut ticked = self.chosen.contains(&disk.number);
                let changed = ui
                    .add_enabled(
                        allowed,
                        eframe::egui::Checkbox::new(&mut ticked, describe(disk)),
                    )
                    .changed();
                if changed && ticked {
                    self.chosen.insert(disk.number);
                } else if changed || !allowed {
                    self.chosen.remove(&disk.number);
                }
            } else {
                ui.label(describe(disk));
            }
        }
    }

    /// Where the window cannot write sticks itself, it shows the command to copy.
    fn command_ui(&mut self, ui: &mut eframe::egui::Ui) {
        ui.label(ADMIN_HINT);
        self.list_ui(ui, false);
        ui.horizontal(|ui| {
            ui.label("Image");
            ui.text_edit_singleline(&mut self.image);
        });
        ui.horizontal(|ui| {
            ui.label("Disk number");
            ui.text_edit_singleline(&mut self.disk);
        });
        if !self.image.trim().is_empty() && !self.disk.trim().is_empty() {
            let command = command_line(&self.image, &self.disk);
            ui.label("Command:");
            ui.monospace(&command);
            if ui.button("Copy the command").clicked() {
                ui.ctx().copy_text(command);
            }
        }
    }

    fn write_ui(&mut self, ui: &mut eframe::egui::Ui) {
        if let Some(job) = &self.job {
            if let Some(progress) = &job.progress {
                ui.add(
                    eframe::egui::ProgressBar::new(fraction(progress))
                        .text(progress_text(progress)),
                );
            } else {
                ui.label("Starting…");
            }
            if ui.button("Cancel").clicked() {
                job.cancel.store(true, Ordering::Relaxed);
                "Cancelling: the stick is then only partly written and must be written again"
                    .clone_into(&mut self.message);
            }
            return;
        }
        ui.label(
            "Every stick gets an image of its own with its own identity. Writing erases the \
             whole stick and asks for administrator permission.",
        );
        for (label, text) in [
            ("Base image", &mut self.base),
            ("Profile file", &mut self.profile),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.text_edit_singleline(text);
            });
        }
        ui.horizontal(|ui| {
            ui.label("Wi-Fi password");
            // Typed for this write only: it is not saved in the profile and never shown.
            ui.add(eframe::egui::TextEdit::singleline(&mut self.wifi_password).password(true));
        });
        ui.checkbox(
            &mut self.first_boot_enrolment,
            "First-boot enrolment (needs the Hub sign-in)",
        );
        ui.checkbox(
            &mut self.allow_virtual,
            "Also allow a virtual disk (to test the writer)",
        );
        self.list_ui(ui, true);
        ui.horizontal(|ui| {
            ui.label("Or write to a file");
            ui.text_edit_singleline(&mut self.to_file);
        });
        let ready = !self.chosen.is_empty() || !self.to_file.trim().is_empty();
        if ui
            .add_enabled(ready, eframe::egui::Button::new("Write"))
            .clicked()
        {
            if self.to_file.trim().is_empty() {
                self.confirming = true;
            } else {
                self.start(ui.ctx());
            }
        }
        if self.confirming {
            self.confirm_ui(ui.ctx());
        }
    }

    /// The last question before disks are erased, naming each one.
    fn confirm_ui(&mut self, ctx: &eframe::egui::Context) {
        let lines: Vec<String> = self
            .disks
            .iter()
            .filter(|disk| self.chosen.contains(&disk.number))
            .map(erase_line)
            .collect();
        let mut answer = None;
        eframe::egui::Window::new("Erase these sticks?")
            .collapsible(false)
            .resizable(false)
            .anchor(eframe::egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("This ERASES everything on:");
                for line in &lines {
                    ui.label(line);
                }
                ui.horizontal(|ui| {
                    if ui.button("Erase and write").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
        match answer {
            Some(true) => {
                self.confirming = false;
                self.start(ctx);
            }
            Some(false) => self.confirming = false,
            None => {}
        }
    }

    /// Starts the write on its own thread. The Wi-Fi password is moved into the request and the
    /// field is emptied.
    fn start(&mut self, ctx: &eframe::egui::Context) {
        let (Some(base), Some(profile)) = (typed(&self.base), typed(&self.profile)) else {
            "Type the base image and the profile file".clone_into(&mut self.message);
            return;
        };
        let to_file = typed(&self.to_file);
        let password = std::mem::take(&mut self.wifi_password);
        let request = Request {
            base,
            profile,
            disks: if to_file.is_some() {
                Vec::new()
            } else {
                self.chosen.iter().copied().collect()
            },
            to_file,
            allow_virtual: self.allow_virtual,
            wifi_password: (!password.is_empty()).then(|| WifiPassword::new(password)),
            first_boot_enrolment: self.first_boot_enrolment,
        };
        let (sender, events) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let ctx = ctx.clone();
        thread::spawn(move || run(&request, &flag, &sender, &|| ctx.request_repaint()));
        self.message.clear();
        self.job = Some(Job {
            cancel,
            progress: None,
            events,
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_the_image_for_the_shell_of_this_system() {
        let command = command_line(" /tmp/it's a.img ", " 4 ");
        let quoted = if cfg!(windows) {
            "'/tmp/it''s a.img'"
        } else {
            r"'/tmp/it'\''s a.img'"
        };
        assert!(command.ends_with(&format!("ck-forge stick write {quoted} --disk 4")));
        assert_eq!(command.starts_with("sudo "), cfg!(target_os = "macos"));
    }

    #[test]
    fn the_command_names_the_tool_inside_the_app_when_it_is_there() {
        let app = tempfile::tempdir().expect("folder");
        let window = app.path().join("keeper-forge");
        assert_eq!(bundled_tool(Some(&window)), "ck-forge");
        assert_eq!(bundled_tool(None), "ck-forge");
        std::fs::write(app.path().join("ck-forge"), b"x").expect("tool");
        let tool = bundled_tool(Some(&window));
        assert!(
            tool.starts_with('\'') && tool.ends_with("ck-forge'"),
            "{tool}"
        );
    }

    #[test]
    fn the_bar_shows_the_writing_and_then_the_checking() {
        let writing = Progress {
            phase: "writing",
            done: 50,
            total: 100,
            checked: 10,
        };
        assert!((fraction(&writing) - 0.5).abs() < f32::EPSILON);
        let checking = Progress {
            phase: "checking",
            done: 100,
            total: 100,
            checked: 25,
        };
        assert!((fraction(&checking) - 0.25).abs() < f32::EPSILON);
        assert!(
            fraction(&Progress {
                total: 0,
                ..writing
            })
            .abs()
                < f32::EPSILON
        );
        assert!(progress_text(&checking).starts_with("checking"));
    }

    #[test]
    fn a_write_needs_the_base_image_and_the_profile_and_starts_nothing_without_them() {
        let mut screen = Screen {
            to_file: "out.img".to_owned(),
            ..Screen::default()
        };
        screen.start(&eframe::egui::Context::default());
        assert!(screen.job.is_none());
        assert!(screen.message.contains("base image"));
    }

    #[test]
    fn starting_a_write_takes_the_password_out_of_the_field() {
        let mut screen = Screen {
            base: "no-such-base.img".to_owned(),
            profile: "no-such-profile.json".to_owned(),
            to_file: "out.img".to_owned(),
            wifi_password: "correct horse battery".to_owned(),
            ..Screen::default()
        };
        screen.start(&eframe::egui::Context::default());
        assert_eq!(screen.wifi_password, "");
        // The job fails on the missing profile, writing nothing, and says so.
        for _ in 0..200 {
            screen.receive();
            if screen.job.is_none() {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(screen.job.is_none());
        assert!(!screen.message.contains("correct horse"));
        assert_ne!(screen.message, "");
    }

    #[test]
    fn powershell_quotes_every_kind_of_single_quote() {
        // PowerShell ends a string at the typographic quotes too.
        assert_eq!(
            quote_powershell("x\u{2019}; calc; #\u{2018}'"),
            "'x\u{2019}\u{2019}; calc; #\u{2018}\u{2018}'''"
        );
        assert_eq!(quote_posix("a'b"), r"'a'\''b'");
    }
}
