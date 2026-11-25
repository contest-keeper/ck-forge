//! The Keeper Forge window, in egui. It only calls `ck-admin-core`, like the command line does.

mod app;
mod folder;
mod hub;
mod images;
mod profile;
mod sticks;
mod write;

fn main() -> eframe::Result {
    eframe::run_native(
        "Keeper Forge",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(app::Forge::default()))),
    )
}
