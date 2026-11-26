//! Pictures of the window, drawn without a screen, to compare with the earlier web window:
//! `CK_SHOTS_DIR=<folder> cargo test shots -- --ignored`
//!
//! Each page's own file has an ignored test, with `shots` in its name, that prepares sample states
//! and calls [`capture`]. Nothing here reads the Forge folder or the credential vault.

use std::path::PathBuf;

use egui_kittest::Harness;

use crate::app::{Forge, MAKE};
use crate::theme::{Mode, Theme};

/// Writes `<name>-<theme>-<mode>.png` at 1000 by 760 for both looks, light and dark. `setup` puts
/// a sample window into the state to show; `scroll` is how far down the page is.
pub fn capture(name: &str, scroll: f32, setup: impl Fn(&mut Forge)) {
    capture_then(name, setup, |harness| wheel(harness, scroll));
}

/// Like [`capture`], with `act` to click and scroll before the picture is taken.
pub fn capture_then(name: &str, setup: impl Fn(&mut Forge), act: impl Fn(&mut Harness<'_>)) {
    let Some(folder) = std::env::var_os("CK_SHOTS_DIR").map(PathBuf::from) else {
        #[allow(clippy::print_stdout)]
        {
            println!("skipped {name}: set CK_SHOTS_DIR to the folder for the pictures");
        }
        return;
    };
    std::fs::create_dir_all(&folder).expect("folder for the pictures");
    for (theme, theme_name) in [(Theme::Modern, "modern"), (Theme::Classic, "classic")] {
        for (mode, mode_name) in [(Mode::Light, "light"), (Mode::Dark, "dark")] {
            let mut forge = Forge::sample();
            forge.state.set_theme(theme);
            forge.state.set_mode(mode);
            setup(&mut forge);
            let mut harness = Harness::builder()
                .with_size([1000.0, 760.0])
                .with_pixels_per_point(1.0)
                // Pictures only: the screens that are not rebuilt yet have fields without a name.
                .with_accessibility_check(false)
                .wgpu()
                .build_ui(|ui| {
                    // The harness leaves a margin around its ui; the window has none.
                    let window = ui.ctx().content_rect();
                    let builder = eframe::egui::UiBuilder::new().max_rect(window);
                    ui.scope_builder(builder, |ui| {
                        ui.set_clip_rect(window);
                        forge.draw(ui);
                    });
                });
            // A few frames: the fonts load and the panels find their sizes.
            harness.run_steps(4);
            act(&mut harness);
            let picture = harness.render().expect("render");
            picture
                .save(folder.join(format!("{name}-{theme_name}-{mode_name}.png")))
                .expect("save");
        }
    }
}

/// Turns the wheel over the page, `scroll` points down.
pub fn wheel(harness: &mut Harness<'_>, scroll: f32) {
    if scroll <= 0.0 {
        return;
    }
    harness.hover_at(eframe::egui::pos2(600.0, 400.0));
    harness.event(eframe::egui::Event::MouseWheel {
        unit: eframe::egui::MouseWheelUnit::Point,
        delta: eframe::egui::vec2(0.0, -scroll),
        phase: eframe::egui::TouchPhase::Move,
        source: eframe::egui::MouseWheelSource::Wheel,
        modifiers: eframe::egui::Modifiers::NONE,
    });
    // Scrolling is smoothed over several frames.
    harness.run_steps(60);
}

/// The shell itself. The pages add their own states in their own files.
#[test]
#[ignore = "writes pictures; needs CK_SHOTS_DIR and a graphics adapter"]
fn shots_of_the_shell() {
    capture("make-default", 0.0, |forge| {
        forge.step = MAKE;
        forge.state.profile_path = Some(PathBuf::from("contest-profile.json"));
        forge
            .state
            .set_base_path(Some(PathBuf::from(r"D:\images\keeper-os-0.4.0.img")));
    });
    capture("sidebar-settings", 0.0, |forge| forge.settings.open = true);
}
