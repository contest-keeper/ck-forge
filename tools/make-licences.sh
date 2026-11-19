#!/usr/bin/env bash
# Writes the THIRD-PARTY-LICENSES file for the Forge installer and the Forge command line tool:
# the licences of the Rust crates of both workspaces and of the npm packages in the window.
# Usage: make-licences.sh <output file>   (run from the repository root, after `pnpm install` in ui)
#
# Fails when cargo-about finds a licence that is not allowed, or when the result is empty.
set -euo pipefail

output=$1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo about generate --locked about.hbs -o "$work/cli.txt"
(cd app && cargo about generate --locked ../about.hbs -o "$work/window.txt")
(cd ui && node scripts/licences.mjs > "$work/npm.txt")

{
  echo "Keeper Forge: third-party licences"
  echo
  echo "Part 1: the window program"
  cat "$work/window.txt"
  echo
  echo "Part 2: the command line tool"
  cat "$work/cli.txt"
  echo
  echo "Part 3: the web page of the window"
  cat "$work/npm.txt"
} > "$output"

# A file with no licence text, or without the usual ones, means something went wrong above.
for expected in "Apache License" "MIT License" "Permission is hereby granted"; do
  if ! grep -q "$expected" "$output"; then
    echo "make-licences: '$expected' is missing from $output" >&2
    exit 1
  fi
done
echo "wrote $output ($(wc -c < "$output") bytes)"
