//! The Keeper Forge window, in egui. It only calls `ck-admin-core`, like the command line does.

// Without this, Windows opens a console window next to the program.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod contest;
mod folder;
mod make;
mod side;
mod update;

const ICON: &[u8] = include_bytes!("../installer/keeper-forge.ico");

/// The icon of the window and of its taskbar button.
fn icon() -> Option<eframe::egui::IconData> {
    let picture = image::load_from_memory(ICON).ok()?.into_rgba8();
    Some(eframe::egui::IconData {
        width: picture.width(),
        height: picture.height(),
        rgba: picture.into_raw(),
    })
}

fn main() -> eframe::Result {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 760.0])
        .with_min_inner_size([760.0, 560.0]);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "Keeper Forge",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|_| Ok(Box::new(app::Forge::default()))),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_built_in_icon_is_readable() {
        assert!(super::icon().is_some());
    }
}
