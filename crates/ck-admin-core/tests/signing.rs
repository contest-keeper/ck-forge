//! An image that carries public keys only accepts settings the Hub signed, so the builders ask the
//! Hub (through the signer they are given) to sign the exact bytes they write, and write the
//! signature next to them.

mod support;

use std::cell::RefCell;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ck_admin_core::{
    EnrolmentBuild, Error, Profile, build_enrolment_image, build_thin_image,
    build_thin_image_signed, configure_image, configure_image_signed, requires_signed_settings,
};
use ck_protocol::{EnrolmentCode, LocalDateTime};
use tempfile::TempDir;

const SIGNED_BASE: &[u8] = include_bytes!("fixtures/signed.ext2");
const SIGNATURE_FILE: &str = "config.json.sig";

fn time() -> LocalDateTime {
    "2026-11-10T15:22".parse().expect("valid time")
}

/// A base image with a `CKCONFIG` partition and, when `keys` is set, a Linux system that carries the
/// key list (and supports first-boot enrolment).
fn base(dir: &TempDir, keys: bool) -> PathBuf {
    let base = support::make_image_with_config(dir, "base.img");
    if keys {
        let mut disk = std::fs::OpenOptions::new()
            .write(true)
            .open(&base)
            .expect("disk");
        disk.seek(SeekFrom::Start(support::CONFIG_END))
            .expect("root");
        disk.write_all(SIGNED_BASE).expect("ext filesystem");
    }
    base
}

fn read(image: &Path, name: &str) -> Vec<u8> {
    ck_image::read_config_file(image, name).expect("a file on the config partition")
}

#[test]
fn an_image_that_carries_keys_says_so() {
    let dir = TempDir::new().expect("temp dir");
    assert!(requires_signed_settings(&base(&dir, true)).expect("detect"));
    let plain = TempDir::new().expect("temp dir");
    assert!(!requires_signed_settings(&base(&plain, false)).expect("detect"));
}

#[test]
fn settings_for_such_an_image_cannot_be_made_without_a_signer() {
    let dir = TempDir::new().expect("temp dir");
    let base = base(&dir, true);
    let (out, registry) = (dir.path().join("stick.img"), dir.path().join("r.json"));
    let error = build_thin_image(
        &base,
        &out,
        &Profile::template(),
        None,
        &registry,
        time(),
        |_, _| {},
    )
    .unwrap_err();
    assert!(matches!(error, Error::SignedSettingsRequired), "{error}");
    // Nothing was made or reserved.
    assert!(!out.exists() && !registry.exists());

    // First-boot enrolment stops before it asks the Hub for a code.
    let mut profile = Profile::template();
    profile.settings.api.url = "https://hub.example.test".to_owned();
    let options = EnrolmentBuild {
        base: &base,
        out: &out,
        profile: &profile,
        wallpaper: None,
        registry: &registry,
        now: time(),
        sign: None,
    };
    let error =
        build_enrolment_image(&options, |_, _, _| panic!("must not mint"), |_, _| {}).unwrap_err();
    assert!(matches!(error, Error::SignedSettingsRequired), "{error}");
    assert!(!out.exists() && !registry.exists());
}

#[test]
fn the_signature_is_made_over_the_exact_bytes_that_are_written() {
    let dir = TempDir::new().expect("temp dir");
    let base = base(&dir, true);
    let (out, registry) = (dir.path().join("stick.img"), dir.path().join("r.json"));
    let seen = RefCell::new(Vec::new());
    let signer = |bytes: &[u8]| {
        seen.borrow_mut().push(bytes.to_vec());
        Ok("fixture-signature\n".to_owned())
    };
    build_thin_image_signed(
        &base,
        &out,
        &Profile::template(),
        None,
        &registry,
        time(),
        Some(&signer),
        |_, _| {},
    )
    .expect("a signed thin image");
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(read(&out, "config.json"), seen.borrow()[0]);
    assert_eq!(read(&out, SIGNATURE_FILE), b"fixture-signature\n");

    // The same for a stick that enrols at its first boot.
    let enrolling = dir.path().join("enrolling.img");
    let mut profile = Profile::template();
    profile.settings.api.url = "https://hub.example.test".to_owned();
    let options = EnrolmentBuild {
        base: &base,
        out: &enrolling,
        profile: &profile,
        wallpaper: None,
        registry: &dir.path().join("e.json"),
        now: time(),
        sign: Some(&signer),
    };
    build_enrolment_image(
        &options,
        |_, _, _| Ok(EnrolmentCode::parse("ABCD-EFGH-JKLM-NPQR-STUV").expect("code")),
        |_, _| {},
    )
    .expect("a signed enrolment image");
    assert_eq!(seen.borrow().len(), 2);
    assert_eq!(read(&enrolling, "config.json"), seen.borrow()[1]);
    assert_eq!(read(&enrolling, SIGNATURE_FILE), b"fixture-signature\n");
}

#[test]
fn a_hub_that_cannot_sign_leaves_nothing_behind() {
    let dir = TempDir::new().expect("temp dir");
    let base = base(&dir, true);
    let (out, registry) = (dir.path().join("stick.img"), dir.path().join("r.json"));
    let refuse = |_: &[u8]| Err(Error::Hub("no signing key".to_owned()));
    let error = build_thin_image_signed(
        &base,
        &out,
        &Profile::template(),
        None,
        &registry,
        time(),
        Some(&refuse),
        |_, _| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("no signing key"), "{error}");
    assert!(!out.exists() && !registry.exists());
}

#[test]
fn an_image_without_keys_is_never_signed_and_the_signer_is_not_asked() {
    let dir = TempDir::new().expect("temp dir");
    let base = base(&dir, false);
    let out = dir.path().join("stick.img");
    let never = |_: &[u8]| -> Result<String, Error> { panic!("a plain image needs no signature") };
    build_thin_image_signed(
        &base,
        &out,
        &Profile::template(),
        None,
        &dir.path().join("r.json"),
        time(),
        Some(&never),
        |_, _| {},
    )
    .expect("a plain thin image");
    assert!(ck_image::read_config_file(&out, SIGNATURE_FILE).is_err());
}

#[test]
fn changing_the_settings_of_a_signed_image_signs_them_again() {
    let dir = TempDir::new().expect("temp dir");
    let base = base(&dir, true);
    let (out, registry) = (dir.path().join("stick.img"), dir.path().join("r.json"));
    let first = |_: &[u8]| Ok("first-signature\n".to_owned());
    build_thin_image_signed(
        &base,
        &out,
        &Profile::template(),
        None,
        &registry,
        time(),
        Some(&first),
        |_, _| {},
    )
    .expect("a signed thin image");

    // A thin image holds only the start of the base, yet its signature file shows it is a signed one.
    let mut changed = Profile::template();
    changed.settings.team = "Another team".to_owned();
    let error = configure_image(&out, &changed, None, &registry).unwrap_err();
    assert!(matches!(error, Error::SignedSettingsRequired), "{error}");
    assert_eq!(read(&out, SIGNATURE_FILE), b"first-signature\n");

    let seen = RefCell::new(Vec::new());
    let second = |bytes: &[u8]| {
        seen.borrow_mut().push(bytes.to_vec());
        Ok("second-signature\n".to_owned())
    };
    configure_image_signed(&out, &changed, None, &registry, Some(&second)).expect("configured");
    assert_eq!(read(&out, "config.json"), seen.borrow()[0]);
    assert_eq!(read(&out, SIGNATURE_FILE), b"second-signature\n");
}
