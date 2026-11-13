mod support;

use ck_admin_core::{Error, MAX_WALLPAPER_BYTES, Profile, Wallpaper, build_image, configure_image};
use ck_protocol::LocalDateTime;
use tempfile::TempDir;

fn time() -> LocalDateTime {
    "2026-11-10T15:22".parse().expect("valid time")
}

fn profile_with_wallpaper(name: &str) -> Profile {
    let mut profile = Profile::template();
    profile.settings.branding.wallpaper = Some(name.to_owned());
    profile
}

#[test]
fn reads_the_picture_the_profile_names_from_the_profile_folder() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(dir.path().join("wall.png"), b"\x89PNG picture").expect("picture");

    let profile = profile_with_wallpaper("wall.png");
    let wallpaper = Wallpaper::read(&profile.stick_settings(), dir.path())
        .expect("read")
        .expect("a wallpaper");
    assert_eq!(wallpaper.name, "wall.png");
    assert_eq!(wallpaper.bytes, b"\x89PNG picture");

    assert_eq!(
        Wallpaper::read(&Profile::template().settings, dir.path()).expect("read"),
        None,
        "no wallpaper named, none read"
    );
}

#[test]
fn a_missing_oversized_or_non_file_wallpaper_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let settings = profile_with_wallpaper("wall.png").stick_settings();
    assert!(matches!(
        Wallpaper::read(&settings, dir.path()),
        Err(Error::Wallpaper(_))
    ));

    let too_big = usize::try_from(MAX_WALLPAPER_BYTES).expect("fits") + 1;
    std::fs::write(dir.path().join("wall.png"), vec![0_u8; too_big]).expect("big file");
    let error = Wallpaper::read(&settings, dir.path()).expect_err("too big");
    assert!(error.to_string().contains("larger than"), "{error}");

    let folder = TempDir::new().expect("temp dir");
    std::fs::create_dir(folder.path().join("wall.png")).expect("folder");
    let error = Wallpaper::read(&settings, folder.path()).expect_err("a folder");
    assert!(error.to_string().contains("not a file"), "{error}");
}

#[test]
fn the_picture_is_put_on_the_stick_next_to_the_settings() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("stick.img");
    let profile = profile_with_wallpaper("wall.png");
    let picture = Wallpaper {
        name: "wall.png".to_owned(),
        bytes: vec![7; 5000],
    };

    build_image(
        &base,
        &out,
        &profile,
        Some(&picture),
        &dir.path().join("r.json"),
        time(),
        |_, _| {},
    )
    .expect("build");
    assert_eq!(
        ck_image::read_file(&out, "ck/wall.png").expect("read back"),
        vec![7; 5000]
    );

    // Configuring again with another picture replaces it.
    let newer = Wallpaper {
        name: "wall.png".to_owned(),
        bytes: vec![9; 100],
    };
    configure_image(&out, &profile, Some(&newer), &dir.path().join("r.json")).expect("configure");
    assert_eq!(
        ck_image::read_file(&out, "ck/wall.png").expect("read back"),
        vec![9; 100]
    );
}

#[test]
fn a_picture_named_like_the_settings_file_is_refused_before_it_can_replace_it() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("stick.img");
    let picture = Wallpaper {
        name: "Config.json".to_owned(),
        bytes: vec![7; 100],
    };
    let error = build_image(
        &base,
        &out,
        &profile_with_wallpaper("Config.json"),
        Some(&picture),
        &dir.path().join("r.json"),
        time(),
        |_, _| {},
    )
    .expect_err("refused");
    assert!(matches!(error, Error::Wallpaper(_)), "{error}");
    assert!(!out.exists());
}

#[test]
fn the_picture_must_be_the_one_the_profile_names_and_nothing_is_copied_otherwise() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let out = dir.path().join("stick.img");
    let registry = dir.path().join("r.json");
    let picture = Wallpaper {
        name: "other.png".to_owned(),
        bytes: vec![1],
    };

    // The profile names a picture and none is given.
    let named = profile_with_wallpaper("wall.png");
    let error = build_image(&base, &out, &named, None, &registry, time(), |_, _| {})
        .expect_err("missing picture");
    assert!(matches!(error, Error::Wallpaper(_)));
    // A different picture than the one named.
    build_image(
        &base,
        &out,
        &named,
        Some(&picture),
        &registry,
        time(),
        |_, _| {},
    )
    .expect_err("wrong picture");
    // A picture given though the profile names none.
    build_image(
        &base,
        &out,
        &Profile::template(),
        Some(&picture),
        &registry,
        time(),
        |_, _| {},
    )
    .expect_err("unwanted picture");

    assert!(!out.exists(), "all three stop before the large copy");
    assert!(!registry.exists());
}
