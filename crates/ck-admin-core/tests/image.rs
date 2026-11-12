mod support;

use ck_admin_core::{Error, Profile, Registry, build_image, configure_image, show_image};
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

#[test]
fn builds_a_stick_image_and_registers_it() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let before = std::fs::read(&base).expect("read base");
    let out = dir.path().join("stick-1.img");
    let registry = dir.path().join("registry.json");

    let built = build_image(
        &base,
        &out,
        &profile("Team A"),
        &registry,
        time(),
        |_, _| {},
    )
    .expect("build");

    assert_eq!(
        std::fs::read(&base).expect("read base"),
        before,
        "base untouched"
    );
    assert_eq!(show_image(&out).expect("show"), built.config);
    let registry = Registry::load(&registry).expect("registry");
    let entry = &registry.list()[0];
    assert_eq!(entry.stick_id, built.config.stick_id);
    assert_eq!(entry.token, built.config.token);
    assert_eq!(entry.image, "stick-1.img");
}

#[test]
fn every_stick_gets_its_own_identity() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let registry = dir.path().join("registry.json");
    let one = build_image(
        &base,
        &dir.path().join("1.img"),
        &profile("A"),
        &registry,
        time(),
        |_, _| {},
    )
    .expect("one");
    let two = build_image(
        &base,
        &dir.path().join("2.img"),
        &profile("B"),
        &registry,
        time(),
        |_, _| {},
    )
    .expect("two");
    assert_ne!(one.config.stick_id, two.config.stick_id);
    assert_ne!(one.config.token, two.config.token);
    assert_eq!(Registry::load(&registry).expect("registry").list().len(), 2);
}

#[test]
fn a_bad_profile_stops_before_any_copy() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("out.img");
    let mut bad = Profile::template();
    bad.settings.api.heartbeat_s = 0;
    let error = build_image(
        &base,
        &out,
        &bad,
        &dir.path().join("r.json"),
        time(),
        |_, _| {},
    )
    .expect_err("invalid");
    assert!(matches!(error, Error::InvalidProfile(_)));
    assert!(!out.exists());
}

#[test]
fn a_failed_build_leaves_no_image_behind() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("out.img");
    // The registry path is a folder, so registering fails after the copy and the settings write.
    let registry = dir.path().join("registry-folder");
    std::fs::create_dir(&registry).expect("folder");
    build_image(&base, &out, &profile("A"), &registry, time(), |_, _| {}).expect_err("fails");
    assert!(!out.exists(), "the half-made image is removed");
}

#[test]
fn never_overwrites_an_existing_image() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = support::make_image(&dir, "taken.img");
    let before = std::fs::read(&out).expect("read");
    build_image(
        &base,
        &out,
        &profile("A"),
        &dir.path().join("r.json"),
        time(),
        |_, _| {},
    )
    .expect_err("exists");
    assert_eq!(std::fs::read(&out).expect("read"), before);
}

#[test]
fn configure_changes_the_settings_but_keeps_the_identity() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("stick.img");
    let built = build_image(
        &base,
        &out,
        &profile("Old"),
        &dir.path().join("r.json"),
        time(),
        |_, _| {},
    )
    .expect("build");
    let changed =
        configure_image(&out, &profile("New"), &dir.path().join("r.json")).expect("configure");
    assert_eq!(changed.settings.team, "New");
    assert_eq!(changed.stick_id, built.config.stick_id);
    assert_eq!(changed.token, built.config.token);
    assert_eq!(changed.created_at, built.config.created_at);
    assert_eq!(show_image(&out).expect("show"), changed);
}

#[test]
fn a_plain_image_has_no_settings_to_show_or_change() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    assert!(matches!(show_image(&base), Err(Error::ImageNotConfigured)));
    assert!(matches!(
        configure_image(&base, &Profile::template(), &dir.path().join("r.json")),
        Err(Error::ImageNotConfigured)
    ));
}

#[test]
fn the_profile_name_becomes_the_contest_name_on_the_stick() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("stick.img");
    let mut named = profile("A");
    "Spring Contest".clone_into(&mut named.name);
    let built = build_image(
        &base,
        &out,
        &named,
        &dir.path().join("r.json"),
        time(),
        |_, _| {},
    )
    .expect("build");
    assert_eq!(built.config.settings.contest.name, "Spring Contest");
    assert_eq!(
        show_image(&out).expect("show").settings.contest.name,
        "Spring Contest"
    );

    // A name set in the settings wins over the profile's name.
    let mut explicit = profile("B");
    "Profile name".clone_into(&mut explicit.name);
    "Settings name".clone_into(&mut explicit.settings.contest.name);
    assert_eq!(explicit.stick_settings().contest.name, "Settings name");
}

#[test]
fn configure_keeps_the_registry_labels_in_step_with_the_image() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("stick.img");
    let registry = dir.path().join("registry.json");
    let built =
        build_image(&base, &out, &profile("Old"), &registry, time(), |_, _| {}).expect("build");

    let mut changed = profile("New");
    changed.settings.site = "Lab 2".to_owned();
    "Autumn Contest".clone_into(&mut changed.name);
    configure_image(&out, &changed, &registry).expect("configure");

    let registry = Registry::load(&registry).expect("registry");
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
fn configure_works_without_a_registry_file_and_does_not_create_one() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("stick.img");
    build_image(
        &base,
        &out,
        &profile("Old"),
        &dir.path().join("made.json"),
        time(),
        |_, _| {},
    )
    .expect("build");
    let elsewhere = dir.path().join("other-registry.json");
    configure_image(&out, &profile("New"), &elsewhere).expect("configure");
    assert!(!elsewhere.exists());
}
