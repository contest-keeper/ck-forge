use crate::{hub, images, profile, sticks};

#[derive(PartialEq, Eq, Clone, Copy)]
enum Page {
    Hub,
    Profile,
    Images,
    Sticks,
}

/// The window. Screens are added here in the order of the plan: Hub sign-in, profile editor,
/// image picker and download, write sticks, retire and re-key.
pub struct Forge {
    page: Page,
    hub: hub::Screen,
    profile: profile::Screen,
    images: images::Screen,
    sticks: sticks::Screen,
}

impl Default for Forge {
    fn default() -> Self {
        Self {
            page: Page::Hub,
            hub: hub::Screen::default(),
            profile: profile::Screen::default(),
            images: images::Screen::default(),
            sticks: sticks::Screen::default(),
        }
    }
}

impl eframe::App for Forge {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::Panel::top("pages").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.page, Page::Hub, "Hub");
                ui.selectable_value(&mut self.page, Page::Profile, "Profile");
                ui.selectable_value(&mut self.page, Page::Images, "Images");
                ui.selectable_value(&mut self.page, Page::Sticks, "Sticks");
            });
        });
        eframe::egui::CentralPanel::default().show(ui, |ui| match self.page {
            Page::Hub => self.hub.show(ui),
            Page::Profile => self.profile.show(ui),
            Page::Images => self.images.show(ui),
            Page::Sticks => self.sticks.show(ui),
        });
    }
}
