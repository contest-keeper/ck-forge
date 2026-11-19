//! Images with a `CKCONFIG` partition keep the stick's settings and wallpaper there, not on the EFI
//! partition.

mod support;

use ck_admin_core::{
    Profile, Wallpaper, build_image, build_thin_image, configure_image, show_image,
};
use ck_protocol::LocalDateTime;
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
    configure_image(&out, &other, Some(&picture(vec![9; 100])), &registry).expect("configure");

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
        |_, _| {},
    )
    .expect("build thin");

    let head = std::fs::metadata(&out).expect("thin").len();
    assert_eq!(head, support::CONFIG_END, "the head ends with CKCONFIG");
    assert_eq!(show_image(&out).expect("show"), built.config);

    // Written out, the thin image is the whole stick with the settings on it.
    let target = dir.path().join("target.img");
    ck_writer::write_image(&out, &target, |_, _, _| {}).expect("write");
    assert_eq!(show_image(&target).expect("show"), built.config);
}
