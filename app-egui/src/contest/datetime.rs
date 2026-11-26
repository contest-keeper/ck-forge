//! A date and time as the admin types or picks it, in the place of the browser's
//! `datetime-local` field, and the UTC times a stick key stores.

use ck_protocol::LocalDateTime;
use eframe::egui::{self, Rect, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2};
use time::format_description::well_known::Rfc3339;
use time::format_description::{BorrowedFormatItem, parse_borrowed};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use crate::theme::tokens;
use crate::widgets::{Btn, Input};

/// How a time is shown and typed, as the earlier window showed it: `11/26/2026 05:14 PM`.
const SHOWN: &str = "[month]/[day]/[year] [hour repr:12]:[minute] [period case_sensitive:false]";
const PLACEHOLDER: &str = "mm/dd/yyyy --:-- --";

fn items() -> Vec<BorrowedFormatItem<'static>> {
    // A constant format; the tests would fail on a wrong one.
    parse_borrowed::<2>(SHOWN).unwrap_or_default()
}

/// A typed date and time. The form of the profile file (`2026-11-15T09:00`) is read too.
pub fn parse(text: &str) -> Option<PrimitiveDateTime> {
    let text = text.trim();
    PrimitiveDateTime::parse(text, &items()).ok().or_else(|| {
        text.parse::<LocalDateTime>()
            .ok()
            .map(LocalDateTime::as_primitive)
    })
}

pub fn shown(at: PrimitiveDateTime) -> String {
    at.format(&items()).unwrap_or_default()
}

/// This computer's distance from UTC at a moment. None where the system does not tell it (macOS
/// with several threads); the times are then taken and shown as UTC, and the page says so.
fn local_offset(at: OffsetDateTime) -> Option<UtcOffset> {
    UtcOffset::local_offset_at(at).ok()
}

fn offset_or_utc(at: OffsetDateTime) -> UtcOffset {
    local_offset(at).unwrap_or(UtcOffset::UTC)
}

/// The name of the clock the times are typed on, such as `UTC+02:00`.
pub fn clock_name() -> String {
    match local_offset(OffsetDateTime::now_utc()) {
        Some(offset) => format!(
            "UTC{}{:02}:{:02}",
            if offset.is_negative() { '-' } else { '+' },
            offset.whole_hours().unsigned_abs(),
            offset.minutes_past_hour().unsigned_abs()
        ),
        None => "UTC".to_owned(),
    }
}

fn utc_text(at: OffsetDateTime) -> String {
    // Whole seconds, as a stick key is written.
    at.replace_nanosecond(0)
        .ok()
        .and_then(|at| at.format(&Rfc3339).ok())
        .unwrap_or_default()
}

/// A UTC time such as `2026-11-19T08:00:00Z` as this computer's clock shows it. Empty when it is
/// not a time.
pub fn utc_to_shown(utc: &str) -> String {
    let Ok(at) = OffsetDateTime::parse(utc, &Rfc3339) else {
        return String::new();
    };
    let at = at.to_offset(offset_or_utc(at));
    shown(PrimitiveDateTime::new(at.date(), at.time()))
}

/// What was typed (this computer's clock) as the UTC time a stick key needs. None when it is not
/// a date and time.
pub fn shown_to_utc(typed: &str) -> Option<String> {
    let local = parse(typed)?;
    // The offset depends on the moment, which depends on the offset: guess, then look again.
    let guess = local.assume_offset(offset_or_utc(local.assume_utc()));
    let at = local.assume_offset(offset_or_utc(guess));
    Some(utc_text(at.to_offset(UtcOffset::UTC)))
}

/// The window a new limit of the stick key starts from: this minute to a day later.
pub fn default_window(now: OffsetDateTime) -> (String, String) {
    let minute = now.replace_second(0).unwrap_or(now);
    (utc_text(minute), utc_text(minute + time::Duration::DAY))
}

/// A text field for a date and time with a calendar at its right. True when the text changed.
pub fn input(ui: &mut Ui, label: egui::Id, name: &str, text: &mut String, invalid: bool) -> bool {
    let t = tokens(ui.ctx());
    let response = Input::new(text)
        .placeholder(PLACEHOLDER)
        .invalid(invalid)
        .show(ui)
        .labelled_by(label);
    let mut changed = response.changed();
    let place = Rect::from_center_size(
        pos2(response.rect.right() - 22.0, response.rect.center().y),
        vec2(28.0, response.rect.height().min(28.0)),
    );
    let button = ui.interact(place, response.id.with("calendar"), Sense::click());
    let title = format!("Pick {name}");
    button.widget_info(|| WidgetInfo::labeled(Role::Button, true, &title));
    paint_calendar(ui, place.center(), t.c.text);
    if button.has_focus() {
        let ring = Stroke::new(2.0, t.c.accent);
        ui.painter()
            .rect_stroke(place, 4.0, ring, StrokeKind::Inside);
    }
    egui::Popup::from_toggle_button_response(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| changed |= calendar(ui, response.id, text));
    changed
}

fn paint_calendar(ui: &Ui, center: egui::Pos2, color: egui::Color32) {
    let page = Rect::from_center_size(center, vec2(13.0, 12.0));
    let painter = ui.painter();
    painter.rect_stroke(page, 1.0, Stroke::new(1.5, color), StrokeKind::Inside);
    painter.rect_filled(Rect::from_min_size(page.min, vec2(13.0, 3.0)), 1.0, color);
}

/// The days of a month and the time of day. A choice is written into `text`.
fn calendar(ui: &mut Ui, id: egui::Id, text: &mut String) -> bool {
    let chosen = parse(text);
    let today = ck_admin_core::local_now().as_primitive();
    let key = id.with("month");
    // Months since the year 0, so that turning a page is one step.
    let months: i32 = ui.data(|data| data.get_temp(key)).unwrap_or_else(|| {
        let date = chosen.unwrap_or(today).date();
        date.year() * 12 + i32::from(u8::from(date.month())) - 1
    });
    let year = months.div_euclid(12);
    let month = u8::try_from(months.rem_euclid(12) + 1).unwrap_or(1);
    let Some(first) = Month::try_from(month)
        .ok()
        .and_then(|month| Date::from_calendar_date(year, month, 1).ok())
    else {
        return false;
    };
    let mut turn = 0;
    ui.horizontal(|ui| {
        if ui.button("<").on_hover_text("Earlier month").clicked() {
            turn = -1;
        }
        ui.label(format!("{} {year}", first.month()));
        if ui.button(">").on_hover_text("Later month").clicked() {
            turn = 1;
        }
    });
    if turn != 0 {
        ui.data_mut(|data| data.insert_temp(key, months + turn));
    }
    let at = chosen.map_or(Time::MIDNIGHT, PrimitiveDateTime::time);
    let mut picked = days(ui, id, first, chosen.map(PrimitiveDateTime::date))
        .map(|date| PrimitiveDateTime::new(date, at));
    if let Some(chosen) = chosen {
        let (mut hour, mut minute) = (at.hour(), at.minute());
        ui.horizontal(|ui| {
            ui.label("Time (24 h)");
            let hours = ui.add(egui::DragValue::new(&mut hour).range(0..=23));
            let minutes = ui.add(egui::DragValue::new(&mut minute).range(0..=59));
            if (hours.changed() || minutes.changed())
                && let Ok(time) = Time::from_hms(hour, minute, 0)
            {
                picked = Some(PrimitiveDateTime::new(chosen.date(), time));
            }
        });
    }
    if Btn::new("Clear").small().show(ui).clicked() {
        text.clear();
        return true;
    }
    if let Some(picked) = picked {
        *text = shown(picked);
    }
    picked.is_some()
}

/// The grid of the days of the month that starts on `first`; the day that was clicked.
fn days(ui: &mut Ui, id: egui::Id, first: Date, chosen: Option<Date>) -> Option<Date> {
    let mut picked = None;
    egui::Grid::new(id.with("days")).show(ui, |ui| {
        for day in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
            ui.label(day);
        }
        ui.end_row();
        let skip = first.weekday().number_days_from_monday();
        for _ in 0..skip {
            ui.label("");
        }
        for day in 1..=first.month().length(first.year()) {
            let Ok(date) = first.replace_day(day) else {
                continue;
            };
            if ui
                .selectable_label(chosen == Some(date), day.to_string())
                .clicked()
            {
                picked = Some(date);
            }
            if (skip + day).is_multiple_of(7) {
                ui.end_row();
            }
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_typed_as_shown_or_as_the_profile_file_writes_it() {
        let shown_form = parse("11/26/2026 05:14 pm").expect("shown form");
        assert_eq!(parse("2026-11-26T17:14"), Some(shown_form));
        assert_eq!(shown(shown_form), "11/26/2026 05:14 PM");
        for bad in ["", "tomorrow", "26/11/2026 05:14 PM", "11/26/2026"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_typed_time_goes_to_utc_and_back() {
        let utc = shown_to_utc("06/15/2026 12:30 PM").expect("a time");
        assert!(utc.ends_with(":00Z"), "{utc}");
        assert_eq!(utc_to_shown(&utc), "06/15/2026 12:30 PM");
        assert_eq!(shown_to_utc("soon"), None);
        assert_eq!(utc_to_shown("soon"), "");
    }

    #[test]
    fn a_new_key_window_is_a_day_from_this_minute() {
        let now = OffsetDateTime::parse("2026-11-26T15:14:59Z", &Rfc3339).expect("now");
        let (from, until) = default_window(now);
        assert_eq!(from, "2026-11-26T15:14:00Z");
        assert_eq!(until, "2026-11-27T15:14:00Z");
    }
}
