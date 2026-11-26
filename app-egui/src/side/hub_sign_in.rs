//! The Hub sign-in screen: the first of the screens, since every later one asks the Hub.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use ck_admin_core::{HUB_URL, HubSession, vault};

const SIGN_IN_WAIT: Duration = Duration::from_secs(600);
const POLL_EVERY: Duration = Duration::from_secs(2);

/// What the screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    SignedOut,
    /// The browser is open and the Hub has not answered yet.
    SigningIn,
    SignedIn(String),
    Failed(String),
}

/// What the worker thread reports.
#[derive(Debug)]
pub enum Event {
    /// The code to show while the admin agrees in the browser.
    Code(String),
    SignedIn(String),
    Failed(String),
}

impl State {
    /// The state after an event, so what each answer does can be tested without a window.
    #[must_use]
    pub fn after(&self, event: Event) -> Self {
        match (self, event) {
            // A sign-out clicked while the browser was open already left this screen.
            (Self::SignedOut, _) => Self::SignedOut,
            (state, Event::Code(_)) => state.clone(),
            (_, Event::SignedIn(email)) => Self::SignedIn(email),
            (_, Event::Failed(why)) => Self::Failed(why),
        }
    }
}

pub struct Screen {
    pub state: State,
    /// The pairing code of the sign-in in progress.
    code: Option<String>,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        let state =
            vault::load().map_or(State::SignedOut, |session| State::SignedIn(session.email));
        Self {
            state,
            code: None,
            events,
            sender,
        }
    }
}

impl Screen {
    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        while let Ok(event) = self.events.try_recv() {
            if let Event::Code(code) = &event {
                self.code = Some(code.clone());
            }
            self.state = self.state.after(event);
        }
        ui.heading("Keeper Hub");
        match self.state.clone() {
            State::SignedOut => {
                if ui.button("Sign in").clicked() {
                    self.start_sign_in(ui.ctx().clone());
                }
            }
            State::SigningIn => {
                ui.label("Waiting for you to agree in the browser…");
                if let Some(code) = &self.code {
                    ui.label("Check that the Hub page shows this code:");
                    ui.heading(code);
                }
                ui.spinner();
            }
            State::SignedIn(email) => {
                ui.label(format!("Signed in as {email}"));
                if ui.button("Sign out").clicked() {
                    sign_out();
                    self.state = State::SignedOut;
                }
            }
            State::Failed(why) => {
                ui.colored_label(ui.visuals().error_fg_color, why);
                if ui.button("Try again").clicked() {
                    self.start_sign_in(ui.ctx().clone());
                }
            }
        }
    }

    fn start_sign_in(&mut self, ctx: eframe::egui::Context) {
        self.state = State::SigningIn;
        self.code = None;
        let sender = self.sender.clone();
        thread::spawn(move || {
            let event = match sign_in(&sender, &ctx) {
                Ok(session) => {
                    vault::save(&session);
                    Event::SignedIn(session.email)
                }
                Err(why) => Event::Failed(why),
            };
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(event);
            ctx.request_repaint();
        });
    }
}

fn sign_in(sender: &Sender<Event>, ctx: &eframe::egui::Context) -> Result<HubSession, String> {
    let pairing = ck_admin_core::start_pairing(HUB_URL).map_err(|e| e.to_string())?;
    // The window may be gone by now; then nobody needs the code.
    let _ = sender.send(Event::Code(pairing.code.clone()));
    ctx.request_repaint();
    open::that(&pairing.url).map_err(|e| format!("could not open the browser: {e}"))?;
    ck_admin_core::wait_for_pairing(HUB_URL, &pairing, SIGN_IN_WAIT, POLL_EVERY)
        .map_err(|e| e.to_string())
}

fn sign_out() {
    if let Some(session) = vault::load() {
        ck_admin_core::sign_out(HUB_URL, &session);
    }
    vault::forget();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_ends_the_wait_and_a_sign_out_is_not_undone_by_a_late_one() {
        let waiting = State::SigningIn;
        assert_eq!(
            waiting.after(Event::SignedIn("a@b.c".into())),
            State::SignedIn("a@b.c".into())
        );
        assert_eq!(
            waiting.after(Event::Failed("no".into())),
            State::Failed("no".into())
        );
        assert_eq!(
            State::SignedOut.after(Event::SignedIn("a@b.c".into())),
            State::SignedOut
        );
    }
}
