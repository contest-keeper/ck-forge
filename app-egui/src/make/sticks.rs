//! The sticks of the organization on the Hub, and retiring the key of one.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use ck_admin_core::{HUB_URL, HubStick, list_sticks, retire_stick_credential, vault};

/// What a worker thread reports.
enum Event {
    Sticks(Result<Vec<HubStick>, String>),
    Retired(String, Result<(), String>),
}

pub struct Screen {
    sticks: Vec<HubStick>,
    /// The stick whose Retire button was clicked once; a second click confirms.
    confirming: Option<String>,
    message: String,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            sticks: Vec::new(),
            confirming: None,
            message: String::new(),
            events,
            sender,
        }
    }
}

/// One line for a stick: its identity and whether its key still works.
fn describe(stick: &HubStick) -> String {
    let machine = stick
        .machine_number
        .map_or_else(String::new, |number| format!(" machine {number}"));
    let state = if stick.retired_at.is_some() {
        " (retired)"
    } else {
        ""
    };
    format!(
        "{}  {} {} {}{machine}{state}",
        stick.stick_id, stick.team, stick.site, stick.contest
    )
}

impl Screen {
    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        self.receive();
        ui.heading("Sticks");
        if ui.button("Refresh from the Hub").clicked() {
            self.refresh(ui.ctx().clone());
        }
        let mut retire = None;
        for stick in &self.sticks {
            ui.horizontal(|ui| {
                ui.label(describe(stick));
                if stick.retired_at.is_none() {
                    let confirming = self.confirming.as_deref() == Some(&stick.stick_id);
                    let label = if confirming {
                        "Click again to retire"
                    } else {
                        "Retire"
                    };
                    if ui.button(label).clicked() {
                        if confirming {
                            retire = Some(stick.stick_id.clone());
                        } else {
                            self.confirming = Some(stick.stick_id.clone());
                        }
                    }
                }
            });
        }
        if let Some(id) = retire {
            self.confirming = None;
            self.retire(id, ui.ctx().clone());
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }

    /// Takes what the worker threads reported.
    pub fn receive(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Sticks(Ok(sticks)) => self.sticks = sticks,
                Event::Sticks(Err(why)) | Event::Retired(_, Err(why)) => self.message = why,
                Event::Retired(id, Ok(())) => {
                    for stick in &mut self.sticks {
                        if stick.stick_id == id {
                            "now".clone_into(stick.retired_at.get_or_insert_with(String::new));
                        }
                    }
                    self.message = format!("Retired the key of {id}");
                }
            }
        }
    }

    fn refresh(&self, ctx: eframe::egui::Context) {
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

    fn retire(&self, id: String, ctx: eframe::egui::Context) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = vault::load()
                .ok_or_else(|| "sign in to the Hub first".to_owned())
                .and_then(|session| {
                    retire_stick_credential(HUB_URL, &session, &id).map_err(|e| e.to_string())
                });
            let _ = sender.send(Event::Retired(id, result));
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stick(retired: bool) -> HubStick {
        serde_json::from_str(&format!(
            r#"{{"stick_id":"03c9f556","team":"T","site":"S","contest":"C","machine_number":2{}}}"#,
            if retired {
                r#","retired_at":"2026-11-24""#
            } else {
                ""
            }
        ))
        .expect("stick")
    }

    #[test]
    fn a_retired_stick_says_so() {
        assert_eq!(describe(&stick(false)), "03c9f556  T S C machine 2");
        assert!(describe(&stick(true)).ends_with("(retired)"));
    }
}
