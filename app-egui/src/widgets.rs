//! The parts every page is made of, one for each class of `base.css` at commit 22fe6e7. They read
//! the look from the context, so a page never asks which theme is on.
//!
//! The pages that are being rebuilt use these; until then some have no caller.
#![allow(dead_code)]

use std::sync::Arc;

use eframe::egui::{
    self, Align, Align2, Color32, Frame, Id, Layout, Rect, Response, RichText, Role, Sense, Shadow,
    Stroke, StrokeKind, TextEdit, TextFormat, Ui, UiBuilder, Vec2, WidgetInfo, pos2,
    text::LayoutJob, vec2,
};

use crate::theme::{Tokens, Weight, font, tokens};

// ---------------------------------------------------------------------------------------------
// Text

/// Wrapped text with the CSS `line-height` as a factor of the size.
pub fn paragraph(
    ui: &mut Ui,
    text: &str,
    weight: Weight,
    size: f32,
    color: Color32,
    line_height: f32,
) -> Response {
    let format = TextFormat {
        font_id: font(weight, size),
        color,
        line_height: Some(size * line_height),
        valign: Align::Center,
        ..TextFormat::default()
    };
    let mut job = LayoutJob::single_section(text.to_owned(), format);
    job.wrap.max_width = ui.available_width();
    ui.add(egui::Label::new(job).wrap())
}

fn rich(text: &str, weight: Weight, size: f32, color: Color32) -> RichText {
    RichText::new(text).font(font(weight, size)).color(color)
}

/// Body text.
pub fn text(ui: &mut Ui, text: &str) -> Response {
    let t = tokens(ui.ctx());
    paragraph(ui, text, Weight::Regular, t.s.rem, t.c.text, 1.35)
}

/// Bold-ish body text (`.n`, `.dn`, `<b>`).
pub fn strong(ui: &mut Ui, text: &str) -> Response {
    let t = tokens(ui.ctx());
    paragraph(ui, text, Weight::Semibold, t.s.rem, t.c.text, 1.35)
}

/// Small muted text (`.hint`).
pub fn hint(ui: &mut Ui, text: &str) -> Response {
    let t = tokens(ui.ctx());
    paragraph(ui, text, Weight::Regular, t.s.hint, t.c.muted, 1.35)
}

/// A card's title (`.card h2`).
pub fn title(ui: &mut Ui, text: &str) -> Response {
    let t = tokens(ui.ctx());
    paragraph(ui, text, Weight::Semibold, t.s.h2, t.c.text, 1.35)
}

/// A title inside a card (`.card h3`).
pub fn subtitle(ui: &mut Ui, text: &str) -> Response {
    let t = tokens(ui.ctx());
    paragraph(ui, text, Weight::Semibold, t.rem(0.9), t.c.text, 1.35)
}

/// A hash, a path or a name of a file (`.mono`).
pub fn mono(ui: &mut Ui, text: &str) -> Response {
    let t = tokens(ui.ctx());
    paragraph(ui, text, Weight::Mono, t.rem(0.85), t.c.text, 1.35)
}

// ---------------------------------------------------------------------------------------------
// Bevels of the classic look

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bevel {
    Raised,
    Pressed,
    Sunken,
    Thin,
}

/// One 1px line along the top and left (`first`) and one along the bottom and right (`second`).
fn edge(ui: &Ui, rect: Rect, first: Color32, second: Color32) {
    let painter = ui.painter();
    let line = |min: egui::Pos2, max: egui::Pos2, color| {
        painter.rect_filled(Rect::from_min_max(min, max), 0.0, color);
    };
    line(rect.min, pos2(rect.max.x, rect.min.y + 1.0), first);
    line(rect.min, pos2(rect.min.x + 1.0, rect.max.y), first);
    line(pos2(rect.min.x, rect.max.y - 1.0), rect.max, second);
    line(pos2(rect.max.x - 1.0, rect.min.y), rect.max, second);
}

/// The `--bevel-*` shadows of the classic style sheet, painted inside `rect`.
pub fn bevel(ui: &Ui, rect: Rect, kind: Bevel) {
    let c = tokens(ui.ctx()).c;
    let inner = rect.shrink(1.0);
    // `line` is the middle grey, which is `--line` in both modes.
    match kind {
        Bevel::Raised => {
            let dark_inner = if c.edge_lo == Color32::BLACK {
                c.input_bg
            } else {
                c.line
            };
            edge(ui, rect, c.edge_hi, c.inner_lo);
            edge(ui, inner, c.inner_hi, dark_inner);
        }
        Bevel::Pressed => {
            edge(ui, rect, c.inner_lo, c.edge_hi);
            edge(ui, inner, c.line, c.inner_hi);
        }
        Bevel::Sunken => {
            edge(ui, rect, c.line, c.edge_hi);
            edge(ui, inner, c.inner_lo, c.inner_hi);
        }
        Bevel::Thin => edge(
            ui,
            rect,
            c.line,
            if c.edge_lo == Color32::BLACK {
                c.edge_lo
            } else {
                c.edge_hi
            },
        ),
    }
}

/// The ring around the control that has the keyboard (`:focus-visible`).
fn focus_ring(ui: &Ui, response: &Response, radius: u8) {
    if !response.has_focus() {
        return;
    }
    let t = tokens(ui.ctx());
    if t.classic() {
        let rect = response.rect.shrink(4.0);
        let corners = [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top(),
        ];
        ui.painter()
            .extend(egui::Shape::dotted_line(&corners, t.c.text, 3.0, 0.7));
    } else {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            radius.saturating_add(2),
            Stroke::new(2.0, t.c.accent),
            StrokeKind::Outside,
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Buttons

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Primary,
    Danger,
}

/// A button (`.btn`): `Btn::new("Save").primary().small().enabled(ready).show(ui)`.
#[must_use = "call show"]
pub struct Btn<'a> {
    text: &'a str,
    kind: Kind,
    small: bool,
    enabled: bool,
    full_width: bool,
}

impl<'a> Btn<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            kind: Kind::Normal,
            small: false,
            enabled: true,
            full_width: false,
        }
    }

    pub fn primary(mut self) -> Self {
        self.kind = Kind::Primary;
        self
    }

    pub fn danger(mut self) -> Self {
        self.kind = Kind::Danger;
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// As wide as its place, with the text at the left.
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let t = tokens(ui.ctx());
        let enabled = self.enabled && ui.is_enabled();
        let size = if self.small { t.rem(0.85) } else { t.s.rem };
        let galley = ui.painter().layout_no_wrap(
            self.text.to_owned(),
            font(Weight::Semibold, size),
            Color32::PLACEHOLDER,
        );
        let pad = if self.small { 12.0 } else { 18.0 };
        let mut width = galley.size().x + 2.0 * pad;
        if self.full_width {
            width = ui.available_width();
        }
        let height = if self.small {
            t.s.button_small
        } else {
            t.s.button
        };
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);
        response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, self.text));
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let radius = if t.classic() { 0 } else { u8::MAX };
        let ink = paint_button(ui, &t, &response, self.kind, enabled);
        let x = if self.full_width {
            rect.left() + pad
        } else {
            rect.center().x - galley.size().x / 2.0
        };
        let at = pos2(x, rect.center().y - galley.size().y / 2.0);
        if t.classic() && !enabled {
            ui.painter().galley_with_override_text_color(
                at + vec2(1.0, 1.0),
                galley.clone(),
                t.c.edge_hi,
            );
        }
        ui.painter()
            .galley_with_override_text_color(at, galley, ink);
        focus_ring(ui, &response, radius);
        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        }
    }
}

/// Paints the face of a button and returns the colour of its text.
fn paint_button(ui: &Ui, t: &Tokens, response: &Response, kind: Kind, enabled: bool) -> Color32 {
    let c = t.c;
    let rect = response.rect;
    let painter = ui.painter();
    if t.classic() {
        let danger = kind == Kind::Danger;
        painter.rect_filled(rect, 0.0, if danger { c.danger_solid } else { c.panel });
        if kind == Kind::Primary {
            // No weight to stand out with: the extension's heavy outline, instead of the bevel.
            painter.rect_stroke(rect, 0.0, Stroke::new(2.0, c.text), StrokeKind::Inside);
        } else if enabled && response.is_pointer_button_down_on() {
            bevel(ui, rect, Bevel::Pressed);
        } else {
            bevel(ui, rect, Bevel::Raised);
        }
        return match (enabled, danger) {
            (false, _) => c.disabled,
            (true, true) => Color32::WHITE,
            (true, false) => c.text,
        };
    }
    let hovered = enabled && response.hovered();
    let (mut fill, mut ink, edge) = match kind {
        Kind::Normal => (if hovered { c.line } else { c.soft }, c.text, c.line),
        Kind::Primary => (c.accent, c.accent_text, Color32::TRANSPARENT),
        Kind::Danger => (c.danger_solid, Color32::WHITE, Color32::TRANSPARENT),
    };
    if hovered && kind != Kind::Normal {
        fill = fill.lerp_to_gamma(Color32::WHITE, 0.08);
    }
    if !enabled {
        fill = fill.gamma_multiply(0.5);
        ink = ink.gamma_multiply(0.5);
    }
    let radius = rect.height() / 2.0;
    painter.rect(
        rect,
        radius,
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    ink
}

// ---------------------------------------------------------------------------------------------
// Cards and boxes

/// The shadow of a modern tile.
fn tile_shadow(t: &Tokens) -> Shadow {
    let dark = t.look.mode == crate::theme::Mode::Dark;
    Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 76 } else { 13 }),
    }
}

/// A box as wide as its place. Returns what the contents returned and where the box is.
fn boxed<R>(ui: &mut Ui, frame: Frame, add: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
    let shown = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    (shown.inner, shown.response.rect)
}

/// A white tile in modern, an etched group box in classic (`.card`).
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = tokens(ui.ctx());
    let mut frame = Frame::new()
        .inner_margin(t.s.card_pad)
        .corner_radius(t.s.radius_lg);
    if !t.classic() {
        frame = frame
            .fill(t.c.panel)
            .stroke(Stroke::new(1.0, t.c.line))
            .shadow(tile_shadow(&t));
    }
    let (inner, rect) = boxed(ui, frame, |ui| {
        ui.spacing_mut().item_spacing.y = t.s.card_gap;
        add(ui)
    });
    if t.classic() {
        // A grey line with a white one beside it, outside and inside.
        let painter = ui.painter();
        for (rect, color) in [
            (rect.translate(vec2(1.0, 1.0)), t.c.edge_hi),
            (rect, t.c.line),
        ] {
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
        }
    }
    inner
}

/// A card that opens and closes under its title (`details.card`). `None` while it is closed.
pub fn details<R>(ui: &mut Ui, summary: &str, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    card(ui, |ui| disclosure(ui, summary, add))
}

/// A title that opens and closes what is under it, inside the place it is in (`<details>`).
pub fn disclosure<R>(ui: &mut Ui, summary: &str, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let t = tokens(ui.ctx());
    let id = ui.make_persistent_id(("details", summary));
    let mut open = ui.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    let galley = ui.painter().layout_no_wrap(
        summary.to_owned(),
        font(Weight::Semibold, t.s.rem),
        t.c.text,
    );
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::DisclosureTriangle, true, summary));
    // The browser's disclosure triangle.
    let at = pos2(rect.left() + 5.0, rect.center().y);
    let points = if open {
        vec![
            at + vec2(-4.5, -3.0),
            at + vec2(4.5, -3.0),
            at + vec2(0.0, 4.0),
        ]
    } else {
        vec![
            at + vec2(-3.0, -4.5),
            at + vec2(4.0, 0.0),
            at + vec2(-3.0, 4.5),
        ]
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(points, t.c.text, Stroke::NONE));
    let at = pos2(rect.left() + 17.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, t.c.text);
    focus_ring(ui, &response, 4);
    if response.clicked() {
        open = !open;
        ui.data_mut(|data| data.insert_temp(id, open));
    }
    open.then(|| {
        ui.add_space(14.0 - t.s.card_gap);
        add(ui)
    })
}

/// The part of a card below a hairline (`.inner`).
pub fn inner<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    hairline(ui);
    add(ui)
}

/// A line across the place.
pub fn hairline(ui: &mut Ui) {
    let t = tokens(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.c.line);
}

// ---------------------------------------------------------------------------------------------
// Fields

/// A label, the control, then the error or else the hint (`Field.svelte`). The control gets the id
/// of the label, to pass to `Response::labelled_by`.
pub fn field<R>(
    ui: &mut Ui,
    label: &str,
    hint_text: Option<&str>,
    error: Option<&str>,
    add: impl FnOnce(&mut Ui, Id) -> R,
) -> R {
    let t = tokens(ui.ctx());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let label = ui.label(rich(label, Weight::Semibold, t.rem(0.9), t.c.text));
        let result = add(ui, label.id);
        if let Some(error) = error {
            paragraph(
                ui,
                error,
                Weight::Semibold,
                t.rem(0.8),
                t.c.danger_text,
                1.35,
            );
        } else if let Some(text) = hint_text {
            hint(ui, text);
        }
        result
    })
    .inner
}

/// A text input: `Input::new(&mut text).password().show(ui)`.
#[must_use = "call show"]
pub struct Input<'a> {
    text: &'a mut String,
    placeholder: &'a str,
    password: bool,
    mono: bool,
    rows: Option<usize>,
    invalid: bool,
    width: Option<f32>,
}

impl<'a> Input<'a> {
    pub fn new(text: &'a mut String) -> Self {
        Self {
            text,
            placeholder: "",
            password: false,
            mono: false,
            rows: None,
            invalid: false,
            width: None,
        }
    }

    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// A text area of this many rows.
    pub fn multiline(mut self, rows: usize) -> Self {
        self.rows = Some(rows);
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let t = tokens(ui.ctx());
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let weight = if self.mono {
            Weight::Mono
        } else {
            Weight::Regular
        };
        // A single line is centred in the CSS `min-height`; a text area has padding above and below.
        let vertical = match self.rows {
            Some(_) if t.classic() => 6,
            Some(_) => 10,
            None => 0,
        };
        let mut edit = match self.rows {
            Some(rows) => TextEdit::multiline(self.text).desired_rows(rows),
            None => TextEdit::singleline(self.text).password(self.password),
        };
        let frame = Frame::new()
            .fill(t.c.input_bg)
            .corner_radius(t.s.radius)
            .inner_margin(egui::Margin::symmetric(12, vertical));
        edit = edit
            .frame(frame)
            .font(font(weight, t.s.rem))
            .hint_text(RichText::new(self.placeholder.to_owned()).color(t.c.muted))
            .desired_width(width)
            .min_size(vec2(width, t.s.input));
        if self.rows.is_none() {
            edit = edit.align(Align2::LEFT_CENTER);
        }
        let response = ui.add(edit);
        input_edge(ui, &response, self.invalid);
        response
    }
}

/// The border of an input: bevel or hairline, the accent while typing, red when invalid.
fn input_edge(ui: &Ui, response: &Response, invalid: bool) {
    let t = tokens(ui.ctx());
    let rect = response.rect;
    let radius = t.s.radius;
    let painter = ui.painter();
    if t.classic() {
        bevel(ui, rect, Bevel::Sunken);
    } else if response.has_focus() {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, t.c.accent),
            StrokeKind::Inside,
        );
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(4.0, t.c.accent_soft),
            StrokeKind::Outside,
        );
    } else {
        painter.rect_stroke(rect, radius, Stroke::new(1.0, t.c.line), StrokeKind::Inside);
    }
    if invalid {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, t.c.danger_text),
            StrokeKind::Inside,
        );
    }
}

/// A drop-down list of `options`, as wide as its place.
pub fn select<T: Copy + PartialEq>(
    ui: &mut Ui,
    id_salt: &str,
    chosen: &mut T,
    options: &[(T, &str)],
) -> Response {
    let t = tokens(ui.ctx());
    let current = options
        .iter()
        .find(|(value, _)| value == chosen)
        .map_or("", |(_, label)| *label);
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = t.s.input;
        ui.spacing_mut().button_padding.x = 12.0;
        let widgets = &mut ui.visuals_mut().widgets;
        for state in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            state.weak_bg_fill = t.c.input_bg;
        }
        let shown = egui::ComboBox::from_id_salt(id_salt)
            .width(ui.available_width())
            .selected_text(rich(current, Weight::Regular, t.s.rem, t.c.text))
            .show_ui(ui, |ui| {
                for (value, label) in options {
                    ui.selectable_value(
                        chosen,
                        *value,
                        rich(label, Weight::Regular, t.s.rem, t.c.text),
                    );
                }
            });
        if t.classic() {
            bevel(ui, shown.response.rect, Bevel::Sunken);
        }
        shown.response
    })
    .inner
}

/// The square or round mark of a check box.
fn tick_box(ui: &Ui, rect: Rect, ticked: bool, enabled: bool) {
    let t = tokens(ui.ctx());
    let painter = ui.painter();
    let radius = if t.classic() { 0.0 } else { 4.0 };
    let (fill, mark) = if t.classic() {
        (t.c.input_bg, t.c.text)
    } else if ticked {
        (t.c.accent, t.c.accent_text)
    } else {
        (t.c.panel, t.c.text)
    };
    let (fill, mark) = if enabled {
        (fill, mark)
    } else {
        (fill.gamma_multiply(0.5), mark.gamma_multiply(0.5))
    };
    painter.rect_filled(rect, radius, fill);
    if t.classic() {
        bevel(ui, rect, Bevel::Sunken);
    } else if !ticked {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.5, t.c.line_strong),
            StrokeKind::Inside,
        );
    }
    if ticked {
        let at = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
        painter.add(egui::Shape::line(
            vec![at(0.22, 0.52), at(0.42, 0.72), at(0.78, 0.3)],
            Stroke::new(2.0, mark),
        ));
    }
}

/// A check box with its label (`.check`). The whole row can be clicked.
pub fn check(ui: &mut Ui, ticked: &mut bool, label: &str) -> Response {
    check_weight(ui, ticked, label, Weight::Regular)
}

pub fn check_weight(ui: &mut Ui, ticked: &mut bool, label: &str, weight: Weight) -> Response {
    check_parts(ui, ticked, &[(label, weight)])
}

/// A check box whose label is a bold part and a regular one after it.
pub fn check_suffixed(ui: &mut Ui, ticked: &mut bool, label: &str, suffix: &str) -> Response {
    let parts = [(label, Weight::Semibold), (suffix, Weight::Regular)];
    check_parts(ui, ticked, &parts)
}

fn check_parts(ui: &mut Ui, ticked: &mut bool, parts: &[(&str, Weight)]) -> Response {
    let t = tokens(ui.ctx());
    let enabled = ui.is_enabled();
    let box_side = 14.0;
    let mut job = LayoutJob::default();
    job.wrap.max_width = (ui.available_width() - box_side - 8.0).max(40.0);
    for (text, weight) in parts {
        let format = TextFormat::simple(font(*weight, t.s.rem), t.c.text);
        job.append(text, 0.0, format);
    }
    let label = job.text.clone();
    let galley = ui.painter().layout_job(job);
    let size = vec2(
        box_side + 8.0 + galley.size().x,
        galley.size().y.max(t.s.check_row),
    );
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *ticked = !*ticked;
        response.mark_changed();
    }
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, *ticked, &label));
    let mark = Rect::from_center_size(
        pos2(rect.left() + box_side / 2.0, rect.center().y),
        Vec2::splat(box_side),
    );
    tick_box(ui, mark, *ticked, enabled);
    let at = pos2(mark.right() + 8.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, t.c.text);
    focus_ring(ui, &response, 4);
    response
}

// ---------------------------------------------------------------------------------------------
// Messages

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Error,
    /// Not usable (`.pill.no`).
    Off,
}

/// A short state beside a row (`.pill`).
pub fn pill(ui: &mut Ui, tone: Tone, text: &str) -> Response {
    let t = tokens(ui.ctx());
    let (fill, ink) = match tone {
        Tone::Ok if t.classic() => (t.c.accent, Color32::WHITE),
        Tone::Ok => (t.c.ok_bg, t.c.ok),
        Tone::Warn => (t.c.warn_bg, t.c.warn),
        Tone::Error => (t.c.danger_bg, t.c.danger_text),
        Tone::Off if t.classic() => (Color32::TRANSPARENT, t.c.text),
        Tone::Off => (t.c.soft, t.c.muted),
    };
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), font(Weight::Semibold, t.s.pill), ink);
    let pad = if t.classic() {
        vec2(7.0, 2.0)
    } else {
        vec2(10.0, 3.0)
    };
    let (rect, response) = ui.allocate_exact_size(galley.size() + 2.0 * pad, Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
    let radius = if t.classic() {
        0.0
    } else {
        rect.height() / 2.0
    };
    ui.painter().rect_filled(rect, radius, fill);
    if t.classic() && tone == Tone::Off {
        bevel(ui, rect, Bevel::Thin);
    }
    ui.painter().galley(rect.min + pad, galley, ink);
    response
}

fn tinted<R>(
    ui: &mut Ui,
    fill: Color32,
    edge: Color32,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, Rect) {
    let t = tokens(ui.ctx());
    let frame = Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, edge))
        .corner_radius(t.s.radius)
        .inner_margin(egui::Margin::symmetric(13, 9));
    boxed(ui, frame, add)
}

/// A result or a failure (`.note`). Nothing is shown for empty text.
pub fn note(ui: &mut Ui, tone: Tone, text: &str) {
    if text.is_empty() {
        return;
    }
    let t = tokens(ui.ctx());
    let (fill, ink, edge) = match tone {
        Tone::Ok => (t.c.ok_bg, t.c.ok, Color32::TRANSPARENT),
        Tone::Warn => (t.c.warn_bg, t.c.warn, Color32::TRANSPARENT),
        Tone::Error => (t.c.danger_bg, t.c.danger_text, t.c.danger_line),
        Tone::Off => (t.c.soft, t.c.muted, Color32::TRANSPARENT),
    };
    let ((), rect) = tinted(ui, fill, edge, |ui| {
        paragraph(ui, text, Weight::Regular, t.s.rem, ink, 1.35);
    });
    if t.classic() && tone == Tone::Ok {
        bevel(ui, rect, Bevel::Sunken);
    }
}

/// The problems and the warnings of a profile as two lists (`Messages.svelte`).
pub fn messages(ui: &mut Ui, problems: &[String], warnings: &[String]) {
    if !problems.is_empty() {
        let lines: Vec<String> = problems
            .iter()
            .map(|p| format!("  \u{2022}  {p}"))
            .collect();
        note(ui, Tone::Error, &lines.join("\n"));
    }
    if !warnings.is_empty() {
        let lines: Vec<String> = warnings
            .iter()
            .map(|w| format!("  \u{2022}  {w}"))
            .collect();
        note(ui, Tone::Warn, &lines.join("\n"));
    }
}

/// A fact the admin should notice, in the accent colour (`.lock`).
pub fn lock(ui: &mut Ui, text: &str) {
    let t = tokens(ui.ctx());
    let (fill, edge) = if t.classic() {
        (t.c.danger_bg, t.c.line)
    } else {
        (t.c.accent_soft, Color32::TRANSPARENT)
    };
    tinted(ui, fill, edge, |ui| {
        paragraph(ui, text, Weight::Regular, t.rem(0.85), t.c.text, 1.45);
    });
}

/// The red box around something that cannot be undone (`.danger`). The contents are laid out by
/// the caller.
pub fn danger_zone<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = tokens(ui.ctx());
    let frame = Frame::new()
        .fill(t.c.danger_bg)
        .stroke(Stroke::new(
            if t.classic() { 1.0 } else { 1.5 },
            t.c.danger_line,
        ))
        .corner_radius(t.s.radius_lg)
        .inner_margin(if t.classic() {
            egui::Margin::symmetric(14, 10)
        } else {
            egui::Margin::symmetric(22, 14)
        });
    boxed(ui, frame, add).0
}

/// A bar from 0 to 1 (`<progress>`).
pub fn progress_bar(ui: &mut Ui, fraction: f32) {
    let t = tokens(ui.ctx());
    let fraction = fraction.clamp(0.0, 1.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
    response.widget_info(|| {
        let mut info = WidgetInfo::new(Role::ProgressIndicator);
        info.value = Some(f64::from(fraction) * 100.0);
        info
    });
    let radius = if t.classic() { 0.0 } else { 4.0 };
    let painter = ui.painter();
    painter.rect(
        rect,
        radius,
        t.c.input_bg,
        Stroke::new(1.0, t.c.line_strong),
        StrokeKind::Inside,
    );
    let mut done = rect;
    done.set_width(rect.width() * fraction);
    painter.rect_filled(done, radius, t.c.accent);
}

/// Text at the left and muted text at the right, as above a progress bar.
pub fn split_line(ui: &mut Ui, left: &str, right: &str) {
    let t = tokens(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(rich(left, Weight::Regular, t.s.rem, t.c.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(rich(right, Weight::Regular, t.s.rem, t.c.muted));
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Rows, steps and choices

/// A row with the main part at the left and controls at the right, centred on each other. The
/// controls take their width first and are added from the right: the first one is the rightmost.
pub fn row<R>(ui: &mut Ui, main: impl FnOnce(&mut Ui), trailing: impl FnOnce(&mut Ui) -> R) -> R {
    // The height is that of the last frame: a layout cannot centre in a height it does not know.
    let id = ui.next_auto_id().with("row");
    let before: f32 = ui.data(|data| data.get_temp(id)).unwrap_or(0.0);
    let size = vec2(ui.available_width(), before);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        let trailing = ui.scope(trailing);
        let main = ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.scope(main).response.rect.height()
        });
        let height = main.inner.max(trailing.response.rect.height());
        if (height - before).abs() > 0.5 {
            ui.data_mut(|data| data.insert_temp(id, height));
            ui.ctx().request_repaint();
        }
        trailing.inner
    })
    .inner
}

/// The filled box around a chosen file or a set of controls (`.path`).
pub fn path_box<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = tokens(ui.ctx());
    let fill = if t.classic() { t.c.input_bg } else { t.c.soft };
    let frame = Frame::new()
        .fill(fill)
        .corner_radius(t.s.radius)
        .inner_margin(egui::Margin::symmetric(14, 10));
    let (result, rect) = boxed(ui, frame, add);
    if t.classic() {
        bevel(ui, rect, Bevel::Sunken);
    }
    result
}

/// A chosen file: its name, where it is and the buttons for it (`.path`). `trailing` is laid out
/// from the right.
pub fn path_row<R>(
    ui: &mut Ui,
    name: &str,
    below: &[&str],
    trailing: impl FnOnce(&mut Ui) -> R,
) -> R {
    path_box(ui, |ui| {
        row(
            ui,
            |ui| {
                // A name alone sits in the middle of the box, beside the taller buttons.
                if below.is_empty() {
                    ui.add_space(6.0);
                    strong(ui, name);
                    ui.add_space(6.0);
                } else {
                    strong(ui, name);
                    for line in below {
                        hint(ui, line);
                    }
                }
            },
            trailing,
        )
    })
}

/// A disk to tick (`.disk`): `selected` is changed by a click anywhere on the row unless the disk
/// is refused. `trailing` is the pill at the right.
pub fn disk_row(
    ui: &mut Ui,
    selected: &mut bool,
    refused: bool,
    name: &str,
    below: &[&str],
    trailing: impl FnOnce(&mut Ui),
) -> Response {
    let t = tokens(ui.ctx());
    let enabled = !refused && ui.is_enabled();
    let on = *selected && !refused;
    let (fill, ink, muted) = match (on, refused) {
        (true, _) if t.classic() => (t.c.selected, t.c.selected_text, t.c.selected_text),

        (false, true) if t.classic() => (t.c.panel, t.c.muted, t.c.muted),
        (false, true) => (Color32::TRANSPARENT, t.c.muted, t.c.muted),
        (false, false) if t.classic() => (t.c.input_bg, t.c.text, t.c.muted),
        (true, _) | (false, false) => (t.c.panel, t.c.text, t.c.muted),
    };
    let mut frame = Frame::new()
        .fill(fill)
        .corner_radius(t.s.radius_lg)
        .inner_margin(egui::Margin::symmetric(16, 10));
    if !t.classic() {
        let edge = if on {
            Stroke::new(2.0, t.c.accent)
        } else {
            Stroke::new(1.0, if refused { t.c.line_strong } else { t.c.line })
        };
        frame = frame.stroke(edge);
    }
    let id = ui.make_persistent_id(("disk", name, below.first()));
    let shown = ui.scope_builder(UiBuilder::new().scope_id(id).sense(Sense::click()), |ui| {
        let (mark_left, rect) = boxed(ui, frame, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                // The mark is painted later, when the height of the row is known.
                let (place, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                row(
                    ui,
                    |ui| {
                        ui.label(rich(name, Weight::Semibold, t.s.rem, ink));
                        for line in below {
                            paragraph(ui, line, Weight::Regular, t.s.hint, muted, 1.35);
                        }
                    },
                    trailing,
                );
                place.left()
            })
            .inner
        });
        let center = pos2(mark_left + 9.0, rect.center().y);
        let mark = Rect::from_center_size(center, Vec2::splat(18.0));
        tick_box(ui, mark, on, enabled);
        rect
    });
    let rect = shown.inner;
    let mut response = shown.response;
    if t.classic() && !on && !refused {
        bevel(ui, rect, Bevel::Thin);
    }
    if enabled && response.clicked() {
        *selected = !*selected;
        response.mark_changed();
    }
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, *selected, name));
    focus_ring(ui, &response, t.s.radius_lg);
    response
}

/// The heading of a numbered step of a page (`.stephead`).
pub fn step_head(ui: &mut Ui, number: u32, done: bool, name: &str, text: &str) {
    let t = tokens(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
        let (fill, ink) = if done {
            (t.c.ok_bg, t.c.ok)
        } else {
            (t.c.accent, t.c.accent_text)
        };
        ui.painter().circle_filled(rect.center(), 14.0, fill);
        if done {
            ui.painter()
                .circle_stroke(rect.center(), 13.25, Stroke::new(1.5, t.c.ok));
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            number,
            font(Weight::Bold, t.rem(0.85)),
            ink,
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            title(ui, name);
            hint(ui, text);
        });
    });
}

const CHOICE_PAD: Vec2 = vec2(14.0, 12.0);

/// The name and the text of a choice as they are laid out in a column of this width.
fn choice_text(
    ui: &Ui,
    name: &str,
    text: &str,
    width: f32,
) -> (Arc<egui::Galley>, Arc<egui::Galley>) {
    let t = tokens(ui.ctx());
    let wrap = width - 2.0 * CHOICE_PAD.x;
    let name_galley = ui.painter().layout(
        name.to_owned(),
        font(Weight::Semibold, t.s.rem),
        t.c.text,
        wrap,
    );
    let text_galley = ui.painter().layout(
        text.to_owned(),
        font(Weight::Regular, t.rem(0.8)),
        t.c.muted,
        wrap,
    );
    (name_galley, text_galley)
}

/// How high a choice has to be for its text.
pub fn choice_height(ui: &Ui, name: &str, text: &str, width: f32) -> f32 {
    let (name, text) = choice_text(ui, name, text, width);
    name.size().y + 4.0 + text.size().y + 2.0 * CHOICE_PAD.y
}

/// One of a few big buttons that choose a way (`.choice`). Give it the width of its column.
pub fn choice(ui: &mut Ui, pressed: bool, name: &str, text: &str, width: f32) -> Response {
    choice_sized(ui, pressed, name, text, width, 0.0)
}

/// [`choice`] at least `height` high, so the choices of one row match.
pub fn choice_sized(
    ui: &mut Ui,
    pressed: bool,
    name: &str,
    text: &str,
    width: f32,
    height: f32,
) -> Response {
    let t = tokens(ui.ctx());
    let enabled = ui.is_enabled();
    let pad = CHOICE_PAD;
    let (name_galley, text_galley) = choice_text(ui, name, text, width);
    let height = height.max(name_galley.size().y + 4.0 + text_galley.size().y + 2.0 * pad.y);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, pressed, name));
    let (fill, edge) = if pressed {
        (t.c.accent_soft, t.c.accent)
    } else {
        (t.c.panel, t.c.line)
    };
    let mut painter = ui.painter().clone();
    if !enabled {
        painter.multiply_opacity(0.55);
    }
    painter.rect(
        rect,
        t.s.radius_lg,
        fill,
        Stroke::new(1.5, edge),
        StrokeKind::Inside,
    );
    let below = rect.min + pad + vec2(0.0, name_galley.size().y + 4.0);
    painter.galley(rect.min + pad, name_galley, t.c.text);
    painter.galley(below, text_galley, t.c.muted);
    focus_ring(ui, &response, t.s.radius_lg);
    response
}

/// Terms and their values in two columns (`.sum`).
pub fn summary(ui: &mut Ui, rows: &[(&str, &str)]) {
    let t = tokens(ui.ctx());
    for (term, value) in rows {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.allocate_ui(vec2(140.0, 0.0), |ui| {
                ui.set_width(140.0);
                paragraph(ui, term, Weight::Regular, t.s.rem, t.c.muted, 1.35);
            });
            ui.vertical(|ui| text(ui, value));
        });
    }
}

/// A modal box over a darkened window (`<dialog>`). Returns what the contents returned and whether
/// the admin pressed Escape or clicked beside it.
pub fn dialog<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    let t = tokens(ctx);
    let width = width.min(ctx.content_rect().width() - 32.0);
    let frame = Frame::new()
        .fill(t.c.panel)
        .stroke(Stroke::new(1.0, t.c.line_strong))
        .corner_radius(t.s.radius_lg)
        .inner_margin(egui::Margin::symmetric(24, 22));
    let shown = egui::Modal::new(Id::unique(id))
        .frame(frame)
        .backdrop_color(Color32::from_black_alpha(115))
        .show(ctx, |ui| {
            ui.set_width(width - 50.0);
            ui.spacing_mut().item_spacing.y = 10.0;
            add(ui)
        });
    let dismissed = shown.should_close();
    (shown.inner, dismissed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Look, Mode, Theme};

    /// Every shared widget on one sheet, to compare with the style sheets and to show what there is.
    #[test]
    #[ignore = "writes pictures; needs CK_SHOTS_DIR and a graphics adapter"]
    fn shots_widgets() {
        let Some(folder) = std::env::var_os("CK_SHOTS_DIR").map(std::path::PathBuf::from) else {
            #[allow(clippy::print_stdout)]
            {
                println!("skipped widgets: set CK_SHOTS_DIR to the folder for the pictures");
            }
            return;
        };
        std::fs::create_dir_all(&folder).expect("folder for the pictures");
        for (theme, name) in [(Theme::Modern, "modern"), (Theme::Classic, "classic")] {
            for (mode, mode_name) in [(Mode::Light, "light"), (Mode::Dark, "dark")] {
                let mut ticked = true;
                let mut clear = false;
                let mut typed = "erase 1".to_owned();
                let mut chosen = 1;
                let mut frame = 0;
                let mut harness = egui_kittest::Harness::builder()
                    .with_size([720.0, 1350.0])
                    .wgpu()
                    // The picker and the amount field of the sheet have no label of their own.
                    .with_accessibility_check(false)
                    .build_ui(|ui| {
                        crate::theme::apply(ui.ctx(), Look { theme, mode });
                        // The fonts arrive with the second frame.
                        frame += 1;
                        if frame > 1 {
                            sheet(ui, &mut ticked, &mut clear, &mut typed, &mut chosen);
                        }
                    });
                harness.run_steps(8);
                harness
                    .render()
                    .expect("render")
                    .save(folder.join(format!("widgets-{name}-{mode_name}.png")))
                    .expect("save");
            }
        }
    }

    fn sheet(
        ui: &mut Ui,
        ticked: &mut bool,
        clear: &mut bool,
        typed: &mut String,
        chosen: &mut u8,
    ) {
        card(ui, |ui| {
            step_head(
                ui,
                1,
                true,
                "Base image",
                "The system every stick is copied from.",
            );
            path_row(ui, "keeper-os.img", &["Keep this file in place."], |ui| {
                let _ = Btn::new("Use another image").small().show(ui);
                pill(ui, Tone::Ok, "Ready");
            });
            ui.horizontal_top(|ui| {
                let width = (ui.available_width() - 16.0) / 3.0;
                choice(ui, true, "Download", "The usual way.", width);
                choice(ui, false, "Use a file", "An image you have.", width);
                ui.add_enabled_ui(false, |ui| {
                    choice(ui, false, "Build one", "Advanced.", width)
                });
            });
        });
        card(ui, |ui| {
            step_head(ui, 2, false, "Sticks to write", "Plug in the sticks.");
            disk_row(
                ui,
                ticked,
                false,
                "Disk 1",
                &["30.8 GB", "Written before"],
                |ui| {
                    pill(ui, Tone::Warn, "Rewrite");
                },
            );
            disk_row(ui, clear, false, "Disk 2", &["61.9 GB"], |ui| {
                pill(ui, Tone::Ok, "Safe to write");
            });
            disk_row(ui, clear, true, "Disk 0", &["1.0 TB"], |ui| {
                pill(ui, Tone::Off, "Protected");
            });
            check(ui, ticked, "A ticked box");
            summary(
                ui,
                &[
                    ("Sticks", "2 (disks 1, 2)"),
                    ("Base image", "keeper-os.img"),
                ],
            );
            danger_zone(ui, |ui| {
                row(
                    ui,
                    |ui| {
                        strong(ui, "Everything on disk 1 will be erased");
                        hint(ui, "This cannot be undone.");
                    },
                    |ui| {
                        let _ = Btn::new("Erase and write").danger().show(ui);
                        Input::new(typed).mono().width(150.0).show(ui);
                    },
                );
            });
            split_line(ui, "Writing: 1.9 GB of 4.8 GB", "about 0:08 left");
            progress_bar(ui, 0.4);
            note(ui, Tone::Ok, "Written and read back.");
            lock(ui, "A locked fact.");
            messages(ui, &["a problem".to_owned()], &["a warning".to_owned()]);
            inner(ui, |ui| {
                field(ui, "Profile name", Some("Only for you."), None, |ui, id| {
                    Input::new(typed).show(ui).labelled_by(id);
                });
                field(ui, "Welcome", None, Some("Too long"), |ui, id| {
                    Input::new(typed)
                        .multiline(3)
                        .invalid(true)
                        .show(ui)
                        .labelled_by(id);
                });
                select(ui, "sheet", chosen, &[(1, "One"), (2, "Two")]);
                ui.horizontal(|ui| {
                    let _ = Btn::new("Normal").show(ui);
                    let _ = Btn::new("Primary").primary().show(ui);
                    let _ = Btn::new("Disabled").enabled(false).show(ui);
                });
            });
        });
        details(ui, "A closed card", |_| {});
    }
}
