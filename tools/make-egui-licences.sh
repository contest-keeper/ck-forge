#!/usr/bin/env bash
# Writes the THIRD-PARTY-LICENSES file for the egui window and the Forge command line tool that is
# installed with it: the licences of the Rust crates of both, for Windows and for macOS.
# Usage: make-egui-licences.sh <output file>   (run from the repository root)
#
# Fails when cargo-about finds a licence that app-egui/about.toml does not allow.
set -euo pipefail

output=$1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

(cd app-egui && cargo about generate --locked -c about.toml ../about.hbs -o "$work/window.txt")
cargo about generate --locked -c app-egui/about.toml about.hbs -o "$work/tool.txt"

{
  echo "Keeper Forge: third-party licences"
  echo
  echo "Part 1: the window program"
  cat "$work/window.txt"
  echo
  echo "Part 2: the command line tool"
  cat "$work/tool.txt"
} > "$output"

# A file with no licence text, or without the usual ones, means something went wrong above.
test -s "$output"
grep -q "Apache License" "$output"
grep -q "MIT" "$output"
