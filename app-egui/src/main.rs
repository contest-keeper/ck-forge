//! The Keeper Forge window, in egui. It only calls `ck-admin-core`, like the command line does.

// Without this, Windows opens a console window next to the program.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod contest;
mod folder;
mod log;
mod make;
#[cfg(test)]
mod shots;
mod side;
mod state;
mod theme;
mod update;
mod widgets;

const ICON: &[u8] = include_bytes!("../installer/keeper-forge.ico");

/// The icon of the Dock on macOS: a rounded square, like the system icons. The program sets it
/// when it starts, so it has to be the same picture as the app icon of the disk image.
const MACOS_ICON: &[u8] = include_bytes!("../installer/keeper-forge-macos.png");

/// The icon of the window and of its taskbar button.
fn icon() -> Option<eframe::egui::IconData> {
    decode(ICON)
}

fn decode(bytes: &[u8]) -> Option<eframe::egui::IconData> {
    let picture = image::load_from_memory(bytes).ok()?.into_rgba8();
    Some(eframe::egui::IconData {
        width: picture.width(),
        height: picture.height(),
        rgba: picture.into_raw(),
    })
}

fn main() -> eframe::Result {
    log::step(&format!(
        "Keeper Forge {} starts on {}; log file {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        log::path().display()
    ));
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 760.0])
        .with_min_inner_size([760.0, 560.0]);
    let shown = if cfg!(target_os = "macos") {
        decode(MACOS_ICON)
    } else {
        icon()
    };
    if let Some(icon) = shown {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "Keeper Forge",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|_| Ok(Box::new(app::Forge::new()))),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_built_in_icon_is_readable() {
        assert!(super::icon().is_some());
        assert!(super::decode(super::MACOS_ICON).is_some());
    }
}
