#!/usr/bin/env bash
# Makes keeper-forge-macos-universal.dmg: the Keeper Forge window as a macOS app that runs on Intel
# and Apple silicon, with the command line tool beside it. Run on a Mac, from the repository root.
# Usage: make-macos-dmg.sh <version> <output dmg>
#
# Needs cargo-about (for the licence notices that must come with the program).
# The app is signed ad hoc (no certificate): macOS runs it after the person allows it once in
# System Settings. A signature by a developer certificate and notarization are not done here.
set -euo pipefail

version=$1
output=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")

rustup target add aarch64-apple-darwin x86_64-apple-darwin
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release --locked --target "$target" -p ck-forge
  cargo build --release --locked --target "$target" --manifest-path app-egui/Cargo.toml
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/Keeper Forge.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
tools/make-egui-licences.sh "$app/Contents/Resources/THIRD-PARTY-LICENSES.txt"
cp LICENSE "$app/Contents/Resources/LICENSE.txt"

lipo -create -output "$app/Contents/MacOS/keeper-forge" \
  app-egui/target/aarch64-apple-darwin/release/ck-forge-egui \
  app-egui/target/x86_64-apple-darwin/release/ck-forge-egui
lipo -create -output "$app/Contents/MacOS/ck-forge" \
  target/aarch64-apple-darwin/release/ck-forge \
  target/x86_64-apple-darwin/release/ck-forge

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Keeper Forge</string>
  <key>CFBundleDisplayName</key><string>Keeper Forge</string>
  <key>CFBundleIdentifier</key><string>org.contest-keeper.forge</string>
  <key>CFBundleExecutable</key><string>keeper-forge</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$app"
# The disk image shows the app next to a shortcut to Applications, to drag it there.
ln -s /Applications "$work/Applications"
rm -f "$output"
hdiutil create -volname "Keeper Forge" -srcfolder "$work" -format UDZO -ov "$output"
echo "Wrote $output"
