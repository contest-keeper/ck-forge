//! The Settings button at the foot of the sidebar and its panel: the look and light or dark
//! (`Settings.svelte`).

use eframe::egui::{
    Align, Area, Color32, CursorIcon, Frame, Id, Key, Layout, Order, Rect, Response, Role, Sense,
    Shadow, Shape, Stroke, StrokeKind, TextFormat, Ui, WidgetInfo, pos2, text::LayoutJob, vec2,
};

use crate::state::AppState;
use crate::theme::{Mode, Theme, Tokens, Weight, font, tokens};
use crate::widgets::{self, Bevel, Btn};

#[derive(Default)]
pub struct Panel {
    pub open: bool,
    /// Which control of the open panel had the keyboard last, to keep Tab inside.
    focus: Option<usize>,
}

impl Panel {
    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        let t = tokens(ui.ctx());
        // Two em spaces make room for the gear, which is painted over them.
        let button = Btn::new("\u{2003}\u{2003}Settings")
            .small()
            .full_width()
            .show(ui);
        button.widget_info(|| WidgetInfo::labeled(Role::Button, true, "Settings"));
        gear(ui, pos2(button.rect.left() + 20.0, button.rect.center().y));
        let opened = button.clicked() && !self.open;
        if button.clicked() {
            self.open = !self.open;
            self.focus = None;
        }
        if !self.open {
            return;
        }
        // Only the keyboard moves into the panel; after a mouse click no ring appears.
        let take_focus = opened && button.has_focus();
        // Below the update dialog, which is in the foreground.
        let done = Area::new(Id::unique("settings"))
            .order(Order::Middle)
            .pivot(eframe::egui::Align2::LEFT_BOTTOM)
            .fixed_pos(button.rect.left_top() - vec2(0.0, 8.0))
            .show(ui.ctx(), |ui| self.panel(ui, &t, state, take_focus))
            .inner;
        if done || ui.input(|input| input.key_pressed(Key::Escape)) {
            self.open = false;
            button.request_focus();
        }
    }

    /// The panel itself. True when Done was pressed.
    fn panel(&mut self, ui: &mut Ui, t: &Tokens, state: &mut AppState, take_focus: bool) -> bool {
        let pad: i8 = if t.classic() { 12 } else { 16 };
        let mut frame = Frame::new()
            .fill(t.c.panel)
            .corner_radius(t.s.radius_lg)
            .inner_margin(pad);
        if !t.classic() {
            frame = frame.stroke(Stroke::new(1.0, t.c.line)).shadow(Shadow {
                offset: [0, 18],
                blur: 48,
                spread: 0,
                color: Color32::from_black_alpha(76),
            });
        }
        let shown = frame.show(ui, |ui| {
            ui.set_width(320.0 - 2.0 * f32::from(pad));
            ui.spacing_mut().item_spacing = vec2(10.0, 14.0);
            contents(ui, t, state)
        });
        if t.classic() {
            widgets::bevel(ui, shown.response.rect, Bevel::Raised);
        }
        let (done, controls) = shown.inner;
        if take_focus {
            let theme = usize::from(state.look().theme == Theme::Classic);
            controls[1 + theme].request_focus();
        }
        self.keep_focus(ui, &controls);
        done
    }

    /// Tab stays inside the panel while it is open.
    fn keep_focus(&mut self, ui: &Ui, controls: &[Response]) {
        let Some(focused) = ui.memory(eframe::egui::Memory::focused) else {
            return;
        };
        if let Some(at) = controls.iter().position(|c| c.id == focused) {
            self.focus = Some(at);
            return;
        }
        // The mouse may take the keyboard anywhere.
        if ui.input(|input| input.pointer.any_down() || input.pointer.any_released()) {
            self.focus = None;
        } else if let Some(left) = self.focus {
            let to = if left == 0 { controls.len() - 1 } else { 0 };
            controls[to].request_focus();
        }
    }
}

/// The title, the two groups and the hint. Returns whether Done was pressed and the controls in the
/// order Tab visits them.
fn contents(ui: &mut Ui, t: &Tokens, state: &mut AppState) -> (bool, [Response; 5]) {
    // As high as the button, so the title is centred beside it.
    let size = vec2(ui.available_width(), t.s.button_small);
    let done = ui
        .allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
            let done = Btn::new("Done").small().show(ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                widgets::paragraph(
                    ui,
                    "Appearance",
                    Weight::Semibold,
                    t.rem(1.0),
                    t.c.text,
                    1.35,
                );
            });
            done
        })
        .inner;
    let look = state.look();
    let [modern, classic] = group(
        ui,
        t,
        "Look",
        [
            Opt {
                on: look.theme == Theme::Modern,
                preview: Some(Theme::Modern),
                name: "New",
                sub: "Clean and modern",
            },
            Opt {
                on: look.theme == Theme::Classic,
                preview: Some(Theme::Classic),
                name: "Classic",
                sub: "Like the browser extension",
            },
        ],
    );
    let [light, dark] = group(
        ui,
        t,
        "Theme",
        [
            Opt {
                on: look.mode == Mode::Light,
                preview: None,
                name: "Light",
                sub: "Bright background",
            },
            Opt {
                on: look.mode == Mode::Dark,
                preview: None,
                name: "Dark",
                sub: "Easy on the eyes",
            },
        ],
    );
    widgets::hint(ui, "Saved on this computer.");
    if modern.clicked() {
        state.set_theme(Theme::Modern);
    }
    if classic.clicked() {
        state.set_theme(Theme::Classic);
    }
    if light.clicked() {
        state.set_mode(Mode::Light);
    }
    if dark.clicked() {
        state.set_mode(Mode::Dark);
    }
    (done.clicked(), [done, modern, classic, light, dark])
}

#[derive(Clone, Copy)]
struct Opt {
    on: bool,
    preview: Option<Theme>,
    name: &'static str,
    sub: &'static str,
}

const PREVIEW: f32 = 34.0;

/// A heading and two options side by side, both as high as the higher one.
fn group(ui: &mut Ui, t: &Tokens, heading: &str, options: [Opt; 2]) -> [Response; 2] {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        widgets::paragraph(ui, heading, Weight::Semibold, t.rem(0.85), t.c.text, 1.35);
        let width = (ui.available_width() - 10.0) / 2.0;
        let texts = options.map(|option| {
            let underline = t.classic() && option.on;
            let galley = |text: &str, weight, size, color| {
                let format = TextFormat {
                    font_id: font(weight, size),
                    color,
                    underline: if underline {
                        Stroke::new(1.0, color)
                    } else {
                        Stroke::NONE
                    },
                    valign: Align::Center,
                    ..TextFormat::default()
                };
                let mut job = LayoutJob::single_section(text.to_owned(), format);
                job.wrap.max_width = width - 20.0;
                ui.painter().layout_job(job)
            };
            (
                galley(option.name, Weight::Bold, t.rem(0.82), t.c.text),
                galley(option.sub, Weight::Regular, t.rem(0.75), t.c.muted),
            )
        });
        let height = options
            .iter()
            .zip(&texts)
            .map(|(option, (name, sub))| {
                let preview = option.preview.map_or(0.0, |_| PREVIEW + 6.0);
                20.0 + preview + name.size().y + 6.0 + sub.size().y
            })
            .fold(44.0, f32::max);
        ui.horizontal_top(|ui| {
            let mut at = 0;
            options.map(|option| {
                let (name, sub) = texts[at].clone();
                at += 1;
                let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
                response.widget_info(|| {
                    WidgetInfo::selected(Role::Button, true, option.on, option.name)
                });
                face(ui, t, rect, option.on);
                let mut at = rect.min + vec2(10.0, 10.0);
                if let Some(theme) = option.preview {
                    preview(
                        ui,
                        Rect::from_min_size(at, vec2(width - 20.0, PREVIEW)),
                        theme,
                    );
                    at.y += PREVIEW + 6.0;
                }
                let below = at + vec2(0.0, name.size().y + 6.0);
                ui.painter().galley(at, name, t.c.text);
                ui.painter().galley(below, sub, t.c.muted);
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect.expand(2.0),
                        t.s.radius,
                        Stroke::new(2.0, if t.classic() { t.c.text } else { t.c.accent }),
                        StrokeKind::Outside,
                    );
                }
                response.on_hover_cursor(CursorIcon::PointingHand)
            })
        })
        .inner
    })
    .inner
}

/// The background of an option (`.opt`).
fn face(ui: &Ui, t: &Tokens, rect: Rect, on: bool) {
    let painter = ui.painter();
    if t.classic() {
        painter.rect_filled(rect, 0.0, if on { t.c.pushed } else { t.c.panel });
        widgets::bevel(ui, rect, if on { Bevel::Pressed } else { Bevel::Raised });
        return;
    }
    let (fill, edge) = if on {
        (t.c.accent_soft, Stroke::new(2.0, t.c.accent))
    } else {
        (t.c.input_bg, Stroke::new(1.0, t.c.line))
    };
    painter.rect(rect, t.s.radius, fill, edge, StrokeKind::Inside);
}

/// A small drawing of a look. It keeps its own colours, whatever the current look is.
fn preview(ui: &Ui, rect: Rect, theme: Theme) {
    let painter = ui.painter();
    let (bar, radius) = if theme == Theme::Modern {
        painter.rect(
            rect,
            8.0,
            Color32::WHITE,
            Stroke::new(1.0, Color32::from_rgb(0xe1, 0xec, 0xec)),
            StrokeKind::Inside,
        );
        (Color32::from_rgb(0x0b, 0x7c, 0x7a), 7.0)
    } else {
        painter.rect_filled(rect, 0.0, Color32::from_gray(0xc0));
        for (rect, light, dark) in [(rect, 0xff, 0x0a), (rect.shrink(1.0), 0xdf, 0x80)] {
            let top_left = [rect.left_bottom(), rect.left_top(), rect.right_top()];
            let bottom_right = [rect.right_top(), rect.right_bottom(), rect.left_bottom()];
            for (points, grey) in [(top_left, light), (bottom_right, dark)] {
                let inside =
                    points.map(|p| p.clamp(rect.min + vec2(0.5, 0.5), rect.max - vec2(0.5, 0.5)));
                painter.add(Shape::line(
                    inside.to_vec(),
                    Stroke::new(1.0, Color32::from_gray(grey)),
                ));
            }
        }
        (Color32::BLACK, 0.0)
    };
    let width = (rect.width() - 12.0 - 4.0) / 2.0;
    for left in [rect.left() + 6.0, rect.left() + 6.0 + width + 4.0] {
        let place = Rect::from_min_size(pos2(left, rect.center().y - 7.0), vec2(width, 14.0));
        painter.rect_filled(place, radius, bar.gamma_multiply(0.25));
    }
}

/// The gear of the earlier window's button: a circle and eight spokes in a box of 16.
fn gear(ui: &Ui, center: eframe::egui::Pos2) {
    let t = tokens(ui.ctx());
    let stroke = Stroke::new(1.3, t.c.text);
    let painter = ui.painter();
    painter.circle_stroke(center, 2.0, stroke);
    for (from, to) in [
        ((0.0, -10.0), (0.0, -7.0)),
        ((0.0, 7.0), (0.0, 10.0)),
        ((-10.0, 0.0), (-7.0, 0.0)),
        ((7.0, 0.0), (10.0, 0.0)),
        ((-7.1, -7.1), (-5.0, -5.0)),
        ((5.0, 5.0), (7.1, 7.1)),
        ((-7.1, 7.1), (-5.0, 5.0)),
        ((5.0, -5.0), (7.1, -7.1)),
    ] {
        let scale = 16.0 / 24.0;
        let at = |(x, y): (f32, f32)| center + vec2(x, y) * scale;
        painter.line_segment([at(from), at(to)], stroke);
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Key, Modifiers};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    #[test]
    fn tab_stays_in_the_open_panel_and_escape_closes_it() {
        let mut state = AppState::load(None);
        let mut frame = 0;
        let mut harness = Harness::builder().with_size([600.0, 600.0]).build_ui_state(
            |ui, panel: &mut Panel| {
                crate::theme::apply(ui.ctx(), crate::theme::Look::default());
                // The fonts arrive with the second frame.
                frame += 1;
                if frame < 2 {
                    return;
                }
                ui.add_space(450.0);
                ui.set_width(220.0);
                panel.show(ui, &mut state);
                let _ = ui.button("outside");
            },
            Panel::default(),
        );
        harness.state_mut().open = true;
        harness.run_steps(3);
        harness.get_by_label("Dark").focus();
        harness.run_steps(2);
        for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
            for _ in 0..8 {
                harness.key_press_modifiers(modifiers, Key::Tab);
                harness.run_steps(3);
                assert!(!harness.get_by_label("outside").is_focused());
            }
        }
        harness.key_press(Key::Escape);
        harness.run_steps(2);
        assert!(!harness.state().open);
    }

    #[test]
    #[ignore = "writes pictures; needs CK_SHOTS_DIR and a graphics adapter"]
    fn shots_of_the_settings() {
        crate::shots::capture("sidebar-settings", 0.0, |forge| forge.settings.open = true);
    }
}
