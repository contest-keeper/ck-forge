//! The Hub sign-in screen: the first of the screens, since every later one asks the Hub.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use ck_admin_core::{HUB_URL, HubSession, vault};

use crate::state::AppState;
use crate::widgets::{self, Btn, Tone};

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

impl Screen {
    pub fn new(state: State) -> Self {
        let (sender, events) = channel();
        Self {
            state,
            code: None,
            events,
            sender,
        }
    }

    /// Signed in when the credential vault of the system holds a session.
    pub fn from_vault() -> Self {
        Self::new(vault::load().map_or(State::SignedOut, |session| State::SignedIn(session.email)))
    }

    /// Called every frame: takes what the sign-in reported and tells the other pages.
    pub fn poll(&mut self, app: &mut AppState) {
        while let Ok(event) = self.events.try_recv() {
            if let Event::Code(code) = &event {
                self.code = Some(code.clone());
            }
            self.state = self.state.after(event);
        }
        app.hub_email = match &self.state {
            State::SignedIn(email) => Some(email.clone()),
            _ => None,
        };
    }

    /// The part at the foot of the sidebar.
    pub fn show(&mut self, ui: &mut eframe::egui::Ui) {
        match self.state.clone() {
            State::SignedOut => {
                widgets::hint(
                    ui,
                    "Hub: not signed in. Sticks made now keep their events on the stick.",
                );
                if Btn::new("Sign in with browser").small().show(ui).clicked() {
                    self.start_sign_in(ui.ctx().clone());
                }
            }
            State::SigningIn => {
                widgets::hint(ui, "Waiting for you to agree in the browser\u{2026}");
                if let Some(code) = &self.code {
                    widgets::hint(ui, "Check that the Hub page shows this code:");
                    widgets::mono(ui, code);
                }
                ui.spinner();
            }
            State::SignedIn(email) => {
                widgets::hint(ui, &format!("Hub: signed in as {email}"));
                if Btn::new("Sign out").small().show(ui).clicked() {
                    sign_out();
                    self.state = State::SignedOut;
                }
            }
            State::Failed(why) => {
                widgets::note(ui, Tone::Error, &why);
                if Btn::new("Try again").small().show(ui).clicked() {
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
