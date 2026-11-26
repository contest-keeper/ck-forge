//! The old keys of rewritten sticks: the question asked before a write that replaces sticks the
//! Hub knows (`ConfirmRetire.svelte`), and retiring keys afterwards (`RetireOldKeys.svelte`).

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use ck_admin_core::{HUB_URL, HubStick, list_sticks, retire_stick_credential, vault};
use eframe::egui::{Align, Context, Frame, Layout, Margin, Stroke, Ui, vec2};

use crate::make::format::stick_label;
use crate::theme::{Weight, tokens};
use crate::widgets::{self, Btn, Tone};

/// What the admin answers when a write replaces sticks the Hub knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Retire,
    Keep,
    Cancel,
}

/// What a worker thread reports.
enum Event {
    Sticks(Result<Vec<HubStick>, String>),
    Retired(String, Result<(), String>),
    /// The keys of the replaced sticks were tried: the ones that are still active, and why.
    OldDone(Vec<String>),
}

pub struct Screen {
    /// The sticks that were on the disks just written, as the Hub knew them before the write.
    pub(super) previous: Vec<HubStick>,
    retired: BTreeSet<String>,
    /// The Hub's own list, for a stick the Hub could not match to a disk.
    pub(super) others: Option<Vec<HubStick>>,
    /// The id of the stick chosen from `others`.
    pub(super) other: String,
    loading: bool,
    /// The stick whose key is being retired after a click here.
    working: Option<String>,
    /// The keys of the replaced sticks are being retired after a write.
    retiring_old: bool,
    /// The stick whose retiring waits for a second click: it cannot be undone.
    pub(super) asking: Option<String>,
    failure: String,
    /// Which old keys a write could not retire.
    pub(super) old_failure: String,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            previous: Vec::new(),
            retired: BTreeSet::new(),
            others: None,
            other: String::new(),
            loading: false,
            working: None,
            retiring_old: false,
            asking: None,
            failure: String::new(),
            old_failure: String::new(),
            events,
            sender,
        }
    }
}

fn retire_on_hub(id: &str) -> Result<(), String> {
    let session = vault::load().ok_or_else(|| "sign in to the Hub first".to_owned())?;
    retire_stick_credential(HUB_URL, &session, id).map_err(|e| e.to_string())
}

/// Retires the keys one by one; a key that fails is told, and the others still go.
fn retire_each(
    sticks: &[HubStick],
    mut retire: impl FnMut(&str) -> Result<(), String>,
    mut retired: impl FnMut(&str),
) -> Vec<String> {
    let mut failed = Vec::new();
    for stick in sticks {
        match retire(&stick.stick_id) {
            Ok(()) => retired(&stick.stick_id),
            Err(why) => failed.push(format!("{}: {why}", stick_label(stick))),
        }
    }
    failed
}

impl Screen {
    /// Whether the old keys of a write are still being retired, which belongs to that write.
    pub fn retiring_old(&self) -> bool {
        self.retiring_old
    }

    pub fn busy(&self) -> bool {
        self.retiring_old || self.working.is_some()
    }

    /// Called only when a write ended with everything written and read back: remembers the sticks
    /// it replaced and, when the admin asked for it, retires their keys.
    pub fn after_write(&mut self, previous: Vec<HubStick>, retire: bool, ctx: &Context) {
        self.retired.clear();
        self.others = None;
        self.old_failure.clear();
        self.previous = previous;
        if !retire || self.previous.is_empty() {
            return;
        }
        self.retiring_old = true;
        let sticks = self.previous.clone();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let failed = retire_each(&sticks, retire_on_hub, |id| {
                // The window may be gone by now; then nobody needs the answer.
                let _ = sender.send(Event::Retired(id.to_owned(), Ok(())));
            });
            let _ = sender.send(Event::OldDone(failed));
            ctx.request_repaint();
        });
    }

    /// The signed-in account changed: what one account's Hub said is not for another. A message
    /// about keys that are still active stays.
    pub fn forget(&mut self) {
        self.previous.clear();
        self.others = None;
        self.other.clear();
        self.asking = None;
        self.failure.clear();
    }

    /// Takes what the worker threads reported.
    pub fn receive(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Sticks(result) => {
                    self.loading = false;
                    match result {
                        Ok(sticks) => self.others = Some(sticks),
                        Err(why) => self.failure = why,
                    }
                }
                Event::Retired(id, result) => {
                    self.working = None;
                    match result {
                        Ok(()) => {
                            if self.other == id {
                                self.other.clear();
                            }
                            self.retired.insert(id);
                        }
                        Err(why) => self.failure = why,
                    }
                }
                Event::OldDone(failed) => {
                    self.retiring_old = false;
                    if !failed.is_empty() {
                        self.old_failure = format!(
                            "The new sticks are written, but these old keys are still active:\n{}",
                            failed.join("\n")
                        );
                    }
                }
            }
        }
    }

    fn refresh(&mut self, ctx: Context) {
        self.loading = true;
        self.failure.clear();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = vault::load()
                .ok_or_else(|| "sign in to the Hub first".to_owned())
                .and_then(|session| list_sticks(HUB_URL, &session).map_err(|e| e.to_string()));
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Sticks(result));
            ctx.request_repaint();
        });
    }

    fn retire(&mut self, id: String, ctx: Context) {
        self.failure.clear();
        self.working = Some(id.clone());
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = retire_on_hub(&id);
            let _ = sender.send(Event::Retired(id, result));
            ctx.request_repaint();
        });
    }

    /// The sticks of `others` whose key still works.
    fn open(&self) -> Vec<&HubStick> {
        self.others
            .iter()
            .flatten()
            .filter(|stick| {
                stick.retired_at.is_none()
                    && stick.archived_at.is_none()
                    && !self.retired.contains(&stick.stick_id)
            })
            .collect()
    }

    pub fn show(&mut self, ui: &mut Ui, locked: bool) {
        let idle = !locked && self.working.is_none() && !self.retiring_old;
        widgets::title(ui, "Retire old keys");
        widgets::hint(
            ui,
            "A rewritten stick has a new key, and the old key keeps working until you retire it. \
             Do it after you have checked the new stick and any uploads from the old one are done. \
             The old stick's history stays in the Hub.",
        );
        let mut retire = None;
        for stick in &self.previous {
            let mut text = "The stick that was on a disk you just wrote".to_owned();
            if let Some(disk) = &stick.hardware {
                let times = if disk.write_count == 1 {
                    "time"
                } else {
                    "times"
                };
                text = format!("{text}, written {} {times} before", disk.write_count);
            }
            text.push('.');
            widgets::path_row(ui, &stick_label(stick), &[&text], |ui| {
                if self.retired.contains(&stick.stick_id) {
                    widgets::pill(ui, Tone::Ok, "Key retired");
                    return;
                }
                let working = self.working.as_ref() == Some(&stick.stick_id);
                let label = if working {
                    "Retiring\u{2026}"
                } else {
                    "Retire its old key\u{2026}"
                };
                if Btn::new(label).small().enabled(idle).show(ui).clicked() {
                    self.asking = Some(stick.stick_id.clone());
                }
            });
            if self.asking.as_ref() == Some(&stick.stick_id)
                && let Some(sure) = sure(ui)
            {
                retire = sure.then(|| stick.stick_id.clone());
                self.asking = None;
            }
        }
        widgets::disclosure(ui, "Retire the key of another stick", |ui| {
            retire = self.other_ui(ui, idle).or(retire.take());
        });
        widgets::note(ui, Tone::Error, &self.failure);
        if let Some(id) = retire {
            self.retire(id, ui.ctx().clone());
        }
    }

    /// The pick of any stick by team and machine. Returns the id the admin confirmed to retire.
    fn other_ui(&mut self, ui: &mut Ui, idle: bool) -> Option<String> {
        widgets::hint(
            ui,
            "Forge only recognises sticks it wrote after it started telling the Hub which disk it \
             writes to, and only when the disk reports a usable serial number. For any other \
             stick, pick it here by team and machine.",
        );
        if self.others.is_none() {
            let text = if self.loading {
                "Loading\u{2026}"
            } else {
                "Load the sticks from the Hub"
            };
            let button = Btn::new(text).small().enabled(!self.loading);
            if button.show(ui).clicked() {
                self.refresh(ui.ctx().clone());
            }
            return None;
        }
        let open = self.open();
        if open.is_empty() {
            widgets::hint(ui, "The Hub has no other stick with a working key.");
            return None;
        }
        let ids: Vec<String> = open.iter().map(|stick| stick.stick_id.clone()).collect();
        let labels: Vec<String> = open.iter().map(|stick| stick_label(stick)).collect();
        let mut options = vec![(usize::MAX, "Choose a stick")];
        options.extend(labels.iter().map(String::as_str).enumerate());
        let mut at = ids
            .iter()
            .position(|id| *id == self.other)
            .unwrap_or(usize::MAX);
        ui.scope(|ui| {
            ui.set_max_width(420.0);
            widgets::field(ui, "Stick", None, None, |ui, _| {
                widgets::select(ui, "other-stick", &mut at, &options);
            });
        });
        self.other = ids.get(at).cloned().unwrap_or_default();
        let chosen = ids.get(at);
        ui.horizontal(|ui| {
            let working = chosen.is_some() && self.working.as_ref() == chosen;
            let label = if working {
                "Retiring\u{2026}"
            } else {
                "Retire its key\u{2026}"
            };
            let button = Btn::new(label).small().enabled(idle && chosen.is_some());
            if button.show(ui).clicked() {
                self.asking = chosen.cloned();
            }
            if chosen.is_none() {
                widgets::hint(ui, "Choose a stick first.");
            }
        });
        let id = chosen.filter(|id| self.asking.as_ref() == Some(id))?;
        let sure = sure(ui)?;
        self.asking = None;
        sure.then(|| id.clone())
    }
}

/// The inline question before a key is retired. `Some(true)` to retire, `Some(false)` to keep.
fn sure(ui: &mut Ui) -> Option<bool> {
    let t = tokens(ui.ctx());
    let mut answer = None;
    let frame = Frame::new()
        .fill(t.c.danger_bg)
        .stroke(Stroke::new(1.0, t.c.danger_line))
        .corner_radius(t.s.radius)
        .inner_margin(Margin::symmetric(14, 10));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::paragraph(
            ui,
            "Retire this key for good? That stick can no longer report to the Hub.",
            Weight::Regular,
            t.s.rem,
            t.c.danger_text,
            1.35,
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let retire = Btn::new("Retire the key").small().danger();
            if retire.show(ui).clicked() {
                answer = Some(true);
            }
            if Btn::new("Keep it").small().show(ui).clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// The question before a write that replaces sticks the Hub knows. Escape and a click beside the
/// box count as Cancel, so nothing is erased by dismissing it.
pub fn confirm_retire(ctx: &Context, sticks: &[HubStick]) -> Option<Choice> {
    let one = sticks.len() == 1;
    let pick = |single: &'static str, many: &'static str| if one { single } else { many };
    let (answer, dismissed) = widgets::dialog(ctx, "retire-old", 660.0, |ui| {
        let t = tokens(ui.ctx());
        let heading = pick(
            "This stick was written before",
            "These sticks were written before",
        );
        widgets::paragraph(ui, heading, Weight::Bold, t.s.h2, t.c.text, 1.35);
        widgets::text(
            ui,
            &format!(
                "The Hub has {} from your account on {} you are about to erase:",
                pick("a stick", "sticks"),
                pick("the disk", "these disks"),
            ),
        );
        for stick in sticks {
            widgets::strong(ui, &format!("   \u{2022}  {}", stick_label(stick)));
        }
        widgets::paragraph(
            ui,
            &format!(
                "Retiring ends the old key at the Hub for good: that stick can no longer report, \
                 and its history stays visible. Forge does it only after the new {} written and \
                 checked. A failed write retires nothing.",
                pick("stick is", "sticks are"),
            ),
            Weight::Regular,
            t.s.hint,
            t.c.muted,
            1.45,
        );
        let mut answer = None;
        let size = vec2(ui.available_width(), t.s.button);
        let layout = Layout::right_to_left(Align::Center).with_main_wrap(true);
        ui.allocate_ui_with_layout(size, layout, |ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
            let retire = pick(
                "Write and retire the old key",
                "Write and retire the old keys",
            );
            if Btn::new(retire).danger().show(ui).clicked() {
                answer = Some(Choice::Retire);
            }
            let keep = pick("Write and keep the old key", "Write and keep the old keys");
            if Btn::new(keep).show(ui).clicked() {
                answer = Some(Choice::Keep);
            }
            let cancel = Btn::new("Cancel").show(ui);
            // The keyboard starts on the answer that erases nothing.
            if ui.memory(|memory| memory.focused().is_none()) {
                cancel.request_focus();
            }
            if cancel.clicked() {
                answer = Some(Choice::Cancel);
            }
        });
        answer
    });
    answer.or(dismissed.then_some(Choice::Cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn stick(id: &str) -> HubStick {
        serde_json::from_str(&format!(r#"{{"stick_id":"{id}","team":"{id}"}}"#)).expect("stick")
    }

    #[test]
    fn one_old_key_that_fails_does_not_stop_the_others_and_is_named() {
        let sticks = [stick("a"), stick("b"), stick("c")];
        let mut retired = Vec::new();
        let failed = retire_each(
            &sticks,
            |id| {
                if id == "b" {
                    Err("down".to_owned())
                } else {
                    Ok(())
                }
            },
            |id| retired.push(id.to_owned()),
        );
        assert_eq!(retired, ["a", "c"]);
        assert_eq!(failed, ["b: down"]);
    }

    #[test]
    fn keeping_the_old_keys_retires_nothing() {
        let mut screen = Screen::default();
        screen.after_write(vec![stick("a")], false, &Context::default());
        assert!(!screen.busy());
        assert_eq!(screen.previous.len(), 1);
    }

    #[test]
    fn only_sticks_with_a_working_key_are_offered_for_retiring() {
        let mut gone = stick("b");
        gone.retired_at = Some("2026-11-24".to_owned());
        let mut archived = stick("c");
        archived.archived_at = Some("2026-11-24".to_owned());
        let mut screen = Screen {
            others: Some(vec![stick("a"), gone, archived, stick("d")]),
            ..Screen::default()
        };
        let done = Event::Retired("d".to_owned(), Ok(()));
        assert!(screen.sender.send(done).is_ok());
        screen.receive();
        let open: Vec<&str> = screen.open().iter().map(|s| s.stick_id.as_str()).collect();
        assert_eq!(open, ["a"]);
    }
}
