//! The old keys of rewritten sticks: the question asked before a write that replaces sticks the
//! Hub knows (`ConfirmRetire.svelte`), and retiring keys afterwards (`RetireOldKeys.svelte`).

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use ck_admin_core::{HUB_URL, HubSession, HubStick, list_sticks, retire_stick_credential, vault};
use eframe::egui::{Context, Frame, Margin, ScrollArea, Stroke, Ui, vec2};

use crate::make::format::stick_name;
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
    /// The list, and the account generation it was asked in.
    Sticks(u64, Result<Vec<HubStick>, String>),
    Retired(u64, String, Result<(), String>),
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
    /// Goes up when the account changes, so a list asked for another account is dropped.
    account: u64,
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
            account: 0,
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

fn hub_error(error: ck_admin_core::Error) -> String {
    match error {
        ck_admin_core::Error::SignedOut(_) => {
            "Your Hub session has expired. Sign out and sign in again in the sidebar.".to_owned()
        }
        other => other.to_string(),
    }
}

fn list_sticks_on_hub() -> Result<Vec<HubStick>, String> {
    // A test must never reach the credential vault or the Hub.
    if cfg!(test) {
        return Err("no Hub in tests".to_owned());
    }
    let session = vault::load().ok_or_else(|| "sign in to the Hub first".to_owned())?;
    list_sticks(HUB_URL, &session).map_err(hub_error)
}

fn saved_session() -> Result<HubSession, String> {
    // A test must never reach the credential vault or retire a key, whatever it clicks.
    if cfg!(test) {
        return Err("no Hub in tests".to_owned());
    }
    vault::load().ok_or_else(|| "sign in to the Hub first".to_owned())
}

fn retire_on_hub(session: &Result<HubSession, String>, id: &str) -> Result<(), String> {
    let session = session.as_ref().map_err(Clone::clone)?;
    retire_stick_credential(HUB_URL, session, id).map_err(hub_error)
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
            Err(why) => failed.push(format!("{}: {why}", stick_name(stick, sticks))),
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
        let account = self.account;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            // Read once: another admin signing in during the loop must not retire the rest.
            let session = saved_session();
            let failed = retire_each(
                &sticks,
                |id| retire_on_hub(&session, id),
                |id| {
                    // The window may be gone by now; then nobody needs the answer.
                    let _ = sender.send(Event::Retired(account, id.to_owned(), Ok(())));
                },
            );
            let _ = sender.send(Event::OldDone(failed));
            ctx.request_repaint();
        });
    }

    /// Clears transient UI errors and open confirmation prompts when leaving the page.
    pub fn leave(&mut self) {
        self.asking = None;
        self.failure.clear();
    }

    /// The signed-in account changed: what one account's Hub said is not for another. A message
    /// about keys that are still active stays.
    pub fn forget(&mut self) {
        self.account += 1;
        self.loading = false;
        self.previous.clear();
        self.retired.clear();
        self.others = None;
        self.other.clear();
        self.asking = None;
        self.failure.clear();
    }

    /// Takes what the worker threads reported.
    pub fn receive(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Sticks(account, _) if account != self.account => {}
                // Its thread has ended; only the answer belongs to the other account.
                Event::Retired(account, _, _) if account != self.account => self.working = None,
                Event::Sticks(_, result) => {
                    self.loading = false;
                    match result {
                        Ok(sticks) => self.others = Some(sticks),
                        Err(why) => self.failure = why,
                    }
                }
                Event::Retired(_, id, result) => {
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
                // Taken whatever the account: the thread has ended, and a key that is still active
                // must stay reported.
                Event::OldDone(failed) => {
                    self.retiring_old = false;
                    if !failed.is_empty() {
                        let keys = failed.join("\n");
                        if self.old_failure.is_empty() {
                            self.old_failure = format!(
                                "The new sticks are written, but these old keys are still active:\n{keys}"
                            );
                        } else {
                            self.old_failure.push('\n');
                            self.old_failure.push_str(&keys);
                        }
                    }
                }
            }
        }
    }

    fn refresh(&mut self, ctx: Context) {
        self.loading = true;
        self.failure.clear();
        let sender = self.sender.clone();
        let account = self.account;
        thread::spawn(move || {
            let result = list_sticks_on_hub();
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Sticks(account, result));
            ctx.request_repaint();
        });
    }

    fn retire(&mut self, id: String, ctx: Context) {
        self.failure.clear();
        self.working = Some(id.clone());
        let account = self.account;
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = retire_on_hub(&saved_session(), &id);
            let _ = sender.send(Event::Retired(account, id, result));
            ctx.request_repaint();
        });
    }

    /// The sticks of `others` whose key still works, excluding sticks already shown in `previous`.
    fn open(&self) -> Vec<&HubStick> {
        self.others
            .iter()
            .flatten()
            .filter(|stick| {
                stick.retired_at.is_none()
                    && stick.archived_at.is_none()
                    && !self.retired.contains(&stick.stick_id)
                    && !self
                        .previous
                        .iter()
                        .any(|prev| prev.stick_id == stick.stick_id)
            })
            .collect()
    }

    pub fn show(&mut self, ui: &mut Ui, locked: bool) {
        let idle = !locked && self.working.is_none() && !self.retiring_old;
        if idle && self.others.is_none() && !self.loading && self.failure.is_empty() && !cfg!(test)
        {
            self.refresh(ui.ctx().clone());
        }
        widgets::title(ui, "Retire sticks");
        widgets::hint(
            ui,
            "Retire old keys after verifying rewritten sticks and finishing uploads. History stays in the Hub.",
        );
        if self.previous.is_empty() {
            widgets::hint(ui, "No recently rewritten sticks on this computer.");
        }
        let mut retire = None;
        for stick in &self.previous {
            let known = self.previous.iter().chain(self.others.iter().flatten());
            let name = stick_name(stick, known);
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
            widgets::path_row(ui, &name, &[&text], |ui| {
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
                    self.failure.clear();
                    self.asking = Some(stick.stick_id.clone());
                }
            });
            if self.asking.as_ref() == Some(&stick.stick_id)
                && let Some(sure) = sure(ui, &name, idle)
            {
                if sure {
                    retire = Some(stick.stick_id.clone());
                } else {
                    self.failure.clear();
                }
                self.asking = None;
            }
        }
        widgets::disclosure_with_default(
            ui,
            "Retire the key of another stick",
            self.previous.is_empty(),
            |ui| {
                retire = self.other_ui(ui, idle).or(retire.take());
            },
        );
        widgets::note(ui, Tone::Error, &self.failure);
        // A write may have started since the question was opened.
        if let Some(id) = retire
            && idle
        {
            self.retire(id, ui.ctx().clone());
        }
    }

    /// The pick of any stick by team and machine. Returns the id the admin confirmed to retire.
    fn other_ui(&mut self, ui: &mut Ui, idle: bool) -> Option<String> {
        widgets::hint(ui, "Pick any stick by team and computer name.");
        let refresh_label = if self.loading {
            "Refreshing\u{2026}"
        } else {
            "Refresh"
        };
        if self.others.is_none() {
            if self.loading {
                widgets::hint(ui, "Loading sticks from the Hub\u{2026}");
            } else if Btn::new("Refresh").small().enabled(idle).show(ui).clicked() {
                self.refresh(ui.ctx().clone());
            }
            return None;
        }
        let open = self.open();
        if open.is_empty() {
            widgets::hint(ui, "The Hub has no other stick with a working key.");
            if Btn::new(refresh_label)
                .small()
                .enabled(idle && !self.loading)
                .show(ui)
                .clicked()
            {
                self.asking = None;
                self.refresh(ui.ctx().clone());
            }
            return None;
        }
        let ids: Vec<String> = open.iter().map(|stick| stick.stick_id.clone()).collect();
        let known = || self.previous.iter().chain(self.others.iter().flatten());
        let labels: Vec<String> = open
            .iter()
            .map(|stick| stick_name(stick, known()))
            .collect();
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
        let mut do_refresh = false;
        ui.horizontal(|ui| {
            let working = chosen.is_some() && self.working.as_ref() == chosen;
            let label = if working {
                "Retiring\u{2026}"
            } else {
                "Retire its key\u{2026}"
            };
            let button = Btn::new(label).small().enabled(idle && chosen.is_some());
            if button.show(ui).clicked() {
                self.failure.clear();
                self.asking = chosen.cloned();
            }
            if Btn::new(refresh_label)
                .small()
                .enabled(idle && !self.loading)
                .show(ui)
                .clicked()
            {
                do_refresh = true;
            }
            if chosen.is_none() {
                widgets::hint(ui, "Choose a stick first.");
            }
        });
        if do_refresh {
            self.asking = None;
            self.refresh(ui.ctx().clone());
            return None;
        }
        let id = chosen.filter(|id| self.asking.as_ref() == Some(id))?;
        let sure = sure(ui, labels.get(at)?, idle)?;
        if !sure {
            self.failure.clear();
        }
        self.asking = None;
        sure.then(|| id.clone())
    }
}

/// The inline question before a key is retired. `Some(true)` to retire, `Some(false)` to keep.
/// While something else runs (`idle` is false) the key cannot be retired.
fn sure(ui: &mut Ui, name: &str, idle: bool) -> Option<bool> {
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
            &format!(
                "Retire the key of {name} for good? That stick can no longer report to the Hub."
            ),
            Weight::Regular,
            t.s.rem,
            t.c.danger_text,
            1.35,
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let retire = Btn::new("Retire old key").small().danger().enabled(idle);
            if retire.show(ui).clicked() {
                answer = Some(true);
            }
            if Btn::new("Keep old key").small().show(ui).clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

fn bullet(ui: &mut Ui, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(10.0);
        widgets::strong(ui, "\u{2022}");
        widgets::strong(ui, text);
    });
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
        ScrollArea::vertical()
            .id_salt("retire-old-list")
            .max_height(160.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for stick in sticks {
                    let name = stick_name(stick, sticks);
                    bullet(ui, &name);
                }
            });
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
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
            let cancel = Btn::new("Cancel").show(ui);
            // The keyboard starts on the answer that erases nothing.
            if ui.memory(|memory| memory.focused().is_none()) {
                cancel.request_focus();
            }
            if cancel.clicked() {
                answer = Some(Choice::Cancel);
            }
            let keep = pick("Write and keep old key", "Write and keep old keys");
            if Btn::new(keep).show(ui).clicked() {
                answer = Some(Choice::Keep);
            }
            let retire = pick("Write and retire old key", "Write and retire old keys");
            if Btn::new(retire).danger().show(ui).clicked() {
                answer = Some(Choice::Retire);
            }
        });
        answer
    });
    answer.or(dismissed.then_some(Choice::Cancel))
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;

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
    fn a_stick_list_that_comes_back_after_the_account_changed_is_dropped() {
        let mut screen = Screen::default();
        let late = Event::Sticks(screen.account, Ok(vec![stick("a")]));
        screen.forget();
        assert!(screen.sender.send(late).is_ok());
        screen.receive();
        assert!(screen.others.is_none());
    }

    #[test]
    fn a_confirmed_retirement_does_not_start_while_a_write_runs() {
        let mut screen = Screen {
            previous: vec![stick("a")],
            asking: Some("a".to_owned()),
            ..Screen::default()
        };
        let mut harness = egui_kittest::Harness::builder()
            .with_size([700.0, 600.0])
            .with_accessibility_check(false)
            .build_ui(|ui| {
                crate::theme::apply(ui.ctx(), crate::theme::Look::default());
                let semibold = crate::theme::font(Weight::Semibold, 1.0).family;
                if ui.fonts(|fonts| fonts.definitions().families.contains_key(&semibold)) {
                    screen.show(ui, true);
                }
            });
        harness.run_steps(4);
        harness.get_by_label("Retire old key").click();
        harness.run_steps(4);
        drop(harness);
        assert!(screen.working.is_none() && screen.asking.is_some());
    }

    #[test]
    fn only_sticks_with_a_working_key_are_offered_for_retiring() {
        let mut gone = stick("b");
        gone.retired_at = Some("2026-11-24".to_owned());
        let mut archived = stick("c");
        archived.archived_at = Some("2026-11-24".to_owned());
        let mut screen = Screen {
            previous: vec![stick("e")],
            others: Some(vec![stick("a"), gone, archived, stick("d"), stick("e")]),
            ..Screen::default()
        };
        let done = Event::Retired(screen.account, "d".to_owned(), Ok(()));
        assert!(screen.sender.send(done).is_ok());
        screen.receive();
        let open: Vec<&str> = screen.open().iter().map(|s| s.stick_id.as_str()).collect();
        assert_eq!(open, ["a"]);
    }

    #[test]
    fn account_switch_clears_active_retire_state_and_drops_stale_events() {
        let mut screen = Screen {
            previous: vec![stick("a")],
            retired: BTreeSet::from(["a".to_owned()]),
            working: Some("b".to_owned()),
            retiring_old: true,
            asking: Some("a".to_owned()),
            failure: "old error".to_owned(),
            ..Screen::default()
        };
        let stale_retired = Event::Retired(screen.account, "b".to_owned(), Ok(()));
        let done = Event::OldDone(vec!["a: down".to_owned()]);
        screen.forget();
        assert!(screen.retired.is_empty());
        // Both threads still run, so closing and writing stay refused.
        assert!(screen.working.is_some() && screen.retiring_old());
        assert!(screen.asking.is_none());
        assert_eq!(screen.failure, "");

        assert!(screen.sender.send(stale_retired).is_ok());
        assert!(screen.sender.send(done).is_ok());
        screen.receive();
        assert!(screen.retired.is_empty());
        assert!(!screen.busy());
        assert!(screen.old_failure.contains("a: down"));
        let later = Event::OldDone(vec!["b: timeout".to_owned()]);
        assert!(screen.sender.send(later).is_ok());
        screen.receive();
        assert!(screen.old_failure.contains("a: down"));
        assert!(screen.old_failure.contains("b: timeout"));
    }

    #[test]
    fn confirm_retire_wraps_buttons_and_bounds_long_stick_list() {
        let sticks: Vec<HubStick> = (1..=8).map(|i| stick(&format!("s{i}"))).collect();
        let mut choice = None;
        let mut harness = egui_kittest::Harness::builder()
            .with_size([520.0, 520.0])
            .with_accessibility_check(false)
            .build_ui(|ui| {
                crate::theme::apply(ui.ctx(), crate::theme::Look::default());
                let semibold = crate::theme::font(Weight::Semibold, 1.0).family;
                if ui.fonts(|fonts| fonts.definitions().families.contains_key(&semibold))
                    && let Some(picked) = confirm_retire(ui.ctx(), &sticks)
                {
                    choice = Some(picked);
                }
            });
        harness.run_steps(4);
        harness.get_by_label("Write and keep old keys").click();
        harness.run_steps(2);
        drop(harness);
        assert_eq!(choice, Some(Choice::Keep));
    }
}
