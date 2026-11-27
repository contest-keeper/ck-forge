//! The Hub sign-in at the foot of the sidebar (`HubSignIn.svelte` and the sign-in commands of the
//! earlier window).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use ck_admin_core::{Error, HUB_URL, PairingEnd, Pushed, vault};
use eframe::egui::{
    Align, Color32, Context, Frame, Margin, Role, Stroke, TextFormat, Ui, WidgetInfo,
    text::LayoutJob, vec2,
};

use crate::state::AppState;
use crate::theme::{Tokens, Weight, font, tokens};
use crate::widgets::{self, Btn};

/// The Hub gives a code for 10 minutes; the program stops asking at the same time.
const SIGN_IN_WAIT: Duration = Duration::from_secs(600);
const POLL_EVERY: Duration = Duration::from_secs(2);

/// What the screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    SignedOut,
    /// Asking for a code, or waiting for the admin to agree in the browser.
    SigningIn,
    SignedIn(String),
    /// A sign-in that ended without a session and without a failure.
    Ended(Ended),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Denied,
    Expired,
    Cancelled,
}

impl Ended {
    /// The Hub says "gone" for a refusal and for an end alike; its 10 minutes tell them apart.
    fn gone_after(waited: Duration) -> Self {
        if waited < Duration::from_secs(590) {
            Self::Denied
        } else {
            Self::Expired
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Expired => "This code ran out of time. Get a new code to try again.",
            Self::Denied => {
                "The Hub page said this was not you, so Forge is not connected. Get a new code to try again."
            }
            Self::Cancelled => "Cancelled. Forge is not connected.",
        }
    }
}

/// What a worker thread reports.
#[derive(Debug)]
pub enum Event {
    /// The code to show while the admin agrees in the browser.
    Code(String),
    /// The admin's email and what happened to the registry that was sent right after.
    SignedIn(String, Option<String>),
    Ended(Ended),
    Failed(String),
    /// The name of the organization of the signed-in account.
    Organization(u64, String),
    /// The Hub ended the session that an earlier run saved.
    SessionEnded,
}

impl State {
    /// The state after an event, so what each answer does can be tested without a window.
    #[must_use]
    pub fn after(&self, event: Event) -> Self {
        match (self, event) {
            (Self::SigningIn, Event::SignedIn(email, _)) => Self::SignedIn(email),
            (Self::SigningIn, Event::Ended(ended)) => Self::Ended(ended),
            (Self::SigningIn, Event::Failed(why)) => Self::Failed(why),
            (Self::SignedIn(_), Event::SessionEnded) => Self::SignedOut,
            // Anything else answers a question that is no longer open.
            (state, _) => state.clone(),
        }
    }
}

pub struct Screen {
    pub state: State,
    /// The pairing code of the sign-in in progress and when it arrived.
    code: Option<(String, Instant)>,
    /// What happened to the registry sent after signing in.
    registry: Option<String>,
    /// The organization of the signed-in account, once the Hub has said; asked once per sign-in.
    organization: Option<String>,
    organization_asked: bool,
    /// Counts the sign-ins and sign-outs, so a lookup that answers late is known to be old.
    generation: u64,
    /// The saved session has not been checked with the Hub yet.
    check: bool,
    /// The session on screen is still the one an earlier run saved.
    restored: bool,
    /// Set by Cancel; each sign-in has its own.
    cancel: Arc<AtomicBool>,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Screen {
    pub fn new(state: State) -> Self {
        let (sender, events) = channel();
        Self {
            state,
            code: None,
            registry: None,
            organization: None,
            organization_asked: false,
            generation: 0,
            check: false,
            restored: false,
            cancel: Arc::default(),
            events,
            sender,
        }
    }

    /// Signed in when the credential vault of the system holds a session.
    pub fn from_vault() -> Self {
        let mut screen =
            Self::new(vault::load().map_or(State::SignedOut, |s| State::SignedIn(s.email)));
        screen.restored = matches!(screen.state, State::SignedIn(_));
        screen.check = screen.restored;
        screen
    }

    /// Waiting with a code, for the screenshots. Nothing is asked of the Hub.
    #[cfg(test)]
    pub fn pairing(code: &str) -> Self {
        let mut screen = Self::new(State::SigningIn);
        screen.code = Some((code.to_owned(), Instant::now()));
        screen
    }

    /// Called every frame: takes what the workers reported and tells the other pages.
    pub fn poll(&mut self, ctx: &Context, app: &mut AppState) {
        if std::mem::take(&mut self.check) {
            check_saved(self.sender.clone(), ctx.clone());
        }
        while let Ok(event) = self.events.try_recv() {
            match self.take(event) {
                Some(Saved::Forget) => vault::forget(),
                Some(Saved::Revoke) => {
                    self.sign_out();
                    self.state = State::Ended(Ended::Cancelled);
                }
                None => {}
            }
        }
        if matches!(self.state, State::SignedIn(_)) && !self.organization_asked {
            self.organization_asked = true;
            ask_organization(self.sender.clone(), ctx.clone(), self.generation);
        }
        app.hub_email = match &self.state {
            State::SignedIn(email) => Some(email.clone()),
            _ => None,
        };
    }

    /// Applies a worker's answer and returns what is left to do with the saved session.
    fn take(&mut self, event: Event) -> Option<Saved> {
        match &event {
            Event::Code(code) => self.code = Some((code.clone(), Instant::now())),
            // Cancel was pressed after the worker last looked.
            Event::SignedIn(..) if self.cancel.load(Ordering::Relaxed) => {
                return Some(Saved::Revoke);
            }
            Event::SignedIn(_, registry) => self.registry.clone_from(registry),
            Event::Organization(generation, name) => {
                // An answer for an earlier account is not this account's organization.
                if *generation == self.generation {
                    self.organization = Some(name.clone());
                }
                return None;
            }
            // About the saved session only: a sign-out or a newer sign-in made it old news.
            Event::SessionEnded if !self.restored => return None,
            Event::SessionEnded => {
                self.restored = false;
                self.forget_organization();
                self.state = self.state.after(event);
                return Some(Saved::Forget);
            }
            _ => {}
        }
        self.state = self.state.after(event);
        None
    }

    /// The part at the foot of the sidebar.
    pub fn show(&mut self, ui: &mut Ui, app: &AppState) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            match self.state.clone() {
                State::SignedIn(email) => {
                    let t = tokens(ui.ctx());
                    // The organization first, then the account: two short lines and one button.
                    if let Some(organization) = &self.organization {
                        widgets::paragraph(
                            ui,
                            organization,
                            Weight::Bold,
                            t.rem(0.82),
                            t.c.text,
                            1.35,
                        );
                    }
                    small(ui, &email);
                    if Btn::new("Sign out").small().show(ui).clicked() {
                        self.sign_out();
                    }
                    if let Some(registry) = &self.registry {
                        status(ui, &format!("Registry: {registry}"), false);
                    }
                }
                State::SigningIn => self.waiting(ui),
                state => {
                    small(ui, "Hub: not signed in.");
                    let again = matches!(state, State::Ended(_));
                    let label = if again {
                        "Get a new code"
                    } else {
                        "Sign in with browser"
                    };
                    if Btn::new(label).small().show(ui).clicked() {
                        self.start_sign_in(ui.ctx().clone(), app.profile_path.clone());
                    }
                    match state {
                        State::Ended(ended) => {
                            status(ui, ended.text(), ended != Ended::Cancelled);
                        }
                        State::Failed(why) => status(ui, &why, true),
                        // The earlier window kept the empty status line's room.
                        _ => {
                            ui.add_space(20.0);
                        }
                    }
                }
            }
        });
    }

    fn waiting(&mut self, ui: &mut Ui) {
        match &self.code {
            None => {
                small(ui, "Asking the Hub for a code\u{2026}");
            }
            Some((code, since)) => {
                small(
                    ui,
                    "Forge opened the Hub in your browser. Check that it shows this code, then choose \"Yes, connect Forge\".",
                );
                code_box(ui, code);
                // Rounded up, so the countdown starts at 10:00.
                let left = SIGN_IN_WAIT.saturating_sub(since.elapsed());
                let left = left.as_millis().div_ceil(1000);
                small(ui, &format!("Time left: {}:{:02}", left / 60, left % 60));
                ui.ctx().request_repaint_after(Duration::from_secs(1));
            }
        }
        // Only asks the worker to stop: the screen changes when it has answered, so no second
        // sign-in can start while this one may still get a session.
        let asked = self.cancel.load(Ordering::Relaxed);
        if Btn::new("Cancel")
            .small()
            .enabled(!asked)
            .show(ui)
            .clicked()
        {
            self.cancel.store(true, Ordering::Relaxed);
        }
        status(ui, "Waiting for you to agree in the browser\u{2026}", false);
    }

    fn start_sign_in(&mut self, ctx: Context, profile: Option<PathBuf>) {
        self.state = State::SigningIn;
        self.code = None;
        self.registry = None;
        self.forget_organization();
        self.restored = false;
        self.cancel = Arc::default();
        let cancel = Arc::clone(&self.cancel);
        let sender = self.sender.clone();
        thread::spawn(move || {
            let event = sign_in(&cancel, profile, &sender, &ctx);
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(event);
            ctx.request_repaint();
        });
    }

    fn forget_organization(&mut self) {
        self.organization = None;
        self.organization_asked = false;
        self.generation += 1;
    }

    fn sign_out(&mut self) {
        let session = vault::load();
        vault::forget();
        self.state = State::SignedOut;
        self.registry = None;
        self.forget_organization();
        self.restored = false;
        if let Some(session) = session {
            // On a thread: a slow Hub must not freeze the window.
            thread::spawn(move || ck_admin_core::sign_out(HUB_URL, &session));
        }
    }
}

/// What an answer asks of the credential vault.
#[derive(Debug, PartialEq, Eq)]
enum Saved {
    Forget,
    /// Forget it and end it at the Hub.
    Revoke,
}

/// Asks the Hub which organization the saved session belongs to. A Hub that cannot be reached or an
/// account with several organizations is not an answer: the line is left out.
fn ask_organization(sender: Sender<Event>, ctx: Context, generation: u64) {
    thread::spawn(move || {
        let Some(session) = vault::load() else {
            return;
        };
        if let Ok(name) = ck_admin_core::organization_name(HUB_URL, &session) {
            let _ = sender.send(Event::Organization(generation, name));
            ctx.request_repaint();
        }
    });
}

/// Asks the Hub whether the saved session still stands. A Hub that cannot be reached is not an
/// answer, so the session is kept.
fn check_saved(sender: Sender<Event>, ctx: Context) {
    thread::spawn(move || {
        let Some(session) = vault::load() else {
            return;
        };
        if matches!(
            ck_admin_core::list_profiles(HUB_URL, &session),
            Err(Error::SignedOut(_))
        )
        // Only the session that was asked: a sign-in may have replaced it meanwhile.
            && vault::load().is_some_and(|now| now.token() == session.token())
        {
            // The window forgets it, in step with its own sign-ins.
            let _ = sender.send(Event::SessionEnded);
            ctx.request_repaint();
        }
    });
}

/// Shows the admin a pairing code, opens the Hub in the browser and waits for the admin to agree
/// there. A profile that already has a registry sends it at once, so the Hub knows the sticks made
/// before signing in.
fn sign_in(
    cancel: &AtomicBool,
    profile: Option<PathBuf>,
    sender: &Sender<Event>,
    ctx: &Context,
) -> Event {
    let pairing = match ck_admin_core::start_pairing(HUB_URL) {
        Ok(pairing) => pairing,
        Err(why) => return Event::Failed(why.to_string()),
    };
    if cancel.load(Ordering::Relaxed) {
        return Event::Ended(Ended::Cancelled);
    }
    let _ = sender.send(Event::Code(pairing.code.clone()));
    ctx.request_repaint();
    // The code is on screen too, so a browser that does not open is not the end of it.
    let _ = open::that(&pairing.url);
    let started = Instant::now();
    match ck_admin_core::watch_pairing(HUB_URL, &pairing, SIGN_IN_WAIT, POLL_EVERY, cancel) {
        Ok(PairingEnd::Approved(session)) => {
            // Agreed while Cancel was being pressed: the session must not stay open.
            if cancel.load(Ordering::Relaxed) {
                ck_admin_core::sign_out(HUB_URL, &session);
                return Event::Ended(Ended::Cancelled);
            }
            let registry = profile
                .map(|path| ck_admin_core::folder_of(&path).join("ck-registry.json"))
                .filter(|registry| registry.is_file())
                .map(|registry| {
                    ck_admin_core::push_registry_signed_in(HUB_URL, &session, &registry)
                        .map_or_else(|why| format!("not sent: {why}"), |pushed| describe(&pushed))
                });
            // Cancel may have been pressed while the registry was being sent.
            if cancel.load(Ordering::Relaxed) {
                ck_admin_core::sign_out(HUB_URL, &session);
                return Event::Ended(Ended::Cancelled);
            }
            vault::save(&session);
            Event::SignedIn(session.email, registry)
        }
        Ok(PairingEnd::Cancelled) => Event::Ended(Ended::Cancelled),
        Ok(PairingEnd::Gone) => Event::Ended(Ended::gone_after(started.elapsed())),
        Ok(PairingEnd::TimedOut) => Event::Ended(Ended::Expired),
        Err(why) => Event::Failed(why.to_string()),
    }
}

fn describe(pushed: &Pushed) -> String {
    let mut text = format!(
        "{} added, {} already known",
        pushed.added, pushed.already_known
    );
    if !pushed.conflicts.is_empty() {
        text = format!(
            "{text}. The Hub has a different secret for {}; they were left as they were",
            pushed.conflicts.join(", ")
        );
    }
    text
}

/// The size of the text at the sidebar's foot (`.side-foot`).
fn foot_size(t: &Tokens) -> f32 {
    if t.classic() { 12.0 } else { t.rem(0.78) }
}

/// Muted text of the sidebar's foot.
fn small(ui: &mut Ui, text: &str) {
    let t = tokens(ui.ctx());
    widgets::paragraph(ui, text, Weight::Regular, foot_size(&t), t.c.muted, 1.3);
}

/// The status line (`.note`): padded, and red when it is a failure.
fn status(ui: &mut Ui, text: &str, error: bool) {
    let t = tokens(ui.ctx());
    let clear = Color32::TRANSPARENT;
    let (fill, ink, edge) = if error {
        (t.c.danger_bg, t.c.danger_text, t.c.danger_line)
    } else {
        (clear, t.c.muted, clear)
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, edge))
        .corner_radius(t.s.radius)
        .inner_margin(Margin::symmetric(13, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::paragraph(ui, text, Weight::Regular, foot_size(&t), ink, 1.3);
        });
}

/// The pairing code, large and spaced out.
fn code_box(ui: &mut Ui, code: &str) {
    let t = tokens(ui.ctx());
    let size = t.rem(1.6);
    let format = TextFormat {
        font_id: font(Weight::Mono, size),
        color: t.c.text,
        line_height: Some(1.2 * size),
        valign: Align::Center,
        extra_letter_spacing: 0.12 * size,
        ..TextFormat::default()
    };
    let job = LayoutJob::single_section(code.to_owned(), format);
    Frame::new()
        .fill(t.c.soft)
        .stroke(Stroke::new(1.0, t.c.line_strong))
        .corner_radius(t.s.radius)
        .inner_margin(Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let label = ui.label(job);
            // Read letter by letter.
            let spelled: Vec<String> = code.chars().map(String::from).collect();
            let name = format!("Your code is {}", spelled.join(" "));
            label.widget_info(|| WidgetInfo::labeled(Role::Label, true, &name));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_ends_only_the_question_that_is_still_open() {
        let waiting = State::SigningIn;
        let signed_in = State::SignedIn("a@b.c".into());
        assert_eq!(
            waiting.after(Event::SignedIn("a@b.c".into(), None)),
            signed_in
        );
        assert_eq!(
            waiting.after(Event::Ended(Ended::Cancelled)),
            State::Ended(Ended::Cancelled)
        );
        assert_eq!(
            waiting.after(Event::Failed("no".into())),
            State::Failed("no".into())
        );
        assert_eq!(waiting.after(Event::Code("X".into())), waiting);
        assert_eq!(signed_in.after(Event::SessionEnded), State::SignedOut);
        // A late answer of the saved session's check leaves a new sign-in alone.
        assert_eq!(waiting.after(Event::SessionEnded), waiting);
        assert_eq!(
            State::SignedOut.after(Event::SignedIn("a@b.c".into(), None)),
            State::SignedOut
        );
    }

    #[test]
    fn a_late_end_of_the_saved_session_leaves_a_newer_sign_in_alone() {
        let mut screen = Screen::new(State::SignedIn("old@b.c".into()));
        screen.restored = true;
        assert_eq!(screen.take(Event::SessionEnded), Some(Saved::Forget));
        assert_eq!(screen.state, State::SignedOut);

        // A second sign-in has completed; `restored` was cleared when it started.
        let mut screen = Screen::new(State::SignedIn("new@b.c".into()));
        assert_eq!(screen.take(Event::SessionEnded), None);
        assert_eq!(screen.state, State::SignedIn("new@b.c".into()));
    }

    #[test]
    fn a_sign_in_that_was_cancelled_is_revoked_even_when_it_still_arrives() {
        let mut screen = Screen::new(State::SigningIn);
        screen.cancel.store(true, Ordering::Relaxed);
        let late = Event::SignedIn("a@b.c".into(), None);
        assert_eq!(screen.take(late), Some(Saved::Revoke));
        assert_eq!(screen.state, State::SigningIn);
    }

    #[test]
    fn the_hubs_clock_tells_a_refusal_from_a_code_that_ran_out() {
        assert_eq!(Ended::gone_after(Duration::from_secs(5)), Ended::Denied);
        assert_eq!(Ended::gone_after(Duration::from_secs(595)), Ended::Expired);
    }

    #[test]
    #[ignore = "writes pictures; needs CK_SHOTS_DIR and a graphics adapter"]
    fn shots_of_the_sign_in() {
        use crate::shots::capture;
        capture("sidebar-pairing", 0.0, |forge| {
            forge.hub = Screen::pairing("KQ7-M2X");
        });
        capture("sidebar-asking", 0.0, |forge| {
            forge.hub.state = State::SigningIn;
        });
        capture("sidebar-denied", 0.0, |forge| {
            forge.hub.state = State::Ended(Ended::Denied);
        });
        capture("sidebar-signed-in", 0.0, |forge| {
            forge.hub.state = State::SignedIn("admin@example.org".to_owned());
            forge.hub.registry = Some("3 added, 1 already known".to_owned());
        });
    }
}
