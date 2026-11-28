use ck_admin_core::{Error, Profile, Registry, RegistryEntry, StickKeyWindow, make_stick_config};
use ck_protocol::{LocalDateTime, Platform, Problem, UploadMode};
use tempfile::TempDir;

fn time() -> LocalDateTime {
    "2026-11-10T15:22".parse().expect("valid time")
}

#[test]
fn a_profile_is_checked_when_saved_and_when_loaded() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("profile.json");

    let mut profile = Profile {
        name: "Example Contest".to_owned(),
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
    let first = make_stick_config(&profile.settings, 1, time()).expect("valid settings");
    let second = make_stick_config(&profile.settings, 1, time()).expect("valid settings");

    assert_eq!(first.validate(), []);
    assert_ne!(first.stick_id, second.stick_id);
    assert_ne!(first.token, second.token);
    assert_eq!(first.settings, profile.settings);
}

#[test]
fn a_stick_key_window_needs_a_contest_and_ordered_utc_dates() {
    let mut profile = Profile::template();
    assert_eq!(profile.credential_scope().expect("legacy"), None);
    profile.stick_key_window = Some(StickKeyWindow {
        valid_from: "2026-11-19T08:00:00Z".to_owned(),
        valid_until: "2026-11-19T07:00:00Z".to_owned(),
    });
    assert!(
        profile.credential_scope().is_err(),
        "a contest ID is required"
    );
    profile.settings.contest.contest_id = "4".to_owned();
    assert!(
        profile.credential_scope().is_err(),
        "the end must be after the start"
    );
    let window = profile.stick_key_window.as_mut().expect("window");
    window.valid_until = "2026-11-19T15:00:00+02:00".to_owned();
    assert!(
        profile.credential_scope().is_err(),
        "the window must be in UTC"
    );
}

#[test]
fn after_contest_upload_warns_when_the_contest_has_no_end() {
    let mut profile = Profile::template();
    profile.settings.api.url = "https://hub.example.com".to_owned();
    profile.settings.recording.upload = UploadMode::AfterContest;
    assert_eq!(
        profile.warnings(),
        ["set the contest end time to upload recordings after the contest"]
    );
    profile.settings.contest.end = Some(time());
    assert_eq!(profile.warnings(), Vec::<String>::new());
}

#[test]
fn the_registry_keeps_sticks_and_survives_a_restart() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("registry.json");
    assert_eq!(
        Registry::load(&path).expect("missing file is empty").list(),
        []
    );

    let entry = entry("stick-1");

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
    let config =
        make_stick_config(&Profile::default().settings, 1, time()).expect("valid settings");
    RegistryEntry {
        stick_id: config.stick_id,
        token: config.token,
        team: name.to_owned(),
        site: "Lab".to_owned(),
        contest: "Example Contest".to_owned(),
        image: format!("{name}.img"),
        created_at: config.created_at,
        machine_number: config.machine_number,
        credential_scope: None,
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
        make_stick_config(&settings, 1, time()),
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

#[test]
fn an_empty_server_address_is_a_warning_not_an_error() {
    let mut profile = Profile::template();
    assert_eq!(profile.problems(), Vec::new());
    assert_eq!(profile.warnings().len(), 1);
    assert!(profile.warnings()[0].contains("no server address"));

    "https://hub.example".clone_into(&mut profile.settings.api.url);
    assert_eq!(profile.warnings(), Vec::<String>::new());
}

#[test]
fn machines_of_a_contest_are_numbered_one_after_another() {
    let mut registry = Registry::default();
    assert_eq!(registry.next_machine_number("Spring"), 1);

    let mut first = entry("a");
    first.contest = "Spring".to_owned();
    first.machine_number = Some(1);
    registry.add(first).expect("add");
    let mut second = entry("b");
    second.contest = "Spring".to_owned();
    second.machine_number = Some(5);
    registry.add(second).expect("add");
    let mut other = entry("c");
    other.contest = "Autumn".to_owned();
    registry.add(other).expect("add");

    assert_eq!(
        registry.next_machine_number("Spring"),
        6,
        "after the highest"
    );
    assert_eq!(
        registry.next_machine_number("Autumn"),
        2,
        "an older entry has no number"
    );
    assert_eq!(
        registry.next_machine_number("Winter"),
        1,
        "every contest counts on its own"
    );
}

#[test]
fn a_new_profile_prefills_nothing_and_a_stick_always_allows_the_anti_bot_check() {
    let mut profile = Profile::template();
    assert_eq!(profile.problems(), []);
    let settings = &profile.settings;
    assert!(settings.team.is_empty() && settings.site.is_empty());
    assert_eq!(settings.recording.mode, ck_protocol::RecordingMode::Off);
    assert!(
        !settings
            .network
            .allowed_domains
            .iter()
            .any(|d| d.contains("codeforces"))
    );
    let built = profile.stick_settings();
    assert_eq!(
        (built.team.as_str(), built.site.as_str()),
        ("default team", "default site")
    );
    assert!(
        built
            .network
            .allowed_domains
            .contains(&"challenges.cloudflare.com".to_owned())
    );
    assert_eq!(built.branding.bookmarks.len(), 1);

    choose(&mut profile, Platform::Codeforces, "2042");
    let bookmarks = profile.stick_settings().branding.bookmarks;
    assert_eq!(bookmarks[0].title, "Codeforces");
    assert_eq!(bookmarks[0].url, "https://codeforces.com/contest/2042");
    let warnings = profile.warnings().len();
    assert_eq!(profile.problems(), []);

    profile.settings.contest.contest_id = "x y".to_owned();
    assert_eq!(profile.warnings().len(), warnings + 1);
}

/// Types a contest id or picks a platform the way the form does: change, then follow.
fn choose(profile: &mut Profile, platform: Platform, id: &str) {
    let was = (
        profile.settings.contest.platform,
        profile.settings.contest.contest_id.clone(),
    );
    profile.settings.contest.platform = platform;
    id.clone_into(&mut profile.settings.contest.contest_id);
    ck_admin_core::follow_contest(profile, was.0, &was.1);
}

#[test]
fn choosing_a_platform_and_contest_adds_its_page_as_the_first_bookmark_and_the_home_page() {
    let mut profile = Profile::template();
    choose(&mut profile, Platform::Codeforces, "2042");
    let branding = &profile.settings.branding;
    assert_eq!(
        branding.bookmarks[0].url,
        "https://codeforces.com/contest/2042"
    );
    assert_eq!(
        branding.home_page.as_deref(),
        Some("https://codeforces.com/contest/2042")
    );

    // Changing the platform moves the bookmark and the home page with it, and lets the site in.
    choose(&mut profile, Platform::Vjudge, "871412");
    let branding = &profile.settings.branding;
    assert_eq!(
        branding.bookmarks[0].url,
        "https://vjudge.net/contest/871412"
    );
    assert_eq!(branding.bookmarks.len(), 2, "moved, not added");
    assert_eq!(
        branding.home_page.as_deref(),
        Some("https://vjudge.net/contest/871412")
    );
    assert!(
        profile
            .settings
            .network
            .allowed_domains
            .contains(&".vjudge.net".to_owned())
    );
    // A number being typed digit by digit follows too, and an id that is not valid takes it away.
    choose(&mut profile, Platform::Vjudge, "8714123");
    assert_eq!(
        profile.settings.branding.bookmarks[0].url,
        "https://vjudge.net/contest/8714123"
    );
    choose(&mut profile, Platform::Vjudge, "");
    assert_eq!(profile.settings.branding.bookmarks.len(), 1);
    assert_eq!(profile.settings.branding.home_page, None);
}

#[test]
fn what_the_admin_changed_is_not_touched_again() {
    let mut profile = Profile::template();
    choose(&mut profile, Platform::Vjudge, "871412");
    // The admin renames the bookmark, points the home page elsewhere, then picks another contest.
    profile.settings.branding.bookmarks[0].url =
        "https://vjudge.net/contest/871412#status".to_owned();
    profile.settings.branding.home_page = Some("https://en.cppreference.com".to_owned());
    choose(&mut profile, Platform::Vjudge, "999");
    let branding = &profile.settings.branding;
    assert_eq!(
        branding.bookmarks[0].url,
        "https://vjudge.net/contest/871412#status"
    );
    assert_eq!(
        branding.home_page.as_deref(),
        Some("https://en.cppreference.com")
    );
    assert!(
        !branding.bookmarks.iter().any(|b| b.url.ends_with("/999")),
        "not added again"
    );
    // A bookmark the admin removed is not brought back by a new id either.
    let mut other = Profile::template();
    choose(&mut other, Platform::Codeforces, "1");
    other.settings.branding.bookmarks.remove(0);
    choose(&mut other, Platform::Codeforces, "12");
    assert_eq!(other.settings.branding.bookmarks.len(), 1);
}

#[test]
fn a_removed_or_changed_bookmark_stays_theirs_across_an_empty_id_a_new_one_and_a_build() {
    let mut removed = Profile::template();
    choose(&mut removed, Platform::Vjudge, "871412");
    removed.settings.branding.bookmarks.remove(0);
    // The form is drawn again before the next change, and sees it is gone.
    choose(&mut removed, Platform::Vjudge, "871412");
    choose(&mut removed, Platform::Vjudge, "");
    choose(&mut removed, Platform::Vjudge, "999");
    assert_eq!(
        removed.settings.branding.bookmarks.len(),
        1,
        "not brought back"
    );
    assert!(
        !removed
            .stick_settings()
            .branding
            .bookmarks
            .iter()
            .any(|b| b.url.ends_with("/999")),
        "a build does not put it back"
    );

    let mut changed = Profile::template();
    choose(&mut changed, Platform::Vjudge, "871412");
    changed.settings.branding.bookmarks[0].url =
        "https://vjudge.net/contest/871412#rank".to_owned();
    choose(&mut changed, Platform::Vjudge, "871412");
    choose(&mut changed, Platform::Vjudge, "");
    choose(&mut changed, Platform::Vjudge, "5");
    let urls: Vec<&str> = changed
        .settings
        .branding
        .bookmarks
        .iter()
        .map(|b| b.url.as_str())
        .collect();
    assert_eq!(
        urls.iter().filter(|u| u.contains("vjudge.net")).count(),
        1,
        "{urls:?}"
    );
    // What the form remembers is saved with the profile, so it lasts across a restart.
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("p.json");
    removed.save(&path).expect("save");
    let mut loaded = Profile::load(&path).expect("load");
    choose(&mut loaded, Platform::Vjudge, "");
    choose(&mut loaded, Platform::Vjudge, "42");
    assert_eq!(loaded.settings.branding.bookmarks.len(), 1);
}

#[test]
fn only_a_numbered_contest_of_a_known_platform_has_a_page_and_a_vjudge_stick_opens_it() {
    assert_eq!(
        ck_admin_core::contest_url(Platform::Vjudge, " 871412 ").as_deref(),
        Some("https://vjudge.net/contest/871412")
    );
    assert_eq!(ck_admin_core::contest_url(Platform::Vjudge, "8x"), None);
    assert_eq!(ck_admin_core::contest_url(Platform::Kilonova, "12"), None);
    let mut profile = Profile::template();
    profile.settings.contest.platform = Platform::Vjudge;
    profile.settings.contest.contest_id = "871412".to_owned();
    let built = profile.stick_settings();
    assert_eq!(
        built.branding.bookmarks[0].url,
        "https://vjudge.net/contest/871412"
    );
    // The contest's site is allowed on the stick even when the profile never listed it.
    assert!(
        built
            .network
            .allowed_domains
            .contains(&".vjudge.net".to_owned())
    );
    assert!(
        !profile
            .settings
            .network
            .allowed_domains
            .contains(&".vjudge.net".to_owned())
    );
    // A non-number on VJudge is warned about like on Codeforces.
    profile.settings.contest.contest_id = "abc".to_owned();
    assert!(
        profile
            .warnings()
            .iter()
            .any(|w| w.contains("not a number"))
    );
}

#[test]
fn the_wifi_password_goes_on_the_stick_but_never_into_the_saved_profile() {
    use ck_protocol::{Wifi, WifiPassword};

    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("profile.json");

    let mut profile = Profile::template();
    profile.settings.wifi = Some(Wifi {
        ssid: "venue".to_owned(),
        password: None,
    });
    profile.set_wifi_password(Some(WifiPassword::new("hunter2-hunter2")));
    assert_eq!(
        profile
            .stick_settings()
            .wifi
            .and_then(|w| w.password)
            .map(|p| p.expose().to_owned()),
        Some("hunter2-hunter2".to_owned()),
        "the stick gets the password"
    );

    profile.save(&path).expect("save");
    let saved = std::fs::read_to_string(&path).expect("read");
    assert!(saved.contains("venue"));
    assert!(
        !saved.contains("hunter2"),
        "the profile file has no password"
    );
    assert!(
        !format!("{profile:?}").contains("hunter2"),
        "Debug hides it"
    );

    // A profile without a network ignores a password instead of inventing a network.
    let mut open = Profile::template();
    open.set_wifi_password(Some(WifiPassword::new("hunter2-hunter2")));
    assert_eq!(open.settings.wifi, None);
}

#[test]
fn a_bookmark_allows_its_own_site_and_never_a_wider_one() {
    use ck_admin_core::site_of;
    assert_eq!(
        site_of("https://www.Example.org/a?b#c").as_deref(),
        Some(".example.org")
    );
    assert_eq!(
        site_of("http://en.cppreference.com").as_deref(),
        Some(".en.cppreference.com")
    );
    // A name under a shared host stays that name, so one user's page never opens the whole host.
    assert_eq!(
        site_of("https://user.github.io/x").as_deref(),
        Some(".user.github.io")
    );
    assert_eq!(
        site_of("https://user:pw@host.test:8443/p").as_deref(),
        Some(".host.test")
    );
    for bad in [
        "ftp://x.org",
        "https://localhost",
        "https://[::1]/",
        "https://bad_host.org",
        "javascript:alert(1)",
        "",
    ] {
        assert_eq!(site_of(bad), None, "{bad}");
    }
}

#[test]
fn the_stick_allows_the_sites_of_its_bookmarks_once_each() {
    let mut profile = Profile::template();
    profile
        .settings
        .branding
        .bookmarks
        .push(ck_protocol::Bookmark {
            title: "Wiki".to_owned(),
            url: "https://wiki.example.org/start".to_owned(),
        });
    // The typed list already lets in .cppreference.com, so the bookmark to a page of it adds nothing.
    let built = profile.stick_settings().network.allowed_domains;
    assert!(built.contains(&".wiki.example.org".to_owned()), "{built:?}");
    assert_eq!(
        built.iter().filter(|d| d.contains("cppreference")).count(),
        1,
        "{built:?}"
    );
    assert!(built.contains(&"challenges.cloudflare.com".to_owned()));
    // What the profile itself keeps is only what the admin typed.
    assert_eq!(
        profile.settings.network.allowed_domains,
        [".cppreference.com"]
    );
}
