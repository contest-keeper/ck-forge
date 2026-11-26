//! The Settings button at the foot of the sidebar and its panel: the look and light or dark.

use eframe::egui::{Align2, Area, Frame, Id, Key, Order, Stroke, Ui, vec2};

use crate::state::AppState;
use crate::theme::{Mode, Theme, tokens};
use crate::widgets::{self, Btn};

#[derive(Default)]
pub struct Panel {
    pub open: bool,
}

impl Panel {
    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        let button = Btn::new("Settings").small().full_width().show(ui);
        if button.clicked() {
            self.open = !self.open;
        }
        if !self.open {
            return;
        }
        let t = tokens(ui.ctx());
        Area::new(Id::unique("settings"))
            .order(Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(button.rect.left_top() - vec2(0.0, 8.0))
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(t.c.panel)
                    .stroke(Stroke::new(1.0, t.c.line))
                    .corner_radius(t.s.radius_lg)
                    .inner_margin(16)
                    .show(ui, |ui| self.contents(ui, state));
            });
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            self.open = false;
        }
    }

    fn contents(&mut self, ui: &mut Ui, state: &mut AppState) {
        ui.set_width(288.0);
        ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
        widgets::row(
            ui,
            |ui| {
                widgets::title(ui, "Appearance");
            },
            |ui| {
                if Btn::new("Done").small().show(ui).clicked() {
                    self.open = false;
                }
            },
        );
        let look = state.look();
        widgets::subtitle(ui, "Look");
        ui.horizontal_top(|ui| {
            let on = look.theme == Theme::Modern;
            if widgets::choice(ui, on, "New", "Clean and modern", 139.0).clicked() {
                state.set_theme(Theme::Modern);
            }
            let on = look.theme == Theme::Classic;
            if widgets::choice(ui, on, "Classic", "Like the browser extension", 139.0).clicked() {
                state.set_theme(Theme::Classic);
            }
        });
        widgets::subtitle(ui, "Theme");
        ui.horizontal_top(|ui| {
            let on = look.mode == Mode::Light;
            if widgets::choice(ui, on, "Light", "Bright background", 139.0).clicked() {
                state.set_mode(Mode::Light);
            }
            let on = look.mode == Mode::Dark;
            if widgets::choice(ui, on, "Dark", "Easy on the eyes", 139.0).clicked() {
                state.set_mode(Mode::Dark);
            }
        });
        widgets::hint(ui, "Saved on this computer.");
    }
}
