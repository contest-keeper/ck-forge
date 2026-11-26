//! Pictures of the contest page in the states of the reference pictures, with the same sample
//! data as the mock of the earlier web window.

use std::path::PathBuf;

use ck_admin_core::{Profile, ProfileSummary, StickKeyWindow};
use ck_protocol::RecordingMode;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::Forge;
use crate::shots::{capture_then, wheel};
use crate::side::hub_sign_in;

fn profile() -> Profile {
    let mut profile = Profile::template();
    let settings = &mut profile.settings;
    "keeper-{n}".clone_into(&mut settings.branding.computer_name_pattern);
    "KEEPER".clone_into(&mut settings.branding.usb_label);
    settings.network.allowed_domains = ["codeforces.com", ".codeforces.com", "en.cppreference.com"]
        .map(str::to_owned)
        .to_vec();
    settings.recording.mode = RecordingMode::Off;
    settings.api.heartbeat_s = 60;
    settings.block_bluetooth = true;
    profile
}

fn sample(forge: &mut Forge) {
    forge.state.use_profile(profile(), None);
}

fn signed_in(forge: &mut Forge) {
    sample(forge);
    forge.hub.state = hub_sign_in::State::SignedIn("admin@example.org".to_owned());
}

fn picture(name: &str, scroll: f32, setup: impl Fn(&mut Forge)) {
    capture_then(name, setup, |harness| wheel(harness, scroll));
}

/// Opens or ticks what is labelled `label`, then scrolls until its top is where the earlier window
/// had it.
fn open(name: &str, label: &str, setup: impl Fn(&mut Forge)) {
    capture_then(name, setup, |harness: &mut Harness<'_>| {
        wheel(harness, 4000.0);
        harness.get_by_label(label).click();
        harness.run_steps(4);
        wheel(harness, 4000.0);
        // Back to the top, then down to the label.
        harness.hover_at(eframe::egui::pos2(600.0, 400.0));
        harness.event(eframe::egui::Event::MouseWheel {
            unit: eframe::egui::MouseWheelUnit::Point,
            delta: eframe::egui::vec2(0.0, 8000.0),
            phase: eframe::egui::TouchPhase::Move,
            source: eframe::egui::MouseWheelSource::Wheel,
            modifiers: eframe::egui::Modifiers::NONE,
        });
        harness.run_steps(60);
        let at = harness.get_by_label(label).rect().top();
        // Where the contents of the page start: lower under the taller header of modern.
        let button = harness.get_by_label("Save and continue").rect().height();
        let top = if button > 35.0 { 109.0 } else { 70.0 };
        wheel(harness, at - top);
    });
}

#[test]
#[ignore = "writes pictures; needs CK_SHOTS_DIR and a graphics adapter"]
fn shots_contest() {
    picture("contest-default", 0.0, sample);
    picture("contest-middle", 560.0, sample);
    picture("contest-bottom", 4000.0, sample);
    picture("contest-signed-in", 0.0, signed_in);
    picture("contest-hub-list", 0.0, |forge| {
        signed_in(forge);
        forge.contest.hub_profiles = ["Regional Finals 2026", "Practice round"]
            .map(|name| ProfileSummary {
                id: name.to_owned(),
                name: name.to_owned(),
            })
            .into();
    });
    picture("contest-invalid", 0.0, |forge| {
        sample(forge);
        let profile = forge.state.profile.as_mut().expect("profile");
        profile.settings.branding.bookmarks[0].title.clear();
        "A LABEL TOO LONG".clone_into(&mut profile.settings.branding.usb_label);
        forge.leave_failure = "the profile has problems:\n- bookmark 1 needs a title".to_owned();
    });
    picture("contest-field-errors", 0.0, |forge| {
        let mut profile = profile();
        "x".clone_into(&mut profile.settings.contest.contest_id);
        forge.state.use_profile(profile, None);
    });
    picture("contest-saved-back", 0.0, |forge| {
        let long = r"C:\Users\admin\AppData\Roaming\org.contest-keeper.forge\profiles\my-contest-1a2b3c4d\contest-profile.json";
        forge
            .state
            .use_profile(profile(), Some(PathBuf::from(long)));
    });
    open("contest-branding", "What contestants see", sample);
    open(
        "contest-network",
        "Network, Hub and screen recording",
        sample,
    );
    open("contest-team", "Team and site (optional)", sample);
    open(
        "contest-key-window",
        "Limit when the stick key works (optional)",
        |forge| {
            sample(forge);
        },
    );
    picture("contest-recording", 4000.0, |forge| {
        let mut profile = profile();
        profile.settings.recording.mode = RecordingMode::Snapshots;
        profile.stick_key_window = Some(StickKeyWindow {
            valid_from: "2026-11-26T15:14:00Z".to_owned(),
            valid_until: "2026-11-27T15:14:00Z".to_owned(),
        });
        forge.state.use_profile(profile, None);
    });
    let snapshots = |forge: &mut Forge| {
        let mut profile = profile();
        profile.settings.recording.mode = RecordingMode::Snapshots;
        forge.state.use_profile(profile, None);
    };
    // The end of the network card, with every recording field.
    capture_then("contest-recording-open", snapshots, |harness| {
        wheel(harness, 4000.0);
        harness
            .get_by_label("Network, Hub and screen recording")
            .click();
        harness.run_steps(4);
        wheel(harness, 4000.0);
    });
    // A wallpaper of its own, in a folder that is removed when the pictures are done.
    let folder = tempfile::tempdir().expect("folder");
    image::RgbImage::from_fn(640, 360, |x, y| {
        let [red, green] = [x / 3, y / 2].map(|v| u8::try_from(v).unwrap_or(u8::MAX));
        image::Rgb([red, green, 140])
    })
    .save(folder.path().join("wallpaper.png"))
    .expect("picture");
    let with_wallpaper = |forge: &mut Forge| {
        let mut profile = profile();
        profile.settings.branding.wallpaper = Some("wallpaper.png".to_owned());
        profile.settings.branding.home_page = Some("https://codeforces.com".to_owned());
        "Good luck!".clone_into(&mut profile.settings.branding.welcome_text);
        let path = folder.path().join("contest-profile.json");
        forge.state.use_profile(profile, Some(path));
    };
    picture("contest-wallpaper-preview", 560.0, with_wallpaper);
    open("contest-wallpaper", "What contestants see", with_wallpaper);
    let dated = |forge: &mut Forge| {
        let mut profile = profile();
        profile.settings.contest.end = "2026-11-15T09:00".parse().ok();
        forge.state.use_profile(profile, None);
    };
    capture_then("contest-calendar", dated, |harness| {
        harness.get_by_label("Pick the contest end").click();
        harness.run_steps(4);
    });
}

/// The calendar is drawn by hand, so the way from a clicked day to the profile is checked.
#[test]
fn a_day_picked_in_the_calendar_goes_into_the_profile() {
    let mut forge = Forge::sample();
    let mut profile = profile();
    profile.settings.contest.end = "2026-11-15T09:00".parse().ok();
    forge.state.use_profile(profile, None);
    let mut harness = Harness::builder()
        .with_size([1000.0, 760.0])
        .with_accessibility_check(false)
        .build_ui_state(|ui, forge: &mut Forge| forge.draw(ui), forge);
    harness.run_steps(4);
    harness.get_by_label("Pick the contest end").click();
    harness.run_steps(4);
    harness.get_by_label("20").click();
    harness.run_steps(4);
    let profile = harness.state().state.profile.as_ref().expect("profile");
    let end = profile.settings.contest.end.expect("an end");
    assert_eq!(end.to_string(), "2026-11-20T09:00");
}

#[test]
fn the_calendar_opens_at_the_month_in_the_field() {
    let mut forge = Forge::sample();
    let mut profile = profile();
    profile.settings.contest.end = "2026-11-15T09:00".parse().ok();
    forge.state.use_profile(profile, None);
    let mut harness = Harness::builder()
        .with_size([1000.0, 760.0])
        .with_accessibility_check(false)
        .build_ui_state(|ui, forge: &mut Forge| forge.draw(ui), forge);
    harness.run_steps(4);
    // Open, look at an earlier month, close, open again.
    let open = "Pick the contest end";
    for label in [open, "<", open, open, "20"] {
        harness.get_by_label(label).click();
        harness.run_steps(4);
    }
    let profile = harness.state().state.profile.as_ref().expect("profile");
    let end = profile.settings.contest.end.expect("an end");
    assert_eq!(end.to_string(), "2026-11-20T09:00");
}
