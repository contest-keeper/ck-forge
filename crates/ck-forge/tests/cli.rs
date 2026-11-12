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
