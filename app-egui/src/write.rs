//! Which disks may be written, and the command that writes one. The window does not write sticks
//! itself yet: that needs administrator rights, which the command line asks for.

use ck_writer::{Disk, list_disks};

#[derive(Default)]
pub struct Screen {
    disks: Vec<Disk>,
    /// The image to write, as typed.
    image: String,
    /// The chosen disk number, as typed.
    disk: String,
    message: String,
}

/// How to start the command line tool with the rights a write needs.
const ADMIN_HINT: &str = if cfg!(target_os = "macos") {
    "Writing a stick needs administrator rights: run the command below in Terminal. It asks for your password (sudo)."
} else {
    "Writing a stick needs administrator rights: run the command below in an administrator terminal."
};

/// One line for a disk: its number, name, size, and whether it may be written.
fn describe(disk: &Disk) -> String {
    let verdict = match disk.check(0, false) {
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

/// The command that writes `image` to disk `number`. The text is quoted for a shell.
fn command_line(image: &str, number: &str) -> String {
    let sudo = if cfg!(target_os = "macos") {
        "sudo "
    } else {
        ""
    };
    let image = image.trim().replace('\'', r"'\''");
    format!(
        "{sudo}ck-forge stick write '{image}' --disk {}",
        number.trim()
    )
}

impl Screen {
    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        ui.heading("Write a stick");
        ui.label(ADMIN_HINT);
        if ui.button("List the disks").clicked() {
            match list_disks() {
                Ok(disks) => {
                    self.disks = disks;
                    self.message.clear();
                }
                Err(error) => self.message = error.to_string(),
            }
        }
        for disk in &self.disks {
            ui.label(describe(disk));
        }
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
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_the_image_for_a_shell() {
        let command = command_line(" /tmp/it's.img ", " 4 ");
        assert!(command.ends_with(r"ck-forge stick write '/tmp/it'\''s.img' --disk 4"));
        assert_eq!(command.starts_with("sudo "), cfg!(target_os = "macos"));
    }
}
