mod support;

use std::path::PathBuf;

use ck_admin_core::{Built, Error, Profile, Registry, build_image, configure_image, show_image};
use ck_protocol::LocalDateTime;
use tempfile::TempDir;

fn time() -> LocalDateTime {
    "2026-11-10T15:22".parse().expect("valid time")
}

fn profile(team: &str) -> Profile {
    let mut profile = Profile::template();
    team.clone_into(&mut profile.settings.team);
    profile
}

/// A folder with a base image, and where the registry of the test goes.
struct Fixture {
    dir: TempDir,
    base: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let base = support::make_image(&dir, "base.img");
        let registry = dir.path().join("registry.json");
        Self {
            dir,
            base,
            registry,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn build(&self, out: &str, profile: &Profile) -> Result<Built, Error> {
        build_image(
            &self.base,
            &self.path(out),
            profile,
            None,
            &self.registry,
            time(),
            |_, _| {},
        )
    }
}

#[test]
fn builds_a_stick_image_and_registers_it() {
    let fixture = Fixture::new();
    let before = std::fs::read(&fixture.base).expect("read base");

    let built = fixture
        .build("stick-1.img", &profile("Team A"))
        .expect("build");

    assert_eq!(
        std::fs::read(&fixture.base).expect("read base"),
        before,
        "base untouched"
    );
    assert_eq!(
        show_image(&fixture.path("stick-1.img")).expect("show"),
        built.config
    );
    let registry = Registry::load(&fixture.registry).expect("registry");
    let entry = &registry.list()[0];
    assert_eq!(entry.stick_id, built.config.stick_id);
    assert_eq!(entry.token, built.config.token);
    assert_eq!(entry.image, "stick-1.img");
}

#[test]
fn a_bad_profile_stops_before_any_copy() {
    let fixture = Fixture::new();
    let mut bad = Profile::template();
    bad.settings.api.heartbeat_s = 0;
    let error = fixture.build("out.img", &bad).expect_err("invalid");
    assert!(matches!(error, Error::InvalidProfile(_)));
    assert!(!fixture.path("out.img").exists());
}

#[test]
fn a_failed_build_leaves_no_image_behind() {
    let mut fixture = Fixture::new();
    // The registry path is a folder, so registering fails after the copy and the settings write.
    fixture.registry = fixture.path("registry-folder");
    std::fs::create_dir(&fixture.registry).expect("folder");
    fixture.build("out.img", &profile("A")).expect_err("fails");
    assert!(
        !fixture.path("out.img").exists(),
        "the half-made image is removed"
    );
}

#[test]
fn never_overwrites_an_existing_image() {
    let fixture = Fixture::new();
    let taken = support::make_image(&fixture.dir, "taken.img");
    let before = std::fs::read(&taken).expect("read");
    fixture
        .build("taken.img", &profile("A"))
        .expect_err("exists");
    assert_eq!(std::fs::read(&taken).expect("read"), before);
}

#[test]
fn configure_changes_the_settings_and_the_registry_labels_but_keeps_the_identity() {
    let fixture = Fixture::new();
    let out = fixture.path("stick.img");
    let built = fixture.build("stick.img", &profile("Old")).expect("build");

    let mut new = profile("New");
    new.settings.site = "Lab 2".to_owned();
    "Autumn Contest".clone_into(&mut new.name);
    let changed = configure_image(&out, &new, None, &fixture.registry).expect("configure");

    assert_eq!(changed.settings.team, "New");
    assert_eq!(changed.stick_id, built.config.stick_id);
    assert_eq!(changed.token, built.config.token);
    assert_eq!(changed.created_at, built.config.created_at);
    assert_eq!(show_image(&out).expect("show"), changed);

    let registry = Registry::load(&fixture.registry).expect("registry");
    let entry = registry.find(&built.config.stick_id).expect("entry");
    assert_eq!(
        (
            entry.team.as_str(),
            entry.site.as_str(),
            entry.contest.as_str()
        ),
        ("New", "Lab 2", "Autumn Contest")
    );
    assert_eq!(
        entry.token, built.config.token,
        "the secret is never changed"
    );
}

#[test]
fn configure_refuses_settings_the_agent_would_refuse_on_this_stick() {
    let fixture = Fixture::new();
    let out = fixture.path("stick.img");
    let built = fixture.build("stick.img", &profile("Old")).expect("build");

    // Fine as settings, but the computer name of this stick would be empty.
    let mut unusable = profile("!!!");
    "{team}".clone_into(&mut unusable.settings.branding.computer_name_pattern);
    let error = configure_image(&out, &unusable, None, &fixture.registry).expect_err("refused");
    assert!(matches!(error, Error::InvalidProfile(_)), "{error}");
    assert_eq!(
        show_image(&out).expect("show"),
        built.config,
        "the stick is unchanged"
    );
}

#[test]
fn a_plain_image_has_no_settings_to_show_or_change() {
    let fixture = Fixture::new();
    assert!(matches!(
        show_image(&fixture.base),
        Err(Error::ImageNotConfigured)
    ));
    assert!(matches!(
        configure_image(&fixture.base, &Profile::template(), None, &fixture.registry),
        Err(Error::ImageNotConfigured)
    ));
}

#[test]
fn the_profile_name_becomes_the_contest_name_on_the_stick() {
    let fixture = Fixture::new();
    let mut named = profile("A");
    "Spring Contest".clone_into(&mut named.name);
    let built = fixture.build("stick.img", &named).expect("build");
    assert_eq!(built.config.settings.contest.name, "Spring Contest");
    assert_eq!(
        show_image(&fixture.path("stick.img"))
            .expect("show")
            .settings
            .contest
            .name,
        "Spring Contest"
    );

    // A name set in the settings wins over the profile's name.
    let mut explicit = profile("B");
    "Profile name".clone_into(&mut explicit.name);
    "Settings name".clone_into(&mut explicit.settings.contest.name);
    assert_eq!(explicit.stick_settings().contest.name, "Settings name");
}

#[test]
fn configure_works_without_a_registry_file_and_does_not_create_one() {
    let fixture = Fixture::new();
    fixture.build("stick.img", &profile("Old")).expect("build");
    let elsewhere = fixture.path("other-registry.json");
    configure_image(
        &fixture.path("stick.img"),
        &profile("New"),
        None,
        &elsewhere,
    )
    .expect("configure");
    assert!(!elsewhere.exists());
}

#[test]
fn sticks_of_one_contest_get_machine_numbers_one_after_another() {
    let fixture = Fixture::new();
    let mut numbers = Vec::new();
    for name in ["1.img", "2.img", "3.img"] {
        let built = fixture.build(name, &profile("A")).expect("build");
        numbers.push(built.config.machine_number);
    }
    assert_eq!(numbers, [Some(1), Some(2), Some(3)]);
    assert_eq!(
        show_image(&fixture.path("3.img"))
            .expect("show")
            .machine_number,
        Some(3)
    );

    // Changing the settings of a stick never changes its number.
    let changed = configure_image(
        &fixture.path("2.img"),
        &profile("B"),
        None,
        &fixture.registry,
    )
    .expect("configure");
    assert_eq!(changed.machine_number, Some(2));
}
