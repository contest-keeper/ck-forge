#!/usr/bin/env bash
# Adds the macOS disk image to the update feed that make-update-manifest.sh wrote for Windows.
# Usage: add-macos-update.sh <version> <dmg url> <dmg .sig file> <manifest>
set -euo pipefail

version=$1
url=$2
signature_file=$3
manifest=$4
# A missing signature would publish a feed that every Mac rejects.
test -s "$signature_file" || { echo "add-macos-update: no signature in $signature_file" >&2; exit 1; }
# The feed has one version for all systems, so a disk image of another build must not join it.
jq -e --arg version "$version" '.version == $version' "$manifest" > /dev/null ||
  { echo "add-macos-update: $manifest is not for version $version" >&2; exit 1; }

jq --arg url "$url" --arg signature "$(cat "$signature_file")" \
  '.platforms["darwin-universal"] = {signature: $signature, url: $url}' "$manifest" > "$manifest.new"
mv "$manifest.new" "$manifest"
