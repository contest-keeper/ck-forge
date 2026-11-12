//! A small fake disk image: a GPT with a FAT32 EFI partition and a Linux partition.

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;

use ck_image::{Guid, SECTOR_SIZE};
use fatfs::{FatType, FormatVolumeOptions};
use fscommon::StreamSlice;
use tempfile::TempDir;

const IMAGE_SIZE: u64 = 96 * 1024 * 1024;
const EFI_FIRST: u64 = 2048;
const EFI_LAST: u64 = EFI_FIRST + 48 * 1024 * 1024 / SECTOR_SIZE - 1;
const ROOT_FIRST: u64 = EFI_LAST + 1;
const ROOT_LAST: u64 = IMAGE_SIZE / SECTOR_SIZE - 34;

pub fn make_image(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("create image");
    file.set_len(IMAGE_SIZE).expect("size image");
    write_gpt(&mut file);
    file.seek(SeekFrom::Start(ROOT_FIRST * SECTOR_SIZE))
        .expect("seek root");
    file.write_all(&[0xAB; 4096]).expect("fill root");
    let slice = StreamSlice::new(file, EFI_FIRST * SECTOR_SIZE, (EFI_LAST + 1) * SECTOR_SIZE)
        .expect("slice");
    let options = FormatVolumeOptions::new()
        .fat_type(FatType::Fat32)
        .volume_label(*b"EFI        ");
    fatfs::format_volume(slice, options).expect("format EFI");
    path
}

fn write_gpt(file: &mut File) {
    let mut entries = vec![0_u8; 128 * 128];
    for (index, (guid, unique, first, last)) in [
        (Guid::EFI_SYSTEM, 1_u8, EFI_FIRST, EFI_LAST),
        (Guid::LINUX_FILESYSTEM, 2_u8, ROOT_FIRST, ROOT_LAST),
    ]
    .into_iter()
    .enumerate()
    {
        let entry = &mut entries[index * 128..(index + 1) * 128];
        entry[..16].copy_from_slice(&guid.to_disk_bytes());
        entry[16..32].copy_from_slice(&[unique; 16]);
        entry[32..40].copy_from_slice(&first.to_le_bytes());
        entry[40..48].copy_from_slice(&last.to_le_bytes());
    }
    let mut header = [0_u8; 92];
    header[..8].copy_from_slice(b"EFI PART");
    header[8..12].copy_from_slice(&0x0001_0000_u32.to_le_bytes());
    header[12..16].copy_from_slice(&92_u32.to_le_bytes());
    header[24..32].copy_from_slice(&1_u64.to_le_bytes());
    header[72..80].copy_from_slice(&2_u64.to_le_bytes());
    header[80..84].copy_from_slice(&128_u32.to_le_bytes());
    header[84..88].copy_from_slice(&128_u32.to_le_bytes());
    header[88..92].copy_from_slice(&crc32fast::hash(&entries).to_le_bytes());
    let crc = crc32fast::hash(&header);
    header[16..20].copy_from_slice(&crc.to_le_bytes());
    file.seek(SeekFrom::Start(SECTOR_SIZE)).expect("seek");
    file.write_all(&header).expect("header");
    file.seek(SeekFrom::Start(2 * SECTOR_SIZE)).expect("seek");
    file.write_all(&entries).expect("entries");
}
