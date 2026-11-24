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
| Retire and re-key | `hub_list_sticks`, `hub_retire_stick` | `list_sticks`, `retire_stick_credential` | `hub sticks`, `hub retire` | [ ] |

Also to carry over: the updater (`app/src/update.rs`) and the Windows elevation of the stick write.
