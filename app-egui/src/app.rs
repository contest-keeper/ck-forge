//! The window: a sidebar of steps and the page beside it, with its title above and the way on
//! below. The rules are those of `App.svelte` of the earlier web window.

use eframe::egui::{
    self, Align, Color32, Frame, Layout, Margin, Panel, Rect, Role, ScrollArea, Sense, Stroke,
    StrokeKind, TextFormat, TextureHandle, TextureOptions, Ui, WidgetInfo, pos2, text::LayoutJob,
    vec2,
};

use crate::side::{hub_sign_in, settings, update_prompt};
use crate::state::AppState;
use crate::theme::{self, Tokens, Weight, font, tokens};
use crate::widgets::{self, Bevel, Btn, Tone};
use crate::{contest, make};

const SIDEBAR: f32 = 248.0;

struct Step {
    title: &'static str,
    hint: &'static str,
    blurb: &'static str,
}

const STEPS: [Step; 2] = [
    Step {
        title: "Contest profile",
        hint: "Name, platform, branding",
        blurb: "Everything a stick needs to know about this contest. Mistakes are shown as you type.",
    },
    Step {
        title: "Make sticks",
        hint: "Base image, erase and write USB",
        blurb: "Choose the base image once, tick the sticks and write. Each stick gets its own identity from the saved profile. Windows asks for administrator permission when the write starts.",
    },
];

/// The step that holds the profile; leaving it saves.
pub const CONTEST: usize = 0;
pub const MAKE: usize = 1;

pub struct Forge {
    pub state: AppState,
    pub step: usize,
    /// Why Continue stayed on the profile.
    pub leave_failure: String,
    pub contest: contest::Page,
    pub make: make::Page,
    pub hub: hub_sign_in::Screen,
    pub settings: settings::Panel,
    pub update: update_prompt::Banner,
    logo: Option<TextureHandle>,
}

impl Forge {
    /// The window as it starts for a person: their Forge folder, their Hub session, the update check.
    pub fn new() -> Self {
        Self::with(
            AppState::load(crate::folder::data_dir()),
            hub_sign_in::Screen::from_vault(),
            update_prompt::Banner::default(),
        )
    }

    /// A window that touches nothing of this computer: no folder, no vault, no network.
    #[cfg(test)]
    pub fn sample() -> Self {
        Self::with(
            AppState::load(None),
            hub_sign_in::Screen::new(hub_sign_in::State::SignedOut),
            update_prompt::Banner::quiet(),
        )
    }

    fn with(state: AppState, hub: hub_sign_in::Screen, update: update_prompt::Banner) -> Self {
        Self {
            state,
            step: CONTEST,
            leave_failure: String::new(),
            contest: contest::Page::default(),
            make: make::Page::default(),
            hub,
            settings: settings::Panel::default(),
            update,
            logo: None,
        }
    }

    /// Moves to the next step. Leaving the profile saves it, in the Forge folder when it has no
    /// file yet; a profile with problems stays here, and the problems are shown.
    fn next(&mut self) {
        self.leave_failure.clear();
        if self.step == CONTEST
            && let Err(why) = self.state.save_profile()
        {
            self.leave_failure = why;
            return;
        }
        self.step += 1;
    }

    /// The sidebar steps go through the same rule as Continue.
    fn go(&mut self, target: usize) {
        if target > self.step {
            self.next();
        } else {
            self.leave_failure.clear();
            self.step = target;
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if let Some(system) = ctx.system_theme() {
            self.state.follow_system(system == egui::Theme::Dark);
        }
        theme::apply(&ctx, self.state.look());
        // New fonts take effect with the next frame; text in them cannot be laid out before.
        let semibold = font(Weight::Semibold, 1.0).family;
        if !ctx.fonts(|fonts| fonts.definitions().families.contains_key(&semibold)) {
            ctx.request_repaint();
            return;
        }
        self.contest.poll(&ctx, &mut self.state);
        self.make.poll(&ctx, &mut self.state);
        self.hub.poll(&ctx, &mut self.state);

        let t = tokens(&ctx);
        ui.painter().rect_filled(ui.max_rect(), 0.0, t.c.bg);
        let window = ui.available_rect_before_wrap();
        self.sidebar(ui, &t);
        self.footer(ui, &t);
        header(ui, &t, &STEPS[self.step]);
        self.body(ui, &t);
        if t.classic() {
            // `.win`: light along the top and left, dark along the bottom and right.
            let painter = ui.painter();
            for (min, max, color) in [
                (
                    window.min,
                    pos2(window.max.x, window.min.y + 1.0),
                    t.c.edge_hi,
                ),
                (
                    window.min,
                    pos2(window.min.x + 1.0, window.max.y),
                    t.c.edge_hi,
                ),
                (
                    pos2(window.min.x, window.max.y - 1.0),
                    window.max,
                    t.c.edge_lo,
                ),
                (
                    pos2(window.max.x - 1.0, window.min.y),
                    window.max,
                    t.c.edge_lo,
                ),
            ] {
                painter.rect_filled(Rect::from_min_max(min, max), 0.0, color);
            }
        }
        // Last, so the dialog is over everything.
        self.update.show(&ctx, &mut self.state);
    }

    fn sidebar(&mut self, ui: &mut Ui, t: &Tokens) {
        let shown = Panel::left("side")
            .exact_size(SIDEBAR)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(t.c.side_bg)
                    .inner_margin(Margin::symmetric(14, 20)),
            )
            .show(ui, |ui| {
                let foot = Panel::bottom("side-foot")
                    .resizable(false)
                    .show_separator_line(false)
                    .frame(side_foot_frame(t))
                    .show(ui, |ui| self.side_foot(ui, t));
                ui.spacing_mut().item_spacing.y = 22.0;
                self.brand(ui, t);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let done = [self.state.profile_path.is_some(), self.state.written];
                    for (at, step) in STEPS.iter().enumerate() {
                        if step_button(ui, at, step, at == self.step, done[at]).clicked() {
                            self.go(at);
                        }
                    }
                });
                foot.response.rect
            });
        if t.classic() {
            widgets::bevel(ui, shown.inner, Bevel::Sunken);
        }
        let rect = shown.response.rect;
        let (color, x) = if t.classic() {
            (t.c.edge_hi, rect.right())
        } else {
            (t.c.line, rect.right() - 1.0)
        };
        ui.painter().rect_filled(
            Rect::from_min_size(pos2(x, rect.top()), vec2(1.0, rect.height())),
            0.0,
            color,
        );
    }

    fn brand(&mut self, ui: &mut Ui, t: &Tokens) {
        if self.logo.is_none() {
            // The pixel icon of the browser extension: keep its edges sharp.
            self.logo = crate::icon().map(|icon| {
                let size = [icon.width as usize, icon.height as usize];
                let picture = egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
                ui.ctx()
                    .load_texture("logo", picture, TextureOptions::NEAREST)
            });
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.add_space(8.0);
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(vec2(32.0, 32.0)));
            }
            ui.label(
                egui::RichText::new("Keeper Forge")
                    .font(font(Weight::Bold, t.s.brand))
                    .color(t.c.text),
            );
        });
    }

    /// Sign-in, the profile's name and the settings (`.side-foot`).
    fn side_foot(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 10.0;
        self.hub.show(ui, &self.state);
        let name = self
            .state
            .profile
            .as_ref()
            .map(|profile| profile.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("No profile yet");
        widgets::paragraph(ui, name, Weight::Bold, t.rem(0.82), t.c.text, 1.35);
        self.settings.show(ui, &mut self.state);
    }

    fn footer(&mut self, ui: &mut Ui, t: &Tokens) {
        let shown = Panel::bottom("foot")
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(t.c.foot_bg).inner_margin(t.s.foot_pad))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.horizontal(|ui| {
                    // Room for a message of several lines before the button is centred beside it.
                    let mut height = t.s.button;
                    for _ in self.leave_failure.lines().skip(1).take(4) {
                        height += t.s.rem * 1.35;
                    }
                    ui.set_min_height(height);
                    if self.step > CONTEST && Btn::new("Back").show(ui).clicked() {
                        self.go(self.step - 1);
                    }
                    // The button takes its place first; the message gets what is left.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.step < MAKE {
                            let ready = self.state.profile.is_some();
                            if Btn::new("Save and continue")
                                .primary()
                                .enabled(ready)
                                .show(ui)
                                .clicked()
                            {
                                self.next();
                            }
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            self.footer_message(ui, t);
                        });
                    });
                });
            });
        let top = shown.response.rect.top();
        let color = if t.classic() { t.c.edge_hi } else { t.c.line };
        ui.painter().rect_filled(
            Rect::from_min_size(
                pos2(shown.response.rect.left(), top),
                vec2(shown.response.rect.width(), 1.0),
            ),
            0.0,
            color,
        );
    }

    /// Why the window stayed on the profile, or else what the profile's file holds.
    fn footer_message(&self, ui: &mut Ui, t: &Tokens) {
        if !self.leave_failure.is_empty() {
            // As wide as its text, as in the earlier window.
            let galley = ui.painter().layout(
                self.leave_failure.clone(),
                font(Weight::Regular, t.s.rem),
                t.c.danger_text,
                ui.available_width() - 28.0,
            );
            ui.set_max_width(galley.size().x.ceil() + 28.0);
            ScrollArea::vertical()
                .id_salt("leave")
                .max_height(120.0)
                .show(ui, |ui| {
                    widgets::note(ui, Tone::Error, &self.leave_failure);
                });
        } else if self.step == CONTEST {
            let text = match &self.state.profile_path {
                None => "Not saved yet. Continue saves it in the Keeper Forge folder.".to_owned(),
                Some(_) if self.state.is_unsaved() => {
                    "Unsaved changes. Continue saves them.".to_owned()
                }
                Some(path) => format!("Saved in {}", path.display()),
            };
            widgets::paragraph(ui, &text, Weight::Regular, t.s.hint, t.c.muted, 1.35);
        }
    }

    fn body(&mut self, ui: &mut Ui, t: &Tokens) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(t.c.bg))
            .show(ui, |ui| {
                // One scroll position for each page.
                ScrollArea::vertical()
                    .id_salt(("page", self.step))
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        Frame::new().inner_margin(t.s.body_pad).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = t.s.gap;
                            match self.step {
                                CONTEST => self.contest.show(ui, &mut self.state),
                                _ => self.make.show(ui, &mut self.state),
                            }
                        });
                    });
            });
    }
}

impl eframe::App for Forge {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.update.poll(ui.ctx(), self.state.data());
        self.draw(ui);
    }
}

fn side_foot_frame(t: &Tokens) -> Frame {
    let fill = if t.classic() { t.c.input_bg } else { t.c.soft };
    Frame::new()
        .fill(fill)
        .corner_radius(t.s.radius)
        .inner_margin(12)
}

fn header(ui: &mut Ui, t: &Tokens, step: &Step) {
    Panel::top("head")
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(t.c.bg).inner_margin(t.s.head_pad))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            let (weight, spacing, height) = if t.classic() {
                (Weight::Regular, 0.0, 1.35)
            } else {
                (Weight::Semibold, -0.03, 1.1)
            };
            let format = TextFormat {
                font_id: font(weight, t.s.h1),
                color: t.c.text,
                extra_letter_spacing: spacing * t.s.h1,
                line_height: Some(height * t.s.h1),
                ..TextFormat::default()
            };
            ui.label(LayoutJob::single_section(step.title.to_owned(), format));
            ui.scope(|ui| {
                ui.set_max_width(600.0);
                widgets::paragraph(ui, step.blurb, Weight::Regular, t.s.rem, t.c.muted, 1.5);
            });
        });
}

/// One step in the sidebar: its number, or a tick when done, its title and a hint.
fn step_button(ui: &mut Ui, at: usize, step: &Step, on: bool, done: bool) -> egui::Response {
    let t = tokens(ui.ctx());
    let wrap = ui.available_width() - 24.0 - t.s.dot - 12.0;
    let (title_color, hint_color) = match (t.classic(), on) {
        (true, _) => (t.c.text, t.c.muted),
        (false, true) => (t.c.accent, t.c.muted),
        (false, false) => (t.c.muted, t.c.muted),
    };
    let galley = |text: &str, weight, size, color: Color32| {
        let format = TextFormat {
            font_id: font(weight, size),
            color,
            // Classic marks the step that is open with an underline.
            underline: if t.classic() && on {
                Stroke::new(1.0, color)
            } else {
                Stroke::NONE
            },
            ..TextFormat::default()
        };
        let mut job = LayoutJob::single_section(text.to_owned(), format);
        job.wrap.max_width = wrap;
        ui.painter().layout_job(job)
    };
    let title = galley(step.title, Weight::Semibold, t.s.step_title, title_color);
    let hint = galley(step.hint, Weight::Regular, t.s.step_hint, hint_color);
    let text_height = title.size().y + hint.size().y;
    let height = (text_height + 16.0).max(t.s.step_height);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    response.widget_info(|| {
        let name = if done {
            format!("{} (done)", step.title)
        } else {
            step.title.to_owned()
        };
        WidgetInfo::selected(Role::Button, true, on, name)
    });
    let painter = ui.painter();
    if t.classic() {
        painter.rect_filled(rect, 0.0, if on { t.c.pushed } else { t.c.panel });
        widgets::bevel(ui, rect, if on { Bevel::Pressed } else { Bevel::Raised });
    } else if on {
        painter.rect_filled(rect, t.s.radius, t.c.accent_soft);
    } else if response.hovered() {
        painter.rect_filled(rect, t.s.radius, t.c.soft);
    }
    let dot = Rect::from_center_size(
        pos2(rect.left() + 12.0 + t.s.dot / 2.0, rect.center().y),
        vec2(t.s.dot, t.s.dot),
    );
    paint_dot(ui, &t, dot, at, on, done);
    let at = pos2(dot.right() + 12.0, rect.center().y - text_height / 2.0);
    let below = at + vec2(0.0, title.size().y);
    ui.painter().galley(at, title, title_color);
    ui.painter().galley(below, hint, hint_color);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            t.s.radius,
            Stroke::new(2.0, t.c.accent),
            StrokeKind::Inside,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_dot(ui: &Ui, t: &Tokens, dot: Rect, at: usize, on: bool, done: bool) {
    // The step that is open wins over done in modern; classic shows done in both.
    let (fill, ink) = match (on, done) {
        (_, true) if t.classic() => (t.c.input_bg, t.c.ok),
        // Open and done: the tick stays in the done colour, as the style sheets cascade.
        (true, true) => (t.c.accent, t.c.ok),
        (true, false) => (t.c.accent, t.c.accent_text),
        (false, true) => (t.c.ok_bg, t.c.ok),
        (false, false) if t.classic() => (t.c.input_bg, t.c.text),
        (false, false) => (t.c.soft, t.c.muted),
    };
    if t.classic() {
        ui.painter().rect_filled(dot, 0.0, fill);
        widgets::bevel(ui, dot, Bevel::Thin);
    } else {
        ui.painter()
            .circle_filled(dot.center(), dot.width() / 2.0, fill);
    }
    if done {
        let c = dot.center();
        ui.painter().add(egui::Shape::line(
            vec![
                c + vec2(-4.0, 0.0),
                c + vec2(-1.2, 3.0),
                c + vec2(4.2, -3.0),
            ],
            Stroke::new(1.8, ink),
        ));
    } else {
        ui.painter().text(
            dot.center(),
            egui::Align2::CENTER_CENTER,
            at + 1,
            font(Weight::Semibold, t.rem(0.8)),
            ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_the_profile_saves_it_and_a_profile_with_problems_stays() {
        let data = tempfile::tempdir().expect("folder");
        let mut forge = Forge::sample();
        forge.state = AppState::load(Some(data.path().to_owned()));
        forge
            .state
            .profile
            .as_mut()
            .expect("profile")
            .settings
            .branding
            .bookmarks
            .push(ck_protocol::Bookmark {
                title: String::new(),
                url: String::new(),
            });
        forge.go(MAKE);
        assert_eq!(forge.step, CONTEST);
        assert!(forge.leave_failure.contains("problems"));
        forge
            .state
            .profile
            .as_mut()
            .expect("profile")
            .settings
            .branding
            .bookmarks
            .pop();
        forge.go(MAKE);
        assert_eq!(forge.step, MAKE);
        assert!(forge.leave_failure.is_empty() && !forge.state.is_unsaved());
        assert!(forge.state.profile_path.is_some());
        forge.go(CONTEST);
        assert_eq!(forge.step, CONTEST);
    }
}
