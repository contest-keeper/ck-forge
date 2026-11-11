use ck_admin_core::{Error, Profile, Registry, RegistryEntry, make_stick_config};
use ck_protocol::{LocalDateTime, Problem};
use tempfile::TempDir;

fn time() -> LocalDateTime {
    "2026-11-10T15:22".parse().expect("valid time")
}

#[test]
fn a_profile_is_checked_when_saved_and_when_loaded() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("rcpc.json");

    let mut profile = Profile {
        name: "RCPC 2026".to_owned(),
        ..Profile::default()
    };
    profile.save(&path).expect("save");
    assert_eq!(Profile::load(&path).expect("load"), profile);

    profile.settings.api.heartbeat_s = 0;
    let error = profile.save(&path).expect_err("invalid profile");
    assert!(matches!(&error, Error::InvalidProfile(p) if p == &[Problem::HeartbeatIntervalZero]));

    std::fs::write(
        &path,
        r#"{"name":"x","settings":{"api":{"heartbeat_s":0}}}"#,
    )
    .expect("write");
    assert!(matches!(
        Profile::load(&path),
        Err(Error::InvalidProfile(_))
    ));
}

#[test]
fn every_stick_gets_its_own_id_and_secret() {
    let profile = Profile::default();
    let first = make_stick_config(&profile.settings, time());
    let second = make_stick_config(&profile.settings, time());

    assert_eq!(first.validate(), []);
    assert_ne!(first.stick_id, second.stick_id);
    assert_ne!(first.token, second.token);
    assert_eq!(first.settings, profile.settings);
}

#[test]
fn the_registry_keeps_sticks_and_survives_a_restart() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("registry.json");
    assert_eq!(
        Registry::load(&path).expect("missing file is empty").list(),
        []
    );

    let config = make_stick_config(&Profile::default().settings, time());
    let entry = RegistryEntry {
        stick_id: config.stick_id.clone(),
        token: config.token.clone(),
        team: config.settings.team.clone(),
        site: config.settings.site.clone(),
        contest: "RCPC 2026".to_owned(),
        image: "stick-1.img".to_owned(),
        created_at: config.created_at,
    };

    let mut registry = Registry::default();
    registry.add(entry.clone()).expect("add");
    assert!(matches!(
        registry.add(entry.clone()),
        Err(Error::DuplicateStick(_))
    ));
    registry.save(&path).expect("save");

    let loaded = Registry::load(&path).expect("load");
    assert_eq!(loaded.find(&entry.stick_id), Some(&entry));
    assert!(!dir.path().join("registry.tmp").exists());
}
