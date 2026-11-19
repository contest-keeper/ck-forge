use std::fs;

use ck_base_image::{Error, import_image, sha256_file};
use tempfile::tempdir;

#[test]
fn imports_only_a_verified_archive_and_preserves_existing_files() {
    let dir = tempdir().expect("temp folder");
    let archive = dir.path().join("base.img.zst");
    let out = dir.path().join("base.img");
    let raw = b"the uncompressed disk image";
    let bytes = zstd::encode_all(&raw[..], 1).expect("compress image");
    fs::write(&archive, &bytes).expect("save the archive");
    let digest = sha256_file(&archive).expect("checksum");

    let wrong = import_image(&archive, &out, &"0".repeat(64), |_, _| {});
    assert!(matches!(wrong, Err(Error::Checksum { .. })));
    assert!(!out.exists());

    let mut progress = Vec::new();
    import_image(&archive, &out, &digest.to_uppercase(), |done, total| {
        progress.push((done, total));
    })
    .expect("import image");
    assert_eq!(fs::read(&out).expect("uncompressed image"), raw);
    let work = bytes.len() as u64 * 2;
    assert_eq!(progress.last(), Some(&(work, work)));

    let repeated = import_image(&archive, &out, &digest, |_, _| {});
    assert!(matches!(repeated, Err(Error::Exists(_))));
    assert_eq!(fs::read(&out).expect("uncompressed image"), raw);
}

#[test]
fn a_truncated_or_malformed_archive_leaves_no_output() {
    let dir = tempdir().expect("temp folder");
    let archive = dir.path().join("base.img.zst");
    let out = dir.path().join("base.img");
    let mut bytes = zstd::encode_all(&b"disk image"[..], 1).expect("compress image");
    bytes.pop();
    fs::write(&archive, bytes).expect("save the archive");

    let digest = sha256_file(&archive).expect("checksum");
    assert!(import_image(&archive, &out, &digest, |_, _| {}).is_err());
    assert!(import_image(&archive, &out, "abc", |_, _| {}).is_err());
    assert_eq!(fs::read_dir(dir.path()).expect("list files").count(), 1);
}

#[test]
fn a_wrong_checksum_is_rejected_before_decoding() {
    let dir = tempdir().expect("temp folder");
    let archive = dir.path().join("base.img.zst");
    let out = dir.path().join("base.img");
    fs::write(&archive, b"not a Zstandard archive").expect("save malformed archive");

    let result = import_image(&archive, &out, &"0".repeat(64), |_, _| {});

    assert!(matches!(result, Err(Error::Checksum { .. })));
    assert_eq!(fs::read_dir(dir.path()).expect("list files").count(), 1);
}

#[test]
fn a_valid_archive_with_an_empty_image_leaves_no_output() {
    let dir = tempdir().expect("temp folder");
    let archive = dir.path().join("base.img.zst");
    let out = dir.path().join("base.img");
    let bytes = zstd::encode_all(&[][..], 1).expect("compress empty image");
    fs::write(&archive, bytes).expect("save the archive");
    let digest = sha256_file(&archive).expect("checksum");

    let result = import_image(&archive, &out, &digest, |_, _| {});

    assert!(matches!(result, Err(Error::EmptyArchive)));
    assert_eq!(fs::read_dir(dir.path()).expect("list files").count(), 1);
}
