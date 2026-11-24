use crate::hub::Screen;

/// The window. Screens are added here in the order of the plan: Hub sign-in, profile editor,
/// image picker and download, write sticks, retire and re-key.
#[derive(Default)]
pub struct Forge {
    hub: Screen,
}

impl eframe::App for Forge {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ui, |ui| self.hub.show(ui));
    }
}
