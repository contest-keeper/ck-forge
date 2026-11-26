#!/usr/bin/env bash
# Writes the file installed copies read to learn about a new version.
# Usage: make-update-manifest.sh <version> <installer url> <installer .sig file> <min-version file> <output>
#
# "min_version" is the oldest version still allowed to run: a copy older than it must update.
set -euo pipefail

version=$1
url=$2
signature_file=$3
min_version=$(tr -d '[:space:]' < "$4")
output=$5
# A missing signature would publish a feed that every installed copy rejects.
test -s "$signature_file" || { echo "make-update-manifest: no signature in $signature_file" >&2; exit 1; }

jq -n \
  --arg version "$version" \
  --arg url "$url" \
  --arg signature "$(cat "$signature_file")" \
  --arg min "$min_version" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  '{version: $version, pub_date: $date, min_version: $min,
    platforms: {"windows-x86_64": {signature: $signature, url: $url}}}' > "$output"
