//! Steps 2 and 3, the sticks and the write: one image for each stick, written to the ticked disks by
//! `ck-forge stick write` started with administrator rights (the permission prompt of Windows or
//! macOS). The window stays unelevated, shows the helper's progress and can cancel. On other
//! systems it shows the command to run in a terminal. The rules are those of `MakePage.svelte` of the earlier window.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Instant;

use ck_admin_core::{
    HUB_URL, HardwareId, HubStick, list_sticks, requires_signed_settings, supports_enrolment, vault,
};
use ck_protocol::WifiPassword;
use ck_writer::{Disk, list_disks};
use eframe::egui::{Align, Context, TextFormat, Ui, text::LayoutJob, vec2};

use crate::make::format::{self, erase_confirmation, stick_label};
use crate::make::progress::Bar;
use crate::make::stick_job::{Event as JobEvent, Request, quote_posix, run};
use crate::make::sticks::{Choice, confirm_retire};
use crate::theme::{Weight, font, tokens};
use crate::widgets::{self, Btn, Input, Tone};

/// What a worker thread reports. The number tells which listing the answer belongs to.
enum Event {
    /// The size of the base image and the disks of this computer.
    Disks(u64, Result<(u64, Vec<Disk>), String>),
    /// What a base image supports, asked while signed in or not.
    Support(PathBuf, bool, Support),
    /// The Hub's sticks, asked as this account.
    Earlier(u64, String, Result<Vec<HubStick>, String>),
}

/// What the base image asks of a write.
#[derive(Clone, Copy, Default)]
pub(super) struct Support {
    /// The image only accepts settings the Hub signed.
    pub(super) signature: bool,
    /// Whether it can make each stick's key at first boot; unknown until it was asked, which is
    /// only done while signed in.
    pub(super) enrolment: Option<bool>,
}

/// Whether it is known which sticks the listed disks held before.
enum Lookup {
    /// Known, not to be known (signed out, no serial numbers), or the admin goes on without it.
    Settled,
    /// The Hub is being asked. Nothing is erased meanwhile, or the question about old keys would
    /// be skipped.
    Pending,
    Failed(String),
}

/// Asks the Hub for its sticks.
fn hub_sticks() -> Result<Vec<HubStick>, String> {
    let session = vault::load().ok_or_else(|| "sign in to the Hub first".to_owned())?;
    list_sticks(HUB_URL, &session).map_err(|e| e.to_string())
}

/// Where a write goes.
enum Target {
    /// The disks to erase, each with its identity token as it was listed.
    Disks(Vec<(u32, String)>),
    /// A file: a test without a stick.
    File(PathBuf),
}

/// A write that waits for the answer about the old keys. What it erases and replaces is taken when
/// the button is pressed, so the question names exactly what the write then does.
pub(super) struct Asking {
    disks: Vec<(u32, String)>,
    before: Vec<HubStick>,
}

// The checkboxes and flags of a window are plain state, not a state machine.
#[allow(clippy::struct_excessive_bools)]
pub struct Screen {
    pub(super) disks: Vec<Disk>,
    /// The size of the base image the disks were listed for.
    pub(super) size: u64,
    /// Whether the disks are being listed, which can take seconds.
    listing: bool,
    /// True after "Find sticks" completed so an empty USB list can show an empty-state message.
    has_listed: bool,
    /// Goes up whenever the disks are forgotten, so a late answer is dropped.
    generation: u64,
    /// The base image the sticks are made from.
    pub(super) base: Option<PathBuf>,
    /// The saved profile the sticks get their settings from.
    pub(super) profile: Option<PathBuf>,
    /// The signed-in admin.
    pub(super) email: Option<String>,
    pub(super) support: Support,
    allow_virtual: bool,
    /// The Wi-Fi password, typed for this write only. It is never saved, logged or shown.
    wifi_password: String,
    /// A file to write to instead of a disk.
    to_file: Option<PathBuf>,
    /// The disks the admin ticked.
    pub(super) chosen: BTreeSet<u32>,
    /// What the admin typed to confirm the erase.
    pub(super) typed: String,
    /// What the Hub knows about each disk, from the sticks written to it before.
    pub(super) earlier: BTreeMap<u32, HubStick>,
    lookup: Lookup,
    /// How the lookup asks; tests never reach the credential vault or the Hub.
    hub_sticks: fn() -> Result<Vec<HubStick>, String>,
    /// The profile and network the Wi-Fi password was typed for.
    password_for: Option<(Option<PathBuf>, String)>,
    asking: Option<Asking>,
    /// The image to write on a system where the window cannot write.
    image: Option<PathBuf>,
    /// The chosen disk number on such a system, as typed.
    disk: String,
    pub(super) failure: String,
    /// What the last write that worked reported.
    pub(super) success: String,
    job: Option<Job>,
    /// A write succeeded in this run.
    pub written: bool,
    /// A write that worked, until the page takes it: the sticks it replaced, and whether to retire
    /// their keys.
    finished: Option<Finished>,
    events: Receiver<Event>,
    sender: Sender<Event>,
}

/// The sticks a write replaced, whether to retire their keys, and the account it was started in.
pub type Finished = (Vec<HubStick>, bool, Option<String>);

/// A write that is running on its own thread.
struct Job {
    cancel: Arc<AtomicBool>,
    bar: Bar,
    events: Receiver<JobEvent>,
    before: Vec<HubStick>,
    retire: bool,
    account: Option<String>,
    /// How many sticks it writes; none for a test file.
    disks: usize,
    started: Instant,
}

impl Default for Screen {
    fn default() -> Self {
        let (sender, events) = channel();
        Self {
            disks: Vec::new(),
            size: 0,
            listing: false,
            has_listed: false,
            generation: 0,
            base: None,
            profile: None,
            email: None,
            support: Support::default(),
            allow_virtual: false,
            wifi_password: String::new(),
            to_file: None,
            chosen: BTreeSet::new(),
            typed: String::new(),
            earlier: BTreeMap::new(),
            lookup: Lookup::Settled,
            hub_sticks: if cfg!(test) {
                || Err("no Hub in tests".to_owned())
            } else {
                hub_sticks
            },
            password_for: None,
            asking: None,
            image: None,
            disk: String::new(),
            failure: String::new(),
            success: String::new(),
            job: None,
            written: false,
            finished: None,
            events,
            sender,
        }
    }
}

/// Whether the window writes the sticks itself, through the permission prompt of the system.
pub(super) const CAN_WRITE: bool = cfg!(any(windows, target_os = "macos"));

/// The command that writes `image` to disk `number` in a terminal, where the window cannot do it.
fn command_line(image: &str, number: &str) -> String {
    format!(
        "sudo ck-forge stick write {} --disk {}",
        quote_posix(image.trim()),
        number.trim()
    )
}

/// The disks that held a stick the Hub still knows as working.
fn matched(disks: &[Disk], sticks: &[HubStick]) -> BTreeMap<u32, HubStick> {
    let mut known = BTreeMap::new();
    for disk in disks {
        let Some(hardware) = HardwareId::of(disk) else {
            continue;
        };
        // The Hub shows the first 8 digits of the fingerprint.
        let id: String = hardware.fingerprint.chars().take(8).collect();
        let stick = sticks.iter().find(|stick| {
            stick.hardware.as_ref().is_some_and(|on| on.id == id)
                && stick.retired_at.is_none()
                && stick.archived_at.is_none()
        });
        if let Some(stick) = stick {
            known.insert(disk.number, stick.clone());
        }
    }
    known
}

fn disk_word(count: usize) -> &'static str {
    if count == 1 { "disk" } else { "disks" }
}

impl Screen {
    pub fn writing(&self) -> bool {
        self.job.is_some()
    }

    /// The write that just worked, if one did.
    pub fn take_finished(&mut self) -> Option<Finished> {
        self.finished.take()
    }

    /// Drops the listed disks and everything said about them. A new listing may put another disk at
    /// a number, so no tick and no typed confirmation may outlive the list it was made on.
    fn forget_disks(&mut self) {
        self.generation += 1;
        self.listing = false;
        self.has_listed = false;
        self.disks.clear();
        self.chosen.clear();
        self.typed.clear();
        self.earlier.clear();
        self.lookup = Lookup::Settled;
        self.asking = None;
    }

    /// The base image was chosen or made again, perhaps at the same place.
    pub fn base_replaced(&mut self) {
        self.forget_disks();
        self.base = None;
    }

    /// Takes in what the shared state has now.
    pub fn follow(
        &mut self,
        base: Option<&Path>,
        profile: Option<&Path>,
        email: Option<&str>,
        ctx: &Context,
    ) {
        self.profile = profile.map(Path::to_owned);
        let mut ask = false;
        if self.base.as_deref() != base {
            // The disks were checked against the size of the old image.
            self.forget_disks();
            self.base = base.map(Path::to_owned);
            ask = true;
        }
        if self.email.as_deref() != email {
            // What one account's Hub said is not for another.
            self.email = email.map(str::to_owned);
            self.earlier.clear();
            self.asking = None;
            self.look_up_earlier(ctx.clone());
            ask = true;
        }
        if ask {
            self.ask_support(ctx.clone());
        }
    }

    /// Asks the base image what it supports; reading it can take a while.
    fn ask_support(&mut self, ctx: Context) {
        self.support = Support::default();
        let Some(base) = self.base.clone() else {
            return;
        };
        let signed_in = self.email.is_some();
        let sender = self.sender.clone();
        thread::spawn(move || {
            // An image that cannot be checked here is checked again when the sticks are built, and
            // one that cannot be asked about enrolment uses the credential flow.
            let support = Support {
                signature: requires_signed_settings(&base).unwrap_or(false),
                enrolment: signed_in.then(|| supports_enrolment(&base).unwrap_or(false)),
            };
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Support(base, signed_in, support));
            ctx.request_repaint();
        });
    }

    fn list(&mut self, ctx: Context) {
        self.forget_disks();
        self.failure.clear();
        self.listing = true;
        let generation = self.generation;
        let base = self.base.clone();
        let sender = self.sender.clone();
        thread::spawn(move || {
            // Without the size of the image a stick that is too small would be offered, so an
            // image that cannot be read lists nothing.
            let size = base.map_or(Ok(0), |base| {
                ck_writer::thin::image_len(&base).map_err(|e| e.to_string())
            });
            let result = size.and_then(|size| {
                let disks = list_disks().map_err(|e| e.to_string())?;
                Ok((size, disks))
            });
            let _ = sender.send(Event::Disks(generation, result));
            ctx.request_repaint();
        });
    }

    /// Asks the Hub which sticks the listed disks held, when somebody is signed in to ask.
    fn look_up_earlier(&mut self, ctx: Context) {
        self.lookup = Lookup::Settled;
        let Some(email) = self.email.clone() else {
            return;
        };
        if !self.disks.iter().any(|disk| HardwareId::of(disk).is_some()) {
            return;
        }
        self.lookup = Lookup::Pending;
        let generation = self.generation;
        let ask = self.hub_sticks;
        let sender = self.sender.clone();
        thread::spawn(move || {
            // The window may be gone by now; then nobody needs the answer.
            let _ = sender.send(Event::Earlier(generation, email, ask()));
            ctx.request_repaint();
        });
    }

    /// Takes what the worker threads reported.
    pub fn receive(&mut self, ctx: &Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Disks(generation, result) if generation == self.generation => {
                    self.listing = false;
                    match result {
                        Ok((size, disks)) => {
                            self.size = size;
                            self.disks = disks;
                            self.has_listed = true;
                            self.look_up_earlier(ctx.clone());
                        }
                        Err(why) => self.failure = why,
                    }
                }
                Event::Support(base, signed_in, support)
                    if self.base.as_ref() == Some(&base) && signed_in == self.email.is_some() =>
                {
                    self.support = support;
                }
                Event::Earlier(generation, email, result)
                    if generation == self.generation && self.email.as_ref() == Some(&email) =>
                {
                    match result {
                        Ok(sticks) => {
                            self.earlier = matched(&self.disks, &sticks);
                            self.lookup = Lookup::Settled;
                        }
                        Err(why) => self.lookup = Lookup::Failed(why),
                    }
                }
                // An answer about disks, an image or an account that is no longer the one shown.
                Event::Disks(..) | Event::Support(..) | Event::Earlier(..) => {}
            }
        }
        self.receive_job(ctx);
    }

    fn receive_job(&mut self, ctx: &Context) {
        let Some(job) = &mut self.job else {
            return;
        };
        let mut ended = None;
        while let Ok(event) = job.events.try_recv() {
            match event {
                JobEvent::Progress(progress) => job.bar.update(progress, Instant::now()),
                JobEvent::Done(result) => ended = Some(result),
            }
        }
        let Some(result) = ended else {
            return;
        };
        let Some(job) = self.job.take() else {
            return;
        };
        self.typed.clear();
        self.wifi_password.clear();
        let took = format::duration(job.started.elapsed().as_secs());
        match &result {
            Ok(hash) => {
                let what = match job.disks {
                    0 => "into the test file (no stick was written)".to_owned(),
                    1 => "on the stick".to_owned(),
                    count => format!("on {count} sticks"),
                };
                self.success = format!(
                    "Written and read back {what} in {took}. The check value (SHA-256) is {hash}."
                );
            }
            Err(why) => self.failure.clone_from(why),
        }
        // A test file replaces no stick, so it completes nothing and retires nothing.
        if job.disks == 0 {
            return;
        }
        // The Hub may point at new sticks now, even after a write that partly failed.
        self.generation += 1;
        self.earlier.clear();
        self.look_up_earlier(ctx.clone());
        if result.is_ok() {
            self.written = true;
            // Only here: the old keys are needed until the new sticks are written and read back.
            self.finished = Some((job.before, job.retire, job.account));
        }
    }

    /// Unticks every disk that may not be written, such as a virtual one after its box is cleared.
    fn prune(&mut self) {
        let (disks, size, allow) = (&self.disks, self.size, self.allow_virtual);
        self.chosen.retain(|number| {
            disks
                .iter()
                .any(|disk| disk.number == *number && disk.check(size, allow).is_ok())
        });
    }

    fn ready(&self) -> bool {
        self.base.is_some() && self.profile.is_some()
    }

    fn must_sign_in(&self) -> bool {
        self.support.signature && self.email.is_none()
    }

    fn can_write(&self, locked: bool) -> bool {
        self.ready() && !self.must_sign_in() && self.job.is_none() && !locked
    }

    /// Whether the admin typed the confirmation for exactly the disks that are ticked.
    fn confirmed(&self) -> bool {
        !self.chosen.is_empty() && self.typed.trim() == erase_confirmation(&self.chosen)
    }

    /// Whether the disks may be erased now: confirmed, and the old keys are known.
    fn can_erase(&self, locked: bool) -> bool {
        self.can_write(locked) && self.confirmed() && matches!(self.lookup, Lookup::Settled)
    }

    /// The write button. When it would replace sticks the Hub knows, it asks first.
    fn press(&mut self, ctx: &Context) {
        self.prune();
        // Nothing is erased without the typed confirmation, whatever called this.
        if !self.can_erase(false) {
            return;
        }
        // What the admin saw when confirming, as listed: the helper erases only these.
        let disks = self
            .disks
            .iter()
            .filter(|disk| self.chosen.contains(&disk.number))
            .map(|disk| (disk.number, disk.identity_token()))
            .collect();
        let before: Vec<HubStick> = self
            .chosen
            .iter()
            .filter_map(|number| self.earlier.get(number).cloned())
            .collect();
        if before.is_empty() {
            self.start(Target::Disks(disks), before, false, ctx);
        } else {
            self.asking = Some(Asking { disks, before });
        }
    }

    fn decided(&mut self, choice: Choice, ctx: &Context) {
        let Some(asking) = self.asking.take() else {
            return;
        };
        if choice != Choice::Cancel {
            let retire = choice == Choice::Retire;
            self.start(Target::Disks(asking.disks), asking.before, retire, ctx);
        }
    }

    /// What the job is asked to do. The Wi-Fi password is moved into it and the field is emptied.
    fn request(&mut self, target: Target) -> Option<Request> {
        let (base, profile) = (self.base.clone()?, self.profile.clone()?);
        let (confirmed, to_file) = match target {
            Target::Disks(disks) => (disks, None),
            Target::File(file) => (Vec::new(), Some(file)),
        };
        let password = std::mem::take(&mut self.wifi_password);
        Some(Request {
            base,
            profile,
            disks: confirmed.iter().map(|(number, _)| *number).collect(),
            confirmed,
            to_file,
            allow_virtual: self.allow_virtual,
            wifi_password: (!password.is_empty()).then(|| WifiPassword::new(password)),
            first_boot_enrolment: self.support.enrolment == Some(true),
        })
    }

    /// Starts the write on its own thread.
    fn start(&mut self, target: Target, before: Vec<HubStick>, retire: bool, ctx: &Context) {
        let Some(request) = self.request(target) else {
            "Choose the base image and save the profile first".clone_into(&mut self.failure);
            return;
        };
        let disks = request.disks.len();
        let (sender, events) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let ctx = ctx.clone();
        thread::spawn(move || run(&request, &flag, &sender, &|| ctx.request_repaint()));
        self.failure.clear();
        self.success.clear();
        self.job = Some(Job {
            cancel,
            bar: Bar::default(),
            events,
            before,
            retire,
            account: self.email.clone(),
            disks,
            started: Instant::now(),
        });
    }

    /// Step 2. `network` is the Wi-Fi the profile joins, if it names one.
    pub fn sticks_ui(&mut self, ui: &mut Ui, network: Option<&str>, locked: bool, dev: bool) {
        self.prune();
        widgets::step_head(
            ui,
            2,
            !self.chosen.is_empty(),
            "Sticks to write",
            "Only removable USB sticks are listed.",
        );
        let idle = !locked && self.job.is_none();
        self.bind_password(network);
        if let Some(network) = network {
            self.wifi_ui(ui, network, idle);
        }
        self.key_ui(ui);
        // Where the window cannot write, the list is only for reading the numbers.
        let may_list = self.ready() || !CAN_WRITE;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = tokens(ui.ctx()).s.gap;
            let find = Btn::new("Find sticks").small();
            let enabled = idle && may_list && !self.listing;
            if find.enabled(enabled).show(ui).clicked() {
                self.list(ui.ctx().clone());
            }
            // Only on a development machine, switched on by hand in `window.json`.
            if dev {
                ui.add_enabled_ui(idle, |ui| {
                    widgets::check(
                        ui,
                        &mut self.allow_virtual,
                        "Also show virtual disks (for testing)",
                    );
                });
            }
        });
        if !self.ready() {
            widgets::hint(
                ui,
                "Choose a base image first, so Forge can check that the sticks are big enough.",
            );
        }
        if self.listing {
            widgets::hint(ui, "Looking for disks\u{2026}");
        }
        self.disks_ui(ui, idle && CAN_WRITE);
    }

    /// Empties the password when the profile or its network is not the one it was typed for, so one
    /// network's password is never written for another.
    fn bind_password(&mut self, network: Option<&str>) {
        let now = network.map(|network| (self.profile.clone(), network.to_owned()));
        if self.password_for != now {
            self.wifi_password.clear();
            self.password_for = now;
        }
    }

    /// While the old keys are not known: that the Hub is asked, or why it could not be.
    fn lookup_ui(&mut self, ui: &mut Ui) {
        match &self.lookup {
            Lookup::Settled => {}
            Lookup::Pending => {
                widgets::hint(
                    ui,
                    "Checking which sticks these disks held before. Writing waits for the Hub.",
                );
            }
            Lookup::Failed(why) => {
                let text =
                    format!("Forge could not check which sticks these disks held before: {why}");
                widgets::note(ui, Tone::Error, &text);
                ui.horizontal(|ui| {
                    if Btn::new("Check again").small().show(ui).clicked() {
                        self.look_up_earlier(ui.ctx().clone());
                    }
                    let without = Btn::new("Continue without the check").small();
                    if without.show(ui).clicked() {
                        self.lookup = Lookup::Settled;
                    }
                });
            }
        }
    }

    fn wifi_ui(&mut self, ui: &mut Ui, network: &str, idle: bool) {
        ui.scope(|ui| {
            ui.set_max_width(520.0);
            widgets::field(
                ui,
                &format!("Wi-Fi password for \u{201c}{network}\u{201d}"),
                Some("Written to the sticks and kept encrypted in the Hub for this profile; never shown again. Leave empty to use the saved one."),
                None,
                |ui, label| {
                    ui.add_enabled_ui(idle, |ui| {
                        let input = Input::new(&mut self.wifi_password)
                            .password()
                            .placeholder("Leave empty to use the saved password")
                            .show(ui);
                        input.labelled_by(label);
                    });
                },
            );
        });
    }

    /// Key handling notes for the selected base image and sign-in state.
    fn key_ui(&self, ui: &mut Ui) {
        if self.must_sign_in() {
            widgets::note(
                ui,
                Tone::Error,
                "Sign in to the Hub (bottom left) — this image requires signed settings.",
            );
        } else if self.email.is_none() {
            widgets::note(
                ui,
                Tone::Warn,
                "Not signed in to the Hub. Sticks will use a static key on disk until you sign in.",
            );
        } else if self.support.enrolment == Some(false) && self.base.is_some() {
            widgets::note(
                ui,
                Tone::Warn,
                "This image stores keys on disk. Newer Keeper OS images activate on first boot.",
            );
        }
    }

    fn disks_ui(&mut self, ui: &mut Ui, pick: bool) {
        let mut shown = 0usize;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for disk in &self.disks {
                let refusal = disk.check(self.size, self.allow_virtual).err();
                // A protected disk (the system disk, a disk that is not a USB stick) is never
                // shown: only sticks that can be written, or are too small, are of use here.
                if matches!(&refusal, Some(why) if !matches!(why, ck_writer::Error::TooSmall { .. })) {
                    continue;
                }
                shown += 1;
                let serial = disk.serial_number.as_deref().unwrap_or("").trim();
                let mut detail = format!("{} \u{b7} {}", disk.size_text(), disk.bus_type);
                if !serial.is_empty() {
                    detail = format!("{detail} \u{b7} serial {serial}");
                }
                if let Some(why) = &refusal {
                    detail = format!("{detail} \u{b7} Not offered: {why}");
                }
                let before = self.earlier.get(&disk.number);
                let written =
                    before.map(|stick| format!("Written before as {}", stick_label(stick)));
                let mut below = vec![detail.as_str()];
                below.extend(written.as_deref());
                let name = disk.friendly_name.as_deref().filter(|n| !n.is_empty());
                let name = name.unwrap_or("unnamed");
                let name = format!("Disk {} \u{b7} {name}", disk.number);
                let (tone, state) = match &refusal {
                    Some(_) => (Tone::Warn, "Too small"),
                    None if before.is_some() => (Tone::Warn, "Rewrite"),
                    None => (Tone::Ok, "Safe to write"),
                };
                let mut ticked = self.chosen.contains(&disk.number);
                let changed = ui
                    .add_enabled_ui(pick, |ui| {
                        let refused = refusal.is_some();
                        widgets::disk_row(ui, &mut ticked, refused, &name, &below, |ui| {
                            widgets::pill(ui, tone, state);
                        })
                        .changed()
                    })
                    .inner;
                if changed && ticked {
                    self.chosen.insert(disk.number);
                } else if changed {
                    self.chosen.remove(&disk.number);
                }
            }
        });
        if shown == 0 && self.has_listed && !self.listing {
            widgets::hint(ui, "No removable USB sticks found.");
        }
    }

    /// Step 3. `profile` is the name of the profile.
    pub fn erase_ui(&mut self, ui: &mut Ui, profile: &str, locked: bool) {
        self.prune();
        widgets::step_head(
            ui,
            3,
            self.written && !self.success.is_empty(),
            "Erase and write",
            "Erases the chosen sticks and writes your saved profile.",
        );
        if !CAN_WRITE {
            self.command_ui(ui);
        } else if self.chosen.is_empty() {
            widgets::hint(ui, "Choose at least one stick above.");
        } else {
            self.summary_ui(ui, profile);
            widgets::danger_zone(ui, |ui| self.danger_ui(ui, locked));
            self.lookup_ui(ui);
        }
        self.job_ui(ui);
        widgets::note(ui, Tone::Error, &self.failure);
        widgets::note(ui, Tone::Ok, &self.success);
    }

    /// The ticked disk numbers, as "1, 2".
    fn names(&self) -> String {
        let numbers: Vec<String> = self.chosen.iter().map(ToString::to_string).collect();
        numbers.join(", ")
    }

    /// What will be written.
    fn summary_ui(&self, ui: &mut Ui, profile: &str) {
        let count = self.chosen.len();
        let sticks = format!("{count} ({} {})", disk_word(count), self.names());
        let base = self
            .base
            .as_deref()
            .map_or_else(|| "none chosen".to_owned(), format::file_name);
        let profile = if profile.is_empty() {
            "unnamed"
        } else {
            profile
        };
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let rows = [
                ("Sticks", sticks.as_str()),
                ("Base image", base.as_str()),
                ("Profile", profile),
            ];
            widgets::summary(ui, &rows);
        });
    }

    /// The warning, the box for the typed confirmation and the button that erases.
    fn danger_ui(&mut self, ui: &mut Ui, locked: bool) {
        let t = tokens(ui.ctx());
        let words = erase_confirmation(&self.chosen);
        let count = self.chosen.len();
        let title = format!(
            "Everything on {} {} will be erased",
            disk_word(count),
            self.names()
        );
        let text = match (&self.job, count) {
            (Some(_), _) => "Writing\u{2026}".to_owned(),
            (None, 1) => "Erase and write".to_owned(),
            (None, count) => format!("Erase and write {count} sticks"),
        };
        let idle = !locked && self.job.is_none();
        let enabled = self.can_erase(locked);
        let bold = font(Weight::Semibold, t.s.rem);
        let galley = ui.painter().layout_no_wrap(text.clone(), bold, t.c.text);
        // The button goes under the text when the three do not fit beside each other.
        let one_line = ui.available_width() >= 220.0 + 150.0 + galley.size().x + 72.0;
        let button = |ui: &mut Ui| {
            let button = Btn::new(&text).danger().enabled(enabled);
            button.show(ui).clicked()
        };
        let label = std::cell::Cell::new(None);
        let mut pressed = false;
        let typed = &mut self.typed;
        ui.spacing_mut().item_spacing = vec2(18.0, 14.0);
        widgets::row(
            ui,
            |ui| {
                widgets::paragraph(ui, &title, Weight::Bold, t.s.rem, t.c.text, 1.35);
                let part = |weight| TextFormat {
                    font_id: font(weight, t.s.hint),
                    color: t.c.danger_text,
                    valign: Align::Center,
                    ..TextFormat::default()
                };
                let mut job = LayoutJob::default();
                job.append("This cannot be undone. Type ", 0.0, part(Weight::Regular));
                job.append(&words, 0.0, part(Weight::Bold));
                job.append(" to confirm.", 0.0, part(Weight::Regular));
                job.wrap.max_width = ui.available_width();
                label.set(Some(ui.label(job).id));
            },
            |ui| {
                if one_line {
                    pressed = button(ui);
                }
                ui.add_enabled_ui(idle, |ui| {
                    let input = Input::new(typed).mono().width(150.0).show(ui);
                    if let Some(label) = label.get() {
                        input.labelled_by(label);
                    }
                });
            },
        );
        if !one_line {
            pressed = button(ui);
        }
        if pressed {
            self.press(ui.ctx());
        }
    }

    /// The write that is running: the warning, how far it is and the way to stop it.
    fn job_ui(&self, ui: &mut Ui) {
        let Some(job) = &self.job else {
            return;
        };
        widgets::note(
            ui,
            Tone::Error,
            "Do not unplug the sticks or close this window until the write is finished.",
        );
        job.bar.show(ui);
        let cancelling = job.cancel.load(Ordering::Relaxed);
        if cancelling {
            widgets::hint(
                ui,
                "Cancelling: the stick is then only partly written and must be written again",
            );
        }
        let cancel = Btn::new("Cancel").small().enabled(!cancelling);
        if cancel.show(ui).clicked() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Where the window cannot write sticks itself, it shows the command to copy.
    fn command_ui(&mut self, ui: &mut Ui) {
        widgets::hint(
            ui,
            "Writing a stick needs administrator rights: run the command below in a terminal. It asks for your password (sudo).",
        );
        let name = self
            .image
            .as_deref()
            .map_or_else(|| "No image chosen".to_owned(), format::file_name);
        let full = self.image.as_ref().map(|path| path.display().to_string());
        let below: Vec<&str> = full.iter().map(String::as_str).collect();
        let choose = widgets::path_row(ui, &name, &below, |ui| {
            Btn::new("Image to write\u{2026}")
                .small()
                .show(ui)
                .clicked()
        });
        if choose
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Disk image", &["img"])
                .pick_file()
        {
            self.image = Some(path);
        }
        widgets::field(ui, "Disk number", None, None, |ui, label| {
            let input = Input::new(&mut self.disk).width(150.0).show(ui);
            input.labelled_by(label);
        });
        if let Some(image) = &full
            && !self.disk.trim().is_empty()
        {
            let command = command_line(image, &self.disk);
            widgets::mono(ui, &command);
            if Btn::new("Copy the command").small().show(ui).clicked() {
                ui.ctx().copy_text(command);
            }
        }
    }

    /// The question about old keys, while a write waits for its answer.
    pub fn dialog_ui(&mut self, ctx: &Context) {
        if let Some(asking) = &self.asking
            && let Some(choice) = confirm_retire(ctx, &asking.before)
        {
            self.decided(choice, ctx);
        }
    }

    /// The same write and check into a file, with no permission prompt.
    pub fn file_ui(&mut self, ui: &mut Ui, locked: bool) {
        widgets::hint(
            ui,
            "A dry run: the same write and the same check into a file, with no permission prompt.",
        );
        let name = self
            .to_file
            .as_deref()
            .map_or_else(|| "none chosen".to_owned(), format::file_name);
        let full = self.to_file.as_ref().map(|path| path.display().to_string());
        let below: Vec<&str> = full.iter().map(String::as_str).collect();
        let idle = !locked && self.job.is_none();
        let ready = self.can_write(locked) && self.to_file.is_some();
        let (write, choose) = widgets::path_row(ui, &name, &below, |ui| {
            let write = Btn::new("Write the file").small().enabled(ready);
            let write = write.show(ui).clicked();
            let choose = Btn::new("Target file\u{2026}").small().enabled(idle);
            (write, choose.show(ui).clicked())
        });
        if choose
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Disk image", &["img"])
                .set_file_name("stick.img")
                .save_file()
        {
            self.to_file = Some(path);
        }
        if write && let Some(file) = self.to_file.clone() {
            self.start(Target::File(file), Vec::new(), false, ui.ctx());
        }
    }
}

/// Sample states for the pictures. Nothing runs.
#[cfg(test)]
impl Screen {
    /// A write that has been going for five seconds.
    pub(super) fn sample_job(&mut self, progress: crate::make::stick_job::Progress) {
        let (_, events) = channel();
        let now = Instant::now();
        let began = now
            .checked_sub(std::time::Duration::from_secs(5))
            .unwrap_or(now);
        let mut bar = Bar::default();
        let mut start = progress;
        (start.done, start.checked) = (0, 1);
        bar.update(start, began);
        bar.update(progress, now);
        self.job = Some(Job {
            cancel: Arc::default(),
            bar,
            events,
            before: Vec::new(),
            retire: false,
            account: None,
            disks: 1,
            started: began,
        });
    }

    /// The question about the old keys of the disks the Hub knows.
    pub(super) fn sample_question(&mut self) {
        self.asking = Some(Asking {
            disks: Vec::new(),
            before: self.earlier.values().cloned().collect(),
        });
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(crate) fn disk(number: u32, name: &str, serial: &str, size: u64, bus: &str) -> Disk {
        serde_json::from_value(serde_json::json!({
            "Number": number,
            "FriendlyName": name,
            "SerialNumber": serial,
            "Path": format!("path-{number}"),
            "Size": size,
            "BusType": bus,
            "IsSystem": false,
            "IsBoot": false,
        }))
        .expect("disk")
    }

    fn stick_on(disk: &Disk, name: &str) -> HubStick {
        let id = &HardwareId::of(disk).expect("id").fingerprint[..8];
        serde_json::from_str(&format!(
            r#"{{"stick_id":"{name}","team":"{name}","hardware":{{"id":"{id}","write_count":2}}}}"#
        ))
        .expect("stick")
    }

    /// Two good sticks ticked, the first one known to the Hub.
    fn two_chosen() -> Screen {
        let first = disk(1, "Stick", "4C5300012", 30_000_000_000, "USB");
        let second = disk(2, "Stick", "60A44C42", 60_000_000_000, "USB");
        let earlier = BTreeMap::from([(1, stick_on(&first, "old"))]);
        Screen {
            disks: vec![first, second],
            size: 5_000_000_000,
            base: Some(PathBuf::from("base.img")),
            profile: Some(PathBuf::from("profile.json")),
            email: Some("admin@example.org".to_owned()),
            chosen: BTreeSet::from([1, 2]),
            earlier,
            wifi_password: "correct horse battery".to_owned(),
            ..Screen::default()
        }
    }

    // No test presses the button without a stick the Hub knows: that would start the writer.
    #[test]
    fn nothing_is_asked_or_started_without_the_typed_confirmation_of_exactly_these_disks() {
        let mut screen = two_chosen();
        for typed in ["", "erase", "erase 1", "erase 2 1", "erase 1 2 3"] {
            typed.clone_into(&mut screen.typed);
            screen.press(&Context::default());
            assert!(screen.job.is_none() && screen.asking.is_none(), "{typed}");
        }
        " erase 1 2 ".clone_into(&mut screen.typed);
        screen.press(&Context::default());
        let tokens: Vec<(u32, String)> = screen
            .disks
            .iter()
            .map(|disk| (disk.number, disk.identity_token()))
            .collect();
        let asking = screen.asking.expect("the question about the old key");
        assert_eq!(asking.disks, tokens);
        assert_eq!(asking.before.len(), 1);
        assert!(screen.job.is_none());
    }

    #[test]
    fn an_image_that_needs_a_signature_writes_nothing_while_signed_out() {
        let mut screen = two_chosen();
        screen.support.signature = true;
        screen.email = None;
        "erase 1 2".clone_into(&mut screen.typed);
        screen.press(&Context::default());
        assert!(screen.job.is_none() && screen.asking.is_none());
    }

    #[test]
    fn cancelling_the_question_starts_nothing_and_keeps_what_was_typed() {
        let mut screen = two_chosen();
        "erase 1 2".clone_into(&mut screen.typed);
        screen.press(&Context::default());
        screen.decided(Choice::Cancel, &Context::default());
        assert!(screen.job.is_none() && screen.asking.is_none());
        assert_eq!(screen.wifi_password, "correct horse battery");
    }

    #[test]
    fn a_disk_that_may_not_be_written_does_not_stay_ticked() {
        let mut screen = two_chosen();
        let bus = "File Backed Virtual";
        let virtual_disk = disk(3, "Virtual", "abcd1234", 60_000_000_000, bus);
        screen.disks.push(virtual_disk);
        screen
            .disks
            .push(disk(4, "Small", "efgh5678", 1_000, "USB"));
        screen.allow_virtual = true;
        screen.chosen.extend([3, 4, 9]);
        screen.prune();
        assert_eq!(screen.chosen, BTreeSet::from([1, 2, 3]));
        screen.allow_virtual = false;
        screen.prune();
        assert_eq!(screen.chosen, BTreeSet::from([1, 2]));
    }

    #[test]
    fn another_base_image_forgets_the_disks_and_a_late_listing_is_dropped() {
        let mut screen = two_chosen();
        "erase 1 2".clone_into(&mut screen.typed);
        let late = Event::Disks(screen.generation, Ok((1, screen.disks.clone())));
        screen.base_replaced();
        assert!(screen.disks.is_empty() && screen.chosen.is_empty());
        assert!(screen.typed.is_empty() && screen.earlier.is_empty());
        assert!(screen.sender.send(late).is_ok());
        screen.receive(&Context::default());
        assert_eq!(screen.disks.len(), 0);
    }

    #[test]
    fn a_test_file_write_names_no_disk_and_takes_the_password_out_of_the_field() {
        let mut screen = two_chosen();
        let file = Target::File(PathBuf::from("out.img"));
        let request = screen.request(file).expect("request");
        assert!(request.disks.is_empty() && request.confirmed.is_empty());
        assert_eq!(request.to_file, Some(PathBuf::from("out.img")));
        assert!(request.wifi_password.is_some());
        assert_eq!(screen.wifi_password, "");
        let disks = Target::Disks(vec![(2, "ab".to_owned())]);
        let request = screen.request(disks).expect("request");
        assert_eq!((request.disks, request.to_file), (vec![2], None));
    }

    #[test]
    fn only_a_write_that_worked_hands_on_the_old_sticks() {
        let results = [
            (Err("removed".to_owned()), false),
            (Ok("abc".to_owned()), true),
        ];
        for (result, worked) in results {
            let mut screen = two_chosen();
            "erase 1 2".clone_into(&mut screen.typed);
            screen.sample_job(crate::make::stick_job::Progress {
                phase: "writing",
                done: 1,
                total: 2,
                checked: 0,
            });
            let (sender, events) = channel();
            let before = screen.earlier.values().cloned().collect();
            let job = screen.job.as_mut().expect("job");
            (job.events, job.before, job.retire) = (events, before, true);
            assert!(sender.send(JobEvent::Done(result)).is_ok());
            screen.receive(&Context::default());
            assert!(screen.job.is_none() && screen.typed.is_empty());
            assert_eq!(screen.wifi_password, "");
            assert_eq!(screen.written, worked);
            let finished = screen.take_finished();
            assert_eq!(finished.is_some(), worked);
            assert!(finished.is_none_or(|(before, retire, _)| retire && before.len() == 1));
        }
    }

    #[test]
    fn nothing_is_erased_while_the_old_keys_are_looked_up_or_after_that_failed() {
        let mut screen = two_chosen();
        "erase 1 2".clone_into(&mut screen.typed);
        for lookup in [Lookup::Pending, Lookup::Failed("down".to_owned())] {
            screen.lookup = lookup;
            screen.press(&Context::default());
            assert!(screen.job.is_none() && screen.asking.is_none());
        }
        screen.lookup = Lookup::Settled;
        screen.press(&Context::default());
        assert!(screen.asking.is_some());
    }

    #[test]
    fn signing_in_after_the_listing_starts_the_lookup() {
        let mut screen = two_chosen();
        let (base, profile) = (screen.base.clone(), screen.profile.clone());
        screen.email = None;
        screen.follow(
            base.as_deref(),
            profile.as_deref(),
            Some("b@example.org"),
            &Context::default(),
        );
        assert!(matches!(screen.lookup, Lookup::Pending));
        assert!(screen.earlier.is_empty());
    }

    #[test]
    fn a_password_is_kept_only_for_the_profile_and_network_it_was_typed_for() {
        let mut screen = two_chosen();
        screen.bind_password(Some("venue"));
        "secret".clone_into(&mut screen.wifi_password);
        screen.bind_password(Some("venue"));
        assert_eq!(screen.wifi_password, "secret");
        screen.bind_password(Some("other"));
        assert_eq!(screen.wifi_password, "");
        "secret".clone_into(&mut screen.wifi_password);
        screen.profile = Some(PathBuf::from("another.json"));
        screen.bind_password(Some("other"));
        assert_eq!(screen.wifi_password, "");
    }

    #[test]
    fn a_test_file_that_worked_completes_no_stick_and_keeps_what_the_disks_held() {
        let mut screen = two_chosen();
        screen.sample_job(crate::make::stick_job::Progress {
            phase: "writing",
            done: 1,
            total: 2,
            checked: 0,
        });
        let (sender, events) = channel();
        let job = screen.job.as_mut().expect("job");
        (job.events, job.disks) = (events, 0);
        assert!(sender.send(JobEvent::Done(Ok("abc".to_owned()))).is_ok());
        screen.receive(&Context::default());
        assert!(screen.success.contains("no stick was written"));
        assert!(!screen.written && screen.take_finished().is_none());
        assert_eq!(screen.earlier.len(), 1);
    }

    #[test]
    fn a_retired_or_archived_stick_is_not_what_a_disk_held() {
        let first = disk(1, "Stick", "4C5300012", 30_000_000_000, "USB");
        let working = stick_on(&first, "now");
        let mut retired = stick_on(&first, "retired");
        retired.retired_at = Some("2026-11-24".to_owned());
        let mut archived = stick_on(&first, "archived");
        archived.archived_at = Some("2026-11-24".to_owned());
        let disks = [first];
        assert!(matched(&disks, &[retired.clone(), archived]).is_empty());
        let known = matched(&disks, &[retired, working]);
        assert_eq!(known[&1].stick_id, "now");
    }

    #[test]
    fn a_write_needs_the_base_image_and_the_profile_and_starts_nothing_without_them() {
        let mut screen = Screen::default();
        let file = Target::File(PathBuf::from("out.img"));
        screen.start(file, Vec::new(), false, &Context::default());
        assert!(screen.job.is_none());
        assert!(screen.failure.contains("base image"));
    }

    #[test]
    fn the_command_quotes_the_image_for_the_shell() {
        let command = command_line(" /tmp/it's a.img ", " 4 ");
        assert_eq!(
            command,
            r"sudo ck-forge stick write '/tmp/it'\''s a.img' --disk 4"
        );
    }

    #[test]
    fn a_quote_in_a_name_cannot_end_a_shell_word() {
        assert_eq!(quote_posix("a'b"), r"'a'\''b'");
    }
}
