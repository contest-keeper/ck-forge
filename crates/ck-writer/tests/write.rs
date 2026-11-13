use std::fs;
use std::path::{Path, PathBuf};

use ck_writer::{Error, Phase, disk_path, write_image};
use tempfile::tempdir;

fn image(dir: &Path, size: usize) -> PathBuf {
    let path = dir.join("image.img");
    let bytes: Vec<u8> = (0..size)
        .map(|i| u8::try_from(i % 251).expect("fits"))
        .collect();
    fs::write(&path, bytes).expect("write image");
    path
}

#[test]
fn writes_a_file_target_and_reads_it_back() {
    let dir = tempdir().unwrap();
    // Not a multiple of the 4 MiB block, to cover the short last block.
    let source = image(dir.path(), 9 * 1024 * 1024 + 512);
    let target = dir.path().join("stick.img");

    let mut phases = Vec::new();
    let hash = write_image(&source, &target, |phase, done, total| {
        phases.push((phase, done, total));
    })
    .unwrap();

    assert_eq!(fs::read(&source).unwrap(), fs::read(&target).unwrap());
    assert_eq!(hash.len(), 64);
    assert_eq!(phases.first().unwrap().0, Phase::Writing);
    let last = phases.last().unwrap();
    assert_eq!((last.0, last.1), (Phase::Verifying, last.2));
}

#[test]
fn pads_the_last_block_to_a_whole_sector() {
    let dir = tempdir().unwrap();
    let source = image(dir.path(), 1000);
    let target = dir.path().join("stick.img");
    write_image(&source, &target, |_, _, _| {}).unwrap();
    let written = fs::read(&target).unwrap();
    assert_eq!(written.len(), 1024);
    assert_eq!(&written[..1000], &fs::read(&source).unwrap()[..]);
    assert!(written[1000..].iter().all(|b| *b == 0));
}

#[test]
fn keeps_the_size_of_a_larger_target() {
    let dir = tempdir().unwrap();
    let source = image(dir.path(), 2048);
    let target = dir.path().join("stick.img");
    fs::write(&target, vec![0xFF; 8192]).unwrap();
    write_image(&source, &target, |_, _, _| {}).unwrap();
    let after = fs::read(&target).unwrap();
    assert_eq!(after.len(), 8192);
    assert_eq!(&after[..2048], &fs::read(&source).unwrap()[..]);
}

#[test]
fn a_disk_is_opened_by_its_device_path_not_as_a_file_on_the_current_drive() {
    // With one backslash less, Windows reads this as `C:\PhysicalDrive3`, an ordinary file.
    assert_eq!(disk_path(3).to_str(), Some(r"\\.\PhysicalDrive3"));
}

#[test]
fn an_empty_image_is_refused() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("empty.img");
    fs::write(&source, b"").unwrap();
    let error = write_image(&source, &dir.path().join("t.img"), |_, _, _| {}).unwrap_err();
    assert!(matches!(error, Error::EmptyImage));
}

#[test]
fn a_missing_image_is_reported() {
    let dir = tempdir().unwrap();
    let error = write_image(
        &dir.path().join("none.img"),
        &dir.path().join("t"),
        |_, _, _| {},
    )
    .unwrap_err();
    assert!(matches!(error, Error::Io { .. }));
}
