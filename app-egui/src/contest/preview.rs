//! The wallpaper: its picture, the choice of it and the preview of what a contestant sees.

use std::path::PathBuf;

use base64::Engine;
use ck_admin_core::{Profile, wallpaper_data_url};
use eframe::egui::{
    self, Align2, Color32, Frame, Margin, Rect, Role, Sense, Shape, Stroke, TextureHandle, Ui,
    WidgetInfo, pos2, vec2,
};

use crate::state::AppState;
use crate::theme::{Weight, font, tokens};
use crate::widgets::{self, Btn, Tone};

/// The longest side of the picture, so a large wallpaper never exceeds a texture size.
const SIDE: u32 = 1024;

/// The wallpaper as a texture, made once when it changes and not on every frame.
#[derive(Default)]
pub struct Picture {
    /// The profile file, picture name and picture revision the texture was made from.
    made_from: Option<(PathBuf, String, u64)>,
    texture: Option<TextureHandle>,
}

impl Picture {
    /// The saved wallpaper, when there is one; an unsaved profile has no folder to read it from.
    pub fn follow(&mut self, ctx: &egui::Context, state: &AppState) {
        let wanted = state.profile_path.as_ref().and_then(|file| {
            let branding = &state.profile.as_ref()?.settings.branding;
            // Choosing a picture of the same type replaces the file but not its name.
            Some((
                file.clone(),
                branding.wallpaper.clone()?,
                state.wallpaper_revision,
            ))
        });
        if self.made_from == wanted {
            return;
        }
        self.texture = wanted
            .as_ref()
            .and_then(|(file, name, _)| wallpaper_data_url(file, name))
            .as_deref()
            .and_then(picture_of)
            .map(|picture| ctx.load_texture("wallpaper", picture, egui::TextureOptions::LINEAR));
        self.made_from = wanted;
    }

    /// The picture filling `rect`, cut at the sides that are too long (CSS `cover`).
    fn cover(&self, rect: Rect, radius: f32) -> Option<Shape> {
        let texture = self.texture.as_ref()?;
        let uv = cover_uv(texture.size_vec2(), rect.size());
        let shape = egui::epaint::RectShape::filled(rect, radius, Color32::WHITE)
            .with_texture(texture.id(), uv);
        Some(shape.into())
    }
}

/// The part of a picture of size `picture` that covers a place of size `place`.
fn cover_uv(picture: egui::Vec2, place: egui::Vec2) -> Rect {
    let scale = (place.x / picture.x).max(place.y / picture.y);
    let part = vec2(place.x / (picture.x * scale), place.y / (picture.y * scale));
    Rect::from_center_size(pos2(0.5, 0.5), part)
}

/// The picture inside a `data:<type>;base64,<data>` address, made small enough to show.
fn picture_of(data_url: &str) -> Option<egui::ColorImage> {
    let (_, encoded) = data_url.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let mut picture = image::load_from_memory(&bytes).ok()?;
    // `thumbnail` also enlarges, which a small picture does not need.
    if picture.width() > SIDE || picture.height() > SIDE {
        picture = picture.thumbnail(SIDE, SIDE);
    }
    let picture = picture.to_rgba8();
    let size = [picture.width() as usize, picture.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        picture.as_raw(),
    ))
}

/// What the buttons of the picker ask for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    Choose,
    Remove,
}

/// The wallpaper row of the branding card (`WallpaperPicker.svelte`).
pub fn picker(ui: &mut Ui, picture: &Picture, name: Option<&str>, failure: &str) -> Option<Asked> {
    let t = tokens(ui.ctx());
    let mut asked = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let size = t.rem(0.9);
        widgets::paragraph(ui, "Wallpaper", Weight::Semibold, size, t.c.text, 1.35);
        let frame = Frame::new()
            .stroke(Stroke::new(1.0, t.c.line))
            .corner_radius(t.s.radius)
            .inner_margin(Margin::symmetric(14, 10));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                thumbnail(ui, picture, name);
                widgets::row(
                    ui,
                    |ui| {
                        widgets::strong(ui, name.unwrap_or("The Keeper OS wallpaper"));
                        let hint =
                            "PNG, JPG, WEBP or BMP up to 16 MB. A copy is kept with the profile.";
                        widgets::hint(ui, hint);
                    },
                    |ui| asked = buttons(ui, name.is_some()),
                );
            });
        });
        widgets::note(ui, Tone::Error, failure);
    });
    asked
}

/// Laid out from the right.
fn buttons(ui: &mut Ui, chosen: bool) -> Option<Asked> {
    let mut asked = None;
    if chosen && Btn::new("Remove").small().show(ui).clicked() {
        asked = Some(Asked::Remove);
    }
    let choose = if chosen {
        "Change picture\u{2026}"
    } else {
        "Choose a picture\u{2026}"
    };
    if Btn::new(choose).small().show(ui).clicked() {
        asked = Some(Asked::Choose);
    }
    asked
}

fn thumbnail(ui: &mut Ui, picture: &Picture, name: Option<&str>) {
    let t = tokens(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(96.0, 54.0), Sense::hover());
    let radius = if t.classic() { 0.0 } else { 6.0 };
    if let Some(shape) = picture.cover(rect, radius) {
        response.widget_info(|| WidgetInfo::labeled(Role::Image, true, "The chosen wallpaper"));
        ui.painter().add(shape);
        return;
    }
    ui.painter().rect_filled(rect, radius, t.c.soft);
    let text = if name.is_some() {
        "No preview"
    } else {
        "Default"
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        font(Weight::Regular, t.s.hint),
        t.c.muted,
    );
}

/// The card "What a contestant will see" (`ContestPreview.svelte`).
pub fn contestant(ui: &mut Ui, picture: &Picture, profile: &Profile) {
    widgets::card(ui, |ui| {
        widgets::title(ui, "What a contestant will see");
        screen(ui, picture, profile);
        widgets::hint(ui, "A preview. The real screen is the Keeper OS desktop.");
    });
}

/// White on a dark panel whatever the picture is, so the text stays readable.
fn dark_box(ui: &mut Ui, margin: Margin, add: impl FnOnce(&mut Ui)) {
    let t = tokens(ui.ctx());
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(8, 20, 24, 209))
        .corner_radius(t.s.radius)
        .inner_margin(margin)
        .show(ui, add);
}

fn screen(ui: &mut Ui, picture: &Picture, profile: &Profile) {
    let t = tokens(ui.ctx());
    let settings = &profile.settings;
    let title = [settings.contest.name.trim(), profile.name.trim()]
        .into_iter()
        .find(|text| !text.is_empty())
        .unwrap_or("Your contest");
    let welcome = match settings.branding.welcome_text.as_str() {
        "" => "No welcome text yet.",
        text => text,
    };
    let start = settings.branding.home_page.as_deref().map(|page| {
        page.strip_prefix("https://")
            .or_else(|| page.strip_prefix("http://"))
            .unwrap_or(page)
    });
    // The background is as tall as the contents, so its places are kept and filled afterwards.
    let plain = ui.painter().add(Shape::Noop);
    let wallpaper = ui.painter().add(Shape::Noop);
    let shown = Frame::new().inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(118.0);
        ui.spacing_mut().item_spacing.y = 12.0;
        dark_box(ui, Margin::symmetric(14, 12), |ui| {
            ui.set_width(ui.available_width().min(380.0 - 28.0));
            ui.spacing_mut().item_spacing.y = 4.0;
            let white = Color32::WHITE;
            widgets::paragraph(ui, title, Weight::Bold, t.s.rem, white, 1.35);
            widgets::paragraph(ui, welcome, Weight::Regular, t.s.rem, white, 1.35);
        });
        if let Some(start) = start.filter(|start| !start.is_empty()) {
            dark_box(ui, Margin::symmetric(10, 4), |ui| {
                let text = format!("Start page: {start}");
                let size = t.rem(0.8);
                widgets::paragraph(ui, &text, Weight::Regular, size, Color32::WHITE, 1.35);
            });
        }
    });
    let rect = shown.response.rect;
    let radius = f32::from(t.s.radius);
    let teal = Color32::from_rgb(0x0f, 0x3d, 0x3c);
    ui.painter()
        .set(plain, Shape::rect_filled(rect, radius, teal));
    if let Some(shape) = picture.cover(rect, radius) {
        ui.painter().set(wallpaper, shape);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wallpaper_address_becomes_a_picture_and_garbage_does_not() {
        // A 2 by 1 PNG made in memory, then handed over as the data address the engine builds.
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(2, 1)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("png");
        let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
        let picture = picture_of(&format!("data:image/png;base64,{encoded}")).expect("picture");
        assert_eq!(picture.size, [2, 1]);
        assert!(picture_of("data:image/png;base64,@@@").is_none());
        assert!(picture_of("not an address").is_none());
    }

    #[test]
    fn a_wallpaper_covers_its_place_and_is_cut_at_the_long_side() {
        let uv = cover_uv(vec2(200.0, 100.0), vec2(100.0, 100.0));
        assert_eq!(uv, Rect::from_min_max(pos2(0.25, 0.0), pos2(0.75, 1.0)));
    }
}
