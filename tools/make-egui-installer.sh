#!/usr/bin/env bash
# Builds the Windows installer from the release builds of the window and of ck-forge, with makensis.
# Usage: make-egui-installer.sh <version> <third-party licences file> <output exe>
# Run from the repository root in Git Bash. Build both with --target-dir (target and app-egui/target):
# with CARGO_TARGET_DIR set, a plain build goes elsewhere and old files would be packaged.
set -euo pipefail

# Absolute paths: makensis reads relative ones from the script folder, not from here.
makensis -V2 "-DVERSION=$1" \
  "-DAPP=$(cygpath -aw app-egui/target/release/ck-forge-egui.exe)" \
  "-DHELPER=$(cygpath -aw target/release/ck-forge.exe)" \
  "-DLICENCE=$(cygpath -aw LICENSE)" \
  "-DTHIRDPARTY=$(cygpath -aw "$2")" \
  "-DOUTFILE=$(cygpath -aw "$3")" \
  app-egui/installer/keeper-forge.nsi
