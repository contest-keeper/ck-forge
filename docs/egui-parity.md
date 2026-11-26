# The window: screens and Windows tests

The window is `app-egui/`. It replaced the earlier web-based window; that code is in the git history.
Every screen calls a `ck-admin-core` function that also has a `ck-forge` command.

| Screen | Engine function | CLI | Window |
| --- | --- | --- | --- |
| Hub sign-in | `sign_in_url`, `sign_in_with_code`, `sign_out`, `vault` | `hub sign-in`, `hub sign-out`, `hub status` | [x] |
| Profile editor | `Profile` | `profile new`, `profile check` | [x] (fields: name, contest, team, site, computer name, USB label, server, sites, home page, welcome text, bookmarks, Bluetooth, contest end, Wi-Fi name, recording: built, Windows test pending; the Wi-Fi password is typed when sticks are written, never saved in the profile) |
| Profile editor | `Profile::save` into the app data folder | none needed | [x] built, Windows test pending |
| Profile editor | `add_wallpaper` | `profile wallpaper` | [x] |
| Profile editor | `wallpaper_data_url` | none needed | [x] built, Windows test pending |
| Profile editor | `list_profiles`, `load_hub_profile`, `save_hub_profile` | `hub profiles`, `hub pull`, `hub push` | [x] built, Windows test pending |
| Image picker | `catalog`, `create_base_image`, `import_image` | `image catalog`, `image create`, `image import` | [x] built, Windows test pending |
| Image picker | `list_hub_images`, `download_hub_image` | `hub images`, `hub download` | [x] |
| Image picker | `requires_signed_settings`, `supports_enrolment` | `image support` | [x] |
| Write sticks | `ck_writer::list_disks`, the elevated `ck-forge stick write` with its JSON progress file | `stick list`, `stick write` | [x] built, Windows test pending (UAC prompt, progress and cancel need a real run) |
| Retire and re-key | `list_sticks`, `retire_stick_credential` | `hub sticks`, `hub retire` | [x] |

## Still to be tested on real machines

"Built, Windows test pending" above means no one has run that screen on a Windows PC yet.

1. On Windows against the dev Hub: sign in, edit and save a profile (including Wi-Fi and recording),
   download a base image, write and verify two sticks, retire a key.
2. The elevated write: permission prompt, progress and cancel; boot a written stick.
3. Installer and updates on Windows: install over an older Keeper Forge (the old one is gone, profiles
   and sign-in are kept), then let an older build find, verify and install a newer one.
4. macOS: open the disk image, run the app, let an older build offer and open a newer image.

## Known gaps

- Paths are typed: there are no file pickers.
- No light or dark setting, and the base image path is not remembered.
- Updates are looked for only when the window opens.
- Hub sign-in has no Cancel and does not re-check a saved session.
