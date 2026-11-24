# Next steps for ck-forge (2026-11-25)

State: the Forge engine is GUI-free, the egui window (`app-egui`) exists with the Hub sign-in, profile editor, image picker, sticks page, bookmarks/home page/Bluetooth/USB-storage options, and the writer runs on Windows and macOS. What is left needs hardware, Windows, or a decision.

## 1. Tests with a real stick

- **macOS write test**: follow the checklist in [testing-the-writer.md](testing-the-writer.md) ("Manual test with a real stick on macOS"). Run `ck-forge stick write` with `sudo`. First run the listing only and check that the right disk is the only one offered.
- **Windows write test** (Kingston stick): same document, Windows section. This also covers the writer check for #27.
- **G4 acceptance for #27**: log in as organizer on the dev Hub, open the manifest and source-offer links of the current image (`/download/image/{id}` and `.../source-offer`), start the image download. Then tick #27.

## 2. Windows parity tests for the egui window (R6.2/R6.3)

The parity table is [egui-parity.md](egui-parity.md). Open rows: write sticks (Windows UAC elevation), catalog build, archive import, wallpaper preview, Wi-Fi (holds a secret), recording, Hub profiles, save in the Forge folder, updater and installer (need the release pipeline).
Run each row on Windows, mark it, and fix gaps in small PRs.

## 3. R6.3: remove the old Tauri app

Only after every parity row passes on Windows (this includes Wi-Fi, recording and the updater, which must each be implemented and tested there) **and** the owner agrees: remove `app/`, `ui/` and their CI jobs, and update the README and installer. The owner said "not yet" on 2026-11-25.

## 4. Developer image creator and enrolment (#24, #32 to #34)

Code is merged. Missing: the packaged-VM tests with `ck-vmtest` on Windows with Hyper-V or QEMU (enrolment against the dev Hub, boot of the created image must PASS, patch step on the Ubuntu base image).

## 5. Releases

Every merge to `develop` rebuilds the rolling pre-release "Preview build 0.1.0-preview.N" (tag `dev`). A numbered release is cut separately by the owner.
Not yet done: code signing (Windows SmartScreen warns) and macOS signing/notarisation.
