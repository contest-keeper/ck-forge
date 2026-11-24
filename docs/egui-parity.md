# egui window: parity with the Svelte window

The egui window (`app-egui/`) replaces `app/` and `ui/` only after every row below is ticked and
tested on Windows (R6.3 also needs the user's agreement). Each row is one of the 24 commands of
`app/src/main.rs`, in the order the screens are built. Every row has a `ck-admin-core` function and
a `ck-forge` command already, so a screen only calls the function.

| Screen | Window command | Engine function | CLI | egui |
|---|---|---|---|---|
| Hub sign-in | `hub_sign_in`, `hub_sign_out`, `hub_status` | `sign_in_url`, `sign_in_with_code`, `sign_out`, `vault` | `hub sign-in`, `hub sign-out`, `hub status` | [x] |
| Profile editor | `new_profile`, `open_profile`, `save_profile`, `check_profile` | `Profile` | `profile new`, `profile check` | [x] (fields: name, contest, team, site, computer name, USB label, server, sites) |
| Profile editor | `save_profile_in_forge_folder` | `Profile::save` into the app data folder | none needed | [ ] |
| Profile editor | `add_wallpaper` | `add_wallpaper` | `profile wallpaper` | [x] |
| Profile editor | `wallpaper_preview` | `wallpaper_data_url` | none needed | [ ] |
| Profile editor | `hub_list_profiles`, `hub_load_profile`, `hub_save_profile` | `list_profiles`, `load_hub_profile`, `save_hub_profile` | `hub profiles`, `hub pull`, `hub push` | [ ] |
| Image picker | `list_catalog`, `make_base_image`, `import_base_image` | `catalog`, `create_base_image`, `import_image` | `image catalog`, `image create`, `image import` | [ ] |
| Image picker | `list_hub_images`, `download_hub_image` | `list_hub_images`, `download_hub_image` | `hub images`, `hub download` | [x] |
| Image picker | `signature_required`, `enrolment_supported` | `requires_signed_settings`, `supports_enrolment` | `image support` | [x] |
| Write sticks | `list_disks`, `write_stick` | `ck_writer::list_disks`, the elevated `ck-forge stick write` with its JSON progress file | `stick list`, `stick write` | [ ] |
| Retire and re-key | `hub_list_sticks`, `hub_retire_stick` | `list_sticks`, `retire_stick_credential` | `hub sticks`, `hub retire` | [x] |

Also to carry over: the updater (`app/src/update.rs`) and the Windows elevation of the stick write.

## R6.3 checklist (prepared, not done)

R6.3 removes `app/`, `ui/` and their CI jobs. It starts only when the user agrees and every row above
is ticked. Steps, in order:

1. Parity: every row of the table is `[x]`, and each screen was used on a Windows PC against the dev Hub:
   sign in, edit and save a profile, download a base image, write and verify two sticks, retire a key.
2. The elevated write works from the egui window (UAC prompt, JSON progress file, cancel) and a stick
   booted from it passes the VM test.
3. The updater and the installer of the egui window exist (`app/src/update.rs`, `tauri.conf.json`
   `bundle` and `updater` settings, the release workflow), and one release was installed over the old one.
4. Remove, in one PR: `app/`, `ui/`, the `window` and `ui` jobs of `.github/workflows/pr.yml`, the
   `app` and `ui` steps of `release.yml`, `tools/` scripts that only the window uses, and the
   Svelte and Tauri words in `README.md`, `AGENTS.md`, `CLAUDE.md` and `docs/`.
5. In `.github`, drop `ui / check` from the required checks of this repository (a user task: branch
   protection is not editable from a session).
6. Check: `rg -i "tauri|svelte|pnpm" .` finds nothing that matters, `cargo machete` is clean, the
   release workflow builds the egui installer.
