use std::fs;
use std::path::{Path, PathBuf};

use ck_writer::{Error, Phase, disk_path, thin, write_image, write_image_to_all, write_images};
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

#[test]
fn writes_several_targets_at_once_and_reports_each() {
    let dir = tempdir().unwrap();
    // Several blocks and a short last one, so every target gets a padded end.
    let source = image(dir.path(), 9 * 1024 * 1024 + 1000);
    let targets: Vec<PathBuf> = (0..3)
        .map(|n| dir.path().join(format!("stick{n}.img")))
        .collect();

    let mut seen = vec![[0_u64; 2]; targets.len()];
    let results = write_image_to_all(&source, &targets, |index, phase, done, total| {
        assert!(done <= total);
        let slot = usize::from(phase == Phase::Verifying);
        seen[index][slot] = seen[index][slot].max(done);
    })
    .unwrap();

    let expected = fs::read(&source).unwrap();
    assert_eq!(results.len(), 3);
    let hashes: Vec<&String> = results.iter().map(|r| r.as_ref().unwrap()).collect();
    assert!(hashes.iter().all(|hash| *hash == hashes[0]));
    for (target, done) in targets.iter().zip(&seen) {
        let written = fs::read(target).unwrap();
        assert_eq!(&written[..expected.len()], &expected[..]);
        assert_eq!(written.len(), expected.len().next_multiple_of(512));
        // Every target was reported to the end of both phases.
        assert_eq!(*done, [expected.len() as u64; 2]);
    }
}

#[test]
fn one_target_that_cannot_be_opened_does_not_stop_the_others() {
    let dir = tempdir().unwrap();
    let source = image(dir.path(), 3 * 1024 * 1024);
    let good = dir.path().join("good.img");
    let bad = dir.path().join("no-such-folder").join("bad.img");

    let results = write_image_to_all(&source, &[bad, good.clone()], |_, _, _, _| {}).unwrap();

    assert!(matches!(&results[0], Err(Error::Io { .. })));
    assert!(results[1].is_ok());
    assert_eq!(fs::read(&good).unwrap(), fs::read(&source).unwrap());
}

#[test]
fn no_targets_give_no_results() {
    let dir = tempdir().unwrap();
    let source = image(dir.path(), 4096);
    assert!(
        write_image_to_all(&source, &[], |_, _, _, _| {})
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_empty_image_is_refused_for_several_targets_too() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("empty.img");
    fs::write(&source, b"").unwrap();
    let targets = [dir.path().join("a.img"), dir.path().join("b.img")];
    let error = write_image_to_all(&source, &targets, |_, _, _, _| {}).unwrap_err();
    assert!(matches!(error, Error::EmptyImage));
    assert!(!targets[0].exists());
}

/// A thin image of `base` whose start has `mark` written into it, as the settings of a stick would.
fn stick(dir: &Path, base: &Path, name: &str, head_len: u64, mark: u8) -> PathBuf {
    let path = dir.join(name);
    thin::create(base, &path, head_len, |_, _| {}).expect("make the thin image");
    let mut head = fs::read(&path).expect("read the head");
    head[700..720].fill(mark);
    fs::write(&path, head).expect("change the head");
    path
}

#[test]
fn sticks_with_their_own_start_are_written_from_one_base_each_with_its_own_hash() {
    let dir = tempdir().unwrap();
    // The start is not a whole number of blocks and the end is not a whole sector.
    let base = image(dir.path(), 9 * 1024 * 1024 + 700);
    let head_len: u64 = 1024 * 1024 + 512;
    let head = usize::try_from(head_len).expect("fits");
    let jobs: Vec<(PathBuf, PathBuf)> = [0x11_u8, 0x22, 0x33]
        .iter()
        .enumerate()
        .map(|(n, mark)| {
            (
                stick(dir.path(), &base, &format!("stick{n}.img"), head_len, *mark),
                dir.path().join(format!("target{n}.img")),
            )
        })
        .collect();

    let mut done = vec![[0_u64; 2]; jobs.len()];
    let results = write_images(&jobs, |index, phase, bytes, total| {
        assert!(bytes <= total);
        let slot = usize::from(phase == Phase::Verifying);
        done[index][slot] = done[index][slot].max(bytes);
    })
    .unwrap();

    let base_bytes = fs::read(&base).unwrap();
    let hashes: Vec<&String> = results.iter().map(|r| r.as_ref().unwrap()).collect();
    assert!(
        hashes[0] != hashes[1] && hashes[1] != hashes[2],
        "each stick has its own hash"
    );
    for (n, (image, target)) in jobs.iter().enumerate() {
        let mut expected = base_bytes.clone();
        expected[..head].copy_from_slice(&fs::read(image).unwrap());
        let written = fs::read(target).unwrap();
        assert_eq!(&written[..expected.len()], &expected[..], "target {n}");
        assert_eq!(written.len(), expected.len().next_multiple_of(512));
        assert_eq!(
            done[n],
            [expected.len() as u64; 2],
            "progress of target {n}"
        );
    }
    // The single-stick call gives the same hash as the batch for the same image.
    let alone = dir.path().join("alone.img");
    let hash = write_image(&jobs[1].0, &alone, |_, _, _| {}).unwrap();
    assert_eq!(&hash, hashes[1]);
}

#[test]
fn images_of_different_bases_are_refused_before_anything_is_written() {
    let dir = tempdir().unwrap();
    let base_a = image(dir.path(), 4 * 1024 * 1024);
    let other = dir.path().join("other");
    fs::create_dir(&other).unwrap();
    let base_b = image(&other, 4 * 1024 * 1024);
    let jobs = vec![
        (
            stick(dir.path(), &base_a, "a.img", 1024, 1),
            dir.path().join("ta.img"),
        ),
        (
            stick(&other, &base_b, "b.img", 1024, 2),
            dir.path().join("tb.img"),
        ),
    ];
    let error = write_images(&jobs, |_, _, _, _| {}).unwrap_err();
    assert!(matches!(error, Error::Thin(_)), "{error}");
    assert!(!jobs[0].1.exists() && !jobs[1].1.exists());
}

#[test]
fn a_stick_whose_target_fails_does_not_stop_the_other_sticks() {
    let dir = tempdir().unwrap();
    let base = image(dir.path(), 3 * 1024 * 1024);
    let jobs = vec![
        (
            stick(dir.path(), &base, "a.img", 1024, 1),
            dir.path().join("no-such-folder").join("ta.img"),
        ),
        (
            stick(dir.path(), &base, "b.img", 1024, 2),
            dir.path().join("tb.img"),
        ),
    ];
    let results = write_images(&jobs, |_, _, _, _| {}).unwrap();
    assert!(results[0].is_err());
    assert!(results[1].is_ok());
    let written = fs::read(&jobs[1].1).unwrap();
    assert_eq!(written[700..720], [2_u8; 20]);
    assert_eq!(written[1024..], fs::read(&base).unwrap()[1024..]);
}
