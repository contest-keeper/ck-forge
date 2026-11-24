#!/usr/bin/env bash
# Tries the real macOS write path without a stick: attaches a scratch file as a disk image, writes a
# test image to it with `ck-forge stick write --allow-virtual`, and compares the result.
# Touches only the disk that this script attached itself. Needs no sudo (the disk belongs to you).
# Usage: tools/test-writer-disk-image.sh [path-to-ck-forge]
set -euo pipefail

forge="${1:-target/debug/ck-forge}"
work="$(mktemp -d)"
disk=""
cleanup() {
  if [ -n "$disk" ]; then hdiutil detach "$disk" -quiet || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

dd if=/dev/zero of="$work/target.img" bs=1m count=64 2>/dev/null
# A size that is not a multiple of the 4 MiB block, to cover the short last block.
dd if=/dev/urandom of="$work/stick.img" bs=1024 count=9728 2>/dev/null

disk="$(hdiutil attach -nomount -imagekey diskimage-class=CRawDiskImage "$work/target.img" | awk 'NR==1 {print $1}')"
case "$disk" in
  /dev/disk[0-9]*) ;;
  *) echo "unexpected answer from hdiutil: $disk" >&2; disk=""; exit 1 ;;
esac
number="${disk#/dev/disk}"
echo "attached $work/target.img as $disk"

"$forge" stick list | grep "^disk $number "
"$forge" stick write "$work/stick.img" --disk "$number" --allow-virtual --yes

size="$(stat -f %z "$work/stick.img")"
cmp -n "$size" "$work/stick.img" "/dev/rdisk$number"
echo "OK: the first $size bytes of $disk equal the image"
