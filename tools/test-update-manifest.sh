#!/usr/bin/env bash
# Checks make-update-manifest.sh and add-macos-update.sh. Run from the repository root; needs jq.
set -euo pipefail

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
feed="$work/update.json"
echo "d2luZG93cw==" > "$work/exe.sig"
echo "bWFj" > "$work/dmg.sig"
echo "0.1.0" > "$work/min"
: > "$work/empty.sig"

fail() { echo "test-update-manifest: $1" >&2; exit 1; }

tools/make-update-manifest.sh 1.2.3 https://example.org/a.exe "$work/exe.sig" "$work/min" "$feed"
tools/add-macos-update.sh 1.2.3 https://example.org/a.dmg "$work/dmg.sig" "$feed"
jq -e '.version == "1.2.3" and .min_version == "0.1.0"
  and .platforms["windows-x86_64"] == {signature: "d2luZG93cw==", url: "https://example.org/a.exe"}
  and .platforms["darwin-universal"] == {signature: "bWFj", url: "https://example.org/a.dmg"}' \
  "$feed" > /dev/null || fail "the feed does not hold both systems"

cp "$feed" "$work/before"
if tools/add-macos-update.sh 9.9.9 https://example.org/a.dmg "$work/dmg.sig" "$feed"; then
  fail "another version was accepted"
fi
if tools/add-macos-update.sh 1.2.3 https://example.org/a.dmg "$work/empty.sig" "$feed"; then
  fail "an empty macOS signature was accepted"
fi
cmp -s "$feed" "$work/before" || fail "a refused call changed the feed"
if tools/make-update-manifest.sh 1.2.3 https://example.org/a.exe "$work/none.sig" "$work/min" "$work/other.json"; then
  fail "a missing Windows signature was accepted"
fi
echo "update manifest: ok"
