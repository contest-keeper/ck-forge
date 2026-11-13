#[path = "../../ck-admin-core/tests/support/mod.rs"]
mod support;

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn forge(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ck-forge"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run ck-forge")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_new_profile_passes_its_own_check_and_is_never_replaced() {
    let dir = TempDir::new().expect("temp dir");
    assert!(
        forge(&["profile", "new", "p.json"], dir.path())
            .status
            .success()
    );
    assert!(
        forge(&["profile", "check", "p.json"], dir.path())
            .status
            .success()
    );

    let again = forge(&["profile", "new", "p.json"], dir.path());
    assert_eq!(again.status.code(), Some(1));
    assert!(text(&again.stderr).contains("already exists"));
}

#[test]
fn check_lists_every_problem_and_fails() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(
        dir.path().join("p.json"),
        r#"{"name":"x","settings":{"api":{"heartbeat_s":0},"team":""}}"#,
    )
    .expect("write");
    let result = forge(&["profile", "check", "p.json"], dir.path());
    assert_eq!(result.status.code(), Some(1));
    let stderr = text(&result.stderr);
    assert!(stderr.contains("heartbeat"), "{stderr}");
    assert!(stderr.contains("team"), "{stderr}");
}

#[test]
fn builds_shows_configures_and_lists_a_stick() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let base = base.to_str().expect("utf-8 path");
    assert!(
        forge(&["profile", "new", "p.json"], dir.path())
            .status
            .success()
    );

    let build = forge(
        &[
            "image",
            "build",
            "--base",
            base,
            "--out",
            "stick.img",
            "--profile",
            "p.json",
        ],
        dir.path(),
    );
    assert!(build.status.success(), "{}", text(&build.stderr));

    let shown = text(&forge(&["image", "show", "stick.img"], dir.path()).stdout);
    assert!(shown.contains("Stick:"), "{shown}");
    let registry = std::fs::read_to_string(dir.path().join("ck-registry.json")).expect("registry");
    let token = registry
        .split("\"token\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("a token in the registry");
    assert!(!shown.contains(token), "show must never print the secret");
    assert!(
        !text(&build.stdout).contains(token),
        "build must never print the secret"
    );

    let configure = forge(
        &["image", "configure", "stick.img", "--profile", "p.json"],
        dir.path(),
    );
    assert!(configure.status.success(), "{}", text(&configure.stderr));

    let list = text(&forge(&["registry", "list"], dir.path()).stdout);
    assert!(list.contains("stick.img"), "{list}");
}

#[test]
fn an_unconfigured_image_is_explained() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let result = forge(
        &["image", "show", base.to_str().expect("utf-8 path")],
        dir.path(),
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(text(&result.stderr).contains("no settings"));
}

#[test]
fn a_wrong_command_is_a_usage_error() {
    let dir = TempDir::new().expect("temp dir");
    assert_eq!(forge(&["bogus"], dir.path()).status.code(), Some(2));
    assert!(forge(&["--help"], dir.path()).status.success());
}

#[test]
fn a_stick_shows_its_contest_name_and_each_problem_is_listed_once() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    assert!(
        forge(&["profile", "new", "p.json"], dir.path())
            .status
            .success()
    );
    let built = forge(
        &[
            "image",
            "build",
            "--base",
            base.to_str().expect("utf-8 path"),
            "--out",
            "stick.img",
            "--profile",
            "p.json",
        ],
        dir.path(),
    );
    assert!(built.status.success(), "{}", text(&built.stderr));
    assert!(text(&built.stdout).contains("Contest:  My contest"));
    assert!(text(&built.stdout).contains("Computer: default-team-1 (machine 1)"));

    std::fs::write(
        dir.path().join("bad.json"),
        r#"{"name":"x","settings":{"api":{"heartbeat_s":0}}}"#,
    )
    .expect("write");
    let result = forge(&["profile", "check", "bad.json"], dir.path());
    let stderr = text(&result.stderr);
    assert_eq!(stderr.matches("heartbeat interval").count(), 1, "{stderr}");
}

#[test]
fn a_wallpaper_next_to_the_profile_goes_onto_the_stick_and_a_missing_one_is_found_early() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    assert!(
        forge(&["profile", "new", "p.json"], dir.path())
            .status
            .success()
    );
    let mut profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("p.json")).expect("read"))
            .expect("json");
    profile["settings"]["branding"]["wallpaper"] = "wall.png".into();
    std::fs::write(dir.path().join("p.json"), profile.to_string()).expect("write");

    // The picture is not there yet: the check says so before anything is built.
    let missing = forge(&["profile", "check", "p.json"], dir.path());
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        text(&missing.stderr).contains("wallpaper"),
        "{}",
        text(&missing.stderr)
    );

    std::fs::write(dir.path().join("wall.png"), b"\x89PNG picture").expect("picture");
    let checked = forge(&["profile", "check", "p.json"], dir.path());
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert!(
        text(&checked.stderr).contains("warning: no server address"),
        "an empty server address is only a warning: {}",
        text(&checked.stderr)
    );

    let built = forge(
        &[
            "image",
            "build",
            "--base",
            base.to_str().expect("utf-8 path"),
            "--out",
            "stick.img",
            "--profile",
            "p.json",
        ],
        dir.path(),
    );
    assert!(built.status.success(), "{}", text(&built.stderr));
    assert!(text(&built.stdout).contains("Wallpaper: wall.png"));
    assert_eq!(
        ck_image::read_file(&dir.path().join("stick.img"), "ck/wall.png").expect("on the stick"),
        b"\x89PNG picture"
    );
}

#[test]
fn a_built_stick_can_be_written_to_a_file_and_still_shows_its_settings() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    assert!(
        forge(&["profile", "new", "p.json"], dir.path())
            .status
            .success()
    );
    let built = forge(
        &[
            "image",
            "build",
            "--base",
            base.to_str().expect("utf-8 path"),
            "--out",
            "stick.img",
            "--profile",
            "p.json",
        ],
        dir.path(),
    );
    assert!(built.status.success(), "{}", text(&built.stderr));

    let written = forge(
        &["stick", "write", "stick.img", "--to", "copy.img"],
        dir.path(),
    );
    assert!(written.status.success(), "{}", text(&written.stderr));
    assert!(text(&written.stdout).contains("Read back and checked"));

    let original = text(&forge(&["image", "show", "stick.img"], dir.path()).stdout);
    let copy = text(&forge(&["image", "show", "copy.img"], dir.path()).stdout);
    assert_eq!(original, copy);
}

#[test]
fn a_disk_or_a_file_is_required() {
    let dir = TempDir::new().expect("temp dir");
    let none = forge(&["stick", "write", "stick.img"], dir.path());
    assert_eq!(none.status.code(), Some(2));
}

#[test]
fn the_progress_file_ends_with_the_hash_or_with_the_reason_for_a_failure() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(dir.path().join("a.img"), vec![7_u8; 4096]).expect("image");

    let done = forge(
        &[
            "stick",
            "write",
            "a.img",
            "--to",
            "b.img",
            "--progress-file",
            "p.json",
        ],
        dir.path(),
    );
    assert!(done.status.success(), "{}", text(&done.stderr));
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("p.json")).expect("progress"))
            .expect("json");
    assert_eq!(state["state"], "done");
    assert_eq!(state["sha256"].as_str().map(str::len), Some(64));

    let failed = forge(
        &[
            "stick",
            "write",
            "missing.img",
            "--to",
            "c.img",
            "--progress-file",
            "q.json",
        ],
        dir.path(),
    );
    assert!(!failed.status.success());
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("q.json")).expect("progress"))
            .expect("json");
    assert_eq!(state["state"], "error");
    assert!(
        state["message"]
            .as_str()
            .unwrap_or("")
            .contains("missing.img")
    );
}

#[test]
fn several_files_are_written_at_once() {
    let dir = TempDir::new().expect("temp dir");
    let image = vec![3_u8; 3 * 1024 * 1024 + 100];
    std::fs::write(dir.path().join("a.img"), &image).expect("image");

    let done = forge(
        &["stick", "write", "a.img", "--to", "b.img", "--to", "c.img"],
        dir.path(),
    );
    assert!(done.status.success(), "{}", text(&done.stderr));
    let out = text(&done.stdout);
    assert!(
        out.contains("to b.img") && out.contains("to c.img"),
        "{out}"
    );
    assert!(out.contains("Read back and checked"), "{out}");
    for name in ["b.img", "c.img"] {
        let written = std::fs::read(dir.path().join(name)).expect("written");
        assert_eq!(&written[..image.len()], &image[..]);
    }
}

#[test]
fn a_failing_stick_is_named_and_the_others_are_still_written() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(dir.path().join("a.img"), vec![5_u8; 4096]).expect("image");

    let result = forge(
        &[
            "stick",
            "write",
            "a.img",
            "--to",
            "good.img",
            "--to",
            "no-such-folder/bad.img",
        ],
        dir.path(),
    );
    assert!(!result.status.success());
    let message = text(&result.stderr);
    assert!(message.contains("1 of 2 sticks failed"), "{message}");
    assert!(message.contains("bad.img"), "{message}");
    assert_eq!(
        std::fs::read(dir.path().join("good.img")).expect("good"),
        vec![5_u8; 4096]
    );
}

#[test]
fn a_disk_cannot_be_given_together_with_a_file() {
    let dir = TempDir::new().expect("temp dir");
    let both = forge(
        &["stick", "write", "a.img", "--disk", "3", "--to", "b.img"],
        dir.path(),
    );
    assert_eq!(both.status.code(), Some(2));
}

#[test]
fn an_image_made_for_one_stick_is_not_written_to_several() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let profile = forge(&["profile", "new", "p.json"], dir.path());
    assert!(profile.status.success(), "{}", text(&profile.stderr));
    let built = forge(
        &[
            "image",
            "build",
            "--base",
            base.to_str().expect("utf-8 path"),
            "--out",
            "stick.img",
            "--profile",
            "p.json",
        ],
        dir.path(),
    );
    assert!(built.status.success(), "{}", text(&built.stderr));

    let several = forge(
        &[
            "stick",
            "write",
            "stick.img",
            "--to",
            "a.img",
            "--to",
            "b.img",
        ],
        dir.path(),
    );
    assert!(!several.status.success());
    assert!(
        text(&several.stderr).contains("share its id"),
        "{}",
        text(&several.stderr)
    );
    assert!(!dir.path().join("a.img").exists(), "nothing is written");

    let one = forge(
        &["stick", "write", "stick.img", "--to", "a.img"],
        dir.path(),
    );
    assert!(one.status.success(), "{}", text(&one.stderr));
}

#[test]
fn an_image_is_thin_unless_a_full_copy_is_asked_for() {
    let dir = TempDir::new().expect("temp dir");
    let base = support::make_image(&dir, "base.img");
    let base_len = std::fs::metadata(&base).expect("base").len();
    assert!(
        forge(&["profile", "new", "p.json"], dir.path())
            .status
            .success()
    );
    let build = |out: &str, extra: &[&str]| {
        let mut args = vec![
            "image",
            "build",
            "--base",
            base.to_str().expect("utf-8 path"),
            "--out",
            out,
            "--profile",
            "p.json",
        ];
        args.extend_from_slice(extra);
        forge(&args, dir.path())
    };

    let thin = build("thin.img", &[]);
    assert!(thin.status.success(), "{}", text(&thin.stderr));
    assert!(
        std::fs::metadata(dir.path().join("thin.img"))
            .expect("thin")
            .len()
            < base_len
    );
    assert!(dir.path().join("thin.img.thin.json").exists());

    let full = build("full.img", &["--full-copy"]);
    assert!(full.status.success(), "{}", text(&full.stderr));
    assert_eq!(
        std::fs::metadata(dir.path().join("full.img"))
            .expect("full")
            .len(),
        base_len
    );
    assert!(!dir.path().join("full.img.thin.json").exists());

    // Written, a thin image is a whole stick image, and shows its settings.
    let written = forge(
        &["stick", "write", "thin.img", "--to", "out.img"],
        dir.path(),
    );
    assert!(written.status.success(), "{}", text(&written.stderr));
    assert_eq!(
        std::fs::metadata(dir.path().join("out.img"))
            .expect("out")
            .len(),
        base_len
    );
    let shown = forge(&["image", "show", "out.img"], dir.path());
    assert!(shown.status.success(), "{}", text(&shown.stderr));
}

/// Two thin images of one base, made for two sticks, in a folder with the base.
fn two_sticks(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let base = support::make_image(dir, "base.img");
    assert!(
        forge(&["profile", "new", "p.json"], dir.path())
            .status
            .success()
    );
    for name in ["a.img", "b.img"] {
        let built = forge(
            &[
                "image",
                "build",
                "--base",
                base.to_str().expect("utf-8 path"),
                "--out",
                name,
                "--profile",
                "p.json",
            ],
            dir.path(),
        );
        assert!(built.status.success(), "{}", text(&built.stderr));
    }
    (dir.path().join("a.img"), base)
}

fn stick_of(dir: &TempDir, image: &str) -> String {
    let shown = forge(&["image", "show", image], dir.path());
    assert!(shown.status.success(), "{}", text(&shown.stderr));
    text(&shown.stdout)
        .lines()
        .find(|line| line.starts_with("Stick:"))
        .expect("a Stick line")
        .to_owned()
}

#[test]
fn several_sticks_each_with_its_own_image_are_written_together_with_their_own_identity() {
    let dir = TempDir::new().expect("temp dir");
    let (_, base) = two_sticks(&dir);
    let base_len = std::fs::metadata(&base).expect("base").len();

    let done = forge(
        &[
            "stick", "write", "a.img", "b.img", "--to", "ta.img", "--to", "tb.img",
        ],
        dir.path(),
    );
    assert!(done.status.success(), "{}", text(&done.stderr));
    assert!(
        text(&done.stdout).contains("against its own image"),
        "{}",
        text(&done.stdout)
    );
    for name in ["ta.img", "tb.img"] {
        assert_eq!(
            std::fs::metadata(dir.path().join(name)).expect(name).len(),
            base_len
        );
    }
    let (a, b) = (stick_of(&dir, "ta.img"), stick_of(&dir, "tb.img"));
    assert_ne!(a, b, "two sticks, two identities");
    assert_eq!(a, stick_of(&dir, "a.img"));
    assert_eq!(b, stick_of(&dir, "b.img"));
}

#[test]
fn images_and_sticks_that_do_not_match_are_refused_before_anything_is_written() {
    let dir = TempDir::new().expect("temp dir");
    two_sticks(&dir);
    let refused = |args: &[&str]| {
        let result = forge(args, dir.path());
        assert!(!result.status.success(), "{args:?}");
        text(&result.stderr)
    };

    let count = refused(&["stick", "write", "a.img", "b.img", "--to", "t1.img"]);
    assert!(count.contains("2 images for 1 sticks"), "{count}");
    let twice = refused(&[
        "stick", "write", "a.img", "a.img", "--to", "t1.img", "--to", "t2.img",
    ]);
    assert!(twice.contains("given twice"), "{twice}");

    // An image of another base.
    let other = TempDir::new().expect("other dir");
    let other_base = support::make_image(&other, "base.img");
    let built = forge(
        &[
            "image",
            "build",
            "--base",
            other_base.to_str().expect("utf-8 path"),
            "--out",
            dir.path().join("c.img").to_str().expect("utf-8 path"),
            "--profile",
            "p.json",
        ],
        dir.path(),
    );
    assert!(built.status.success(), "{}", text(&built.stderr));
    let mixed = refused(&[
        "stick", "write", "a.img", "c.img", "--to", "t1.img", "--to", "t2.img",
    ]);
    assert!(mixed.contains("same base"), "{mixed}");

    for name in ["t1.img", "t2.img"] {
        assert!(!dir.path().join(name).exists(), "{name} was written");
    }
}
