//! The Keeper Forge window. The Svelte page in `ui/` shows the screens; these commands do the work
//! by calling the same `ck-admin-core` and `ck-writer` code as the `ck-forge` command line.

// Without this, Windows opens a console window next to the program.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod base_image;
mod hub;
mod profiles;
mod sticks;
mod update;
mod vault;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(hub::HubState::restored())
        .setup(|app| {
            update::check_on_start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            profiles::new_profile,
            profiles::open_profile,
            profiles::save_profile,
            profiles::check_profile,
            base_image::list_catalog,
            base_image::make_base_image,
            sticks::list_disks,
            sticks::write_stick,
            hub::hub_sign_in,
            hub::hub_sign_out,
            hub::hub_list_profiles,
            hub::hub_load_profile,
            hub::hub_save_profile,
            hub::hub_status,
        ])
        .run(tauri::generate_context!())
        .expect("the Keeper Forge window failed to start");
}
