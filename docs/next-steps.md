# Next steps for ck-forge (2026-11-25)

State: the Forge engine is GUI-free, the egui window (`app-egui`) exists with the Hub sign-in, profile editor, image picker, sticks page, bookmarks/home page/Bluetooth/USB-storage options, and the writer runs on Windows and macOS. What is left needs hardware, Windows, or a decision.

## 1. Tests with a real stick

- **macOS write test**: follow the checklist in [testing-the-writer.md](testing-the-writer.md) ("Manual test with a real stick on macOS"). Run `ck-forge stick write` with `sudo`. First run the listing only and check that the right disk is the only one offered.
- **Windows write test** (Kingston stick): same document, Windows section. This also covers the writer check for #27.
- **G4 acceptance for #27**: log in as organizer on the dev Hub, open the manifest and source-offer links of the current image (`/download/image/{id}` and `.../source-offer`), start the image download. Then tick #27.

## 2. Windows and macOS tests of the window

The list is in [egui-parity.md](egui-parity.md). Run it on real machines and fix gaps in small PRs.

## 3. Developer image creator and enrolment (#24, #32 to #34)

Code is merged. Missing: the packaged-VM tests with `ck-vmtest` on Windows with Hyper-V or QEMU (enrolment against the dev Hub, boot of the created image must PASS, patch step on the Ubuntu base image).

## 4. Releases

Every merge to `develop` rebuilds the rolling pre-release "Preview build 0.1.0-preview.N" (tag `dev`). A numbered release is cut separately by the owner.
Before the first one: deploy the production Hub (it holds the update key), give this repository's `production` environment its Azure sign-in, and approve the two font licences in `app-egui/about.toml`.
Not yet done: code signing (Windows SmartScreen warns) and macOS signing/notarisation.
