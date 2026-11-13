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

Only USB disks are written. The disk Windows runs from, internal drives (SATA, NVMe), disks smaller
than the image and virtual disks (unless `--allow-virtual`) are refused. Before writing, the disk is
looked up again and must still be the same one that was listed.

A "32 GB" stick holds about 32,000,000,000 bytes or less. The image is 29,884,416,000 bytes, so use
sticks of at least 32 GB; the writer says "use a larger stick" when one is too small.
