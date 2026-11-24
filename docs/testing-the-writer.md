# Testing the stick writer without a stick

Four levels, from safest to most real. Only the last one touches a real stick, and you do it.

1. **Unit and CLI tests** (`cargo test`): write to a plain file and read it back. No admin rights.
2. **A file as the target**: `ck-forge stick write stick.img --to copy.img` runs the same code that
   writes a disk (4 MiB blocks, SHA-256 check after reading back), only on a file.
3. **A virtual disk**: `tools/test-writer-vhd.ps1` makes a VHD, attaches it, and writes it with
   `--disk N --allow-virtual`. This exercises the real path: listing, the safety checks, taking the
   disk offline and writing `\\.\PhysicalDriveN`. Run it in an administrator PowerShell. It needs free
   space equal to the image (about 30 GB).
4. **A real stick** (A.9): `ck-forge stick list`, pick the number of your stick, then
   `ck-forge stick write stick.img --disk N` in an administrator terminal. You type `erase N` to confirm.

## What the writer refuses

Only USB disks are written. The disk the computer runs from, internal drives (SATA, NVMe), disks
smaller than the image and virtual disks (unless `--allow-virtual`) are refused. Before writing, the
disk is looked up again and must still be the same one that was listed.

On macOS the rules are the same, judged from `diskutil info -plist`: the disk must be external,
physical, removable media on USB with 512-byte sectors. Refused besides: the disk `/` lives on and
the disk under its APFS container, internal disks, APFS containers, USB drives that are not
removable (an external SSD) and disks with 4096-byte sectors. These rules are tested with recorded
plists (`crates/ck-writer/fixtures/diskutil`); no test touches a disk.

## macOS

The writer uses `diskutil` to list disks and unmount the stick, then writes `/dev/rdiskN` (the
unbuffered device) in the same 4 MiB blocks as on Windows and reads it back.

- `cargo test` runs everything that needs no disk, including the plist tests.
- `tools/test-writer-disk-image.sh` attaches a 64 MB scratch file with `hdiutil`, writes a test image
  to it with `--allow-virtual` and compares the bytes. Needs no `sudo` (the attached disk is yours)
  and touches only the disk it attached.
- A real stick needs `sudo`: the raw device belongs to root. Forge says so before it unmounts anything.

```sh
ck-forge stick list                       # find the number of your stick (diskN)
sudo ck-forge stick write stick.img --disk N
```

### Manual test with a real stick on macOS (checklist for the user)

Use a stick you can erase, at least as large as the image, plugged in directly.

1. `ck-forge stick list`: your stick shows `USB` and `[can be written]`; the internal disks and the
   disk you boot from say `refused`. If your stick says refused, stop and read why.
2. Without `sudo`: `ck-forge stick write stick.img --disk N` stops with "run the command again with
   sudo", and the stick is still mounted in Finder.
3. With `sudo`: type `erase N` when asked. The bars show writing, then checking; it ends with
   "Read back and checked, SHA-256 ...".
4. `diskutil list` shows the stick with the image's partitions (macOS may offer to eject or ignore
   volumes it cannot read: click Ignore).
5. Pull the stick, plug it in again, run `ck-forge stick write` once more: it is quicker, because
   unchanged blocks are skipped, and ends with the same hash.
6. Try `--disk` with the number of your internal disk: refused, nothing is written.
7. Boot a laptop from the stick (the Keeper OS part, see ck-os).

A "32 GB" stick holds about 32,000,000,000 bytes or less. The image is 29,884,416,000 bytes, so use
sticks of at least 32 GB; the writer says "use a larger stick" when one is too small.
