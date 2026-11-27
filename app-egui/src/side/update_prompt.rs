//! The update dialog (`UpdatePrompt.svelte`): the check when the window opens and every few hours,
//! the offer, the download and the restart (on macOS, the opened disk image). What is decided (is
//! it newer, is it mandatory, is it signed) lives in `update.rs`.

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{
    Align, Button, Context, Id, Key, LayerId, Layout, Order, Response, RichText, Shape, Stroke,
    StrokeKind, Ui, ViewportCommand,
};

use crate::state::AppState;
use crate::theme::{Weight, font, tokens};
use crate::update::{self, Available};
use crate::widgets::{self, Btn, Tone};

/// A window left open still hears about a new build, soon after it is published.
const CHECK_EVERY: Duration = Duration::from_secs(5 * 60);
/// How long a build that was put off stays quiet, so the frequent checks do not nag.
const ASK_AGAIN_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
const RELEASES: &str = "https://github.com/contest-keeper/ck-forge/releases/tag/v";

/// What a worker thread reports.
enum Event {
    Found(Result<Option<Available>, String>),
    Progress(u64, u64),
    /// The download was checked and the installer started or the disk image opened.
    Started(Result<(), String>),
}

#[derive(Clone, Copy)]
enum State {
    /// Nothing to say: no newer build, no key, or the check has not answered.
    Quiet,
    Offer,
    Downloading(u64, u64),
    /// macOS: the disk image is open and the person takes it from here.
    Opened,
}

/// What a click in the dialog asks for.
#[derive(Clone, Copy)]
enum Act {
    Notes,
    Skip,
    Later,
    Install,
    Reopen,
    Back,
    Close,
}

pub struct Banner {
    state: State,
    /// The update that was offered, kept while it downloads or after it failed: a mandatory one
    /// keeps the window blocked and any one can be tried again.
    pending: Option<Available>,
    /// Why the last try failed.
    failure: String,
    /// The profile could not be saved: pressing the button again updates and drops the edits.
    unsaved: bool,
    /// The dialog was on screen in the last frame.
    shown: bool,
    next_check: Option<Instant>,
    /// The version that was put off, and when it may be offered again.
    put_off: Option<(String, Instant)>,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

impl Default for Banner {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            state: State::Quiet,
            pending: None,
            failure: String::new(),
            unsaved: false,
            shown: false,
            next_check: Some(Instant::now()),
            put_off: None,
            events,
            sender,
        }
    }
}

impl Banner {
    /// A banner that never asks the feed, for the tests and screenshots.
    #[cfg(test)]
    pub fn quiet() -> Self {
        Self {
            next_check: None,
            ..Self::default()
        }
    }

    /// A banner that offers a sample build and never asks the feed. Nothing may press Install.
    #[cfg(test)]
    pub fn offering(mandatory: bool) -> Self {
        let minimum = if mandatory { "0.5.0" } else { "0.1.0" };
        let feed = format!(
            r#"{{"version":"0.5.0","min_version":"{minimum}","platforms":{{"windows-x86_64":
            {{"signature":"c2ln","url":"https://example.invalid/setup.exe"}}}}}}"#
        );
        let found = update::newer_in(&feed, "0.4.0", "windows").expect("sample feed");
        Self {
            state: State::Offer,
            pending: found,
            ..Self::quiet()
        }
    }

    /// Whether a mandatory update is about to be on screen for the first time.
    pub fn appearing(&self) -> bool {
        !self.shown
            && !matches!(self.state, State::Quiet)
            && self.pending.as_ref().is_some_and(|u| u.mandatory)
    }

    /// Starts a check when one is due and takes what the worker threads reported. Closes the window
    /// when an update was started. `data` is the Forge folder, which remembers a skipped version.
    pub fn poll(&mut self, ctx: &Context, data: Option<&Path>) {
        if let Some(due) = self.next_check {
            let now = Instant::now();
            let mut wait = due.saturating_duration_since(now);
            if wait.is_zero() {
                self.next_check = Some(now + CHECK_EVERY);
                self.check(ctx.clone());
                wait = CHECK_EVERY;
            }
            // Asked every frame: egui forgets the request once the window repaints for another reason.
            ctx.request_repaint_after(wait);
        }
        while let Ok(event) = self.events.try_recv() {
            self.receive(ctx, data, event);
        }
    }

    fn receive(&mut self, ctx: &Context, data: Option<&Path>, event: Event) {
        match event {
            // A later check never disturbs an install that is under way.
            Event::Found(Ok(Some(update))) if matches!(self.state, State::Quiet | State::Offer) => {
                let skipped = data.and_then(update::skipped_version);
                let waiting = !update.mandatory
                    && self.put_off.as_ref().is_some_and(|(version, until)| {
                        *version == update.version && Instant::now() < *until
                    });
                if !waiting && update::worth_offering(&update, skipped.as_deref()) {
                    self.pending = Some(update);
                    self.state = State::Offer;
                }
            }
            // Offline or no feed yet: not worth bothering the admin, the next check tries again.
            Event::Found(_) => {}
            Event::Progress(done, total) => self.state = State::Downloading(done, total),
            Event::Started(Ok(())) if cfg!(target_os = "macos") => self.state = State::Opened,
            Event::Started(Ok(())) => ctx.send_viewport_cmd(ViewportCommand::Close),
            Event::Started(Err(why)) => {
                self.state = State::Offer;
                self.failure = why;
            }
        }
    }

    /// The dialog over the window, while there is something to say. It is drawn every frame, so a
    /// mandatory update keeps everything behind it out of reach.
    pub fn show(&mut self, ctx: &Context, app: &mut AppState) {
        let (Some(update), false) = (self.pending.clone(), matches!(self.state, State::Quiet))
        else {
            self.shown = false;
            return;
        };
        let first = !std::mem::replace(&mut self.shown, true);
        // Read before the dialog, which takes the key away. A click beside it does nothing.
        let escape = ctx.input(|input| input.key_pressed(Key::Escape));
        let (act, _) = widgets::dialog(ctx, "update", 460.0, |ui| {
            self.contents(ui, &update, app.busy, first)
        });
        // A page's own dialog that opens later must not end up over a mandatory update.
        ctx.move_to_top(LayerId::new(Order::Foreground, Id::unique("update")));
        let can_put_off = !matches!(self.state, State::Downloading(..));
        match act.or((escape && can_put_off).then_some(Act::Later)) {
            Some(Act::Notes) => {
                // The page names only the version, which was read as a version number.
                let _ = open::that(format!("{RELEASES}{}", update.version));
            }
            Some(Act::Skip) => {
                if !update.mandatory {
                    if let Some(folder) = app.data() {
                        let _ = update::remember_skipped(folder, &update.version);
                    }
                    self.later();
                }
            }
            Some(Act::Later) => self.later(),
            Some(Act::Install) => {
                if !app.busy && self.ready(app) {
                    self.download(update, ctx.clone());
                }
            }
            // Downloaded and checked again: the file left on disk is not trusted a second time.
            Some(Act::Reopen) => self.download(update, ctx.clone()),
            Some(Act::Back) => self.state = State::Offer,
            // Closing would cut off what is running.
            Some(Act::Close) if !app.busy => ctx.send_viewport_cmd(ViewportCommand::Close),
            None | Some(Act::Close) => {}
        }
    }

    /// Puts the update off until the next check. A mandatory one cannot be put off.
    fn later(&mut self) {
        if self.pending.as_ref().is_some_and(|u| u.mandatory) {
            return;
        }
        self.state = State::Quiet;
        self.put_off = self
            .pending
            .take()
            .map(|update| (update.version, Instant::now() + ASK_AGAIN_AFTER));
        self.failure.clear();
        self.unsaved = false;
    }

    /// Saves the profile, because the restart would drop what was typed. False when it cannot be
    /// saved and the admin has not yet been told; asking again goes on without saving.
    fn ready(&mut self, app: &mut AppState) -> bool {
        self.failure.clear();
        if app.profile.is_some()
            && !self.unsaved
            && let Err(why) = app.save_profile()
        {
            self.unsaved = true;
            self.failure = format!("{why}\n\nUpdating now loses these edits.");
            return false;
        }
        true
    }

    fn contents(&self, ui: &mut Ui, update: &Available, busy: bool, first: bool) -> Option<Act> {
        let t = tokens(ui.ctx());
        let mut act = None;
        let words = |ui: &mut Ui, text: &str| {
            widgets::paragraph(ui, text, Weight::Regular, t.s.rem, t.c.text, 1.45);
        };
        widgets::paragraph(
            ui,
            &format!("Keeper Forge {} is available", update.version),
            Weight::Bold,
            t.rem(1.05),
            t.c.text,
            1.35,
        );
        if matches!(self.state, State::Opened) {
            words(
                ui,
                "The disk image is open. Close Keeper Forge, then drag the new Keeper Forge to Applications and replace the old one.",
            );
        } else if update.mandatory {
            words(
                ui,
                "This version is no longer supported. Update to continue.",
            );
        } else if cfg!(target_os = "macos") {
            words(
                ui,
                "A new version is available. Would you like to download it and open the disk image?",
            );
        } else {
            words(
                ui,
                "A new version is available. Would you like to relaunch with the updated version?",
            );
        }
        let notes = link(ui, "What's new");
        if first {
            notes.request_focus();
        }
        if notes.clicked() {
            act = Some(Act::Notes);
        }
        widgets::note(ui, Tone::Error, &self.failure);
        if busy {
            words(ui, "Something is still running. Wait for it to finish.");
        }
        if let State::Downloading(done, total) = self.state {
            const MB: u64 = 1024 * 1024;
            if total == 0 {
                words(ui, &format!("Downloading the update: {} MB", done / MB));
                ui.spinner();
            } else {
                widgets::split_line(
                    ui,
                    "Downloading the update",
                    &format!("{} of {} MB", done / MB, total / MB),
                );
                // Only a picture of the numbers above.
                #[allow(clippy::cast_precision_loss)]
                widgets::progress_bar(ui, done as f32 / total as f32);
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // The skip link sits at the left, the buttons of the earlier window at the right.
            if matches!(self.state, State::Offer)
                && !update.mandatory
                && link(ui, "Skip this version").clicked()
            {
                act = Some(Act::Skip);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if let Some(clicked) = self.buttons(ui, update.mandatory, busy) {
                    act = Some(clicked);
                }
            });
        });
        act
    }

    /// The buttons, from the right.
    fn buttons(&self, ui: &mut Ui, mandatory: bool, busy: bool) -> Option<Act> {
        let mut act = None;
        let mut button = |ui: &mut Ui, button: Btn, what: Act| {
            if button.show(ui).clicked() {
                act = Some(what);
            }
        };
        if matches!(self.state, State::Opened) {
            let close = Btn::new("Close Keeper Forge").primary().enabled(!busy);
            button(ui, close, Act::Close);
            button(ui, Btn::new("Open again"), Act::Reopen);
            button(ui, Btn::new("Back"), Act::Back);
            return act;
        }
        let installing = matches!(self.state, State::Downloading(..));
        let label = if installing {
            "Updating\u{2026}"
        } else if self.unsaved {
            "Update without saving"
        } else if cfg!(target_os = "macos") {
            "Download and open"
        } else {
            "Update and restart"
        };
        let ready = !installing && !busy;
        button(ui, Btn::new(label).primary().enabled(ready), Act::Install);
        if mandatory {
            button(ui, Btn::new("Close").enabled(ready), Act::Close);
        } else {
            button(ui, Btn::new("Later").enabled(!installing), Act::Later);
        }
        act
    }

    /// Checks in the background, so a slow or missing network never delays the window.
    fn check(&self, ctx: Context) {
        if !update::enabled() {
            return;
        }
        let sender = self.sender.clone();
        thread::spawn(move || {
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Found(update::check()));
            ctx.request_repaint();
        });
    }

    fn download(&mut self, update: Available, ctx: Context) {
        self.state = State::Downloading(0, 0);
        self.failure.clear();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let folder = std::env::temp_dir().join(format!("ck-update-{}", std::process::id()));
            let result = update::download(&update, &folder, |done, total| {
                let _ = sender.send(Event::Progress(done, total));
                ctx.request_repaint();
            })
            .and_then(|file| update::install(&file));
            let _ = sender.send(Event::Started(result));
            ctx.request_repaint();
        });
    }
}

/// A button that looks like a link (`.link`).
fn link(ui: &mut Ui, text: &str) -> Response {
    let t = tokens(ui.ctx());
    let text = RichText::new(text)
        .font(font(Weight::Regular, t.s.rem))
        .color(t.c.accent)
        .underline();
    let response = ui.add(Button::new(text).frame(false));
    if response.has_focus() && t.classic() {
        // The dotted ring of the classic look.
        let r = response.rect;
        let corners = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        ui.painter()
            .extend(Shape::dotted_line(&corners, t.c.text, 3.0, 0.7));
    } else if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            4,
            Stroke::new(2.0, t.c.accent),
            StrokeKind::Outside,
        );
    }
    response.on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use eframe::egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;
    use crate::app::{CONTEST, Forge, MAKE};

    /// A window that saves into `data`, so that leaving the profile works.
    fn window(data: &Path, update: Banner) -> Harness<'static, Forge> {
        let mut forge = Forge::sample();
        forge.state = AppState::load(Some(data.to_owned()));
        forge.update = update;
        let mut harness = Harness::builder()
            .with_size([1000.0, 760.0])
            .with_accessibility_check(false)
            .build_ui_state(|ui, forge: &mut Forge| forge.draw(ui), forge);
        harness.run_steps(4);
        harness
    }

    /// Clicks the second step in the sidebar and presses it from the keyboard.
    fn try_to_leave(harness: &mut Harness<'static, Forge>) {
        let step = harness.get_by_role_and_label(Role::Button, "Make sticks");
        step.click();
        harness.run_steps(3);
        harness
            .get_by_role_and_label(Role::Button, "Make sticks")
            .focus();
        harness.run_steps(2);
        harness.key_press(Key::Enter);
        harness.run_steps(3);
    }

    #[test]
    fn a_mandatory_update_keeps_the_window_out_of_reach_and_cannot_be_put_off() {
        let data = tempfile::tempdir().expect("folder");
        // The same actions do reach the window when nothing is in the way.
        let mut open = window(data.path(), Banner::quiet());
        try_to_leave(&mut open);
        assert_eq!(open.state().step, MAKE);

        let mut blocked = window(data.path(), Banner::offering(true));
        try_to_leave(&mut blocked);
        assert_eq!(blocked.state().step, CONTEST);
        blocked.key_press(Key::Escape);
        blocked.run_steps(3);
        assert!(blocked.query_by_label("Later").is_none());
        assert!(blocked.query_by_label("Skip this version").is_none());
        assert!(
            blocked.query_by_label("Update and restart").is_some() || cfg!(target_os = "macos")
        );
    }

    #[test]
    fn a_click_in_the_frame_that_brings_a_mandatory_update_does_nothing() {
        let data = tempfile::tempdir().expect("folder");
        let mut harness = window(data.path(), Banner::quiet());
        harness
            .get_by_role_and_label(Role::Button, "Make sticks")
            .click();
        harness.state_mut().update = Banner::offering(true);
        harness.run_steps(4);
        assert_eq!(harness.state().step, CONTEST);
    }

    #[test]
    fn close_waits_for_running_work() {
        let mut app = AppState::load(None);
        app.busy = true;
        let mut banner = Banner::offering(true);
        let mut frame = 0;
        let mut harness = Harness::builder()
            .with_accessibility_check(false)
            .build_ui(|ui| {
                crate::theme::apply(ui.ctx(), tokens(ui.ctx()).look);
                // The fonts arrive with the second frame.
                frame += 1;
                if frame > 1 {
                    banner.show(ui.ctx(), &mut app);
                }
            });
        harness.run_steps(4);
        assert!(harness.get_by_label("Close").accesskit_node().is_disabled());
    }

    #[test]
    fn a_build_that_was_put_off_stays_quiet_until_its_time_or_a_newer_one() {
        let offer = || Banner::offering(false).pending.expect("sample");
        let mut banner = Banner::offering(false);
        let found = |banner: &mut Banner, update: Available| {
            banner.receive(&Context::default(), None, Event::Found(Ok(Some(update))));
        };
        banner.later();
        found(&mut banner, offer());
        assert!(matches!(banner.state, State::Quiet));

        let mut newer = offer();
        newer.version = "0.6.0".to_owned();
        found(&mut banner, newer);
        assert!(matches!(banner.state, State::Offer));

        banner.later();
        banner.put_off = Some(("0.5.0".to_owned(), Instant::now()));
        found(&mut banner, offer());
        assert!(matches!(banner.state, State::Offer));
    }

    #[test]
    fn escape_puts_an_optional_update_off_and_skip_remembers_it_in_the_forge_folder() {
        let data = tempfile::tempdir().expect("folder");
        let mut harness = window(data.path(), Banner::offering(false));
        harness.key_press(Key::Escape);
        harness.run_steps(3);
        assert!(harness.query_by_label("Later").is_none());
        assert_eq!(update::skipped_version(data.path()), None);

        let mut harness = window(data.path(), Banner::offering(false));
        harness.get_by_label("Skip this version").click();
        harness.run_steps(3);
        assert!(harness.query_by_label("Later").is_none());
        assert_eq!(
            update::skipped_version(data.path()).as_deref(),
            Some("0.5.0")
        );
        // The next check stays quiet about it.
        let found = Banner::offering(false).pending;
        let forge = harness.state_mut();
        forge.update.receive(
            &Context::default(),
            Some(data.path()),
            Event::Found(Ok(found)),
        );
        assert!(matches!(forge.update.state, State::Quiet));
    }

    #[test]
    fn a_profile_that_cannot_be_saved_asks_before_it_is_lost() {
        let data = tempfile::tempdir().expect("folder");
        let mut app = AppState::load(Some(data.path().to_owned()));
        app.blocked = Some("fix the contest end".to_owned());
        let mut banner = Banner::offering(false);
        assert!(!banner.ready(&mut app));
        assert!(banner.failure.contains("loses these edits"));
        assert!(banner.ready(&mut app));
        assert!(app.profile_path.is_none());

        app.blocked = None;
        let mut banner = Banner::offering(false);
        assert!(banner.ready(&mut app));
        assert!(app.profile_path.is_some() && !app.is_unsaved());
    }

    #[test]
    fn a_failed_install_stays_offered_and_a_later_check_leaves_a_download_alone() {
        let ctx = Context::default();
        let mut banner = Banner::offering(true);
        banner.state = State::Downloading(1, 2);
        let found = Banner::offering(false).pending;
        banner.receive(&ctx, None, Event::Found(Ok(found)));
        assert!(matches!(banner.state, State::Downloading(..)));
        assert!(banner.pending.as_ref().is_some_and(|u| u.mandatory));
        banner.receive(&ctx, None, Event::Started(Err("no".to_owned())));
        assert!(matches!(banner.state, State::Offer));
        assert_eq!(banner.failure, "no");
        banner.later();
        assert!(banner.pending.is_some());
    }

    #[test]
    #[ignore = "writes pictures; needs CK_SHOTS_DIR and a graphics adapter"]
    fn shots_of_the_update_dialog() {
        use crate::shots::capture;
        capture("update-offer", 0.0, |forge| {
            forge.update = Banner::offering(false);
        });
        capture("update-mandatory", 0.0, |forge| {
            forge.update = Banner::offering(true);
        });
        capture("update-downloading", 0.0, |forge| {
            forge.update = Banner::offering(false);
            forge.update.state = State::Downloading(21 << 20, 48 << 20);
        });
        capture("update-failed", 0.0, |forge| {
            forge.update = Banner::offering(false);
            forge.update.failure =
                "could not download the update: the connection was reset".to_owned();
        });
        capture("update-unsaved", 0.0, |forge| {
            forge.update = Banner::offering(true);
            forge.update.unsaved = true;
            forge.update.failure =
                "the profile has problems:\n- bookmark 1 needs a title\n\nUpdating now loses these edits."
                    .to_owned();
        });
        capture("update-opened", 0.0, |forge| {
            forge.update = Banner::offering(false);
            forge.update.state = State::Opened;
        });
    }
}
