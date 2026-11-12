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
    let first = make_stick_config(&profile.settings, time()).expect("valid settings");
    let second = make_stick_config(&profile.settings, time()).expect("valid settings");

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

    let config = make_stick_config(&Profile::default().settings, time()).expect("valid settings");
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
    // Only the registry remains: no temporary files.
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list folder")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("name")
        })
        .collect();
    names.sort();
    assert_eq!(names, ["registry.json"]);
}

fn entry(name: &str) -> RegistryEntry {
    let config = make_stick_config(&Profile::default().settings, time()).expect("valid settings");
    RegistryEntry {
        stick_id: config.stick_id,
        token: config.token,
        team: name.to_owned(),
        site: "Lab".to_owned(),
        contest: "RCPC".to_owned(),
        image: format!("{name}.img"),
        created_at: config.created_at,
    }
}

#[test]
fn sticks_added_at_the_same_time_are_all_kept() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("registry.json");

    std::thread::scope(|scope| {
        for number in 0..8 {
            let path = &path;
            scope.spawn(move || {
                Registry::register(path, entry(&format!("team-{number}"))).expect("register");
            });
        }
    });

    assert_eq!(Registry::load(&path).expect("load").list().len(), 8);
}

#[test]
fn files_that_differ_only_in_extension_do_not_clash() {
    let dir = TempDir::new().expect("temp dir");
    let json = dir.path().join("contest.json");
    let tmp = dir.path().join("contest.tmp");
    let first = Profile {
        name: "one".to_owned(),
        ..Profile::default()
    };
    let second = Profile {
        name: "two".to_owned(),
        ..Profile::default()
    };

    first.save(&json).expect("save json");
    second.save(&tmp).expect("save tmp");

    assert_eq!(Profile::load(&json).expect("load").name, "one");
    assert_eq!(Profile::load(&tmp).expect("load").name, "two");
}

#[test]
fn a_missing_profile_says_so() {
    let dir = TempDir::new().expect("temp dir");
    assert!(matches!(
        Profile::load(&dir.path().join("none.json")),
        Err(Error::ProfileMissing(_))
    ));
}

#[test]
fn no_stick_is_made_from_invalid_settings() {
    let mut settings = Profile::default().settings;
    settings.api.heartbeat_s = 0;
    assert!(matches!(
        make_stick_config(&settings, time()),
        Err(Error::InvalidProfile(_))
    ));
}

#[cfg(unix)]
#[test]
fn the_registry_is_private_to_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("registry.json");
    Registry::register(&path, entry("team")).expect("register");
    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "group and others must have no access");
}
