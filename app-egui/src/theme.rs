//! The two looks of the window, each light and dark. Colours, radii and sizes are those of the style
//! sheets of the earlier web window (`classic.css`, `modern.css` and `base.css` at commit 22fe6e7).

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
    TextStyle,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Modern,
    Classic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub theme: Theme,
    pub mode: Mode,
}

/// The colours, named after the CSS variables.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub bg: Color32,
    pub side_bg: Color32,
    pub panel: Color32,
    /// The bar at the bottom.
    pub foot_bg: Color32,
    pub soft: Color32,
    /// Classic: a button that is pushed in.
    pub pushed: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub disabled: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_text: Color32,
    pub ok: Color32,
    pub ok_bg: Color32,
    pub warn: Color32,
    pub warn_bg: Color32,
    pub danger_text: Color32,
    pub danger_bg: Color32,
    pub danger_line: Color32,
    pub danger_solid: Color32,
    pub input_bg: Color32,
    /// Classic: the ticked disk row.
    pub selected: Color32,
    pub selected_text: Color32,
    /// Classic: the light and dark edges of a bevel, from the outer to the inner line.
    pub edge_hi: Color32,
    pub edge_lo: Color32,
    pub inner_hi: Color32,
    pub inner_lo: Color32,
}

/// Sizes in points, which are the CSS pixels of the style sheets.
#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    /// One `rem`: 16 in modern, 13 in classic.
    pub rem: f32,
    pub radius: u8,
    pub radius_lg: u8,
    /// The gap between the cards of a page.
    pub gap: f32,
    /// The gap inside a card.
    pub card_gap: f32,
    pub card_pad: egui::Margin,
    pub button: f32,
    pub button_small: f32,
    pub input: f32,
    pub check_row: f32,
    pub hint: f32,
    pub brand: f32,
    pub step_title: f32,
    pub step_hint: f32,
    pub step_height: f32,
    pub dot: f32,
    pub h1: f32,
    pub h2: f32,
    pub pill: f32,
    pub head_pad: egui::Margin,
    pub body_pad: egui::Margin,
    pub foot_pad: egui::Margin,
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub look: Look,
    pub c: Colors,
    pub s: Sizes,
}

impl Tokens {
    pub fn classic(&self) -> bool {
        self.look.theme == Theme::Classic
    }

    /// `rem(0.8)` is the CSS `0.8rem`.
    pub fn rem(&self, rem: f32) -> f32 {
        rem * self.s.rem
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Semibold,
    Bold,
    Mono,
}

/// The font of a weight. Classic has the regular weight only, whatever is asked for.
pub fn font(weight: Weight, size: f32) -> FontId {
    let family = match weight {
        Weight::Regular => FontFamily::Proportional,
        Weight::Semibold => FontFamily::Name("semibold".into()),
        Weight::Bold => FontFamily::Name("bold".into()),
        Weight::Mono => FontFamily::Monospace,
    };
    FontId::new(size, family)
}

const fn hex(rgb: u32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// A CSS `rgba()` colour.
fn rgba(rgb: u32, alpha: f32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    // From 0 to 255 by construction.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let alpha = (alpha * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// A translucent colour on an opaque one: the sidebar and the bottom bar of modern, which the web
/// window also blurred.
fn over(top: Color32, bottom: Color32) -> Color32 {
    bottom.blend(top)
}

fn modern(mode: Mode) -> Colors {
    let dark = mode == Mode::Dark;
    let pick = |light: u32, night: u32| hex(if dark { night } else { light });
    let bg = pick(0x00f5_f9f9, 0x000c_1216);
    let danger_text = pick(0x00c4_221b, 0x00ff_9d96);
    Colors {
        bg,
        side_bg: over(rgba(if dark { 0x0013_1b21 } else { 0x00ff_ffff }, 0.72), bg),
        panel: pick(0x00ff_ffff, 0x0013_1b21),
        foot_bg: bg,
        soft: pick(0x00e6_efef, 0x001b_262e),
        pushed: pick(0x00e6_efef, 0x001b_262e),
        text: pick(0x000f_1f2e, 0x00f2_f6f8),
        muted: pick(0x005b_6b78, 0x00a9_b7c1),
        disabled: pick(0x005b_6b78, 0x00a9_b7c1),
        line: pick(0x00e1_ecec, 0x0022_303a),
        line_strong: pick(0x00b9_cccc, 0x003a_4c58),
        accent: pick(0x000b_7c7a, 0x006f_d6d2),
        accent_soft: if dark {
            rgba(0x006f_d6d2, 0.16)
        } else {
            rgba(0x000b_7c7a, 0.14)
        },
        accent_text: pick(0x00ff_ffff, 0x0006_222a),
        ok: pick(0x001a_6b36, 0x007f_dca1),
        ok_bg: pick(0x00e3_f4e8, 0x0017_3826),
        warn: pick(0x008a_5200, 0x00f2_c56b),
        warn_bg: pick(0x00ff_f3d1, 0x003a_2d10),
        danger_text,
        danger_bg: pick(0x00fd_ecea, 0x003d_1a1a),
        danger_line: danger_text,
        danger_solid: hex(0x00c4_221b),
        input_bg: pick(0x00f5_f9f9, 0x000c_1216),
        selected: pick(0x00ff_ffff, 0x0013_1b21),
        selected_text: pick(0x000f_1f2e, 0x00f2_f6f8),
        edge_hi: Color32::TRANSPARENT,
        edge_lo: Color32::TRANSPARENT,
        inner_hi: Color32::TRANSPARENT,
        inner_lo: Color32::TRANSPARENT,
    }
}

fn classic(mode: Mode) -> Colors {
    let dark = mode == Mode::Dark;
    let pick = |light: u32, night: u32| hex(if dark { night } else { light });
    let face = pick(0x00c0_c0c0, 0x002b_2b2b);
    let danger = pick(0x00a0_0000, 0x00ff_8c84);
    Colors {
        bg: face,
        side_bg: face,
        panel: face,
        foot_bg: face,
        soft: pick(0x00df_dfdf, 0x001c_1c1c),
        pushed: pick(0x00d4_d0c8, 0x003c_3c3c),
        text: pick(0x0000_0000, 0x00e6_e6e6),
        muted: pick(0x0040_4040, 0x00b4_b4b4),
        disabled: pick(0x0080_8080, 0x008a_8a8a),
        line: pick(0x0080_8080, 0x005a_5a5a),
        line_strong: pick(0x0080_8080, 0x005a_5a5a),
        accent: hex(0x000a_6f6c),
        accent_soft: pick(0x00d4_d0c8, 0x003c_3c3c),
        accent_text: hex(0x00ff_ffff),
        ok: pick(0x0006_4643, 0x006f_e3a0),
        ok_bg: pick(0x00ff_ffff, 0x000d_0d0d),
        warn: pick(0x007a_4200, 0x00f2_c56b),
        warn_bg: pick(0x00ff_d84a, 0x003a_2d10),
        danger_text: danger,
        danger_bg: pick(0x00ff_ffe1, 0x003a_1414),
        danger_line: danger,
        danger_solid: hex(0x00a0_0000),
        input_bg: pick(0x00ff_ffff, 0x000d_0d0d),
        selected: pick(0x000b_2545, 0x000f_5f63),
        selected_text: hex(0x00ff_ffff),
        edge_hi: pick(0x00ff_ffff, 0x006a_6a6a),
        edge_lo: pick(0x0080_8080, 0x0000_0000),
        inner_hi: pick(0x00df_dfdf, 0x003c_3c3c),
        inner_lo: pick(0x000a_0a0a, 0x0000_0000),
    }
}

pub fn tokens_for(look: Look) -> Tokens {
    let margin = |left: i8, right: i8, top: i8, bottom: i8| egui::Margin {
        left,
        right,
        top,
        bottom,
    };
    let (c, s) = match look.theme {
        Theme::Modern => (
            modern(look.mode),
            Sizes {
                rem: 16.0,
                radius: 12,
                radius_lg: 20,
                gap: 14.0,
                card_gap: 12.0,
                card_pad: margin(22, 22, 18, 18),
                button: 44.0,
                button_small: 44.0,
                input: 44.0,
                check_row: 44.0,
                hint: 12.8,
                brand: 16.8,
                step_title: 14.1,
                step_hint: 12.5,
                step_height: 52.0,
                dot: 28.0,
                h1: 32.0,
                h2: 15.2,
                pill: 12.5,
                head_pad: margin(32, 32, 32, 14),
                body_pad: margin(32, 32, 0, 24),
                foot_pad: margin(32, 32, 12, 12),
            },
        ),
        Theme::Classic => (
            classic(look.mode),
            Sizes {
                rem: 13.0,
                radius: 0,
                radius_lg: 0,
                gap: 10.0,
                card_gap: 10.0,
                card_pad: margin(14, 14, 12, 12),
                button: 30.0,
                button_small: 30.0,
                input: 30.0,
                check_row: 32.0,
                hint: 12.0,
                brand: 15.0,
                step_title: 13.0,
                step_hint: 12.0,
                step_height: 46.0,
                dot: 24.0,
                h1: 18.0,
                h2: 13.0,
                pill: 11.0,
                head_pad: margin(18, 18, 14, 10),
                body_pad: margin(18, 18, 0, 14),
                foot_pad: margin(18, 18, 8, 8),
            },
        ),
    };
    Tokens { look, c, s }
}

fn key() -> egui::Id {
    egui::Id::unique("forge-tokens")
}

/// The look in use. Widgets read it from the context, so they need no argument for it.
pub fn tokens(ctx: &Context) -> Tokens {
    ctx.data(|data| data.get_temp(key()))
        .unwrap_or_else(|| tokens_for(Look::default()))
}

/// Applies a look to the whole window: fonts, colours and the style of the plain egui widgets, which
/// the screens that are not rebuilt yet still use. Does nothing when it is already applied.
pub fn apply(ctx: &Context, look: Look) {
    let before: Option<Tokens> = ctx.data(|data| data.get_temp(key()));
    if before.is_some_and(|before| before.look == look) {
        return;
    }
    if before.is_none_or(|before| before.look.theme != look.theme) {
        ctx.set_fonts(fonts(look.theme));
    }
    let t = tokens_for(look);
    ctx.data_mut(|data| data.insert_temp(key(), t));
    ctx.set_theme(match look.mode {
        Mode::Light => egui::Theme::Light,
        Mode::Dark => egui::Theme::Dark,
    });
    ctx.all_styles_mut(|style| set_style(style, &t));
}

fn set_style(style: &mut egui::Style, t: &Tokens) {
    let c = t.c;
    let body = t.s.rem;
    style.text_styles = [
        (TextStyle::Small, font(Weight::Regular, t.s.hint)),
        (TextStyle::Body, font(Weight::Regular, body)),
        (TextStyle::Button, font(Weight::Semibold, body)),
        (TextStyle::Heading, font(Weight::Semibold, t.s.h2)),
        (TextStyle::Monospace, font(Weight::Mono, t.rem(0.85))),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 4.0);
    style.spacing.interact_size.y = if t.classic() { 24.0 } else { 32.0 };
    style.spacing.icon_width = 18.0;
    style.spacing.scroll = egui::style::ScrollStyle::solid();

    let visuals = &mut style.visuals;
    visuals.panel_fill = c.bg;
    visuals.window_fill = c.panel;
    visuals.window_stroke = Stroke::new(1.0, c.line_strong);
    visuals.window_corner_radius = CornerRadius::same(t.s.radius_lg);
    visuals.menu_corner_radius = CornerRadius::same(t.s.radius);
    visuals.extreme_bg_color = c.input_bg;
    visuals.text_edit_bg_color = Some(c.input_bg);
    visuals.faint_bg_color = c.soft;
    visuals.code_bg_color = c.soft;
    visuals.error_fg_color = c.danger_text;
    visuals.warn_fg_color = c.warn;
    visuals.hyperlink_color = c.accent;
    visuals.weak_text_color = Some(c.muted);
    visuals.selection.bg_fill = if t.classic() {
        c.selected
    } else {
        c.accent_soft
    };
    visuals.selection.stroke = Stroke::new(
        1.0,
        if t.classic() {
            c.selected_text
        } else {
            c.accent
        },
    );
    let widgets = &mut visuals.widgets;
    for (state, fill, edge) in [
        (&mut widgets.noninteractive, c.panel, c.line),
        (&mut widgets.inactive, c.soft, c.line_strong),
        (&mut widgets.hovered, c.line, c.line_strong),
        (&mut widgets.active, c.pushed, c.accent),
        (&mut widgets.open, c.soft, c.line_strong),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, edge);
        state.fg_stroke = Stroke::new(1.0, c.text);
        state.corner_radius = CornerRadius::same(t.s.radius);
        state.expansion = 0.0;
    }
}

/// Where the fonts that the style sheets name are on this system: regular, semibold, bold and the
/// monospace one. They are read from the system when the window starts and never shipped with it.
fn font_files(theme: Theme) -> [PathBuf; 4] {
    if cfg!(windows) {
        let folder = std::env::var_os("WINDIR")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
            .join("Fonts");
        let names = match theme {
            Theme::Modern => ["segoeui.ttf", "seguisb.ttf", "segoeuib.ttf", "consola.ttf"],
            Theme::Classic => ["tahoma.ttf", "tahoma.ttf", "tahoma.ttf", "cour.ttf"],
        };
        names.map(|name| folder.join(name))
    } else {
        // macOS. The system font is one variable file, so every weight shows as regular.
        let names = match theme {
            Theme::Modern => ["SFNS.ttf", "SFNS.ttf", "SFNS.ttf", "SFNSMono.ttf"],
            Theme::Classic => [
                "Supplemental/Tahoma.ttf",
                "Supplemental/Tahoma.ttf",
                "Supplemental/Tahoma.ttf",
                "Supplemental/Courier New.ttf",
            ],
        };
        names.map(|name| PathBuf::from("/System/Library/Fonts").join(name))
    }
}

/// The system fonts of a theme, with the fonts of egui behind them: for a missing file and for the
/// characters a system font does not have.
fn fonts(theme: Theme) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let fallback = |fonts: &FontDefinitions, family: &FontFamily| {
        fonts.families.get(family).cloned().unwrap_or_default()
    };
    let proportional = fallback(&fonts, &FontFamily::Proportional);
    let monospace = fallback(&fonts, &FontFamily::Monospace);
    let families = [
        (FontFamily::Proportional, &proportional),
        (FontFamily::Name("semibold".into()), &proportional),
        (FontFamily::Name("bold".into()), &proportional),
        (FontFamily::Monospace, &monospace),
    ];
    for ((family, behind), file) in families.into_iter().zip(font_files(theme)) {
        let mut chain = behind.clone();
        if let Ok(bytes) = std::fs::read(&file) {
            let name = file.display().to_string();
            fonts
                .font_data
                .entry(name.clone())
                .or_insert_with(|| Arc::new(FontData::from_owned(bytes)));
            chain.insert(0, name);
        }
        fonts.families.insert(family, chain);
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_font_family_has_a_font_even_when_the_system_has_none() {
        for theme in [Theme::Modern, Theme::Classic] {
            let fonts = fonts(theme);
            for weight in [
                Weight::Regular,
                Weight::Semibold,
                Weight::Bold,
                Weight::Mono,
            ] {
                let family = font(weight, 12.0).family;
                assert!(!fonts.families[&family].is_empty(), "{family:?}");
            }
        }
    }
}
