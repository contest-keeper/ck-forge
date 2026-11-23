//! Images with a `CKCONFIG` partition keep the stick's settings and wallpaper there, not on the EFI
//! partition.

mod support;

use std::io::{Seek, SeekFrom, Write};

use ck_admin_core::{
    EnrolmentBuild, Error, ImageConfiguration, Profile, Wallpaper, build_enrolment_image,
    build_image, build_thin_image, check_one_stick, configure_image, image_configuration,
    show_image, supports_enrolment,
};
use ck_protocol::{ENROLMENT_SCHEMA, EnrolmentCode, LocalDateTime};
use tempfile::TempDir;

fn time() -> LocalDateTime {
    "2026-11-10T15:22".parse().expect("valid time")
}

fn profile() -> Profile {
    let mut profile = Profile::template();
    profile.settings.branding.wallpaper = Some("wall.png".to_owned());
    profile
}

fn picture(bytes: Vec<u8>) -> Wallpaper {
    Wallpaper {
        name: "wall.png".to_owned(),
        bytes,
    }
}

#[test]
fn enrolment_requires_a_compatible_base_and_only_writes_the_code_to_ckconfig() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image_with_config(&dir, "base.img");
    let out = dir.path().join("enrolling.img");
    let registry = dir.path().join("registry.json");
    let mut profile = profile();
    profile.settings.api.url = "https://hub.example.test".to_owned();
    let wallpaper = picture(vec![7; 500]);
    let options = EnrolmentBuild {
        base: &base,
        out: &out,
        profile: &profile,
        wallpaper: Some(&wallpaper),
        registry: &registry,
        now: time(),
        sign: None,
    };
    assert!(!supports_enrolment(&base).expect("old image"));
    assert!(matches!(
        build_enrolment_image(&options, |_, _, _| panic!("must not mint"), |_, _| {}),
        Err(Error::EnrolmentUnsupported)
    ));
    assert!(!registry.exists() && !out.exists());

    let mut disk = std::fs::OpenOptions::new()
        .write(true)
        .open(&base)
        .expect("disk");
    disk.seek(SeekFrom::Start(support::CONFIG_END))
        .expect("root");
    disk.write_all(include_bytes!("fixtures/enrolment.ext2"))
        .expect("ext filesystem");
    drop(disk);
    assert!(supports_enrolment(&base).expect("marker"));
    let fixture_code = "ABCD-EFGH-JKLM-NPQR-STUV";
    let built = build_enrolment_image(
        &options,
        |settings, number, scope| {
            assert_eq!(settings.team, profile.settings.team);
            assert_eq!(number, 1);
            assert!(scope.is_none());
            Ok(EnrolmentCode::parse(fixture_code).expect("fixture code"))
        },
        |_, _| {},
    )
    .expect("build");
    let bytes = ck_image::read_config_file(&out, "config.json").expect("config partition");
    // The hash Forge gives the Hub is the hash of the file that is on the stick.
    assert_eq!(
        ck_admin_core::enrolment_config_sha256(&built),
        ck_crypto::sha256_hex(&bytes)
    );
    let mut json: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    assert_eq!(json["schema"], ENROLMENT_SCHEMA);
    assert_eq!(json["stick_id"], built.stick_id.as_str());
    assert_eq!(json["enrolment_code"], fixture_code);
    assert!(json.get("token").is_none());
    assert!(ck_image::read_file(&out, "ck/config.json").is_err());
    let registry_json = std::fs::read_to_string(registry).expect("registry");
    assert!(!registry_json.contains(fixture_code));
    assert!(!registry_json.contains("token"));
    assert!(matches!(
        image_configuration(&out),
        Ok(ImageConfiguration::Enrolment(_))
    ));
    assert!(matches!(
        check_one_stick(&out, 2),
        Err(Error::ImageForOneStick(_))
    ));

    json["token"] = serde_json::json!("fixture-legacy-token");
    ck_image::write_config_file(
        &out,
        "config.json",
        &serde_json::to_vec(&json).expect("JSON"),
    )
    .expect("ambiguous config");
    let error = check_one_stick(&out, 2).unwrap_err().to_string();
    assert!(!error.contains("fixture-legacy-token"));
    assert!(!error.contains(fixture_code));
}

#[test]
fn the_settings_and_the_wallpaper_go_to_the_config_partition_only() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image_with_config(&dir, "base.img");
    let out = dir.path().join("stick.img");
    let registry = dir.path().join("r.json");

    let built = build_image(
        &base,
        &out,
        &profile(),
        Some(&picture(vec![7; 5000])),
        &registry,
        time(),
        |_, _| {},
    )
    .expect("build");

    assert_eq!(show_image(&out).expect("show"), built.config);
    assert_eq!(
        ck_image::read_config_file(&out, "config.json").expect("on CKCONFIG"),
        serde_json::to_vec_pretty(&built.config).expect("json")
    );
    assert_eq!(
        ck_image::read_config_file(&out, "wall.png").expect("on CKCONFIG"),
        vec![7; 5000]
    );
    assert!(ck_image::read_file(&out, "ck/config.json").is_err());
    assert!(ck_image::read_file(&out, "ck/wall.png").is_err());
    assert_eq!(built.config.schema, ck_protocol::SCHEMA);
}

#[test]
fn configuring_again_replaces_the_files_on_the_config_partition() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image_with_config(&dir, "base.img");
    let out = dir.path().join("stick.img");
    let registry = dir.path().join("r.json");
    build_image(
        &base,
        &out,
        &profile(),
        Some(&picture(vec![1; 10])),
        &registry,
        time(),
        |_, _| {},
    )
    .expect("build");

    let mut other = profile();
    other.settings.team = "Team B".to_owned();
    configure_image(&out, &other, Some(&picture(vec![9; 100])), &registry, None)
        .expect("configure");

    assert_eq!(show_image(&out).expect("show").settings.team, "Team B");
    assert_eq!(
        ck_image::read_config_file(&out, "wall.png").expect("read"),
        vec![9; 100]
    );
}

#[test]
fn a_thin_image_reaches_the_end_of_the_config_partition() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image_with_config(&dir, "base.img");
    let out = dir.path().join("thin.img");
    let registry = dir.path().join("r.json");

    let built = build_thin_image(
        &base,
        &out,
        &profile(),
        Some(&picture(vec![7; 100])),
        &registry,
        time(),
        None,
        |_, _| {},
    )
    .expect("build thin");

    let head = std::fs::metadata(&out).expect("thin").len();
    assert_eq!(head, support::CONFIG_END, "the head ends with CKCONFIG");
    let on_stick = ck_image::read_config_file(&out, "config.json").expect("settings file");
    assert_eq!(
        ck_admin_core::stick_config_sha256(&built.config),
        ck_crypto::sha256_hex(&on_stick)
    );
    assert_eq!(show_image(&out).expect("show"), built.config);

    // Written out, the thin image is the whole stick with the settings on it.
    let target = dir.path().join("target.img");
    ck_writer::write_image(&out, &target, |_, _, _| {}).expect("write");
    assert_eq!(show_image(&target).expect("show"), built.config);
}

#[test]
fn changing_the_contest_window_requires_a_fresh_stick_key() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image_with_config(&dir, "base.img");
    let first_image = dir.path().join("first.img");
    let second_image = dir.path().join("second.img");
    let registry = dir.path().join("r.json");
    let scoped = |contest_id: &str| -> Profile {
        serde_json::from_value(serde_json::json!({
            "name": "Scoped Contest",
            "settings": {"contest": {"platform": "codeforces", "contest_id": contest_id}},
            "stick_key_window": {
                "valid_from": "2026-11-19T08:00:00Z",
                "valid_until": "2026-11-19T13:00:00Z"
            }
        }))
        .expect("scoped profile")
    };

    let first = build_thin_image(
        &base,
        &first_image,
        &scoped("4"),
        None,
        &registry,
        time(),
        None,
        |_, _| {},
    )
    .expect("first key");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&registry).expect("registry"))
            .expect("valid registry JSON");
    assert_eq!(saved["sticks"][0]["credential_scope"]["contest_id"], "4");
    assert_eq!(
        saved["sticks"][0]["credential_scope"]["valid_until"],
        "2026-11-19T13:00:00Z"
    );
    assert!(configure_image(&first_image, &scoped("5"), None, &registry, None).is_err());
    assert_eq!(show_image(&first_image).expect("unchanged"), first.config);

    let next = build_thin_image(
        &base,
        &second_image,
        &scoped("5"),
        None,
        &registry,
        time(),
        None,
        |_, _| {},
    )
    .expect("new key");
    assert_ne!(first.config.stick_id, next.config.stick_id);
    assert_ne!(first.config.token, next.config.token);
}
